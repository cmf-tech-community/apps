//! Pong on a circle: pure game logic, no widgets, no_std. Angles are degrees, 0 = right, 90 = down (screen y
//! points down), so a clockwise crown turn increases the angle. Coordinates are pixels from the screen centre.

pub const R_PADDLE: f32 = 205.0; // where the paddles are drawn
pub const R_HIT: f32 = 190.0; // ball centre this far out: the paddle is checked
pub const R_OUT: f32 = 224.0; // ball centre this far out: it got past
const COS2_HIT: f32 = 0.78; // cos(28 deg)^2: paddle half width plus ball radius
pub const WIN: u8 = 5;

// Classic Pong speed: the ball starts slow, gets faster with every paddle hit and never slows down inside a rally.
// A point resets it to SPEED0. The cap keeps it from tunnelling through a paddle (the hit band is 34 px deep).
const SPEED0: f32 = 4.0; // pixels per step at the serve
const HIT_GAIN: f32 = 0.067; // every paddle hit: speed x 1.067
const EDGE_GAIN: f32 = 0.22; // a hit with the very edge of the paddle adds up to this much more (x 1.287 in all)
const SPEED_MAX: f32 = 11.0;
const SERVE_STEPS: u16 = 45;
const OVER_STEPS: u16 = 90;

// the opponent ("medium")
const AI_MAX_STEP: f32 = 3.5; // degrees per step at most
const AI_RETARGET: u16 = 7; // it re-reads the ball only this often: reaction delay
const AI_NOISE: f32 = 3.5; // usual aim error, degrees
const AI_JERK_CHANCE: u32 = 10; // percent of reads that overshoot like a human
const AI_JERK: f32 = 10.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Player,
    Ai,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Serve(u16),
    Play,
    Over(u16),
}

#[derive(Clone, Copy, Default)]
pub struct Out {
    pub hit: bool,
    pub scored: bool,
}

pub fn sqrt(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut g = f32::from_bits((x.to_bits() >> 1) + 0x1fbd_1df5);
    for _ in 0..3 {
        g = 0.5 * (g + x / g);
    }
    g
}

/// sin of an angle in degrees (Bhaskara approximation, error < 0.002).
pub fn sin(deg: f32) -> f32 {
    let mut x = deg % 360.0;
    if x > 180.0 {
        x -= 360.0;
    } else if x < -180.0 {
        x += 360.0;
    }
    let (s, a) = if x < 0.0 { (-1.0, -x) } else { (1.0, x) };
    let p = a * (180.0 - a);
    s * 4.0 * p / (40500.0 - p)
}

pub fn cos(deg: f32) -> f32 {
    sin(deg + 90.0)
}

fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo { lo } else if v > hi { hi } else { v }
}

#[derive(Clone, Copy)]
pub struct Game {
    pub x: f32,
    pub y: f32,
    vx: f32,
    vy: f32,
    speed: f32,
    pub player: f32,
    pub ai: f32,
    /// who struck the ball last; the other side defends
    pub last: Side,
    pub score_player: u8,
    pub score_ai: u8,
    pub phase: Phase,
    ai_vel: f32,
    ai_tx: f32,
    ai_ty: f32,
    ai_bias: f32,
    ai_timer: u16,
    frame: u32,
    rng: u32,
}

impl Game {
    pub fn new(seed: u32) -> Game {
        Game {
            x: 0.0,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
            speed: SPEED0,
            player: 0.0,
            ai: 180.0,
            last: Side::Player,
            score_player: 0,
            score_ai: 0,
            phase: Phase::Serve(SERVE_STEPS),
            ai_vel: 0.0,
            ai_tx: -1.0,
            ai_ty: 0.0,
            ai_bias: 0.0,
            ai_timer: 0,
            frame: 0,
            rng: seed | 1,
        }
    }

