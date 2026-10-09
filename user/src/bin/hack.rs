//! hack: break into a security terminal (the password minigame of old
//! green-screen terminals).
//!
//! A memory dump hides a dozen words of the same length; one of them is
//! the password. Pick a word: if it is wrong, the terminal tells how many
//! letters stand in the right place ("likeness"). Four attempts, then the
//! terminal locks. Bracket pairs such as `(..)`, `[..]`, `{..}` and `<..>`
//! on one line remove a dud word or restore the attempts.
//!
//! Keys: arrows (or hjkl) move, Enter selects, n new game, q quits.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use huldra_user::rand::Rng;
use huldra_user::term::{self, Key, Keys, RawMode, Screen, Style};
use huldra_user::{eprintln, format};

huldra_user::main!(main);

const COLS: usize = 12;
const ROWS: usize = 16;
const BLOCK: usize = COLS * ROWS;
const SIZE: usize = 2 * BLOCK;
const ATTEMPTS: u32 = 4;
const WORDS: usize = 12;
const JUNK: &[u8] = b"!\"#$%&'()*+,-./:;<=>?@[\\]^_{|}`~";

const WORDS5: &[&str] = &[
    "ARMOR", "BLAST", "CRATE", "DEPOT", "EMBER", "FLAME", "GHOST", "HATCH", "IRONS", "JOINT", "KNIFE", "LASER", "METAL", "NERVE", "ORBIT", "PANIC", "QUEST",
    "RADIO", "SCRAP", "TOWER", "UNITS", "VAULT", "WASTE", "YIELD", "ZONES", "STEAM", "STORM", "SHELL", "SPARK", "TRACE", "TRAIL", "SKULL", "SLATE", "PLANT",
    "POWER", "GRAIN", "CRANE", "BRICK", "CABLE", "DRONE",
];
const WORDS6: &[&str] = &[
    "ATOMIC", "BUNKER", "CANYON", "DESERT", "ENERGY", "FALLEN", "GLOBAL", "HUNTER", "JUNGLE", "KERNEL", "LEGION", "MUTANT", "OUTPUT",
    "PLASMA", "RADIUM", "SECTOR", "SHADOW", "SIGNAL", "SYSTEM", "TARGET", "TUNNEL", "UNLOCK", "VECTOR", "WANDER", "BATTLE", "COPPER", "DANGER", "ESCAPE",
    "FOSSIL", "GRAVEL", "HAZARD", "MEMORY", "MODULE", "PATROL", "REMOTE", "SUMMIT", "STATIC", "SURVEY", "TERROR", "WIRING",
];
const WORDS7: &[&str] = &[
    "ARSENAL", "BATTERY", "CAPSULE", "COMMAND", "CONSOLE", "CONTROL", "DEFENSE", "FACTORY", "FREEDOM", "GENERAL", "MACHINE", "MILITIA",
    "MISSION", "MONITOR", "NETWORK", "OUTPOST", "PROGRAM", "PROTECT", "QUARTER", "REACTOR", "RECEIVE", "RESERVE", "ROUTINE", "SCANNER", "SHELTER",
    "SOLDIER", "STATION", "SURFACE", "SURVIVE", "TRAFFIC", "UNKNOWN", "VILLAGE", "WARFARE", "WARNING", "WEATHER", "ZEALOUS", "PATIENT",
];

#[derive(PartialEq, Eq)]
enum State {
    Playing,
    Granted,
    Locked,
}

struct Word {
    start: usize,
    text: String,
    removed: bool,
}

struct Game {
    rng: Rng,
    grid: Vec<u8>,
    words: Vec<Word>,
    answer: usize,
    used_brackets: Vec<usize>,
    attempts: u32,
    log: Vec<String>,
    /// Cursor: column 0..2*COLS across both blocks, row 0..ROWS.
    x: usize,
    y: usize,
    state: State,
    base: u16,
}

fn likeness(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).filter(|(x, y)| x == y).count()
}

fn closing(c: u8) -> Option<u8> {
    match c {
        b'(' => Some(b')'),
        b'[' => Some(b']'),
        b'{' => Some(b'}'),
        b'<' => Some(b'>'),
        _ => None,
    }
}

/// What the cursor is on.
enum Pick {
    Word(usize),
    Brackets(usize, usize),
    Char(usize),
}

