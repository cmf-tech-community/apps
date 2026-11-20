//! The crate against `rpc-vectors.json`, a copy of the companion repo's
//! `ios/WireKit/Tests/WireKitTests/rpc-vectors.json` (fmctools/companion). When the spec changes, copy it again.
//! Only the watch's side is checked: what it sends and what it reads. Phone-side rules (ordering, FNV ids,
//! clamping, rejecting bad requests) are tested in the companion.

use companion_rpc::*;
use std::collections::BTreeMap;

// ---- a small JSON reader, enough for the vectors file

#[derive(Debug, Clone)]
enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(BTreeMap<String, J>),
}

impl J {
    fn parse(s: &str) -> J {
        let (v, rest) = value(s.trim_start());
        assert!(rest.trim().is_empty(), "trailing JSON");
        v
    }
    fn get(&self, k: &str) -> &J {
        match self {
            J::Obj(m) => m.get(k).unwrap_or(&J::Null),
            _ => &J::Null,
        }
    }
    fn arr(&self) -> &[J] {
        match self {
            J::Arr(a) => a,
            _ => panic!("not an array: {self:?}"),
        }
    }
    fn str(&self) -> &str {
        match self {
            J::Str(s) => s,
            _ => panic!("not a string: {self:?}"),
        }
    }
    fn num(&self) -> f64 {
        match self {
            J::Num(n) => *n,
            _ => panic!("not a number: {self:?}"),
        }
    }
    fn int(&self) -> i64 {
        self.num() as i64
    }
    fn truthy(&self) -> bool {
        matches!(self, J::Bool(true))
    }
    fn hex(&self) -> Vec<u8> {
        hex(self.str())
    }
}

