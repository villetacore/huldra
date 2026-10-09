//! blocks: falling blocks (a Tetris-like game) for the graphical session.
//!
//! Keys: Left/Right move, Up or x rotate, z rotate back, Down drops one
//! row, Space drops to the bottom, p pauses, n starts again. Every ten
//! lines the level goes up and the blocks fall faster.

#![no_std]
#![no_main]

use huldra_gfx::keymap;
use huldra_user::gui::*;
use huldra_user::rand::Rng;
use huldra_user::time;
use huldra_user::{eprintln, format, vec, Vec};

huldra_user::main!(main);

const W: i32 = 10;
const H: i32 = 20;
const CELL: i32 = 20;
const PAD: i32 = 10;
const SIDE: i32 = 150;
const WIN_W: i32 = W * CELL + 3 * PAD + SIDE;
const WIN_H: i32 = H * CELL + 2 * PAD;

/// The seven pieces in their box (size, cells as (x, y)).
const SHAPES: [(i32, [(i32, i32); 4]); 7] = [
    (4, [(0, 1), (1, 1), (2, 1), (3, 1)]), // I
    (2, [(0, 0), (1, 0), (0, 1), (1, 1)]), // O
    (3, [(1, 0), (0, 1), (1, 1), (2, 1)]), // T
    (3, [(1, 0), (2, 0), (0, 1), (1, 1)]), // S
    (3, [(0, 0), (1, 0), (1, 1), (2, 1)]), // Z
    (3, [(0, 0), (0, 1), (1, 1), (2, 1)]), // J
    (3, [(2, 0), (0, 1), (1, 1), (2, 1)]), // L
];

const COLORS: [u32; 7] = [theme::TEAL, theme::AMBER, 0xFFB08AA6, theme::ACCENT, theme::RUST, 0xFF7FCFC6, 0xFFE8C77A];

#[derive(Clone, Copy)]
struct Piece {
    kind: usize,
    rot: i32,
    x: i32,
    y: i32,
}

impl Piece {
    fn cells(&self) -> [(i32, i32); 4] {
        let (n, base) = SHAPES[self.kind];
        let mut out = base;
        for c in out.iter_mut() {
            for _ in 0..self.rot.rem_euclid(4) {
                *c = (n - 1 - c.1, c.0);
            }
            *c = (c.0 + self.x, c.1 + self.y);
        }
        out
    }
}

struct Game {
    board: Vec<Option<usize>>,
    piece: Piece,
    next: usize,
    bag: Vec<usize>,
    rng: Rng,
    score: u32,
    lines: u32,
    over: bool,
    paused: bool,
    /// Rows being flashed before they disappear.
    clearing: Vec<i32>,
}

impl Game {
    fn new() -> Game {
        let mut g = Game { board: vec![None; (W * H) as usize], piece: Piece { kind: 0, rot: 0, x: 0, y: 0 }, next: 0, bag: Vec::new(), rng: Rng::from_time(), score: 0, lines: 0, over: false, paused: false, clearing: Vec::new() };
        g.next = g.draw_bag();
        g.spawn();
        g
    }

    /// Pieces come in shuffled sets of all seven.
    fn draw_bag(&mut self) -> usize {
        if self.bag.is_empty() {
            self.bag = (0..7).collect();
            let mut bag = core::mem::take(&mut self.bag);
            self.rng.shuffle(&mut bag);
            self.bag = bag;
        }
        self.bag.pop().unwrap_or(0)
    }

    fn level(&self) -> u32 {
        self.lines / 10 + 1
    }

    fn interval(&self) -> u64 {
        (850u64.saturating_sub(self.level() as u64 * 75)).max(90)
    }

    fn fits(&self, p: &Piece) -> bool {
        p.cells().iter().all(|&(x, y)| x >= 0 && x < W && y < H && (y < 0 || self.board[(y * W + x) as usize].is_none()))
    }

    fn spawn(&mut self) {
        let kind = self.next;
        self.next = self.draw_bag();
        let n = SHAPES[kind].0;
        self.piece = Piece { kind, rot: 0, x: (W - n) / 2, y: if kind == 0 { -1 } else { 0 } };
        if !self.fits(&self.piece) {
            self.over = true;
        }
    }

