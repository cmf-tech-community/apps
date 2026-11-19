//! Drawing, against `Surface`: the watch through `&Api`, a host framebuffer in tests.
//!
//! Layout on the round 466 px panel, black like the stock faces: the time now, the time left big with the
//! distance and arrival, and the road at the top; a heading-up map of the route in white around the car; the next
//! turn big at the bottom (arrow, distance, speed limit) over an arc that fills as the drive goes. Text is Noto Sans (`font`, `fonts`), with
//! the loader's pixel font for a road name in a script those fonts lack.

use crate::font::poly;
use crate::fonts::{L, S, XL};
use crate::proto::{Model, man};
use crate::sign::{Sign, head};
use cmf_wp2_ui::draw::{arc, disc, line, ring};
use cmf_wp2_ui::motion::{Q14, cos, sin};
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

/// Bottom of the top lines and top of the turn; the map shows between them.
pub const TOP: i32 = 130;
pub const BOTTOM: i32 = 316;
/// The car, heading up.
pub const CAR: (i32, i32) = (C, 278);
/// The progress arc along the bottom edge: radii, and half its span either side of 6 o'clock.
const ARC_R: (i32, i32) = (214, 222);
const ARC_HALF: u32 = 8192; // 45°
/// Map scales the crown steps through, in metres per pixel.
pub const ZOOMS: [i32; 6] = [1, 2, 3, 5, 8, 13];
pub const ZOOM_DEFAULT: usize = 2;

/// What the screen is about, besides the model itself.
pub struct Ctx {
    /// The phone has been heard from recently.
    pub live: bool,
    /// Local time of day (hours, minutes), when the watch has it.
    pub clock: Option<(u8, u8)>,
    pub zoom: usize,
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

fn polyline(s: &mut impl Surface, m: &Model, from: usize, to: usize, w: i32, c: Rgb, mpp: i32) {
    let st = &m.state;
    let car = [st.x, st.y];
    let mut prev: Option<(i32, i32)> = None;
    for i in from..to {
        let p = project(m.route[i], car, st.course, mpp);
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
        let p = project(m.route[i], car, st.course, mpp);
        if geom::inside(p.0, p.1) && p.1 >= TOP - w && p.1 <= BOTTOM + w {
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
            ring(s, cx, cy, r - w, r, ACCENT, BLACK);
            let k = ACCENT.rgb565();
            s.rect(cx - w / 2, cy - r / 2, w, r * 2 / 3, k);
            s.rect(cx - w / 2, cy + r / 3, w, w, k);
        }
        _ => {}
    }
    let m = w + 6;
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
        S.centre(s, C, y, t, room(&S, y, 14), c.rgb565(), 0);
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
    s.rect(0, 0, SIZE, TOP, 0);
    let rem = st.remaining_s as u32;
    if let Some(c) = clock {
        let now = arrival(c, 0, st.clock24());
        S.draw(s, C - S.width(now.as_str()) / 2, 38, now.as_str(), GREY.rgb565(), 0);
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
            x = L.draw(s, x, y, " ", 0, 0);
        }
        let digits = word.starts_with(|c: char| c.is_ascii_digit());
        x = L.draw(s, x, y, word, if digits { WHITE } else { GREY }.rgb565(), 0);
    }
    x = S.draw(s, x, y, rest.as_str(), GREY.rgb565(), 0);
    if let (true, Some(e)) = (show_eta, &eta) {
        x = S.draw(s, x, y, " · ", GREY.rgb565(), 0);
        S.draw(s, x, y, e.as_str(), WHITE.rgb565(), 0);
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
        L.draw(s, C - 50, y - 26, "Off route", ACCENT.rgb565(), 0);
        S.draw(s, C - 50, y + 8, "Rerouting…", GREY.rgb565(), 0);
        return;
    }
    let d = distance(st.dist_m as u32, st.imperial());
    let (num, unit) = d.as_str().split_once(' ').unwrap_or((d.as_str(), ""));
    let (iw, gap, ugap) = (72, 22, 8);
    let w = iw + gap + XL.width(num) + ugap + L.width(unit);
    let x = C - w / 2;
    icon(s, st.maneuver, x + iw / 2, y - XL.cap / 2, 34, WHITE);
    let after = XL.draw(s, x + iw + gap, y, num, WHITE.rgb565(), 0);
    let end = L.draw(s, after + ugap, y, unit, GREY.rgb565(), 0);
    if st.after != man::NONE && st.after_m <= 300 {
        icon(s, st.after, end + 26, y - L.cap - 30, 13, GREY);
    }
}

/// The whole screen.
pub fn draw(s: &mut impl Surface, m: &Model, ctx: &Ctx) {
    let st = &m.state;
    if !m.has_state || !ctx.live {
        return message(s, "Vela Nav", if m.has_state { "Phone out of reach" } else { "Waiting for Vela…" });
    }
    if !st.active() {
        return message(s, "Vela Nav", "Start a route in Vela");
    }
    if st.arrived() {
        s.rect(0, 0, SIZE, SIZE, 0);
        icon(s, man::ARRIVE, C, 168, 46, RED);
        L.draw(s, C - L.width("Arrived") / 2, 268, "Arrived", WHITE.rgb565(), 0);
        small(s, 308, m.text(), GREY);
        return;
    }
    let mpp = ZOOMS[ctx.zoom.min(ZOOMS.len() - 1)];
    s.rect(0, TOP, SIZE, BOTTOM - TOP, 0);
    if m.route_len >= 2 {
        let p = (st.progress as usize).clamp(1, m.route_len);
        polyline(s, m, 0, p, 10, PASSED, mpp);
        polyline(s, m, p - 1, m.route_len, 16, WHITE, mpp);
        let end = project(m.route[m.route_len - 1], [st.x, st.y], st.course, mpp);
        if geom::inside(end.0, end.1) && end.1 > TOP + 12 && end.1 < BOTTOM - 12 {
            disc(s, end.0, end.1, 13, BLACK, WHITE);
            disc(s, end.0, end.1, 9, RED, BLACK);
        }
    }
    car(s, st.fix());
    s.rect(0, BOTTOM, SIZE, SIZE - BOTTOM, 0);
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
        draw(&mut fb, &m, &Ctx { live: true, clock: Some((17, 0)), zoom: ZOOM_DEFAULT });
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
