//! Drawing, against `Surface`: the watch through `&Api`, a host framebuffer in tests.
//!
//! Layout on the round 466 px panel, black like the stock faces: a heading-up map fills it, the route in white
//! around the car over the streets nearby, which fade out away from the route (as Vela's ESP32 display does them).
//! Over the map, in white with a black outline: the time now, the time left big with the distance and arrival, and
//! the road at the top; the next turn big at the bottom (arrow, distance, speed limit) over an arc that fills as
//! the drive goes. Text is Noto Sans (`font`, `fonts`), with the loader's pixel font for a road name in a script
//! those fonts lack.

use crate::font::{Font, poly};
use crate::fonts::{L, S, XL};
use crate::proto::{Model, man};
use crate::sign::{Sign, head};
use cmf_wp2_ui::draw::{arc, disc, line, ring, stroke};
use cmf_wp2_ui::motion::{Q14, cos, isqrt, sin};
use cmf_wp2_ui::text::fitted;
use cmf_wp2_ui::{C, CHAR, Rgb, SIZE, Surface, geom};
use core::fmt::Write;

const BLACK: Rgb = Rgb(0, 0, 0);
const WHITE: Rgb = Rgb(255, 255, 255);
const SHADE: Rgb = Rgb(178, 180, 186);
const GREY: Rgb = Rgb(140, 140, 146);
const TRACK: Rgb = Rgb(58, 58, 62);
const PASSED: Rgb = Rgb(62, 62, 68);
const ACCENT: Rgb = Rgb(255, 160, 0);
const RED: Rgb = Rgb(228, 36, 36);

/// Streets at full strength, beside the route; they fade to black away from it.
const ROAD_MINOR: Rgb = Rgb(0x4A, 0x50, 0x57);
const ROAD_MAJOR: Rgb = Rgb(0x6E, 0x75, 0x7E);
/// The car, heading up.
pub const CAR: (i32, i32) = (C, 278);
/// The progress arc along the bottom edge: radii, and half its span either side of 6 o'clock.
const ARC_R: (i32, i32) = (214, 222);
const ARC_HALF: u32 = 8192; // 45°
/// Map scales the crown steps through, in metres per pixel.
pub const ZOOMS: [i32; 6] = [1, 2, 3, 5, 8, 13];
pub const ZOOM_DEFAULT: usize = 2;

/// How far each part of the screen is from the route ahead: cells of `CELL` px, each a distance in half cells.
/// Rebuilt each frame; 3.5 KB, so it lives in a static on the watch.
pub struct Fade {
    d: [u8; GRID * GRID],
}
const CELL: i32 = 8;
const GRID: usize = (SIZE as usize).div_ceil(CELL as usize);
/// Streets are at full strength up to this far from the route, and gone from the second.
const FADE_PX: (i32, i32) = (24, 160);
const SHADES: usize = 16;

impl Fade {
    pub const fn new() -> Fade {
        Fade { d: [255; GRID * GRID] }
    }

    fn seed(&mut self, x: i32, y: i32) {
        if (0..SIZE).contains(&x) && (0..SIZE).contains(&y) {
            self.d[(y / CELL) as usize * GRID + (x / CELL) as usize] = 0;
        }
    }

    /// The shade (0 none .. SHADES-1 full) of a street at (x, y).
    fn shade(&self, x: i32, y: i32) -> usize {
        if !(0..SIZE).contains(&x) || !(0..SIZE).contains(&y) {
            return 0;
        }
        let px = self.d[(y / CELL) as usize * GRID + (x / CELL) as usize] as i32 * (CELL / 2);
        let (near, far) = FADE_PX;
        if px <= near {
            SHADES - 1
        } else if px >= far {
            0
        } else {
            ((far - px) * (SHADES as i32 - 1) / (far - near)) as usize
        }
    }

    /// Seeds the car and the route ahead, then spreads: a two-pass chamfer distance, 2 a cell straight and 3
    /// diagonally.
    fn build(&mut self, m: &Model, at: Pose, mpp: i32) {
        self.d.fill(255);
        self.seed(CAR.0, CAR.1);
        let st = &m.state;
        if m.route_len >= 2 {
            let from = (st.progress as usize).clamp(1, m.route_len) - 1;
            let mut prev: Option<(i32, i32)> = None;
            for i in from..m.route_len {
                let p = project(m.route[i], at.pos(), at.course, mpp);
                if let Some(q) = prev
                    && let Some((a, b)) = clip(q, p, 0, SIZE - 1)
                {
                    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                    let steps = dx.abs().max(dy.abs()) / (CELL / 2) + 1;
                    for k in 0..=steps {
                        self.seed(a.0 + dx * k / steps, a.1 + dy * k / steps);
                    }
                }
                prev = Some(p);
            }
        }
        let g = GRID;
        let d = &mut self.d;
        let relax = |v: &mut u8, n: u8, cost: u8| *v = (*v).min(n.saturating_add(cost));
        for y in 0..g {
            for x in 0..g {
                let mut v = d[y * g + x];
                if x > 0 {
                    relax(&mut v, d[y * g + x - 1], 2);
                }
                if y > 0 {
                    relax(&mut v, d[(y - 1) * g + x], 2);
                    if x > 0 {
                        relax(&mut v, d[(y - 1) * g + x - 1], 3);
                    }
                    if x + 1 < g {
                        relax(&mut v, d[(y - 1) * g + x + 1], 3);
                    }
                }
                d[y * g + x] = v;
            }
        }
        for y in (0..g).rev() {
            for x in (0..g).rev() {
                let mut v = d[y * g + x];
                if x + 1 < g {
                    relax(&mut v, d[y * g + x + 1], 2);
                }
                if y + 1 < g {
                    relax(&mut v, d[(y + 1) * g + x], 2);
                    if x > 0 {
                        relax(&mut v, d[(y + 1) * g + x - 1], 3);
                    }
                    if x + 1 < g {
                        relax(&mut v, d[(y + 1) * g + x + 1], 3);
                    }
                }
                d[y * g + x] = v;
            }
        }
    }
}

