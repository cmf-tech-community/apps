//! calendar: a month grid with a dot per calendar on the days that have events, and today in a widget.
//!
//! - From the Apps menu: the month view. The crown or `<` `>` change the month, a tap on the title goes back to
//!   today's month, a tap on a day with events lists them (a tap on that title comes back), BUTTON exits.
//! - As the page next to the watch face (`app_features!(PAGE)`): the widget view (today, this week, today's count).
//!   A tap opens the month view, a tap on its title comes back.
//! - The card left of the watch face (`app_features!(WIDGET)`): "Sun 15 Nov" and today's count.
//!
//! Events for the coming week come from FMC Companion on the phone (`calendar.events`, `rpc.rs`); the watch keeps
//! the last answer (`model.rs`) and turns it into a count and calendar-coloured dots per day (`marks.rs`). Without
//! the phone the calendar works and says "not synced".
#![cfg_attr(target_os = "none", no_std, no_main)]

#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod date;
#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod marks;
#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod model;
#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod rpc;
#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod screen;

#[cfg(not(target_os = "none"))]
fn main() {}

#[cfg(target_os = "none")]
mod device {
    use crate::date::{Day, Month};
    use crate::marks::Marks;
    use crate::model::{BLOB_MAX, Got, Model, wall_min};
    use crate::screen::{self, DayView, MonthView, WidgetView};
    use cmf_wp2_app::lv::{Batch, Lv, LvError};
    use cmf_wp2_app::{Api, BUTTON, CROWN, LV, MESSAGE, MSG_MAX, PAUSE, RESUME};

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();
    cmf_wp2_app::app_features!(cmf_wp2_app::WIDGET | cmf_wp2_app::PAGE);

    const CLICKED: i16 = 7;
    /// How often to look at the clock (a new day).
    const TICK_MS: u32 = 30_000;
    const FALLBACK: Month = Month { year: 2026, month: 1 };

    // read by an emulator e2e: last submit result, view (1 month, 2 widget), shown month (yyyymm), phone answers
    const RC: usize = 0;
    const VIEW: usize = 1;
    const SHOWN: usize = 2;
    const SYNCS: usize = 3;
    // non-zero: in .data, fresh at every launch; RC 1 until the first screen is up
    #[unsafe(no_mangle)]
    static mut STATS: [i32; 4] = [1, 0, 0, 0];
    static mut STORE: [u8; BLOB_MAX] = [0; BLOB_MAX];
    // ponytail: ~4.6 KB of events, kept off the 8 KiB stack
    static mut MODEL: Model = Model::new();

    fn set(i: usize, v: i32) {
        unsafe { (&raw mut STATS[i]).write_volatile(v) }
    }
    fn get(i: usize) -> i32 {
        unsafe { (&raw const STATS[i]).read_volatile() }
    }
    fn code(r: Result<(), LvError>) -> i32 {
        match r {
            Ok(()) => 0,
            Err(LvError::Code(c)) => c,
            Err(_) => -1000,
        }
    }

    fn wall(api: &Api) -> Option<i32> {
        let t = api.time()?;
        Some(wall_min(t.year as i32, t.month as u32, t.day as u32, t.hour as u32, t.minute as u32))
    }

    fn today(api: &Api) -> Option<Day> {
        let t = api.time()?;
        let month = Month::new(t.year, t.month)?;
        (1..=month.days()).contains(&t.day).then_some(Day { month, day: t.day })
    }

    #[derive(Clone, Copy)]
    enum View {
        Month(MonthView),
        Widget(WidgetView),
        Day(DayView),
    }

    impl View {
        fn root(&self) -> cmf_wp2_app::lv::Obj {
            match self {
                View::Month(v) => v.root,
                View::Widget(v) => v.root,
                View::Day(v) => v.root,
            }
        }
    }

    enum Want {
        Month(Month),
        /// The page, listing that day of this week.
        Widget(Day),
        Day(Day),
    }

    /// The phone's events that start on `d`.
    fn events(m: &Model, w: i32, today: Option<Day>, d: Day) -> impl Iterator<Item = &crate::rpc::Event> {
        let off = crate::marks::day_no(d) - today.map_or(0, crate::marks::day_no);
        (0..m.n).filter(move |&i| m.span(i, w).0 == off).map(move |i| &m.events[i])
    }

    struct App {
        api: &'static Api,
        lv: Lv,
        b: Batch,
        model: &'static mut Model,
        marks: Marks,
        view: View,
        today: Option<Day>,
    }

    impl App {
        /// Replaces the screen with `want`; on an error the old screen stays.
        fn show(&mut self, want: Want) {
            let lv = self.lv;
            let mut flush = |b: &mut Batch| b.submit(&lv);
            let old = self.view.root();
            let (m, w, today) = (&*self.model, wall(self.api).unwrap_or(0), self.today);
            let r = screen::leave(&mut self.b, old).and_then(|_| match want {
                Want::Month(m) => {
                    screen::month_view(&mut self.b, &mut flush, m, self.today, &self.marks).map(View::Month)
                }
                Want::Widget(d) => {
                    let t = self.today.unwrap_or(d);
                    screen::widget_view(&mut self.b, &mut flush, t, d, &self.marks, events(m, w, today, d)).map(View::Widget)
                }
                Want::Day(d) => screen::day_view(&mut self.b, &mut flush, d, events(m, w, today, d)).map(View::Day),
            });
            let r = match r {
                Ok(v) => {
                    self.view = v;
                    self.b.submit(&lv)
                }
                Err(e) => Err(e),
            };
            set(RC, code(r));
            match self.view {
                View::Month(v) => {
                    set(VIEW, 1);
                    set(SHOWN, v.shown.year as i32 * 100 + v.shown.month as i32);
                }
                View::Widget(_) => set(VIEW, 2),
                View::Day(_) => set(VIEW, 3),
            }
        }

