//! The Mood screen: a month as seven columns of rounded squares, in the stock look and strictly black to white.
//!
//! ```text
//!   y  46.. 80   title: "Nov 2026"          32 px, white
//!   y  82..102   subtitle: "tap a day"      18 px, grey
//!   y 108..384   6 x 7 grid of 40 px cells  316 px wide, 276 px tall, 6 px gaps
//!   y 388..410   hint: "crown: month"       18 px, grey
//! ```
//!
//! A cell's fill is the day's level: near-black for no entry, four steps up to pure white. A month does not fit in
//! one batch (31 cells and 31 labels are ~430 records), so [`cells`] hands back where it stopped with
//! [`LvError::Full`] and the caller submits and calls it again.

use cmf_wp2_app::lv::*;
use cmf_wp2_app::stock;

use crate::model::{days_in_month, weekday, Year};

pub const FONT_UI: u8 = stock::FONT;
pub const FONT_SMALL: u8 = 1;

pub const CELL: i32 = 40;
pub const GAP: i32 = 6;
pub const PITCH: i32 = CELL + GAP;
pub const GRID_X: i16 = 75;
pub const GRID_Y: i16 = 108;
pub const MAX_DAYS: usize = 31;
/// Records one cell and its number take (~140 B measured, padded).
const CELL_BYTES: usize = 200;
/// The round screen is only this wide at the top and the bottom.
pub const TITLE_W: i32 = 250;
pub const HINT_W: i32 = 280;

/// Fill for each level: 0 is "no entry", 4 is white.
const FILL: [i32; 5] = [0x2124, 0x39E7, 0x7BEF, 0xBDF7, stock::WHITE];

pub struct Screen {
    pub page: Obj,
    pub title: Obj,
    pub subtitle: Obj,
    pub grid: Obj,
    pub cells: [Obj; MAX_DAYS],
    pub labels: [Obj; MAX_DAYS],
    has_grid: bool,
}

fn fill(level: u8) -> i32 {
    FILL[(level as usize).min(FILL.len() - 1)]
}

/// Level 0..=1 are dark squares, so their number is white; 2..=4 take a black number.
fn ink(level: u8) -> i32 {
    if level <= 1 { stock::WHITE } else { stock::BLACK }
}

pub fn open_font(b: &mut Batch, slot: u8, font: StockFont) -> Result<(), LvError> {
    b.font(slot, font)
}

/// Page, title, subtitle and hint. The grid is added by [`grid`] and [`cells`].
pub fn build(b: &mut Batch) -> Result<Screen, LvError> {
    stock::theme(b)?;

    let page = b.create(Class::Obj, ROOT)?;
    b.size(page, stock::SCREEN, stock::SCREEN)?;
    for p in [Prop::PadTop, Prop::PadBottom, Prop::PadLeft, Prop::PadRight, Prop::BorderWidth, Prop::BgOpa] {
        b.style(page, p, 0, 0)?;
    }
    b.flags(page, &[], &[Flag::Scrollable])?;

    let title = b.create(Class::Label, page)?;
    b.size(title, TITLE_W, 36)?;
    b.align(title, Align::TopMid, 0, 46)?;
    b.style(title, Prop::TextAlign, 0, 2)?;
    b.text(title, "Mood")?;

    let subtitle = b.create(Class::Label, page)?;
    b.size(subtitle, 300, 22)?;
    b.align(subtitle, Align::TopMid, 0, 82)?;
    b.style(subtitle, Prop::TextAlign, 0, 2)?;
    b.style(subtitle, Prop::TextColor, 0, stock::GREY)?;
    b.style(subtitle, Prop::TextFont, 0, FONT_SMALL as i32)?;
    b.text(subtitle, "tap a day")?;

    let hint = b.create(Class::Label, page)?;
    b.size(hint, HINT_W, 22)?;
    b.align(hint, Align::TopMid, 0, 388)?;
    b.style(hint, Prop::TextAlign, 0, 2)?;
    b.style(hint, Prop::TextColor, 0, stock::GREY)?;
    b.style(hint, Prop::TextFont, 0, FONT_SMALL as i32)?;
    b.text(hint, "crown: month")?;

    Ok(Screen {
        page,
        title,
        subtitle,
        grid: ROOT,
        cells: [ROOT; MAX_DAYS],
        labels: [ROOT; MAX_DAYS],
        has_grid: false,
    })
}