impl Default for Fade {
    fn default() -> Self {
        Fade::new()
    }
}

/// The streets around, under the route: minor 3 px, major 5 px, each pixel run shaded by how far it is from the
/// route.
fn roads(s: &mut impl Surface, m: &Model, fade: &Fade, at: Pose, mpp: i32) {
    let r = m.roads();
    let ramp = |c: Rgb| {
        let mut out = [BLACK; SHADES];
        for (i, v) in out.iter_mut().enumerate() {
            *v = BLACK.mix(c, i as i32 * cmf_wp2_ui::ONE / (SHADES as i32 - 1));
        }
        out
    };
    let (minor, major) = (ramp(ROAD_MINOR), ramp(ROAD_MAJOR));
    for l in &r.lines[..r.n_lines] {
        let (colours, w) = if l.major { (&major, 5) } else { (&minor, 3) };
        let mut prev: Option<(i32, i32)> = None;
        for p in r.line(l) {
            let p = project(p, at.pos(), at.course, mpp);
            if let Some(q) = prev
                && let Some((a, b)) = clip(q, p, -w, SIZE + w)
            {
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let len = isqrt(256 * (dx * dx + dy * dy));
                if len > 0 {
                    let (ux, uy) = (dx * Q14 * 16 / len, dy * Q14 * 16 / len);
                    let q2 = (Q14 as i64) * (Q14 as i64);
                    // `along` is the distance from `a` in Q14 px
                    let shade = |along: i32| {
                        let x = a.0 + (ux as i64 * along as i64 / q2) as i32;
                        let y = a.1 + (uy as i64 * along as i64 / q2) as i32;
                        colours[fade.shade(x, y)]
                    };
                    // only the stretches that are not faded out: a black stroke on black is wasted rects
                    let total = (len + 8) / 16;
                    let lit = |k: i32| {
                        let k = k.min(total);
                        fade.shade(
                            a.0 + ((ux as i64 * k as i64) >> 14) as i32,
                            a.1 + ((uy as i64 * k as i64) >> 14) as i32,
                        ) > 0
                    };
                    let step = CELL / 2;
                    let mut k = 0;
                    while k <= total {
                        if !lit(k) {
                            k += step;
                            continue;
                        }
                        let start = (k - step).max(0);
                        while k <= total && lit(k) {
                            k += step;
                        }
                        stroke(s, a.0, a.1, ux, uy, start, k.min(total), w, 0, shade, BLACK);
                    }
                }
            }
            prev = Some(p);
        }
    }
}

/// Text over the map: a black outline, then the text. Returns the pen position after it.
fn ink(f: &Font, s: &mut impl Surface, x: i32, y: i32, t: &str, c: Rgb) -> i32 {
    f.halo(s, x, y, t, if f.cap > 20 { 3 } else { 2 });
    f.draw(s, x, y, t, c.rgb565(), 0)
}

/// [ink], centred on `cx` and cut with an ellipsis to `max` px.
fn ink_centre(f: &Font, s: &mut impl Surface, cx: i32, y: i32, t: &str, max: i32, c: Rgb) {
    let (t, cut) = f.fit(t, max);
    let w = f.width(t) + if cut { f.width("…") } else { 0 };
    let end = ink(f, s, cx - w / 2, y, t, c);
    if cut {
        ink(f, s, end, y, "…", c);
    }
}

/// The car gliding from where it is drawn to the phone's latest position, over the time the phone takes between
/// updates (about a second), so the map moves and turns smoothly instead of jumping once a second. It runs a
/// little behind the phone for that. A jump of more than `SNAP_M` (a new route, a reroute) is not glided.
pub struct Glide {
    from: Pose,
    to: Pose,
    t0: u32,
    dur: u32,
    last: Option<u32>,
}

const SNAP_M: i32 = 300;

impl Glide {
    pub const fn new() -> Glide {
        let p = Pose { x: 0, y: 0, course: 0 };
        Glide { from: p, to: p, t0: 0, dur: 1, last: None }
    }

    /// A new position from the phone at `now` (ms).
    pub fn target(&mut self, to: Pose, now: u32) {
        if self.last.is_some() && to == self.to {
            return;
        }
        let gap = self.last.map_or(0, |t| now.wrapping_sub(t)).clamp(300, 1500);
        let jump = (to.x - self.to.x).abs().max((to.y - self.to.y).abs()) > SNAP_M;
        self.from = if self.last.is_none() || jump { to } else { self.at(now) };
        (self.to, self.t0, self.dur, self.last) = (to, now, gap, Some(now));
    }

    /// Where to draw the car at `now`.
    pub fn at(&self, now: u32) -> Pose {
        let k = (now.wrapping_sub(self.t0).min(self.dur) as i64 * cmf_wp2_ui::ONE as i64 / self.dur as i64) as i32;
        self.from.toward(self.to, k)
    }

    /// Still on its way.
    pub fn moving(&self, now: u32) -> bool {
        now.wrapping_sub(self.t0) < self.dur && self.from != self.to
    }
}

impl Default for Glide {
    fn default() -> Self {
        Glide::new()
    }
}

/// What the screen is about, besides the model itself.
pub struct Ctx {
    /// The phone has been heard from recently.
    pub live: bool,
    /// Local time of day (hours, minutes), when the watch has it.
    pub clock: Option<(u8, u8)>,
    pub zoom: usize,
    /// Where the car is drawn, when it glides between the phone's updates; else the last state's.
    pub pose: Option<Pose>,
    /// The Home / Work button just tapped, while the phone starts that drive.
    pub pending: Option<u8>,
}

