//! What the screens show per day, built from the phone's events (`model.rs`): how many events and up to
//! [`DOTS`] calendar colours. The phone answers for [`DAYS`] days from today, so days outside them have no marks.

use crate::date::Day;
use crate::model::{Model, Status, days};

pub const DAYS: usize = 7;
pub const DOTS: usize = 3;

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Mark {
    pub count: u8,
    /// RGB565 (as LVGL styles take it) of each calendar with an event that day, `dots` of them.
    pub colours: [i32; DOTS],
    pub dots: u8,
}

pub struct Marks {
    /// Day number (since 1970) of `days[0]`.
    from: i32,
    days: [Mark; DAYS],
    /// What the summary says instead of a count; `None` once the phone answered.
    pub note: Option<&'static str>,
}

pub fn day_no(d: Day) -> i32 {
    days(d.month.year as i32, d.month.month as u32, d.day as u32)
}

impl Marks {
    pub const fn empty() -> Marks {
        Marks { from: 0, days: [Mark { count: 0, colours: [0; DOTS], dots: 0 }; DAYS], note: Some("not synced") }
    }

    /// The marks for `today` at `wall` (watch minutes, `model::wall_min`).
    pub fn build(m: &Model, today: Day, wall: i32) -> Marks {
        let mut k = Marks::empty();
        k.from = day_no(today);
        k.note = match m.status {
            Status::Ask => Some("allow on your phone"),
            Status::Denied => Some("no calendar access"),
            _ if m.fetched.is_none() => Some("not synced"),
            _ => None,
        };
        // ponytail: an event marks the day it starts on; a multi-day one shows on its first day only
        for i in 0..m.n {
            let (day, _, _) = m.span(i, wall);
            let Some(d) = usize::try_from(day).ok().filter(|&d| d < DAYS) else { continue };
            let mk = &mut k.days[d];
            mk.count = mk.count.saturating_add(1);
            let c = m.events[i].color as i32;
            if (mk.dots as usize) < DOTS && !mk.colours[..mk.dots as usize].contains(&c) {
                mk.colours[mk.dots as usize] = c;
                mk.dots += 1;
            }
        }
        k
    }

    /// The day's mark; empty outside the phone's window.
    pub fn get(&self, d: Day) -> Mark {
        usize::try_from(day_no(d) - self.from).ok().and_then(|i| self.days.get(i)).copied().unwrap_or_default()
    }

    /// Every day of the window from `today` marked `mark`, for screen tests.
    #[cfg(test)]
    pub fn filled(today: Day, mark: Mark) -> Marks {
        Marks { from: day_no(today), days: [mark; DAYS], note: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::Month;
    use crate::model::wall_min;

    fn rec(day: i8, start: u16, color: u16) -> Vec<u8> {
        let mut r = vec![day as u8];
        r.extend(start.to_le_bytes());
        r.extend(30u16.to_le_bytes());
        r.push(4);
        r.extend(color.to_le_bytes());
        r
    }

    #[test]
    fn events_become_counts_and_dots() {
        let nov = Month::new(2026, 11).unwrap();
        let today = Day { month: nov, day: 17 };
        let wall = wall_min(2026, 11, 17, 8, 0);
        let mut m = Model::new();
        assert_eq!(Marks::build(&m, today, wall).note, Some("not synced"));
        m.ask_now(0);
        m.poll(0);
        let recs = [rec(0, 570, 0xF800), rec(0, 600, 0xF800), rec(0, 700, 0x07E0), rec(2, 60, 0x001F), rec(9, 0, 0xFFFF)];
        for (i, r) in recs.iter().enumerate() {
            let mut msg = vec![b'R', b'1', b'A', 1, 0, i as u8, recs.len() as u8];
            msg.extend(r);
            m.reply(&msg, 0, wall);
        }
        let k = Marks::build(&m, today, wall);
        assert_eq!(k.note, None);
        assert_eq!(k.get(today), Mark { count: 3, colours: [0xF800, 0x07E0, 0], dots: 2 });
        assert_eq!(k.get(Day { month: nov, day: 19 }).colours[0], 0x001F);
        // outside the window: before today, past 7 days
        assert_eq!(k.get(Day { month: nov, day: 16 }), Mark::default());
        assert_eq!(k.get(Day { month: nov, day: 26 }), Mark::default());
        // the same cache a day later: everything moved one day closer
        let k = Marks::build(&m, Day { month: nov, day: 18 }, wall + 1440);
        assert_eq!(k.get(Day { month: nov, day: 19 }).count, 1);
    }
}
