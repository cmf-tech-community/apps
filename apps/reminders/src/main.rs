//! reminders: the iPhone's open reminders on the watch, from FMC Companion (`reminders`, `reminders.complete`).
//!
//! - The list (`screen::list_view`): overdue first in red, today, then by day, then the ones without a date; the
//!   crown scrolls. A tap on a row asks "Done?" (`screen::confirm_view`); Done ticks it off on the phone, buzzes
//!   once and the row goes. BUTTON (the right swipe, `own_back`) on "Done?" comes back, on the list exits.
//! - The card left of the watch face (`app_features!(WIDGET)`): how many are due today and the next one, written
//!   whenever it changes while the app runs.
//!
//! The list comes from the phone at start and every 3 minutes while open, and is kept in the app's storage so it
//! opens at once without the phone (`model.rs`). Ticks the phone hasn't confirmed are kept too and sent again.
#![cfg_attr(target_os = "none", no_std, no_main)]

#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod model;
#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod screen;

#[cfg(not(target_os = "none"))]
fn main() {}

#[cfg(target_os = "none")]
mod device {
    use crate::model::{self, BLOB_MAX, Got, Line, Model, When, wall_min};
    use crate::screen::{self, ConfirmView, ListView};
    use cmf_wp2_app::lv::{Batch, Lv, Obj, ROOT};
    use cmf_wp2_app::{Api, BUTTON, LV, MESSAGE, MSG_MAX, PAUSE, RESUME};

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();
    cmf_wp2_app::app_features!(cmf_wp2_app::WIDGET);

    const CLICKED: i16 = 7;
    /// How often to look at the clock: a reminder's time passing makes it overdue.
    const TICK_MS: u32 = 30_000;
    /// The buzz of a tick: one short pulse.
    const BUZZ: (u16, u16, u32) = (650, 31, 60);

    static mut STORE: [u8; BLOB_MAX] = [0; BLOB_MAX];
    // ponytail: ~3.6 KB of reminders, kept off the 8 KiB stack
    static mut MODEL: Model = Model::new();

    fn wall(api: &Api) -> i32 {
        api.time().map_or(0, |t| wall_min(t.year as i32, t.month as u32, t.day as u32, t.hour as u32, t.minute as u32))
    }

    #[derive(Clone, Copy)]
    enum View {
        List(ListView),
        Confirm(ConfirmView),
    }

    struct App {
        api: &'static Api,
        lv: Lv,
        b: Batch,
        model: &'static mut Model,
        view: View,
        /// What the list shows depends on: the day and how many are overdue.
        shown: (i32, usize),
        card: Line,
        buzz_off: Option<u32>,
    }

    impl App {
        fn root(&self) -> Obj {
            match self.view {
                View::List(v) => v.root,
                View::Confirm(v) => v.root,
            }
        }

        fn key(&self, w: i32) -> (i32, usize) {
            let m = &*self.model;
            (w.div_euclid(1440), (0..m.n).filter(|&i| m.when(i, w) == When::Overdue).count())
        }

        fn list(&mut self) {
            let (lv, w) = (self.lv, wall(self.api));
            let mut flush = |b: &mut Batch| b.submit(&lv);
            let old = self.root();
            let r = screen::leave(&mut self.b, old)
                .and_then(|_| screen::list_view(&mut self.b, &mut flush, self.model, w))
                .and_then(|v| self.b.submit(&lv).map(|_| v));
            if let Ok(v) = r {
                self.view = View::List(v);
            }
            self.shown = self.key(w);
        }

        fn confirm(&mut self, i: usize) {
            let (lv, w) = (self.lv, wall(self.api));
            let mut flush = |b: &mut Batch| b.submit(&lv);
            let old = self.root();
            let r = screen::leave(&mut self.b, old)
                .and_then(|_| screen::confirm_view(&mut self.b, &mut flush, self.model, i, w))
                .and_then(|v| self.b.submit(&lv).map(|_| v));
            match r {
                Ok(v) => self.view = View::Confirm(v),
                Err(_) => self.list(),
            }
        }

        fn save(&mut self) {
            let blob = unsafe { &mut *core::ptr::addr_of_mut!(STORE) };
            let n = self.model.save(blob);
            if n > 0 {
                self.api.save(&blob[..n]); // on change: the app can end without notice
            }
        }

