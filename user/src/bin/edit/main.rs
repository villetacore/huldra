//! edit: a full-screen text editor in the spirit of nano.
//!
//! Keys: arrows, Home/End, PgUp/PgDn — move;  ^S save  ^Q quit  ^F find
//! F3/^N find next  ^R replace  ^G go to line  ^K cut line  ^C copy line
//! ^U paste  ^D duplicate line  ^Z undo  ^Y redo  ^L line numbers  F1 help.

#![no_std]
#![no_main]

extern crate alloc;

mod syntax;

use alloc::string::String;
use alloc::vec::Vec;
use huldra_user::term::{self, Key, Keys, RawMode, Screen, Style};
use huldra_user::{env, eprintln, format, fs};
use syntax::Lang;

huldra_user::main!(main);

const TAB: usize = 4;
const UNDO_LIMIT: usize = 200;

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Insert,
    Delete,
    Other,
}

struct Snapshot {
    lines: Vec<Vec<char>>,
    cy: usize,
    cx: usize,
}

struct Editor {
    lines: Vec<Vec<char>>,
    cy: usize,
    cx: usize,
    /// Preferred render column for vertical movement.
    want_col: usize,
    top: usize,
    left: usize,
    path: Option<String>,
    dirty: bool,
    message: String,
    clipboard: Vec<Vec<char>>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_edit: EditKind,
    search: String,
    quit_armed: bool,
    /// Consecutive ^K presses collect lines into one clipboard.
    cut_streak: bool,
    numbers: bool,
    lang: Lang,
    screen: Screen,
    keys: Keys,
}

fn render_col(line: &[char], cx: usize) -> usize {
    let mut col = 0;
    for &c in &line[..cx.min(line.len())] {
        col = if c == '\t' { (col / TAB + 1) * TAB } else { col + 1 };
    }
    col
}

fn index_for_col(line: &[char], target: usize) -> usize {
    let mut col = 0;
    for (i, &c) in line.iter().enumerate() {
        let next = if c == '\t' { (col / TAB + 1) * TAB } else { col + 1 };
        if next > target {
            return i;
        }
        col = next;
    }
    line.len()
}

impl Editor {
    fn new(path: Option<String>) -> Editor {
        let mut lines: Vec<Vec<char>> = Vec::new();
        let mut message = String::from("F1 help | ^S save | ^Q quit | ^F find");
        if let Some(p) = &path {
            match fs::read(p) {
                Ok(data) => {
                    let text = String::from_utf8_lossy(&data);
                    lines = text.split('\n').map(|l| l.trim_end_matches('\r').chars().collect()).collect();
                    if text.ends_with('\n') {
                        lines.pop();
                    }
                }
                Err(huldra_user::Errno::ENOENT) => message = format!("New file: {}", p),
                Err(e) => message = format!("Cannot read {}: {}", p, e),
            }
        }
        if lines.is_empty() {
            lines.push(Vec::new());
        }
        let lang = Lang::detect(path.as_deref().unwrap_or(""), lines.first().map(|l| l.iter().collect::<String>()).as_deref());
        Editor {
            lines,
            cy: 0,
            cx: 0,
            want_col: 0,
            top: 0,
            left: 0,
            path,
            dirty: false,
            message,
            clipboard: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: EditKind::Other,
            search: String::new(),
            quit_armed: false,
            cut_streak: false,
            numbers: true,
            lang,
            screen: Screen::new(),
            keys: Keys::new(),
        }
    }

    fn text_rows(&self) -> usize {
        self.screen.rows.saturating_sub(3).max(1)
    }

    fn gutter(&self) -> usize {
        if self.numbers {
            format!("{}", self.lines.len()).len().max(3) + 1
        } else {
            0
        }
    }

    // ------------------------------------------------------------ undo

