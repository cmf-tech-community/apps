//! mood: a month of days as seven columns of squares. Tap a day to cycle its level (clear, low, ok, good, great);
//! the crown flips between the months of the current year. A whole year of levels lives in the app's 4 KiB blob.
#![cfg_attr(target_os = "none", no_std, no_main)]

#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod model;
mod screen;

#[cfg(not(target_os = "none"))]
fn main() {}

#[cfg(target_os = "none")]
mod device {
    use crate::model::{days_in_month, Year, BLOB_LEN, MONTHS};
    use crate::screen::{self, Screen};
    use cmf_wp2_app::lv::{Batch, Lv, LvError, StockFont};
    use cmf_wp2_app::{stock, Api, BUTTON, CROWN, LV, PAUSE, RESUME};

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();

    /// LVGL 8.3 `LV_EVENT_CLICKED`.
    const CLICKED: i16 = 7;
    const LEVEL_WORDS: [&str; 5] = ["clear", "low", "ok", "good", "great"];

    fn load(api: &Api, year: i32) -> Year {
        let mut buf = [0u8; BLOB_LEN];
        let n = api.load(&mut buf);
        let mut y = Year::new(year);
        if n > 0 {
            if let Some(stored) = Year::from_blob(&buf[..n as usize]) {
                y.adopt(&stored);
            }
        }
        y
    }

    fn save(api: &Api, y: &Year) {
        let blob = y.blob();
        api.save(&blob);
    }

    fn push(buf: &mut [u8], at: &mut usize, s: &str) {
        for &c in s.as_bytes() {
            if *at < buf.len() {
                buf[*at] = c;
                *at += 1;
            }
        }
    }

    fn push_u8(buf: &mut [u8], at: &mut usize, mut n: u8) {
        if n >= 10 {
            push(buf, at, core::str::from_utf8(&[b'0' + n / 10]).unwrap_or(""));
            n %= 10;
        }
        push(buf, at, core::str::from_utf8(&[b'0' + n]).unwrap_or(""));
    }

    /// "Nov 2026"
    fn title_text(year: i32, month: u8, buf: &mut [u8; 8]) -> &str {
        let mut n = 0;
        push(buf, &mut n, MONTHS[(month as usize).clamp(1, 12) - 1]);
        push(buf, &mut n, " ");
        let mut y = year.clamp(0, 9999) as u16;
        let mut digits = [0u8; 4];
        for i in (0..4).rev() {
            digits[i] = b'0' + (y % 10) as u8;
            y /= 10;
        }
        push(buf, &mut n, core::str::from_utf8(&digits).unwrap_or(""));
        core::str::from_utf8(&buf[..n]).unwrap_or("")
    }

    /// "today 16"
    fn today_text(day: u8, buf: &mut [u8; 10]) -> &str {
        let mut n = 0;
        push(buf, &mut n, "today ");
        push_u8(buf, &mut n, day);
        core::str::from_utf8(&buf[..n]).unwrap_or("")
    }

    /// "16 Nov: good"
    fn tap_text(day: u8, month: u8, level: u8, buf: &mut [u8; 16]) -> &str {
        let mut n = 0;
        push_u8(buf, &mut n, day);
        push(buf, &mut n, " ");
        push(buf, &mut n, MONTHS[(month as usize).clamp(1, 12) - 1]);
        push(buf, &mut n, ": ");
        push(buf, &mut n, LEVEL_WORDS[(level as usize).min(4)]);
        core::str::from_utf8(&buf[..n]).unwrap_or("")
    }

    /// A whole month in as many batches as the 2 KiB record buffer needs: the grid, then cells, then the header.
    fn draw_month(
        b: &mut Batch,
        lv: &Lv,
        s: &mut Screen,
        model: &Year,
        month: u8,
        subtitle: &str,
        year: i32,
    ) -> Result<(), LvError> {
        screen::grid(b, s)?;
        b.submit(lv)?;
        let mut day = 1;
        while day <= days_in_month(year, month) {
            let next = screen::cells(b, s, model, year, month, day)?;
            if screen::month_done(year, month, next) {
                break;
            }
            b.submit(lv)?; // bank this chunk and keep filling
            day = next;
        }
        let mut tb = [0u8; 8];
        screen::texts(b, s, title_text(year, month, &mut tb), subtitle)?;
        b.submit(lv)
    }

    fn main(api: &'static Api) -> i32 {
        let Some(lv) = api.lv() else { return -95 }; // loader older than ABI 1.2
        let Some(mut b) = Batch::new() else { return -1 };

        for (slot, font) in [(screen::FONT_UI, stock::SYSTEM_FONT), (screen::FONT_SMALL, StockFont::F18M)] {
            if screen::open_font(&mut b, slot, font).and_then(|_| b.submit(&lv)).is_err() {
                return 1;
            }
        }
        let Ok(mut s) = screen::build(&mut b) else { return 2 };
        if b.submit(&lv).is_err() {
            return 3;
        }

        let Some(clock) = api.time() else {
            // no RTC yet: nothing to show, but say why instead of a blank screen
            let _ = screen::texts(&mut b, &s, "Mood", "set the clock").and_then(|_| b.submit(&lv));
            loop {
                match api.event(u32::MAX) {
                    Some(e) if e.kind == BUTTON => return 0,
                    Some(_) => {}
                    None => {}
                }
            }
        };

        let year = clock.year as i32;
        let mut model = load(api, year);
        let mut month = clock.month.clamp(1, 12);
        let mut buf = [0u8; 16];
        let mut today_buf = [0u8; 10];
        let subtitle = today_text(clock.day, &mut today_buf);
        if draw_month(&mut b, &lv, &mut s, &model, month, subtitle, year).is_err() {
            return 4;
        }

        loop {
            let Some(e) = api.event(u32::MAX) else { continue };
            match e.kind {
                BUTTON => break,
                CROWN => {
                    let next = (month as i32 - 1 + e.delta as i32).rem_euclid(12) as u8 + 1;
                    if next != month {
                        month = next;
                        let sub = if month == clock.month { today_text(clock.day, &mut today_buf) } else { "" };
                        if draw_month(&mut b, &lv, &mut s, &model, month, sub, year).is_err() {
                            break;
                        }
                    }
                }
                LV => {
                    if e.y == CLICKED && e.x >= 0 {
                        if let Some(day) = screen::day_of(&s, e.x as u16) {
                            let level = model.cycle(month, day);
                            let sub = tap_text(day, month, level, &mut buf);
                            if screen::level(&mut b, &s, day, level)
                                .and_then(|_| b.text(s.subtitle, sub))
                                .and_then(|_| b.submit(&lv))
                                .is_ok()
                            {
                                save(api, &model);
                            }
                        }
                    }
                }
                PAUSE => loop {
                    match api.event(u32::MAX) {
                        Some(e) if e.kind == RESUME => break,
                        Some(e) if e.kind == BUTTON => {
                            save(api, &model);
                            return 0;
                        }
                        _ => {}
                    }
                },
                _ => {}
            }
        }

        save(api, &model);
        0
    }
}
