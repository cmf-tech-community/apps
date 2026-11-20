//! The watch screen: generic playback state, transport controls and a music-volume slider.
//!
//! Track names, artists and artwork deliberately stay on the phone. The watch receives only whether
//! media is active/playing and the current music-stream volume.

use crate::model::{Model, Status};
use cmf_wp2_ui::color::{BLACK, Rgb};
use cmf_wp2_ui::geom::text_box;
use cmf_wp2_ui::text::{centre, fitted};
use cmf_wp2_ui::{C, SIZE, Surface};

const BLACK_U16: u16 = 0x0000;
const WHITE: u16 = 0xFFFF;
const GREY: u16 = 0x8C92;
const DIM: u16 = 0x52AB;
const ACCENT: u16 = 0xFA23;
const RED: u16 = 0xF213;
const STATUS_Y: i32 = 174;
const DETAIL_Y: i32 = 202;
const VOLUME_Y: i32 = 312;
const FOOTER_Y: i32 = 378;

const PREV_X: i32 = 91;
const PLAY_X: i32 = 233;
const NEXT_X: i32 = 375;
const CONTROL_Y: i32 = 250;
const CONTROL_R: i32 = 34;

/// What a tap in the transport row should do.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub enum Action {
    Previous,
    Toggle,
    Next,
    VolumeUp,
    VolumeDown,
    SetVolume(u8),
    None,
}

/// Maps a touch to a playback or volume action.
pub fn action(x: i32, y: i32, max_volume: u8) -> Action {
    if (CONTROL_Y - CONTROL_R..CONTROL_Y + CONTROL_R).contains(&y) {
        if (PREV_X - CONTROL_R..PREV_X + CONTROL_R).contains(&x) {
            return Action::Previous;
        }
        if (PLAY_X - CONTROL_R..PLAY_X + CONTROL_R).contains(&x) {
            return Action::Toggle;
        }
        if (NEXT_X - CONTROL_R..NEXT_X + CONTROL_R).contains(&x) {
            return Action::Next;
        }
    }
    if (VOLUME_Y - 16..VOLUME_Y + 34).contains(&y) {
        if x < 112 {
            return Action::VolumeDown;
        }
        if x > 354 {
            return Action::VolumeUp;
        }
        if max_volume > 0 {
            let value = ((x - 115).clamp(0, 236) * max_volume as i32 + 118) / 236;
            return Action::SetVolume(value.clamp(0, max_volume as i32) as u8);
        }
    }
    Action::None
}

pub fn frame(s: &mut impl Surface, m: &Model) {
    s.rect(0, 0, SIZE, SIZE, BLACK_U16);

    if !m.has_snapshot && matches!(m.status, Status::Ask | Status::Denied) {
        let (title, detail) = match m.status {
            Status::Ask => ("Allow on your phone", "Open Music Companion"),
            _ => ("No music access", "Allow it in Music Companion"),
        };
        centre(s, C, 194, title, WHITE);
        centre(s, C, 224, detail, GREY);
        return;
    }

    let active = m.active();
    draw_music_mark(s, active);

    let (state, colour) = if !active {
        ("NO MEDIA", GREY)
    } else if m.playing() {
        ("PLAYING", ACCENT)
    } else {
        ("PAUSED", WHITE)
    };
    centre(s, C, STATUS_Y, state, colour);
    let detail = if active {
        "Music session"
    } else {
        "Open a music app"
    };
    centre(s, C, DETAIL_Y, detail, DIM);

    transport(s, m.playing(), active);
    volume(s, m.volume(), m.max_volume(), active);

    let (footer, colour) = match m.status {
        Status::Ask => ("Allow music on your phone", ACCENT),
        Status::Denied => ("Music access denied", RED),
        Status::Unavailable => ("Phone can't answer", ACCENT),
        Status::Silent if m.has_snapshot => ("No phone reply - showing cached", ACCENT),
        Status::Silent => ("Phone not answering", ACCENT),
        Status::Waiting => ("Waiting for phone", DIM),
        Status::Ok => ("Connected to phone", DIM),
    };
    let (x, cells) = text_box(FOOTER_Y, 8);
    fitted(s, x, FOOTER_Y, footer, cells, colour);
}

/// Draws a generic two-note mark; it contains no information about the current track.
fn draw_music_mark(s: &mut impl Surface, active: bool) {
    let colour = if active { ACCENT } else { DIM };
    let fill = if active {
        Rgb(250, 35, 0)
    } else {
        Rgb(82, 82, 82)
    };
    s.rect(174, 72, 6, 52, colour);
    s.rect(228, 62, 6, 62, colour);
    s.rect(180, 72, 48, 6, colour);
    s.rect(234, 62, 48, 6, colour);
    cmf_wp2_ui::draw::disc(s, 171, 124, 12, fill, BLACK);
    cmf_wp2_ui::draw::disc(s, 225, 114, 12, fill, BLACK);
}

