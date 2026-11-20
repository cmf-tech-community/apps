//! The two screens, in the stock look (`cmf_wp2_app::stock`):
//! - the list: the accent "Reminders" title, then a section per day (overdue in red first, today, tomorrow, later
//!   days, no date), a row card per reminder with its list's colour as a dot, the title and its time; or one line
//!   saying why there is nothing (allow on the phone, not an iPhone, all done);
//! - the confirm: "Done?", the reminder, Cancel and Done.
//!
//! Each screen is one root under `ROOT`, built hidden over as many batches as it needs and shown in the last one;
//! leaving it deletes the root and everything in it.

use crate::model::{self, Model, Status, When, MAX};
use cmf_wp2_app::lv::*;
use cmf_wp2_app::stock::{self, ACCENT, BLACK, CIRCLE, GREY, ROW_BG, ROW_PRESSED, SCREEN, WHITE};

/// Font slot of the 24 px font (the system font is slot 0).
pub const F24: u8 = 1;
/// Overdue: iOS Reminders' red, #FF3B30.
pub const RED: i32 = 0xF9C6;

const ROW_1: i32 = 72;
const ROW_2: i32 = 100;
const HEAD_H: i32 = 40;
const TEXT_X: i16 = 64;
const MSG_W: i32 = 340;

// bytes one row, its header included, can take: flush first when fewer are left
const GROUP: usize = 420;

pub type Flush<'a> = dyn FnMut(&mut Batch) -> Result<(), LvError> + 'a;

fn room(b: &mut Batch, flush: &mut Flush) -> Result<(), LvError> {
    if b.bytes().len() + GROUP > BUF { flush(b) } else { Ok(()) }
}

/// The fonts, in a batch of their own.
pub fn fonts(b: &mut Batch) -> Result<(), LvError> {
    stock::font(b)?;
    b.font(F24, StockFont::F24M)
}

/// Black behind everything, the system font and white text: first, once.
pub fn theme(b: &mut Batch) -> Result<(), LvError> {
    stock::theme(b)?;
    b.style(ROOT, Prop::BgColor, 0, BLACK)
}

/// What to say instead of the list when it is empty.
pub fn empty_text(m: &Model) -> &'static str {
    match m.status {
        Status::Ask => "Allow Reminders in FMC Companion on your phone",
        Status::Denied => "Reminders are off for this app. Turn them on in FMC Companion: Apps, Reminders",
        Status::Unavailable => "Reminders need an iPhone",
        _ if m.silent() => "Phone not reachable",
        Status::Waiting => "Asking your phone...",
        Status::Ok => "All done",
    }
}

/// A line under a list from the cache, when the phone isn't giving a fresh one.
pub fn note(m: &Model) -> Option<&'static str> {
    match m.status {
        Status::Ask => Some("Allow Reminders in FMC Companion"),
        Status::Denied => Some("Reminders are off in FMC Companion"),
        Status::Unavailable => Some("Reminders need an iPhone"),
        _ if m.silent() => Some("Phone not reachable"),
        _ => None,
    }
}

fn root(b: &mut Batch) -> Result<Obj, LvError> {
    let r = b.create(Class::Obj, ROOT)?;
    b.size(r, SCREEN, SCREEN)?;
    for p in [Prop::PadTop, Prop::PadLeft, Prop::PadRight, Prop::BorderWidth, Prop::BgOpa, Prop::Radius] {
        b.style(r, p, 0, 0)?;
    }
    b.style(r, Prop::TextAlign, 0, 2)?; // LV_TEXT_ALIGN_CENTER, inherited by every label
    b.flags(r, &[Flag::Hidden], &[Flag::Scrollable])?;
    Ok(r)
}

fn label(b: &mut Batch, parent: Obj, text: &str, w: i32, x: i16, y: i32) -> Result<Obj, LvError> {
    let l = b.create(Class::Label, parent)?;
    b.size(l, w, CONTENT)?;
    b.text(l, text)?;
    b.align(l, Align::TopMid, x, y as i16)?;
    Ok(l)
}

fn title(b: &mut Batch, r: Obj, text: &str) -> Result<Obj, LvError> {
    let t = label(b, r, text, stock::ROW_W - 80, 0, stock::TITLE_Y)?;
    b.style(t, Prop::TextColor, 0, ACCENT)?;
    Ok(t)
}

/// The crown scrolls it; then it shows.
fn finish(b: &mut Batch, r: Obj) -> Result<(), LvError> {
    b.flags(r, &[Flag::Scrollable], &[])?;
    b.style(r, Prop::PadBottom, 0, SCREEN / 2)?; // the last row can scroll up to the middle
    b.scroll_bar(r)?;
    b.knob(r)?;
    b.flags(r, &[], &[Flag::Hidden])
}

#[derive(Clone, Copy, Debug)]
pub struct ListView {
    pub root: Obj,
    /// Row cards in screen order; a tap asks "Done?" for `index[k]`.
    pub rows: [Obj; MAX],
    /// The `Model::list` index of each row.
    pub index: [u8; MAX],
    pub n: usize,
}