    fn rand(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    /// Uniform in -1..1.
    fn unit(&mut self) -> f32 {
        (self.rand() % 2001) as f32 / 1000.0 - 1.0
    }

    pub fn winner(&self) -> Option<Side> {
        if self.score_player >= WIN {
            Some(Side::Player)
        } else if self.score_ai >= WIN {
            Some(Side::Ai)
        } else {
            None
        }
    }

    fn launch(&mut self) {
        // towards the defender (the side that did not strike last), within about 35 degrees of its paddle
        let toward = if self.last == Side::Player { self.ai } else { self.player };
        let a = toward + self.unit() * 35.0;
        self.speed = SPEED0;
        self.vx = cos(a) * self.speed;
        self.vy = sin(a) * self.speed;
        self.phase = Phase::Play;
    }

    /// Where the ball will cross the paddle circle, as a direction (unit vector).
    fn predict(&self) -> (f32, f32) {
        let (px, py, vx, vy) = (self.x, self.y, self.vx, self.vy);
        let v2 = vx * vx + vy * vy;
        let b = px * vx + py * vy;
        let c = px * px + py * py - R_PADDLE * R_PADDLE;
        let disc = b * b - v2 * c;
        if disc < 0.0 || v2 < 0.01 {
            let d = sqrt(px * px + py * py).max(1.0);
            return (px / d, py / d);
        }
        let t = (-b + sqrt(disc)) / v2;
        let (qx, qy) = (px + vx * t, py + vy * t);
        let d = sqrt(qx * qx + qy * qy).max(1.0);
        (qx / d, qy / d)
    }

    fn step_ai(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        if self.ai_timer == 0 {
            self.ai_timer = AI_RETARGET;
            if self.last == Side::Player && self.phase == Phase::Play {
                let (tx, ty) = self.predict();
                self.ai_tx = tx;
                self.ai_ty = ty;
                self.ai_bias = self.unit() * AI_NOISE;
                if self.rand() % 100 < AI_JERK_CHANCE {
                    let s = if self.unit() >= 0.0 { 1.0 } else { -1.0 };
                    self.ai_bias += s * AI_JERK; // overshoots, then corrects on the next read
                }
            } else {
                // not its ball: drift back to the left rim, wandering a little
                let wander = sin((self.frame % 120) as f32 * 3.0) * 25.0;
                self.ai_tx = cos(180.0 + wander);
                self.ai_ty = sin(180.0 + wander);
                self.ai_bias = 0.0;
            }
        } else {
            self.ai_timer -= 1;
        }
        let (pdx, pdy) = (cos(self.ai), sin(self.ai));
        let cross = pdx * self.ai_ty - pdy * self.ai_tx; // sin of the error
        let dot = pdx * self.ai_tx + pdy * self.ai_ty;
        let mut err = if dot < 0.0 {
            if cross >= 0.0 { 90.0 } else { -90.0 }
        } else {
            clamp(cross * 57.3, -90.0, 90.0)
        };
        err += self.ai_bias;
        let want = clamp(err, -AI_MAX_STEP, AI_MAX_STEP);
        self.ai_vel += (want - self.ai_vel) * 0.35;
        // a little hand shake
        self.ai += self.ai_vel + sin((self.frame % 360) as f32 * 13.0) * 0.25;
        if self.ai >= 360.0 {
            self.ai -= 360.0;
        } else if self.ai < 0.0 {
            self.ai += 360.0;
        }
    }

    /// One game step. `turn` is the crown movement in degrees since the last step.
    pub fn step(&mut self, turn: f32) -> Out {
        let mut out = Out::default();
        self.player += turn;
        while self.player >= 360.0 {
            self.player -= 360.0;
        }
        while self.player < 0.0 {
            self.player += 360.0;
        }
        self.step_ai();
        match self.phase {
            Phase::Serve(n) => {
                if n == 0 {
                    self.launch();
                } else {
                    self.phase = Phase::Serve(n - 1);
                }
            }
            Phase::Over(n) => {
                if n == 0 {
                    self.score_player = 0;
                    self.score_ai = 0;
                    self.last = Side::Player;
                    self.x = 0.0;
                    self.y = 0.0;
                    self.phase = Phase::Serve(SERVE_STEPS);
                } else {
                    self.phase = Phase::Over(n - 1);
                }
            }
            Phase::Play => {
                self.x += self.vx;
                self.y += self.vy;
                let d2 = self.x * self.x + self.y * self.y;
                let defender = if self.last == Side::Player { Side::Ai } else { Side::Player };
                if d2 >= R_HIT * R_HIT && self.x * self.vx + self.y * self.vy > 0.0 {
                    let pa = if defender == Side::Ai { self.ai } else { self.player };
                    let (pdx, pdy) = (cos(pa), sin(pa));
                    let dot = self.x * pdx + self.y * pdy;
                    if dot > 0.0 && dot * dot >= COS2_HIT * d2 {
                        self.bounce(pdx, pdy, d2, defender);
                        out.hit = true;
                    } else if d2 >= R_OUT * R_OUT {
                        // got past the defender: the striker scores
                        if defender == Side::Ai {
                            self.score_player += 1;
                        } else {
                            self.score_ai += 1;
                        }
                        out.scored = true;
                        self.x = 0.0;
                        self.y = 0.0;
                        self.phase = if self.winner().is_some() { Phase::Over(OVER_STEPS) } else { Phase::Serve(SERVE_STEPS) };
                        // the side that just conceded defends the next serve
                        self.last = if defender == Side::Ai { Side::Player } else { Side::Ai };
                    }
                }
            }
        }
        out
    }

    fn bounce(&mut self, pdx: f32, pdy: f32, d2: f32, defender: Side) {
        let d = sqrt(d2).max(1.0);
        let (nx, ny) = (self.x / d, self.y / d);
        let vn = self.vx * nx + self.vy * ny;
        self.vx -= 2.0 * vn * nx;
        self.vy -= 2.0 * vn * ny;
        // hitting with the edge of the paddle sends the ball along the rim to that side
        let off = pdx * ny - pdy * nx; // sin of the angle between ball and paddle centre
        self.vx += -ny * off * self.speed * 1.1;
        self.vy += nx * off * self.speed * 1.1;
        // every hit speeds the ball up a fixed percentage; the outer part of the paddle adds more
        let edge = ((off.abs() / 0.47 - 0.4) / 0.6).max(0.0).min(1.0);
        self.speed = (self.speed * (1.0 + HIT_GAIN + edge * EDGE_GAIN)).min(SPEED_MAX);
        let v = sqrt(self.vx * self.vx + self.vy * self.vy).max(0.01);
        self.vx = self.vx / v * self.speed;
        self.vy = self.vy / v * self.speed;
        self.x = nx * (R_HIT - 2.0);
        self.y = ny * (R_HIT - 2.0);
        self.last = defender;
        self.ai_timer = 0; // re-read the ball at once after any hit
    }
}