/// The buttons on the screen without a drive: what each asks for, its label, and its top.
pub const BUTTONS: [(u8, &str, i32); 2] = [(crate::proto::P_HOME, "Home", 186), (crate::proto::P_WORK, "Work", 276)];
const BUTTON_W: i32 = 300;
const BUTTON_H: i32 = 76;
const PILL: Rgb = Rgb(40, 40, 46);

/// The button under a tap at `y`, if any. Only `y` is reliable on this panel, and the buttons are stacked.
pub fn button_at(y: i32) -> Option<u8> {
    BUTTONS.iter().find(|b| y >= b.2 - 6 && y < b.2 + BUTTON_H + 6).map(|b| b.0)
}

/// A house or a briefcase, `fg` on the button's `bg`.
fn place_icon(s: &mut impl Surface, kind: u8, cx: i32, cy: i32, fg: Rgb, bg: Rgb) {
    let (k, b) = (fg.rgb565(), bg.rgb565());
    if kind == crate::proto::P_HOME {
        let house = [
            (cx, cy - 15),
            (cx + 15, cy - 1),
            (cx + 11, cy - 1),
            (cx + 11, cy + 13),
            (cx - 11, cy + 13),
            (cx - 11, cy - 1),
            (cx - 15, cy - 1),
        ];
        poly(s, &house, b, |_| k);
        s.rect(cx - 3, cy + 4, 7, 9, b); // the door
    } else {
        cmf_wp2_ui::draw::rrect(s, cx - 7, cy - 14, 14, 10, 3, fg, bg); // the handle
        cmf_wp2_ui::draw::rrect(s, cx - 3, cy - 10, 6, 7, 1, bg, fg);
        cmf_wp2_ui::draw::rrect(s, cx - 15, cy - 7, 30, 21, 4, fg, bg);
        s.rect(cx - 15, cy + 1, 30, 2, b); // the lid's seam
    }
}

/// The screen without a drive: Home and Work, each a tap away from a drive there.
fn launcher(s: &mut impl Surface, m: &Model, pending: Option<u8>) {
    s.rect(0, 0, SIZE, SIZE, 0);
    L.draw(s, C - L.width("Vela Nav") / 2, 136, "Vela Nav", WHITE.rgb565(), 0);
    for (kind, label, top) in BUTTONS {
        let set = m.places & kind != 0;
        let on = pending == Some(kind);
        let (fill, fg) = if on {
            (WHITE, BLACK)
        } else if set {
            (PILL, WHITE)
        } else {
            (Rgb(24, 24, 28), Rgb(90, 90, 96))
        };
        let x = C - BUTTON_W / 2;
        cmf_wp2_ui::draw::rrect(s, x, top, BUTTON_W, BUTTON_H, BUTTON_H / 2, fill, BLACK);
        let cy = top + BUTTON_H / 2;
        place_icon(s, kind, x + 58, cy, fg, fill);
        let text = if on { "Starting…" } else { label };
        L.draw(s, x + 96, cy + L.cap / 2, text, fg.rgb565(), fill.rgb565());
    }
    let note = if m.start == crate::proto::START_FAILED {
        "Couldn't plan that drive"
    } else if m.places == 0 {
        "Set Home and Work in Vela"
    } else {
        "or start one in Vela"
    };
    small(s, 396, note, if m.start == crate::proto::START_FAILED { ACCENT } else { GREY });
}

/// The car's position (metres, the route's frame) and heading (1/65536 turns clockwise from north).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pose {
    pub x: i32,
    pub y: i32,
    pub course: u16,
}

impl Pose {
    pub fn of(st: &crate::proto::State) -> Pose {
        Pose { x: st.x, y: st.y, course: st.course }
    }

    fn pos(self) -> [i32; 2] {
        [self.x, self.y]
    }

    /// `k` (0..=ONE) of the way from `self` to `to`, turning the short way round.
    pub fn toward(self, to: Pose, k: i32) -> Pose {
        let k = k.clamp(0, cmf_wp2_ui::ONE) as i64;
        let lerp = |a: i32, b: i32| a + ((b as i64 - a as i64) * k / cmf_wp2_ui::ONE as i64) as i32;
        let turn = to.course.wrapping_sub(self.course) as i16 as i64;
        let course = self.course.wrapping_add((turn * k / cmf_wp2_ui::ONE as i64) as i16 as u16);
        Pose { x: lerp(self.x, to.x), y: lerp(self.y, to.y), course }
    }

    /// The largest change between the two on screen at `mpp` metres a pixel, in pixels (turning counts as the
    /// move of a point at the screen's edge).
    pub fn moved_px(self, to: Pose, mpp: i32) -> i32 {
        let d = ((to.x - self.x).abs().max((to.y - self.y).abs()) / mpp.max(1)) as i64;
        let turn = (to.course.wrapping_sub(self.course) as i16 as i64).abs();
        d.max(turn * C as i64 * 6 / 65536) as i32
    }
}

/// A small text buffer for `write!` without a heap.
pub struct Buf {
    b: [u8; 32],
    n: usize,
}

impl Buf {
    pub fn new() -> Buf {
        Buf { b: [0; 32], n: 0 }
    }
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.b[..self.n]).unwrap_or("")
    }
}

impl Default for Buf {
    fn default() -> Self {
        Buf::new()
    }
}

impl Write for Buf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let k = s.len().min(self.b.len() - self.n);
        self.b[self.n..self.n + k].copy_from_slice(&s.as_bytes()[..k]);
        self.n += k;
        Ok(())
    }
}