    fn shift(&mut self, dx: i32, dy: i32) -> bool {
        let p = Piece { x: self.piece.x + dx, y: self.piece.y + dy, ..self.piece };
        let ok = self.fits(&p);
        if ok {
            self.piece = p;
        }
        ok
    }

    fn rotate(&mut self, dir: i32) {
        // Simple wall kicks: try the spot, then a step or two aside, then up.
        for (dx, dy) in [(0, 0), (-1, 0), (1, 0), (-2, 0), (2, 0), (0, -1)] {
            let p = Piece { rot: self.piece.rot + dir, x: self.piece.x + dx, y: self.piece.y + dy, ..self.piece };
            if self.fits(&p) {
                self.piece = p;
                return;
            }
        }
    }

    fn ghost(&self) -> Piece {
        let mut p = self.piece;
        loop {
            let q = Piece { y: p.y + 1, ..p };
            if !self.fits(&q) {
                return p;
            }
            p = q;
        }
    }

    fn lock(&mut self) {
        for (x, y) in self.piece.cells() {
            if y < 0 {
                self.over = true;
                return;
            }
            self.board[(y * W + x) as usize] = Some(self.piece.kind);
        }
        self.clearing = (0..H).filter(|&y| (0..W).all(|x| self.board[(y * W + x) as usize].is_some())).collect();
        if self.clearing.is_empty() {
            self.spawn();
        }
    }

    /// Removes the flashed rows and scores them.
    fn finish_clear(&mut self) {
        let n = self.clearing.len() as u32;
        for &y in &self.clearing {
            self.board.drain((y * W) as usize..((y + 1) * W) as usize);
            for _ in 0..W {
                self.board.insert(0, None);
            }
        }
        self.score += [0, 100, 300, 500, 800][n as usize] * self.level();
        self.lines += n;
        self.clearing.clear();
        self.spawn();
    }

    fn fall(&mut self) {
        if !self.shift(0, 1) {
            self.lock();
        }
    }

    fn hard_drop(&mut self) {
        let mut rows = 0;
        while self.shift(0, 1) {
            rows += 1;
        }
        self.score += rows * 2;
        self.lock();
    }
}

fn block(c: &mut Canvas, x: i32, y: i32, color: u32) {
    let r = Rect::new(x, y, CELL, CELL).inset(1);
    c.fill_rect(r, huldra_gfx::canvas::mix(color, theme::BG, 150));
    c.rect_outline(r, color);
    c.fill_rect(r.inset(4), color);
}

fn draw(d: &mut Display, win: u32, font: &Font, g: &Game, flash: bool) {
    let mut c = Canvas::new(WIN_W, WIN_H);
    c.fill_rect(c.bounds(), theme::SURFACE);
    let well = Rect::new(PAD, PAD, W * CELL, H * CELL);
    c.fill_rect(well, theme::BG);
    for y in 0..H {
        for x in 0..W {
            c.put(well.x + x * CELL + CELL / 2, well.y + y * CELL + CELL / 2, theme::LINE_DIM);
        }
    }
    c.rect_outline(well.inset(-1), theme::LINE);
    for y in 0..H {
        let lit = flash && g.clearing.contains(&y);
        for x in 0..W {
            if let Some(k) = g.board[(y * W + x) as usize] {
                block(&mut c, well.x + x * CELL, well.y + y * CELL, if lit { theme::BRIGHT } else { COLORS[k] });
            }
        }
    }
    if !g.over && g.clearing.is_empty() {
        for (x, y) in g.ghost().cells() {
            if y >= 0 {
                c.rect_outline(Rect::new(well.x + x * CELL, well.y + y * CELL, CELL, CELL).inset(2), theme::LINE_DIM);
            }
        }
        for (x, y) in g.piece.cells() {
            if y >= 0 {
                block(&mut c, well.x + x * CELL, well.y + y * CELL, COLORS[g.piece.kind]);
            }
        }
    }
    // The side panel.
    let sx = well.right() + 2 * PAD;
    c.draw_text_bold(font, sx, PAD, "NEXT", theme::TEXT_DIM);
    let preview = Rect::new(sx, PAD + 20, 4 * CELL + 8, 3 * CELL);
    c.fill_rect(preview, theme::BG);
    c.rect_outline(preview, theme::LINE_DIM);
    let (n, cells) = SHAPES[g.next];
    let ox = preview.x + (preview.w - n * CELL) / 2;
    for (x, y) in cells {
        block(&mut c, ox + x * CELL, preview.y + 10 + y * CELL - if n == 4 { CELL / 2 } else { 0 }, COLORS[g.next]);
    }
    let mut y = preview.bottom() + 20;
    for (label, value) in [("SCORE", g.score), ("LINES", g.lines), ("LEVEL", g.level())] {
        c.draw_text(font, sx, y, label, theme::TEXT_DIM, None);
        c.draw_text_bold(font, sx, y + 18, &format!("{:>7}", value), theme::AMBER);
        y += 46;
    }
    for (i, line) in ["<- ->  move", "^ x z  rotate", "v      down", "space  drop", "p      pause", "n      new game"].iter().enumerate() {
        c.draw_text(font, sx, WIN_H - PAD - 16 * (6 - i as i32), line, theme::LINE, None);
    }
    let banner = if g.over {
        Some("GAME OVER")
    } else if g.paused {
        Some("PAUSED")
    } else {
        None
    };
    if let Some(text) = banner {
        let r = Rect::new(well.x + 10, well.y + well.h / 2 - 30, well.w - 20, 60);
        theme::panel(&mut c, r, theme::ACCENT);
        c.draw_text_bold(font, r.x + (r.w - font.text_width(text)) / 2, r.y + 12, text, theme::BRIGHT);
        let hint = if g.over { "n - new game" } else { "p - continue" };
        c.draw_text(font, r.x + (r.w - font.text_width(hint)) / 2, r.y + 34, hint, theme::TEXT_DIM, None);
    }
    d.put_canvas(win, &c, c.bounds(), 0, 0);
    d.flush();
}

