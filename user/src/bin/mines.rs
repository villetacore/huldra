//! mines: minesweeper, in the colors of the system.
//!
//! Left click opens a cell, right click puts or removes a flag; clicking
//! an opened number whose flags are all placed opens its neighbours. The
//! first click is always safe. Keys: n new game, 1/2/3 size.

#![no_std]
#![no_main]

use huldra_gfx::keymap;
use huldra_user::gui::*;
use huldra_user::rand::Rng;
use huldra_user::time;
use huldra_user::{eprintln, format, vec, Vec};

huldra_user::main!(main);

const CELL: i32 = 22;
const HEAD: i32 = 36;
const PAD: i32 = 8;
const LEVELS: [(i32, i32, usize); 3] = [(9, 9, 10), (16, 16, 40), (24, 16, 70)];

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Ready,
    Playing,
    Won,
    Lost,
}

struct Game {
    cols: i32,
    rows: i32,
    mines: usize,
    mine: Vec<bool>,
    open: Vec<bool>,
    flag: Vec<bool>,
    state: State,
    started: u64,
    elapsed: u64,
    /// The mine that went off.
    boom: Option<usize>,
}

impl Game {
    fn new(level: usize) -> Game {
        let (cols, rows, mines) = LEVELS[level];
        let n = (cols * rows) as usize;
        Game { cols, rows, mines, mine: vec![false; n], open: vec![false; n], flag: vec![false; n], state: State::Ready, started: 0, elapsed: 0, boom: None }
    }

    fn size(&self) -> (i32, i32) {
        (self.cols * CELL + 2 * PAD, HEAD + self.rows * CELL + 2 * PAD)
    }

    fn neighbours(&self, i: usize) -> Vec<usize> {
        let (x, y) = (i as i32 % self.cols, i as i32 / self.cols);
        let mut v = Vec::new();
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (nx, ny) = (x + dx, y + dy);
                if (dx, dy) != (0, 0) && nx >= 0 && ny >= 0 && nx < self.cols && ny < self.rows {
                    v.push((ny * self.cols + nx) as usize);
                }
            }
        }
        v
    }

    fn count(&self, i: usize) -> usize {
        self.neighbours(i).into_iter().filter(|&j| self.mine[j]).count()
    }

    /// Lays the mines away from the first click.
    fn lay(&mut self, first: usize) {
        let mut rng = Rng::from_time();
        let keep: Vec<usize> = self.neighbours(first);
        let mut cells: Vec<usize> = (0..self.mine.len()).filter(|&i| i != first && !keep.contains(&i)).collect();
        rng.shuffle(&mut cells);
        for &i in cells.iter().take(self.mines) {
            self.mine[i] = true;
        }
        self.state = State::Playing;
        self.started = time::uptime_ms();
    }

    fn reveal(&mut self, i: usize) {
        if self.state == State::Ready {
            self.lay(i);
        }
        if self.state != State::Playing || self.flag[i] {
            return;
        }
        if self.open[i] {
            // Chord: open the neighbours of a satisfied number.
            let around = self.neighbours(i);
            let flags = around.iter().filter(|&&j| self.flag[j]).count();
            if flags == self.count(i) {
                for j in around {
                    if !self.open[j] && !self.flag[j] {
                        self.open_cell(j);
                    }
                }
            }
        } else {
            self.open_cell(i);
        }
        if self.state == State::Playing && (0..self.mine.len()).all(|j| self.mine[j] || self.open[j]) {
            self.state = State::Won;
            for j in 0..self.mine.len() {
                self.flag[j] = self.mine[j];
            }
        }
        if self.state != State::Playing {
            self.elapsed = time::uptime_ms() - self.started;
        }
    }

    fn open_cell(&mut self, i: usize) {
        if self.mine[i] {
            self.open[i] = true;
            self.boom = Some(i);
            self.state = State::Lost;
            return;
        }
        // Flood fill from empty cells.
        let mut stack = Vec::from([i]);
        while let Some(j) = stack.pop() {
            if self.open[j] || self.flag[j] {
                continue;
            }
            self.open[j] = true;
            if self.count(j) == 0 {
                stack.extend(self.neighbours(j).into_iter().filter(|&k| !self.open[k]));
            }
        }
    }

    fn toggle_flag(&mut self, i: usize) {
        if matches!(self.state, State::Ready | State::Playing) && !self.open[i] {
            self.flag[i] = !self.flag[i];
        }
    }

    fn seconds(&self) -> u64 {
        match self.state {
            State::Ready => 0,
            State::Playing => (time::uptime_ms() - self.started) / 1000,
            _ => self.elapsed / 1000,
        }
    }

    fn cell_at(&self, x: i32, y: i32) -> Option<usize> {
        let (cx, cy) = ((x - PAD).div_euclid(CELL), (y - HEAD - PAD).div_euclid(CELL));
        (x >= PAD && y >= HEAD + PAD && cx < self.cols && cy < self.rows).then(|| (cy * self.cols + cx) as usize)
    }
}

fn new_button(w: i32) -> Rect {
    Rect::new(w / 2 - 32, 7, 64, 22)
}

