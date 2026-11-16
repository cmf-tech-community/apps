// The game itself: no Api, so `cargo test` runs it on the host. Fixed point only (the watch has no FPU): the ball's
// position and speed are in 1/256 px, angles in u16 turns (65536 = a full turn, 0 = 3 o'clock, clockwise, as the
// kit's `arc` and `angle` use them). One `step` is one 33 ms tick.

use cmf_wp2_ui::motion::{angle, cos, isqrt, sin};

/// Screen size and centre; the panel is a disc of radius `C`.
pub const SIZE: i32 = 466;
pub const C: i32 = SIZE / 2;

/// Fixed-point one for the ball's position and speed.
pub const Q: i32 = 256;
/// The ball's radius, px.
pub const BALL_R: i32 = 10;
/// The rail the paddle rides on: a ring from `RAIL_R0` to `RAIL_R1` px from the centre. The ball bounces off its
/// inner edge where the paddle is, and flies through it everywhere else.
pub const RAIL_R0: i32 = 200;
pub const RAIL_R1: i32 = 214;
/// The run ends once the ball is this far out: past the screen's edge, so you see it go.
pub const OUT_R: i32 = C + BALL_R + 4;

/// Ball speed per tick (Q): the first serve, the gain per point and the cap.
pub const SPEED0: i32 = 4 * Q;
pub const SPEED_STEP: i32 = 48;
pub const SPEED_MAX: i32 = 10 * Q;

/// The paddle's length in turns: about 51° at the start, 110/65536 shorter per point, never under 30°.
pub const SPAN0: i32 = 65536 / 7;
pub const SPAN_STEP: i32 = 110;
pub const SPAN_MIN: i32 = 65536 / 12;

/// One crown detent moves the paddle by 8°.
pub const CROWN_STEP: i32 = 1456;
/// A tap sends the paddle toward the tapped side at 4° a tick. Taps this close to the centre don't steer.
pub const STEER: i32 = 728;
pub const STEER_DEAD: i32 = 60;

/// Where on the paddle the ball lands tilts the bounce by up to 30°; the bounce never leaves more than 60° off
/// the line through the centre, so the ball can't skim round the rail forever.
pub const ENGLISH: i32 = 5461;
pub const MAX_TILT: i32 = 10923;
/// The ball's own half-width as an angle at the rail, so a ball that clips the paddle's end still counts.
const BALL_ANGLE: i32 = (BALL_R * 65536) / (RAIL_R0 * 314 / 50);

/// The paddle starts at 6 o'clock.
pub const HOME: u16 = 16384;

/// Ready: the ball waits in the middle for a tap. Over: the ball got out, the screen holds still.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Ready,
    Playing,
    Over,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Nothing,
    Scored,
    Crashed,
}

#[derive(Clone)]
pub struct Game {
    pub state: State,
    /// The ball's centre relative to the screen's centre, Q.
    pub x: i32,
    pub y: i32,
    /// Its velocity per tick, Q.
    pub vx: i32,
    pub vy: i32,
    pub speed: i32,
    /// The paddle's centre, turns.
    pub paddle: u16,
    /// Where a tap asked the paddle to go.
    pub target: Option<u16>,
    pub score: u32,
    /// The ball went past the paddle: no more bounces this run.
    missed: bool,
    rng: u32,
}

/// xorshift32: no random source on the watch, the app seeds it from the clock.
fn next(rng: &mut u32) -> u32 {
    let mut x = *rng;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *rng = x;
    x
}

/// `a - b` as a signed angle, -32768..=32767.
pub fn diff(a: u16, b: u16) -> i32 {
    a.wrapping_sub(b) as i16 as i32
}

impl Game {
    pub fn new(seed: u32) -> Game {
        let mut g = Game {
            state: State::Ready,
            x: 0,
            y: 0,
            vx: 0,
            vy: 0,
            speed: SPEED0,
            paddle: HOME,
            target: None,
            score: 0,
            missed: false,
            rng: seed | 1, // xorshift sticks at 0
        };
        g.restart();
        g
    }

    /// Back to Ready with the ball in the middle, keeping the random sequence going.
    pub fn restart(&mut self) {
        self.state = State::Ready;
        (self.x, self.y, self.vx, self.vy) = (0, 0, 0, 0);
        self.speed = SPEED0;
        self.paddle = HOME;
        self.target = None;
        self.score = 0;
        self.missed = false;
    }