/// Replaces the old grid with an empty one. Submit after it, then fill it with [`cells`].
pub fn grid(b: &mut Batch, s: &mut Screen) -> Result<(), LvError> {
    if s.has_grid {
        b.del(s.grid)?;
        s.has_grid = false;
    }
    let grid = b.create(Class::Obj, s.page)?;
    b.size(grid, 7 * PITCH - GAP, 6 * PITCH - GAP)?;
    b.align(grid, Align::TopLeft, GRID_X, GRID_Y)?;
    for p in [Prop::PadTop, Prop::PadBottom, Prop::PadLeft, Prop::PadRight, Prop::BorderWidth, Prop::BgOpa] {
        b.style(grid, p, 0, 0)?;
    }
    b.flags(grid, &[], &[Flag::Scrollable])?;
    s.grid = grid;
    s.has_grid = true;
    for slot in 0..MAX_DAYS {
        s.cells[slot] = ROOT;
        s.labels[slot] = ROOT;
    }
    Ok(())
}

/// Adds cells from day `from` until the batch is nearly full, and returns the day to continue at: past the month's
/// last day when the month is complete, otherwise the caller submits and calls it again with that day.
pub fn cells(b: &mut Batch, s: &mut Screen, model: &Year, year: i32, month: u8, from: u8) -> Result<u8, LvError> {
    let first = weekday(year, month, 1) as i32; // column of the 1st, 0 = Sunday
    let days = days_in_month(year, month);
    let mut day = from.max(1);
    while day <= days {
        // never start a cell that would not fit whole: a half-written cell would reach the loader without its
        // events and never send a tap
        if b.bytes().len() + CELL_BYTES > BUF {
            break;
        }
        let slot = (first + day as i32 - 1) as usize;
        let (col, row) = (slot % 7, slot / 7);
        let level = model.get(month, day);

        let cell = match b.create(Class::Obj, s.grid) {
            Ok(c) => c,
            Err(e) => return Err(e),
        };
        b.size(cell, CELL, CELL)?;
        b.align(cell, Align::TopLeft, (col as i32 * PITCH) as i16, (row as i32 * PITCH) as i16)?;
        b.style(cell, Prop::Radius, 0, 12)?;
        b.style(cell, Prop::BgColor, 0, fill(level))?;
        b.style(cell, Prop::BgOpa, 0, 255)?;
        b.style(cell, Prop::BorderWidth, 0, 0)?;
        b.flags(cell, &[Flag::Clickable], &[Flag::Scrollable])?;
        b.events(cell, &[Ev::Clicked])?;
        let label = b.create(Class::Label, cell)?;
        b.align(label, Align::Center, 0, 0)?;
        b.style(label, Prop::TextColor, 0, ink(level))?;
        b.style(label, Prop::TextFont, 0, FONT_SMALL as i32)?;
        let mut buf = [0u8; 2];
        b.text(label, day_text(day, &mut buf))?;

        s.cells[day as usize - 1] = cell;
        s.labels[day as usize - 1] = label;
        day += 1;
    }
    Ok(day)
}

/// Whether [`cells`] has finished the month.
pub fn month_done(year: i32, month: u8, next: u8) -> bool {
    next > days_in_month(year, month)
}

fn day_text(day: u8, buf: &mut [u8; 2]) -> &str {
    let mut n = day;
    let mut len = 0;
    if n >= 10 {
        buf[0] = b'0' + n / 10;
        len = 1;
        n %= 10;
    }
    buf[len] = b'0' + n;
    core::str::from_utf8(&buf[..len + 1]).unwrap_or("?")
}

/// The day a CLICKED handle belongs to, if it is one of the month's cells.
pub fn day_of(s: &Screen, handle: u16) -> Option<u8> {
    s.cells.iter().position(|c| c.0 == handle).map(|i| i as u8 + 1)
}

/// Repaints one cell for a new level.
pub fn level(b: &mut Batch, s: &Screen, day: u8, level: u8) -> Result<(), LvError> {
    let i = day as usize - 1;
    b.style(s.cells[i], Prop::BgColor, 0, fill(level))?;
    b.style(s.labels[i], Prop::TextColor, 0, ink(level))
}