fn value(s: &str) -> (J, &str) {
    match s.as_bytes()[0] {
        b'{' => {
            let mut m = BTreeMap::new();
            let mut s = s[1..].trim_start();
            if let Some(r) = s.strip_prefix('}') {
                return (J::Obj(m), r);
            }
            loop {
                let (k, r) = value(s);
                let r = r.trim_start().strip_prefix(':').unwrap().trim_start();
                let (v, r) = value(r);
                m.insert(k.str().to_owned(), v);
                let r = r.trim_start();
                if let Some(r) = r.strip_prefix('}') {
                    return (J::Obj(m), r);
                }
                s = r.strip_prefix(',').unwrap().trim_start();
            }
        }
        b'[' => {
            let mut a = Vec::new();
            let mut s = s[1..].trim_start();
            if let Some(r) = s.strip_prefix(']') {
                return (J::Arr(a), r);
            }
            loop {
                let (v, r) = value(s);
                a.push(v);
                let r = r.trim_start();
                if let Some(r) = r.strip_prefix(']') {
                    return (J::Arr(a), r);
                }
                s = r.strip_prefix(',').unwrap().trim_start();
            }
        }
        b'"' => {
            let mut out = String::new();
            let mut it = s[1..].char_indices();
            while let Some((i, c)) = it.next() {
                match c {
                    '"' => return (J::Str(out), &s[i + 2..]),
                    '\\' => match it.next().unwrap().1 {
                        'u' => {
                            let h: String = (0..4).map(|_| it.next().unwrap().1).collect();
                            let mut cp = u32::from_str_radix(&h, 16).unwrap();
                            if (0xD800..0xDC00).contains(&cp) {
                                it.next();
                                it.next();
                                let l: String = (0..4).map(|_| it.next().unwrap().1).collect();
                                cp = 0x10000 + ((cp - 0xD800) << 10) + (u32::from_str_radix(&l, 16).unwrap() - 0xDC00);
                            }
                            out.push(char::from_u32(cp).unwrap());
                        }
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        e => out.push(e),
                    },
                    c => out.push(c),
                }
            }
            panic!("unterminated string")
        }
        b't' => (J::Bool(true), &s[4..]),
        b'f' => (J::Bool(false), &s[5..]),
        b'n' => (J::Null, &s[4..]),
        _ => {
            let end = s.find(|c: char| !(c.is_ascii_digit() || "+-.eE".contains(c))).unwrap_or(s.len());
            (J::Num(s[..end].parse().unwrap()), &s[end..])
        }
    }
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn vectors() -> J {
    J::parse(include_str!("rpc-vectors.json"))
}

fn joined(parts: Parts<'_>) -> Vec<Vec<u8>> {
    parts.map(|m| m.to_vec()).collect()
}

fn hexes(j: &J) -> Vec<Vec<u8>> {
    j.arr().iter().map(J::hex).collect()
}

// ---- requests and replies

#[test]
fn requests() {
    let v = vectors();
    for r in v.get("requests").arr() {
        let m = r.get("hex").hex();
        if matches!(r.get("id"), J::Null) {
            assert_eq!(reply(&m), None, "{r:?} is no reply");
            continue;
        }
        let built = request(r.get("id").int() as u8, r.get("method").int() as u8, &r.get("args").hex()).unwrap();
        assert_eq!(built.to_vec(), m);
    }
    // the typed builders give the same bytes
    assert_eq!(caps(7).to_vec(), hex("5231510701"));
    assert_eq!(calendar_events(8, 0, 2, 5).to_vec(), hex("5231510810000205"));
    assert_eq!(http_get(9, "https://example.com/a?b=1").unwrap().to_vec(), hex("523151094068747470733a2f2f6578616d706c652e636f6d2f613f623d31"));
}

#[test]
fn replies_through_a_call() {
    let v = vectors();
    for r in v.get("replies").arr() {
        let (id, status, records) = (r.get("id").int() as u8, r.get("status").int() as u8, hexes(r.get("records")));
        let mut ids = id - 1;
        let mut c = Call::new(300_000);
        c.now(0);
        assert_eq!(c.due(0, &mut ids), Some(id));
        let mut got = vec![Vec::new(); records.len()];
        for (n, m) in hexes(r.get("messages")).iter().enumerate() {
            let p = reply(m).unwrap();
            assert_eq!((p.id, p.status), (id, status));
            match c.take(m) {
                Got::Status(s) => assert!(records.is_empty() && s == status),
                Got::Record { index, record, last, .. } => {
                    got[index as usize] = record.to_vec();
                    assert_eq!(last, n + 1 == records.len());
                }
                Got::Nothing => panic!("{r:?}"),
            }
        }
        assert_eq!(got, records);
        assert!(!c.open());
    }
}

#[test]
fn utf8_cut_as_the_phone() {
    for r in vectors().get("utf8_cut").arr() {
        let cut = utf8_cut(r.get("text").str().as_bytes(), r.get("max").int() as usize);
        assert_eq!(cut.as_bytes(), r.get("hex").hex());
    }
}

// ---- records, method by method

#[test]
fn calendar() {
    for r in vectors().get("calendar").arr() {
        let (i, bytes) = (r.get("in"), r.get("hex").hex());
        let e = Event::decode(&bytes).unwrap();
        let show = i.get("show").truthy();
        let mut title = i.get("title").str().to_owned();
        title = utf8_cut(title.as_bytes(), TEXT_MAX).to_owned();
        assert_eq!(e.day as i64, i.get("day").int());
        assert_eq!((e.start as i64, e.duration as i64, e.color as i64), (i.get("start").int(), i.get("duration").int(), i.get("color").int()));
        assert_eq!(e.flags & Event::ALL_DAY != 0, i.get("all_day").truthy());
        assert_eq!(e.flags & Event::TENTATIVE != 0, i.get("tentative").truthy());
        assert_eq!(e.flags & HIDDEN != 0, !show);
        assert_eq!(e.title.as_str(), if show { title.as_str() } else { "" });
        let mut out = [0u8; Event::RECORD_MAX];
        let n = e.encode(&mut out);
        assert_eq!(&out[..n], &bytes[..]);
    }
    assert_eq!(Event::decode(&hex("003a021e0004")), None);
}

#[test]
fn reminders() {
    let v = vectors();
    for r in v.get("reminders").get("records").arr() {
        let (i, bytes) = (r.get("in"), r.get("hex").hex());
        let x = Reminder::decode(&bytes).unwrap();
        let short = v.get("reminders").get("ids").arr().iter().find(|d| d.get("id").str() == i.get("id").str()).unwrap();
        assert_eq!(x.id as i64, short.get("short").int());
        let due = !matches!(i.get("day"), J::Null);
        let timed = !matches!(i.get("minute"), J::Null);
        assert_eq!((x.due(), x.timed()), (due, timed));
        assert_eq!(x.day as i64, if due { i.get("day").int() } else { 0 });
        assert_eq!(x.minute as i64, if timed { i.get("minute").int() } else { 0 });
        assert_eq!(x.color as i64, i.get("color").int());
        let show = i.get("show").truthy();
        assert_eq!(x.flags & HIDDEN != 0, !show);
        assert_eq!(x.title.as_str(), if show { i.get("title").str() } else { "" });
        let mut out = [0u8; Reminder::RECORD_MAX];
        let n = x.encode(&mut out);
        assert_eq!(&out[..n], &bytes[..]);
    }
    assert_eq!(companion_rpc::reminders(3, 20).to_vec(), hex("523151031114"));
    assert_eq!(companion_rpc::reminders_complete(4, 0xCBE7AE7D).to_vec(), hex("52315104127daee7cb"));
}

#[test]
fn location() {
    for r in vectors().get("location").arr() {
        let (i, l) = (r.get("in"), Location::decode(&r.get("hex").hex()).unwrap());
        assert_eq!(l.lat as i64, (i.get("lat").num() * 10.0).round() as i64);
        assert_eq!(l.lon as i64, (i.get("lon").num() * 10.0).round() as i64);
        assert_eq!(l.age as i64, i.get("age_min").int().min(0xFFFF));
        assert_eq!(l.place.as_str(), i.get("place").str());
    }
}

#[test]
fn weather() {
    for r in vectors().get("weather").arr() {
        let i = r.get("in");
        if i.get("kind").str() == "hours" {
            let Some(w @ Weather::Hours { hour, n, .. }) = Weather::decode(&r.get("hex").hex()) else { panic!("not hours") };
            let hours = i.get("hours").arr();
            assert_eq!((hour as i64, n as usize), (i.get("hour").int(), hours.len()));
            for (k, h) in hours.iter().enumerate() {
                let want = Hour {
                    temp: h.get("temp").num().round() as i8,
                    code: h.get("code").int() as u8,
                    precip: h.get("precip").int() as u8,
                    night: h.get("night").truthy(),
                };
                assert_eq!(w.hour(k), Some(want));
            }
            assert_eq!(w.hour(hours.len()), None);
            continue;
        }
        let want = if i.get("kind").str() == "now" {
            Weather::Now {
                temp: (i.get("temp").num() * 10.0).round() as i16,
                code: i.get("code").int() as u8,
                wind: i.get("wind").int() as u8,
                humidity: i.get("humidity").int() as u8,
                age: i.get("age_min").int() as u8,
            }
        } else {
            Weather::Day {
                day: i.get("day").int() as i8,
                min: i.get("min").num().round() as i8,
                max: i.get("max").num().round() as i8,
                code: i.get("code").int() as u8,
                precip: i.get("precip").int() as u8,
            }
        };
        assert_eq!(Weather::decode(&r.get("hex").hex()), Some(want));
    }
}

#[test]
fn weather_args() {
    for r in vectors().get("weather_args").arr() {
        if r.get("bad").truthy() {
            continue; // what the phone refuses; `weather` can't build them except hours over 24, cut below
        }
        let m = companion_rpc::weather(9, r.get("days").int() as u8, r.get("hours").int() as u8);
        assert_eq!(&m[..], &[&hex("52315109")[..], &[0x30], &r.get("args").hex()[..]].concat()[..]);
    }
    assert_eq!(&companion_rpc::weather(9, 3, 30)[..], &hex("5231510930").iter().copied().chain([3, 24]).collect::<Vec<u8>>()[..], "hours over 24 are 24");
}

#[test]
fn weather_hours_odd_lengths() {
    // a cut entry is dropped; more than 24 hours are 24; no entry at all is still a record
    let Some(w @ Weather::Hours { n: 1, .. }) = Weather::decode(&[2, 5, 1, 2, 3, 9, 9]) else { panic!() };
    assert_eq!(w.hour(0), Some(Hour { temp: 1, code: 2, precip: 3, night: false }));
    let long = [&[2u8, 23][..], &[0x80u8; 90][..]].concat();
    let Some(w @ Weather::Hours { n: 24, hour: 23, .. }) = Weather::decode(&long) else { panic!() };
    assert_eq!(w.hour(23), Some(Hour { temp: -128, code: 0x80, precip: 0, night: true }));
    assert!(matches!(Weather::decode(&[2, 7]), Some(Weather::Hours { n: 0, .. })));
    assert_eq!(Weather::decode(&[2]), None);
}

#[test]
fn http() {
    let v = vectors();
    for r in v.get("http").arr() {
        let (i, recs) = (r.get("in"), hexes(r.get("records")));
        let body = i.get("body").hex();
        assert_eq!(HttpHead::decode(&recs[0]), Some(HttpHead { code: i.get("code").int() as u16, length: body.len() as u16 }));
        assert_eq!(recs[1..].concat(), body);
        assert!(recs[1..].iter().all(|c| c.len() <= RECORD_MAX));
    }
    for r in v.get("http_request").arr() {
        if r.get("bad").truthy() {
            continue;
        }
        let q = r.get("request");
        let msgs = hexes(r.get("messages"));
        let method = ["GET", "POST", "PUT", "PATCH", "DELETE"].iter().position(|m| *m == q.get("method").str()).unwrap() as u8;
        let mut out = [0u8; JOINED_MAX];
        let p = http_request(msgs[0][3], method, q.get("url").str(), &q.get("body").hex(), &mut out).unwrap();
        assert_eq!(joined(p), msgs, "{}", r.get("note").str());
    }
}

#[test]
fn notifications() {
    for r in vectors().get("notifications").arr() {
        let (i, n) = (r.get("in"), Notification::decode(&r.get("hex").hex()).unwrap());
        let show = i.get("show").truthy();
        assert_eq!((n.age as i64, n.app.as_str()), (i.get("age_min").int(), i.get("app").str()));
        assert_eq!(n.flags & HIDDEN != 0, !show);
        assert_eq!(n.text.as_str(), if show { i.get("text").str() } else { "" });
    }
}

#[test]
fn notify() {
    for r in vectors().get("notify").arr() {
        let args = r.get("args").hex();
        if r.get("bad").truthy() {
            continue; // malformed bytes the typed builder can't produce; the title over 64 bytes is refused below
        }
        let a = r.get("alert");
        let flags = a.get("sound").truthy() as u8 * NOTIFY_SOUND | a.get("find").truthy() as u8 * NOTIFY_FIND_PHONE;
        let m = companion_rpc::notify(1, flags, a.get("title").str(), a.get("body").str()).unwrap();
        assert_eq!(&m[5..], &args[..]);
    }
    assert!(companion_rpc::notify(1, 1, &"A".repeat(65), "").is_none());
}

#[test]
fn storage() {
    let v = vectors();
    let s = v.get("storage");
    for k in s.get("keys").arr() {
        assert_eq!(storage_key_ok(&k.get("key").hex()), k.get("ok").truthy(), "{k:?}");
    }
    for p in s.get("put").arr() {
        let msgs = hexes(p.get("messages"));
        let mut out = [0u8; JOINED_MAX];
        let parts = storage_put(msgs[0][3], p.get("key").str(), &p.get("value").hex(), &mut out).unwrap();
        assert_eq!(joined(parts), msgs, "{}", p.get("note").str());
    }
    for g in s.get("get").arr() {
        let (recs, value) = (hexes(g.get("records")), g.get("value").hex());
        assert_eq!(storage_length(&recs[0]), Some(value.len() as u16));
        assert_eq!(recs[1..].concat(), value);
    }
    assert_eq!(storage_get(2, "score").unwrap().to_vec(), hex("523151026073636f7265"));
    assert_eq!(storage_delete(2, "score").unwrap().to_vec(), hex("523151026273636f7265"));
}

#[test]
fn battery() {
    for r in vectors().get("battery").arr() {
        let (i, b) = (r.get("in"), Battery::decode(&r.get("hex").hex()).unwrap());
        assert_eq!(b.level as i64, i.get("level").int().clamp(0, 100));
        assert_eq!(b.plugged, i.get("charging").truthy());
    }
}

#[test]
fn media() {
    for r in vectors().get("media").arr() {
        let (i, n) = (r.get("in"), NowPlaying::decode(&r.get("hex").hex()).unwrap());
        let secs = |k: &str| match i.get(k) {
            J::Null => 0xFFFF,
            j => j.int().min(0xFFFE), // 0xFFFF means unknown
        };
        assert_eq!(n.playing, i.get("playing").truthy());
        assert_eq!((n.position as i64, n.duration as i64), (secs("position"), secs("duration")));
        assert_eq!(n.app.as_str(), utf8_cut(i.get("app").str().as_bytes(), 24));
        assert_eq!(n.title.as_str(), utf8_cut(i.get("title").str().as_bytes(), 48));
        assert_eq!(n.artist.as_str(), utf8_cut(i.get("artist").str().as_bytes(), 32));
    }
    assert_eq!(media_control(1, media::NEXT).to_vec(), hex("523151018103"));
}

#[test]
fn jobs() {
    for r in vectors().get("jobs").arr() {
        if r.get("bad").truthy() {
            continue; // payloads the typed builder can't produce, or refuses (`job_set` unit test)
        }
        let (j, payload) = (r.get("job"), r.get("payload").hex());
        let spec = j.get("spec").hex();
        let what = match j.get("kind").int() {
            0 => {
                let (url_len, url) = (spec[1] as usize, &spec[2..2 + spec[1] as usize]);
                Job::Web { method: spec[0], url: std::str::from_utf8(url).unwrap(), body: &spec[2 + url_len..] }
            }
            1 => Job::Weather { days: spec[0], hours: spec.get(1).copied().unwrap_or(0) },
            _ => Job::Calendar { from_day: spec[0] as i8, days: spec[1], limit: spec[2] },
        };
        // the vectors' `minutes` is what the phone makes of the payload's; build from the payload's own
        let minutes = u16::from_le_bytes([payload[1], payload[2]]);
        let mut out = [0u8; JOINED_MAX];
        let mut p = job_set(5, j.get("id").int() as u8, minutes, what, j.get("target").int() as u8, &mut out).unwrap();
        let m = p.next().unwrap();
        assert!(p.next().is_none());
        assert_eq!(&m[..], &[&hex("52315105900001")[..], &payload].concat()[..]);
    }
    assert_eq!(job_delete(6, 2).to_vec(), hex("523151069102"));
}
