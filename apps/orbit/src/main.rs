#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]

#[cfg_attr(not(any(target_os = "none", wp2_sim)), allow(dead_code))]
mod game;
#[cfg_attr(not(any(target_os = "none", wp2_sim)), allow(dead_code))]
mod view;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
mod device {
    use crate::game::{Game, State, Step, decode, encode};
    use crate::view::frame;
    use cmf_wp2_app::{Api, BUTTON, CROWN, PAUSE, RESUME, TOUCH_DOWN};
    use cmf_wp2_ui::haptic::{DONE, Motor, TICK};

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();

    /// One game tick.
    const FRAME_MS: u32 = 33;
    /// After a slow frame, catch up at most this many ticks; beyond that the game slows down instead of jumping.
    const CATCH_UP: u32 = 3;
    /// Taps right after a miss don't restart: they were meant for the run that just ended.
    const RETRY_MS: u32 = 700;

    fn due(now: u32, at: u32) -> bool {
        (now.wrapping_sub(at) as i32) >= 0
    }

    // true on RESUME, false on BUTTON
    fn paused(api: &Api) -> bool {
        loop {
            match api.event(u32::MAX).map(|e| e.kind) {
                Some(RESUME) => return true,
                Some(BUTTON) => return false,
                _ => {}
            }
        }
    }

    fn seed(api: &Api) -> u32 {
        let t = api.time().map_or(0, |t| (t.minute as u32 * 60 + t.second as u32) * 1000 + t.millis as u32);
        api.now().wrapping_mul(2_654_435_761) ^ t
    }

    fn main(api: &'static Api) -> i32 {
        let mut screen = api;
        let mut motor = Motor::new(api); // dropped on return: the motor goes off
        let mut blob = [0u8; 16];
        let n = api.load(&mut blob);
        let mut best = if n > 0 { decode(&blob[..n as usize]) } else { 0 };
        let mut game = Game::new(seed(api));
        let mut seen = Some(frame(&mut screen, &game, best, None));
        api.show();
        let mut next = api.now().wrapping_add(FRAME_MS);
        let mut over_at = 0u32;
        loop {
            motor.update();
            let now = api.now();
            let still = game.state != State::Playing;
            let left = if still { u32::MAX } else { (next.wrapping_sub(now) as i32).max(0) as u32 };
            let wait = motor.wait().map_or(left, |m| m.min(left));
            if let Some(e) = api.event(wait) {
                match e.kind {
                    BUTTON => {
                        api.keep_screen_on(false);
                        return 0;
                    }
                    PAUSE => {
                        motor.off();
                        api.keep_screen_on(false);
                        if !paused(api) {
                            return 0;
                        }
                        seen = Some(frame(&mut screen, &game, best, None));
                        api.show();
                        next = api.now().wrapping_add(FRAME_MS);
                    }
                    TOUCH_DOWN => {
                        let now = api.now();
                        match game.state {
                            State::Playing => game.steer(e.x as i32, e.y as i32),
                            State::Over if !due(now, over_at.wrapping_add(RETRY_MS)) => {}
                            state => {
                                if state == State::Over {
                                    game.restart(); // Ready keeps the paddle where the crown put it
                                }
                                game.launch();
                                seen = Some(frame(&mut screen, &game, best, seen));
                                api.show();
                                api.keep_screen_on(true);
                                next = now.wrapping_add(FRAME_MS);
                            }
                        }
                    }
                    CROWN => {
                        game.crown(e.delta);
                        if still {
                            seen = Some(frame(&mut screen, &game, best, seen));
                            api.show();
                        }
                    }
                    _ => {}
                }
                continue;
            }
            let now = api.now();
            if still || !due(now, next) {
                continue; // the motor's deadline, not a tick
            }
            for _ in 0..CATCH_UP {
                match game.step() {
                    Step::Scored => motor.play(TICK),
                    Step::Crashed => {
                        motor.play(DONE);
                        over_at = now;
                        if game.score > best {
                            best = game.score;
                            api.save(&encode(best)); // on change: the app can be ended without notice
                        }
                    }
                    Step::Nothing => {}
                }
                next = next.wrapping_add(FRAME_MS);
                if game.state != State::Playing || !due(now, next) {
                    break;
                }
            }
            if due(now, next) {
                next = now.wrapping_add(FRAME_MS);
            }
            seen = Some(frame(&mut screen, &game, best, seen));
            api.show();
            if game.state != State::Playing {
                api.keep_screen_on(false);
            }
        }
    }
}
