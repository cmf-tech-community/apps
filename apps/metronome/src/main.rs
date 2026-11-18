#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]

mod lab;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
mod device {
    use crate::lab::{Field, Settings, TapTracker};
    use cmf_wp2_app::lv::{Align, Batch, Class, Ev, Flag, Lv, LvError, Obj, Prop, StockFont, ROOT};
    use cmf_wp2_app::stock::{self, ACCENT, BLACK, CIRCLE, GREY, PRESSED, ROW_BG, ROW_PRESSED, WHITE};
    use cmf_wp2_app::{Api, BUTTON, CROWN, LV, PAUSE, RESUME};
    use core::fmt::Write;

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();

    const FONT_BIG: u8 = 1;
    const FONT_SMALL: u8 = 2;
    const CLICKED: i16 = 7;
    const RED: i32 = 0xF800;
    const MAX_DOTS: usize = 8;

    struct Line {
        buf: [u8; 24],
        len: usize,
    }

    impl Line {
        fn new() -> Self {
            Line { buf: [0; 24], len: 0 }
        }
        fn as_str(&self) -> &str {
            core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
        }
    }

    impl Write for Line {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let n = s.len().min(self.buf.len() - self.len);
            self.buf[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
            self.len += n;
            Ok(())
        }
    }

    enum Screen {
        Main {
            root: Obj,
            arc: Obj,
            dots: [Obj; MAX_DOTS],
            bpm_val: Obj,
            btn_cfg: Obj,
            cfg_lbl: Obj,
            btn_minus: Obj,
            btn_tap: Obj,
            btn_plus: Obj,
            btn_play: Obj,
            play_lbl: Obj,
        },
        Settings {
            root: Obj,
            card_beats: Obj,
            beats_val: Obj,
            card_power: Obj,
            power_val: Obj,
            btn_done: Obj,
        },
    }

    fn make_container(b: &mut Batch) -> Result<Obj, LvError> {
        let root = b.create(Class::Obj, ROOT)?;
        b.size(root, 466, 466)?;
        for p in [Prop::PadTop, Prop::PadBottom, Prop::PadLeft, Prop::PadRight, Prop::BorderWidth, Prop::Radius, Prop::BgOpa] {
            b.style(root, p, 0, 0)?;
        }
        b.flags(root, &[], &[Flag::Scrollable])?;
        Ok(root)
    }

    fn make_pill(b: &mut Batch, parent: Obj, w: i32, h: i32, bg: i32, bg_press: i32, text: &str, fg: i32, font: Option<u8>) -> Result<(Obj, Obj), LvError> {
        let btn = b.create(Class::Obj, parent)?;
        b.size(btn, w, h)?;
        b.style(btn, Prop::BgColor, 0, bg)?;
        b.style(btn, Prop::BgColor, PRESSED, bg_press)?;
        b.style(btn, Prop::BgOpa, 0, 255)?;
        b.style(btn, Prop::Radius, 0, CIRCLE)?;
        b.style(btn, Prop::BorderWidth, 0, 0)?;
        b.flags(btn, &[], &[Flag::Scrollable])?;
        b.events(btn, &[Ev::Clicked])?;

        let lbl = b.create(Class::Label, btn)?;
        if let Some(f) = font {
            b.style(lbl, Prop::TextFont, 0, f as i32)?;
        }
        b.style(lbl, Prop::TextColor, 0, fg)?;
        b.text(lbl, text)?;
        b.align(lbl, Align::Center, 0, 0)?;
        Ok((btn, lbl))
    }

