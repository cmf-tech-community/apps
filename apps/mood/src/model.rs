//! A year of mood levels and the date arithmetic to index it. Pure logic, host-tested.

/// No entry, then four levels. Level 0 is "untouched", so a tap always starts at 1.
pub const LEVELS: u8 = 5;
/// 366 days: a leap year fits.
pub const YEAR_LEN: usize = 366;
/// `"MD"`, version, year (u16 LE), then one level per day of the year.
pub const BLOB_LEN: usize = 2 + 1 + 2 + YEAR_LEN;

pub const MONTHS: [&str; 12] =
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

pub fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

pub fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Day of the year, 0-based, or `None` for a date that does not exist.
pub fn day_index(year: i32, month: u8, day: u8) -> Option<usize> {
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return None;
    }
    let mut n = 0;
    for m in 1..month {
        n += days_in_month(year, m) as usize;
    }
    Some(n + day as usize - 1)
}

/// 0 = Sunday .. 6 = Saturday (Sakamoto's method, proleptic Gregorian).
pub fn weekday(year: i32, month: u8, day: u8) -> u8 {
    const OFFSETS: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut y = year;
    if month < 3 {
        y -= 1;
    }
    let w = (y + y / 4 - y / 100 + y / 400 + OFFSETS[(month as usize).clamp(1, 12) - 1] + day as i32) % 7;
    w.rem_euclid(7) as u8
}

/// One year of levels. Out-of-range days read as 0 and are ignored on write.
#[derive(Clone, Copy)]
pub struct Year {
    pub year: i32,
    days: [u8; YEAR_LEN],
}

impl Year {
    pub fn new(year: i32) -> Self {
        Self { year, days: [0; YEAR_LEN] }
    }

    pub fn get(&self, month: u8, day: u8) -> u8 {
        day_index(self.year, month, day).map(|i| self.days[i]).unwrap_or(0)
    }

    /// Cycles 0 -> 1 -> 2 -> 3 -> 4 -> 0. Returns the new level, or 0 for a date that does not exist.
    pub fn cycle(&mut self, month: u8, day: u8) -> u8 {
        let Some(i) = day_index(self.year, month, day) else { return 0 };
        self.days[i] = (self.days[i] + 1) % LEVELS;
        self.days[i]
    }

    #[cfg(test)]
    pub fn set(&mut self, month: u8, day: u8, level: u8) {
        if let Some(i) = day_index(self.year, month, day) {
            self.days[i] = level % LEVELS;
        }
    }

    pub fn blob(&self) -> [u8; BLOB_LEN] {
        let mut b = [0u8; BLOB_LEN];
        b[0] = b'M';
        b[1] = b'D';
        b[2] = 1;
        b[3..5].copy_from_slice(&(self.year as u16).to_le_bytes());
        b[5..].copy_from_slice(&self.days);
        b
    }

    /// The stored year and days. `None` when the blob is not ours; a blob from another year is
    /// accepted but comes back empty for the requested year.
    pub fn from_blob(blob: &[u8]) -> Option<Self> {
        if blob.len() != BLOB_LEN || blob[0..2] != *b"MD" || blob[2] != 1 {
            return None;
        }
        let year = i32::from(u16::from_le_bytes([blob[3], blob[4]]));
        let mut days = [0u8; YEAR_LEN];
        for (slot, byte) in days.iter_mut().zip(&blob[5..]) {
            *slot = byte % LEVELS;
        }
        Some(Self { year, days })
    }

    /// Keeps this year's levels, and takes `other`'s when it is the same year. Returns whether it took them.
    pub fn adopt(&mut self, other: &Self) -> bool {
        if other.year != self.year {
            return false;
        }
        self.days = other.days;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_lengths_and_leap_years() {
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(1900, 2), 28); // centuries are not leap years
        assert_eq!(days_in_month(2000, 2), 29); // unless divisible by 400
        assert_eq!(days_in_month(2026, 4), 30);
        assert_eq!(days_in_month(2026, 12), 31);
        assert_eq!(days_in_month(2026, 13), 0);
    }

    #[test]
    fn day_index_counts_from_january_first() {
        assert_eq!(day_index(2026, 1, 1), Some(0));
        assert_eq!(day_index(2026, 3, 1), Some(59)); // 31 + 28
        assert_eq!(day_index(2024, 3, 1), Some(60)); // leap year
        assert_eq!(day_index(2026, 12, 31), Some(364));
        assert_eq!(day_index(2024, 12, 31), Some(365));
        assert_eq!(day_index(2026, 2, 29), None);
        assert_eq!(day_index(2026, 4, 31), None);
        assert_eq!(day_index(2026, 0, 1), None);
    }

    #[test]
    fn weekdays_match_known_dates() {
        assert_eq!(weekday(2000, 1, 1), 6); // Saturday
        assert_eq!(weekday(2024, 2, 29), 4); // Thursday
        assert_eq!(weekday(2026, 11, 16), 1); // Monday
        assert_eq!(weekday(1970, 1, 1), 4); // Thursday
    }

    #[test]
    fn levels_cycle_back_to_empty() {
        let mut y = Year::new(2026);
        assert_eq!(y.get(11, 16), 0);
        for want in [1, 2, 3, 4, 0] {
            assert_eq!(y.cycle(11, 16), want);
        }
        y.set(11, 16, 3);
        assert_eq!(y.get(11, 16), 3);
        assert_eq!(y.cycle(2, 30), 0); // no such day, nothing written
    }

    #[test]
    fn the_blob_round_trips_and_rejects_foreign_data() {
        let mut y = Year::new(2026);
        y.set(1, 1, 4);
        y.set(12, 31, 1);
        let blob = y.blob();
        assert_eq!(blob.len(), BLOB_LEN);

        let back = Year::from_blob(&blob).unwrap();
        assert_eq!(back.year, 2026);
        assert_eq!(back.get(1, 1), 4);
        assert_eq!(back.get(12, 31), 1);

        assert!(Year::from_blob(b"").is_none());
        assert!(Year::from_blob(&[b'X'; BLOB_LEN]).is_none());
        let mut bad = blob;
        bad[2] = 9; // other version
        assert!(Year::from_blob(&bad).is_none());
    }

    #[test]
    fn another_year_starts_empty() {
        let mut old = Year::new(2025);
        old.set(5, 5, 2);
        let mut new = Year::new(2026);
        assert!(!new.adopt(&old));
        assert_eq!(new.get(5, 5), 0);
        let mut same = Year::new(2025);
        assert!(same.adopt(&old));
        assert_eq!(same.get(5, 5), 2);
    }
}
