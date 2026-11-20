#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]

#[cfg_attr(not(any(target_os = "none", wp2_sim)), allow(dead_code))]
mod game;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
mod device {
    use crate::game::{Dir, Game};
    use cmf_wp2_app::{Api, BUTTON, CROWN, PAUSE, RESUME, TOUCH_DOWN};

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();

    const BG: u16 = 0x0000;
    const WHITE: u16 = 0xFFFF;
    const BOARD: u16 = 0x2124;
    const EMPTY: u16 = 0x39E7;
    const ORANGE: u16 = 0xFD20;
    const MINT: u16 = 0x07E0;
    const RED: u16 = 0xF800;
    const CELL: i16 = 72;
    const GAP: i16 = 5;
    const LEFT: i16 = 84;
    const TOP: i16 = 105;
    const RESET_X: i16 = 183;
    const RESET_Y: i16 = 52;
    const LEFT_X: i16 = 42;
    const RIGHT_X: i16 = 332;
    const ARROW_Y: i16 = 205;
    const ARROW_W: u16 = 68;
    const ARROW_H: u16 = 100;

    fn tile_color(v: u16) -> u16 {
        match v {
            0 => EMPTY, 2 => 0xDEFB, 4 => 0xDEB6, 8 => 0xFD20,
            16 => 0xFC60, 32 => 0xFBA0, 64 => 0xF920,
            128 => 0xFFE0, 256 => 0xD7E0, 512 => 0xAFE0,
            1024 => 0x87E0, _ => MINT,
        }
    }

    fn draw(api: &Api, game: &Game, _dir: Dir) {
        api.rect(0, 0, api.width, api.height, BG);
        api.rect(LEFT - 8, TOP - 8, 4 * CELL as u16 + 5 * GAP as u16 + 16, 4 * CELL as u16 + 5 * GAP as u16 + 16, BOARD);
        for row in 0..4 {
            for col in 0..4 {
                let x = LEFT + col * (CELL + GAP);
                let y = TOP + row * (CELL + GAP);
                let v = game.cells[(row * 4 + col) as usize];
                api.rect(x, y, CELL as u16, CELL as u16, tile_color(v));
                if v != 0 {
                    let mut digits = [0u8; 10];
                    let len = decimal(v as u32, &mut digits);
                    let tx = x + (CELL - len as i16 * 16) / 2;
                    api.text(tx, y + 28, core::str::from_utf8(&digits[..len]).unwrap_or("?"), if v >= 128 { WHITE } else { 0 });
                }
            }
        }
        let mut score_digits = [0u8; 10];
        let score_len = decimal(game.score, &mut score_digits);
        api.text(88, 61, core::str::from_utf8(&score_digits[..score_len]).unwrap_or("0"), WHITE);
        let mut best_digits = [0u8; 10];
        let best_len = decimal(game.best, &mut best_digits);
        api.text(354, 61, core::str::from_utf8(&best_digits[..best_len]).unwrap_or("0"), WHITE);
        api.rect(RESET_X, RESET_Y, 100, 42, ORANGE);
        api.text(209, 87, "RESET", BG);

        // Left/right tap buttons sit beside the board inside the circular safe area.
        api.rect(LEFT_X, ARROW_Y, ARROW_W, ARROW_H, EMPTY);
        api.rect(RIGHT_X, ARROW_Y, ARROW_W, ARROW_H, EMPTY);
        api.text(LEFT_X + 26, ARROW_Y + 42, "<", WHITE);
        api.text(RIGHT_X + 26, ARROW_Y + 42, ">", WHITE);
        if game.won { api.rect(199, 393, 68, 5, MINT); }
        else if game.over() { api.rect(199, 393, 68, 5, RED); }
        api.show();
    }

    fn decimal(mut n: u32, out: &mut [u8; 10]) -> usize {
        if n == 0 { out[0] = b'0'; return 1; }
        let mut len = 0;
        while n > 0 && len < out.len() { out[len] = b'0' + (n % 10) as u8; n /= 10; len += 1; }
        out[..len].reverse();
        len
    }

    fn direction(i: usize) -> Dir {
        match i % 4 { 0 => Dir::Left, 1 => Dir::Up, 2 => Dir::Right, _ => Dir::Down }
    }

    fn main(api: &'static Api) -> i32 {
        let mut saved = [0u8; 4];
        let got = api.load(&mut saved);
        let best = if got == 4 { u32::from_le_bytes(saved) } else { 0 };
        let seed = api.now().wrapping_mul(2_654_435_761) ^ best.rotate_left(11);
        let mut game = Game::new(seed, best);
        game.restart();
        api.own_back(true);
        let mut selected = 0usize;
        draw(api, &game, direction(selected));
        loop {
            let Some(e) = api.event(u32::MAX) else { continue };
            let mut changed = false;
            match e.kind {
                BUTTON => { api.own_back(false); return 0; }
                PAUSE => loop {
                    match api.event(u32::MAX).map(|e| e.kind) {
                        Some(RESUME) => { changed = true; break; }
                        Some(BUTTON) => { api.own_back(false); return 0; }
                        _ => {}
                    }
                },
                CROWN if e.delta != 0 => {
                    // Crown controls only vertical moves: one detent per move.
                    let d = if e.delta > 0 { Dir::Up } else { Dir::Down };
                    selected = if d == Dir::Up { 1 } else { 3 };
                    if game.over() { game.restart(); } else { game.move_dir(d); }
                    changed = true;
                }
                TOUCH_DOWN => {
                    // Read touch-down only: this loader doesn't provide drag events and
                    // touch-up coordinates are unreliable. own_back keeps right-swipe BACK
                    // in the event queue instead of immediately force-stopping the game.
                    if (RESET_Y..RESET_Y + 42).contains(&e.y) && (RESET_X..RESET_X + 100).contains(&e.x) {
                        game.restart();
                        changed = true;
                    } else if (ARROW_Y..ARROW_Y + ARROW_H as i16).contains(&e.y) {
                        let d = if (LEFT_X..LEFT_X + ARROW_W as i16).contains(&e.x) { Some(Dir::Left) }
                            else if (RIGHT_X..RIGHT_X + ARROW_W as i16).contains(&e.x) { Some(Dir::Right) }
                            else { None };
                        if let Some(d) = d {
                            selected = match d { Dir::Left => 0, Dir::Up => 1, Dir::Right => 2, Dir::Down => 3 };
                            if game.over() { game.restart(); } else { game.move_dir(d); }
                            changed = true;
                        }
                    }
                }
                RESUME => changed = true,
                _ => {}
            }
            if changed {
                let bytes = game.best.to_le_bytes();
                api.save(&bytes);
                draw(api, &game, direction(selected));
            }
        }
    }
}
