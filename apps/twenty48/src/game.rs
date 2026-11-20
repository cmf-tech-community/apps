pub const N: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir { Left, Up, Right, Down }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Game {
    pub cells: [u16; 16],
    pub score: u32,
    pub best: u32,
    seed: u32,
    pub won: bool,
}

impl Game {
    pub const fn new(seed: u32, best: u32) -> Self {
        Self { cells: [0; 16], score: 0, best, seed: seed | 1, won: false }
    }

    fn random(&mut self) -> u32 {
        let mut x = self.seed;
        x ^= x << 13; x ^= x >> 17; x ^= x << 5;
        self.seed = x;
        x
    }

    pub fn spawn(&mut self) -> bool {
        let empty = self.cells.iter().filter(|&&v| v == 0).count();
        if empty == 0 { return false; }
        let mut pick = (self.random() as usize) % empty;
        for i in 0..self.cells.len() {
            if self.cells[i] == 0 {
                if pick == 0 {
                    let value = if self.random() % 10 == 0 { 4 } else { 2 };
                    self.cells[i] = value;
                    return true;
                }
                pick -= 1;
            }
        }
        false
    }

    fn line_index(dir: Dir, line: usize, pos: usize) -> usize {
        match dir {
            Dir::Left => line * 4 + pos,
            Dir::Right => line * 4 + (3 - pos),
            Dir::Up => pos * 4 + line,
            Dir::Down => (3 - pos) * 4 + line,
        }
    }

    pub fn move_dir(&mut self, dir: Dir) -> bool {
        let old = self.cells;
        for line in 0..4 {
            let mut vals = [0u16; 4];
            let mut n = 0;
            for pos in 0..4 {
                let v = self.cells[Self::line_index(dir, line, pos)];
                if v != 0 { vals[n] = v; n += 1; }
            }
            let mut out = [0u16; 4];
            let (mut i, mut j) = (0, 0);
            while i < n {
                if i + 1 < n && vals[i] == vals[i + 1] {
                    out[j] = vals[i].saturating_mul(2);
                    self.score = self.score.saturating_add(out[j] as u32);
                    i += 2;
                } else { out[j] = vals[i]; i += 1; }
                j += 1;
            }
            for pos in 0..4 { self.cells[Self::line_index(dir, line, pos)] = out[pos]; }
        }
        let changed = self.cells != old;
        if changed {
            self.best = self.best.max(self.score);
            self.spawn();
            if self.cells.contains(&2048) { self.won = true; }
        }
        changed
    }

    pub fn over(&self) -> bool {
        if self.cells.contains(&0) { return false; }
        for y in 0..4 { for x in 0..4 {
            let i = y * 4 + x;
            if x < 3 && self.cells[i] == self.cells[i + 1] { return false; }
            if y < 3 && self.cells[i] == self.cells[i + 4] { return false; }
        }}
        true
    }

    pub fn restart(&mut self) {
        self.cells = [0; 16]; self.score = 0; self.won = false;
        self.spawn(); self.spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(cells: [u16; 16]) -> Game { Game { cells, score: 0, best: 0, seed: 12345, won: false } }

    #[test]
    fn left_merges_once_and_scores() {
        let mut g = ready([2, 2, 2, 2, 0, 0, 0, 0, 4, 4, 8, 0, 0, 0, 0, 0]);
        assert!(g.move_dir(Dir::Left));
        assert_eq!(&g.cells[0..4], &[4, 4, 0, 0]);
        assert_eq!(&g.cells[8..12], &[8, 8, 0, 0]);
        assert_eq!(g.score, 16); // 2+2+2+2 -> 4+4 scores 8, 4+4 -> 8 scores 8
    }

    #[test]
    fn vertical_and_reverse_moves() {
        let mut g = ready([2,0,0,0, 2,0,0,0, 4,0,0,0, 0,0,0,0]);
        g.move_dir(Dir::Up);
        assert_eq!(&g.cells[0..4], &[4,0,0,0]);
        assert_eq!(&g.cells[4..8], &[4,0,0,0]);
        let mut h = ready([2,2,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,0]);
        h.move_dir(Dir::Right);
        assert_eq!(&h.cells[0..4], &[0,0,0,4]);
    }

    #[test]
    fn full_board_detects_legal_move_and_game_over() {
        let mut g = ready([2,2,4,8, 16,32,64,128, 256,512,1024,2, 4,8,16,32]);
        assert!(!g.over());
        assert!(g.move_dir(Dir::Left));
        let mut h = ready([2,4,8,16, 32,64,128,256, 512,1024,2,4, 8,16,32,64]);
        assert!(h.over());
        assert!(!h.move_dir(Dir::Left));
    }

    #[test]
    fn restart_keeps_best_and_spawns_two_tiles() {
        let mut g = ready([0; 16]); g.best = 2048; g.restart();
        assert_eq!(g.cells.iter().filter(|&&v| v != 0).count(), 2);
        assert_eq!(g.best, 2048);
        assert_eq!(g.score, 0);
    }
}
