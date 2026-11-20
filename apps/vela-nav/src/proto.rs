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
//! | `B` | id u8, count u16, scale u8 (m per unit), frame u16 (optional) | a new route line begins; it replaces the shown one at its `E` |
//! | `R` | id u8, start u16, n u8, n × (x i16, y i16) | route points `start..start+n`, n ≤ 30 |
//! | `E` | id u8, count u16 | the route line is complete |
//! | `G` | id u8, x i32 m, y i32 m | a new set of streets around (x, y) begins |
//! | `H` | id u8, then records: n u8 (2..=30), class u8 (0 minor, 1 major), n × (x i16, y i16) m from the set's (x, y) | streets |
//! | `J` | id u8, lines u16, points u16 | the set is complete: it replaces the one shown |
//! | `P` | flags u8 (1 Home set, 2 Work set), status u8 (0 idle, 1 starting a drive, 2 could not plan it) | the shortcuts the watch may offer: whether each is set, never where |
//!
//! Streets are drawn under the route, fading out away from it. They share the route's frame: a route in another
//! frame (its `frame` tag differs, or has none) drops them when it is complete, and the phone sends a set for the new
//! frame after it. The same route sent again (a snapshot) keeps them.
//!
//! Watch → phone: `n1Q`, "send me everything" (on start, after a gap, after `RESUME`, and while nothing arrives);
//! `n1A` + 1 or 2, "start a drive to Home / Work" (a button on the screen shown without a drive).
//!
//! Positions are metres east (`x`) and north (`y`) of an origin the phone picks per drive, never latitude and
//! longitude: the channel can be read by anyone in Bluetooth range. `course` is the heading in 1/65536 turns
//! clockwise from north. `progress` is the index of the first route point still ahead of the car.

pub const ASK: [u8; 3] = *b"n1Q";
pub const P_HOME: u8 = 1;
pub const P_WORK: u8 = 2;
pub const START_BUSY: u8 = 1;
pub const START_FAILED: u8 = 2;

/// "Start a drive to Home / Work" (`P_HOME`, `P_WORK`).
pub fn action(kind: u8) -> [u8; 4] {
    [b'n', b'1', b'A', kind]
}
pub const ROUTE_MAX: usize = 512;
pub const TEXT_MAX: usize = 100;
pub const CHUNK_MAX: usize = 30;
/// Street points and lines one set holds; the phone sends the nearest streets first and stops below these.
pub const ROADS_PTS: usize = 1200;
pub const ROADS_LINES: usize = 240;
pub const ROAD_LINE_MAX: usize = 30;

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

/// One street line: `n` points from `first` in `Roads::pts`.
#[derive(Clone, Copy, Default)]
pub struct RoadLine {
    pub first: u16,
    pub n: u16,
    pub major: bool,
}

/// A set of streets: points in metres from `origin`, which is in the route's frame.
pub struct Roads {
    pub origin: [i32; 2],
    pub pts: [[i16; 2]; ROADS_PTS],
    pub lines: [RoadLine; ROADS_LINES],
    pub n_pts: usize,
    pub n_lines: usize,
}

impl Roads {
    pub const fn new() -> Roads {
        Roads {
            origin: [0; 2],
            pts: [[0; 2]; ROADS_PTS],
            lines: [RoadLine { first: 0, n: 0, major: false }; ROADS_LINES],
            n_pts: 0,
            n_lines: 0,
        }
    }

    /// A line's points in the route's frame.
    pub fn line(&self, l: &RoadLine) -> impl Iterator<Item = [i32; 2]> + '_ {
        self.pts[l.first as usize..(l.first + l.n) as usize]
            .iter()
            .map(|p| [self.origin[0] + p[0] as i32, self.origin[1] + p[1] as i32])
    }
}

impl Default for Roads {
    fn default() -> Self {
        Roads::new()
    }
}

pub struct Model {
    pub state: State,
    /// A state arrived since the last `reset` (the screen shows "waiting" until then).
    pub has_state: bool,
    /// The shortcuts the phone has (`P_HOME`, `P_WORK` bits) and what came of the last start (`START_*`).
    pub places: u8,
    pub start: u8,
    /// A state with `F_SNAPSHOT` arrived after the last gap: the watch has everything.
    pub synced: bool,
    text: [u8; TEXT_MAX],
    text_len: usize,
    pub route: [[i32; 2]; ROUTE_MAX],
    /// Points of the complete route line shown; 0 while none. A new one arrives in `incoming` beside it.
    pub route_len: usize,
    incoming: [[i32; 2]; ROUTE_MAX],
    /// The frame of the line shown, and of the one arriving.
    frame: Option<u16>,
    incoming_frame: Option<u16>,
    route_id: Option<u8>,
    route_count: usize,
    route_got: usize,
    route_scale: i32,
    /// The set shown is `roads[live]`; the next one arrives in the other.
    roads: [Roads; 2],
    live: usize,
    roads_id: Option<u8>,
    /// Lines of the set arriving, kept or not.
    roads_seen: usize,
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
            places: 0,
            start: 0,
            synced: false,
            text: [0; TEXT_MAX],
            text_len: 0,
            route: [[0; 2]; ROUTE_MAX],
            route_len: 0,
            incoming: [[0; 2]; ROUTE_MAX],
            frame: None,
            incoming_frame: None,
            route_id: None,
            route_count: 0,
            route_got: 0,
            route_scale: 1,
            roads: [Roads::new(), Roads::new()],
            live: 0,
            roads_id: None,
            roads_seen: 0,
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

