//! focus-timer: an adjustable focus session on the watch. The crown sets the length (±1 minute), the white pill
//! starts, pauses and restarts it, and three motor pulses tell the user it is over. The chosen length survives a
//! restart in the app's 4 KiB blob.
//!
//! The screen is the stock LVGL kit in black and white (`src/screen.rs`); the state machine and the pulse pattern
//! are pure code with host tests (`src/timer.rs`).
#![cfg_attr(target_os = "none", no_std, no_main)]

#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod screen;
#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod timer;

#[cfg(not(target_os = "none"))]
fn main() {}

#[cfg(target_os = "none")]
mod device {
    use crate::screen::{self, Screen};
    use crate::timer::{Buzz, Focus, Phase, BUZZ_HZ, DEFAULT_S};
    use cmf_wp2_app::lv::{Batch, Lv, StockFont};
    use cmf_wp2_app::{stock, Api, BUTTON, CROWN, LV, PAUSE, RESUME};

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();

    /// LVGL 8.3 `LV_EVENT_CLICKED`.
    const CLICKED: i16 = 7;
    /// Blob layout: `"FT"`, version, `u16` seconds. Anything else is treated as "never saved".
    const STORE_MAGIC: [u8; 2] = *b"FT";
    const STORE_VERSION: u8 = 1;
    const STORE_LEN: usize = 5;

    fn load(api: &Api) -> Focus {
        let mut buf = [0u8; STORE_LEN];
        if api.load(&mut buf) == STORE_LEN as i32
            && buf[0..2] == STORE_MAGIC
            && buf[2] == STORE_VERSION
        {
            Focus::new(u16::from_le_bytes([buf[3], buf[4]]) as u32)
        } else {
            Focus::new(DEFAULT_S)
        }
    }

    fn save(api: &Api, f: &Focus) {
        let mut buf = [0u8; STORE_LEN];
        buf[0..2].copy_from_slice(&STORE_MAGIC);
        buf[2] = STORE_VERSION;
        buf[3..5].copy_from_slice(&(f.total_s as u16).to_le_bytes());
        api.save(&buf);
    }

    /// One batch: countdown, ring, hint and button word. `drawn` keeps the last countdown that reached the panel.
    fn redraw(b: &mut Batch, lv: &Lv, s: &Screen, f: &Focus, now: u32, drawn: &mut u32) {
        let mut buf = [0u8; 5];
        let text = f.label(now, &mut buf);
        let arc = f.arc(now);
        if screen::draw(b, s, text, arc, f.hint(), f.action()).and_then(|_| b.submit(lv)).is_ok() {
            *drawn = f.remaining_s(now);
        }
    }

    fn main(api: &'static Api) -> i32 {
        let Some(lv) = api.lv() else { return -95 }; // loader older than ABI 1.2
        let Some(mut b) = Batch::new() else { return -1 };

        // Each font in a batch of its own: a missing file stops the batch it is in.
        for (slot, font) in [
            (screen::FONT_UI, stock::SYSTEM_FONT),
            (screen::FONT_TIME, StockFont::F48M),
            (screen::FONT_SMALL, StockFont::F18M),
        ] {
            if screen::open_font(&mut b, slot, font).and_then(|_| b.submit(&lv)).is_err() {
                return 1;
            }
        }
        let Ok(s) = screen::build(&mut b) else { return 2 };
        if b.submit(&lv).is_err() {
            return 3;
        }

        let mut focus = load(api);
        let mut buzz = Buzz::default();
        let mut drawn = u32::MAX; // force the first paint
        let mut now = api.now();
        redraw(&mut b, &lv, &s, &focus, now, &mut drawn);

        loop {
            let _ = buzz.poll(now).map(|duty| api.vibrate(duty, BUZZ_HZ));

            if focus.phase == Phase::Running {
                let left = focus.remaining_s(now);
                if left == 0 {
                    focus.phase = Phase::Done;
                    buzz.start(now);
                    redraw(&mut b, &lv, &s, &focus, now, &mut drawn);
                } else if left != drawn {
                    redraw(&mut b, &lv, &s, &focus, now, &mut drawn);
                }
            }

            let timeout = if focus.phase == Phase::Running { 200 } else { u32::MAX };
            let Some(e) = api.event(timeout) else {
                now = api.now();
                continue;
            };
            now = api.now();

            match e.kind {
                BUTTON => break,
                CROWN => {
                    if focus.adjust(e.delta as i32) {
                        save(api, &focus);
                        redraw(&mut b, &lv, &s, &focus, now, &mut drawn);
                    }
                }
                LV => {
                    if e.y == CLICKED && e.x >= 0 && e.x as u16 == s.button.0 {
                        match focus.phase {
                            Phase::Idle => focus.start(now),
                            // stop the ringing and keep what is left, so the next tap resumes
                            Phase::Running => {
                                buzz.stop();
                                api.vibrate(0, 0);
                                focus.pause(now);
                                save(api, &focus);
                            }
                            Phase::Done => focus.reset(),
                        }
                        redraw(&mut b, &lv, &s, &focus, now, &mut drawn);
                    }
                }
                PAUSE => {
                    // a stock popup covers the app: motor off, then wait. The deadline keeps counting.
                    buzz.stop();
                    api.vibrate(0, 0);
                    loop {
                        match api.event(u32::MAX) {
                            Some(e) if e.kind == RESUME => break,
                            Some(e) if e.kind == BUTTON => {
                                save(api, &focus);
                                return 0;
                            }
                            _ => {}
                        }
                    }
                    now = api.now();
                    drawn = u32::MAX;
                    redraw(&mut b, &lv, &s, &focus, now, &mut drawn);
                }
                RESUME => {
                    now = api.now();
                    drawn = u32::MAX;
                    redraw(&mut b, &lv, &s, &focus, now, &mut drawn);
                }
                _ => {}
            }
        }

        api.vibrate(0, 0); // the loader cuts it too, but the app's own pattern has to stop
        save(api, &focus);
        0
    }
}