    fn build_main(b: &mut Batch, lv: &Lv, settings: &Settings, running: bool, beat: u8) -> Result<Screen, LvError> {
        b.clear();
        stock::theme(b)?;
        let root = make_container(b)?;

        let arc = b.create(Class::Arc, root)?;
        b.size(arc, 448, 448)?;
        b.align(arc, Align::Center, 0, 0)?;
        b.angles(arc, 0, 360, 270)?;
        b.value(arc, 0, false)?;
        stock::arc(b, arc, 10)?;
        b.flags(arc, &[], &[Flag::Clickable])?;

        let mut dots = [ROOT; MAX_DOTS];
        for d in dots.iter_mut() {
            let dot = b.create(Class::Obj, root)?;
            b.size(dot, 22, 10)?;
            b.style(dot, Prop::Radius, 0, CIRCLE)?;
            b.style(dot, Prop::BgColor, 0, ROW_BG)?;
            b.style(dot, Prop::BgOpa, 0, 255)?;
            b.style(dot, Prop::BorderWidth, 0, 0)?;
            b.flags(dot, &[], &[Flag::Clickable])?;
            *d = dot;
        }
        b.submit(lv)?;
        b.clear();

        let bpm_val = b.create(Class::Label, root)?;
        b.style(bpm_val, Prop::TextFont, 0, FONT_BIG as i32)?;
        b.style(bpm_val, Prop::TextColor, 0, WHITE)?;
        b.text(bpm_val, "120")?;
        b.align(bpm_val, Align::Center, 0, -76)?;

        let (btn_cfg, cfg_lbl) = make_pill(b, root, 210, 38, ROW_BG, ROW_PRESSED, "4/4  |  100%", GREY, Some(FONT_SMALL))?;
        b.align(btn_cfg, Align::Center, 0, -18)?;

        let (btn_minus, _) = make_pill(b, root, 88, 56, ROW_BG, ROW_PRESSED, "-5", WHITE, None)?;
        b.align(btn_minus, Align::Center, -108, 44)?;

        let (btn_tap, _) = make_pill(b, root, 108, 56, ROW_BG, ROW_PRESSED, "TAP", ACCENT, None)?;
        b.align(btn_tap, Align::Center, 0, 44)?;

        let (btn_plus, _) = make_pill(b, root, 88, 56, ROW_BG, ROW_PRESSED, "+5", WHITE, None)?;
        b.align(btn_plus, Align::Center, 108, 44)?;

        let (btn_play, play_lbl) = make_pill(b, root, 260, 68, ACCENT, ROW_PRESSED, "START", BLACK, None)?;
        b.align(btn_play, Align::Center, 0, 124)?;
        b.submit(lv)?;

        let screen = Screen::Main {
            root,
            arc,
            dots,
            bpm_val,
            btn_cfg,
            cfg_lbl,
            btn_minus,
            btn_tap,
            btn_plus,
            btn_play,
            play_lbl,
        };
        sync_screen(b, lv, &screen, settings, running, beat)?;
        Ok(screen)
    }

    fn build_settings(b: &mut Batch, lv: &Lv, settings: &Settings) -> Result<Screen, LvError> {
        b.clear();
        stock::theme(b)?;
        let root = make_container(b)?;

        let title = b.create(Class::Label, root)?;
        b.style(title, Prop::TextColor, 0, ACCENT)?;
        b.text(title, "Settings")?;
        b.align(title, Align::Center, 0, -148)?;

        let card_beats = stock::card(b, root, 86)?;
        b.size(card_beats, 360, 86)?;
        b.style(card_beats, Prop::BgColor, PRESSED, ROW_PRESSED)?;
        b.events(card_beats, &[Ev::Clicked])?;
        b.align(card_beats, Align::Center, 0, -64)?;

        let lbl_b = b.create(Class::Label, card_beats)?;
        b.text(lbl_b, "Beats")?;
        b.align(lbl_b, Align::LeftMid, 28, 0)?;

        let beats_val = b.create(Class::Label, card_beats)?;
        b.style(beats_val, Prop::TextColor, 0, ACCENT)?;
        b.text(beats_val, "4/4")?;
        b.align(beats_val, Align::RightMid, -28, 0)?;

        let card_power = stock::card(b, root, 86)?;
        b.size(card_power, 360, 86)?;
        b.style(card_power, Prop::BgColor, PRESSED, ROW_PRESSED)?;
        b.events(card_power, &[Ev::Clicked])?;
        b.align(card_power, Align::Center, 0, 34)?;

        let lbl_p = b.create(Class::Label, card_power)?;
        b.text(lbl_p, "Power")?;
        b.align(lbl_p, Align::LeftMid, 28, 0)?;

        let power_val = b.create(Class::Label, card_power)?;
        b.style(power_val, Prop::TextColor, 0, ACCENT)?;
        b.text(power_val, "100%")?;
        b.align(power_val, Align::RightMid, -28, 0)?;

        let (btn_done, _) = make_pill(b, root, 220, 62, ACCENT, ROW_PRESSED, "DONE", BLACK, None)?;
        b.align(btn_done, Align::Center, 0, 132)?;
        b.submit(lv)?;

        let screen = Screen::Settings {
            root,
            card_beats,
            beats_val,
            card_power,
            power_val,
            btn_done,
        };
        sync_screen(b, lv, &screen, settings, false, 0)?;
        Ok(screen)
    }

