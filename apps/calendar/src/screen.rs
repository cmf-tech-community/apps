//! The two screens, in the stock look (`cmf_wp2_app::stock`):
//! - month: the accent month title between `<` and `>`, weekday initials, a 6×7 grid of days with a dot per calendar
//!   that has an event, today in an accent pill, a summary line under it;
//! - day: a day's events as cards under its date;
//! - widget (the page next to the watch face): the day screen with this week between the date and the events.
//!
//! Each screen is one root object under `ROOT`; leaving it deletes the root and everything in it. A screen is built
//! hidden, over as many batches as it needs, and shown in the last one.

use crate::date::{self, Day, Month, CELLS};
use crate::marks::{Mark, Marks};
use crate::rpc::Event;
use cmf_wp2_app::lv::*;
use cmf_wp2_app::stock::{self, ACCENT, BLACK, CIRCLE, GREY, SCREEN, WHITE};
use core::fmt::Write;

/// Font slots besides the system font (slot 0).
pub const F24: u8 = 1;
pub const F48: u8 = 2;

const DOT: i32 = 6;
const DOT_STEP: i16 = 10;

// month grid geometry, inside the 466 px circle (the bottom row's corners at radius ~200)
const COL_W: i32 = 48;
const ROW_H: i32 = 44;
const GRID_X: i32 = (SCREEN - 7 * COL_W) / 2;
const GRID_Y: i32 = 128;
const CELL_W: i32 = COL_W - 4;
const CELL_H: i32 = ROW_H - 4;
const TITLE_Y: i16 = 44;
const TITLE_W: i32 = 190;
const ARROW: i32 = 64;
const ARROW_DX: i16 = 125;
const WEEKDAYS_Y: i16 = 94;
const FOOTER_Y: i16 = GRID_Y as i16 + 6 * ROW_H as i16 + 8;

// bytes one cell, dots included, can take: flush first when fewer are left
const GROUP: usize = 480;

pub type Flush<'a> = dyn FnMut(&mut Batch) -> Result<(), LvError> + 'a;

fn room(b: &mut Batch, flush: &mut Flush) -> Result<(), LvError> {
    if b.bytes().len() + GROUP > BUF { flush(b) } else { Ok(()) }
}

/// The fonts, in a batch of their own.
pub fn fonts(b: &mut Batch) -> Result<(), LvError> {
    stock::font(b)?;
    b.font(F24, StockFont::F24M)?;
    b.font(F48, StockFont::F48M)
}

/// A short line of text in a fixed buffer.
pub struct Line {
    buf: [u8; 40],
    len: usize,
}