impl Game {
    fn new(mut rng: Rng) -> Game {
        let list = match rng.below(3) {
            0 => WORDS5,
            1 => WORDS6,
            _ => WORDS7,
        };
        let len = list[0].len();
        let mut pool: Vec<&str> = list.iter().copied().filter(|w| w.len() == len).collect();
        rng.shuffle(&mut pool);
        // The answer plus words that share letters with it, so the
        // likeness numbers actually help.
        let answer_text = pool[0];
        let mut chosen: Vec<&str> = Vec::from([answer_text]);
        let mut rest: Vec<(usize, &str)> = pool[1..].iter().map(|&w| (likeness(w, answer_text) * 4 + rng.below(4), w)).collect();
        rest.sort_by_key(|&(k, _)| core::cmp::Reverse(k));
        chosen.extend(rest.iter().take(WORDS - 1).map(|&(_, w)| w));
        rng.shuffle(&mut chosen);
        let mut grid: Vec<u8> = (0..SIZE).map(|_| JUNK[rng.below(JUNK.len())]).collect();
        // One word per slot, somewhere inside it.
        let slot = SIZE / chosen.len();
        let mut words = Vec::new();
        for (i, w) in chosen.iter().enumerate() {
            let start = i * slot + rng.below(slot - len);
            grid[start..start + len].copy_from_slice(w.as_bytes());
            words.push(Word { start, text: String::from(*w), removed: false });
        }
        let answer = words.iter().position(|w| w.text == answer_text).unwrap_or(0);
        let base = 0xF000 + (rng.below(0x0E00) as u16 & !0xF);
        Game {
            rng,
            grid,
            words,
            answer,
            used_brackets: Vec::new(),
            attempts: ATTEMPTS,
            log: Vec::new(),
            x: 0,
            y: 0,
            state: State::Playing,
            base,
        }
    }

    fn index(&self) -> usize {
        let (block, col) = if self.x < COLS { (0, self.x) } else { (1, self.x - COLS) };
        block * BLOCK + self.y * COLS + col
    }

    fn pick(&self) -> Pick {
        let i = self.index();
        if let Some(w) = self.words.iter().position(|w| !w.removed && i >= w.start && i < w.start + w.text.len()) {
            return Pick::Word(w);
        }
        if let Some(close) = closing(self.grid[i]) {
            if !self.used_brackets.contains(&i) {
                // The pair must close on the same line with no letters between.
                let line_end = (i / COLS + 1) * COLS;
                for j in i + 1..line_end {
                    let c = self.grid[j];
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                    if c == close {
                        return Pick::Brackets(i, j);
                    }
                }
            }
        }
        Pick::Char(i)
    }

    fn selection(&self) -> (usize, usize) {
        match self.pick() {
            Pick::Word(w) => (self.words[w].start, self.words[w].start + self.words[w].text.len()),
            Pick::Brackets(a, b) => (a, b + 1),
            Pick::Char(i) => (i, i + 1),
        }
    }

    fn selected_text(&self) -> String {
        let (a, b) = self.selection();
        self.grid[a..b].iter().map(|&c| c as char).collect()
    }

    fn say(&mut self, s: &str) {
        self.log.push(format!(">{}", s));
    }

    fn enter(&mut self) {
        if self.state != State::Playing {
            return;
        }
        let text = self.selected_text();
        match self.pick() {
            Pick::Word(w) => {
                self.say(&text);
                if w == self.answer {
                    self.say("Exact match!");
                    self.say("Please wait while system");
                    self.say("is accessed.");
                    self.state = State::Granted;
                    return;
                }
                let n = likeness(&self.words[w].text, &self.words[self.answer].text);
                self.say("Entry denied.");
                self.say(&format!("Likeness={}", n));
                self.attempts -= 1;
                if self.attempts == 0 {
                    self.say("Lockout in progress.");
                    self.state = State::Locked;
                }
            }
            Pick::Brackets(a, _) => {
                self.say(&text);
                self.used_brackets.push(a);
                let duds: Vec<usize> = (0..self.words.len()).filter(|&w| w != self.answer && !self.words[w].removed).collect();
                if !duds.is_empty() && (self.attempts == ATTEMPTS || self.rng.below(4) != 0) {
                    let w = duds[self.rng.below(duds.len())];
                    self.words[w].removed = true;
                    let (s, l) = (self.words[w].start, self.words[w].text.len());
                    self.grid[s..s + l].fill(b'.');
                    self.say("Dud removed.");
                } else {
                    self.attempts = ATTEMPTS;
                    self.say("Tries reset.");
                }
            }
            Pick::Char(_) => {
                self.say(&text);
                self.say("Error");
            }
        }
    }

