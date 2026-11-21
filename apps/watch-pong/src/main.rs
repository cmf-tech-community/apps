//! Watch Pong: circular Pong on the watch's own LVGL widgets (same batch API as ui-gallery and Watch Music).
//! The paddles are rows of small round objects along the rim, the ball is one round object: moving one is a
//! single `align` command. The crown arrives as CROWN events (like the Watch Music volume), 25 ms steps come
//! from `api.now()`. Back quits.
#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]

#[cfg_attr(not(any(target_os = "none", wp2_sim)), allow(dead_code))]
mod game;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
mod device {
    use crate::game::{self, Game, Phase, Side};
    use cmf_wp2_app::lv::*;
    use cmf_wp2_app::stock::{self, CIRCLE, SCREEN, WHITE};
    use cmf_wp2_app::{Api, BUTTON, CROWN, PAUSE, RESUME};
    use cmf_wp2_ui::haptic::{Motor, TICK};

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();

    const F24: u8 = 1;
    const PADDLE_W: i32 = 14; // thickness of a paddle line
    const PADDLE_SPAN: u16 = 40; // its length in degrees (the game's hit zone is a little wider)
    const BALL: i32 = 18;
    // colours are RGB565
    const RED: i32 = 0xFA08;
    const BG: i32 = 0x0000; // true black: the AMOLED switches those pixels off
    const STEP_MS: u32 = 25;
    /// Degrees of paddle movement per crown click.
    const DEG_PER_CLICK: f32 = 7.0;

    struct View {
        player: Obj,
        ai: Obj,
        ball: Obj,
        score_player: Obj,
        score_ai: Obj,
        msg: Obj,
    }

    fn after(now: u32, t: u32) -> bool {
        (now.wrapping_sub(t) as i32) >= 0
    }

    type Flush<'a> = &'a mut dyn FnMut(&mut Batch) -> Result<(), LvError>;

    // bytes one object can take: flush first when fewer are left (the batch buffer is small)
    const GROUP: usize = 560;
    fn room(b: &mut Batch, flush: Flush) -> Result<(), LvError> {
        if b.bytes().len() + GROUP > BUF { flush(b) } else { Ok(()) }
    }

    fn round(b: &mut Batch, flush: Flush, parent: Obj, size: i32, colour: i32) -> Result<Obj, LvError> {
        room(b, flush)?;
        let o = b.create(Class::Obj, parent)?;
        b.size(o, size as _, size as _)?;
        b.style(o, Prop::Radius, 0, CIRCLE)?;
        b.style(o, Prop::BgColor, 0, colour)?;
        b.style(o, Prop::BgOpa, 0, 255)?;
        b.style(o, Prop::BorderWidth, 0, 0)?;
        b.flags(o, &[], &[Flag::Clickable, Flag::Scrollable])?;
        b.align(o, Align::Center, 0, 0)?;
        Ok(o)
    }

    /// A paddle: a thick arc along the rim with round ends. Only its track is drawn (like Watch Music's glyph arcs);
    /// moving it is one `angles` command.
    fn paddle(b: &mut Batch, flush: Flush, parent: Obj, colour: i32, at: f32) -> Result<Obj, LvError> {
        room(b, flush)?;
        let size = 2 * (game::R_PADDLE as i32 + PADDLE_W / 2);
        let a = b.create(Class::Arc, parent)?;
        b.size(a, size as _, size as _)?;
        b.align(a, Align::Center, 0, 0)?;
        b.angles(a, 0, PADDLE_SPAN as _, rotation(at) as _)?;
        b.value(a, 0, false)?;
        b.style(a, Prop::ArcWidth, 0, PADDLE_W)?;
        b.style(a, Prop::ArcRounded, 0, 1)?;
        b.style(a, Prop::ArcColor, 0, colour)?;
        b.style(a, Prop::ArcOpa, 0, 255)?;
        b.style(a, Prop::ArcOpa, stock::INDICATOR, 0)?;
        b.style(a, Prop::BgOpa, stock::KNOB, 0)?;
        b.flags(a, &[], &[Flag::Clickable, Flag::Scrollable])?;
        Ok(a)
    }

    /// Where the arc starts, in whole degrees, for a paddle centred on `at`.
    fn rotation(at: f32) -> i32 {
        let mut r = at as i32 - PADDLE_SPAN as i32 / 2;
        while r < 0 {
            r += 360;
        }
        r % 360
    }

    fn label(b: &mut Batch, flush: Flush, parent: Obj, colour: i32, y: i16) -> Result<Obj, LvError> {
        room(b, flush)?;
        let l = b.create(Class::Label, parent)?;
        b.style(l, Prop::TextFont, 0, F24 as i32)?;
        b.style(l, Prop::TextColor, 0, colour)?;
        b.text(l, " ")?;
        b.align(l, Align::Center, 0, y)?;
        Ok(l)
    }

