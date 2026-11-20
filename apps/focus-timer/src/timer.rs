//! Focus timer state: a whole-minute duration, the countdown and the end-of-session buzz.
//! Pure logic, no SDK calls, so it is host-tested (`cargo test -p focus-timer`).

/// Where the session is: choosing a length, counting down, or finished.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Idle,
    Running,
    Done,
}

pub const DEFAULT_S: u32 = 25 * 60;
pub const MIN_S: u32 = 60;
/// 99 minutes keeps the countdown at five glyphs (`99:00`).
pub const MAX_S: u32 = 99 * 60;
/// One crown detent, and one saved step, is one minute.
pub const STEP_S: u32 = 60;

/// End-of-session pattern: this many pulses, each on for [`PULSE_MS`].
pub const PULSES: u8 = 3;
pub const PULSE_MS: u32 = 250;
pub const BUZZ_DUTY: u16 = 800;
pub const BUZZ_HZ: u16 = 31;

#[derive(Clone, Copy, Debug)]
pub struct Focus {
    pub total_s: u32,
    pub phase: Phase,
    ends_at: u32,
}

fn clamp_s(s: i64) -> u32 {
    s.clamp(MIN_S as i64, MAX_S as i64) as u32
}

impl Focus {
    pub fn new(total_s: u32) -> Self {
        Self { total_s: clamp_s(total_s as i64), phase: Phase::Idle, ends_at: 0 }
    }

    /// Crown detent: `minutes` may be negative. Only while idle or done. Returns whether the length changed.
    pub fn adjust(&mut self, minutes: i32) -> bool {
        if self.phase == Phase::Running {
            return false;
        }
        let next = clamp_s(self.total_s as i64 + minutes as i64 * STEP_S as i64);
        if next == self.total_s && self.phase == Phase::Idle {
            return false;
        }
        self.total_s = next;
        self.phase = Phase::Idle;
        true
    }

    pub fn start(&mut self, now: u32) {
        self.ends_at = now.wrapping_add(self.total_s.saturating_mul(1000));
        self.phase = Phase::Running;
    }

    /// Stops the clock and keeps what is left, so tapping again resumes from there.
    pub fn pause(&mut self, now: u32) {
        if self.phase == Phase::Running {
            self.total_s = clamp_s(self.remaining_s(now) as i64);
            self.phase = Phase::Idle;
        }
    }

    /// Back to the chosen length without starting.
    pub fn reset(&mut self) {
        self.phase = Phase::Idle;
    }

    /// Whole seconds left, rounded up so the last second is shown as `00:01`, not `00:00`.
    pub fn remaining_s(&self, now: u32) -> u32 {
        if self.phase != Phase::Running {
            return self.total_s;
        }
        let ms = self.ends_at.wrapping_sub(now) as i32; // signed compare survives the 49-day wrap
        if ms <= 0 {
            0
        } else {
            (ms as u32).div_ceil(1000)
        }
    }

    /// Arc value 0..=100: full at the start of a session, empty when it is done.
    pub fn arc(&self, now: u32) -> i32 {
        match self.phase {
            Phase::Idle => 100,
            Phase::Done => 0,
            Phase::Running => {
                let left = self.remaining_s(now).min(self.total_s);
                // rounded up: a ring that still shows a sliver until the very end
                ((left * 100).div_ceil(self.total_s.max(1))) as i32
            }
        }
    }

    /// `MM:SS` into `buf`. `total_s` is clamped to 99 minutes, so five bytes always fit.
    pub fn label<'a>(&self, now: u32, buf: &'a mut [u8; 5]) -> &'a str {
        let s = match self.phase {
            Phase::Done => 0,
            _ => self.remaining_s(now).min(MAX_S),
        };
        let (m, sec) = (s / 60, s % 60);
        buf[0] = b'0' + (m / 10) as u8;
        buf[1] = b'0' + (m % 10) as u8;
        buf[2] = b':';
        buf[3] = b'0' + (sec / 10) as u8;
        buf[4] = b'0' + (sec % 10) as u8;
        core::str::from_utf8(buf).unwrap_or("--:--")
    }

    /// The word the button carries in this phase.
    pub fn action(&self) -> &'static str {
        match self.phase {
            Phase::Idle => "Start",
            Phase::Running => "Pause",
            Phase::Done => "Restart",
        }
    }

    /// The grey line under the title.
    pub fn hint(&self) -> &'static str {
        match self.phase {
            Phase::Idle => "crown: minutes",
            Phase::Running => "tap pause to stop",
            Phase::Done => "session done",
        }
    }
}

/// The pulse pattern after a finished session: on, gap, on, gap, on, then off.
/// Times are `api.now()` milliseconds, compared with the same signed-wrap rule as [`Focus`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Buzz {
    next_at: u32,
    left: u8,
    on: bool,
    active: bool,
}