pub fn texts(b: &mut Batch, s: &Screen, title: &str, subtitle: &str) -> Result<(), LvError> {
    b.text(s.title, title)?;
    b.text(s.subtitle, subtitle)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::model::Year;
    use std::boxed::Box;
    use wp2_core::lvcmd::{self, Table};

    fn batch() -> Batch {
        Batch::with_buf(Box::leak(Box::new([0; BUF])))
    }

    fn apply(t: &mut Table, records: &[u8]) {
        for r in lvcmd::records(records) {
            lvcmd::apply(t, &r.unwrap());
        }
    }

    fn launch() -> (Batch, Screen, Table, u8) {
        let mut t = Table::new();
        let mut b = batch();
        let mut mask = 0u8;
        for (slot, font) in [(FONT_UI, stock::SYSTEM_FONT), (FONT_SMALL, StockFont::F18M)] {
            open_font(&mut b, slot, font).unwrap();
            assert_eq!(lvcmd::validate(b.bytes(), &t, mask), Ok(()), "font {slot}");
            apply(&mut t, b.bytes());
            mask |= 1 << slot;
        }
        let s = build(&mut b).unwrap();
        assert_eq!(lvcmd::validate(b.bytes(), &t, mask), Ok(()));
        apply(&mut t, b.bytes());
        (b, s, t, mask)
    }

    /// Fills a month the way the app does: one batch per chunk, submitted when the buffer is full.
    fn fill_month(b: &mut Batch, s: &mut Screen, t: &mut Table, mask: u8, model: &Year, year: i32, month: u8) {
        grid(b, s).unwrap();
        assert_eq!(lvcmd::validate(b.bytes(), t, mask), Ok(()));
        apply(t, b.bytes());
        b.clear(); // stands in for the submit

        let mut day = 1;
        let mut batches = 0;
        loop {
            let next = cells(b, s, model, year, month, day).unwrap();
            batches += 1;
            assert!(next > day, "must make progress");
            assert_eq!(lvcmd::validate(b.bytes(), t, mask), Ok(()), "chunk at day {day}");
            apply(t, b.bytes());
            b.clear();
            if month_done(year, month, next) {
                break;
            }
            day = next;
        }
        std::println!("{month}/{year}: {batches} cell batches");
    }

    #[test]
    fn a_month_fills_in_chunks_and_validates() {
        let (mut b, mut s, mut t, mask) = launch();
        let model = Year::new(2026);
        b.clear();
        fill_month(&mut b, &mut s, &mut t, mask, &model, 2026, 11);
        // 30 days in November
        assert_ne!(s.cells[29].0, 0);
        assert_eq!(s.cells[30], ROOT);
        assert_eq!(day_of(&s, s.cells[15].0), Some(16));
    }

    #[test]
    fn a_leap_february_has_29_cells() {
        let (mut b, mut s, mut t, mask) = launch();
        let model = Year::new(2024);
        b.clear();
        fill_month(&mut b, &mut s, &mut t, mask, &model, 2024, 2);
        assert_ne!(s.cells[28].0, 0);
        assert_eq!(s.cells[29], ROOT);
    }

    #[test]
    fn switching_months_frees_the_old_grid() {
        let (mut b, mut s, mut t, mask) = launch();
        let model = Year::new(2026);
        b.clear();
        fill_month(&mut b, &mut s, &mut t, mask, &model, 2026, 1);
        let first = s.grid;
        b.clear();
        fill_month(&mut b, &mut s, &mut t, mask, &model, 2026, 2);
        assert_eq!(s.grid, first); // freed and recreated at the same handle
    }

    #[test]
    fn one_tap_repaints_one_cell() {
        let (mut b, mut s, mut t, mask) = launch();
        let model = Year::new(2026);
        b.clear();
        fill_month(&mut b, &mut s, &mut t, mask, &model, 2026, 11);
        b.clear();
        level(&mut b, &s, 16, 4).unwrap();
        texts(&mut b, &s, "Nov 2026", "16 Nov").unwrap();
        assert_eq!(lvcmd::validate(b.bytes(), &t, mask), Ok(()));
    }

    #[test]
    fn the_grid_stays_inside_the_round_screen() {
        fn half(y0: i32, y1: i32) -> i32 {
            let d = (y0 - 233).abs().max((y1 - 233).abs());
            let w = (233 * 233 - d * d).max(0) as f64;
            w.sqrt() as i32 - 6
        }
        let w = 7 * PITCH - GAP;
        assert!(TITLE_W / 2 <= half(46, 80), "title");
        assert!(300 / 2 <= half(82, 102), "subtitle");
        assert!(HINT_W / 2 <= half(388, 410), "hint");
        for row in 0..6 {
            let y0 = GRID_Y as i32 + row * PITCH;
            assert!(w / 2 <= half(y0, y0 + CELL), "grid row {row}");
        }
    }

    #[test]
    fn day_numbers_are_two_glyphs() {
        let mut buf = [0u8; 2];
        assert_eq!(day_text(1, &mut buf), "1");
        assert_eq!(day_text(9, &mut buf), "9");
        assert_eq!(day_text(10, &mut buf), "10");
        assert_eq!(day_text(31, &mut buf), "31");
    }
}