    fn build(b: &mut Batch, flush: Flush) -> Result<View, LvError> {
        room(b, flush)?;
        // the page, built like Watch Music's player page: full screen, no padding, no scrolling
        let page = b.create(Class::Obj, ROOT)?;
        b.size(page, SCREEN, SCREEN)?;
        for p in [Prop::PadTop, Prop::PadBottom, Prop::PadLeft, Prop::PadRight, Prop::BorderWidth, Prop::Radius] {
            b.style(page, p, 0, 0)?;
        }
        b.style(page, Prop::BgColor, 0, BG)?;
        b.style(page, Prop::BgOpa, 0, 255)?;
        b.flags(page, &[], &[Flag::Scrollable])?;
        // the crown comes to the app as CROWN events; the page does not scroll
        b.knob(page)?;

        let score_ai = label(b, flush, page, RED, -80)?;
        let score_player = label(b, flush, page, WHITE as i32, 80)?;
        let msg = label(b, flush, page, WHITE as i32, 40)?;
        let ai = paddle(b, flush, page, RED, 180.0)?;
        let player = paddle(b, flush, page, WHITE as i32, 0.0)?;
        let ball = round(b, flush, page, BALL, WHITE as i32)?;
        Ok(View { player, ai, ball, score_player, score_ai, msg })
    }

    /// Moves a paddle to angle `at`.
    fn place(b: &mut Batch, arc: Obj, at: f32) -> Result<(), LvError> {
        b.angles(arc, 0, PADDLE_SPAN as _, rotation(at) as _)
    }

    fn main(api: &'static Api) -> i32 {
        let Some(lv) = api.lv() else { return -95 };
        let Some(mut b) = Batch::new() else { return -1 };
        let fonts = stock::font(&mut b).and_then(|_| b.font(F24, StockFont::F24M));
        if fonts.and_then(|_| b.submit(&lv)).is_err() {
            return 1;
        }
        let mut flush = |b: &mut Batch| b.submit(&lv);
        let v = match stock::theme(&mut b).and_then(|_| build(&mut b, &mut flush)) {
            Ok(v) => v,
            Err(_) => return 2,
        };
        if b.submit(&lv).is_err() {
            return 3;
        }
        api.own_back(true);

        let mut motor = Motor::new(api);
        let mut g = Game::new(api.now() | 1);
        let mut turn = 0.0f32;
        let mut paused = false;
        let mut next = api.now();
        let (mut shown_p, mut shown_a) = (-999.0f32, -999.0f32);
        let (mut shown_sp, mut shown_sa) = (255u8, 255u8);
        let mut shown_msg = 9u8; // 0 none, 1 win, 2 lose
        let mut shown_c = -1i32; // the ball is white when it is your turn to hit it and red when it is the watch's

        loop {
            motor.update();
            let now = api.now();
            let mut wait = if paused { u32::MAX } else { next.wrapping_sub(now).min(STEP_MS) };
            if let Some(w) = motor.wait() {
                wait = wait.min(w);
            }
            if let Some(e) = api.event(wait.max(1)) {
                match e.kind {
                    BUTTON => break,
                    PAUSE => {
                        paused = true;
                        motor.off();
                    }
                    RESUME => {
                        paused = false;
                        next = api.now();
                    }
                    CROWN => turn += e.delta as f32 * DEG_PER_CLICK,
                    _ => {}
                }
            }
            if paused {
                continue;
            }
            // fixed 25 ms steps from the clock (at most three if the loop fell behind)
            let mut steps = 0;
            let mut out_hit = false;
            while after(api.now(), next) && steps < 3 {
                let out = g.step(turn);
                turn = 0.0;
                out_hit |= out.hit || out.scored;
                next = next.wrapping_add(STEP_MS);
                steps += 1;
            }
            if steps == 0 {
                continue;
            }
            if steps == 3 && after(api.now(), next) {
                next = api.now();
            }
            if out_hit {
                motor.play(TICK);
            }

            let mut r = b.align(v.ball, Align::Center, g.x as i16, g.y as i16);
            // the colour of whoever has to hit it next: red after you hit it (the watch's turn), white after the watch hit it (yours)
            let c = if g.last == Side::Player { RED } else { WHITE as i32 };
            if c != shown_c {
                r = r.and_then(|_| b.style(v.ball, Prop::BgColor, 0, c));
                shown_c = c;
            }
            if (g.player - shown_p).abs() > 0.3 {
                r = r.and_then(|_| place(&mut b, v.player, g.player));
                shown_p = g.player;
            }
            if (g.ai - shown_a).abs() > 0.3 {
                r = r.and_then(|_| place(&mut b, v.ai, g.ai));
                shown_a = g.ai;
            }
            if g.score_player != shown_sp || g.score_ai != shown_sa {
                let mut buf = [0u8; 11];
                r = r.and_then(|_| b.text(v.score_player, cmf_wp2_ui::text::int(g.score_player as i32, &mut buf)));
                let mut buf = [0u8; 11];
                r = r.and_then(|_| b.text(v.score_ai, cmf_wp2_ui::text::int(g.score_ai as i32, &mut buf)));
                (shown_sp, shown_sa) = (g.score_player, g.score_ai);
            }
            let m = match (g.phase, g.winner()) {
                (Phase::Over(_), Some(Side::Player)) => 1,
                (Phase::Over(_), Some(Side::Ai)) => 2,
                _ => 0,
            };
            if m != shown_msg {
                r = r.and_then(|_| b.text(v.msg, [" ", "You win", "You lose"][m as usize]));
                shown_msg = m;
            }
            let _ = r.and_then(|_| b.submit(&lv));
        }
        motor.off();
        0
    }
}