impl Line {
    pub fn new() -> Line {
        Line { buf: [0; 40], len: 0 }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl Default for Line {
    fn default() -> Self {
        Self::new()
    }
}

impl Write for Line {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        if self.len + s.len() > self.buf.len() {
            return Err(core::fmt::Error);
        }
        self.buf[self.len..self.len + s.len()].copy_from_slice(s.as_bytes());
        self.len += s.len();
        Ok(())
    }
}

fn events(n: u32, out: &mut Line) {
    let _ = match n {
        0 => write!(out, "no events"),
        1 => write!(out, "1 event"),
        n => write!(out, "{n} events"),
    };
}

/// The line under the month grid.
/// The line under the month grid: today's count whatever month is shown (the phone only tells the coming week).
pub fn month_summary(today: Option<Day>, k: &Marks) -> Line {
    let mut out = Line::new();
    if let Some(n) = k.note {
        let _ = out.write_str(n);
    } else if let Some(t) = today {
        let _ = out.write_str("today: ");
        events(k.get(t).count as u32, &mut out);
    }
    out
}

/// Today's events, for the widget screen and the card.
pub fn today_summary(today: Day, k: &Marks) -> Line {
    let mut out = Line::new();
    if let Some(n) = k.note {
        let _ = out.write_str(n);
    } else {
        events(k.get(today).count as u32, &mut out);
        let _ = out.write_str(" today");
    }
    out
}

/// The card left of the watch face: "Sun 15 Nov" and today's events.
pub fn card_value(today: Day, k: &Marks) -> Line {
    const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let mut out = Line::new();
    let _ = write!(
        out,
        "{} {} {}\n{}",
        DAYS[date::weekday(today) as usize],
        today.day,
        date::SHORT[today.month.month as usize - 1],
        today_summary(today, k).as_str()
    );
    out
}

fn bare(b: &mut Batch, o: Obj) -> Result<(), LvError> {
    for p in [Prop::PadTop, Prop::PadBottom, Prop::PadLeft, Prop::PadRight, Prop::BorderWidth, Prop::BgOpa, Prop::Radius] {
        b.style(o, p, 0, 0)?;
    }
    b.flags(o, &[], &[Flag::Scrollable])
}

/// A full-screen, transparent, hidden root.
fn root(b: &mut Batch) -> Result<Obj, LvError> {
    let r = b.create(Class::Obj, ROOT)?;
    b.size(r, SCREEN, SCREEN)?;
    bare(b, r)?;
    b.style(r, Prop::TextAlign, 0, 2)?; // LV_TEXT_ALIGN_CENTER, inherited by every label
    b.flags(r, &[Flag::Hidden], &[])?;
    Ok(r)
}

fn label(b: &mut Batch, parent: Obj, text: &str, w: i32, x: i16, y: i16) -> Result<Obj, LvError> {
    let l = b.create(Class::Label, parent)?;
    b.size(l, w, CONTENT)?;
    b.text(l, text)?;
    b.align(l, Align::TopMid, x, y)?;
    Ok(l)
}

fn num(n: u8) -> Line {
    let mut l = Line::new();
    let _ = write!(l, "{n}");
    l
}

/// One day: its number, `today` in the accent pill, a dot per calendar with events. `x`, `y`: top left in `parent`.
fn cell(b: &mut Batch, parent: Obj, d: Day, dim: bool, today: bool, mark: Mark, x: i32, y: i32) -> Result<Obj, LvError> {
    let c = b.create(Class::Label, parent)?;
    if mark.count > 0 {
        // a tap shows the day's events
        b.flags(c, &[Flag::Clickable], &[])?;
        b.events(c, &[Ev::Clicked])?;
        b.style(c, Prop::BgColor, stock::PRESSED, stock::ROW_PRESSED)?;
        b.style(c, Prop::BgOpa, stock::PRESSED, 255)?;
        b.style(c, Prop::Radius, 0, 12)?;
    }
    b.size(c, CELL_W, CELL_H)?;
    b.align(c, Align::TopLeft, x as i16, y as i16)?;
    // 24 px: two digits of the 32 px system font fill a 48 px column and run into the next
    b.style(c, Prop::TextFont, 0, F24 as i32)?;
    b.style(c, Prop::PadTop, 0, 4)?;
    b.text(c, num(d.day).as_str())?;
    if today {
        b.style(c, Prop::BgColor, 0, ACCENT)?;
        b.style(c, Prop::BgOpa, 0, 255)?;
        b.style(c, Prop::Radius, 0, 12)?;
    } else if dim {
        b.style(c, Prop::TextColor, 0, GREY)?;
    }
    let n = mark.dots as i16;
    for (i, &colour) in mark.colours[..mark.dots as usize].iter().enumerate() {
        let i = i as i16;
        let dot = b.create(Class::Obj, c)?;
        b.size(dot, DOT, DOT)?;
        b.style(dot, Prop::Radius, 0, CIRCLE)?;
        b.style(dot, Prop::BorderWidth, 0, 0)?;
        // on the accent pill a calendar's colour can vanish: white there
        let colour = if today { WHITE } else { colour };
        b.style(dot, Prop::BgColor, 0, colour)?;
        b.style(dot, Prop::BgOpa, 0, if dim { 128 } else { 255 })?;
        b.align(dot, Align::BottomMid, -(n - 1) * DOT_STEP / 2 + i * DOT_STEP, -3)?;
    }
    Ok(c)
}

/// Heroicons' `chevron-left` (outline, `M15.75 19.5 8.25 12l7.5-7.5`), or mirrored to the right for `dir` 1. LVGL
/// here has no line or image, so the stroke is round dots a pixel apart along both legs: a 2·[`CHEVRON`] px tall
/// chevron, round caps and joint like the SVG's.
fn chevron(b: &mut Batch, flush: &mut Flush, parent: Obj, dir: i16) -> Result<(), LvError> {
    let h = CHEVRON;
    for i in 0..=2 * h {
        if i % 4 == 0 {
            room(b, flush)?;
        }
        // the leg from the top end to the point, then the one down to the bottom end
        let (x, y) = if i <= h { (h / 2 - i, i - h) } else { (-h / 2 + (i - h), i - h) };
        let d = b.create(Class::Obj, parent)?;
        b.size(d, STROKE, STROKE)?;
        b.style(d, Prop::Radius, 0, CIRCLE)?;
        b.style(d, Prop::BorderWidth, 0, 0)?;
        b.style(d, Prop::BgColor, 0, GREY)?;
        b.style(d, Prop::BgOpa, 0, 255)?;
        b.flags(d, &[], &[Flag::Clickable])?; // a press goes to the tap target
        b.align(d, Align::Center, -dir * x, y)?;
    }
    Ok(())
}

/// Half the chevron's height and its stroke, px: 20 px tall; a 4 px stroke reads at arm's length.
const CHEVRON: i16 = 10;
const STROKE: i32 = 4;

/// A round tap target with a grey chevron; `dir` -1 left, 1 right.
fn arrow(b: &mut Batch, flush: &mut Flush, parent: Obj, dir: i16, dx: i16) -> Result<Obj, LvError> {
    let a = b.create(Class::Obj, parent)?;
    b.size(a, ARROW, ARROW)?;
    bare(b, a)?;
    b.style(a, Prop::BgColor, stock::PRESSED, stock::ROW_PRESSED)?;
    b.style(a, Prop::BgOpa, stock::PRESSED, 255)?;
    b.style(a, Prop::Radius, 0, CIRCLE)?;
    // centred on the title's glyphs (digits at y 47-63 in the emulator), not its 32 px line box
    b.align(a, Align::TopMid, dx, TITLE_Y + 11 - ARROW as i16 / 2)?;
    b.events(a, &[Ev::Clicked])?;
    chevron(b, flush, a, dir)?;
    Ok(a)
}

#[derive(Clone, Copy, Debug)]
pub struct MonthView {
    pub root: Obj,
    pub title: Obj,
    pub prev: Obj,
    pub next: Obj,
    pub foot: Obj,
    pub shown: Month,
    /// The days, under the title: [`month_turn`] swaps it for another month's.
    pub grid: Obj,
    /// The grid's cells, as `date::grid(shown)`: a tap on one opens its day.
    pub cells: [Obj; CELLS],
}

/// `shown`'s days in a hidden box over the grid area (the caller shows it), a cell per day.
fn grid(b: &mut Batch, flush: &mut Flush, r: Obj, shown: Month, today: Option<Day>, k: &Marks)
    -> Result<(Obj, [Obj; CELLS]), LvError> {
    room(b, flush)?;
    let g = b.create(Class::Obj, r)?;
    b.size(g, 7 * COL_W, 6 * ROW_H)?;
    bare(b, g)?;
    b.flags(g, &[Flag::Hidden], &[Flag::Clickable])?;
    b.align(g, Align::TopLeft, GRID_X as i16, GRID_Y as i16)?;
    let mut cells = [ROOT; CELLS];
    for (i, d) in date::grid(shown).iter().enumerate().take(CELLS) {
        room(b, flush)?;
        let (x, y) = ((i % 7) as i32 * COL_W + 2, (i / 7) as i32 * ROW_H + 2);
        cells[i] = cell(b, g, *d, d.month != shown, today == Some(*d), k.get(*d), x, y)?;
    }
    Ok((g, cells))
}

fn month_title(shown: Month) -> Line {
    let mut t = Line::new();
    let _ = write!(t, "{} {}", shown.name(), shown.year);
    t
}

/// Another month on the month screen without leaving it: the new grid is built hidden next to the old one, then one
/// batch drops the old grid, shows the new one and renames the title, so the screen never goes black in between.
pub fn month_turn(b: &mut Batch, flush: &mut Flush, v: MonthView, shown: Month, today: Option<Day>, k: &Marks)
    -> Result<MonthView, LvError> {
    let (g, cells) = grid(b, flush, v.root, shown, today, k)?;
    flush(b)?;
    b.text(v.title, month_title(shown).as_str())?;
    b.text(v.foot, month_summary(today, k).as_str())?;
    b.del(v.grid)?;
    b.flags(g, &[], &[Flag::Hidden])?;
    Ok(MonthView { shown, grid: g, cells, ..v })
}

pub fn month_view(b: &mut Batch, flush: &mut Flush, shown: Month, today: Option<Day>, k: &Marks)
    -> Result<MonthView, LvError> {
    room(b, flush)?;
    let r = root(b)?;
    let title = label(b, r, month_title(shown).as_str(), TITLE_W, 0, TITLE_Y)?;
    b.style(title, Prop::TextColor, 0, ACCENT)?;
    b.style(title, Prop::TextFont, 0, F24 as i32)?;
    b.flags(title, &[Flag::Clickable], &[])?;
    b.events(title, &[Ev::Clicked])?;
    let prev = arrow(b, flush, r, -1, -ARROW_DX)?;
    let next = arrow(b, flush, r, 1, ARROW_DX)?;
    room(b, flush)?;
    for (i, w) in date::WEEKDAYS.iter().enumerate() {
        let l = b.create(Class::Label, r)?;
        b.size(l, COL_W, CONTENT)?;
        b.style(l, Prop::TextColor, 0, GREY)?;
        b.style(l, Prop::TextFont, 0, F24 as i32)?;
        b.text(l, w)?;
        b.align(l, Align::TopLeft, (GRID_X + i as i32 * COL_W) as i16, WEEKDAYS_Y)?;
    }
    let (grid, cells) = grid(b, flush, r, shown, today, k)?;
    b.flags(grid, &[], &[Flag::Hidden])?;
    room(b, flush)?;
    let foot = label(b, r, month_summary(today, k).as_str(), 300, 0, FOOTER_Y)?;
    b.style(foot, Prop::TextColor, 0, GREY)?;
    b.style(foot, Prop::TextFont, 0, F24 as i32)?;
    b.flags(r, &[], &[Flag::Hidden])?;
    Ok(MonthView { root: r, title, prev, next, foot, shown, grid, cells })
}

#[derive(Clone, Copy, Debug)]
pub struct WidgetView {
    pub root: Obj,
    /// Tap: the month.
    pub title: Obj,
    pub today: Day,
    /// The day whose events are listed.
    pub sel: Day,
    /// `date::week(today)`: a tap lists that day.
    pub cells: [Obj; 7],
    pub cards: Cards,
}

const EVENT_H: i32 = 96;
/// One line of the 24 px font.
const NAME_H: i32 = 30;
const DAY_TOP: i32 = 110;
// the page: the week under the title, the events under the week
const W_WEEK_Y: i32 = GRID_Y;
const W_TOP: i32 = GRID_Y + ROW_H + 16;

/// "09:30–10:00", "all day", or from the start on for one that runs past midnight.
pub fn span_text(e: &Event) -> Line {
    let mut out = Line::new();
    let hm = |m: u32| (m / 60 % 24, m % 60);
    if e.all_day() {
        let _ = out.write_str("all day");
    } else {
        let (a, b) = (hm(e.start as u32), hm(e.start as u32 + e.duration as u32));
        let _ = write!(out, "{:02}:{:02}–{:02}:{:02}", a.0, a.1, b.0, b.1);
    }
    out
}

/// Event cards, in their events' order: a tap opens that event.
pub type Cards = [Obj; crate::model::MAX];

/// A crown-scrolled root with the accent "wed 18 november" title (clickable).
fn day_root(b: &mut Batch, day: Day) -> Result<(Obj, Obj), LvError> {
    // laid out by hand: stock::page's flex column came out 16 px left in the emulator
    let r = root(b)?;
    b.flags(r, &[Flag::Scrollable], &[])?;
    b.style(r, Prop::PadBottom, 0, DAY_TOP)?; // the last card can scroll up to the middle
    let mut t = Line::new();
    let _ = write!(t, "{} {} {}", &date::WEEKDAY_NAMES[date::weekday(day) as usize][..3], day.day, day.month.name());
    let title = label(b, r, t.as_str(), stock::ROW_W - 80, 0, TITLE_Y)?;
    b.style(title, Prop::TextColor, 0, ACCENT)?;
    b.flags(title, &[Flag::Clickable], &[])?;
    b.events(title, &[Ev::Clicked])?;
    Ok((r, title))
}

/// A card per event from `top` down, with its calendar's colour, its time and its title ("busy" while the phone
/// keeps titles); `empty` when there are none. Then the scroll bar, and the root shown.
fn event_cards<'e>(b: &mut Batch, flush: &mut Flush, r: Obj, top: i32, events: impl Iterator<Item = &'e Event>,
    empty: &str) -> Result<Cards, LvError> {
    let mut cards = [ROOT; crate::model::MAX];
    let mut n = 0;
    for e in events.take(cards.len()) {
        room(b, flush)?;
        let c = stock::card(b, r, EVENT_H)?;
        b.align(c, Align::TopMid, 0, (top + n as i32 * (EVENT_H + stock::ROW_GAP)) as i16)?;
        // a tap opens the event, its whole title
        b.events(c, &[Ev::Clicked])?;
        b.style(c, Prop::BgColor, stock::PRESSED, stock::ROW_PRESSED)?;
        cards[n] = c;
        let bar = b.create(Class::Obj, c)?;
        b.flags(bar, &[], &[Flag::Clickable])?; // a press goes to the card
        b.size(bar, 8, EVENT_H - 40)?;
        b.style(bar, Prop::Radius, 0, CIRCLE)?;
        b.style(bar, Prop::BorderWidth, 0, 0)?;
        b.style(bar, Prop::BgColor, 0, e.color as i32)?;
        b.style(bar, Prop::BgOpa, 0, 255)?;
        b.align(bar, Align::LeftMid, 40, 0)?;
        let time = b.create(Class::Label, c)?;
        b.text(time, span_text(e).as_str())?;
        b.align(time, Align::TopLeft, 68, 10)?;
        let name = b.create(Class::Label, c)?;
        // one line: with CONTENT height a long title wraps out of the card instead of ending in "..."
        b.size(name, stock::ROW_W - 68 - 48, NAME_H)?;
        b.long_mode(name, LongMode::Dot)?;
        b.style(name, Prop::TextAlign, 0, 1)?; // LV_TEXT_ALIGN_LEFT: the root centres
        b.style(name, Prop::TextFont, 0, F24 as i32)?;
        b.style(name, Prop::TextColor, 0, GREY)?;
        b.text(name, if e.title().is_empty() { "busy" } else { e.title() })?;
        b.align(name, Align::TopLeft, 68, 52)?;
        n += 1;
    }
    if n == 0 {
        let l = label(b, r, empty, 300, 0, top as i16)?;
        b.style(l, Prop::TextColor, 0, GREY)?;
    }
    b.scroll_bar(r)?;
    b.knob(r)?;
    b.flags(r, &[], &[Flag::Hidden])?;
    Ok(cards)
}

