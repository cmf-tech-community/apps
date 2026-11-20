//! What the app knows and when it asks, without `Api`: the last list from the phone, the reminders ticked off on the
//! watch that the phone hasn't confirmed yet, the cache blob, and where each reminder goes on the screen.
//!
//! Two calls to the phone side by side, one id counter: `reminders` every few minutes, and `reminders.complete` for
//! each ticked reminder, oldest first, until the phone answers. `now` is `api.now()` ms; `wall` the watch's local time
//! in minutes since 1970-01-01 ([`wall_min`]).

use companion_rpc::{self as rpc, Call, Reminder};
use core::fmt::Write;

/// Reminders asked for, the most the phone sends.
pub const MAX: usize = 20;
/// Ask again while the app is open.
pub const PERIOD_MS: u32 = 180_000;
/// While the phone asks the user: look again soon, the grant may come any moment.
pub const ASK_AGAIN_MS: u32 = 10_000;
/// A completion the phone couldn't do now (`UNAVAILABLE`): try again after this.
pub const LATER_MS: u32 = 30_000;
/// Ticked reminders the phone hasn't confirmed, kept in the cache.
pub const DONE_MAX: usize = 8;
/// The cache blob: magic, fetched (i32 wall minute), count, `len ‖ record` per reminder, then the done queue.
const MAGIC: &[u8; 2] = b"M1";
pub const BLOB_MAX: usize = 7 + MAX * (1 + Reminder::RECORD_MAX) + 1 + DONE_MAX * 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// No answer since start.
    Waiting,
    Ok,
    /// The phone asked the user to allow Reminders for this app.
    Ask,
    /// The user said no, or iOS's Reminders access is off.
    Denied,
    /// The phone can't: an Android phone (no system reminders there), or anything else it refused.
    Unavailable,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Got {
    Nothing,
    /// The screen changes.
    Changed,
    /// The screen changes and the cache too: save it.
    Saved,
}

/// Where a reminder goes, in screen order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum When {
    /// Due before today, or today at a time that has passed.
    Overdue,
    Today,
    /// Days from today, 1 or more.
    Later(i32),
    NoDate,
}

pub struct Model {
    /// In the phone's order: due ones by day (untimed first, then by minute), then the rest.
    pub list: [Reminder; MAX],
    pub n: usize,
    /// When `list` came, wall minutes; `None` = nothing ever came.
    pub fetched: Option<i32>,
    pub status: Status,
    done: [u32; DONE_MAX],
    done_n: usize,
    list_call: Call,
    done_call: Call,
    ids: u8,
    pending: [Reminder; MAX],
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

/// The civil date of a day number: (year, month 1-12, day).
pub fn civil(z: i32) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + (m <= 2) as i32, m, d)
}

pub fn wall_min(y: i32, m: u32, d: u32, hour: u32, minute: u32) -> i32 {
    days(y, m, d) * 1440 + (hour * 60 + minute) as i32
}

const EMPTY: Reminder = Reminder { id: 0, day: 0, minute: 0, flags: 0, color: 0, title: rpc::Text::EMPTY };

impl Model {
    pub const fn new() -> Self {
        Model {
            list: [EMPTY; MAX],
            n: 0,
            fetched: None,
            status: Status::Waiting,
            done: [0; DONE_MAX],
            done_n: 0,
            list_call: Call::new(PERIOD_MS),
            done_call: Call::new(PERIOD_MS),
            ids: 0,
            pending: [EMPTY; MAX],
            ok: 0,
        }
    }

    /// Ask for the list at once (start, `RESUME`), and send what is still to be completed.
    pub fn ask_now(&mut self, now: u32) {
        self.list_call.now(now);
        if self.done_n > 0 {
            self.done_call.now(now);
        }
    }

    /// A request to send now, if one is due: completions first.
    pub fn poll(&mut self, now: u32) -> Option<rpc::Msg> {
        if self.done_n > 0 {
            if let Some(id) = self.done_call.due(now, &mut self.ids) {
                return Some(rpc::reminders_complete(id, self.done[0]));
            }
        }
        let id = self.list_call.due(now, &mut self.ids)?;
        self.ok = 0;
        Some(rpc::reminders(id, MAX as u8))
    }

    /// Ms until [`Model::poll`] has something to do.
    pub fn wait(&self, now: u32) -> u32 {
        self.list_call.wait(now).min(self.done_call.wait(now))
    }