    fn sync_screen(b: &mut Batch, lv: &Lv, screen: &Screen, settings: &Settings, running: bool, beat: u8) -> Result<(), LvError> {
        b.clear();

        match screen {
            Screen::Main {
                arc,
                dots,
                bpm_val,
                cfg_lbl,
                btn_play,
                play_lbl,
                ..
            } => {
                let mut line = Line::new();
                let _ = write!(line, "{}", settings.bpm);
                b.text(*bpm_val, line.as_str())?;

                let mut cfg = Line::new();
                let _ = write!(cfg, "{}/4  |  {}%", settings.beats_per_bar, settings.strength_pct);
                b.text(*cfg_lbl, cfg.as_str())?;

                b.style(*btn_play, Prop::BgColor, 0, if running { RED } else { ACCENT })?;
                b.style(*play_lbl, Prop::TextColor, 0, if running { WHITE } else { BLACK })?;
                b.text(*play_lbl, if running { "STOP" } else { "START" })?;

                let arc_pct = if running {
                    ((beat as i32 + 1) * 100) / (settings.beats_per_bar as i32)
                } else {
                    0
                };
                b.value(*arc, arc_pct, false)?;

                let count = settings.beats_per_bar as usize;
                let w: i32 = if count <= 4 { 32 } else { 20 };
                let gap: i32 = if count <= 4 { 10 } else { 6 };
                let total_w = count as i32 * w + (count as i32 - 1) * gap;
                let start_x = -total_w / 2 + w / 2;

                for (i, &dot) in dots.iter().enumerate() {
                    if i < count {
                        let x = (start_x + i as i32 * (w + gap)) as i16;
                        b.size(dot, w, 10)?;
                        b.align(dot, Align::Center, x, -134)?;
                        let active = running && (i as u8 == beat);
                        let col = if active {
                            if i == 0 && count > 1 { RED } else { ACCENT }
                        } else {
                            ROW_BG
                        };
                        b.style(dot, Prop::BgColor, 0, col)?;
                        b.style(dot, Prop::BgOpa, 0, 255)?;
                    } else {
                        b.style(dot, Prop::BgOpa, 0, 0)?;
                    }
                }
            }
            Screen::Settings {
                beats_val,
                power_val,
                ..
            } => {
                let mut b_str = Line::new();
                let _ = write!(b_str, "{}/4", settings.beats_per_bar);
                b.text(*beats_val, b_str.as_str())?;

                let mut p_str = Line::new();
                let _ = write!(p_str, "{}%", settings.strength_pct);
                b.text(*power_val, p_str.as_str())?;
            }
        }

        b.submit(lv)
    }

    fn delete_screen(b: &mut Batch, lv: &Lv, screen: &Screen) -> Result<(), LvError> {
        b.clear();
        let root = match screen {
            Screen::Main { root, .. } | Screen::Settings { root, .. } => *root,
        };
        b.del(root)?;
        b.submit(lv)
    }

