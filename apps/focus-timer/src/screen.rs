//! The focus-timer screen: the stock look, in black and white. A black page, a white progress ring, the countdown in
//! the largest stock font inside it, a grey hint line and one white pill button.
//!
//! Layout on the 466x466 round screen (visible half-width at `y` is `isqrt(233² - (y-233)²)`):
//!
//! ```text
//!   y  52.. 88   title "Focus"            32 px, white
//!   y  90..112   hint line                18 px, grey
//!   y 119..359   arc, 240 px              the countdown sits in its centre (48 px, white)
//!   y 372..436   button, 190x64           white pill, black label (at y 436 the screen is 228 px wide)
//! ```

use cmf_wp2_app::lv::*;
use cmf_wp2_app::stock;

/// Slot of the stock system font, opened by [`stock::font`].
pub const FONT_UI: u8 = stock::FONT;
/// The countdown: the largest stock font.
pub const FONT_TIME: u8 = 1;
/// The hint line under the title.
pub const FONT_SMALL: u8 = 2;

pub const ARC_D: i32 = 240;
/// At y 52..88 the screen is only ~280 px wide, so the title and the hint line are narrower than a full row.
pub const TITLE_W: i32 = 240;
pub const HINT_W: i32 = 320;
pub const BUTTON_W: i32 = 190;
pub const BUTTON_H: i32 = 64;
pub const BUTTON_Y: i16 = -30;

/// Dark grey track of the ring, and the lighter grey a press turns the pill.
const TRACK: i32 = 0x2124;
const PRESSED_FILL: i32 = 0xC618;

pub struct Screen {
    pub arc: Obj,
    pub time: Obj,
    pub hint: Obj,
    pub button: Obj,
    pub button_label: Obj,
}

/// Opens one font. Each font needs a batch of its own: a missing font file stops the batch it is in.
pub fn open_font(b: &mut Batch, slot: u8, font: StockFont) -> Result<(), LvError> {
    b.font(slot, font)
}

/// The whole screen, in the batch the caller submits after it.
pub fn build(b: &mut Batch) -> Result<Screen, LvError> {
    stock::theme(b)?;

    let page = b.create(Class::Obj, ROOT)?;
    b.size(page, stock::SCREEN, stock::SCREEN)?;
    for p in [Prop::PadTop, Prop::PadBottom, Prop::PadLeft, Prop::PadRight, Prop::BorderWidth, Prop::BgOpa] {
        b.style(page, p, 0, 0)?;
    }
    b.flags(page, &[], &[Flag::Scrollable])?; // the crown belongs to the app, not to a scroller

    let title = b.create(Class::Label, page)?;
    b.size(title, TITLE_W, 36)?;
    b.align(title, Align::TopMid, 0, 52)?;
    b.style(title, Prop::TextAlign, 0, 2)?;
    b.text(title, "Focus")?;

    let hint = b.create(Class::Label, page)?;
    b.size(hint, HINT_W, 22)?;
    b.align(hint, Align::TopMid, 0, 90)?;
    b.style(hint, Prop::TextAlign, 0, 2)?;
    b.style(hint, Prop::TextColor, 0, stock::GREY)?;
    b.style(hint, Prop::TextFont, 0, FONT_SMALL as i32)?;
    b.text(hint, "crown: minutes")?;

    let arc = b.create(Class::Arc, page)?;
    b.size(arc, ARC_D, ARC_D)?;
    b.align(arc, Align::Center, 0, 6)?;
    b.flags(arc, &[], &[Flag::Clickable])?; // display only
    b.value(arc, 100, false)?;
    for (part, colour) in [(0, TRACK), (stock::INDICATOR, stock::WHITE)] {
        b.style(arc, Prop::ArcColor, part, colour)?;
        b.style(arc, Prop::ArcOpa, part, 255)?;
        b.style(arc, Prop::ArcWidth, part, 12)?;
        b.style(arc, Prop::ArcRounded, part, 1)?;
    }

    let time = b.create(Class::Label, arc)?;
    b.align(time, Align::Center, 0, 0)?;
    b.style(time, Prop::TextFont, 0, FONT_TIME as i32)?;
    b.text(time, "25:00")?;

    let button = b.create(Class::Obj, page)?;
    b.size(button, BUTTON_W, BUTTON_H)?;
    b.align(button, Align::BottomMid, 0, BUTTON_Y)?;
    b.style(button, Prop::Radius, 0, BUTTON_H / 2)?;
    b.style(button, Prop::BgColor, 0, stock::WHITE)?;
    b.style(button, Prop::BgOpa, 0, 255)?;
    b.style(button, Prop::BgColor, stock::PRESSED, PRESSED_FILL)?;
    b.style(button, Prop::BorderWidth, 0, 0)?;
    b.flags(button, &[Flag::Clickable], &[Flag::Scrollable])?;
    b.events(button, &[Ev::Clicked])?;
    let button_label = b.create(Class::Label, button)?;
    b.align(button_label, Align::Center, 0, 0)?;
    b.style(button_label, Prop::TextColor, 0, stock::BLACK)?;
    b.text(button_label, "Start")?;

    Ok(Screen { arc, time, hint, button, button_label })
}