/// The list at `wall` (minutes, `model::wall_min`).
pub fn list_view(b: &mut Batch, flush: &mut Flush, m: &Model, wall: i32) -> Result<ListView, LvError> {
    room(b, flush)?;
    let r = root(b)?;
    title(b, r, "Reminders")?;
    let (order, n) = m.order(wall);
    let mut v = ListView { root: r, rows: [ROOT; MAX], index: order, n };
    let mut y = stock::TOP;
    if n == 0 {
        let l = label(b, r, empty_text(m), MSG_W, 0, y + 40)?;
        b.long_mode(l, LongMode::Wrap)?;
        if m.status != Status::Ok || m.silent() {
            b.style(l, Prop::TextColor, 0, GREY)?;
        }
        return finish(b, r).map(|_| v);
    }
    let mut section = None;
    for (k, &i) in order[..n].iter().enumerate() {
        room(b, flush)?;
        let i = i as usize;
        let w = match m.when(i, wall) {
            When::Later(d) if d > 1 => When::Later(d), // one section per day
            w => w,
        };
        if section != Some(w) {
            section = Some(w);
            let h = label(b, r, model::section_label(w, wall).as_str(), stock::ROW_W - 80, 0, y)?;
            b.style(h, Prop::TextFont, 0, F24 as i32)?;
            b.style(h, Prop::TextColor, 0, if w == When::Overdue { RED } else { GREY })?;
            y += HEAD_H;
        }
        let rem = &m.list[i];
        let detail = model::detail(m, i, wall);
        let h = if detail.as_str().is_empty() { ROW_1 } else { ROW_2 };
        let c = stock::card(b, r, h)?;
        b.align(c, Align::TopMid, 0, y as i16)?;
        b.events(c, &[Ev::Clicked])?;
        b.style(c, Prop::BgColor, stock::PRESSED, ROW_PRESSED)?;
        v.rows[k] = c;
        let dot = b.create(Class::Obj, c)?;
        b.flags(dot, &[], &[Flag::Clickable])?; // a press goes to the card
        b.size(dot, 18, 18)?;
        b.style(dot, Prop::Radius, 0, CIRCLE)?;
        b.style(dot, Prop::BorderWidth, 0, 0)?;
        b.style(dot, Prop::BgColor, 0, rem.color as i32)?;
        b.style(dot, Prop::BgOpa, 0, 255)?;
        b.align(dot, Align::LeftMid, 30, 0)?;
        let t = b.create(Class::Label, c)?;
        // one line ending in "...": with CONTENT height a long title would wrap out of the card
        b.size(t, stock::ROW_W - TEXT_X as i32 - 44, 40)?;
        b.long_mode(t, LongMode::Dot)?;
        b.style(t, Prop::TextAlign, 0, 1)?; // LV_TEXT_ALIGN_LEFT: the root centres
        b.text(t, model::title(rem))?;
        if rem.title.is_empty() {
            b.style(t, Prop::TextColor, 0, GREY)?;
        }
        if h == ROW_1 {
            b.align(t, Align::LeftMid, TEXT_X, 0)?;
        } else {
            b.align(t, Align::TopLeft, TEXT_X, 12)?;
            let d = b.create(Class::Label, c)?;
            b.style(d, Prop::TextFont, 0, F24 as i32)?;
            b.style(d, Prop::TextColor, 0, if w == When::Overdue { RED } else { GREY })?;
            b.text(d, detail.as_str())?;
            b.align(d, Align::TopLeft, TEXT_X, 56)?;
        }
        y += h + stock::ROW_GAP;
    }
    if let Some(t) = note(m) {
        let l = label(b, r, t, MSG_W, 0, y + 8)?;
        b.style(l, Prop::TextFont, 0, F24 as i32)?;
        b.style(l, Prop::TextColor, 0, GREY)?;
        b.long_mode(l, LongMode::Wrap)?;
    }
    finish(b, r)?;
    Ok(v)
}

#[derive(Clone, Copy, Debug)]
pub struct ConfirmView {
    pub root: Obj,
    pub done: Obj,
    pub cancel: Obj,
    /// The reminder asked about (its index may change while the screen is up).
    pub id: u32,
}

fn button(b: &mut Batch, r: Obj, text: &str, bg: i32, x: i16) -> Result<Obj, LvError> {
    let btn = b.create(Class::Btn, r)?;
    b.size(btn, 170, 84)?;
    b.style(btn, Prop::Radius, 0, CIRCLE)?;
    b.style(btn, Prop::BorderWidth, 0, 0)?;
    b.style(btn, Prop::BgColor, 0, bg)?;
    b.style(btn, Prop::BgOpa, 0, 255)?;
    b.style(btn, Prop::BgColor, stock::PRESSED, ROW_PRESSED)?;
    b.align(btn, Align::TopMid, x, 286)?;
    b.events(btn, &[Ev::Clicked])?;
    let l = b.create(Class::Label, btn)?;
    b.style(l, Prop::TextColor, 0, WHITE)?;
    b.text(l, text)?;
    b.align(l, Align::Center, 0, 0)?;
    Ok(btn)
}