    /// Serve: the ball leaves the centre in a random direction.
    pub fn launch(&mut self) {
        if self.state != State::Ready {
            return;
        }
        self.state = State::Playing;
        let a = next(&mut self.rng) as u16;
        self.aim(a);
    }

    fn aim(&mut self, a: u16) {
        self.vx = (self.speed * cos(a)) >> 14;
        self.vy = (self.speed * sin(a)) >> 14;
    }

    /// The paddle's length now, turns.
    pub fn span(&self) -> i32 {
        (SPAN0 - SPAN_STEP * self.score.min(1000) as i32).max(SPAN_MIN)
    }

    /// The ball's centre on screen, px.
    pub fn ball(&self) -> (i32, i32) {
        (C + ((self.x + Q / 2) >> 8), C + ((self.y + Q / 2) >> 8))
    }

    /// The ball's distance from the centre, 1/16 px.
    pub fn dist16(&self) -> i32 {
        let (x, y) = (self.x >> 4, self.y >> 4);
        isqrt(x * x + y * y)
    }

    /// A crown detent: +1 moves the paddle clockwise. It overrides a tap that is still steering.
    pub fn crown(&mut self, delta: i16) {
        self.paddle = self.paddle.wrapping_add((delta as i32 * CROWN_STEP) as u16);
        self.target = None;
    }

    /// A tap at screen point (x, y) while playing: the paddle heads for that side of the ring.
    pub fn steer(&mut self, x: i32, y: i32) {
        let (dx, dy) = (x - C, y - C);
        if dx * dx + dy * dy >= STEER_DEAD * STEER_DEAD {
            self.target = Some(angle(dx, dy));
        }
    }

    pub fn step(&mut self) -> Step {
        if self.state != State::Playing {
            return Step::Nothing;
        }
        if let Some(t) = self.target {
            let d = diff(t, self.paddle);
            if d.abs() <= STEER {
                self.paddle = t;
                self.target = None;
            } else {
                self.paddle = self.paddle.wrapping_add((d.signum() * STEER) as u16);
            }
        }
        self.x += self.vx;
        self.y += self.vy;
        let d = self.dist16();
        let outward = (self.x >> 4) * self.vx + (self.y >> 4) * self.vy > 0;
        if !self.missed && outward && d + BALL_R * 16 >= RAIL_R0 * 16 {
            let at = angle(self.x >> 4, self.y >> 4);
            let off = diff(at, self.paddle);
            let half = self.span() / 2;
            if off.abs() > half + BALL_ANGLE {
                self.missed = true;
            } else {
                // Mirror the incoming direction about the radius, then tilt it by where it hit the paddle.
                let phi = diff(angle(self.vx, self.vy), at);
                let english = (off * ENGLISH / half).clamp(-ENGLISH, ENGLISH);
                let tilt = (english - phi).clamp(-MAX_TILT, MAX_TILT);
                self.score += 1;
                self.speed = (self.speed + SPEED_STEP).min(SPEED_MAX);
                self.aim(at.wrapping_add(32768).wrapping_add(tilt as u16));
                // Put the ball back against the rail so it can't hit twice.
                let r = (RAIL_R0 - BALL_R) * 16;
                if d > r {
                    self.x = self.x * r / d;
                    self.y = self.y * r / d;
                }
                return Step::Scored;
            }
        }
        if d > OUT_R * 16 {
            self.state = State::Over;
            return Step::Crashed;
        }
        Step::Nothing
    }
}

/// The best score as the storage blob: "OB1" and a u32, little endian.
pub fn encode(best: u32) -> [u8; 7] {
    let b = best.to_le_bytes();
    [b'O', b'B', b'1', b[0], b[1], b[2], b[3]]
}

