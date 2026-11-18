//! The Vela ↔ watch protocol and the navigation model it fills. Pure: no `Api`, so `make test` covers it.
//!
//! Every message starts with the magic `n1`. Phone → watch messages then carry a kind byte and a `seq` byte that
//! counts every phone message (wrapping); a gap makes the watch ask for a snapshot again. All numbers are
//! little-endian.
//!
//! | Kind | Body | Meaning |
//! |---|---|---|
//! | `S` | flags u8, maneuver u8, dist u16 m, after u8, after dist u16 m, remaining u16 (10 m), remaining u16 s, limit u8, x i32 m, y i32 m, course u16, progress u16 | the drive, about once a second |
//! | `T` | UTF-8, ≤ 100 B | the road being entered (or the destination on arrival) |
//! | `B` | id u8, count u16, scale u8 (m per unit) | a new route line begins |
//! | `R` | id u8, start u16, n u8, n × (x i16, y i16) | route points `start..start+n`, n ≤ 30 |
//! | `E` | id u8, count u16 | the route line is complete |
//!
//! Watch → phone: `n1Q`, "send me everything" (on start, after a gap, after `RESUME`, and while nothing arrives).
//!
//! Positions are metres east (`x`) and north (`y`) of an origin the phone picks per drive, never latitude and
//! longitude: the channel can be read by anyone in Bluetooth range. `course` is the heading in 1/65536 turns
//! clockwise from north. `progress` is the index of the first route point still ahead of the car.

pub const ASK: [u8; 3] = *b"n1Q";
pub const ROUTE_MAX: usize = 512;
pub const TEXT_MAX: usize = 100;
pub const CHUNK_MAX: usize = 30;

pub const F_ACTIVE: u8 = 1 << 0;
pub const F_ARRIVED: u8 = 1 << 1;
pub const F_IMPERIAL: u8 = 1 << 3;
pub const F_FIX: u8 = 1 << 4;
pub const F_CLOCK24: u8 = 1 << 5;
/// This state ends the snapshot the watch asked for with `n1Q`: everything before it in the run is current.
pub const F_SNAPSHOT: u8 = 1 << 6;

/// Maneuver codes, the same numbers as Vela's `DeviceManeuver`.
pub mod man {
    pub const NONE: u8 = 0;
    pub const CONTINUE: u8 = 1;
    pub const SLIGHT_LEFT: u8 = 2;
    pub const LEFT: u8 = 3;
    pub const SHARP_LEFT: u8 = 4;
    pub const SLIGHT_RIGHT: u8 = 5;
    pub const RIGHT: u8 = 6;
    pub const SHARP_RIGHT: u8 = 7;
    pub const KEEP_LEFT: u8 = 8;
    pub const KEEP_RIGHT: u8 = 9;
    pub const UTURN: u8 = 10;
    pub const ROUNDABOUT: u8 = 11;
    pub const OFF_ROUTE: u8 = 12;
    pub const ARRIVE: u8 = 13;
}

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct State {
    pub flags: u8,
    pub maneuver: u8,
    pub dist_m: u16,
    pub after: u8,
    pub after_m: u16,
    pub remaining_10m: u16,
    pub remaining_s: u16,
    pub limit: u8,
    pub x: i32,
    pub y: i32,
    pub course: u16,
    pub progress: u16,
}

impl State {
    pub fn active(&self) -> bool {
        self.flags & F_ACTIVE != 0
    }
    pub fn arrived(&self) -> bool {
        self.flags & F_ARRIVED != 0
    }
    pub fn imperial(&self) -> bool {
        self.flags & F_IMPERIAL != 0
    }
    pub fn fix(&self) -> bool {
        self.flags & F_FIX != 0
    }
    pub fn clock24(&self) -> bool {
        self.flags & F_CLOCK24 != 0
    }
}

/// What a message did to the model.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Effect {
    /// Not ours or malformed: ignored.
    Ignored,
    /// Applied; the screen is out of date.
    Changed,
    /// Messages were lost: ask the phone for a snapshot (`ASK`).
    Resync,
}