    /// The streets shown.
    pub fn roads(&self) -> &Roads {
        &self.roads[self.live]
    }

    #[cfg(test)]
    pub fn drop_roads_for_test(&mut self) {
        self.drop_roads()
    }

    fn drop_roads(&mut self) {
        self.roads[self.live].n_lines = 0;
        self.roads_id = None;
    }

    /// Length of the route line in metres; 0 while there is none.
    pub fn length_m(&self) -> i64 {
        self.route[..self.route_len]
            .windows(2)
            .map(|w| {
                let (dx, dy) = ((w[1][0] - w[0][0]) as i64, (w[1][1] - w[0][1]) as i64);
                isqrt((dx * dx + dy * dy) as u64) as i64
            })
            .sum()
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
            b'G' => self.on_roads_begin(body),
            b'H' => self.on_roads(body),
            b'J' => self.on_roads_end(body),
            b'P' => self.on_places(body),
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
            self.frame = None;
            self.drop_roads();
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
        self.incoming_frame = if b.len() >= 6 { Some(u16_at(b, 4)) } else { None };
        // the line shown stays until this one is complete
        Some(false)
    }

    fn on_points(&mut self, b: &[u8]) -> Option<bool> {
        if b.len() < 4 {
            return Some(false);
        }
        let (id, start, n) = (b[0], u16_at(b, 1) as usize, b[3] as usize);
        if self.route_id != Some(id) {
            return None; // a chunk of a line whose begin we missed: ask again
        }
        if b.len() < 4 + 4 * n || n > CHUNK_MAX || start != self.route_got || start + n > self.route_count {
            self.route_id = None;
            return None;
        }
        for i in 0..n {
            let at = 4 + 4 * i;
            self.incoming[start + i] =
                [i16_at(b, at) as i32 * self.route_scale, i16_at(b, at + 2) as i32 * self.route_scale];
        }
        self.route_got += n;
        Some(false) // nothing to show until the end
    }

    fn on_end(&mut self, b: &[u8]) -> Option<bool> {
        if b.len() < 3 {
            return Some(false);
        }
        if self.route_id != Some(b[0]) || u16_at(b, 1) as usize != self.route_got || self.route_got != self.route_count
        {
            self.route_id = None;
            return None;
        }
        let n = self.route_got;
        self.route[..n].copy_from_slice(&self.incoming[..n]);
        self.route_len = n;
        self.route_id = None;
        if self.incoming_frame.is_none() || self.incoming_frame != self.frame {
            self.drop_roads(); // another frame: the streets shown are in the old one
        }
        self.frame = self.incoming_frame;
        Some(true)
    }
}

impl Model {
    fn on_places(&mut self, b: &[u8]) -> Option<bool> {
        let [flags, start, ..] = *b else { return Some(false) };
        (self.places, self.start) = (flags & (P_HOME | P_WORK), start);
        Some(true)
    }

    fn on_roads_begin(&mut self, b: &[u8]) -> Option<bool> {
        if b.len() < 9 {
            return Some(false);
        }
        let next = &mut self.roads[1 - self.live];
        (next.origin, next.n_pts, next.n_lines) = ([i32_at(b, 1), i32_at(b, 5)], 0, 0);
        (self.roads_id, self.roads_seen) = (Some(b[0]), 0);
        Some(false)
    }

    fn on_roads(&mut self, b: &[u8]) -> Option<bool> {
        if b.is_empty() || self.roads_id != Some(b[0]) {
            return Some(false); // a set whose begin we missed: the seq gap asks for a snapshot
        }
        let next = &mut self.roads[1 - self.live];
        let mut at = 1;
        while at + 2 <= b.len() {
            let (n, major) = (b[at] as usize, b[at + 1] == 1);
            if !(2..=ROAD_LINE_MAX).contains(&n) || at + 2 + 4 * n > b.len() {
                self.roads_id = None; // malformed: drop the set
                return Some(false);
            }
            // a set bigger than the watch holds keeps what fits, the nearest streets
            if next.n_lines < ROADS_LINES && next.n_pts + n <= ROADS_PTS {
                next.lines[next.n_lines] = RoadLine { first: next.n_pts as u16, n: n as u16, major };
                for i in 0..n {
                    let p = at + 2 + 4 * i;
                    next.pts[next.n_pts + i] = [i16_at(b, p), i16_at(b, p + 2)];
                }
                next.n_lines += 1;
                next.n_pts += n;
            }
            self.roads_seen += 1;
            at += 2 + 4 * n;
        }
        Some(false)
    }