    /// The phone left the last request for the list unanswered.
    pub fn silent(&self) -> bool {
        self.list_call.silent()
    }

    /// Ticks reminder `i` off: it leaves the list now and the phone is told (again after a restart, from the
    /// cache, until it answers). `false` if the queue is full.
    pub fn complete(&mut self, i: usize, now: u32) -> bool {
        if i >= self.n || self.done_n == DONE_MAX {
            return false;
        }
        self.done[self.done_n] = self.list[i].id;
        self.done_n += 1;
        if self.done_n == 1 {
            self.done_call.now(now);
        }
        self.list.copy_within(i + 1..self.n, i);
        self.n -= 1;
        true
    }

    fn waiting_done(&self, id: u32) -> bool {
        self.done[..self.done_n].contains(&id)
    }

    /// One message from the channel.
    pub fn reply(&mut self, m: &[u8], now: u32, wall: i32) -> Got {
        match self.done_call.take(m) {
            // `reminders.complete` has no records: a reply with one is not the phone's
            rpc::Got::Nothing | rpc::Got::Record { .. } => {}
            rpc::Got::Status(rpc::UNAVAILABLE) => {
                self.done_call.after(now, LATER_MS);
                return Got::Nothing;
            }
            // OK, BAD_REQUEST (done meanwhile, or deleted), DENIED, ASK: settled either way; the list says the truth
            rpc::Got::Status(_) if self.done_n > 0 => {
                self.done.copy_within(1..self.done_n, 0);
                self.done_n -= 1;
                if self.done_n > 0 {
                    self.done_call.now(now);
                } else {
                    self.done_call.stop();
                }
                self.list_call.now(now);
                return Got::Saved;
            }
            rpc::Got::Status(_) => {}
        }
        match self.list_call.take(m) {
            rpc::Got::Nothing => Got::Nothing,
            rpc::Got::Status(s) => {
                self.status = match s {
                    rpc::OK => Status::Ok,
                    rpc::ASK => Status::Ask,
                    rpc::DENIED => Status::Denied,
                    _ => Status::Unavailable,
                };
                self.list_call.after(now, if self.status == Status::Ask { ASK_AGAIN_MS } else { PERIOD_MS });
                if self.status != Status::Ok {
                    return Got::Changed;
                }
                self.n = 0;
                self.fetched = Some(wall);
                Got::Saved
            }
            // one record each, any order; one that doesn't decode is left out
            rpc::Got::Record { index, record, last, .. } => {
                if let (Some(r), true) = (Reminder::decode(record), (index as usize) < MAX) {
                    self.pending[index as usize] = r;
                    self.ok |= 1 << index;
                }
                if !last {
                    return Got::Nothing;
                }
                self.n = 0;
                for i in 0..MAX {
                    if self.ok & 1 << i != 0 && !self.waiting_done(self.pending[i].id) {
                        self.list[self.n] = self.pending[i];
                        self.n += 1;
                    }
                }
                self.fetched = Some(wall);
                self.status = Status::Ok;
                self.list_call.after(now, PERIOD_MS);
                Got::Saved
            }
        }
    }

    /// Reminder `i`'s due day in days from today at `wall`: the phone's `day` counts from the day of the answer, so
    /// a cache from yesterday moves one day closer. `None` without a due date.
    pub fn day(&self, i: usize, wall: i32) -> Option<i32> {
        let r = &self.list[i];
        let shift = wall.div_euclid(1440) - self.fetched.unwrap_or(wall).div_euclid(1440);
        r.due().then(|| r.day as i32 - shift)
    }

    pub fn when(&self, i: usize, wall: i32) -> When {
        let r = &self.list[i];
        match self.day(i, wall) {
            None => When::NoDate,
            Some(d) if d < 0 => When::Overdue,
            Some(0) if r.timed() && (r.minute as i32) < wall.rem_euclid(1440) => When::Overdue,
            Some(0) => When::Today,
            Some(d) => When::Later(d),
        }
    }

    /// The screen's order: the overdue ones, then the rest, each in the phone's order. Indexes into `list`.
    pub fn order(&self, wall: i32) -> ([u8; MAX], usize) {
        let mut out = [0u8; MAX];
        let mut k = 0;
        for overdue in [true, false] {
            for i in 0..self.n {
                if (self.when(i, wall) == When::Overdue) == overdue {
                    out[k] = i as u8;
                    k += 1;
                }
            }
        }
        (out, k)
    }