/// "Done?" for reminder `i`.
pub fn confirm_view(b: &mut Batch, flush: &mut Flush, m: &Model, i: usize, wall: i32) -> Result<ConfirmView, LvError> {
    room(b, flush)?;
    let r = root(b)?;
    title(b, r, "Done?")?;
    let rem = &m.list[i];
    let t = label(b, r, model::title(rem), MSG_W, 0, 120)?;
    b.long_mode(t, LongMode::Wrap)?;
    if rem.title.is_empty() {
        b.style(t, Prop::TextColor, 0, GREY)?;
    }
    let mut when = model::detail(m, i, wall);
    if when.as_str().is_empty() {
        if let Some(d) = m.day(i, wall) {
            model::day_label(d, wall, &mut when);
        }
    }
    let d = label(b, r, when.as_str(), MSG_W, 0, 232)?;
    b.style(d, Prop::TextFont, 0, F24 as i32)?;
    b.style(d, Prop::TextColor, 0, if m.when(i, wall) == When::Overdue { RED } else { GREY })?;
    room(b, flush)?;
    let cancel = button(b, r, "Cancel", ROW_BG, -92)?;
    let done = button(b, r, "Done", ACCENT, 92)?;
    b.flags(r, &[], &[Flag::Hidden])?;
    Ok(ConfirmView { root: r, done, cancel, id: rem.id })
}

/// Deletes a screen.
pub fn leave(b: &mut Batch, root: Obj) -> Result<(), LvError> {
    if root != ROOT { b.del(root) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::model::BLOB_MAX;
    use std::boxed::Box;
    use std::vec::Vec;
    use wp2_core::lvcmd::{self, Table};

    // 2026-11-19 10:00
    const WALL: i32 = 20_776 * 1440 + 600;
    const FONTS: u8 = 0b11;

    fn batch() -> Batch {
        Batch::with_buf(Box::leak(Box::new([0; BUF])))
    }

    /// Runs `build` with a flush that validates each batch against the loader's table and applies it; returns
    /// the result and the batches' sizes.
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

    /// A model with `n` reminders over every section, long titles, half of them hidden.
    fn full(n: usize) -> Model {
        let mut blob = Vec::from(*b"M1");
        blob.extend(WALL.to_le_bytes());
        blob.push(n as u8);
        for k in 0..n {
            let day = k as i8 - 3;
            let hidden = k % 2 == 1;
            let mut r = (k as u32 + 1).to_le_bytes().to_vec();
            r.push(day as u8);
            r.extend(((k as u16 * 97) % 1440).to_le_bytes());
            r.push(if k % 5 == 4 { 0 } else { 1 | ((k % 3 == 0) as u8) << 1 } | (hidden as u8) << 2);
            r.extend(0x2D7Fu16.to_le_bytes());
            if !hidden {
                r.extend("Позвонить в банк насчёт карты и заодно спросить про вклад".as_bytes()[..64].iter());
            }
            blob.push(r.len() as u8);
            blob.extend(r);
        }
        blob.push(0);
        assert!(blob.len() <= BLOB_MAX);
        let mut m = Model::new();
        m.load(&blob);
        assert_eq!(m.n, n);
        m
    }

    #[test]
    fn fonts_validate_on_their_own() {
        let mut b = batch();
        fonts(&mut b).unwrap();
        assert_eq!(lvcmd::validate(b.bytes(), &Table::new(), 0), Ok(()));
    }

    #[test]
    fn list_and_confirm_validate_within_the_budgets_and_free_everything() {
        for n in [0, 1, 7, MAX] {
            let m = full(n);
            let mut t = Table::new();
            let mut b = batch();
            let (v, sizes) = run(&mut t, &mut b, |b, f| {
                theme(b)?;
                list_view(b, f, &m, WALL)
            });
            assert!(sizes.iter().all(|&s| s <= BUF));
            std::println!("list of {n}: {} batches, {sizes:?} B", sizes.len());
            assert_eq!(v.n, n);
            assert!(v.rows[..n].iter().all(|&r| r != ROOT));
            // a tap: the confirm, then back; leaving frees every handle, so the list comes back on the same ones
            let (_, _) = run(&mut t, &mut b, |b, _| leave(b, v.root));
            if n > 0 {
                let (c, _) = run(&mut t, &mut b, |b, f| confirm_view(b, f, &m, v.index[0] as usize, WALL));
                assert_ne!(c.done, c.cancel);
                let (_, _) = run(&mut t, &mut b, |b, _| leave(b, c.root));
            }
            let (w, _) = run(&mut t, &mut b, |b, f| list_view(b, f, &m, WALL));
            assert_eq!((w.root, w.rows), (v.root, v.rows));
        }
    }

    #[test]
    fn empty_states() {
        let mut m = Model::new();
        assert_eq!(empty_text(&m), "Asking your phone...");
        m.status = Status::Unavailable;
        assert_eq!(empty_text(&m), "Reminders need an iPhone");
        m.status = Status::Ok;
        assert_eq!((empty_text(&m), note(&m)), ("All done", None));
    }
}