    fn on_roads_end(&mut self, b: &[u8]) -> Option<bool> {
        if b.len() < 5 || self.roads_id != Some(b[0]) {
            return Some(false);
        }
        self.roads_id = None;
        // a set cut to what fits is still shown; a short one (a lost message) is not
        if u16_at(b, 1) as usize != self.roads_seen || self.route_len == 0 {
            return Some(false);
        }
        self.live = 1 - self.live;
        Some(true)
    }
}

impl Default for Model {
    fn default() -> Self {
        Model::new()
    }
}

fn isqrt(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    let mut x = 1u64 << ((64 - n.leading_zeros()).div_ceil(2));
    loop {
        let y = (x + n / x) / 2;
        if y >= x {
            return x;
        }
        x = y;
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
    pub fn route(seq: u8, id: u8, pts: &[[i32; 2]]) -> (Vec<Vec<u8>>, u8) {
        route_in(seq, id, pts, None)
    }

    /// [route] tagged with the frame it is in.
    pub fn route_in(mut seq: u8, id: u8, pts: &[[i32; 2]], frame: Option<u16>) -> (Vec<Vec<u8>>, u8) {
        let max = pts.iter().flat_map(|p| p.iter()).map(|v| v.abs()).max().unwrap_or(0);
        let scale = (max / i16::MAX as i32 + 1) as u8;
        let mut out = Vec::new();
        let n = (pts.len() as u16).to_le_bytes();
        let mut b = vec![id, n[0], n[1], scale];
        if let Some(f) = frame {
            b.extend(f.to_le_bytes());
        }
        out.push(msg(b'B', seq, &b));
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

    /// A street set around `origin`: begin, one message of whole lines each, end.
    pub fn roads(mut seq: u8, id: u8, origin: [i32; 2], lines: &[(bool, Vec<[i32; 2]>)]) -> (Vec<Vec<u8>>, u8) {
        let mut out = Vec::new();
        let mut b = vec![id];
        b.extend(origin[0].to_le_bytes());
        b.extend(origin[1].to_le_bytes());
        out.push(msg(b'G', seq, &b));
        seq = seq.wrapping_add(1);
        let mut pts = 0u16;
        for (major, l) in lines {
            let mut b = vec![id, l.len() as u8, *major as u8];
            for p in l {
                b.extend(((p[0] - origin[0]) as i16).to_le_bytes());
                b.extend(((p[1] - origin[1]) as i16).to_le_bytes());
            }
            pts += l.len() as u16;
            out.push(msg(b'H', seq, &b));
            seq = seq.wrapping_add(1);
        }
        let (n, p) = ((lines.len() as u16).to_le_bytes(), pts.to_le_bytes());
        out.push(msg(b'J', seq, &[id, n[0], n[1], p[0], p[1]]));
        (out, seq.wrapping_add(1))
    }
}

#[cfg(test)]
mod tests {
    use super::enc::*;
    use super::*;
    use std::boxed::Box;

    fn active() -> State {
        State {
            flags: F_ACTIVE | F_FIX,
            maneuver: man::LEFT,
            dist_m: 350,
            x: -12,
            y: 40000,
            course: 16384,
            ..State::default()
        }
    }

    #[test]
    fn state_round_trips() {
        let mut m = Box::new(Model::new());
        let s = State {
            limit: 50,
            remaining_10m: 820,
            remaining_s: 840,
            after: man::RIGHT,
            after_m: 90,
            progress: 7,
            ..active()
        };
        assert_eq!(m.apply(&state(0, &s)), Effect::Changed);
        assert_eq!(m.state, s);
        assert!(m.has_state);
    }

    fn drive_with_route(m: &mut Model) -> u8 {
        let pts: Vec<[i32; 2]> = (0..10).map(|i| [0, i * 50]).collect();
        let (msgs, seq) = route(0, 1, &pts);
        msgs.iter().for_each(|x| {
            m.apply(x);
        });
        m.apply(&state(seq, &active()));
        seq.wrapping_add(1)
    }

    #[test]
    fn streets_swap_in_whole_and_follow_the_route() {
        let mut m = Box::new(Model::new());
        let seq = drive_with_route(&mut m);
        let lines = vec![(true, vec![[100, 100], [300, 100]]), (false, vec![[100, 0], [100, 400], [150, 450]])];
        let (msgs, seq) = roads(seq, 7, [100, 200], &lines);
        let (last, rest) = msgs.split_last().unwrap();
        rest.iter().for_each(|x| {
            assert_eq!(m.apply(x), Effect::Ignored);
        });
        assert_eq!(m.roads().n_lines, 0, "nothing shows until the end");
        assert_eq!(m.apply(last), Effect::Changed);
        let r = m.roads();
        assert_eq!((r.n_lines, r.n_pts), (2, 5));
        assert!(r.lines[0].major && !r.lines[1].major);
        assert_eq!(r.line(&r.lines[1]).collect::<Vec<_>>(), vec![[100, 0], [100, 400], [150, 450]]);
        // the next set arrives beside the shown one
        let (msgs, seq) = roads(seq, 8, [0, 0], &lines[..1]);
        m.apply(&msgs[0]);
        assert_eq!(m.roads().n_lines, 2);
        msgs[1..].iter().for_each(|x| {
            m.apply(x);
        });
        assert_eq!(m.roads().n_lines, 1);
        // the same route sent again in the same frame keeps them; a route in another frame drops them, once whole
        let (msgs, seq) = route_in(seq, 2, &[[0, 0], [0, 10]], Some(5));
        msgs.iter().for_each(|x| {
            m.apply(x);
        });
        assert_eq!(m.roads().n_lines, 0, "the first route had no frame tag");
        let (msgs, seq) = roads(seq, 9, [0, 0], &lines[..1]);
        msgs.iter().for_each(|x| {
            m.apply(x);
        });
        let (msgs, seq) = route_in(seq, 3, &[[0, 0], [0, 10]], Some(5));
        msgs.iter().for_each(|x| {
            m.apply(x);
        });
        assert_eq!(m.roads().n_lines, 1, "same frame");
        let (msgs, _) = route_in(seq, 4, &[[0, 0], [0, 10]], Some(6));
        m.apply(&msgs[0]);
        assert_eq!(m.roads().n_lines, 1, "kept until the new line is whole");
        msgs[1..].iter().for_each(|x| {
            m.apply(x);
        });
        assert_eq!(m.roads().n_lines, 0, "another frame");
    }

    #[test]
    fn a_short_street_set_is_dropped() {
        let mut m = Box::new(Model::new());
        let seq = drive_with_route(&mut m);
        let lines = vec![(true, vec![[0, 0], [10, 0]]), (false, vec![[0, 0], [0, 10]])];
        let (msgs, _) = roads(seq, 3, [0, 0], &lines);
        m.apply(&msgs[0]);
        m.apply(&msgs[1]);
        // msgs[2] lost: the end's seq jumps, so the watch asks again, and the set is not shown
        assert_eq!(m.apply(&msgs[3]), Effect::Resync);
        assert_eq!(m.roads().n_lines, 0);
        // garbage never panics
        m.apply(&msg(b'H', 0, &[3, 31, 0]));
        m.apply(&msg(b'G', 1, &[1]));
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
            assert_eq!(m.apply(&state(0, &active())), Effect::Resync); // (seq aside) a state between keeps it
            m.reset_seq();
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
        assert_eq!(m.apply(&msgs[0]), Effect::Ignored);
        assert_eq!(m.apply(&msgs[1]), Effect::Ignored);
        assert_eq!(m.apply(&msgs[3]), Effect::Resync); // chunk 2 lost
        assert_eq!(m.apply(&msgs[4]), Effect::Resync);
        assert_eq!(m.route_len, 0);
    }

    #[test]
    fn a_new_line_replaces_the_shown_one_only_when_whole() {
        let mut m = Box::new(Model::new());
        let a: Vec<[i32; 2]> = (0..40).map(|i| [0, i * 10]).collect();
        let b: Vec<[i32; 2]> = (0..40).map(|i| [i * 10, 0]).collect();
        let (msgs, seq) = route_in(0, 1, &a, Some(7));
        msgs.iter().for_each(|x| {
            m.apply(x);
        });
        assert_eq!((m.route_len, m.route[39]), (40, [0, 390]));
        let (msgs, _) = route_in(seq, 2, &b, Some(7));
        for (i, x) in msgs.iter().enumerate() {
            m.apply(x);
            // the old line shows while the new one arrives: never an empty map
            let want = if i + 1 == msgs.len() { [390, 0] } else { [0, 390] };
            assert_eq!((m.route_len, m.route[39]), (40, want));
        }
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
        for junk in [
            &b""[..],
            b"n",
            b"x1S\x00",
            b"n1S\x00\x01",
            b"n1Z\x00abc",
            b"n1B\x00\x01\xff\xff\x01",
            b"n1B\x00\x01\x02\x00\x00",
        ] {
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