/// A blob that isn't ours reads as no best score yet.
pub fn decode(blob: &[u8]) -> u32 {
    match blob {
        [b'O', b'B', b'1', a, b, c, d] => u32::from_le_bytes([*a, *b, *c, *d]),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing(seed: u32) -> Game {
        let mut g = Game::new(seed);
        g.launch();
        g
    }

    #[test]
    fn serve_leaves_the_centre_at_the_start_speed() {
        for seed in 1..50 {
            let g = playing(seed);
            assert_eq!(g.state, State::Playing);
            let v = isqrt(g.vx * g.vx + g.vy * g.vy);
            assert!((v - SPEED0).abs() <= 2, "speed {v}");
        }
    }

    #[test]
    fn crown_moves_and_wraps() {
        let mut g = Game::new(7);
        g.crown(1);
        assert_eq!(g.paddle, HOME + CROWN_STEP as u16);
        g.paddle = 100;
        g.crown(-1);
        assert_eq!(g.paddle, 100u16.wrapping_sub(CROWN_STEP as u16));
    }

    #[test]
    fn tap_steers_the_short_way_and_crown_cancels_it() {
        let mut g = playing(3);
        g.paddle = 0; // 3 o'clock
        g.steer(C, C - 150); // 12 o'clock: counter-clockwise is shorter
        g.step();
        assert_eq!(g.paddle, 0u16.wrapping_sub(STEER as u16));
        g.steer(C + 10, C); // the dead zone in the middle doesn't change the target
        assert_eq!(g.target, Some(49152));
        g.crown(1);
        assert_eq!(g.target, None);
    }

    #[test]
    fn steering_arrives_and_stops() {
        let mut g = playing(3);
        g.paddle = 0;
        g.steer(C - 150, C); // 9 o'clock
        for _ in 0..60 {
            g.step();
            if g.state != State::Playing {
                break;
            }
        }
        assert_eq!(g.paddle, 32768);
        assert_eq!(g.target, None);
    }

    #[test]
    fn a_hit_scores_speeds_up_and_sends_the_ball_inward() {
        let mut g = playing(11);
        let mut hits = 0;
        for _ in 0..400 {
            g.paddle = angle(g.x >> 4, g.y >> 4); // a perfect player
            let before = g.speed;
            if g.step() == Step::Scored {
                hits += 1;
                assert_eq!(g.speed, before + SPEED_STEP);
                assert!((g.x >> 4) * g.vx + (g.y >> 4) * g.vy < 0, "bounce goes back in");
                assert!(g.dist16() <= (RAIL_R0 - BALL_R) * 16);
            }
        }
        assert!(hits >= 3, "{hits} hits");
        assert_eq!(g.score, hits);
    }

    #[test]
    fn missing_ends_the_run_once() {
        let mut g = playing(5);
        let mut crashed = 0;
        for _ in 0..300 {
            g.paddle = angle(g.x >> 4, g.y >> 4).wrapping_add(32768); // always on the wrong side
            if g.step() == Step::Crashed {
                crashed += 1;
            }
        }
        assert_eq!((crashed, g.state, g.score), (1, State::Over, 0));
        assert_eq!(g.step(), Step::Nothing);
    }

    #[test]
    fn the_paddle_end_still_counts() {
        let mut g = playing(9);
        // Sit the paddle so the ball meets it just inside its end.
        for _ in 0..200 {
            let at = angle(g.x >> 4, g.y >> 4);
            g.paddle = at.wrapping_add((g.span() / 2 + BALL_ANGLE - 200) as u16);
            if g.step() == Step::Scored {
                return;
            }
            assert_eq!(g.state, State::Playing);
        }
        panic!("never reached the rail");
    }

    #[test]
    fn autopilot_runs_long_and_it_gets_harder() {
        for seed in [1, 42, 999, 123_456] {
            let mut g = playing(seed);
            for _ in 0..5000 {
                g.paddle = angle(g.x >> 4, g.y >> 4);
                g.step();
                assert_eq!(g.state, State::Playing, "seed {seed}");
                assert!(g.dist16() <= (RAIL_R0 - BALL_R) * 16 + SPEED_MAX / 16 + 16);
            }
            assert!(g.score > 60, "seed {seed}: {}", g.score);
            assert_eq!(g.speed, SPEED_MAX);
            assert_eq!(g.span(), SPAN_MIN);
        }
    }

    #[test]
    fn restart_keeps_the_dice_rolling() {
        let mut g = playing(77);
        let first = (g.vx, g.vy);
        g.restart();
        assert_eq!((g.state, g.score, g.ball()), (State::Ready, 0, (C, C)));
        g.launch();
        assert_ne!((g.vx, g.vy), first);
    }

    #[test]
    fn best_score_blob() {
        assert_eq!(decode(&encode(0)), 0);
        assert_eq!(decode(&encode(1234)), 1234);
        assert_eq!(decode(b"H1xxxxx"), 0);
        assert_eq!(decode(b"OB1"), 0);
        assert_eq!(decode(&[]), 0);
    }
}
