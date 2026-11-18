//! Drawing, against `Surface`: the watch through `&Api`, a host framebuffer in tests.
//!
//! Layout on the round 466 px panel: the next turn at the top (icon, distance, road), a heading-up map of the
//! route line around the car in the middle, arrival time and what is left at the bottom.

use crate::proto::{Model, State, man};
use cmf_wp2_ui::draw::{arc, disc, line, ring};
use cmf_wp2_ui::motion::{Q14, cos, sin};
use cmf_wp2_ui::text::{big, big_width, centre, fitted};
use cmf_wp2_ui::{C, CHAR, Rgb, SIZE, Surface, geom};
use core::fmt::Write;

const BLACK: Rgb = Rgb(0, 0, 0);
const WHITE: Rgb = Rgb(255, 255, 255);
const GREY: Rgb = Rgb(110, 110, 110);
const DIM: u16 = 0x8410;
const ROUTE: Rgb = Rgb(60, 140, 255);
const PASSED: Rgb = Rgb(70, 70, 80);
const ACCENT: Rgb = Rgb(255, 160, 0);
const RED: Rgb = Rgb(230, 50, 40);

/// Bottom of the turn banner and top of the trip line; the map shows between them.
pub const TOP: i32 = 156;
pub const BOTTOM: i32 = 384;
/// The car, heading up.
pub const CAR: (i32, i32) = (C, 318);
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
            if tenths < 100 { write!(b, "{}.{} mi", tenths / 10, tenths % 10) } else { write!(b, "{} mi", (tenths + 5) / 10) }
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
    let code = |p: (i32, i32)| {
        (p.0 < lo) as u8 | ((p.0 > hi) as u8) << 1 | ((p.1 < lo) as u8) << 2 | ((p.1 > hi) as u8) << 3
    };
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
    // the bends would have notches. The disc lies inside the line, so its edge blends into the line's colour.
    for i in from + 1..to.saturating_sub(1) {
        let p = project(m.route[i], car, st.course, mpp);
        if geom::inside(p.0, p.1) && p.1 >= TOP - w && p.1 <= BOTTOM + w {
            disc(s, p.0, p.1, w / 2, c, c);
        }
    }
}

/// Direction `a` (turns, clockwise from up) as a screen vector of length `r`.
fn dir(a: u16, r: i32) -> (i32, i32) {
    ((r * sin(a)) >> 14, -(r * cos(a)) >> 14)
}

/// A filled triangle (no antialiasing; small icons only).
fn triangle(s: &mut impl Surface, p: [(i32, i32); 3], c: u16) {
    let y0 = p.iter().map(|q| q.1).min().unwrap_or(0);
    let y1 = p.iter().map(|q| q.1).max().unwrap_or(0);
    for y in y0..=y1 {
        let (mut lo, mut hi) = (i32::MAX, i32::MIN);
        for i in 0..3 {
            let (a, b) = (p[i], p[(i + 1) % 3]);
            if (a.1 <= y && y <= b.1) || (b.1 <= y && y <= a.1) {
                let x = if a.1 == b.1 { a.0.min(b.0) } else { a.0 + (b.0 - a.0) * (y - a.1) / (b.1 - a.1) };
                let x2 = if a.1 == b.1 { a.0.max(b.0) } else { x };
                (lo, hi) = (lo.min(x), hi.max(x2));
            }
        }
        if lo <= hi {
            s.rect(lo, y, hi - lo + 1, 1, c);
        }
    }
}

/// An arrowhead with its tip at `tip`, pointing along `a`, `h` px long.
fn arrow_head(s: &mut impl Surface, tip: (i32, i32), a: u16, h: i32, c: Rgb) {
    let back = dir(a, -h);
    let side = dir(a.wrapping_add(0x4000), h * 3 / 4);
    let base = (tip.0 + back.0, tip.1 + back.1);
    triangle(s, [tip, (base.0 + side.0, base.1 + side.1), (base.0 - side.0, base.1 - side.1)], c.rgb565());
}