fn transport(s: &mut impl Surface, playing: bool, active: bool) {
    let colour = if active { WHITE } else { DIM };
    let icon_colour = if active { BLACK_U16 } else { DIM };
    // The focused play/pause button has a stock accent disc behind it.
    if active {
        cmf_wp2_ui::draw::disc(s, PLAY_X, CONTROL_Y, 28, Rgb(250, 35, 0), BLACK);
    }
    // Previous: a vertical bar at the left and a triangle whose point faces left.
    s.rect(PREV_X - 10, CONTROL_Y - 11, 4, 22, colour);
    triangle_left(s, PREV_X + 11, CONTROL_Y, 12, 11, colour);
    // Next: a triangle whose point faces right and a vertical bar at the right.
    triangle_right(s, NEXT_X - 10, CONTROL_Y, 12, 11, colour);
    s.rect(NEXT_X + 7, CONTROL_Y - 11, 4, 22, colour);

    if playing {
        s.rect(PLAY_X - 7, CONTROL_Y - 13, 5, 26, icon_colour);
        s.rect(PLAY_X + 2, CONTROL_Y - 13, 5, 26, icon_colour);
    } else {
        triangle_right(s, PLAY_X - 7, CONTROL_Y, 14, 14, icon_colour);
    }
}

/// Draws a solid left-pointing triangle with its point at `x - half_width`.
fn triangle_left(
    s: &mut impl Surface,
    x: i32,
    y: i32,
    half_width: i32,
    half_height: i32,
    colour: u16,
) {
    for row in 0..=half_height {
        let width = half_width - row;
        if width > 0 {
            s.rect(x - width, y - row, width, 1, colour);
            s.rect(x - width, y + row, width, 1, colour);
        }
    }
}

/// Draws a solid right-pointing triangle with its point at `x + half_width - 1`.
fn triangle_right(
    s: &mut impl Surface,
    x: i32,
    y: i32,
    half_width: i32,
    half_height: i32,
    colour: u16,
) {
    for row in 0..=half_height {
        let width = half_width - row;
        if width > 0 {
            s.rect(x, y - row, width, 1, colour);
            s.rect(x, y + row, width, 1, colour);
        }
    }
}

fn volume(s: &mut impl Surface, value: u8, max: u8, active: bool) {
    let colour = if active { WHITE } else { DIM };
    s.rect(115, VOLUME_Y, 236, 5, DIM);
    if max > 0 {
        let width = ((value.min(max) as i32 * 236) / max as i32).clamp(0, 236);
        s.rect(115, VOLUME_Y, width, 5, ACCENT);
    }
    s.rect(87, VOLUME_Y - 11, 15, 3, colour);
    s.rect(99, VOLUME_Y - 17, 3, 15, colour);
    s.rect(364, VOLUME_Y - 11, 15, 3, colour);
    s.rect(364, VOLUME_Y - 17, 3, 15, colour);

    let mut percent = crate::model::Buf::<8>::new();
    crate::model::percent(&mut percent, value, max);
    centre(s, C, VOLUME_Y + 17, percent.as_str(), colour);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Model;
    use cmf_wp2_ui::Fb;

    #[test]
    fn every_state_draws_inside_the_circle() {
        let mut px = vec![0u16; (SIZE * SIZE) as usize];
        let mut fb = Fb::new(&mut px);
        let mut m = Model::new();
        frame(&mut fb, &m);
        m.status = Status::Ask;
        frame(&mut fb, &m);
        m.status = Status::Denied;
        frame(&mut fb, &m);
        for y in 0..SIZE {
            for x in 0..SIZE {
                let (dx, dy) = (x - C, y - C);
                if dx * dx + dy * dy > (C - 2) * (C - 2) {
                    assert_eq!(fb.at(x, y), 0, "{x},{y}");
                }
            }
        }
    }

    #[test]
    fn controls_have_distinct_touch_targets() {
        assert_eq!(action(PREV_X, CONTROL_Y, 15), Action::Previous);
        assert_eq!(action(PLAY_X, CONTROL_Y, 15), Action::Toggle);
        assert_eq!(action(NEXT_X, CONTROL_Y, 15), Action::Next);
        assert_eq!(action(75, VOLUME_Y, 15), Action::VolumeDown);
        assert_eq!(action(390, VOLUME_Y, 15), Action::VolumeUp);
        assert_eq!(action(C, VOLUME_Y, 15), Action::SetVolume(8));
        assert_eq!(action(C, VOLUME_Y, 0), Action::None);
    }

    #[test]
    fn transport_triangles_point_in_the_right_direction() {
        let mut px = vec![0u16; (SIZE * SIZE) as usize];
        let mut fb = Fb::new(&mut px);
        triangle_left(&mut fb, 100, 100, 12, 11, WHITE);
        assert_eq!(fb.at(88, 100), WHITE);
        assert_eq!(fb.at(99, 100), WHITE);
        assert_eq!(fb.at(100, 100), 0);
        assert_eq!(fb.at(87, 100), 0);
        assert_eq!(fb.at(99, 89), WHITE);

        let mut px = vec![0u16; (SIZE * SIZE) as usize];
        let mut fb = Fb::new(&mut px);
        triangle_right(&mut fb, 100, 100, 12, 11, WHITE);
        assert_eq!(fb.at(100, 100), WHITE);
        assert_eq!(fb.at(111, 100), WHITE);
        assert_eq!(fb.at(112, 100), 0);
    }
}