    fn main(api: &'static Api) -> i32 {
        let Some(lv) = api.lv() else { return -95 };
        let Some(mut b) = Batch::new() else { return -1 };

        if stock::font(&mut b)
            .and_then(|_| b.font(FONT_BIG, StockFont::F48M))
            .and_then(|_| b.font(FONT_SMALL, StockFont::F24M))
            .and_then(|_| b.submit(&lv))
            .is_err()
        {
            return -3;
        }

        let mut settings = Settings::default();
        let mut tap_tracker = TapTracker::default();
        let mut running = false;
        let mut paused = false;
        let mut beat: u8 = 0;
        let mut motor_on = false;
        let mut motor_off_at: u32 = 0;
        let mut next_beat_at: u32 = 0;

        let mut screen = match build_main(&mut b, &lv, &settings, running, beat) {
            Ok(s) => s,
            Err(LvError::Code(c)) => return c,
            Err(_) => return -4,
        };

        loop {
            let now = api.now();

            if running {
                if motor_on && (now.wrapping_sub(motor_off_at) as i32) >= 0 {
                    api.vibrate(0, 0);
                    motor_on = false;
                }

                if (now.wrapping_sub(next_beat_at) as i32) >= 0 {
                    let (duty, freq, pulse_ms) = settings.pulse_params(beat);
                    api.vibrate(duty, freq);
                    motor_on = true;
                    motor_off_at = now.wrapping_add(pulse_ms);
                    if !paused {
                        let _ = sync_screen(&mut b, &lv, &screen, &settings, running, beat);
                    }

                    beat = (beat + 1) % settings.beats_per_bar;
                    next_beat_at = next_beat_at.wrapping_add(settings.interval_ms());
                }
            }

            let timeout = if !running {
                u32::MAX
            } else {
                let to_beat = next_beat_at.wrapping_sub(api.now()) as i32;
                let wait = if motor_on {
                    let to_off = motor_off_at.wrapping_sub(api.now()) as i32;
                    to_beat.min(to_off)
                } else {
                    to_beat
                };
                wait.max(1) as u32
            };

            let Some(event) = api.event(timeout) else { continue };

            match event.kind {
                BUTTON => {
                    api.vibrate(0, 0);
                    api.keep_screen_on(false);
                    return 0;
                }
                PAUSE => {
                    paused = true;
                    api.keep_screen_on(false);
                }
                RESUME => {
                    paused = false;
                    if running {
                        api.keep_screen_on(true);
                    }
                    let _ = sync_screen(&mut b, &lv, &screen, &settings, running, beat);
                }
                CROWN if event.delta != 0 => {
                    let step = event.delta.signum();
                    match screen {
                        Screen::Main { .. } => settings.adjust(Field::Bpm, step),
                        Screen::Settings { .. } => settings.adjust(Field::Beats, step),
                    }
                    if !paused {
                        let _ = sync_screen(&mut b, &lv, &screen, &settings, running, beat);
                    }
                }
                LV if event.y == CLICKED => {
                    let id = event.x as u16;
                    match screen {
                        Screen::Main {
                            btn_play,
                            btn_minus,
                            btn_plus,
                            btn_tap,
                            btn_cfg,
                            ..
                        } => {
                            if id == btn_cfg.0 {
                                let _ = delete_screen(&mut b, &lv, &screen);
                                if let Ok(s) = build_settings(&mut b, &lv, &settings) {
                                    screen = s;
                                }
                                continue;
                            } else if id == btn_play.0 {
                                running = !running;
                                api.keep_screen_on(running);
                                if running {
                                    beat = 0;
                                    next_beat_at = api.now();
                                    continue;
                                } else {
                                    motor_on = false;
                                    api.vibrate(0, 0);
                                }
                            } else if id == btn_minus.0 {
                                settings.adjust_bpm(-5);
                            } else if id == btn_plus.0 {
                                settings.adjust_bpm(5);
                            } else if id == btn_tap.0 {
                                if let Some(new_bpm) = tap_tracker.tap(api.now()) {
                                    settings.bpm = new_bpm;
                                    if running {
                                        next_beat_at = api.now().wrapping_add(settings.interval_ms());
                                    }
                                }
                            }
                        }
                        Screen::Settings {
                            card_beats,
                            card_power,
                            btn_done,
                            ..
                        } => {
                            if id == btn_done.0 {
                                let _ = delete_screen(&mut b, &lv, &screen);
                                if let Ok(s) = build_main(&mut b, &lv, &settings, running, beat) {
                                    screen = s;
                                }
                                continue;
                            } else if id == card_beats.0 {
                                settings.beats_per_bar = if settings.beats_per_bar >= 8 { 1 } else { settings.beats_per_bar + 1 };
                            } else if id == card_power.0 {
                                settings.strength_pct = if settings.strength_pct >= 100 { 20 } else { settings.strength_pct + 10 };
                            }
                        }
                    }

                    if !paused {
                        let _ = sync_screen(&mut b, &lv, &screen, &settings, running, beat);
                    }
                }
                _ => {}
            }
        }
    }
}