/// "350 m", "1.2 km", "14 km"; "500 ft", "0.3 mi", "12 mi".
pub fn distance(m: u32, imperial: bool) -> Buf {
    let mut b = Buf::new();
    let _ = if imperial {
        if m < 161 {
            write!(b, "{} ft", (m * 3281 / 1000 + 25) / 50 * 50)
        } else {
            let tenths = (m * 100 + 8047) / 16093; // m / 160.93, rounded
            if tenths < 100 {
                write!(b, "{}.{} mi", tenths / 10, tenths % 10)
            } else {
                write!(b, "{} mi", (tenths + 5) / 10)
            }
        }
    } else if m < 1000 {
        write!(b, "{} m", if m < 100 { m / 5 * 5 } else { m / 10 * 10 })
    } else if m < 10_000 {
        let h = (m + 50) / 100;
        write!(b, "{}.{} km", h / 10, h % 10)
    } else {
        write!(b, "{} km", (m + 500) / 1000)
    };
    b
}

/// "14 min", "1 h 05".
pub fn duration(s: u32) -> Buf {
    let mut b = Buf::new();
    let min = (s + 30) / 60;
    let _ = if min < 60 { write!(b, "{} min", min) } else { write!(b, "{} h {:02}", min / 60, min % 60) };
    b
}