/// The page next to the watch face: `sel`'s date, this week with its dots (a tap picks a day), and `sel`'s events
/// under it, scrolled by the crown.
pub fn widget_view<'e>(b: &mut Batch, flush: &mut Flush, today: Day, sel: Day, k: &Marks,
    events: impl Iterator<Item = &'e Event>) -> Result<WidgetView, LvError> {
    room(b, flush)?;
    let (r, title) = day_root(b, sel)?;
    for (i, w) in date::WEEKDAYS.iter().enumerate() {
        let l = b.create(Class::Label, r)?;
        b.size(l, COL_W, CONTENT)?;
        b.style(l, Prop::TextColor, 0, GREY)?;
        b.style(l, Prop::TextFont, 0, F24 as i32)?;
        b.text(l, w)?;
        b.align(l, Align::TopLeft, (GRID_X + i as i32 * COL_W) as i16, WEEKDAYS_Y)?;
    }
    let mut cells = [ROOT; 7];
    for (i, d) in date::week(today).iter().enumerate() {
        room(b, flush)?;
        let c = cell(b, r, *d, d.month != today.month, *d == today, k.get(*d), GRID_X + i as i32 * COL_W + 2, W_WEEK_Y)?;
        // every day of the week picks, an empty one too; the picked one sits on the pressed grey
        b.flags(c, &[Flag::Clickable], &[])?;
        b.events(c, &[Ev::Clicked])?;
        b.style(c, Prop::BgColor, stock::PRESSED, stock::ROW_PRESSED)?;
        b.style(c, Prop::BgOpa, stock::PRESSED, 255)?;
        b.style(c, Prop::Radius, 0, 12)?;
        if *d == sel && sel != today {
            b.style(c, Prop::BgColor, 0, stock::ROW_PRESSED)?;
            b.style(c, Prop::BgOpa, 0, 255)?;
        }
        cells[i] = c;
    }
    room(b, flush)?;
    let cards = event_cards(b, flush, r, W_TOP, events, k.note.unwrap_or("no events"))?;
    Ok(WidgetView { root: r, title, today, sel, cells, cards })
}

