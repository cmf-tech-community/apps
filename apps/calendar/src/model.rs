//! What the app knows and when it asks: the last answer, the cache, polling with retry and back-off. No `Api`: `now` is `api.now()` ms, `wall` the watch's local time in minutes
//! since 1970-01-01 (see [`wall_min`]).

use companion_rpc::{self as rpc, Call, Event};

/// Events asked for (the week from today, at most this many; the RPC sends 32 at most).
pub const MAX: usize = 32;
/// Poll while running.
pub const PERIOD_MS: u32 = 300_000;
/// While the phone asks the user: look again soon, the grant may come any moment.
pub const ASK_AGAIN_MS: u32 = 10_000;
/// The cache blob: magic, fetched (i32 wall minute), count, then `len ‖ record` per event.
const MAGIC: &[u8; 2] = b"C1";
pub const BLOB_MAX: usize = 7 + MAX * (1 + Event::RECORD_MAX);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Status {
    /// No answer yet since start.
    Waiting,
    Ok,
    Ask,
    Denied,
    /// UNAVAILABLE, BAD_REQUEST and anything unknown: the phone answered without data.
    Unavailable,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Got {
    Nothing,
    Changed,
    /// New events: save the cache.
    Events,
}

pub struct Model {
    pub events: [Event; MAX],
    pub n: usize,
    /// When `events` came, wall minutes; `None` = nothing ever came.
    pub fetched: Option<i32>,
    pub status: Status,
    call: Call,
    ids: u8,
    pending: [Event; MAX],
    /// The records of the open answer that decoded.
    ok: u32,
}

/// Days since 1970-01-01 of a civil date (H. Hinnant's `days_from_civil`).
pub fn days(y: i32, m: u32, d: u32) -> i32 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i32 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i32 - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

pub fn wall_min(y: i32, m: u32, d: u32, hour: u32, minute: u32) -> i32 {
    days(y, m, d) * 1440 + (hour * 60 + minute) as i32
}

const EMPTY: Event = Event { day: 0, start: 0, duration: 0, flags: 0, color: 0, title: rpc::Text::EMPTY };

impl Model {
    pub const fn new() -> Self {
        Model {
            events: [EMPTY; MAX],
            n: 0,
            fetched: None,
            status: Status::Waiting,
            call: Call::new(PERIOD_MS),
            ids: 0,
            pending: [EMPTY; MAX],
            ok: 0,
        }
    }

    /// A request to send now, if one is due (the first one, or a retry that backs off while the phone is silent).
    pub fn poll(&mut self, now: u32) -> Option<rpc::Msg> {
        let id = self.call.due(now, &mut self.ids)?;
        self.ok = 0;
        Some(rpc::calendar_events(id, 0, crate::marks::DAYS as u8, MAX as u8))
    }

    /// Ms until [`Model::poll`] has something to do.
    pub fn wait(&self, now: u32) -> u32 {
        self.call.wait(now)
    }

    /// Ask at once (start, RESUME), with a fresh back-off.
    pub fn ask_now(&mut self, now: u32) {
        self.call.now(now);
    }

    /// One message from the channel. Replies to anything but the latest request are dropped.
    pub fn reply(&mut self, m: &[u8], now: u32, wall: i32) -> Got {
        match self.call.take(m) {
            rpc::Got::Nothing => Got::Nothing,
            rpc::Got::Status(s) => {
                self.status = match s {
                    rpc::OK => Status::Ok,
                    rpc::ASK => Status::Ask,
                    rpc::DENIED => Status::Denied,
                    _ => Status::Unavailable,
                };
                self.call.after(now, if self.status == Status::Ask { ASK_AGAIN_MS } else { PERIOD_MS });
                if self.status != Status::Ok {
                    return Got::Changed;
                }
                self.n = 0;
                self.fetched = Some(wall);
                Got::Events
            }
            // a multi-part answer: one record each, any order; one that doesn't decode is left out
            rpc::Got::Record { index, record, last, .. } => {
                if let (Some(e), true) = (Event::decode(record), (index as usize) < MAX) {
                    self.pending[index as usize] = e;
                    self.ok |= 1 << index;
                }
                if !last {
                    return Got::Nothing;
                }
                self.n = 0;
                for i in 0..MAX {
                    if self.ok & 1 << i != 0 {
                        self.events[self.n] = self.pending[i];
                        self.n += 1;
                    }
                }
                self.fetched = Some(wall);
                self.status = Status::Ok;
                self.call.after(now, PERIOD_MS);
                Got::Events
            }
        }
    }

    /// The cache blob into `out` ([`BLOB_MAX`] bytes); its length, 0 if there is nothing to keep.
    pub fn save(&self, out: &mut [u8]) -> usize {
        let Some(f) = self.fetched else { return 0 };
        out[..2].copy_from_slice(MAGIC);
        out[2..6].copy_from_slice(&f.to_le_bytes());
        out[6] = self.n as u8;
        let mut at = 7;
        for e in &self.events[..self.n] {
            let len = e.encode(&mut out[at + 1..]);
            out[at] = len as u8;
            at += 1 + len;
        }
        at
    }

    /// Restores a cache blob; anything it cannot read leaves the model empty.
    pub fn load(&mut self, b: &[u8]) {
        if b.len() < 7 || &b[..2] != MAGIC || b[6] as usize > MAX {
            return;
        }
        let mut at = 7;
        for i in 0..b[6] as usize {
            let Some(&len) = b.get(at) else { return };
            let Some(e) = b.get(at + 1..at + 1 + len as usize).and_then(Event::decode) else { return };
            self.pending[i] = e;
            at += 1 + len as usize;
        }
        self.n = b[6] as usize;
        self.events[..self.n].copy_from_slice(&self.pending[..self.n]);
        self.fetched = Some(i32::from_le_bytes([b[2], b[3], b[4], b[5]]));
    }

    /// Event `i` relative to today 00:00 at `wall`: (day, start minute, end minute). The phone's `day` counts from
    /// the day of the answer, so a cache from yesterday moves one day back.
    pub fn span(&self, i: usize, wall: i32) -> (i32, i32, i32) {
        let e = &self.events[i];
        let shift = wall.div_euclid(1440) - self.fetched.unwrap_or(wall).div_euclid(1440);
        let day = e.day as i32 - shift;
        let start = day * 1440 + e.start as i32;
        (day, start, start + e.duration as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use companion_rpc::RETRY_MS;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    // 2026-11-17 08:00
    const WALL: i32 = 20_774 * 1440 + 8 * 60;

    fn rec(day: i8, start: u16, dur: u16, flags: u8, title: &str) -> Vec<u8> {
        let mut r = vec![day as u8];
        r.extend(start.to_le_bytes());
        r.extend(dur.to_le_bytes());
        r.push(flags);
        r.extend(0xF800u16.to_le_bytes());
        r.extend(title.as_bytes());
        r
    }

    fn msg(id: u8, status: u8, index: u8, total: u8, record: &[u8]) -> Vec<u8> {
        let mut m = vec![b'R', b'1', b'A', id, status, index, total];
        m.extend(record);
        m
    }

    #[test]
    fn days_since_epoch() {
        assert_eq!(days(1970, 1, 1), 0);
        assert_eq!(days(2026, 11, 17), 20_774);
        assert_eq!(days(2024, 3, 1) - days(2024, 2, 28), 2);
    }

    #[test]
    fn multi_part_answer_in_any_order_then_cached() {
        let mut m = Model::new();
        assert!(m.poll(0).is_none()); // nothing before the start's ask_now
        m.ask_now(0);
        let req = m.poll(0).unwrap();
        assert_eq!(&req[..], [b'R', b'1', b'Q', 1, 0x10, 0, 7, 32]);
        assert_eq!(m.reply(&msg(1, 0, 1, 2, &rec(0, 600, 30, 0, "Обед")), 100, WALL), Got::Nothing);
        assert_eq!(m.reply(&msg(1, 0, 0, 2, &rec(0, 570, 30, 4, "")), 100, WALL), Got::Events);
        assert_eq!((m.n, m.status, m.events[1].title.as_str(), m.events[0].title.as_str()), (2, Status::Ok, "Обед", ""));
        assert!(m.poll(PERIOD_MS + 99).is_none() && m.poll(PERIOD_MS + 100).is_some());

        let mut blob = [0u8; BLOB_MAX];
        let n = m.save(&mut blob);
        let mut back = Model::new();
        back.load(&blob[..n]);
        assert_eq!((back.n, back.fetched, back.events), (2, Some(WALL), m.events));
        let mut junk = Model::new();
        junk.load(&blob[..n - 1]);
        junk.load(b"C2\0\0\0\0\0");
        assert_eq!((junk.n, junk.fetched), (0, None));
    }

    #[test]
    fn spec_reply_vector_with_a_hidden_title() {
        // rpc-vectors.json "replies", id 8: one record, title hidden
        let mut m = Model::new();
        for _ in 0..7 {
            m.ask_now(0);
            m.poll(0);
        }
        m.ask_now(0);
        assert_eq!(m.poll(0).unwrap()[3], 8);
        assert_eq!(m.reply(&hex("52314108000001003a021e000400f8"), 0, WALL), Got::Events);
        assert_eq!((m.n, m.events[0].title.as_str()), (1, ""));
    }

    #[test]
    fn stale_ids_dropped_silence_backs_off() {
        let mut m = Model::new();
        m.ask_now(0);
        m.poll(0);
        assert!(m.poll(RETRY_MS - 1).is_none());
        assert_eq!(m.poll(RETRY_MS).unwrap()[3], 2);
        assert!(m.call.silent());
        assert_eq!(m.wait(RETRY_MS), 2 * RETRY_MS);
        // the late answer to request 1 is not the latest
        assert_eq!(m.reply(&msg(1, 0, 0, 0, &[]), RETRY_MS + 5, WALL), Got::Nothing);
        assert!(m.poll(3 * RETRY_MS).is_some());
        assert_eq!(m.wait(3 * RETRY_MS), 4 * RETRY_MS);
        let mut t = 3 * RETRY_MS;
        for _ in 0..10 {
            t += m.wait(t);
            m.poll(t);
        }
        assert_eq!(m.wait(t), PERIOD_MS);
        // ASK for the latest id, look again in 10 s, the silence is over
        let id = m.ids;
        assert_eq!(m.reply(&msg(id, rpc::ASK, 0, 0, &[]), 7, WALL), Got::Changed);
        assert_eq!((m.status, m.call.silent(), m.wait(7)), (Status::Ask, false, ASK_AGAIN_MS));
        // a second answer to the same id is ignored
        assert_eq!(m.reply(&msg(id, rpc::DENIED, 0, 0, &[]), 8, WALL), Got::Nothing);
    }
}