    /// Overdue and due today: the card's count.
    pub fn due_now(&self, wall: i32) -> usize {
        (0..self.n).filter(|&i| matches!(self.when(i, wall), When::Overdue | When::Today)).count()
    }

    /// The cache blob into `out` ([`BLOB_MAX`] bytes); its length, 0 if there is nothing to keep.
    pub fn save(&self, out: &mut [u8]) -> usize {
        let Some(f) = self.fetched else { return 0 };
        out[..2].copy_from_slice(MAGIC);
        out[2..6].copy_from_slice(&f.to_le_bytes());
        out[6] = self.n as u8;
        let mut at = 7;
        for r in &self.list[..self.n] {
            let len = r.encode(&mut out[at + 1..]);
            out[at] = len as u8;
            at += 1 + len;
        }
        out[at] = self.done_n as u8;
        at += 1;
        for id in &self.done[..self.done_n] {
            out[at..at + 4].copy_from_slice(&id.to_le_bytes());
            at += 4;
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
            let Some(r) = b.get(at + 1..at + 1 + len as usize).and_then(Reminder::decode) else { return };
            self.pending[i] = r;
            at += 1 + len as usize;
        }
        let Some(&dn) = b.get(at) else { return };
        let Some(ids) = b.get(at + 1..at + 1 + 4 * dn as usize).filter(|_| dn as usize <= DONE_MAX) else { return };
        for (k, c) in ids.chunks(4).enumerate() {
            self.done[k] = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        }
        self.done_n = dn as usize;
        self.n = b[6] as usize;
        self.list[..self.n].copy_from_slice(&self.pending[..self.n]);
        self.fetched = Some(i32::from_le_bytes([b[2], b[3], b[4], b[5]]));
    }
}

/// A short line of text in a fixed buffer; writes that don't fit are dropped whole.
pub struct Line {
    buf: [u8; 64],
    len: usize,
}

