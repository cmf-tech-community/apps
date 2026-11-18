//! FMC Companion miniapp RPC, the watch side of `calendar.events` (companion repo, `docs/miniapp-rpc.md`).
//! Request `"R1Q" id method args`, reply `"R1A" id status index total record`; integers little-endian.

use cmf_wp2_ui::text::fit;

pub const CALENDAR_EVENTS: u8 = 0x10;

pub const OK: u8 = 0;
pub const DENIED: u8 = 2;
pub const ASK: u8 = 4;

pub const TITLE_MAX: usize = 64;
/// Header of a calendar record: day, start, duration, flags, color.
const HEAD: usize = 8;
pub const RECORD_MAX: usize = HEAD + TITLE_MAX;

/// `calendar.events` for `days` days from `from_day` (0 = the phone's today), at most `limit` events.
pub fn calendar_request(id: u8, from_day: i8, days: u8, limit: u8) -> [u8; 8] {
    [b'R', b'1', b'Q', id, CALENDAR_EVENTS, from_day as u8, days, limit]
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Reply<'a> {
    pub id: u8,
    pub status: u8,
    pub index: u8,
    pub total: u8,
    pub record: &'a [u8],
}

/// A reply message, or `None` for anything else on the channel (other prefixes, requests, short messages).
pub fn reply(m: &[u8]) -> Option<Reply<'_>> {
    match m {
        [b'R', b'1', b'A', id, status, index, total, record @ ..] => {
            Some(Reply { id: *id, status: *status, index: *index, total: *total, record })
        }
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Event {
    /// Days from the phone's today at the time of the answer.
    pub day: i8,
    /// Minute of the day, local; 0 for all-day.
    pub start: u16,
    /// Real minutes from start to end.
    pub duration: u16,
    pub flags: u8,
    /// RGB565.
    pub color: u16,
    title: [u8; TITLE_MAX],
    len: u8,
}

impl Event {
    pub const EMPTY: Event = Event { day: 0, start: 0, duration: 0, flags: 0, color: 0, title: [0; TITLE_MAX], len: 0 };

    pub fn all_day(&self) -> bool {
        self.flags & 1 != 0
    }

    #[cfg(test)]
    pub fn tentative(&self) -> bool {
        self.flags & 2 != 0
    }

    /// Empty when the phone hides titles (flags bit 2, the default) or the event has none.
    pub fn title(&self) -> &str {
        fit(&self.title[..self.len as usize], TITLE_MAX)
    }

    /// A calendar record; the title is cut to valid UTF-8 of at most 64 bytes. `None` if the header is short.
    pub fn decode(r: &[u8]) -> Option<Event> {
        if r.len() < HEAD {
            return None;
        }
        let u16_at = |i: usize| u16::from_le_bytes([r[i], r[i + 1]]);
        let mut e = Event {
            day: r[0] as i8,
            start: u16_at(1),
            duration: u16_at(3),
            flags: r[5],
            color: u16_at(6),
            ..Event::EMPTY
        };
        let t = fit(&r[HEAD..], TITLE_MAX).as_bytes();
        e.title[..t.len()].copy_from_slice(t);
        e.len = t.len() as u8;
        Some(e)
    }

    /// The record again (for the cache); `out` holds at least [`RECORD_MAX`] bytes. Returns its length.
    pub fn encode(&self, out: &mut [u8]) -> usize {
        out[0] = self.day as u8;
        out[1..3].copy_from_slice(&self.start.to_le_bytes());
        out[3..5].copy_from_slice(&self.duration.to_le_bytes());
        out[5] = self.flags;
        out[6..8].copy_from_slice(&self.color.to_le_bytes());
        let n = self.len as usize;
        out[HEAD..HEAD + n].copy_from_slice(&self.title[..n]);
        HEAD + n
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    // Copied from the companion repo, branch miniapp-rpc, ios/WireKit/Tests/WireKitTests/rpc-vectors.json,
    // section "calendar": (hex, day, start, duration, flags, color, title).
    const CALENDAR: [(&str, i8, u16, u16, u8, u16, &str); 4] = [
        ("003a021e000000f85374616e647570", 0, 570, 30, 0, 63488, "Standup"),
        ("003a021e000400f8", 0, 570, 30, 4, 63488, ""),
        ("010000a00503e007d09ed182d0bfd183d181d0ba", 1, 0, 1440, 3, 2016, "Отпуск"),
        (
            "ff64057800001f00d0a1d0bed0b7d0b2d0bed0bd20d18120d0bad0bed0bcd0b0d0bdd0b4d0bed0b920d0bfd0be20d180d0b5d0bbd0b8d0b7d18320d0b820d0bed0b1d181d183d0b6",
            -1,
            1380,
            120,
            0,
            31,
            // the phone's 64-byte cut of "Созвон с командой по релизу и обсуждение планов на квартал"
            "Созвон с командой по релизу и обсуж",
        ),
    ];

    #[test]
    fn calendar_vectors_decode_and_encode_back() {
        for (h, day, start, duration, flags, color, title) in CALENDAR {
            let bytes = hex(h);
            let e = Event::decode(&bytes).unwrap();
            assert_eq!((e.day, e.start, e.duration, e.flags, e.color, e.title()), (day, start, duration, flags, color, title));
            let mut out = [0u8; RECORD_MAX];
            let n = e.encode(&mut out);
            assert_eq!(&out[..n], &bytes[..]);
        }
        let hidden = Event::decode(&hex(CALENDAR[1].0)).unwrap();
        assert!(hidden.flags & 4 != 0 && hidden.title().is_empty());
        let off = Event::decode(&hex(CALENDAR[2].0)).unwrap();
        assert!(off.all_day() && off.tentative());
        assert_eq!(Event::decode(&hex("003a021e0004")), None);
    }

    #[test]
    fn request_vector() {
        // rpc-vectors.json "requests": id 8, calendar.events, args 00 02 05
        assert_eq!(calendar_request(8, 0, 2, 5).to_vec(), hex("5231510810000205"));
    }

    #[test]
    fn reply_vectors() {
        // rpc-vectors.json "replies": (message, id, status, index, total, record)
        let cases = [
            ("52314107000000", 7, OK, 0, 0, ""),
            ("52314107040000", 7, ASK, 0, 0, ""),
            ("52314108000001003a021e000400f8", 8, OK, 0, 1, "003a021e000400f8"),
            ("52314109000002c8000800", 9, OK, 0, 2, "c8000800"),
            ("523141090001027b2274223a32317d", 9, OK, 1, 2, "7b2274223a32317d"),
        ];
        for (m, id, status, index, total, record) in cases {
            let m = hex(m);
            let r = reply(&m).unwrap();
            assert_eq!((r.id, r.status, r.index, r.total, r.record), (id, status, index, total, &hex(record)[..]));
        }
        // "requests": a request and a non-RPC message are not replies
        assert_eq!(reply(&hex("5231510701")), None);
        assert_eq!(reply(&hex("5231500100")), None);
        assert_eq!(reply(&hex("523141")), None);
    }
}