pub struct Model {
    pub state: State,
    /// A state arrived since the last `reset` (the screen shows "waiting" until then).
    pub has_state: bool,
    /// A state with `F_SNAPSHOT` arrived after the last gap: the watch has everything.
    pub synced: bool,
    text: [u8; TEXT_MAX],
    text_len: usize,
    pub route: [[i32; 2]; ROUTE_MAX],
    /// Points of a complete route line; 0 while none (or while a new one arrives).
    pub route_len: usize,
    route_id: Option<u8>,
    route_count: usize,
    route_got: usize,
    route_scale: i32,
    last_seq: Option<u8>,
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn i16_at(b: &[u8], i: usize) -> i16 {
    i16::from_le_bytes([b[i], b[i + 1]])
}
fn i32_at(b: &[u8], i: usize) -> i32 {
    i32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

impl Model {
    pub const fn new() -> Model {
        Model {
            state: State {
                flags: 0,
                maneuver: 0,
                dist_m: 0,
                after: 0,
                after_m: 0,
                remaining_10m: 0,
                remaining_s: 0,
                limit: 0,
                x: 0,
                y: 0,
                course: 0,
                progress: 0,
            },
            has_state: false,
            synced: false,
            text: [0; TEXT_MAX],
            text_len: 0,
            route: [[0; 2]; ROUTE_MAX],
            route_len: 0,
            route_id: None,
            route_count: 0,
            route_got: 0,
            route_scale: 1,
            last_seq: None,
        }
    }

    /// Forget the sequence (after a pause or a resync request): the next message starts a new run.
    pub fn reset_seq(&mut self) {
        self.last_seq = None;
    }

    /// The road line, cut on a character boundary.
    pub fn text(&self) -> &str {
        cmf_wp2_ui::text::fit(&self.text[..self.text_len], TEXT_MAX)
    }

    pub fn apply(&mut self, m: &[u8]) -> Effect {
        let [b'n', b'1', kind, seq, body @ ..] = m else {
            return Effect::Ignored;
        };
        let gap = self.last_seq.is_some_and(|l| *seq != l.wrapping_add(1));
        self.last_seq = Some(*seq);
        if gap {
            self.synced = false;
        }
        let applied = match kind {
            b'S' => self.on_state(body),
            b'T' => self.on_text(body),
            b'B' => self.on_begin(body),
            b'R' => self.on_points(body),
            b'E' => self.on_end(body),
            _ => Some(false),
        };
        match applied {
            None => {
                self.synced = false;
                Effect::Resync
            }
            Some(_) if gap => Effect::Resync,
            Some(true) => Effect::Changed,
            Some(false) => Effect::Ignored,
        }
    }

    // Each handler: Some(true) applied, Some(false) malformed (ignored), None the route broke (resync).

    fn on_state(&mut self, b: &[u8]) -> Option<bool> {
        if b.len() < 24 {
            return Some(false);
        }
        self.state = State {
            flags: b[0],
            maneuver: b[1],
            dist_m: u16_at(b, 2),
            after: b[4],
            after_m: u16_at(b, 5),
            remaining_10m: u16_at(b, 7),
            remaining_s: u16_at(b, 9),
            limit: b[11],
            x: i32_at(b, 12),
            y: i32_at(b, 16),
            course: u16_at(b, 20),
            progress: u16_at(b, 22),
        };
        self.has_state = true;
        if self.state.flags & F_SNAPSHOT != 0 {
            self.synced = true;
        }
        if !self.state.active() {
            // no drive: drop the line so a later drive never shows the previous one
            self.route_len = 0;
            self.route_id = None;
        }
        Some(true)
    }

    fn on_text(&mut self, b: &[u8]) -> Option<bool> {
        let t = cmf_wp2_ui::text::fit(b, TEXT_MAX);
        self.text[..t.len()].copy_from_slice(t.as_bytes());
        self.text_len = t.len();
        Some(true)
    }

    fn on_begin(&mut self, b: &[u8]) -> Option<bool> {
        if b.len() < 4 {
            return Some(false);
        }
        let count = u16_at(b, 1) as usize;
        if count > ROUTE_MAX || b[3] == 0 {
            return Some(false);
        }
        (self.route_id, self.route_count, self.route_got, self.route_scale) = (Some(b[0]), count, 0, b[3] as i32);
        self.route_len = 0;
        Some(true)
    }

    fn on_points(&mut self, b: &[u8]) -> Option<bool> {
        if b.len() < 4 {
            return Some(false);
        }
        let (id, start, n) = (b[0], u16_at(b, 1) as usize, b[3] as usize);
        if self.route_id != Some(id) {
            self.route_len = 0; // a chunk of a line whose begin we missed: the line shown is old
            return None;
        }
        if b.len() < 4 + 4 * n || n > CHUNK_MAX || start != self.route_got || start + n > self.route_count {
            self.route_id = None;
            return None;
        }
        for i in 0..n {
            let at = 4 + 4 * i;
            self.route[start + i] =
                [i16_at(b, at) as i32 * self.route_scale, i16_at(b, at + 2) as i32 * self.route_scale];
        }
        self.route_got += n;
        Some(false) // nothing to show until the end
    }

    fn on_end(&mut self, b: &[u8]) -> Option<bool> {
        if b.len() < 3 {
            return Some(false);
        }
        if self.route_id != Some(b[0]) || u16_at(b, 1) as usize != self.route_got || self.route_got != self.route_count {
            self.route_id = None;
            return None;
        }
        self.route_len = self.route_got;
        Some(true)
    }
}

impl Default for Model {
    fn default() -> Self {
        Model::new()
    }
}

#[cfg(test)]
pub mod enc {
    //! The phone side, for tests: what Vela's encoder must produce.
    use super::*;
    use std::vec::Vec;

    pub fn msg(kind: u8, seq: u8, body: &[u8]) -> Vec<u8> {
        let mut v = vec![b'n', b'1', kind, seq];
        v.extend_from_slice(body);
        assert!(v.len() <= 128);
        v
    }

    pub fn state(seq: u8, s: &State) -> Vec<u8> {
        let mut b = vec![s.flags, s.maneuver];
        b.extend(s.dist_m.to_le_bytes());
        b.push(s.after);
        b.extend(s.after_m.to_le_bytes());
        b.extend(s.remaining_10m.to_le_bytes());
        b.extend(s.remaining_s.to_le_bytes());
        b.push(s.limit);
        b.extend(s.x.to_le_bytes());
        b.extend(s.y.to_le_bytes());
        b.extend(s.course.to_le_bytes());
        b.extend(s.progress.to_le_bytes());
        msg(b'S', seq, &b)
    }

    /// Begin, chunks and end for a line of points in metres; returns the messages and the next seq.
    pub fn route(mut seq: u8, id: u8, pts: &[[i32; 2]]) -> (Vec<Vec<u8>>, u8) {
        let max = pts.iter().flat_map(|p| p.iter()).map(|v| v.abs()).max().unwrap_or(0);
        let scale = (max / i16::MAX as i32 + 1) as u8;
        let mut out = Vec::new();
        let n = (pts.len() as u16).to_le_bytes();
        out.push(msg(b'B', seq, &[id, n[0], n[1], scale]));
        seq = seq.wrapping_add(1);
        for (c, chunk) in pts.chunks(CHUNK_MAX).enumerate() {
            let start = ((c * CHUNK_MAX) as u16).to_le_bytes();
            let mut b = vec![id, start[0], start[1], chunk.len() as u8];
            for p in chunk {
                b.extend(((p[0] / scale as i32) as i16).to_le_bytes());
                b.extend(((p[1] / scale as i32) as i16).to_le_bytes());
            }
            out.push(msg(b'R', seq, &b));
            seq = seq.wrapping_add(1);
        }
        out.push(msg(b'E', seq, &[id, n[0], n[1]]));
        (out, seq.wrapping_add(1))
    }
}

#[cfg(test)]
mod tests {
    use super::enc::*;
    use super::*;
    use std::boxed::Box;

    fn active() -> State {
        State { flags: F_ACTIVE | F_FIX, maneuver: man::LEFT, dist_m: 350, x: -12, y: 40000, course: 16384, ..State::default() }
    }

    #[test]
    fn state_round_trips() {
        let mut m = Box::new(Model::new());
        let s = State { limit: 50, remaining_10m: 820, remaining_s: 840, after: man::RIGHT, after_m: 90, progress: 7, ..active() };
        assert_eq!(m.apply(&state(0, &s)), Effect::Changed);
        assert_eq!(m.state, s);
        assert!(m.has_state);
    }

    #[test]
    fn route_is_shown_only_when_complete() {
        let mut m = Box::new(Model::new());
        let pts: std::vec::Vec<[i32; 2]> = (0..100).map(|i| [i * 37 - 2000, i * i * 9]).collect();
        let (msgs, _) = route(0, 3, &pts);
        assert_eq!(msgs.len(), 1 + 4 + 1);
        for (i, msg) in msgs.iter().enumerate() {
            m.apply(msg);
            assert_eq!(m.route_len, if i + 1 == msgs.len() { 100 } else { 0 });
        }
        // max |v| is 89100 m, so scale 3: points come back within 3 m
        for (a, b) in m.route[..100].iter().zip(&pts) {
            assert!((a[0] - b[0]).abs() < 3 && (a[1] - b[1]).abs() < 3, "{a:?} {b:?}");
        }
    }

    #[test]
    fn a_lost_chunk_asks_again_and_shows_no_line() {
        let mut m = Box::new(Model::new());
        let pts: std::vec::Vec<[i32; 2]> = (0..90).map(|i| [i, -i]).collect();
        let (msgs, _) = route(10, 1, &pts);
        assert_eq!(m.apply(&msgs[0]), Effect::Changed);
        assert_eq!(m.apply(&msgs[1]), Effect::Ignored);
        assert_eq!(m.apply(&msgs[3]), Effect::Resync); // chunk 2 lost
        assert_eq!(m.apply(&msgs[4]), Effect::Resync);
        assert_eq!(m.route_len, 0);
    }

    #[test]
    fn seq_gap_and_wrap() {
        let mut m = Box::new(Model::new());
        assert_eq!(m.apply(&state(255, &active())), Effect::Changed);
        assert_eq!(m.apply(&state(0, &active())), Effect::Changed);
        assert_eq!(m.apply(&state(2, &active())), Effect::Resync);
        m.reset_seq();
        assert_eq!(m.apply(&state(9, &active())), Effect::Changed);
    }

    #[test]
    fn synced_only_by_a_snapshot_end() {
        let mut m = Box::new(Model::new());
        m.apply(&state(0, &active()));
        assert!(!m.synced);
        m.apply(&state(1, &State { flags: active().flags | F_SNAPSHOT, ..active() }));
        assert!(m.synced);
        m.apply(&state(5, &active())); // gap
        assert!(!m.synced);
    }

    #[test]
    fn ending_the_drive_drops_the_line() {
        let mut m = Box::new(Model::new());
        let (msgs, seq) = route(0, 1, &[[0, 0], [10, 10]]);
        msgs.iter().for_each(|x| {
            m.apply(x);
        });
        assert_eq!(m.route_len, 2);
        m.apply(&state(seq, &State::default()));
        assert_eq!(m.route_len, 0);
    }

    #[test]
    fn junk_is_ignored() {
        let mut m = Box::new(Model::new());
        for junk in [&b""[..], b"n", b"x1S\x00", b"n1S\x00\x01", b"n1Z\x00abc", b"n1B\x00\x01\xff\xff\x01", b"n1B\x00\x01\x02\x00\x00"] {
            m.reset_seq();
            assert_eq!(m.apply(junk), Effect::Ignored, "{junk:?}");
        }
        assert!(!m.has_state);
    }

    #[test]
    fn text_is_cut_on_a_char_boundary() {
        let mut m = Box::new(Model::new());
        let mut long = std::vec::Vec::from(&b"Stra\xc3\x9fe "[..]);
        long.resize(99, b'a');
        long.extend_from_slice("ß".as_bytes()); // 2 bytes, crosses 100
        m.apply(&msg(b'T', 0, &long));
        assert_eq!(m.text().len(), 99);
        m.apply(&msg(b'T', 1, b"Main St\xff"));
        assert_eq!(m.text(), "Main St");
    }
}