        /// The same screen again, for a new day or new marks.
        fn refresh(&mut self) {
            let want = match self.view {
                View::Month(v) => Want::Month(v.shown),
                View::Day(v) => Want::Day(v.day),
                // the picked day stays while it's still this week's
                View::Widget(v) => match self.today {
                    Some(_) if Some(v.today) == self.today => Want::Widget(v.sel),
                    Some(d) => Want::Widget(d),
                    None => Want::Month(FALLBACK),
                },
            };
            self.show(want);
        }

        fn card(&self) {
            if let Some(d) = self.today {
                self.api.widget("Calendar", screen::card_value(d, &self.marks).as_str());
            }
        }

        fn remark(&mut self) {
            if let (Some(d), Some(w)) = (self.today, wall(self.api)) {
                self.marks = Marks::build(self.model, d, w);
            }
        }

        fn poll(&mut self) {
            if let Some(req) = self.model.poll(self.api.now()) {
                self.api.send(&req); // -107 with no phone: the retry timer covers it
            }
        }

        /// A new day since the last look: the screens and the card follow.
        fn tick(&mut self) {
            let now = today(self.api);
            if now == self.today {
                return;
            }
            let was = self.today;
            self.today = now;
            self.remark();
            if let (View::Month(v), Some(old), Some(new)) = (self.view, was, now) {
                if v.shown == old.month {
                    self.show(Want::Month(new.month));
                    self.card();
                    return;
                }
            }
            self.refresh();
            self.card();
        }

        fn message(&mut self, m: &[u8]) {
            let w = wall(self.api).unwrap_or(self.model.fetched.unwrap_or(0));
            let got = self.model.reply(m, self.api.now(), w);
            if got == Got::Events {
                let blob = unsafe { &mut *core::ptr::addr_of_mut!(STORE) };
                let n = self.model.save(blob);
                self.api.save(&blob[..n]); // on change: the app can end without notice
                set(SYNCS, get(SYNCS) + 1);
            }
            if got != Got::Nothing {
                self.remark();
                self.refresh();
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
        let today = today(api);
        let placeholder = View::Month(MonthView {
            root: cmf_wp2_app::lv::ROOT,
            title: cmf_wp2_app::lv::ROOT,
            prev: cmf_wp2_app::lv::ROOT,
            next: cmf_wp2_app::lv::ROOT,
            shown: FALLBACK,
            cells: [cmf_wp2_app::lv::ROOT; crate::date::CELLS],
        });
        let mut app = App { api, lv, b, model, marks: Marks::empty(), view: placeholder, today };
        app.remark();
        let on_page = api.on_page();
        app.show(match (on_page, today) {
            (true, Some(d)) => Want::Widget(d),
            (_, d) => Want::Month(d.map_or(FALLBACK, |d| d.month)),
        });
        app.card();
        let mut paused = false;
        let mut msg = [0u8; MSG_MAX];
        loop {
            if !paused {
                app.poll();
            }
            let e = api.event(if paused { u32::MAX } else { app.model.wait(api.now()).min(TICK_MS) });
            if !paused {
                app.tick();
            }
            let Some(e) = e else { continue };
            match e.kind {
                BUTTON => break,
                PAUSE => paused = true,
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
                CROWN if !paused => {
                    if let View::Month(v) = app.view {
                        app.show(Want::Month(v.shown.add(if e.delta > 0 { 1 } else { -1 })));
                    }
                }
                LV if e.y == CLICKED && !paused => {
                    let h = e.x as u16;
                    match app.view {
                        View::Month(v) if h == v.prev.0 => app.show(Want::Month(v.shown.add(-1))),
                        View::Month(v) if h == v.next.0 => app.show(Want::Month(v.shown.add(1))),
                        View::Month(v) if h == v.title.0 => app.show(match (on_page, app.today) {
                            (true, Some(d)) => Want::Widget(d),
                            (_, d) => Want::Month(d.map_or(v.shown, |d| d.month)),
                        }),
                        View::Month(v) => {
                            if let Some(i) = v.cells.iter().position(|c| c.0 == h) {
                                app.show(Want::Day(crate::date::grid(v.shown)[i]));
                            }
                        }
                        View::Day(v) if h == v.title.0 => app.show(Want::Month(v.day.month)),
                        View::Widget(w) if h == w.title.0 => app.show(Want::Month(w.today.month)),
                        View::Widget(w) => {
                            if let Some(i) = w.cells.iter().position(|c| c.0 == h) {
                                app.show(Want::Widget(crate::date::week(w.today)[i]));
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        0
    }
}
