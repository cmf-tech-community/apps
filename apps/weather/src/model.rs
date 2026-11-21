//! Weather's state, without `Api`: the two calls to the phone (`location` once for the place, `weather` every 15
//! minutes while open), what they answered, the cache blob, and the texts and geometry the screens show.
//!
//! Hours and days count from the phone's answer: a cached answer moves with the clock, so the hours that passed and
//! yesterday drop out instead of being shown as now and today. They count from the phone's hour, not the watch's
//! at the moment the answer came (`Model::base`): the two clocks can sit on either side of an hour or of midnight.

use companion_rpc as rpc;
use core::fmt::Write;

/// After a temperature in text: nothing. The watch's fonts have no degree sign (it draws as a blank the width of
/// a letter, seen in the emulator), so texts carry plain numbers and the now screen draws a ring instead.
pub const DEG: &str = "";
/// A fresh answer this often while the app is open; the phone itself caches for 30 minutes.
pub const PERIOD_MS: u32 = 15 * 60_000;
/// While the phone waits for the user's "allow".
pub const ASK_AGAIN_MS: u32 = 10_000;
/// Hours asked for and shown.
pub const ASK_HOURS: u8 = 12;
pub const HOURS: usize = 24;
pub const DAYS: usize = 7;
const MAGIC: &[u8; 2] = b"W1";
pub const BLOB_MAX: usize = 2 + 4 + 1 + rpc::TEXT_MAX + 8 + HOURS * 3 + 1 + DAYS * 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// No answer to this run's request yet.
    Waiting,
    Ok,
    Ask,
    Denied,
    Unavailable,
}

/// What a message changed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Got {
    Nothing,
    /// The status: redraw.
    Changed,
    /// The data: redraw and store.
    Saved,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Clear,
    Partly,
    Cloud,
    Fog,
    Drizzle,
    Rain,
    Snow,
    Storm,
}

/// `temp` in 0.1 °C, `wind` m/s, `humidity` %.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Now {
    pub temp: i16,
    pub code: u8,
    pub wind: u8,
    pub humidity: u8,
}

/// `day` counts from the day of the answer; `min`/`max` °C, `precip` %.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Day {
    pub day: i8,
    pub min: i8,
    pub max: i8,
    pub code: u8,
    pub precip: u8,
}

const NO_HOUR: rpc::Hour = rpc::Hour { temp: 0, code: 0, precip: 0, night: false };
const NO_DAY: Day = Day { day: 0, min: 0, max: 0, code: 0, precip: 0 };

/// One answer of `weather`.
#[derive(Clone, Copy)]
struct Data {
    now: Option<Now>,
    /// Minutes the phone's data was old when it answered.
    age: u8,
    hour0: u8,
    nh: usize,
    hours: [rpc::Hour; HOURS],
    nd: usize,
    /// Sorted by `day`, each day once.
    days: [Day; DAYS],
}

const EMPTY: Data = Data { now: None, age: 0, hour0: 0, nh: 0, hours: [NO_HOUR; HOURS], nd: 0, days: [NO_DAY; DAYS] };

pub struct Model {
    pub status: Status,
    pub now: Option<Now>,
    /// The last answer had no hours: a companion from before them (or a source without).
    pub no_hours: bool,
    data: Data,
    pending: Data,
    /// Wall minutes of the answer `data` is.
    at: i32,
    place: rpc::Text,
    /// `location` answered in this run, with a place or a refusal.
    located: bool,
    /// Ask with hours; off once the phone called that a bad request.
    with_hours: bool,
    weather: rpc::Call,
    location: rpc::Call,
    ids: u8,
}

/// Days since 1970-01-01 of a civil date.
pub fn days(y: i32, m: u32, d: u32) -> i32 {
    let y = if m <= 2 { y - 1 } else { y };
    let (era, yoe) = (y.div_euclid(400), y.rem_euclid(400));
    let doy = (153 * ((m as i32 + 9) % 12) + 2) / 5 + d as i32 - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// Wall-clock minutes since 1970: what `at` and every `wall` argument are.
pub fn wall_min(y: i32, m: u32, d: u32, hour: u32, minute: u32) -> i32 {
    days(y, m, d) * 1440 + (hour * 60 + minute) as i32
}

pub fn icon(code: u8) -> Icon {
    match code {
        0 => Icon::Clear,
        1 | 2 => Icon::Partly,
        45 | 48 => Icon::Fog,
        51..=57 => Icon::Drizzle,
        61..=67 | 80..=82 => Icon::Rain,
        71..=77 | 85 | 86 => Icon::Snow,
        95..=99 => Icon::Storm,
        _ => Icon::Cloud,
    }
}

pub fn condition(code: u8) -> &'static str {
    match icon(code) {
        Icon::Clear => "Clear",
        Icon::Partly => "Partly cloudy",
        Icon::Cloud => "Cloudy",
        Icon::Fog => "Fog",
        Icon::Drizzle => "Drizzle",
        Icon::Rain => "Rain",
        Icon::Snow => "Snow",
        Icon::Storm => "Storm",
    }
}