#[derive(Clone, Copy, Debug)]
pub struct DayView {
    pub root: Obj,
    /// Tap: back to the month.
    pub title: Obj,
    pub day: Day,
    pub cards: Cards,
}

/// One day's events, a crown-scrolled page like the stock lists: the accent date (tap: back to the month), then a
/// card per event.
pub fn day_view<'e>(b: &mut Batch, flush: &mut Flush, day: Day, events: impl Iterator<Item = &'e Event>)
    -> Result<DayView, LvError> {
    room(b, flush)?;
    let (r, title) = day_root(b, day)?;
    let cards = event_cards(b, flush, r, DAY_TOP, events, "no events")?;
    Ok(DayView { root: r, title, day, cards })
}

#[derive(Clone, Copy, Debug)]
pub struct EventView {
    /// Tap anywhere: back to the list.
    pub root: Obj,
    pub day: Day,
    /// Opened from the page (back to it) rather than from a day.
    pub page: bool,
}

/// One event: its date, time and calendar colour, and the whole title wrapped (as much as the phone sends,
/// `rpc::TITLE_MAX` bytes); the crown scrolls a long one.
pub fn event_view(b: &mut Batch, flush: &mut Flush, day: Day, e: &Event, page: bool) -> Result<EventView, LvError> {
    room(b, flush)?;
    let (r, title) = day_root(b, day)?;
    b.flags(title, &[], &[Flag::Clickable])?; // the root takes the tap
    b.flags(r, &[Flag::Clickable], &[])?;
    b.events(r, &[Ev::Clicked])?;
    let time = label(b, r, span_text(e).as_str(), 300, 0, DAY_TOP as i16)?;
    b.style(time, Prop::TextFont, 0, F48 as i32)?;
    let bar = b.create(Class::Obj, r)?;
    b.size(bar, 64, 8)?;
    b.style(bar, Prop::Radius, 0, CIRCLE)?;
    b.style(bar, Prop::BorderWidth, 0, 0)?;
    b.style(bar, Prop::BgColor, 0, e.color as i32)?;
    b.style(bar, Prop::BgOpa, 0, 255)?;
    b.flags(bar, &[], &[Flag::Clickable])?;
    b.align(bar, Align::TopMid, 0, DAY_TOP as i16 + 76)?;
    let name = label(b, r, if e.title().is_empty() { "busy" } else { e.title() }, stock::ROW_W - 40, 0,
        DAY_TOP as i16 + 104)?;
    b.long_mode(name, LongMode::Wrap)?;
    if e.title().is_empty() {
        b.style(name, Prop::TextColor, 0, GREY)?;
    }
    b.scroll_bar(r)?;
    b.knob(r)?;
    b.flags(r, &[], &[Flag::Hidden])?;
    Ok(EventView { root: r, day, page })
}

