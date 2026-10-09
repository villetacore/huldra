//! less: a pager. `less [-N] [file...]`, or reads standard input.
//!
//! Keys: q quit; j/k/arrows line; Space/b/PgDn/PgUp page; d/u half page;
//! g/G top/bottom; Left/Right scroll; / and ? search; n/N next/previous
//! match; :n/:p next/previous file; -N or # toggles line numbers.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use huldra_user::abi::fs::O_RDWR;
use huldra_user::io::{write_all, Reader, STDIN, STDOUT};
use huldra_user::term::{self, Key, Keys, RawMode, Screen, Style};
use huldra_user::{env, eprintln, format, fs, sys};

huldra_user::main!(main);

struct Doc {
    name: String,
    lines: Vec<String>,
}

fn expand_tabs(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c == '\t' {
            let n = 8 - out.chars().count() % 8;
            out.extend(core::iter::repeat_n(' ', n));
        } else if (c as u32) < 0x20 && c != '\x1b' {
            out.push('^');
            out.push((c as u8 + 0x40) as char);
        } else {
            out.push(c);
        }
    }
    out
}

fn load(name: &str, fd: i32) -> Doc {
    let data = Reader::new(fd).read_to_end().unwrap_or_default();
    let text = String::from_utf8_lossy(&data);
    let mut lines: Vec<String> = text.split('\n').map(|l| expand_tabs(l.trim_end_matches('\r'))).collect();
    if text.ends_with('\n') {
        lines.pop();
    }
    Doc { name: String::from(name), lines }
}

/// Splits a line into characters with their style, interpreting SGR
/// escape sequences (colors, bold, reverse) as `help` and `ls` print them.
fn styled(line: &str) -> Vec<(char, Style)> {
    let mut out = Vec::new();
    let mut st = Style::NORMAL;
    let mut it = line.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\x1b' {
            out.push((c, st));
            continue;
        }
        if it.peek() != Some(&'[') {
            continue;
        }
        it.next();
        let mut params = String::new();
        let mut fin = ' ';
        for d in it.by_ref() {
            if d.is_ascii_digit() || d == ';' {
                params.push(d);
            } else {
                fin = d;
                break;
            }
        }
        if fin != 'm' {
            continue;
        }
        for p in params.split(';') {
            match p.parse::<u8>().unwrap_or(0) {
                0 => st = Style::NORMAL,
                1 => st.bold = true,
                22 => st.bold = false,
                7 => st.reverse = true,
                27 => st.reverse = false,
                n @ 30..=37 => st.fg = Some(n - 30),
                39 => st.fg = None,
                n @ 40..=47 => st.bg = Some(n - 40),
                49 => st.bg = None,
                n @ 90..=97 => st.fg = Some(n - 90 + term::BRIGHT),
                n @ 100..=107 => st.bg = Some(n - 100 + term::BRIGHT),
                _ => {}
            }
        }
    }
    out
}

struct Pager {
    docs: Vec<Doc>,
    current: usize,
    top: usize,
    left: usize,
    numbers: bool,
    search: String,
    forward: bool,
    message: String,
    screen: Screen,
    keys: Keys,
}

impl Pager {
    fn doc(&self) -> &Doc {
        &self.docs[self.current]
    }

    fn page(&self) -> usize {
        self.screen.rows.saturating_sub(1).max(1)
    }

    fn max_top(&self) -> usize {
        self.doc().lines.len().saturating_sub(self.page())
    }

    fn scroll(&mut self, delta: isize) {
        self.top = (self.top as isize + delta).clamp(0, self.max_top() as isize) as usize;
    }

