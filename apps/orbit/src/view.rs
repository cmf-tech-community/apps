// Drawing, through cmf-wp2-ui's Surface: the watch draws with `&Api`, the tests with `Fb`. There is one buffer and
// no double buffering, so a frame while playing repaints only what changed: the ball's old spot, the score if the
// ball crossed it, the rail if the paddle moved or the ball touched it, and the ball. Each frame comes out the same
// as drawing the whole screen again (the tests check that pixel for pixel).

use crate::game::{BALL_R, C, Game, RAIL_R0, RAIL_R1, SIZE, State};
use cmf_wp2_ui::draw::{arc, disc};
use cmf_wp2_ui::motion::isqrt;
use cmf_wp2_ui::text::{big, big_width, centre, int};
use cmf_wp2_ui::{Rgb, Surface};

const BG: Rgb = Rgb(0, 0, 0);
const TRACK: Rgb = Rgb(38, 38, 46);
/// CMF orange.
const PADDLE: Rgb = Rgb(255, 92, 0);
const BALL: Rgb = Rgb(255, 255, 255);
const WHITE: u16 = 0xFFFF;
const GREY: u16 = 0x8C71;
/// The score behind the ball while playing: there, but quiet.
const DIM: u16 = cmf_wp2_ui::color::rgb565(70, 70, 82);

/// The playing score: font scale and top edge, centred on the screen.
const SCORE_SCALE: i32 = 6;
const SCORE_Y: i32 = C - 4 * SCORE_SCALE;
/// The ball's old spot is cleared this far out, so its antialiased edge goes too.
const ERASE_R: i32 = BALL_R + 2;

/// What was on screen after the last frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Seen {
    state: State,
    ball: (i32, i32),
    paddle: u16,
    span: i32,
    score: u32,
}

fn seen(g: &Game) -> Seen {
    Seen { state: g.state, ball: g.ball(), paddle: g.paddle, span: g.span(), score: g.score }
}

fn rail(s: &mut impl Surface, g: &Game) {
    let span = g.span();
    arc(s, C, C, RAIL_R0, RAIL_R1, g.paddle.wrapping_sub((span / 2) as u16), span as u32, PADDLE, Some(TRACK), BG);
}

fn ball(s: &mut impl Surface, (x, y): (i32, i32)) {
    disc(s, x, y, BALL_R, BALL, BG);
}

/// The playing score's box, `[x0, y0, x1, y1)`.
fn score_box(score: u32) -> [i32; 4] {
    let mut buf = [0u8; 11];
    let w = big_width(int(score as i32, &mut buf), SCORE_SCALE);
    [C - w / 2, SCORE_Y, C - w / 2 + w, SCORE_Y + 8 * SCORE_SCALE]
}

fn score(s: &mut impl Surface, score: u32) {
    let mut buf = [0u8; 11];
    let b = score_box(score);
    big(s, b[0], b[1], int(score as i32, &mut buf), SCORE_SCALE, DIM);
}

/// The ball's antialiased edge or its cleared spot can reach the rail.
fn near_rail((x, y): (i32, i32)) -> bool {
    let (dx, dy) = (x - C, y - C);
    isqrt(dx * dx + dy * dy) + ERASE_R + 2 >= RAIL_R0
}

fn overlaps((x, y): (i32, i32), r: i32, b: [i32; 4]) -> bool {
    x + r + 1 >= b[0] && x - r - 1 < b[2] && y + r + 1 >= b[1] && y - r - 1 < b[3]
}

/// "best N" centred at `y`, or nothing before the first run.
fn best_line(s: &mut impl Surface, y: i32, best: u32, c: u16) {
    if best > 0 {
        let mut buf = [0u8; 16];
        buf[..5].copy_from_slice(b"best ");
        let mut digits = [0u8; 11];
        let d = int(best.min(9_999_999) as i32, &mut digits).as_bytes();
        buf[5..5 + d.len()].copy_from_slice(d);
        centre(s, C, y, core::str::from_utf8(&buf[..5 + d.len()]).unwrap_or(""), c);
    }
}

/// The whole screen.
pub fn full(s: &mut impl Surface, g: &Game, best: u32) {
    s.rect(0, 0, SIZE, SIZE, BG.rgb565());
    match g.state {
        State::Ready => {
            big(s, C - big_width("ORBIT", 4) / 2, 112, "ORBIT", 4, WHITE);
            centre(s, C, 296, "tap to start", WHITE);
            centre(s, C, 320, "crown steers", GREY);
            best_line(s, 352, best, GREY);
        }
        State::Playing => score(s, g.score),
        State::Over => {
            centre(s, C, 120, "GAME OVER", GREY);
            let mut buf = [0u8; 11];
            let text = int(g.score as i32, &mut buf);
            big(s, C - big_width(text, 8) / 2, C - 48, text, 8, WHITE);
            best_line(s, 280, best, GREY);
            centre(s, C, 310, "tap to play again", WHITE);
        }
    }
    rail(s, g);
    if g.state != State::Over {
        ball(s, g.ball());
    }
}