/// The time of day `s` seconds after `clock`, "17:05" or "5:05 pm".
pub fn arrival(clock: (u8, u8), s: u32, h24: bool) -> Buf {
    let total = (clock.0 as u32 * 60 + clock.1 as u32 + (s + 30) / 60) % (24 * 60);
    let (h, m) = (total / 60, total % 60);
    let mut b = Buf::new();
    let _ = if h24 {
        write!(b, "{:02}:{:02}", h, m)
    } else {
        write!(b, "{}:{:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, m, if h < 12 { "am" } else { "pm" })
    };
    b
}

/// A route point in metres → screen pixels, heading up around `CAR`. Far points are clamped (they only feed the
/// clipper).
pub fn project(p: [i32; 2], car: [i32; 2], course: u16, mpp: i32) -> (i32, i32) {
    let (dx, dy) = (p[0] as i64 - car[0] as i64, p[1] as i64 - car[1] as i64);
    let (s, c) = (sin(course) as i64, cos(course) as i64);
    let right = (dx * c - dy * s) / (Q14 as i64 * mpp as i64);
    let ahead = (dx * s + dy * c) / (Q14 as i64 * mpp as i64);
    const LIM: i64 = 1 << 20;
    ((CAR.0 as i64 + right.clamp(-LIM, LIM)) as i32, (CAR.1 as i64 - ahead.clamp(-LIM, LIM)) as i32)
}

/// Cohen–Sutherland: the part of a segment inside `[lo, hi]²`, or `None`.
pub fn clip(mut a: (i32, i32), mut b: (i32, i32), lo: i32, hi: i32) -> Option<((i32, i32), (i32, i32))> {
    let code =
        |p: (i32, i32)| (p.0 < lo) as u8 | ((p.0 > hi) as u8) << 1 | ((p.1 < lo) as u8) << 2 | ((p.1 > hi) as u8) << 3;
    let (mut ca, mut cb) = (code(a), code(b));
    for _ in 0..8 {
        if ca | cb == 0 {
            return Some((a, b));
        }
        if ca & cb != 0 {
            return None;
        }
        let out = if ca != 0 { ca } else { cb };
        let (x0, y0, x1, y1) = (a.0 as i64, a.1 as i64, b.0 as i64, b.1 as i64);
        let p = if out & 8 != 0 {
            (x0 + (x1 - x0) * (hi as i64 - y0) / (y1 - y0), hi as i64)
        } else if out & 4 != 0 {
            (x0 + (x1 - x0) * (lo as i64 - y0) / (y1 - y0), lo as i64)
        } else if out & 2 != 0 {
            (hi as i64, y0 + (y1 - y0) * (hi as i64 - x0) / (x1 - x0))
        } else {
            (lo as i64, y0 + (y1 - y0) * (lo as i64 - x0) / (x1 - x0))
        };
        let p = (p.0 as i32, p.1 as i32);
        if out == ca {
            (a, ca) = (p, code(p));
        } else {
            (b, cb) = (p, code(p));
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn polyline(s: &mut impl Surface, m: &Model, from: usize, to: usize, w: i32, c: Rgb, at: Pose, mpp: i32) {
    let (car, course) = (at.pos(), at.course);
    let mut prev: Option<(i32, i32)> = None;
    for i in from..to {
        let p = project(m.route[i], car, course, mpp);
        if let Some(q) = prev
            && let Some((a, b)) = clip(q, p, -w, SIZE + w)
        {
            line(s, a.0, a.1, b.0, b.1, w, c, BLACK);
        }
        prev = Some(p);
    }
    // Round every joint in a second pass: a segment's soft end would otherwise darken the one before it, and
    // the bends would have notches. The disc's soft edge is painted solid (bg = c), so a radius one short of
    // the line's half width fills it to the line's own soft edge without a bump.
    for i in from + 1..to.saturating_sub(1) {
        let p = project(m.route[i], car, course, mpp);
        if geom::inside(p.0, p.1) {
            disc(s, p.0, p.1, w / 2 - 1, c, c);
        }
    }
}

/// Direction `a` (turns, clockwise from up) as a screen vector of length `r`.
fn dir(a: u16, r: i32) -> (i32, i32) {
    ((r * sin(a)) >> 14, -(r * cos(a)) >> 14)
}

/// A maneuver icon in a box of radius `r` around (cx, cy): thick strokes with round ends, like a road sign.
pub fn icon(s: &mut impl Surface, code: u8, cx: i32, cy: i32, r: i32, c: Rgb) {
    let w = (r * 2 / 7).max(4);
    let mut g = Sign::new(w);
    g.halo = 3;
    let hl = r * 3 / 5; // arrowhead length
    // a stem up from the bottom that bends towards `a` at `elbow` below the centre
    // (the stem leans away from the turn by `lean` so the whole sign sits centred)
    let turn = |g: &mut Sign, a: u16, elbow: i32, len: i32, lean: i32, hl: i32| {
        let sh = dir(a, len - hl / 2);
        let x = cx - lean;
        let e = (x, cy + elbow);
        let tip = (x + dir(a, len).0, e.1 + dir(a, len).1);
        g.seg((x, cy + r), e).seg(e, (x + sh.0, e.1 + sh.1)).tri(head(tip, a, hl));
    };
    let gentle = |g: &mut Sign, a: u16, elbow: i32| turn(g, a, elbow, r, dir(a, r * 2 / 5).0 / 2, hl);
    let sharp = |g: &mut Sign, a: u16| turn(g, a, -r * 3 / 4, r * 13 / 10, dir(a, r).0 * 7 / 10, r / 2);
    // a right angle with a round bend: up, then `sign` (1 right, -1 left)
    let square = |g: &mut Sign, sign: i32| {
        let rho = r * 3 / 5;
        let (x, y) = (cx - sign * r / 2, cy - r / 3);
        let tip = (cx + sign * r, y);
        let start = if sign > 0 { 0x8000 } else { 0 };
        g.seg((x, cy + r), (x, y + rho))
            .arc((x + sign * rho, y + rho), rho, start, sign * 0x4000, 6)
            .seg((x + sign * rho, y), (tip.0 - sign * hl / 2, y))
            .tri(head(tip, if sign > 0 { 0x4000 } else { 0xC000 }, hl));
    };
    match code {
        man::CONTINUE => {
            g.seg((cx, cy + r), (cx, cy - r + hl / 2)).tri(head((cx, cy - r), 0, hl));
        }
        man::SLIGHT_LEFT => gentle(&mut g, 0xE000, 0),
        man::SLIGHT_RIGHT => gentle(&mut g, 0x2000, 0),
        man::LEFT => square(&mut g, -1),
        man::RIGHT => square(&mut g, 1),
        man::SHARP_LEFT => sharp(&mut g, 0xA000),
        man::SHARP_RIGHT => sharp(&mut g, 0x6000),
        man::KEEP_LEFT => gentle(&mut g, 0xF000, r / 4),
        man::KEEP_RIGHT => gentle(&mut g, 0x1000, r / 4),
        man::UTURN => {
            let (rx, top) = (r / 2, cy - r / 4);
            g.seg((cx + rx, cy + r), (cx + rx, top))
                .arc((cx, top), rx, 0, -0x8000, 10)
                .seg((cx - rx, top), (cx - rx, cy + r - hl / 2))
                .tri(head((cx - rx, cy + r), 0x8000, hl));
        }
        man::ROUNDABOUT => {
            let rr = r * 2 / 5;
            g.ring((cx, cy), rr)
                .seg((cx, cy + r), (cx, cy + rr))
                .seg((cx, cy - rr), (cx, cy - r - 4 + r / 4))
                .tri(head((cx, cy - r - 4), 0, r / 2));
        }
        man::ARRIVE => {
            let pin = cy - r / 4;
            let k = RED.rgb565();
            poly(s, &[(cx - r / 2 + 1, pin + r / 6), (cx + r / 2 - 1, pin + r / 6), (cx, cy + r)], 0, |_| k);
            disc(s, cx, pin, r / 2, RED, BLACK);
            disc(s, cx, pin, r / 5, WHITE, RED);
        }
        man::OFF_ROUTE => {
            disc(s, cx, cy, r + 3, BLACK, BLACK);
            ring(s, cx, cy, r - w, r, ACCENT, BLACK);
            let k = ACCENT.rgb565();
            s.rect(cx - w / 2, cy - r / 2, w, r * 2 / 3, k);
            s.rect(cx - w / 2, cy + r / 3, w, w, k);
        }
        _ => {}
    }
    let m = w + 6 + g.halo;
    g.draw(s, cx - r - m, cy - r - m, cx + r + m, cy + r + m, c.rgb565(), 0);
}

/// The car: a chevron pointing up, lit on its left face, with a black rim so it reads over the white route.
fn car(s: &mut impl Surface, fix: bool) {
    let (x, y) = CAR;
    let shape = |k: i32| [(x, y - 24 - k), (x + 18 + k, y + 18 + k), (x, y + 8 + k / 2), (x - 18 - k, y + 18 + k)];
    poly(s, &shape(5), 0, |_| 0);
    let (lit, dark) = if fix { (WHITE.rgb565(), SHADE.rgb565()) } else { (GREY.rgb565(), TRACK.rgb565()) };
    poly(s, &shape(0), 0, |px| if px < x { lit } else { dark });
}

fn limit_badge(s: &mut impl Surface, limit: u8, x: i32, y: i32) {
    disc(s, x, y, 31, RED, BLACK);
    disc(s, x, y, 24, WHITE, RED);
    let mut b = Buf::new();
    let _ = write!(b, "{}", limit);
    let t = b.as_str();
    L.draw(s, x - L.width(t) / 2, y + L.cap / 2, t, 0, WHITE.rgb565());
}

/// How wide a text line with its baseline at `y` may be, from what the circle leaves there.
fn room(f: &crate::font::Font, y: i32, margin: i32) -> i32 {
    let (x0, x1) = geom::chord(y - f.cap, y + f.descent / 2, margin);
    x1 - x0
}

/// A line of small text centred at baseline `y`; a road name in a script the font lacks falls back to the
/// loader's font (ASCII and Cyrillic).
fn small(s: &mut impl Surface, y: i32, t: &str, c: Rgb) {
    if S.covers(t) {
        ink_centre(&S, s, C, y, t, room(&S, y, 14), c);
    } else {
        let (_, cells) = geom::text_box(y - S.cap, 14);
        let n = cmf_wp2_ui::text::chars(t).min(cells);
        fitted(s, C - n as i32 * CHAR / 2, y - S.cap, t, n, c.rgb565());
    }
}

fn message(s: &mut impl Surface, title: &str, sub: &str) {
    s.rect(0, 0, SIZE, SIZE, 0);
    L.draw(s, C - L.width(title) / 2, C - 4, title, WHITE.rgb565(), 0);
    small(s, C + 38, sub, GREY);
}

/// The top: the time now, small; the time left, big, with the distance left and the arrival; the road.
fn header(s: &mut impl Surface, m: &Model, clock: Option<(u8, u8)>) {
    let st = &m.state;
    let rem = st.remaining_s as u32;
    if let Some(c) = clock {
        let now = arrival(c, 0, st.clock24());
        ink(&S, s, C - S.width(now.as_str()) / 2, 38, now.as_str(), GREY);
    }
    // "5 min" / "1 h 05" big, its numbers white and its units grey
    let left = duration(rem);
    let mut rest = Buf::new();
    let _ = write!(rest, " · {}", distance(st.remaining_10m as u32 * 10, st.imperial()).as_str());
    let eta = clock.map(|c| arrival(c, rem, st.clock24()));
    let y = 88;
    let max = room(&L, y, 4);
    let mut w = L.width(left.as_str()) + S.width(rest.as_str());
    let eta_w = eta.as_ref().map_or(0, |e| S.width(" · ") + S.width(e.as_str()));
    let show_eta = w + eta_w <= max;
    if show_eta {
        w += eta_w;
    }
    let mut x = C - w / 2;
    for (k, word) in left.as_str().split(' ').enumerate() {
        if k > 0 {
            x += L.width(" ");
        }
        let digits = word.starts_with(|c: char| c.is_ascii_digit());
        x = ink(&L, s, x, y, word, if digits { WHITE } else { GREY });
    }
    x = ink(&S, s, x, y, rest.as_str(), GREY);
    if let (true, Some(e)) = (show_eta, &eta) {
        x = ink(&S, s, x, y, " · ", GREY);
        ink(&S, s, x, y, e.as_str(), WHITE);
    }
    small(s, 120, m.text(), WHITE);
}

/// The share of the route behind the car, 0..=ONE, from the road distance left and the route line's length.
pub fn done(m: &Model) -> i32 {
    let total = m.length_m();
    if total == 0 {
        return 0;
    }
    let left = (m.state.remaining_10m as i64 * 10).min(total);
    ((total - left) * cmf_wp2_ui::ONE as i64 / total) as i32
}

fn progress(s: &mut impl Surface, k: i32) {
    let (r0, r1) = ARC_R;
    let span = 2 * ARC_HALF;
    let right = (0x4000 - ARC_HALF) as u16;
    arc(s, C, C, r0, r1, right, span, TRACK, None, BLACK);
    // filled from the left end, which is the start of the arc's span when going anticlockwise
    let fill = span * k.clamp(0, cmf_wp2_ui::ONE) as u32 / cmf_wp2_ui::ONE as u32;
    if fill > 0 {
        arc(s, C, C, r0, r1, (0x4000 + ARC_HALF - fill) as u16, fill, WHITE, None, BLACK);
    }
    let rm = (r0 + r1) / 2;
    let end = |a: u32| {
        (
            (C as i64 + rm as i64 * cos(a as u16) as i64 / Q14 as i64) as i32,
            (C as i64 + rm as i64 * sin(a as u16) as i64 / Q14 as i64) as i32,
        )
    };
    let (l, rgt) = (end(0x4000 + ARC_HALF), end(0x4000 - ARC_HALF));
    disc(s, l.0, l.1, (r1 - r0) / 2, if fill > 0 { WHITE } else { TRACK }, BLACK);
    disc(s, rgt.0, rgt.1, (r1 - r0) / 2, if fill >= span { WHITE } else { TRACK }, BLACK);
}

/// The next turn, big at the bottom: arrow, distance, unit; the one after it small when it comes quickly.
fn turn(s: &mut impl Surface, m: &Model) {
    let st = &m.state;
    let y = 378; // baseline of the distance
    if st.maneuver == man::OFF_ROUTE {
        icon(s, man::OFF_ROUTE, C - 96, y - 28, 30, ACCENT);
        ink(&L, s, C - 50, y - 26, "Off route", ACCENT);
        ink(&S, s, C - 50, y + 8, "Rerouting…", GREY);
        return;
    }
    let d = distance(st.dist_m as u32, st.imperial());
    let (num, unit) = d.as_str().split_once(' ').unwrap_or((d.as_str(), ""));
    let (iw, gap, ugap) = (72, 22, 8);
    let w = iw + gap + XL.width(num) + ugap + L.width(unit);
    let x = C - w / 2;
    icon(s, st.maneuver, x + iw / 2, y - XL.cap / 2, 34, WHITE);
    let after = ink(&XL, s, x + iw + gap, y, num, WHITE);
    let end = ink(&L, s, after + ugap, y, unit, GREY);
    if st.after != man::NONE && st.after_m <= 300 {
        icon(s, st.after, end + 26, y - L.cap - 30, 13, GREY);
    }
}

/// The whole screen.
pub fn draw(s: &mut impl Surface, m: &Model, ctx: &Ctx, fade: &mut Fade) {
    let st = &m.state;
    if !m.has_state || !ctx.live {
        return message(s, "Vela Nav", if m.has_state { "Phone out of reach" } else { "Waiting for Vela…" });
    }
    if !st.active() {
        return launcher(s, m, ctx.pending);
    }
    if st.arrived() {
        s.rect(0, 0, SIZE, SIZE, 0);
        icon(s, man::ARRIVE, C, 168, 46, RED);
        L.draw(s, C - L.width("Arrived") / 2, 268, "Arrived", WHITE.rgb565(), 0);
        small(s, 308, m.text(), GREY);
        return;
    }
    let mpp = ZOOMS[ctx.zoom.min(ZOOMS.len() - 1)];
    let at = ctx.pose.unwrap_or(Pose::of(st));
    s.rect(0, 0, SIZE, SIZE, 0);
    if m.roads().n_lines > 0 {
        fade.build(m, at, mpp);
        roads(s, m, fade, at, mpp);
    }
    if m.route_len >= 2 {
        let p = (st.progress as usize).clamp(1, m.route_len);
        polyline(s, m, 0, p, 10, PASSED, at, mpp);
        polyline(s, m, p - 1, m.route_len, 16, WHITE, at, mpp);
        let end = project(m.route[m.route_len - 1], at.pos(), at.course, mpp);
        if geom::inside(end.0, end.1) {
            disc(s, end.0, end.1, 13, BLACK, WHITE);
            disc(s, end.0, end.1, 9, RED, BLACK);
        }
    }
    car(s, st.fix());
    if st.limit > 0 {
        limit_badge(s, st.limit, 388, 282);
    }
    header(s, m, ctx.clock);
    turn(s, m);
    progress(s, done(m));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::enc::{route, state};
    use crate::proto::{F_ACTIVE, F_CLOCK24, F_FIX, State};
    use cmf_wp2_ui::Fb;
    use std::boxed::Box;
    use std::vec;

    #[test]
    fn formats() {
        assert_eq!(distance(347, false).as_str(), "340 m");
        assert_eq!(distance(42, false).as_str(), "40 m");
        assert_eq!(distance(1249, false).as_str(), "1.2 km");
        assert_eq!(distance(9960, false).as_str(), "10.0 km");
        assert_eq!(distance(23_600, false).as_str(), "24 km");
        assert_eq!(distance(100, true).as_str(), "350 ft");
        assert_eq!(distance(500, true).as_str(), "0.3 mi");
        assert_eq!(distance(40_000, true).as_str(), "25 mi");
        assert_eq!(duration(840).as_str(), "14 min");
        assert_eq!(duration(3900).as_str(), "1 h 05");
        assert_eq!(arrival((23, 50), 1200, true).as_str(), "00:10");
        assert_eq!(arrival((12, 5), 0, false).as_str(), "12:05 pm");
        assert_eq!(arrival((0, 59), 60, false).as_str(), "1:00 am");
    }

    #[test]
    fn heading_up() {
        // heading east (a quarter turn): a point 100 m east is straight ahead, one 100 m south is to the right
        assert_eq!(project([100, 0], [0, 0], 0x4000, 1), (CAR.0, CAR.1 - 100));
        assert_eq!(project([0, -100], [0, 0], 0x4000, 1), (CAR.0 + 100, CAR.1));
        assert_eq!(project([0, 100], [0, 0], 0, 2), (CAR.0, CAR.1 - 50));
        // far away does not overflow
        let far = project([2_000_000_000, -2_000_000_000], [-2_000_000_000, 2_000_000_000], 0x2000, 1);
        assert!(far.0 > SIZE);
    }

    #[test]
    fn clipping() {
        assert_eq!(clip((10, 10), (20, 20), 0, 100), Some(((10, 10), (20, 20))));
        assert_eq!(clip((-50, -50), (-10, -10), 0, 100), None);
        assert_eq!(clip((-100, 50), (200, 50), 0, 100), Some(((0, 50), (100, 50))));
    }

    fn model() -> Box<Model> {
        let mut m = Box::new(Model::new());
        let pts: vec::Vec<[i32; 2]> =
            (0..60).map(|i| if i < 30 { [0, i * 20] } else { [(i - 29) * 20, 580] }).collect();
        let (msgs, seq) = route(0, 1, &pts);
        msgs.iter().for_each(|x| {
            m.apply(x);
        });
        let st = State {
            flags: F_ACTIVE | F_FIX | F_CLOCK24,
            maneuver: man::RIGHT,
            dist_m: 180,
            remaining_10m: 120,
            remaining_s: 300,
            limit: 50,
            x: 0,
            y: 400,
            progress: 21,
            ..State::default()
        };
        m.apply(&state(seq, &st));
        m.apply(&crate::proto::enc::msg(b'T', seq.wrapping_add(1), b"Bannerghatta Main Road"));
        m
    }

    #[test]
    fn draws_a_drive() {
        let m = model();
        let mut px = vec![0u16; (SIZE * SIZE) as usize];
        let mut fb = Fb::new(&mut px);
        draw(
            &mut fb,
            &m,
            &Ctx { live: true, clock: Some((17, 0)), zoom: ZOOM_DEFAULT, pose: None, pending: None },
            &mut Box::new(Fade::new()),
        );
        // the route ahead passes above the car, the car is drawn, the turn goes right of the centre
        assert_eq!(fb.at(CAR.0, CAR.1 - 50), WHITE.rgb565());
        // the car: lit left face, shaded right face, a black rim above its tip
        assert_eq!(fb.at(CAR.0 - 5, CAR.1 + 4), WHITE.rgb565());
        assert_eq!(fb.at(CAR.0 + 5, CAR.1 + 4), SHADE.rgb565());
        assert_eq!(fb.at(CAR.0, CAR.1 - 27), 0);
        assert_eq!(fb.at(388, 262), WHITE.rgb565()); // limit badge, above its digits
        assert_eq!(fb.at(388, 282 - 28), RED.rgb565());
        // 1.2 km left of a 1.18 km line: nothing done yet, the arc is all track
        assert_eq!(done(&m), 0);
        assert_eq!(fb.at(C, C + 218), TRACK.rgb565());
        assert!(fb.rects < 20_000, "{}", fb.rects);
        // nothing lit outside the circle
        for y in 0..SIZE {
            for x in 0..SIZE {
                if !geom::inside(x, y) {
                    assert_eq!(fb.at(x, y), 0, "lit at {x},{y}");
                }
            }
        }
    }

    #[test]
    fn streets_fade_away_from_the_route() {
        let mut m = model();
        // the route runs north at x 0; a street crosses it east-west at y 500, and one runs north far to the east
        let lines = vec![(false, vec![[-600, 500], [600, 500]]), (true, vec![[500, 300], [500, 700]])];
        // the model's run used seq 0..=5
        let (msgs, _) = crate::proto::enc::roads(6, 1, [0, 400], &lines);
        msgs.iter().for_each(|x| {
            m.apply(x);
        });
        assert_eq!(m.roads().n_lines, 2);
        let mut px = vec![0u16; (SIZE * SIZE) as usize];
        let mut fb = Fb::new(&mut px);
        let mut fade = Box::new(Fade::new());
        draw(&mut fb, &m, &Ctx { live: true, clock: None, zoom: ZOOM_DEFAULT, pose: None, pending: None }, &mut fade);
        // 100 m ahead of the car at 3 m/px: the cross street, full strength beside the route, gone 200 px away
        let y = CAR.1 - 100 / 3;
        assert_eq!(fb.at(C + 30, y), ROAD_MINOR.rgb565());
        assert_eq!(fb.at(C - 220, y), 0);
        assert!(fb.rects < 30_000, "{}", fb.rects);
    }

    #[test]
    fn the_car_glides_the_short_way_round() {
        let a = Pose { x: 0, y: 0, course: 65000 };
        let b = Pose { x: 100, y: -40, course: 500 };
        let half = a.toward(b, 512);
        assert_eq!((half.x, half.y), (50, -20));
        // 65000 -> 500 turns through north (1036 units), not back round the whole compass
        assert_eq!(half.course, 65000u16.wrapping_add(518));
        assert_eq!(a.toward(b, 1024), b);
        assert_eq!(a.toward(b, 0), a);
        assert!(a.moved_px(b, 3) >= 33);
        assert_eq!(a.moved_px(a, 3), 0);
    }

    #[test]
    fn the_glide_follows_the_updates() {
        let mut g = Glide::new();
        let p = |x, course| Pose { x, y: 0, course };
        g.target(p(0, 0), 1000);
        assert_eq!(g.at(1000), p(0, 0)); // the first one is where it is
        g.target(p(20, 1000), 2000);
        assert!(g.moving(2400));
        assert_eq!(g.at(2500), p(10, 500)); // half way through the second between updates
        assert_eq!(g.at(3000), p(20, 1000));
        assert!(!g.moving(3000));
        // an update arriving mid-glide starts from where the car is drawn, not where it was going
        g.target(p(40, 1000), 3000);
        g.target(p(60, 1000), 3500);
        assert_eq!(g.at(3500).x, 30);
        // a reroute far away is not glided across the map
        g.target(p(5000, 0), 4000);
        assert_eq!(g.at(4000), p(5000, 0));
    }

    #[test]
    fn without_a_drive_home_and_work_are_buttons() {
        let mut m = Box::new(Model::new());
        m.apply(&state(0, &State { flags: F_CLOCK24, ..State::default() }));
        m.apply(&crate::proto::enc::msg(b'P', 1, &[crate::proto::P_HOME, 0]));
        assert_eq!(m.places, crate::proto::P_HOME);
        let mut px = vec![0u16; (SIZE * SIZE) as usize];
        let mut fb = Fb::new(&mut px);
        let ctx = |pending| Ctx { live: true, clock: None, zoom: ZOOM_DEFAULT, pose: None, pending };
        draw(&mut fb, &m, &ctx(None), &mut Box::new(Fade::new()));
        let (home, work) = (BUTTONS[0].2 + 4, BUTTONS[1].2 + 4);
        assert_eq!(fb.at(C, home), PILL.rgb565()); // Home is set
        assert_ne!(fb.at(C, work), PILL.rgb565()); // Work is not: dimmed
        drop(fb);
        let mut fb = Fb::new(&mut px);
        draw(&mut fb, &m, &ctx(Some(crate::proto::P_HOME)), &mut Box::new(Fade::new()));
        assert_eq!(fb.at(C, home), WHITE.rgb565()); // tapped: lit while the phone starts it
        assert_eq!(button_at(BUTTONS[0].2 + 30), Some(crate::proto::P_HOME));
        assert_eq!(button_at(BUTTONS[1].2 + 30), Some(crate::proto::P_WORK));
        assert_eq!(button_at(60), None);
        assert!(L.covers("Home Work Starting…") && S.covers("Couldn't plan that drive Set Home and Work in Vela"));
    }

    #[test]
    fn the_arc_fills_with_the_drive() {
        let mut m = model();
        assert_eq!(m.length_m(), 1180);
        m.state.remaining_10m = 59;
        assert_eq!(done(&m), 512);
        let mut px = vec![0u16; (SIZE * SIZE) as usize];
        let mut fb = Fb::new(&mut px);
        progress(&mut fb, 512);
        // half full: white left of 6 o'clock, track right of it
        assert_eq!(fb.at(C - 40, C + 214), WHITE.rgb565());
        assert_eq!(fb.at(C + 40, C + 214), TRACK.rgb565());
    }

    #[test]
    fn every_icon_stays_in_its_box() {
        for code in 0..=13 {
            let mut px = vec![0u16; (SIZE * SIZE) as usize];
            let mut fb = Fb::new(&mut px);
            icon(&mut fb, code, C, C, 32, WHITE);
            for y in 0..SIZE {
                for x in 0..SIZE {
                    if fb.at(x, y) != 0 {
                        assert!((x - C).abs() <= 40 && (y - C).abs() <= 40, "icon {code} at {x},{y}");
                    }
                }
            }
        }
    }
}
