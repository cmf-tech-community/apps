#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]

//! Vela Nav: shows the drive that the Vela maps app on the phone is navigating. Protocol: `proto.rs`.

// the host build (tests) has no device loop, so some of the protocol is unused there
#[cfg_attr(not(any(target_os = "none", wp2_sim)), allow(dead_code))]
mod font;
#[rustfmt::skip]
mod fonts;
#[cfg_attr(not(any(target_os = "none", wp2_sim)), allow(dead_code))]
mod proto;
mod sign;
#[cfg_attr(not(any(target_os = "none", wp2_sim)), allow(dead_code))]
mod view;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
mod device {
    use crate::proto::{self, ASK, Effect, Model};
    use crate::view::{self, Ctx, Fade, Glide, Pose, ZOOM_DEFAULT, ZOOMS};
    use cmf_wp2_app::{Api, BUTTON, CROWN, MSG_MAX, PAUSE, RESUME, TOUCH_DOWN};
    use cmf_wp2_ui::haptic::{self, Pattern};

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();

    /// Ask for a snapshot again this often while none comes.
    const ASK_MS: u32 = 3000;
    /// The phone sends the drive every second; after this long without one it is out of reach.
    const STALE_MS: u32 = 6000;
    /// At most this often a redraw (a route arrives as a burst of messages), and the shortest frame of a glide;
    /// a glide's frames are spaced by what the last one took to draw, so a busy map gets fewer, not a stall.
    const FRAME_MS: u32 = 80;

    // 4 KiB of route: too big for the 8 KiB stack
    static mut MODEL: Model = Model::new();
    static mut FADE: Fade = Fade::new();

    /// How long a tapped button waits for the drive to start before it is a button again.
    const START_WAIT_MS: u32 = 30_000;

    /// Plays a short pattern, waiting it out (it is under 200 ms), and leaves the motor off.
    fn buzz(api: &Api, p: Pattern) {
        for &(duty, ms) in p.steps() {
            api.vibrate(duty, haptic::HZ);
            let t = api.now();
            while api.now().wrapping_sub(t) < ms as u32 {}
        }
        api.vibrate(0, haptic::HZ);
    }

    // true on RESUME, false on BUTTON
    fn paused(api: &Api) -> bool {
        loop {
            match api.event(u32::MAX).map(|e| e.kind) {
                Some(RESUME) => return true,
                Some(BUTTON) => return false,
                _ => {}
            }
        }
    }

    fn main(api: &'static Api) -> i32 {
        // SAFETY: the app has one thread and this is the only reference.
        let m = unsafe { &mut *core::ptr::addr_of_mut!(MODEL) };
        // SAFETY: as above.
        let fade = unsafe { &mut *core::ptr::addr_of_mut!(FADE) };
        let mut zoom = ZOOM_DEFAULT;
        let mut want;
        let mut asked: Option<u32> = None;
        let mut heard: Option<u32> = None;
        let mut dirty = true;
        let mut drawn_at = 0u32;
        let mut live = false;
        let mut screen_on = false;
        let mut glide = Glide::new();
        let mut shown: Option<Pose> = None;
        let mut frame_ms = FRAME_MS;
        // the Home / Work button tapped, and when
        let mut pending: Option<(u8, u32)> = None;
        loop {
            let now = api.now();
            // the car glides between the phone's updates: redraw when it has moved a pixel
            let pose = glide.at(now);
            let moving = glide.moving(now);
            if moving && shown.is_none_or(|s| s.moved_px(pose, ZOOMS[zoom]) >= 1) {
                dirty = true;
            }
            want = !m.synced;
            if want && asked.is_none_or(|t| now.wrapping_sub(t) >= ASK_MS) {
                api.send(&ASK); // -107 while Vela is not listening: asked again in 3 s
                asked = Some(now);
            }
            let is_live = heard.is_some_and(|t| now.wrapping_sub(t) < STALE_MS);
            if is_live != live {
                live = is_live;
                dirty = true;
                if !live {
                    (m.synced, asked) = (false, None);
                    m.reset_seq();
                }
            }
            // a tapped button is lit until the drive starts, the phone says it could not, or a while passes
            if let Some((_, at)) = pending
                && (m.state.active() || m.start == proto::START_FAILED || now.wrapping_sub(at) > START_WAIT_MS)
            {
                (pending, dirty) = (None, true);
            }
            let on = live && m.state.active();
            if on != screen_on {
                screen_on = on;
                api.keep_screen_on(on);
            }
            if dirty && now.wrapping_sub(drawn_at) >= frame_ms {
                let clock = api.time().map(|t| (t.hour, t.minute));
                let mut screen = api;
                let ctx = Ctx { live, clock, zoom, pose: Some(pose), pending: pending.map(|p| p.0) };
                view::draw(&mut screen, m, &ctx, fade);
                api.show();
                // the next frame waits at least as long as this one took, plus a little for the messages
                frame_ms = (api.now().wrapping_sub(now) * 5 / 4).clamp(FRAME_MS, 400);
                (dirty, drawn_at, shown) = (false, now, Some(pose));
            }
            let wait = if dirty || moving { frame_ms } else { 1000 };
            match api.event(wait) {
                Some(e) if e.kind == BUTTON => return 0,
                Some(e) if e.kind == TOUCH_DOWN && live && m.has_state && !m.state.active() && pending.is_none() => {
                    if let Some(kind) = view::button_at(e.y as i32)
                        && m.places & kind != 0
                        && api.send(&proto::action(kind)) >= 0
                    {
                        buzz(api, haptic::TICK);
                        (pending, dirty) = (Some((kind, api.now())), true);
                    }
                }
                Some(e) if e.kind == CROWN => {
                    let z = (zoom as i32 + e.delta.signum() as i32).clamp(0, ZOOMS.len() as i32 - 1) as usize;
                    dirty |= z != zoom;
                    zoom = z;
                }
                Some(e) if e.kind == PAUSE => {
                    screen_on = false;
                    if !paused(api) {
                        return 0;
                    }
                    // messages were refused meanwhile
                    (m.synced, asked, dirty) = (false, None, true);
                    m.reset_seq();
                }
                _ => {}
            }
            let mut buf = [0u8; MSG_MAX];
            while let Some(msg) = api.recv(&mut buf) {
                match m.apply(msg) {
                    Effect::Ignored => continue,
                    Effect::Changed => {
                        dirty = true;
                        glide.target(Pose::of(&m.state), api.now());
                    }
                    Effect::Resync => {
                        dirty = true;
                        if !want {
                            asked = None; // ask right away
                        }
                    }
                }
                heard = Some(api.now());
                want = !m.synced;
            }
        }
    }
}