/// Everything that changes with the phase: countdown, ring, hint, button word.
pub fn draw(b: &mut Batch, s: &Screen, time: &str, arc: i32, hint: &str, action: &str) -> Result<(), LvError> {
    b.text(s.time, time)?;
    b.value(s.arc, arc.clamp(0, 100), false)?;
    b.text(s.hint, hint)?;
    b.text(s.button_label, action)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
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

    /// The launch as the app does it: three font batches, then the screen, validated in order on one Batch.
    fn launch() -> (Batch, Screen, Table) {
        let mut t = Table::new();
        let mut b = batch();
        let mut mask = 0u8;
        for (slot, font) in
            [(FONT_UI, stock::SYSTEM_FONT), (FONT_TIME, StockFont::F48M), (FONT_SMALL, StockFont::F18M)]
        {
            open_font(&mut b, slot, font).unwrap();
            assert_eq!(lvcmd::validate(b.bytes(), &t, mask), Ok(()), "font {slot}");
            apply(&mut t, b.bytes());
            mask |= 1 << slot;
        }
        let s = build(&mut b).unwrap();
        std::println!("screen batch: {} B", b.bytes().len());
        assert_eq!(lvcmd::validate(b.bytes(), &t, mask), Ok(()));
        apply(&mut t, b.bytes());
        (b, s, t)
    }

    #[test]
    fn the_screen_validates_in_four_batches() {
        let (mut b, s, t) = launch();
        let n = b.bytes().len();
        for (time, arc, hint, action) in
            [("25:00", 100, "crown: minutes", "Start"), ("00:01", 1, "session done", "Restart")]
        {
            draw(&mut b, &s, time, arc, hint, action).unwrap();
        }
        assert_eq!(lvcmd::validate(&b.bytes()[n..], &t, 0b111), Ok(()));
    }

    #[test]
    fn every_layout_row_fits_the_round_screen() {
        // half-width of the visible disc at a row, minus the 6 px margin
        fn half(y0: i32, y1: i32) -> i32 {
            let d = (y0 - 233).abs().max((y1 - 233).abs());
            let w = (233 * 233 - d * d).max(0) as f64;
            w.sqrt() as i32 - 6
        }
        assert!(TITLE_W / 2 <= half(52, 88), "title");
        assert!(HINT_W / 2 <= half(90, 112), "hint");
        // the arc is a circle: its widest row is its centre
        assert!(ARC_D / 2 <= half(119, 359) + 6, "arc");
        assert!(BUTTON_W / 2 <= half(372, 436), "button");
    }

    #[test]
    fn the_button_sits_below_the_arc() {
        let arc_bottom = 233 + 6 + ARC_D / 2;
        let button_top = 466 - 30 - BUTTON_H; // BottomMid, BUTTON_Y
        assert!(button_top > arc_bottom, "{button_top} > {arc_bottom}");
    }
}