    fn checkpoint(&mut self, kind: EditKind) {
        if kind == EditKind::Other || kind != self.last_edit {
            self.undo.push(Snapshot { lines: self.lines.clone(), cy: self.cy, cx: self.cx });
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        self.last_edit = kind;
        self.dirty = true;
    }

    fn undo(&mut self) {
        match self.undo.pop() {
            Some(s) => {
                self.redo.push(Snapshot { lines: core::mem::replace(&mut self.lines, s.lines), cy: self.cy, cx: self.cx });
                self.cy = s.cy;
                self.cx = s.cx;
                self.dirty = true;
                self.message = String::from("Undone");
            }
            None => self.message = String::from("Nothing to undo"),
        }
        self.last_edit = EditKind::Other;
    }

    fn redo(&mut self) {
        match self.redo.pop() {
            Some(s) => {
                self.undo.push(Snapshot { lines: core::mem::replace(&mut self.lines, s.lines), cy: self.cy, cx: self.cx });
                self.cy = s.cy;
                self.cx = s.cx;
                self.dirty = true;
                self.message = String::from("Redone");
            }
            None => self.message = String::from("Nothing to redo"),
        }
        self.last_edit = EditKind::Other;
    }

    // ------------------------------------------------------------ editing

    fn insert_char(&mut self, c: char) {
        self.checkpoint(if c == ' ' { EditKind::Other } else { EditKind::Insert });
        self.lines[self.cy].insert(self.cx, c);
        self.cx += 1;
    }

    fn newline(&mut self) {
        self.checkpoint(EditKind::Other);
        let rest = self.lines[self.cy].split_off(self.cx);
        let indent: Vec<char> = self.lines[self.cy].iter().take_while(|c| **c == ' ' || **c == '\t').copied().collect();
        let mut new_line = indent.clone();
        new_line.extend(rest);
        self.cy += 1;
        self.lines.insert(self.cy, new_line);
        self.cx = indent.len();
    }

    fn backspace(&mut self) {
        if self.cx > 0 {
            self.checkpoint(EditKind::Delete);
            // Remove a whole indent level when only spaces precede the cursor.
            let line = &self.lines[self.cy];
            let only_spaces = line[..self.cx].iter().all(|&c| c == ' ');
            let n = if only_spaces && self.cx >= TAB { (self.cx - 1) % TAB + 1 } else { 1 };
            self.lines[self.cy].drain(self.cx - n..self.cx);
            self.cx -= n;
        } else if self.cy > 0 {
            self.checkpoint(EditKind::Other);
            let line = self.lines.remove(self.cy);
            self.cy -= 1;
            self.cx = self.lines[self.cy].len();
            self.lines[self.cy].extend(line);
        }
    }

    fn delete(&mut self) {
        if self.cx < self.lines[self.cy].len() {
            self.checkpoint(EditKind::Delete);
            self.lines[self.cy].remove(self.cx);
        } else if self.cy + 1 < self.lines.len() {
            self.checkpoint(EditKind::Other);
            let next = self.lines.remove(self.cy + 1);
            self.lines[self.cy].extend(next);
        }
    }

    fn indent(&mut self) {
        self.checkpoint(EditKind::Other);
        let n = TAB - render_col(&self.lines[self.cy], self.cx) % TAB;
        for _ in 0..n {
            self.lines[self.cy].insert(self.cx, ' ');
        }
        self.cx += n;
    }

    fn unindent(&mut self) {
        let n = self.lines[self.cy].iter().take(TAB).take_while(|&&c| c == ' ').count();
        if n > 0 {
            self.checkpoint(EditKind::Other);
            self.lines[self.cy].drain(..n);
            self.cx = self.cx.saturating_sub(n);
        }
    }

    fn cut_line(&mut self) {
        self.checkpoint(EditKind::Other);
        if self.last_cut_continues() {
            self.clipboard.push(self.lines[self.cy].clone());
        } else {
            self.clipboard = alloc::vec![self.lines[self.cy].clone()];
        }
        if self.lines.len() > 1 {
            self.lines.remove(self.cy);
            if self.cy >= self.lines.len() {
                self.cy = self.lines.len() - 1;
            }
        } else {
            self.lines[0].clear();
        }
        self.cx = 0;
        self.message = format!("Cut {} line(s)", self.clipboard.len());
        self.cut_streak = true;
    }

    fn last_cut_continues(&self) -> bool {
        self.cut_streak
    }

    fn copy_line(&mut self) {
        self.clipboard = alloc::vec![self.lines[self.cy].clone()];
        self.message = String::from("Copied 1 line");
    }

    fn paste(&mut self) {
        if self.clipboard.is_empty() {
            self.message = String::from("Clipboard is empty");
            return;
        }
        self.checkpoint(EditKind::Other);
        for (i, l) in self.clipboard.clone().into_iter().enumerate() {
            self.lines.insert(self.cy + i, l);
        }
        self.cy += self.clipboard.len();
        self.cx = 0;
    }

    fn duplicate(&mut self) {
        self.checkpoint(EditKind::Other);
        let l = self.lines[self.cy].clone();
        self.lines.insert(self.cy + 1, l);
        self.cy += 1;
    }

    // ------------------------------------------------------------ movement

    fn move_vertical(&mut self, delta: isize) {
        let target = (self.cy as isize + delta).clamp(0, self.lines.len() as isize - 1) as usize;
        self.cy = target;
        self.cx = index_for_col(&self.lines[self.cy], self.want_col);
    }

    fn remember_col(&mut self) {
        self.want_col = render_col(&self.lines[self.cy], self.cx);
    }

    fn scroll_into_view(&mut self) {
        let rows = self.text_rows();
        if self.cy < self.top {
            self.top = self.cy;
        }
        if self.cy >= self.top + rows {
            self.top = self.cy + 1 - rows;
        }
        let width = self.screen.cols.saturating_sub(self.gutter()).max(1);
        let col = render_col(&self.lines[self.cy], self.cx);
        if col < self.left {
            self.left = col;
        }
        if col >= self.left + width {
            self.left = col + 1 - width;
        }
    }

    // ------------------------------------------------------------ prompts

    /// Reads a line on the message row; None if cancelled with Esc/^C.
    fn prompt(&mut self, label: &str, initial: &str) -> Option<String> {
        let mut input: Vec<char> = initial.chars().collect();
        loop {
            let text: String = input.iter().collect();
            self.message = format!("{}{}", label, text);
            self.draw();
            let row = self.screen.rows - 2;
            let col = (label.chars().count() + input.len()).min(self.screen.cols - 1);
            self.screen.set_cursor(row, col);
            self.screen.present();
            match self.keys.read().ok()? {
                Key::Enter => {
                    self.message.clear();
                    return Some(input.into_iter().collect());
                }
                Key::Escape | Key::Ctrl('c') | Key::Ctrl('q') => {
                    self.message = String::from("Cancelled");
                    return None;
                }
                Key::Backspace => {
                    input.pop();
                }
                Key::Char(c) => input.push(c),
                _ => {}
            }
        }
    }

    fn save(&mut self, ask_name: bool) {
        if ask_name || self.path.is_none() {
            let current = self.path.clone().unwrap_or_default();
            match self.prompt("Save as: ", &current) {
                Some(p) if !p.is_empty() => self.path = Some(p),
                _ => return,
            }
        }
        let path = self.path.clone().unwrap();
        let mut text = String::new();
        for l in &self.lines {
            text.extend(l.iter());
            text.push('\n');
        }
        match fs::write(&path, text.as_bytes()) {
            Ok(()) => {
                self.dirty = false;
                self.message = format!("Wrote {} lines to {}", self.lines.len(), path);
                self.lang = Lang::detect(&path, None);
            }
            Err(e) => self.message = format!("Cannot save {}: {}", path, e),
        }
    }

    fn find_next(&mut self, from_next: bool) {
        if self.search.is_empty() {
            return;
        }
        let needle: Vec<char> = self.search.chars().collect();
        let n = self.lines.len();
        let start_line = self.cy;
        for step in 0..=n {
            let y = (self.cy + step) % n;
            let start = if step == 0 { self.cx + from_next as usize } else { 0 };
            let line = &self.lines[y];
            if start > line.len() {
                continue;
            }
            if let Some(pos) = (start..=line.len().saturating_sub(needle.len()))
                .find(|&i| i + needle.len() <= line.len() && line[i..i + needle.len()] == needle[..])
            {
                self.cy = y;
                self.cx = pos;
                self.remember_col();
                self.message = if step > 0 && y <= start_line { String::from("Search wrapped") } else { String::new() };
                return;
            }
        }
        self.message = format!("\"{}\" not found", self.search);
    }

    fn find(&mut self) {
        let initial = self.search.clone();
        if let Some(s) = self.prompt("Find: ", &initial) {
            self.search = s;
            self.find_next(false);
        }
    }

    fn replace(&mut self) {
        let Some(from) = self.prompt("Replace: ", &self.search.clone()) else { return };
        if from.is_empty() {
            return;
        }
        let Some(to) = self.prompt(&format!("Replace '{}' with: ", from), "") else { return };
        self.checkpoint(EditKind::Other);
        let mut count = 0;
        for line in self.lines.iter_mut() {
            let text: String = line.iter().collect();
            if text.contains(&from) {
                count += text.matches(&from).count();
                *line = text.replace(&from, &to).chars().collect();
            }
        }
        self.cx = self.cx.min(self.lines[self.cy].len());
        self.search = from;
        self.message = format!("Replaced {} occurrence(s)", count);
    }

    fn goto(&mut self) {
        if let Some(s) = self.prompt("Go to line: ", "") {
            match s.trim().parse::<usize>() {
                Ok(n) if n >= 1 => {
                    self.cy = (n - 1).min(self.lines.len() - 1);
                    self.cx = 0;
                    self.remember_col();
                }
                _ => self.message = String::from("Not a line number"),
            }
        }
    }

    fn help(&mut self) {
        const HELP: &[&str] = &[
            "edit - keys",
            "",
            "  Arrows Home End PgUp PgDn   move the cursor",
            "  Ctrl+S          save              Ctrl+Q    quit",
            "  Ctrl+F          find              F3/Ctrl+N find next",
            "  Ctrl+R          replace all       Ctrl+G    go to line",
            "  Ctrl+K          cut line          Ctrl+C    copy line",
            "  Ctrl+U          paste             Ctrl+D    duplicate line",
            "  Ctrl+Z          undo              Ctrl+Y    redo",
            "  Tab / Shift+Tab indent/unindent   Ctrl+L    line numbers",
            "  F2              save as",
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

    // ------------------------------------------------------------ drawing

    fn draw(&mut self) {
        self.scroll_into_view();
        let (rows, cols) = (self.screen.rows, self.screen.cols);
        let gutter = self.gutter();
        self.screen.clear();

        // Title bar.
        let bar = Style::colors(term::BLACK, term::CYAN);
        let name = self.path.as_deref().unwrap_or("[new file]");
        let title = format!(" edit  {}{}", name, if self.dirty { "  [modified]" } else { "" });
        self.screen.fill_row(0, 0, bar);
        self.screen.text(0, 0, &title, bar);
        let pos = format!("{}:{}  {} ", self.cy + 1, self.cx + 1, self.lang.name());
        self.screen.text(0, cols.saturating_sub(pos.len()), &pos, bar);

        // Text with syntax highlighting.
        let text_rows = self.text_rows();
        let mut state = syntax::State::default();
        for y in 0..self.top.min(self.lines.len()) {
            state = self.lang.scan_state(&self.lines[y], state);
        }
        for r in 0..text_rows {
            let y = self.top + r;
            let row = r + 1;
            if y >= self.lines.len() {
                self.screen.text(row, 0, "~", Style::fg(term::BLUE | term::BRIGHT));
                continue;
            }
            if self.numbers {
                let num = format!("{:>w$} ", y + 1, w = gutter - 1);
                self.screen.text(row, 0, &num, Style::fg(if y == self.cy { term::YELLOW } else { term::BLACK | term::BRIGHT }));
            }
            let line = &self.lines[y];
            let (styles, next) = self.lang.highlight(line, state);
            state = next;
            let mut col = 0;
            for (i, &c) in line.iter().enumerate() {
                let width = if c == '\t' { TAB - col % TAB } else { 1 };
                for k in 0..width {
                    let vis = col + k;
                    if vis >= self.left && vis - self.left + gutter < cols {
                        let ch = if c == '\t' { ' ' } else { c };
                        self.screen.put(row, gutter + vis - self.left, ch, styles[i]);
                    }
                }
                col += width;
            }
            if self.search.len() > 1 {
                // Highlight search matches.
                let needle: Vec<char> = self.search.chars().collect();
                let mut i = 0;
                while i + needle.len() <= line.len() {
                    if line[i..i + needle.len()] == needle[..] {
                        for j in i..i + needle.len() {
                            let vis = render_col(line, j);
                            if vis >= self.left && vis - self.left + gutter < cols {
                                self.screen.put(row, gutter + vis - self.left, if line[j] == '\t' { ' ' } else { line[j] }, Style::colors(term::BLACK, term::YELLOW));
                            }
                        }
                        i += needle.len();
                    } else {
                        i += 1;
                    }
                }
            }
        }

        // Message and help rows.
        let msg = self.message.clone();
        self.screen.text(rows - 2, 0, &msg, Style::fg(term::YELLOW | term::BRIGHT));
        let help = [("^S", "Save"), ("^Q", "Quit"), ("^F", "Find"), ("^R", "Replace"), ("^G", "Goto"), ("^K", "Cut"), ("^U", "Paste"), ("^Z", "Undo"), ("F1", "Help")];
        let mut c = 0;
        for (k, label) in help {
            c = self.screen.text(rows - 1, c, k, Style::REVERSE);
            c = self.screen.text(rows - 1, c, &format!(" {} ", label), Style::NORMAL);
        }

        let col = render_col(&self.lines[self.cy], self.cx);
        self.screen.set_cursor(self.cy - self.top + 1, gutter + col.saturating_sub(self.left));
    }

    fn run(&mut self) {
        loop {
            self.draw();
            self.screen.present();
            let Ok(key) = self.keys.read() else { return };
            if !matches!(key, Key::Ctrl('k')) {
                self.cut_streak = false;
            }
            if key != Key::Ctrl('q') {
                self.quit_armed = false;
            }
            let msg_before = self.message.clone();
            match key {
                Key::Ctrl('q') => {
                    if self.dirty && !self.quit_armed {
                        self.message = String::from("Unsaved changes! Press ^Q again to quit without saving, ^S to save.");
                        self.quit_armed = true;
                        continue;
                    }
                    return;
                }
                Key::Ctrl('s') => self.save(false),
                Key::F(2) => self.save(true),
                Key::Ctrl('f') => self.find(),
                Key::F(3) | Key::Ctrl('n') => self.find_next(true),
                Key::Ctrl('r') => self.replace(),
                Key::Ctrl('g') => self.goto(),
                Key::Ctrl('k') => self.cut_line(),
                Key::Ctrl('c') => self.copy_line(),
                Key::Ctrl('u') => self.paste(),
                Key::Ctrl('d') => self.duplicate(),
                Key::Ctrl('z') => self.undo(),
                Key::Ctrl('y') => self.redo(),
                Key::Ctrl('l') => {
                    self.numbers = !self.numbers;
                    self.screen.invalidate();
                }
                Key::F(1) => {
                    self.help();
                    self.screen.invalidate();
                }
                Key::Enter => self.newline(),
                Key::Backspace => self.backspace(),
                Key::Delete => self.delete(),
                Key::Tab => self.indent(),
                Key::BackTab => self.unindent(),
                Key::Char(c) => self.insert_char(c),
                Key::Up => self.move_vertical(-1),
                Key::Down => self.move_vertical(1),
                Key::PageUp => self.move_vertical(-(self.text_rows() as isize - 1)),
                Key::PageDown => self.move_vertical(self.text_rows() as isize - 1),
                Key::Left => {
                    if self.cx > 0 {
                        self.cx -= 1;
                    } else if self.cy > 0 {
                        self.cy -= 1;
                        self.cx = self.lines[self.cy].len();
                    }
                }
                Key::Right => {
                    if self.cx < self.lines[self.cy].len() {
                        self.cx += 1;
                    } else if self.cy + 1 < self.lines.len() {
                        self.cy += 1;
                        self.cx = 0;
                    }
                }
                Key::Home => {
                    // Toggle between the first non-blank character and column 0.
                    let first = self.lines[self.cy].iter().take_while(|c| c.is_whitespace()).count();
                    self.cx = if self.cx == first { 0 } else { first };
                }
                Key::End => self.cx = self.lines[self.cy].len(),
                _ => {}
            }
            if !matches!(key, Key::Up | Key::Down | Key::PageUp | Key::PageDown) {
                self.remember_col();
            }
            if self.message == msg_before && matches!(key, Key::Char(_) | Key::Enter | Key::Backspace | Key::Up | Key::Down | Key::Left | Key::Right) {
                self.message.clear();
            }
        }
    }
}

fn main() -> i32 {
    let args = env::args();
    if !term::is_tty(0) || !term::is_tty(1) {
        eprintln!("edit: needs a terminal");
        return 1;
    }
    let path = args.get(1).cloned();
    let Ok(_raw) = RawMode::enable() else {
        eprintln!("edit: cannot switch the terminal to raw mode");
        return 1;
    };
    let mut ed = Editor::new(path);
    ed.run();
    term::reset_screen();
    0
}