/// 0.1 °C to whole degrees, halves away from zero; −0.4 is 0, never "-0".
pub fn deg(t10: i16) -> i16 {
    let t = t10 as i32;
    (if t >= 0 { (t + 5) / 10 } else { -((5 - t) / 10) }) as i16
}

/// A short text built in place; what doesn't fit is cut at a character.
#[derive(Clone, Copy)]
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
        let s = rpc::utf8_cut(s.as_bytes(), self.buf.len() - self.len);
        self.buf[self.len..self.len + s.len()].copy_from_slice(s.as_bytes());
        self.len += s.len();
        Ok(())
    }
}

/// "-3".
pub fn temp_text(deg: i16) -> Line {
    let mut l = Line::new();
    let _ = write!(l, "{deg}{DEG}");
    l
}

/// The hour of day of entry `i` of hours that start at `first`.
pub fn hour_label(first: u8, i: u8) -> u8 {
    ((first as u16 + i as u16) % 24) as u8
}

/// Of a day number (days since 1970, a Thursday).
pub fn weekday(abs_day: i32) -> &'static str {
    ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][(abs_day + 3).rem_euclid(7) as usize]
}

/// Where a day's `min..max` lies on a track `width` wide that spans the week's `lo..hi`: (x, length), the length
/// at least `min_len` and always inside the track.
pub fn bar(min: i8, max: i8, lo: i8, hi: i8, width: i16, min_len: i16) -> (i16, i16) {
    if hi <= lo {
        return (0, width);
    }
    let (lo, hi, w) = (lo as i32, hi as i32, width as i32);
    let (a, b) = ((min as i32).clamp(lo, hi), (max as i32).clamp(lo, hi));
    let (a, b) = (a.min(b), a.max(b));
    let x = (a - lo) * w / (hi - lo);
    let len = ((b - lo) * w / (hi - lo) - x).max(min_len as i32).min(w);
    (x.min(w - len) as i16, len as i16)
}

/// "Updated 50 min ago" once the data is older than the phone's own cache keeps it.
pub fn updated(stale_min: i32) -> Option<Line> {
    if stale_min <= 45 {
        return None;
    }
    let mut l = Line::new();
    let _ = match stale_min {
        ..90 => write!(l, "Updated {stale_min} min ago"),
        ..2880 => write!(l, "Updated {} h ago", stale_min / 60),
        _ => write!(l, "Updated {} d ago", stale_min / 1440),
    };
    Some(l)
}

struct Rd<'a>(&'a [u8]);

impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Some(a)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
}

impl Model {
    pub const fn new() -> Self {
        Model {
            status: Status::Waiting,
            now: None,
            no_hours: false,
            data: EMPTY,
            pending: EMPTY,
            at: 0,
            place: rpc::Text::EMPTY,
            located: false,
            with_hours: true,
            weather: rpc::Call::new(60_000),
            location: rpc::Call::new(60_000),
            ids: 0,
        }
    }

    /// Ask at once (start, `RESUME`, the page shown again); the place only until the phone answered about it.
    pub fn ask_now(&mut self, now: u32) {
        self.weather.now(now);
        if !self.located {
            self.location.now(now);
        }
    }

    /// A request to send now, if one is due.
    pub fn poll(&mut self, now: u32) -> Option<rpc::Msg> {
        if let Some(id) = self.location.due(now, &mut self.ids) {
            return Some(rpc::location(id));
        }
        let id = self.weather.due(now, &mut self.ids)?;
        self.pending = EMPTY;
        Some(rpc::weather(id, DAYS as u8, if self.with_hours { ASK_HOURS } else { 0 }))
    }

    /// Ms until [`Model::poll`] has something to do.
    pub fn wait(&self, now: u32) -> u32 {
        self.weather.wait(now).min(self.location.wait(now))
    }

    /// The phone left the last weather request unanswered.
    pub fn silent(&self) -> bool {
        self.weather.silent()
    }

