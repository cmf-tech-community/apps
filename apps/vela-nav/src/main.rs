#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]

//! Vela Nav: shows the drive that the Vela maps app on the phone is navigating. Protocol: `proto.rs`.

// the host build (tests) has no device loop, so some of the protocol is unused there
#[cfg_attr(not(any(target_os = "none", wp2_sim)), allow(dead_code))]
mod proto;
#[cfg_attr(not(any(target_os = "none", wp2_sim)), allow(dead_code))]
mod view;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
mod device {
    use crate::proto::{ASK, Effect, Model};
    use crate::view::{self, Ctx, ZOOM_DEFAULT, ZOOMS};
    use cmf_wp2_app::{Api, BUTTON, CROWN, MSG_MAX, PAUSE, RESUME};

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();

    /// Ask for a snapshot again this often while none comes.
    const ASK_MS: u32 = 3000;
    /// The phone sends the drive every second; after this long without one it is out of reach.
    const STALE_MS: u32 = 6000;
    /// At most this often a redraw (a route arrives as a burst of messages).
    const FRAME_MS: u32 = 150;

    // 4 KiB of route: too big for the 8 KiB stack
    static mut MODEL: Model = Model::new();

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
        let mut zoom = ZOOM_DEFAULT;
        let mut want;
        let mut asked: Option<u32> = None;
        let mut heard: Option<u32> = None;
        let mut dirty = true;
        let mut drawn_at = 0u32;
        let mut live = false;
        let mut screen_on = false;
        loop {
            let now = api.now();
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
            let on = live && m.state.active();
            if on != screen_on {
                screen_on = on;
                api.keep_screen_on(on);
            }
            if dirty && now.wrapping_sub(drawn_at) >= FRAME_MS {
                let clock = api.time().map(|t| (t.hour, t.minute));
                let mut screen = api;
                view::draw(&mut screen, m, &Ctx { live, clock, zoom });
                api.show();
                (dirty, drawn_at) = (false, now);
            }
            let wait = if dirty { FRAME_MS } else { 1000 };
            match api.event(wait) {
                Some(e) if e.kind == BUTTON => return 0,
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
                    Effect::Changed => dirty = true,
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