/// Deletes a screen.
pub fn leave(b: &mut Batch, root: Obj) -> Result<(), LvError> {
    if root != ROOT { b.del(root) } else { Ok(()) }
}

/// Black behind everything, the system font and white text: first, once.
pub fn theme(b: &mut Batch) -> Result<(), LvError> {
    stock::theme(b)?;
    b.style(ROOT, Prop::BgColor, 0, BLACK)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::boxed::Box;
    use std::vec::Vec;
    use wp2_core::lvcmd::{self, Table};

    fn batch() -> Batch {
        Batch::with_buf(Box::leak(Box::new([0; BUF])))
    }

    fn m(y: u16, mo: u8) -> Month {
        Month::new(y, mo).unwrap()
    }

    const FONTS: u8 = 0b111;

    /// Runs `build` with a flush that validates each batch against the loader's table and applies it; returns the
    /// table, the batches' sizes and the result.
    fn run<T>(t: &mut Table, b: &mut Batch, build: impl FnOnce(&mut Batch, &mut Flush) -> Result<T, LvError>)
        -> (T, Vec<usize>) {
        let sizes = std::cell::RefCell::new(Vec::new());
        let mut flush = |b: &mut Batch| -> Result<(), LvError> {
            assert_eq!(lvcmd::validate(b.bytes(), t, FONTS), Ok(()));
            for r in lvcmd::records(b.bytes()) {
                lvcmd::apply(t, &r.unwrap());
            }
            sizes.borrow_mut().push(b.bytes().len());
            b.clear();
            Ok(())
        };
        let out = build(b, &mut flush).unwrap();
        flush(b).unwrap();
        (out, sizes.into_inner())
    }

    fn busy(today: Day) -> Marks {
        // a week of days with three calendars each: the most objects a screen can take
        Marks::filled(today, Mark { count: 31, colours: [ACCENT, 0x867D, 0x9772], dots: 3 })
    }

    #[test]
    fn fonts_validate_on_their_own() {
        let mut b = batch();
        fonts(&mut b).unwrap();
        assert_eq!(lvcmd::validate(b.bytes(), &Table::new(), 0), Ok(()));
    }

    #[test]
    fn month_view_validates_and_fits_the_budget() {
        let today = Day { month: m(2026, 11), day: 15 };
        for k in [Marks::empty(), busy(today)] {
            let mut t = Table::new();
            let mut b = batch();
            let (v, sizes) = run(&mut t, &mut b, |b, f| {
                theme(b)?;
                month_view(b, f, m(2026, 11), Some(today), &k)
            });
            assert!(sizes.iter().all(|&n| n <= BUF));
            std::println!("month view: {} batches, {sizes:?} B", sizes.len());
            // leaving frees every handle: the next month takes the same ones
            let (_, _) = run(&mut t, &mut b, |b, _| leave(b, v.root));
            let (w, _) = run(&mut t, &mut b, |b, f| month_view(b, f, m(2026, 12), Some(today), &k));
            assert_eq!(w.root, v.root);
            assert_eq!((w.title, w.prev, w.next), (v.title, v.prev, v.next));
            // turning months in place: two grids at once stay inside the object budget, and the old one is freed
            let mut v = w;
            for mo in [11, 10, 9, 11] {
                let (n, _) = run(&mut t, &mut b, |b, f| month_turn(b, f, v, m(2026, mo), Some(today), &k));
                assert_eq!((n.root, n.title), (v.root, v.title));
                v = n;
            }
        }
    }

    #[test]
    fn widget_view_validates() {
        let today = Day { month: m(2026, 12), day: 1 }; // its week starts in November
        let all: Vec<Event> = (0..32).map(|i| event(i * 30, 30, 0, "Созвон с командой по релизу и обсуждение")).collect();
        for k in [Marks::empty(), busy(today)] {
            let mut t = Table::new();
            let mut b = batch();
            let (v, sizes) = run(&mut t, &mut b, |b, f| {
                theme(b)?;
                widget_view(b, f, today, today, &k, all.iter())
            });
            std::println!("widget view: {sizes:?} B");
            run(&mut t, &mut b, |b, _| leave(b, v.root));
            // and the month view after it, as a tap does
            run(&mut t, &mut b, |b, f| month_view(b, f, today.month, Some(today), &k));
        }
    }

    fn event(start: u16, dur: u16, flags: u8, title: &str) -> Event {
        let mut r = std::vec![0u8];
        r.extend(start.to_le_bytes());
        r.extend(dur.to_le_bytes());
        r.push(flags);
        r.extend(0x4CFDu16.to_le_bytes());
        r.extend(title.as_bytes());
        Event::decode(&r).unwrap()
    }

    #[test]
    fn day_view_validates_then_the_month_again() {
        let today = Day { month: m(2026, 11), day: 17 };
        let long = "Созвон с командой по релизу и обсуждение планов";
        let all: Vec<Event> = (0..32).map(|i| event(i * 30, 30, 0, long)).collect();
        for evs in [&all[..], &[][..]] {
            let mut t = Table::new();
            let mut b = batch();
            let (v, sizes) = run(&mut t, &mut b, |b, f| {
                theme(b)?;
                day_view(b, f, today, evs.iter())
            });
            std::println!("day view: {sizes:?} B");
            run(&mut t, &mut b, |b, _| leave(b, v.root));
            if let Some(e) = evs.first() {
                let (v, _) = run(&mut t, &mut b, |b, f| event_view(b, f, today, e, false));
                run(&mut t, &mut b, |b, _| leave(b, v.root));
            }
            let (v, _) = run(&mut t, &mut b, |b, f| event_view(b, f, today, &event(0, 1440, 1, ""), true));
            run(&mut t, &mut b, |b, _| leave(b, v.root));
            run(&mut t, &mut b, |b, f| month_view(b, f, today.month, Some(today), &busy(today)));
        }
    }

    #[test]
    fn event_times() {
        assert_eq!(span_text(&event(570, 30, 0, "")).as_str(), "09:30–10:00");
        assert_eq!(span_text(&event(1380, 120, 0, "")).as_str(), "23:00–01:00");
        assert_eq!(span_text(&event(0, 1440, 1, "")).as_str(), "all day");
    }

    #[test]
    fn summaries() {
        let nov = m(2026, 11);
        let today = Day { month: nov, day: 15 };
        let k = Marks::empty();
        assert_eq!(month_summary(Some(today), &k).as_str(), "not synced");
        assert_eq!(today_summary(today, &k).as_str(), "not synced");
        let one = Mark { count: 1, colours: [ACCENT, 0, 0], dots: 1 };
        let k = Marks::filled(today, Mark { count: 3, ..one });
        assert_eq!(month_summary(Some(today), &k).as_str(), "today: 3 events");
        assert_eq!(today_summary(today, &k).as_str(), "3 events today");
        assert_eq!(today_summary(today, &Marks::filled(today, one)).as_str(), "1 event today");
        assert_eq!(today_summary(today, &Marks::filled(today, Mark::default())).as_str(), "no events today");
        assert_eq!(card_value(today, &k).as_str(), "Sun 15 Nov\n3 events today");
        // the card's value limit, the longest note too
        let dec30 = Day { month: m(2026, 12), day: 30 };
        assert!(card_value(dec30, &busy(dec30)).as_str().len() <= 39);
        let mut ask = Marks::empty();
        ask.note = Some("allow on your phone");
        assert!(card_value(dec30, &ask).as_str().len() <= 39);
    }
}