    /// One message from the phone, at `now` ms and `wall` minutes.
    pub fn reply(&mut self, m: &[u8], now: u32, wall: i32) -> Got {
        match self.location.take(m) {
            rpc::Got::Nothing => {}
            rpc::Got::Status(rpc::ASK) => {
                self.location.after(now, ASK_AGAIN_MS);
                return Got::Nothing;
            }
            // denied, no fix: the weather shows without a place
            rpc::Got::Status(_) => {
                self.located = true;
                self.location.stop();
                return Got::Nothing;
            }
            rpc::Got::Record { record, .. } => {
                self.located = true;
                self.location.stop();
                let Some(l) = rpc::Location::decode(record) else { return Got::Nothing };
                self.place = l.place;
                return Got::Saved;
            }
        }
        match self.weather.take(m) {
            rpc::Got::Nothing => Got::Nothing,
            // a companion from before hours (Android): once more the old way
            rpc::Got::Status(rpc::BAD_REQUEST) if self.with_hours => {
                self.with_hours = false;
                self.weather.now(now);
                Got::Nothing
            }
            rpc::Got::Status(s) => {
                self.status = match s {
                    rpc::ASK => Status::Ask,
                    rpc::DENIED => Status::Denied,
                    _ => Status::Unavailable, // OK without records too
                };
                self.weather.after(now, if self.status == Status::Ask { ASK_AGAIN_MS } else { PERIOD_MS });
                Got::Changed
            }
            // any order; one that doesn't decode is left out
            rpc::Got::Record { record, last, .. } => {
                self.record(record);
                if !last {
                    return Got::Nothing;
                }
                self.weather.after(now, PERIOD_MS);
                if self.pending.now.is_none() {
                    self.status = Status::Unavailable;
                    return Got::Changed;
                }
                self.data = self.pending;
                self.now = self.data.now;
                self.at = wall;
                self.no_hours = self.data.nh == 0;
                self.status = Status::Ok;
                Got::Saved
            }
        }
    }

    fn record(&mut self, record: &[u8]) {
        let p = &mut self.pending;
        match rpc::Weather::decode(record) {
            Some(rpc::Weather::Now { temp, code, wind, humidity, age }) => {
                p.now = Some(Now { temp, code, wind, humidity });
                p.age = age;
            }
            Some(w @ rpc::Weather::Hours { hour, n, .. }) => {
                p.hour0 = hour % 24;
                p.nh = (n as usize).min(HOURS);
                for i in 0..p.nh {
                    p.hours[i] = w.hour(i).unwrap_or(NO_HOUR);
                }
            }
            Some(rpc::Weather::Day { day, min, max, code, precip }) => {
                Self::add_day(p, Day { day, min, max, code, precip });
            }
            None => {}
        }
    }

    /// Keeps `days` sorted; a day before the answer's, a repeat or an eighth is dropped.
    fn add_day(p: &mut Data, d: Day) {
        if d.day < 0 || p.nd == DAYS || p.days[..p.nd].iter().any(|x| x.day == d.day) {
            return;
        }
        let at = p.days[..p.nd].iter().position(|x| x.day > d.day).unwrap_or(p.nd);
        p.days.copy_within(at..p.nd, at + 1);
        p.days[at] = d;
        p.nd += 1;
    }

    /// The locality, empty when the phone gave none.
    pub fn place(&self) -> &str {
        self.place.as_str()
    }

    /// Minutes since the phone fetched what is shown.
    pub fn stale_min(&self, wall: i32) -> i32 {
        wall - self.at + self.data.age as i32
    }

    /// The hour (hours since 1970, wall clock) the phone was in when it answered: the first hourly entry's, and
    /// the one its day 0 is the day of. The watch's own hour at `at`, moved to the nearest one that is `hour0`
    /// o'clock; without hours, the watch's.
    fn base(&self) -> i32 {
        let h = self.at.div_euclid(60);
        if self.data.nh == 0 {
            return h;
        }
        h + (self.data.hour0 as i32 - h.rem_euclid(24) + 36).rem_euclid(24) - 12
    }

    /// Hours of the answer that passed by `wall`.
    fn hours_gone(&self, wall: i32) -> usize {
        (wall.div_euclid(60) - self.base()).clamp(0, self.data.nh as i32) as usize
    }

    pub fn hours_len(&self, wall: i32) -> usize {
        self.data.nh - self.hours_gone(wall)
    }

    /// Row `k` of the hours at `wall`: the hour of day, whether it is the current hour, the hour.
    pub fn hour_at(&self, k: usize, wall: i32) -> Option<(u8, bool, rpc::Hour)> {
        let i = self.hours_gone(wall) + k;
        if i >= self.data.nh {
            return None;
        }
        let is_now = self.base() + i as i32 == wall.div_euclid(60);
        Some((hour_label(self.data.hour0, i as u8), is_now, self.data.hours[i]))
    }

    /// `wall`'s day counted from the answer's.
    fn today_index(&self, wall: i32) -> i32 {
        wall.div_euclid(1440) - self.base().div_euclid(24)
    }

    fn shown_days(&self, wall: i32) -> &[Day] {
        let t = self.today_index(wall);
        let d = &self.data.days[..self.data.nd];
        &d[d.iter().position(|x| x.day as i32 >= t).unwrap_or(d.len())..]
    }