/// A maneuver icon in a box of radius `r` around (cx, cy).
pub fn icon(s: &mut impl Surface, code: u8, cx: i32, cy: i32, r: i32, c: Rgb) {
    let w = (r / 4).max(3);
    let turn = |s: &mut _, a: u16, elbow: i32| {
        let e = (cx, cy + elbow);
        line(s, cx, cy + r, e.0, e.1, w, c, BLACK);
        let d = dir(a, r);
        let tip = (e.0 + d.0, e.1 + d.1);
        let shaft = dir(a, r - r / 3);
        disc(s, e.0, e.1, w / 2, c, BLACK);
        line(s, e.0, e.1, e.0 + shaft.0, e.1 + shaft.1, w, c, BLACK);
        arrow_head(s, tip, a, r / 2, c);
    };
    match code {
        man::CONTINUE => {
            line(s, cx, cy + r, cx, cy - r / 2, w, c, BLACK);
            arrow_head(s, (cx, cy - r), 0, r / 2, c);
        }
        man::SLIGHT_LEFT => turn(s, 0xE000, 0),
        man::SLIGHT_RIGHT => turn(s, 0x2000, 0),
        man::LEFT => turn(s, 0xC000, -r / 6),
        man::RIGHT => turn(s, 0x4000, -r / 6),
        man::SHARP_LEFT => turn(s, 0xA000, -r / 2),
        man::SHARP_RIGHT => turn(s, 0x6000, -r / 2),
        man::KEEP_LEFT => turn(s, 0xF000, r / 4),
        man::KEEP_RIGHT => turn(s, 0x1000, r / 4),
        man::UTURN => {
            let (rx, top) = (r / 2, cy - r / 4);
            line(s, cx + rx, cy + r, cx + rx, top, w, c, BLACK);
            arc(s, cx, top, rx - w / 2, rx + w / 2, 0x8000, 0x8000, c, None, BLACK);
            line(s, cx - rx, top, cx - rx, cy + r / 3, w, c, BLACK);
            arrow_head(s, (cx - rx, cy + r), 0x8000, r * 2 / 3, c);
        }
        man::ROUNDABOUT => {
            let rr = r * 2 / 5;
            ring(s, cx, cy, rr - w / 2, rr + w / 2, c, BLACK);
            line(s, cx, cy + r, cx, cy + rr, w, c, BLACK);
            line(s, cx, cy - rr, cx, cy - r / 2 - 2, w, c, BLACK);
            arrow_head(s, (cx, cy - r - 4), 0, r / 2, c);
        }
        man::ARRIVE => {
            let pin = cy - r / 4;
            line(s, cx, pin, cx, cy + r, w, RED, BLACK);
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
}

fn car(s: &mut impl Surface, fix: bool) {
    let (x, y) = CAR;
    let body = if fix { ROUTE } else { GREY };
    disc(s, x, y, 17, WHITE, BLACK);
    disc(s, x, y, 14, body, WHITE);
    // a white triangle pointing up
    for row in 0..14 {
        let half = row * 7 / 13;
        s.rect(x - half, y - 8 + row, 2 * half + 1, 1, WHITE.rgb565());
    }
}

fn limit_badge(s: &mut impl Surface, limit: u8) {
    let (x, y) = (70, 250);
    disc(s, x, y, 32, RED, BLACK);
    disc(s, x, y, 25, WHITE, RED);
    let mut b = Buf::new();
    let _ = write!(b, "{}", limit);
    let scale = if limit >= 100 { 2 } else { 3 };
    big(s, x - big_width(b.as_str(), scale) / 2, y - 4 * scale, b.as_str(), scale, BLACK.rgb565());
}

/// A text line centred on the screen at `y`, cut to what the circle leaves there.
fn line_at(s: &mut impl Surface, y: i32, t: &str, c: u16) {
    let (_, cells) = geom::text_box(y, 8);
    let n = cmf_wp2_ui::text::chars(t).min(cells);
    fitted(s, C - n as i32 * CHAR / 2, y, t, n, c);
}

fn message(s: &mut impl Surface, title: &str, sub: &str) {
    s.rect(0, 0, SIZE, SIZE, 0);
    big(s, C - big_width(title, 3) / 2, 196, title, 3, WHITE.rgb565());
    line_at(s, 244, sub, DIM);
}

fn banner(s: &mut impl Surface, m: &Model) {
    let st = &m.state;
    s.rect(0, 0, SIZE, TOP, 0);
    if st.maneuver == man::OFF_ROUTE {
        icon(s, man::OFF_ROUTE, C, 62, 30, ACCENT);
        line_at(s, 108, "Off route", ACCENT.rgb565());
        line_at(s, 130, "Rerouting...", DIM);
        return;
    }
    icon(s, st.maneuver, C, 60, 32, WHITE);
    let d = distance(st.dist_m as u32, st.imperial());
    big(s, C - big_width(d.as_str(), 3) / 2, 100, d.as_str(), 3, WHITE.rgb565());
    line_at(s, 130, m.text(), DIM);
    // the turn right after this one, when it comes quickly
    if st.after != man::NONE && st.after_m <= 300 {
        icon(s, st.after, 330, 70, 14, GREY);
    }
}

fn trip(s: &mut impl Surface, st: &State, clock: Option<(u8, u8)>) {
    s.rect(0, BOTTOM, SIZE, SIZE - BOTTOM, 0);
    let rem = st.remaining_s as u32;
    if let Some(c) = clock {
        let a = arrival(c, rem, st.clock24());
        centre(s, C, BOTTOM + 8, a.as_str(), WHITE.rgb565());
    }
    let mut b = Buf::new();
    let _ = write!(b, "{}  {}", distance(st.remaining_10m as u32 * 10, st.imperial()).as_str(), duration(rem).as_str());
    line_at(s, BOTTOM + 30, b.as_str(), DIM);
}

/// The whole screen.
pub fn draw(s: &mut impl Surface, m: &Model, ctx: &Ctx) {
    let st = &m.state;
    if !m.has_state || !ctx.live {
        return message(s, "Vela Nav", if m.has_state { "Phone out of reach" } else { "Waiting for Vela..." });
    }
    if !st.active() {
        return message(s, "Vela Nav", "Start a route in Vela");
    }
    if st.arrived() {
        s.rect(0, 0, SIZE, SIZE, 0);
        icon(s, man::ARRIVE, C, 150, 50, RED);
        big(s, C - big_width("Arrived", 3) / 2, 230, "Arrived", 3, WHITE.rgb565());
        line_at(s, 276, m.text(), DIM);
        return;
    }
    let mpp = ZOOMS[ctx.zoom.min(ZOOMS.len() - 1)];
    s.rect(0, TOP, SIZE, BOTTOM - TOP, 0);
    if m.route_len >= 2 {
        let p = (st.progress as usize).clamp(1, m.route_len);
        polyline(s, m, 0, p, 8, PASSED, mpp);
        polyline(s, m, p - 1, m.route_len, 12, ROUTE, mpp);
        let end = project(m.route[m.route_len - 1], [st.x, st.y], st.course, mpp);
        if geom::inside(end.0, end.1) {
            disc(s, end.0, end.1, 11, WHITE, BLACK);
            disc(s, end.0, end.1, 8, RED, WHITE);
        }
    }
    car(s, st.fix());
    if st.limit > 0 {
        limit_badge(s, st.limit);
    }
    banner(s, m);
    trip(s, st, ctx.clock);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::enc::{route, state};
    use crate::proto::{F_ACTIVE, F_CLOCK24, F_FIX};
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
        let pts: vec::Vec<[i32; 2]> = (0..60).map(|i| if i < 30 { [0, i * 20] } else { [(i - 29) * 20, 580] }).collect();
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
        assert_eq!(fb.at(CAR.0, CAR.1 - 40), ROUTE.rgb565());
        assert_eq!(fb.at(CAR.0, CAR.1 + 2), WHITE.rgb565());
        assert_eq!(fb.at(70, 229), WHITE.rgb565()); // limit badge, above its digits
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