fn draw(d: &mut Display, win: u32, font: &Font, g: &Game) {
    let (w, h) = g.size();
    let mut c = Canvas::new(w, h);
    c.fill_rect(c.bounds(), theme::SURFACE);
    // Header: mines left, the new game button, time.
    let left = g.mines as i64 - g.flag.iter().filter(|&&f| f).count() as i64;
    let counter = |c: &mut Canvas, x: i32, text: &str| {
        let r = Rect::new(x, 7, 52, 22);
        c.fill_rect(r, theme::BG);
        c.rect_outline(r, theme::LINE_DIM);
        c.draw_text_bold(font, r.x + (r.w - font.text_width(text)) / 2, r.y + 3, text, theme::AMBER);
    };
    counter(&mut c, PAD, &format!("{:03}", left.clamp(-99, 999)));
    counter(&mut c, w - PAD - 52, &format!("{:03}", g.seconds().min(999)));
    let label = match g.state {
        State::Won => "WIN!",
        State::Lost => "BOOM",
        _ => "NEW",
    };
    theme::button(&mut c, font, new_button(w), label, g.state == State::Won);
    // The field.
    let field = Rect::new(PAD - 1, HEAD + PAD - 1, g.cols * CELL + 2, g.rows * CELL + 2);
    c.rect_outline(field, theme::LINE);
    let over = g.state == State::Lost;
    for i in 0..g.mine.len() {
        let (x, y) = (PAD + (i as i32 % g.cols) * CELL, HEAD + PAD + (i as i32 / g.cols) * CELL);
        let r = Rect::new(x, y, CELL, CELL);
        let inner = r.inset(1);
        if g.open[i] || (over && g.mine[i] && !g.flag[i]) {
            c.fill_rect(inner, if g.boom == Some(i) { theme::RUST } else { theme::BG });
            if g.mine[i] {
                let col = if g.boom == Some(i) { theme::BG } else { theme::RUST };
                c.fill_circle(x + CELL / 2, y + CELL / 2, 5, col);
                c.thick_line(x + 4, y + CELL / 2, x + CELL - 5, y + CELL / 2, 1, col);
                c.thick_line(x + CELL / 2, y + 4, x + CELL / 2, y + CELL - 5, 1, col);
            } else {
                let n = g.count(i);
                if n > 0 {
                    let col = [theme::TEAL, theme::ACCENT, theme::AMBER, theme::RUST, theme::BRIGHT][(n - 1).min(4)];
                    let s = format!("{}", n);
                    c.draw_text_bold(font, x + (CELL - 8) / 2, y + (CELL - 16) / 2, &s, col);
                }
            }
        } else {
            c.fill_rect(inner, theme::RAISED);
            c.fill_rect(Rect::new(inner.x, inner.y, inner.w, 1), theme::HOVER);
            if g.flag[i] {
                let wrong = over && !g.mine[i];
                let col = if wrong { theme::RUST } else { theme::AMBER };
                c.fill_rect(Rect::new(x + 8, y + 5, 2, 12), theme::TEXT);
                for k in 0..6 {
                    c.fill_rect(Rect::new(x + 10, y + 5 + k, 6 - k, 1), col);
                    c.fill_rect(Rect::new(x + 10, y + 10 - k, 6 - k, 1), col);
                }
                c.fill_rect(Rect::new(x + 5, y + 16, 8, 2), theme::TEXT);
            }
        }
        c.rect_outline(r, theme::LINE_DIM);
    }
    d.put_canvas(win, &c, c.bounds(), 0, 0);
    d.flush();
}

fn main() -> i32 {
    let mut d = match Display::connect() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("mines: cannot connect to the display: {}", e);
            return 1;
        }
    };
    let font = load_font();
    let mut level = 1;
    let mut g = Game::new(level);
    let (w, h) = g.size();
    let win = d.create_window(120, 80, w, h, KIND_NORMAL);
    d.set_title(win, "Mines");
    d.map(win);
    d.flush();
    let mut shown = u64::MAX;
    loop {
        let ev = d.wait_event(250);
        match ev {
            Some(Event::Expose { .. }) => draw(&mut d, win, &font, &g),
            Some(Event::Button { x, y, button, pressed: true, .. }) => {
                let (w, _) = g.size();
                if new_button(w).contains(x, y) {
                    g = Game::new(level);
                } else if let Some(i) = g.cell_at(x, y) {
                    if button == BUTTON_LEFT {
                        g.reveal(i);
                    } else if button == BUTTON_RIGHT {
                        g.toggle_flag(i);
                    }
                }
                draw(&mut d, win, &font, &g);
            }
            Some(Event::Key { code, pressed: true, ch, .. }) => {
                let ch = char::from_u32(ch).unwrap_or('\0');
                if ch == 'n' || code == keymap::F1 + 1 {
                    g = Game::new(level);
                } else if let Some(l) = ch.to_digit(10).filter(|l| (1..=3).contains(l)) {
                    level = l as usize - 1;
                    g = Game::new(level);
                    let (w, h) = g.size();
                    // Position 0 keeps the window where it is (the window manager decides).
                    d.configure(win, Rect::new(0, 0, w, h));
                }
                draw(&mut d, win, &font, &g);
            }
            Some(Event::CloseRequest { .. }) => return 0,
            Some(_) => {}
            None if d.closed => return 0,
            None => {}
        }
        // Tick the timer.
        if g.state == State::Playing && g.seconds() != shown {
            shown = g.seconds();
            draw(&mut d, win, &font, &g);
        }
    }
}