    fn draw(&self, s: &mut Screen) {
        s.clear();
        let hot = Style::fg(term::GREEN | term::BRIGHT).bold();
        let normal = Style::NORMAL;
        let dim = Style::fg(term::BLACK | term::BRIGHT);
        s.text(0, 1, "HULDRA INDUSTRIES (TM) SECURE TERMINAL PROTOCOL", hot);
        match self.state {
            State::Playing if self.attempts == 1 => {
                s.text(1, 1, "!!! WARNING: LOCKOUT IMMINENT !!!", Style::fg(term::RED | term::BRIGHT).bold());
            }
            State::Playing => {
                s.text(1, 1, "ENTER PASSWORD NOW", normal);
            }
            State::Granted => {
                s.text(1, 1, "ACCESS GRANTED.  n - new terminal, q - log off", Style::fg(term::YELLOW | term::BRIGHT).bold());
            }
            State::Locked => {
                s.text(1, 1, "TERMINAL LOCKED.  PLEASE CONTACT AN ADMINISTRATOR", Style::fg(term::RED | term::BRIGHT).bold());
                s.text(2, 1, "n - try another terminal, q - log off", normal);
            }
        }
        if self.state != State::Locked {
            let col = s.text(3, 1, &format!("{} ATTEMPT(S) LEFT:", self.attempts), normal);
            for k in 0..self.attempts as usize {
                s.put(3, col + 1 + 2 * k, '█', Style::fg(term::GREEN | term::BRIGHT));
            }
        }
        let top = 5;
        let (sa, sb) = self.selection();
        for block in 0..2 {
            let left = 1 + block * 20;
            for row in 0..ROWS {
                let addr = self.base as usize + (block * BLOCK + row * COLS);
                s.text(top + row, left, &format!("0x{:04X}", addr & 0xFFFF), dim);
                for col in 0..COLS {
                    let i = block * BLOCK + row * COLS + col;
                    let ch = self.grid[i] as char;
                    let style = if self.state == State::Playing && i >= sa && i < sb {
                        Style::REVERSE
                    } else if ch.is_ascii_alphabetic() {
                        Style::fg(term::GREEN | term::BRIGHT)
                    } else {
                        normal
                    };
                    s.put(top + row, left + 7 + col, ch, style);
                }
            }
        }
        // The log grows up from the input line, like the real thing.
        let log_col = 42;
        let shown = ROWS - 1;
        let start = self.log.len().saturating_sub(shown);
        for (k, line) in self.log[start..].iter().enumerate() {
            let row = top + shown - (self.log.len() - start) + k;
            s.text(row, log_col, line, normal);
        }
        let input_row = top + ROWS - 1;
        let end = if self.state == State::Playing { s.text(input_row, log_col, &format!(">{}", self.selected_text()), hot) } else { s.text(input_row, log_col, ">", hot) };
        s.text(top + ROWS + 1, 1, "arrows move  Enter select  n new  q quit", dim);
        s.set_cursor(input_row, end);
    }

    fn key(&mut self, k: Key) {
        match k {
            Key::Left | Key::Char('h') => self.x = (self.x + 2 * COLS - 1) % (2 * COLS),
            Key::Right | Key::Char('l') => self.x = (self.x + 1) % (2 * COLS),
            Key::Up | Key::Char('k') => self.y = (self.y + ROWS - 1) % ROWS,
            Key::Down | Key::Char('j') => self.y = (self.y + 1) % ROWS,
            Key::Enter | Key::Char(' ') => self.enter(),
            _ => {}
        }
    }
}

fn main() -> i32 {
    if !term::is_tty(huldra_user::io::STDIN) {
        eprintln!("hack: needs a terminal");
        return 1;
    }
    let Ok(_raw) = RawMode::enable() else {
        eprintln!("hack: cannot use the terminal");
        return 1;
    };
    let mut screen = Screen::new();
    if screen.rows < 24 || screen.cols < 80 {
        drop(_raw);
        eprintln!("hack: the terminal must be at least 80x24");
        return 1;
    }
    let mut keys = Keys::new();
    let mut seed = Rng::from_time();
    let mut game = Game::new(Rng::new(seed.next()));
    loop {
        game.draw(&mut screen);
        screen.present();
        let Ok(k) = keys.read() else { break };
        match k {
            Key::Char('q') | Key::Escape | Key::Ctrl('c') => break,
            Key::Char('n') => game = Game::new(Rng::new(seed.next())),
            Key::Ctrl('l') => screen.invalidate(),
            k => game.key(k),
        }
    }
    term::reset_screen();
    0
}