impl Buzz {
    pub fn start(&mut self, now: u32) {
        self.left = PULSES;
        self.on = false;
        self.next_at = now;
        self.active = PULSES > 0;
    }

    pub fn stop(&mut self) {
        self.active = false;
    }

    /// The duty to apply now, or `None` when nothing is due. `Some(0)` switches the motor off.
    pub fn poll(&mut self, now: u32) -> Option<u16> {
        if !self.active || (now.wrapping_sub(self.next_at) as i32) < 0 {
            return None;
        }
        if self.on {
            self.on = false;
            self.next_at = now.wrapping_add(PULSE_MS);
            return Some(0);
        }
        if self.left == 0 {
            self.active = false;
            return Some(0);
        }
        self.left -= 1;
        self.on = true;
        self.next_at = now.wrapping_add(PULSE_MS);
        Some(BUZZ_DUTY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_steps_by_a_minute_and_clamps() {
        let mut f = Focus::new(DEFAULT_S);
        assert_eq!(f.total_s, 1500);
        assert!(f.adjust(1));
        assert_eq!(f.total_s, 1560);
        assert!(f.adjust(-1));
        assert_eq!(f.total_s, DEFAULT_S);
        for _ in 0..200 {
            f.adjust(-1);
        }
        assert_eq!(f.total_s, MIN_S);
        for _ in 0..200 {
            f.adjust(1);
        }
        assert_eq!(f.total_s, MAX_S);
        assert!(!f.adjust(1)); // already at the top
    }

    #[test]
    fn crown_is_ignored_while_running() {
        let mut f = Focus::new(600);
        f.start(1000);
        assert!(!f.adjust(5));
        assert_eq!(f.total_s, 600);
    }

    #[test]
    fn counts_down_and_finishes_on_zero() {
        let mut f = Focus::new(60);
        f.start(10_000);
        assert_eq!(f.remaining_s(10_000), 60);
        assert_eq!(f.remaining_s(10_500), 60); // rounded up
        assert_eq!(f.remaining_s(69_000), 1);
        assert_eq!(f.remaining_s(70_000), 0);
        assert_eq!(f.arc(10_000), 100);
        assert_eq!(f.arc(40_000), 50);
        assert!(f.arc(69_000) > 0); // a sliver stays until the last millisecond
    }

    #[test]
    fn pause_keeps_the_remainder_and_resumes() {
        let mut f = Focus::new(600);
        f.start(0);
        f.pause(90_000); // 8.5 minutes left, rounded up = 510 s
        assert_eq!(f.phase, Phase::Idle);
        assert_eq!(f.total_s, 510);
        f.start(100_000);
        assert_eq!(f.remaining_s(100_000), 510);
        assert_eq!(f.remaining_s(100_000 + 510_000), 0);
    }

    #[test]
    fn the_wrap_of_now_does_not_stop_the_clock() {
        let mut f = Focus::new(60);
        f.start(u32::MAX - 30_000); // ends after the wrap
        assert_eq!(f.remaining_s(0), 30);
        assert_eq!(f.remaining_s(30_000), 0);
    }

    #[test]
    fn label_is_always_five_glyphs() {
        let mut buf = [0u8; 5];
        let mut f = Focus::new(DEFAULT_S);
        assert_eq!(f.label(0, &mut buf), "25:00");
        f = Focus::new(99 * 60);
        assert_eq!(f.label(0, &mut buf), "99:00");
        f = Focus::new(60);
        f.start(0);
        assert_eq!(f.label(59_500, &mut buf), "00:01");
        assert_eq!(f.label(60_000, &mut buf), "00:00");
        f.phase = Phase::Done;
        assert_eq!(f.label(0, &mut buf), "00:00");
        assert_eq!(f.action(), "Restart");
    }

    #[test]
    fn buzz_is_three_pulses_then_off() {
        let mut b = Buzz::default();
        b.start(0);
        assert_eq!(b.poll(0), Some(BUZZ_DUTY)); // on
        assert_eq!(b.poll(100), None); // still inside the pulse
        assert_eq!(b.poll(250), Some(0)); // off
        assert_eq!(b.poll(500), Some(BUZZ_DUTY)); // second pulse
        assert_eq!(b.poll(750), Some(0));
        assert_eq!(b.poll(1000), Some(BUZZ_DUTY)); // third
        assert_eq!(b.poll(1250), Some(0));
        assert_eq!(b.poll(1500), Some(0)); // final off, not active any more
        assert!(!b.active);
        assert_eq!(b.poll(2000), None);
    }

    #[test]
    fn stop_cancels_the_rest() {
        let mut b = Buzz::default();
        b.start(0);
        assert_eq!(b.poll(0), Some(BUZZ_DUTY));
        b.stop();
        assert_eq!(b.poll(250), None);
    }
}
