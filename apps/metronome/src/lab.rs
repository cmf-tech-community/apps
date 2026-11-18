#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Field {
    Bpm,
    Beats,
    Strength,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Settings {
    pub bpm: u16,
    pub beats_per_bar: u8,
    pub strength_pct: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            bpm: 120,
            beats_per_bar: 4,
            strength_pct: 100,
        }
    }
}

fn stepped(value: i32, clicks: i16, step: i32, min: i32, max: i32) -> i32 {
    (value + clicks as i32 * step).clamp(min, max)
}

impl Settings {
    pub fn adjust(&mut self, field: Field, clicks: i16) {
        match field {
            Field::Bpm => self.bpm = stepped(self.bpm as i32, clicks, 1, 30, 260) as u16,
            Field::Beats => self.beats_per_bar = stepped(self.beats_per_bar as i32, clicks, 1, 1, 8) as u8,
            Field::Strength => self.strength_pct = stepped(self.strength_pct as i32, clicks, 10, 20, 100) as u16,
        }
    }

    pub fn adjust_bpm(&mut self, delta: i16) {
        self.bpm = (self.bpm as i32 + delta as i32).clamp(30, 260) as u16;
    }

    pub fn interval_ms(&self) -> u32 {
        60_000 / (self.bpm.max(1) as u32)
    }

    pub fn pulse_params(&self, beat_index: u8) -> (u16, u16, u32) {
        let is_accent = beat_index == 0 && self.beats_per_bar > 1;
        let base_permille = self.strength_pct * 10;
        if is_accent {
            (base_permille.clamp(150, 1000), 31, 75)
        } else {
            ((base_permille * 3 / 4).clamp(100, 900), 62, 45)
        }
    }
}

#[derive(Default)]
pub struct TapTracker {
    last_ms: Option<u32>,
    intervals: [u32; 4],
    count: usize,
    idx: usize,
}

impl TapTracker {
    pub fn tap(&mut self, now_ms: u32) -> Option<u16> {
        let Some(prev) = self.last_ms else {
            self.last_ms = Some(now_ms);
            return None;
        };
        let dt = now_ms.wrapping_sub(prev);
        self.last_ms = Some(now_ms);

        // Сброс, если пауза больше 2 секунд (< 30 BPM) или случайный дребезг (< 200 мс)
        if !(200..=2000).contains(&dt) {
            self.count = 0;
            self.idx = 0;
            return None;
        }

        self.intervals[self.idx] = dt;
        self.idx = (self.idx + 1) % self.intervals.len();
        if self.count < self.intervals.len() {
            self.count += 1;
        }

        let sum: u32 = self.intervals[..self.count].iter().sum();
        let avg = sum / self.count as u32;
        Some((60_000 / avg).clamp(30, 260) as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bpm_clamps_and_interval() {
        let mut s = Settings::default();
        assert_eq!(s.interval_ms(), 500);
        s.adjust_bpm(-500);
        assert_eq!(s.bpm, 30);
        s.adjust_bpm(500);
        assert_eq!(s.bpm, 260);
    }

    #[test]
    fn accent_on_first_beat() {
        let s = Settings { bpm: 120, beats_per_bar: 4, strength_pct: 100 };
        let (d0, f0, ms0) = s.pulse_params(0);
        let (d1, f1, ms1) = s.pulse_params(1);
        assert!(d0 > d1 && ms0 > ms1);
        assert_ne!(f0, f1);
    }

    #[test]
    fn tap_tempo_averages_and_resets() {
        let mut t = TapTracker::default();
        assert_eq!(t.tap(1000), None);
        assert_eq!(t.tap(1500), Some(120));
        assert_eq!(t.tap(2000), Some(120));
        // Пауза > 2000 мс сбрасывает серию
        assert_eq!(t.tap(5000), None);
        assert_eq!(t.tap(5400), Some(150));
    }
}