impl Line {
    pub const fn new() -> Line {
        Line { buf: [0; 64], len: 0 }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl Write for Line {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        if self.len + s.len() > self.buf.len() {
            return Err(core::fmt::Error);
        }
        self.buf[self.len..self.len + s.len()].copy_from_slice(s.as_bytes());
        self.len += s.len();
        Ok(())
    }
}

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// "Today", "Tomorrow", "Yesterday", else "Wed 25 Nov": `d` days from the day of `wall`.
pub fn day_label(d: i32, wall: i32, out: &mut Line) {
    let _ = match d {
        0 => out.write_str("Today"),
        1 => out.write_str("Tomorrow"),
        -1 => out.write_str("Yesterday"),
        _ => {
            let z = wall.div_euclid(1440) + d;
            let (_, m, day) = civil(z);
            write!(out, "{} {} {}", WEEKDAYS[(z + 3).rem_euclid(7) as usize], day, MONTHS[m as usize - 1])
        }
    };
}

/// "18:05".
pub fn time_label(minute: u16, out: &mut Line) {
    let _ = write!(out, "{:02}:{:02}", minute / 60 % 24, minute % 60);
}

/// The header of a section.
pub fn section_label(w: When, wall: i32) -> Line {
    let mut out = Line::new();
    match w {
        When::Overdue => {
            let _ = out.write_str("Overdue");
        }
        When::Today => day_label(0, wall, &mut out),
        When::Later(d) => day_label(d, wall, &mut out),
        When::NoDate => {
            let _ = out.write_str("No date");
        }
    }
    out
}

/// The grey (or red) line under a reminder's title: its time; for an overdue one its day too.
pub fn detail(m: &Model, i: usize, wall: i32) -> Line {
    let r = &m.list[i];
    let mut out = Line::new();
    if m.when(i, wall) == When::Overdue {
        day_label(m.day(i, wall).unwrap_or(0), wall, &mut out);
        if r.timed() {
            let _ = out.write_str(" ");
        }
    }
    if r.timed() {
        time_label(r.minute, &mut out);
    }
    out
}

/// The title, or "Reminder" while the phone keeps titles (flag bit 2, the default).
pub fn title(r: &Reminder) -> &str {
    if r.title.is_empty() { "Reminder" } else { r.title.as_str() }
}

/// The card left of the watch face: how many are due today (overdue too), then the next one.
pub fn card_value(m: &Model, wall: i32) -> Line {
    let mut out = Line::new();
    let (order, n) = m.order(wall);
    let _ = match (m.fetched, n, m.due_now(wall)) {
        (None, ..) => out.write_str("Open to sync"),
        (_, 0, _) => out.write_str("All done"),
        (_, _, 0) => out.write_str("Nothing due today"),
        (_, _, 1) => out.write_str("1 due today"),
        (_, _, k) => write!(out, "{k} due today"),
    };
    if n > 0 {
        let i = order[0] as usize;
        let r = &m.list[i];
        let _ = out.write_str("\n");
        if r.title.is_empty() {
            let _ = out.write_str("Reminder");
            if r.timed() && matches!(m.when(i, wall), When::Today | When::Overdue) {
                let _ = out.write_str(" ");
                time_label(r.minute, &mut out);
            }
        } else {
            // the card cuts it at 39 bytes with "…"
            let t = companion_rpc::utf8_cut(r.title.as_str().as_bytes(), 64 - out.len);
            let _ = out.write_str(t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec;
    use std::vec::Vec;

    // 2026-11-19 (Thursday) 10:00
    const WALL: i32 = 20_776 * 1440 + 600;

    fn rec(id: u32, day: Option<i8>, minute: Option<u16>, title: &str) -> Vec<u8> {
        let flags = day.is_some() as u8 | (minute.is_some() as u8) << 1 | (title.is_empty() as u8) << 2;
        let mut r = id.to_le_bytes().to_vec();
        r.push(day.unwrap_or(0) as u8);
        r.extend(minute.unwrap_or(0).to_le_bytes());
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

    /// The model with `recs` as the phone's answer to its first request.
    fn answered(recs: &[Vec<u8>]) -> Model {
        let mut m = Model::new();
        m.ask_now(0);
        let q = m.poll(0).unwrap();
        assert_eq!(&q[..], [b'R', b'1', b'Q', 1, 0x11, MAX as u8]);
        for (i, r) in recs.iter().enumerate() {
            let got = m.reply(&msg(1, 0, i as u8, recs.len() as u8, r), 0, WALL);
            assert_eq!(got, if i + 1 == recs.len() { Got::Saved } else { Got::Nothing });
        }
        m
    }

    #[test]
    fn calendar_maths() {
        assert_eq!(days(2026, 11, 19), 20_776);
        for z in [-1, 0, 20_776, 20_800, 11_016] {
            let (y, m, d) = civil(z);
            assert_eq!(days(y, m, d), z);
        }
        let mut l = Line::new();
        day_label(6, WALL, &mut l);
        assert_eq!(l.as_str(), "Wed 25 Nov");
        let mut l = Line::new();
        day_label(-3, WALL, &mut l);
        assert_eq!(l.as_str(), "Mon 16 Nov");
    }

    #[test]
    fn sections_overdue_first() {
        // the phone's order: by day, untimed first in a day, then by minute; then undated
        let m = answered(&[
            rec(1, Some(-2), None, "Pay rent"),
            rec(2, Some(0), None, "Call mum"),
            rec(3, Some(0), Some(540), ""),   // 09:00, passed at 10:00
            rec(4, Some(0), Some(1080), "Купить молоко"),
            rec(5, Some(1), Some(600), "Dentist"),
            rec(6, Some(6), None, "Taxes"),
            rec(7, None, None, "Someday"),
        ]);
        let (order, n) = m.order(WALL);
        assert_eq!(order[..n].iter().map(|&i| m.list[i as usize].id).collect::<Vec<_>>(), [1, 3, 2, 4, 5, 6, 7]);
        let when: Vec<When> = order[..n].iter().map(|&i| m.when(i as usize, WALL)).collect();
        assert_eq!(when, [When::Overdue, When::Overdue, When::Today, When::Today, When::Later(1), When::Later(6), When::NoDate]);
        assert_eq!(m.due_now(WALL), 4);
        assert_eq!(detail(&m, 0, WALL).as_str(), "Tue 17 Nov");
        assert_eq!(detail(&m, 2, WALL).as_str(), "Today 09:00");
        assert_eq!(detail(&m, 3, WALL).as_str(), "18:00");
        assert_eq!(detail(&m, 1, WALL).as_str(), "");
        assert_eq!(section_label(When::Later(1), WALL).as_str(), "Tomorrow");
        assert_eq!(title(&m.list[2]), "Reminder");
        assert_eq!(card_value(&m, WALL).as_str(), "4 due today\nPay rent");
        // a day later from the cache: everything one day closer, today's 18:00 one now overdue
        let next = WALL + 1440;
        assert_eq!(m.when(4, next), When::Today);
        assert_eq!(m.when(3, next), When::Overdue);
        assert_eq!(m.due_now(next), 5);
    }

    #[test]
    fn card_states() {
        let mut m = Model::new();
        assert_eq!(card_value(&m, WALL).as_str(), "Open to sync");
        m.ask_now(0);
        m.poll(0);
        assert_eq!(m.reply(&msg(1, rpc::OK, 0, 0, &[]), 0, WALL), Got::Saved);
        assert_eq!(card_value(&m, WALL).as_str(), "All done");
        let m = answered(&[rec(9, Some(0), Some(1080), "")]);
        assert_eq!(card_value(&m, WALL).as_str(), "1 due today\nReminder 18:00");
        let m = answered(&[rec(9, Some(2), None, "Taxes")]);
        assert_eq!(card_value(&m, WALL).as_str(), "Nothing due today\nTaxes");
    }

    #[test]
    fn statuses_keep_the_cache() {
        let mut m = answered(&[rec(1, None, None, "a")]);
        m.ask_now(5);
        let q = m.poll(5).unwrap();
        assert_eq!(m.reply(&msg(q[3], rpc::UNAVAILABLE, 0, 0, &[]), 6, WALL), Got::Changed);
        assert_eq!((m.status, m.n, m.wait(6)), (Status::Unavailable, 1, PERIOD_MS));
        m.ask_now(7);
        let q = m.poll(7).unwrap();
        assert_eq!(m.reply(&msg(q[3], rpc::ASK, 0, 0, &[]), 8, WALL), Got::Changed);
        assert_eq!((m.status, m.wait(8)), (Status::Ask, ASK_AGAIN_MS));
        // no answer at all: silent, the next try waits longer
        let q = m.poll(8 + ASK_AGAIN_MS).unwrap();
        m.poll(8 + ASK_AGAIN_MS + companion_rpc::RETRY_MS).unwrap();
        assert!(m.silent());
        assert_eq!(m.reply(&msg(q[3], rpc::DENIED, 0, 0, &[]), 9, WALL), Got::Nothing); // an old id
    }

    #[test]
    fn complete_goes_out_survives_a_restart_and_hides_the_row() {
        let mut m = answered(&[rec(0xCBE7AE7D, Some(0), None, "a"), rec(2, None, None, "b")]);
        assert!(m.complete(0, 100));
        assert_eq!((m.n, m.list[0].id), (1, 2));
        let q = m.poll(100).unwrap();
        assert_eq!(&q[..], [b'R', b'1', b'Q', 2, 0x12, 0x7D, 0xAE, 0xE7, 0xCB]);
        // no answer, then the app is closed: the cache still holds the tick
        let mut blob = [0u8; BLOB_MAX];
        let n = m.save(&mut blob);
        let mut back = Model::new();
        back.load(&blob[..n]);
        assert_eq!((back.n, back.done_n, back.done[0], back.fetched), (1, 1, 0xCBE7AE7D, Some(WALL)));
        back.ask_now(0);
        let q = back.poll(0).unwrap();
        assert_eq!(q[4], 0x12); // the completion first
        // a list that still has it (the phone answered before completing) doesn't bring the row back
        let l = back.poll(0).unwrap();
        assert_eq!(l[4], 0x11);
        assert_eq!(back.reply(&msg(l[3], 0, 0, 2, &rec(0xCBE7AE7D, Some(0), None, "a")), 1, WALL), Got::Nothing);
        assert_eq!(back.reply(&msg(l[3], 0, 1, 2, &rec(2, None, None, "b")), 1, WALL), Got::Saved);
        assert_eq!(back.n, 1);
        // UNAVAILABLE: later; OK: off the queue, and the list again
        assert_eq!(back.reply(&msg(q[3], rpc::UNAVAILABLE, 0, 0, &[]), 2, WALL), Got::Nothing);
        assert_eq!(back.wait(2), LATER_MS);
        let q = back.poll(2 + LATER_MS).unwrap();
        assert_eq!(back.reply(&msg(q[3], rpc::OK, 0, 0, &[]), 3, WALL), Got::Saved);
        assert_eq!((back.done_n, back.wait(3)), (0, 0));
        // a bad blob loads as nothing
        let mut junk = Model::new();
        junk.load(&blob[..n - 1]);
        junk.load(b"M2\0\0\0\0\0\0");
        assert_eq!((junk.n, junk.fetched, junk.done_n), (0, None, 0));
    }
}