    fn draw(&mut self) {
        let rows = self.page();
        let cols = self.screen.cols;
        let gutter = if self.numbers { 8 } else { 0 };
        self.screen.clear();
        let lines: Vec<String> = self.doc().lines[self.top..(self.top + rows).min(self.doc().lines.len())].to_vec();
        for (r, line) in lines.iter().enumerate() {
            if self.numbers {
                self.screen.text(r, 0, &format!("{:>6}  ", self.top + r + 1), Style::fg(term::YELLOW));
            }
            let cells = styled(line);
            for (i, &(c, st)) in cells.iter().skip(self.left).take(cols - gutter).enumerate() {
                self.screen.put(r, gutter + i, c, st);
            }
            if !self.search.is_empty() {
                let chars: Vec<char> = cells.iter().map(|&(c, _)| c).collect();
                let needle: Vec<char> = self.search.chars().collect();
                let mut i = 0;
                while i + needle.len() <= chars.len() {
                    if chars[i..i + needle.len()] == needle[..] {
                        for j in i..i + needle.len() {
                            if j >= self.left && j - self.left + gutter < cols {
                                self.screen.put(r, gutter + j - self.left, chars[j], Style::REVERSE);
                            }
                        }
                        i += needle.len();
                    } else {
                        i += 1;
                    }
                }
            }
        }
        for r in lines.len()..rows {
            self.screen.text(r, 0, "~", Style::fg(term::BLUE | term::BRIGHT));
        }
        let total = self.doc().lines.len();
        let last = (self.top + rows).min(total);
        let pct = if total == 0 { 100 } else { last * 100 / total };
        let status = if self.message.is_empty() {
            let files = if self.docs.len() > 1 { format!(" (file {} of {})", self.current + 1, self.docs.len()) } else { String::new() };
            format!(" {}{}  lines {}-{}/{}  {}%{}  (q quit, h help)", self.doc().name, files, self.top + 1, last, total, pct, if last == total { " (END)" } else { "" })
        } else {
            self.message.clone()
        };
        self.screen.fill_row(self.screen.rows - 1, 0, Style::REVERSE);
        self.screen.text(self.screen.rows - 1, 0, &status, Style::REVERSE);
        self.screen.set_cursor(self.screen.rows - 1, status.chars().count().min(cols - 1));
    }

    fn prompt(&mut self, label: &str) -> Option<String> {
        let mut input = String::new();
        loop {
            self.message = format!("{}{}", label, input);
            self.draw();
            self.screen.present();
            match self.keys.read().ok()? {
                Key::Enter => {
                    self.message.clear();
                    return Some(input);
                }
                Key::Escape | Key::Ctrl('c') => {
                    self.message.clear();
                    return None;
                }
                Key::Backspace => {
                    if input.pop().is_none() {
                        self.message.clear();
                        return None;
                    }
                }
                Key::Char(c) => input.push(c),
                _ => {}
            }
        }
    }

    fn find(&mut self, forward: bool) {
        if self.search.is_empty() {
            return;
        }
        let n = self.doc().lines.len();
        let range: Vec<usize> = if forward { (self.top + 1..n).collect() } else { (0..self.top).rev().collect() };
        match range.into_iter().find(|&i| self.doc().lines[i].contains(self.search.as_str())) {
            Some(i) => self.top = i.min(self.max_top().max(i)),
            None => self.message = format!(" Pattern not found: {}", self.search),
        }
    }

    fn help(&mut self) {
        const HELP: &[&str] = &[
            "less - keys",
            "",
            "  q, Q, Esc          quit",
            "  j, Down, Enter     forward one line      k, Up      back one line",
            "  Space, f, PgDn     forward one page      b, PgUp    back one page",
            "  d / u              half page down / up",
            "  g, Home / G, End   first / last line",
            "  Left / Right       scroll horizontally",
            "  /pattern  ?pattern search forward / backward;  n / N  repeat",
            "  :n  :p             next / previous file",
            "  #                  toggle line numbers",
            "",
            "Press any key to return.",
        ];
        self.screen.clear();
        for (i, l) in HELP.iter().enumerate() {
            self.screen.text(i + 1, 2, l, if i == 0 { Style::fg(term::CYAN | term::BRIGHT).bold() } else { Style::NORMAL });
        }
        self.screen.present();
        let _ = self.keys.read();
    }

