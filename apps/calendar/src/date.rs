//! Gregorian dates for a month grid: weeks start on Monday, six rows of seven days.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Month {
    pub year: u16,
    /// 1..=12
    pub month: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Day {
    pub month: Month,
    /// 1..=31
    pub day: u8,
}

pub const NAMES: [&str; 12] = [
    "january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november",
    "december",
];
pub const SHORT: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
/// Monday first, as the grid.
pub const WEEKDAYS: [&str; 7] = ["mo", "tu", "we", "th", "fr", "sa", "su"];
pub const WEEKDAY_NAMES: [&str; 7] = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];
pub const ROWS: usize = 6;
pub const CELLS: usize = ROWS * 7;

pub fn leap(year: u16) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

impl Month {
    /// `None` for a month outside 1..=12 or a year outside 1970..=2199 (what the grid and the protocol accept).
    pub fn new(year: u16, month: u8) -> Option<Month> {
        ((1..=12).contains(&month) && (1970..=2199).contains(&year)).then_some(Month { year, month })
    }

    pub fn days(self) -> u8 {
        match self.month {
            2 if leap(self.year) => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        }
    }

    /// `n` months later (negative: earlier).
    pub fn add(self, n: i32) -> Month {
        let i = self.year as i32 * 12 + self.month as i32 - 1 + n;
        Month { year: i.div_euclid(12) as u16, month: (i.rem_euclid(12) + 1) as u8 }
    }

    /// Weekday of the 1st, 0 = Monday.
    pub fn first_weekday(self) -> u8 {
        weekday(Day { month: self, day: 1 })
    }

    pub fn name(self) -> &'static str {
        NAMES[self.month as usize - 1]
    }
}

/// 0 = Monday (Sakamoto's method).
pub fn weekday(d: Day) -> u8 {
    const T: [u16; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let m = d.month.month as usize;
    let y = d.month.year - (m < 3) as u16;
    let sunday0 = (y + y / 4 - y / 100 + y / 400 + T[m - 1] + d.day as u16) % 7;
    ((sunday0 + 6) % 7) as u8
}

/// The 42 days of `m`'s grid: the tail of the month before, `m`, the head of the next.
pub fn grid(m: Month) -> [Day; CELLS] {
    let lead = m.first_weekday() as i32;
    let prev = m.add(-1);
    let mut out = [Day { month: m, day: 1 }; CELLS];
    for (i, cell) in out.iter_mut().enumerate() {
        let n = i as i32 - lead + 1; // day number within m
        *cell = if n < 1 {
            Day { month: prev, day: (prev.days() as i32 + n) as u8 }
        } else if n > m.days() as i32 {
            Day { month: m.add(1), day: (n - m.days() as i32) as u8 }
        } else {
            Day { month: m, day: n as u8 }
        };
    }
    out
}

/// The Monday-to-Sunday week of `d`.
pub fn week(d: Day) -> [Day; 7] {
    let g = grid(d.month);
    let i = g.iter().position(|c| *c == d).unwrap_or(0);
    let start = i - i % 7;
    let mut out = [d; 7];
    out.copy_from_slice(&g[start..start + 7]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(y: u16, mo: u8) -> Month {
        Month::new(y, mo).unwrap()
    }

    #[test]
    fn month_lengths_and_leap_years() {
        assert_eq!(m(2026, 2).days(), 28);
        assert_eq!(m(2028, 2).days(), 29);
        assert_eq!(m(2100, 2).days(), 28);
        assert_eq!(m(2000, 2).days(), 29);
        assert_eq!(m(2026, 11).days(), 30);
        assert_eq!(m(2026, 12).days(), 31);
    }

    #[test]
    fn months_add_across_years() {
        assert_eq!(m(2026, 11).add(2), m(2027, 1));
        assert_eq!(m(2026, 1).add(-1), m(2025, 12));
        assert_eq!(m(2026, 1).add(-13), m(2024, 12));
        assert_eq!(m(2026, 5).add(0), m(2026, 5));
    }

    #[test]
    fn weekdays() {
        // 2026-11-15 is a Sunday, 2026-11-01 too; 2000-01-01 a Saturday; 2024-02-29 a Thursday
        assert_eq!(weekday(Day { month: m(2026, 11), day: 15 }), 6);
        assert_eq!(m(2026, 11).first_weekday(), 6);
        assert_eq!(m(2000, 1).first_weekday(), 5);
        assert_eq!(weekday(Day { month: m(2024, 2), day: 29 }), 3);
        assert_eq!(m(2026, 12).first_weekday(), 1);
    }

    #[test]
    fn november_2026_grid_matches_a_wall_calendar() {
        let g = grid(m(2026, 11));
        // starts on Monday 26 October, as the reference screenshot
        assert_eq!(g[0], Day { month: m(2026, 10), day: 26 });
        assert_eq!(g[6], Day { month: m(2026, 11), day: 1 });
        assert_eq!(g[34], Day { month: m(2026, 11), day: 29 });
        assert_eq!(g[35], Day { month: m(2026, 11), day: 30 });
        assert_eq!(g[41], Day { month: m(2026, 12), day: 6 });
        // every day of the month once, in order
        let days: std::vec::Vec<u8> = g.iter().filter(|d| d.month == m(2026, 11)).map(|d| d.day).collect();
        assert_eq!(days, (1..=30).collect::<std::vec::Vec<u8>>());
    }

    #[test]
    fn a_month_starting_on_monday_has_no_lead() {
        // June 2026 starts on a Monday
        assert_eq!(grid(m(2026, 6))[0], Day { month: m(2026, 6), day: 1 });
    }

    #[test]
    fn week_of_a_day() {
        let w = week(Day { month: m(2026, 11), day: 15 });
        assert_eq!(w[0], Day { month: m(2026, 11), day: 9 });
        assert_eq!(w[6], Day { month: m(2026, 11), day: 15 });
        let w = week(Day { month: m(2026, 12), day: 1 });
        assert_eq!(w[0], Day { month: m(2026, 11), day: 30 });
    }

    #[test]
    fn bad_months_are_refused() {
        assert!(Month::new(2026, 0).is_none());
        assert!(Month::new(2026, 13).is_none());
        assert!(Month::new(1969, 5).is_none());
    }
}