/// Draws the frame for `g` over what `last` says is on screen (None: nothing known, draw it all) and returns what is
/// on screen now. The caller `show`s.
pub fn frame(s: &mut impl Surface, g: &Game, best: u32, last: Option<Seen>) -> Seen {
    let now = seen(g);
    let Some(last) = last.filter(|l| l.state == now.state) else {
        full(s, g, best);
        return now;
    };
    let rail_moved = (last.paddle, last.span) != (now.paddle, now.span);
    if g.state != State::Playing {
        // Ready and Over hold still; only the crown moves the paddle.
        if rail_moved {
            rail(s, g);
        }
        return now;
    }
    let moved = last.ball != now.ball;
    if moved {
        disc(s, last.ball.0, last.ball.1, ERASE_R, BG, BG);
    }
    let boxed = score_box(now.score);
    let score_dirty = last.score != now.score || (moved && overlaps(last.ball, ERASE_R, boxed));
    if score_dirty {
        let b = score_box(last.score.max(now.score));
        s.rect(b[0], b[1], b[2] - b[0], b[3] - b[1], BG.rgb565());
        score(s, now.score);
    }
    let rail_dirty = rail_moved || (moved && near_rail(last.ball)) || near_rail(now.ball);
    if rail_dirty {
        rail(s, g);
    }
    if moved || score_dirty || rail_dirty {
        ball(s, now.ball);
    }
    now
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Step;
    use cmf_wp2_ui::Fb;
    use cmf_wp2_ui::motion::angle;

    const N: usize = (SIZE * SIZE) as usize;

    fn fresh(g: &Game, best: u32) -> Vec<u16> {
        let mut px = vec![0x1234u16; N];
        full(&mut Fb::new(&mut px), g, best);
        px
    }

    fn first_diff(a: &[u16], b: &[u16]) -> Option<(i32, i32)> {
        a.iter().zip(b).position(|(x, y)| x != y).map(|i| (i as i32 % SIZE, i as i32 / SIZE))
    }

    /// Plays `ticks` with `pilot` choosing the paddle, drawing each frame over the last, and checks every frame
    /// against a full redraw.
    fn check_run(seed: u32, ticks: usize, pilot: impl Fn(&Game, usize) -> Option<u16>) -> Game {
        let mut g = Game::new(seed);
        let mut px = vec![0u16; N];
        let mut seen = Some(frame(&mut Fb::new(&mut px), &g, 5, None));
        g.launch();
        for t in 0..ticks {
            if let Some(p) = pilot(&g, t) {
                g.paddle = p;
            }
            if t % 97 == 13 {
                g.crown(1);
            }
            let step = g.step();
            seen = Some(frame(&mut Fb::new(&mut px), &g, 5, seen));
            assert_eq!(first_diff(&px, &fresh(&g, 5)), None, "seed {seed} tick {t} ({step:?})");
            if g.state == State::Over {
                break;
            }
        }
        g
    }

    #[test]
    fn partial_frames_match_full_redraws_while_scoring() {
        let g = check_run(42, 900, |g, t| (t % 5 != 0).then(|| angle(g.x >> 4, g.y >> 4)));
        assert!(g.score > 5, "{}", g.score);
    }

    #[test]
    fn partial_frames_match_through_a_miss_and_game_over() {
        let g = check_run(7, 600, |g, _| Some(angle(g.x >> 4, g.y >> 4).wrapping_add(32768)));
        assert_eq!(g.state, State::Over);
    }

    #[test]
    fn crown_on_a_still_screen_redraws_only_the_rail() {
        let g0 = Game::new(1);
        let mut px = vec![0u16; N];
        let seen = frame(&mut Fb::new(&mut px), &g0, 12, None);
        let mut g = g0.clone();
        g.crown(-3);
        let mut fb = Fb::new(&mut px);
        frame(&mut fb, &g, 12, Some(seen));
        let rects = fb.rects;
        assert_eq!(first_diff(&px, &fresh(&g, 12)), None);
        let mut whole = vec![0u16; N];
        let mut fb = Fb::new(&mut whole);
        full(&mut fb, &g, 12);
        assert!(rects < fb.rects, "{rects} vs {}", fb.rects);
    }

    #[test]
    fn a_quiet_tick_costs_far_less_than_a_full_frame() {
        let mut g = Game::new(3);
        g.launch();
        let mut px = vec![0u16; N];
        let seen = frame(&mut Fb::new(&mut px), &g, 0, None);
        // A tick in open space: away from the rail and the score.
        let before = g.ball();
        assert_eq!(g.step(), Step::Nothing);
        assert_ne!(before, g.ball());
        let mut fb = Fb::new(&mut px);
        frame(&mut fb, &g, 0, Some(seen));
        let partial = fb.rects;
        let mut whole = vec![0u16; N];
        let mut fb = Fb::new(&mut whole);
        full(&mut fb, &g, 0);
        assert!(partial * 10 < fb.rects, "{partial} vs {}", fb.rects);
    }

    #[test]
    fn text_fits_inside_the_rail() {
        // Every text line of the still screens stays clear of the rail's inner edge.
        for (y, chars) in [(296, 12), (320, 12), (352, 12), (120, 9), (280, 12), (310, 17)] {
            let d = (y - C).abs().max((y + 16 - C).abs());
            let half = isqrt(RAIL_R0 * RAIL_R0 - d * d);
            assert!(8 * chars + 6 <= half, "line at {y}");
        }
    }
}