    pub fn days_len(&self, wall: i32) -> usize {
        self.shown_days(wall).len()
    }

    /// Row `k` of the days at `wall`: the day's number (days since 1970), whether it is today, the day.
    pub fn day_at(&self, k: usize, wall: i32) -> Option<(i32, bool, Day)> {
        let d = *self.shown_days(wall).get(k)?;
        Some((self.base().div_euclid(24) + d.day as i32, d.day as i32 == self.today_index(wall), d))
    }

    pub fn today(&self, wall: i32) -> Option<Day> {
        self.day_at(0, wall).filter(|x| x.1).map(|x| x.2)
    }

    /// Lowest min and highest max of the days shown.
    pub fn range(&self, wall: i32) -> (i8, i8) {
        let d = self.shown_days(wall);
        (d.iter().map(|x| x.min).min().unwrap_or(0), d.iter().map(|x| x.max).max().unwrap_or(0))
    }

    /// The widget card: title (≤ 23 bytes) and value (≤ 39 bytes, two lines).
    pub fn card(&self, wall: i32) -> (Line, Line) {
        let (mut t, mut v) = (Line::new(), Line::new());
        let place = rpc::utf8_cut(self.place.as_str().as_bytes(), 23);
        let _ = t.write_str(if place.is_empty() { "Weather" } else { place });
        let Some(n) = self.now else {
            let _ = v.write_str("Open the app");
            return (t, v);
        };
        let _ = write!(v, "{}{DEG} {}", deg(n.temp), condition(n.code));
        if let Some(d) = self.today(wall) {
            let _ = write!(v, "\nH {}{DEG} L {}{DEG}", d.max, d.min);
        }
        (t, v)
    }

    /// The cache blob; 0 before the first answer.
    pub fn save(&self, out: &mut [u8]) -> usize {
        let (Some(n), d) = (self.data.now, &self.data) else { return 0 };
        let place = self.place.as_str().as_bytes();
        let len = 2 + 4 + 1 + place.len() + 8 + d.nh * 3 + 1 + d.nd * 5;
        if out.len() < len {
            return 0;
        }
        let mut k = 0;
        let mut put = |b: &[u8]| {
            out[k..k + b.len()].copy_from_slice(b);
            k += b.len();
        };
        put(MAGIC);
        put(&self.at.to_le_bytes());
        put(&[place.len() as u8]);
        put(place);
        put(&n.temp.to_le_bytes());
        put(&[n.code, n.wind, n.humidity, d.age, d.hour0, d.nh as u8]);
        for h in &d.hours[..d.nh] {
            put(&[h.temp as u8, h.code, h.precip | (h.night as u8) << 7]);
        }
        put(&[d.nd as u8]);
        for x in &d.days[..d.nd] {
            put(&[x.day as u8, x.min as u8, x.max as u8, x.code, x.precip]);
        }
        len
    }

    /// Reads a blob of [`Model::save`]; anything else (another version, cut short, counts out of range) is left
    /// alone and the model stays as it was.
    pub fn load(&mut self, b: &[u8]) -> bool {
        let Some((at, place, data)) = Self::parse(b) else { return false };
        (self.at, self.place, self.data, self.now) = (at, place, data, data.now);
        self.no_hours = false; // not known until the phone answers
        true
    }

    fn parse(b: &[u8]) -> Option<(i32, rpc::Text, Data)> {
        let mut r = Rd(b);
        if r.take(2)? != MAGIC {
            return None;
        }
        let at = i32::from_le_bytes(r.take(4)?.try_into().ok()?);
        let n = r.u8()? as usize;
        let place = core::str::from_utf8(r.take(n)?).ok()?;
        let mut d = EMPTY;
        let temp = i16::from_le_bytes(r.take(2)?.try_into().ok()?);
        d.now = Some(Now { temp, code: r.u8()?, wind: r.u8()?, humidity: r.u8()? });
        d.age = r.u8()?;
        d.hour0 = r.u8()?;
        d.nh = r.u8()? as usize;
        if d.hour0 > 23 || d.nh > HOURS {
            return None;
        }
        for i in 0..d.nh {
            let e = r.take(3)?;
            d.hours[i] = rpc::Hour { temp: e[0] as i8, code: e[1], precip: e[2] & 0x7F, night: e[2] & 0x80 != 0 };
        }
        let nd = r.u8()? as usize;
        if nd > DAYS {
            return None;
        }
        for _ in 0..nd {
            let e = r.take(5)?;
            Self::add_day(&mut d, Day { day: e[0] as i8, min: e[1] as i8, max: e[2] as i8, code: e[3], precip: e[4] });
        }
        if !r.0.is_empty() || d.nd != nd {
            return None;
        }
        Some((at, rpc::Text::new(place.as_bytes()), d))
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