    fn run(&mut self) {
        loop {
            self.draw();
            self.screen.present();
            let Ok(key) = self.keys.read() else { return };
            self.message.clear();
            let page = self.page() as isize;
            match key {
                Key::Char('q') | Key::Char('Q') | Key::Escape | Key::Ctrl('c') => return,
                Key::Char('j') | Key::Down | Key::Enter | Key::Ctrl('n') | Key::Ctrl('e') => self.scroll(1),
                Key::Char('k') | Key::Up | Key::Ctrl('p') | Key::Ctrl('y') => self.scroll(-1),
                Key::Char(' ') | Key::Char('f') | Key::PageDown | Key::Ctrl('f') => self.scroll(page),
                Key::Char('b') | Key::PageUp | Key::Ctrl('b') => self.scroll(-page),
                Key::Char('d') | Key::Ctrl('d') => self.scroll(page / 2),
                Key::Char('u') | Key::Ctrl('u') => self.scroll(-page / 2),
                Key::Char('g') | Key::Char('<') | Key::Home => self.top = 0,
                Key::Char('G') | Key::Char('>') | Key::End => self.top = self.max_top(),
                Key::Left => self.left = self.left.saturating_sub(8),
                Key::Right => self.left += 8,
                Key::Char('#') => self.numbers = !self.numbers,
                Key::Char('h') | Key::F(1) => {
                    self.help();
                    self.screen.invalidate();
                }
                Key::Char('/') | Key::Char('?') => {
                    let forward = key == Key::Char('/');
                    if let Some(p) = self.prompt(if forward { "/" } else { "?" }) {
                        if !p.is_empty() {
                            self.search = p;
                        }
                        self.forward = forward;
                        self.find(forward);
                    }
                }
                Key::Char('n') => self.find(self.forward),
                Key::Char('N') => self.find(!self.forward),
                Key::Char(':') => match self.prompt(":").as_deref() {
                    Some("n") if self.current + 1 < self.docs.len() => {
                        self.current += 1;
                        self.top = 0;
                    }
                    Some("p") if self.current > 0 => {
                        self.current -= 1;
                        self.top = 0;
                    }
                    Some("q") => return,
                    _ => {}
                },
                _ => {}
            }
        }
    }
}

fn main() -> i32 {
    let mut numbers = false;
    let mut files: Vec<String> = Vec::new();
    for a in &env::args()[1..] {
        match a.as_str() {
            "-N" => numbers = true,
            _ => files.push(a.clone()),
        }
    }
    let mut docs = Vec::new();
    if files.is_empty() {
        docs.push(load("(stdin)", STDIN));
    }
    for f in &files {
        match sys::open(f, 0, 0) {
            Ok(fd) => {
                if fs::metadata(f).map(|s| fs::is_dir(&s)).unwrap_or(false) {
                    eprintln!("less: {}: is a directory", f);
                    let _ = sys::close(fd);
                    continue;
                }
                docs.push(load(f, fd));
                let _ = sys::close(fd);
            }
            Err(e) => eprintln!("less: {}: {}", f, e),
        }
    }
    if docs.is_empty() {
        return 1;
    }
    // Not a terminal: behave like cat.
    if !term::is_tty(STDOUT) {
        for d in &docs {
            for l in &d.lines {
                let _ = write_all(STDOUT, l.as_bytes());
                let _ = write_all(STDOUT, b"\n");
            }
        }
        return 0;
    }
    // Keys come from the terminal even when the text came from a pipe.
    let key_fd = if term::is_tty(STDIN) { STDIN } else { sys::open("/dev/tty", O_RDWR, 0).unwrap_or(STDIN) };
    let Ok(_raw) = RawMode::enable_fd(key_fd) else {
        eprintln!("less: cannot use the terminal");
        return 1;
    };
    let mut p = Pager {
        docs,
        current: 0,
        top: 0,
        left: 0,
        numbers,
        search: String::new(),
        forward: true,
        message: String::new(),
        screen: Screen::new(),
        keys: Keys::from_fd(key_fd),
    };
    p.run();
    term::reset_screen();
    0
}