fn main() -> i32 {
    let mut d = match Display::connect() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("blocks: cannot connect to the display: {}", e);
            return 1;
        }
    };
    let font = load_font();
    let win = d.create_window(160, 60, WIN_W, WIN_H, KIND_NORMAL);
    d.set_title(win, "Blocks");
    d.map(win);
    d.flush();
    let mut g = Game::new();
    let mut next_tick = time::uptime_ms() + g.interval();
    loop {
        let now = time::uptime_ms();
        let wait = if g.over || g.paused { -1 } else { next_tick.saturating_sub(now).max(1) as i32 };
        let ev = d.wait_event(wait);
        let mut dirty = false;
        match ev {
            Some(Event::Expose { .. }) => dirty = true,
            Some(Event::Key { code, pressed: true, ch, .. }) => {
                let ch = char::from_u32(ch).unwrap_or('\0').to_ascii_lowercase();
                dirty = true;
                if ch == 'n' {
                    g = Game::new();
                    next_tick = time::uptime_ms() + g.interval();
                } else if ch == 'p' && !g.over {
                    g.paused = !g.paused;
                    next_tick = time::uptime_ms() + g.interval();
                } else if !g.over && !g.paused && g.clearing.is_empty() {
                    match (code, ch) {
                        (keymap::LEFT, _) => {
                            g.shift(-1, 0);
                        }
                        (keymap::RIGHT, _) => {
                            g.shift(1, 0);
                        }
                        (keymap::UP, _) | (_, 'x') => g.rotate(1),
                        (_, 'z') => g.rotate(-1),
                        (keymap::DOWN, _) => {
                            if g.shift(0, 1) {
                                g.score += 1;
                            }
                            next_tick = time::uptime_ms() + g.interval();
                        }
                        (keymap::SPACE, _) => {
                            g.hard_drop();
                            next_tick = time::uptime_ms() + if g.clearing.is_empty() { g.interval() } else { 0 };
                        }
                        _ => dirty = false,
                    }
                }
            }
            Some(Event::Focus { focused: false, .. }) if !g.over => {
                g.paused = true;
                dirty = true;
            }
            Some(Event::CloseRequest { .. }) => return 0,
            Some(_) => {}
            None if d.closed => return 0,
            None => {}
        }
        if !g.over && !g.paused && time::uptime_ms() >= next_tick {
            if g.clearing.is_empty() {
                g.fall();
            }
            if !g.clearing.is_empty() {
                // Flash the full rows for a moment, then drop them.
                draw(&mut d, win, &font, &g, true);
                time::sleep_ms(120);
                g.finish_clear();
            }
            next_tick = time::uptime_ms() + g.interval();
            dirty = true;
        }
        if dirty {
            draw(&mut d, win, &font, &g, false);
        }
    }
}