        /// The card, when its text changes.
        fn card(&mut self) {
            let v = model::card_value(self.model, wall(self.api));
            if v.as_str() != self.card.as_str() {
                self.api.widget("Reminders", v.as_str());
                self.card = v;
            }
        }

        fn done(&mut self, id: u32) {
            let now = self.api.now();
            if let Some(i) = (0..self.model.n).find(|&i| self.model.list[i].id == id) {
                if self.model.complete(i, now) {
                    self.save();
                    let (duty, hz, ms) = BUZZ;
                    if self.api.vibrate(duty, hz) == 0 {
                        self.buzz_off = Some(now.wrapping_add(ms));
                    } else {
                        self.api.vibrate(0, hz); // -4: clear the lock, skip this buzz
                    }
                }
            }
            self.list();
            self.card();
        }

        fn stop_buzz(&mut self) {
            if self.buzz_off.take().is_some() {
                self.api.vibrate(0, BUZZ.1);
            }
        }

        fn poll(&mut self) {
            let now = self.api.now();
            while let Some(req) = self.model.poll(now) {
                if self.api.send(&req) == -11 {
                    break; // outbox full: the retry timer sends it again
                } // -107 with no phone: the retry timer covers it too
            }
        }

        fn message(&mut self, m: &[u8]) {
            let got = self.model.reply(m, self.api.now(), wall(self.api));
            if got == Got::Saved {
                self.save();
            }
            if got != Got::Nothing {
                if let View::List(_) = self.view {
                    self.list();
                }
                self.card();
            }
        }

        /// The clock moved: a reminder became overdue, or a new day.
        fn tick(&mut self) {
            if self.key(wall(self.api)) != self.shown {
                if let View::List(_) = self.view {
                    self.list();
                }
                self.card();
            }
        }
    }

    fn main(api: &'static Api) -> i32 {
        let Some(lv) = api.lv() else { return -95 }; // loader older than ABI 1.2
        let Some(mut b) = Batch::new() else { return -1 };
        if screen::fonts(&mut b).and_then(|_| b.submit(&lv)).is_err() {
            return 1;
        }
        if screen::theme(&mut b).and_then(|_| b.submit(&lv)).is_err() {
            return 2;
        }
        let model = unsafe { &mut *core::ptr::addr_of_mut!(MODEL) };
        let blob = unsafe { &mut *core::ptr::addr_of_mut!(STORE) };
        let n = api.load(blob);
        if n > 0 {
            model.load(&blob[..n as usize]);
        }
        model.ask_now(api.now());
        let placeholder = View::List(ListView { root: ROOT, rows: [ROOT; model::MAX], index: [0; model::MAX], n: 0 });
        let mut app = App { api, lv, b, model, view: placeholder, shown: (0, 0), card: Line::new(), buzz_off: None };
        api.own_back(true); // the right swipe on "Done?" comes back to the list, not out (no-op before ABI 1.3)
        app.list();
        app.card();
        let mut paused = false;
        let mut msg = [0u8; MSG_MAX];
        loop {
            if !paused {
                app.poll();
            }
            let now = api.now();
            let buzz = app.buzz_off.map_or(u32::MAX, |t| (t.wrapping_sub(now) as i32).max(0) as u32);
            let wait = if paused { u32::MAX } else { app.model.wait(now).min(TICK_MS).min(buzz) };
            let e = api.event(wait);
            if let Some(t) = app.buzz_off {
                if (api.now().wrapping_sub(t) as i32) >= 0 {
                    app.stop_buzz();
                }
            }
            if !paused {
                app.tick();
            }
            let Some(e) = e else { continue };
            match e.kind {
                BUTTON => match app.view {
                    View::Confirm(_) => app.list(),
                    View::List(_) => break,
                },
                PAUSE => {
                    app.stop_buzz();
                    paused = true;
                }
                RESUME => {
                    paused = false;
                    app.model.ask_now(api.now());
                    app.tick();
                }
                MESSAGE => {
                    while let Some(m) = api.recv(&mut msg) {
                        app.message(m);
                    }
                }
                LV if e.y == CLICKED && !paused => {
                    let h = e.x as u16;
                    match app.view {
                        View::List(v) => {
                            if let Some(k) = v.rows[..v.n].iter().position(|r| r.0 == h) {
                                app.confirm(v.index[k] as usize);
                            }
                        }
                        View::Confirm(v) if h == v.done.0 => app.done(v.id),
                        View::Confirm(v) if h == v.cancel.0 => app.list(),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        app.stop_buzz();
        0
    }
}
