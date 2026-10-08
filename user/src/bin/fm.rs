//! fm: a two-panel file manager in the style of Midnight Commander.
//!
//! Tab switches panels; Enter opens; F3 view, F4 edit, F5 copy, F6 move,
//! F7 mkdir, F8 delete, F9 new file, F10 quit; h hidden files; ! command.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use huldra_user::abi::fs::*;
use huldra_user::fs::{self, mode_string, File};
use huldra_user::process::{self, ExitStatus};
use huldra_user::term::{self, Key, Keys, RawMode, Screen, Style};
use huldra_user::time::{DateTime, MONTHS};
use huldra_user::{env, eprintln, format, io, sys, Errno};

huldra_user::main!(main);

#[derive(Clone)]
struct Entry {
    name: String,
    is_dir: bool,
    size: i64,
    mode: u32,
    mtime: i64,
}

struct Panel {
    path: String,
    entries: Vec<Entry>,
    cursor: usize,
    top: usize,
    error: Option<String>,
}

impl Panel {
    fn new(path: &str) -> Panel {
        let mut p = Panel { path: String::from(path), entries: Vec::new(), cursor: 0, top: 0, error: None };
        p.load(false);
        p
    }

    fn load(&mut self, hidden: bool) {
        self.entries.clear();
        self.error = None;
        if self.path != "/" {
            self.entries.push(Entry { name: "..".into(), is_dir: true, size: 0, mode: S_IFDIR | 0o755, mtime: 0 });
        }
        match fs::read_dir(&self.path) {
            Ok(list) => {
                let mut dirs = Vec::new();
                let mut files = Vec::new();
                for e in list {
                    if !hidden && e.name.starts_with('.') {
                        continue;
                    }
                    let st = fs::metadata(&fs::join(&self.path, &e.name)).unwrap_or_default();
                    let entry = Entry { name: e.name, is_dir: fs::is_dir(&st), size: st.st_size, mode: st.st_mode, mtime: st.st_mtime };
                    if entry.is_dir {
                        dirs.push(entry);
                    } else {
                        files.push(entry);
                    }
                }
                self.entries.extend(dirs);
                self.entries.extend(files);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        self.cursor = self.cursor.min(self.entries.len().saturating_sub(1));
    }

    fn selected(&self) -> Option<&Entry> {
        self.entries.get(self.cursor)
    }

    fn selected_path(&self) -> Option<String> {
        self.selected().filter(|e| e.name != "..").map(|e| fs::join(&self.path, &e.name))
    }

    fn select_name(&mut self, name: &str) {
        if let Some(i) = self.entries.iter().position(|e| e.name == name) {
            self.cursor = i;
        }
    }
}

fn parent(path: &str) -> String {
    match path.trim_end_matches('/').rfind('/') {
        Some(0) | None => String::from("/"),
        Some(i) => String::from(&path[..i]),
    }
}

fn human(size: i64) -> String {
    const UNITS: [&str; 4] = ["", "K", "M", "G"];
    let mut v = size as f64;
    let mut u = 0;
    while v >= 1024.0 && u < 3 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{}", size)
    } else {
        format!("{:.1}{}", v, UNITS[u])
    }
}

fn copy_tree(src: &str, dst: &str) -> Result<(), Errno> {
    let st = fs::metadata(src)?;
    if fs::is_dir(&st) {
        match fs::create_dir(dst) {
            Ok(()) | Err(Errno::EEXIST) => {}
            Err(e) => return Err(e),
        }
        for e in fs::read_dir(src)? {
            copy_tree(&fs::join(src, &e.name), &fs::join(dst, &e.name))?;
        }
        return Ok(());
    }
    let input = File::open(src)?;
    let output = File::open_with(dst, O_WRONLY | O_CREAT | O_TRUNC, st.st_mode & 0o777)?;
    let mut buf = [0u8; 16384];
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        output.write_all(&buf[..n])?;
    }
}

struct Fm {
    panels: [Panel; 2],
    active: usize,
    hidden: bool,
    message: String,
    screen: Screen,
    keys: Keys,
}

const FRAME: Style = Style { fg: Some(term::CYAN | term::BRIGHT), bg: Some(term::BLUE), bold: false, reverse: false };
const ITEM: Style = Style { fg: Some(term::WHITE | term::BRIGHT), bg: Some(term::BLUE), bold: false, reverse: false };
const DIR: Style = Style { fg: Some(term::WHITE | term::BRIGHT), bg: Some(term::BLUE), bold: true, reverse: false };
const EXEC: Style = Style { fg: Some(term::GREEN | term::BRIGHT), bg: Some(term::BLUE), bold: false, reverse: false };
const CURSOR: Style = Style { fg: Some(term::BLACK), bg: Some(term::CYAN), bold: false, reverse: false };

impl Fm {
    fn other(&self) -> usize {
        1 - self.active
    }

    fn reload(&mut self) {
        let h = self.hidden;
        for p in self.panels.iter_mut() {
            p.load(h);
        }
    }

    fn list_rows(&self) -> usize {
        self.screen.rows.saturating_sub(5).max(1)
    }

    fn draw_panel(&mut self, idx: usize) {
        let cols = self.screen.cols;
        let width = cols / 2;
        let x0 = idx * width;
        let w = if idx == 1 { cols - width } else { width };
        let rows = self.list_rows();
        let active = idx == self.active;
        let panel = &self.panels[idx];

        // Frame.
        self.screen.put(1, x0, '┌', FRAME);
        self.screen.put(1, x0 + w - 1, '┐', FRAME);
        for x in x0 + 1..x0 + w - 1 {
            self.screen.put(1, x, '─', FRAME);
            self.screen.put(rows + 2, x, '─', FRAME);
        }
        self.screen.put(rows + 2, x0, '└', FRAME);
        self.screen.put(rows + 2, x0 + w - 1, '┘', FRAME);
        for y in 2..rows + 2 {
            self.screen.put(y, x0, '│', FRAME);
            self.screen.put(y, x0 + w - 1, '│', FRAME);
            for x in x0 + 1..x0 + w - 1 {
                self.screen.put(y, x, ' ', ITEM);
            }
        }
        let title_text = format!(" {} ", panel.path);
        let title: String = title_text.chars().rev().take(w - 4).collect::<Vec<_>>().into_iter().rev().collect();
        let title_style = if active { CURSOR } else { FRAME };
        let tx = x0 + (w - title.chars().count()) / 2;
        let panel_top = panel.top;
        let cursor = panel.cursor;
        let entries: Vec<Entry> = panel.entries.clone();
        let error = panel.error.clone();
        self.screen.text(1, tx, &title, title_style);

        let inner = w - 2;
        let size_w = 7;
        for r in 0..rows {
            let i = panel_top + r;
            let Some(e) = entries.get(i) else { break };
            let selected = active && i == cursor;
            let base = if e.is_dir {
                DIR
            } else if e.mode & 0o111 != 0 {
                EXEC
            } else {
                ITEM
            };
            let style = if selected { CURSOR } else { base };
            let marker = if e.is_dir { "/" } else if e.mode & 0o111 != 0 { "*" } else { " " };
            let size = if e.name == ".." { "UP--DIR".to_string() } else if e.is_dir { "DIR".to_string() } else { human(e.size) };
            let name_w = inner - size_w - 2;
            let mut name: String = format!("{}{}", marker, e.name);
            if name.chars().count() > name_w {
                name = name.chars().take(name_w - 1).collect::<String>() + "~";
            }
            let line = format!("{:<nw$}│{:>sw$}", name, size, nw = name_w + 1, sw = size_w);
            for x in x0 + 1..x0 + w - 1 {
                self.screen.put(2 + r, x, ' ', style);
            }
            self.screen.text(2 + r, x0 + 1, &line, style);
        }
        if let Some(err) = error {
            self.screen.text(3, x0 + 2, &err, Style::colors(term::RED | term::BRIGHT, term::BLUE));
        }
        // Info line about the selected entry.
        if let Some(e) = entries.get(cursor) {
            let t = DateTime::from_unix(e.mtime);
            let info = if e.name == ".." {
                String::from(" parent directory ")
            } else {
                format!(" {} {} {:>2} {:02}:{:02} {} ", mode_string(e.mode), MONTHS[(t.month.max(1) - 1) as usize], t.day, t.hour, t.minute, human(e.size))
            };
            let info: String = info.chars().take(w - 2).collect();
            self.screen.text(rows + 2, x0 + 1, &info, FRAME);
        }
    }

    fn draw(&mut self) {
        let rows = self.screen.rows;
        let cols = self.screen.cols;
        self.screen.clear();
        let bar = Style::colors(term::BLACK, term::CYAN);
        self.screen.fill_row(0, 0, bar);
        self.screen.text(0, 1, "fm - Huldra file manager", bar);
        let hint = if self.hidden { "[hidden: on] " } else { "[hidden: off] " };
        self.screen.text(0, cols - hint.len(), hint, bar);
        let rows_list = self.list_rows();
        for idx in 0..2 {
            let p = &mut self.panels[idx];
            if p.cursor < p.top {
                p.top = p.cursor;
            }
            if p.cursor >= p.top + rows_list {
                p.top = p.cursor + 1 - rows_list;
            }
            self.draw_panel(idx);
        }
        let msg = self.message.clone();
        self.screen.text(rows - 2, 0, &msg, Style::fg(term::YELLOW | term::BRIGHT));
        let keys = [("1", "Help"), ("2", "Shell"), ("3", "View"), ("4", "Edit"), ("5", "Copy"), ("6", "Move"), ("7", "Mkdir"), ("8", "Delete"), ("9", "NewFile"), ("10", "Quit")];
        let mut c = 0;
        for (k, label) in keys {
            c = self.screen.text(rows - 1, c, k, Style::NORMAL);
            c = self.screen.text(rows - 1, c, &format!("{:<6}", label), Style::colors(term::BLACK, term::CYAN));
        }
        self.screen.set_cursor(rows - 2, msg.chars().count().min(cols - 1));
    }

    fn prompt(&mut self, label: &str, initial: &str) -> Option<String> {
        let mut input: String = String::from(initial);
        loop {
            self.message = format!("{}{}", label, input);
            self.draw();
            self.screen.present();
            match self.keys.read().ok()? {
                Key::Enter => {
                    self.message.clear();
                    return Some(input);
                }
                Key::Escape | Key::Ctrl('c') | Key::F(10) => {
                    self.message.clear();
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

    fn confirm(&mut self, question: &str) -> bool {
        matches!(self.prompt(&format!("{} (y/n) ", question), "").as_deref(), Some("y" | "Y" | "yes"))
    }

    /// Runs a program with the terminal in normal mode.
    fn run_program(&mut self, raw: &RawMode, path: &str, args: &[&str], wait_key: bool) -> Option<ExitStatus> {
        raw.suspend();
        term::reset_screen();
        let _ = fs::set_current_dir(&self.panels[self.active].path);
        let status = process::run(path, args).ok();
        if wait_key {
            huldra_user::print!("\n\x1b[7m Press any key to return to fm \x1b[0m");
            io::flush_stdout();
            raw.resume();
            let _ = self.keys.read();
        } else {
            raw.resume();
        }
        self.screen.invalidate();
        self.reload();
        status
    }

    fn open(&mut self, raw: &RawMode) {
        let p = &mut self.panels[self.active];
        let Some(e) = p.selected().cloned() else { return };
        if e.is_dir {
            let old = p.path.clone();
            p.path = if e.name == ".." { parent(&p.path) } else { fs::join(&p.path, &e.name) };
            p.cursor = 0;
            p.top = 0;
            p.load(self.hidden);
            if e.name == ".." {
                let name = old.rsplit('/').next().unwrap_or("").to_string();
                self.panels[self.active].select_name(&name);
            }
        } else if e.mode & 0o111 != 0 {
            let path = fs::join(&self.panels[self.active].path, &e.name);
            let st = self.run_program(raw, &path, &[&path], true);
            self.message = format!("{} exited: {:?}", e.name, st);
        } else {
            let path = fs::join(&self.panels[self.active].path, &e.name);
            self.run_program(raw, "/bin/less", &["less", &path], false);
        }
    }

    fn act(&mut self, raw: &RawMode, key: Key) -> bool {
        let rows = self.list_rows();
        let a = self.active;
        let len = self.panels[a].entries.len();
        match key {
            Key::F(10) | Key::Char('q') => return false,
            Key::Tab => self.active = self.other(),
            Key::Up | Key::Char('k') => self.panels[a].cursor = self.panels[a].cursor.saturating_sub(1),
            Key::Down | Key::Char('j') => self.panels[a].cursor = (self.panels[a].cursor + 1).min(len.saturating_sub(1)),
            Key::PageUp => self.panels[a].cursor = self.panels[a].cursor.saturating_sub(rows),
            Key::PageDown => self.panels[a].cursor = (self.panels[a].cursor + rows).min(len.saturating_sub(1)),
            Key::Home => self.panels[a].cursor = 0,
            Key::End => self.panels[a].cursor = len.saturating_sub(1),
            Key::Enter | Key::Right => self.open(raw),
            Key::Backspace | Key::Left => {
                if self.panels[a].path != "/" {
                    self.panels[a].cursor = 0;
                    self.open(raw);
                }
            }
            Key::Char('h') => {
                self.hidden = !self.hidden;
                self.reload();
            }
            Key::Char('r') | Key::Ctrl('r') => self.reload(),
            Key::F(1) => {
                self.message = String::from("Tab panel | Enter open | F3 view F4 edit F5 copy F6 move F7 mkdir F8 del F9 new | ! run | h hidden");
            }
            Key::F(2) | Key::Ctrl('o') => {
                let shell = env::var("SHELL").unwrap_or("/bin/sh").to_string();
                self.run_program(raw, &shell, &["sh"], false);
            }
            Key::F(3) | Key::Char('v') => {
                if let Some(path) = self.panels[a].selected_path() {
                    self.run_program(raw, "/bin/less", &["less", &path], false);
                }
            }
            Key::F(4) | Key::Char('e') => {
                if let Some(path) = self.panels[a].selected_path() {
                    self.run_program(raw, "/bin/edit", &["edit", &path], false);
                }
            }
            Key::F(5) | Key::Char('c') => {
                if let Some(src) = self.panels[a].selected_path() {
                    let name = self.panels[a].selected().unwrap().name.clone();
                    let initial = fs::join(&self.panels[self.other()].path, &name);
                    if let Some(dst) = self.prompt("Copy to: ", &initial) {
                        self.message = match copy_tree(&src, &dst) {
                            Ok(()) => format!("Copied {} -> {}", src, dst),
                            Err(e) => format!("Copy failed: {}", e),
                        };
                        self.reload();
                    }
                }
            }
            Key::F(6) | Key::Char('m') => {
                if let Some(src) = self.panels[a].selected_path() {
                    let name = self.panels[a].selected().unwrap().name.clone();
                    let initial = fs::join(&self.panels[self.other()].path, &name);
                    if let Some(dst) = self.prompt("Move/rename to: ", &initial) {
                        let result = match fs::rename(&src, &dst) {
                            Err(Errno::EXDEV) => copy_tree(&src, &dst).and_then(|_| fs::remove_all(&src)),
                            r => r,
                        };
                        self.message = match result {
                            Ok(()) => format!("Moved {} -> {}", src, dst),
                            Err(e) => format!("Move failed: {}", e),
                        };
                        self.reload();
                    }
                }
            }
            Key::F(7) | Key::Char('n') => {
                if let Some(name) = self.prompt("New directory: ", "") {
                    if !name.is_empty() {
                        let path = fs::join(&self.panels[a].path, &name);
                        self.message = match fs::create_dir(&path) {
                            Ok(()) => format!("Created {}", path),
                            Err(e) => format!("mkdir failed: {}", e),
                        };
                        self.reload();
                        self.panels[a].select_name(&name);
                    }
                }
            }
            Key::F(9) | Key::Char('t') => {
                if let Some(name) = self.prompt("New file: ", "") {
                    if !name.is_empty() {
                        let path = fs::join(&self.panels[a].path, &name);
                        self.message = match sys::open(&path, O_WRONLY | O_CREAT, 0o644) {
                            Ok(fd) => {
                                let _ = sys::close(fd);
                                format!("Created {}", path)
                            }
                            Err(e) => format!("Cannot create: {}", e),
                        };
                        self.reload();
                        self.panels[a].select_name(&name);
                    }
                }
            }
            Key::F(8) | Key::Delete | Key::Char('d') => {
                if let Some(path) = self.panels[a].selected_path() {
                    if self.confirm(&format!("Delete {}?", path)) {
                        self.message = match fs::remove_all(&path) {
                            Ok(()) => format!("Deleted {}", path),
                            Err(e) => format!("Delete failed: {}", e),
                        };
                        self.reload();
                    }
                }
            }
            Key::Char('!') | Key::Char(':') => {
                if let Some(cmd) = self.prompt("Command: ", "") {
                    if !cmd.trim().is_empty() {
                        self.run_program(raw, "/bin/sh", &["sh", "-c", &cmd], true);
                    }
                }
            }
            _ => {}
        }
        true
    }
}

fn main() -> i32 {
    if !term::is_tty(0) {
        eprintln!("fm: needs a terminal");
        return 1;
    }
    let cwd = fs::current_dir().unwrap_or_else(|_| String::from("/"));
    let other = env::args().get(1).cloned().unwrap_or_else(|| String::from("/"));
    let Ok(raw) = RawMode::enable() else { return 1 };
    let mut fm = Fm {
        panels: [Panel::new(&cwd), Panel::new(&other)],
        active: 0,
        hidden: false,
        message: String::from("F1 help, Tab switches panels, F10 quits"),
        screen: Screen::new(),
        keys: Keys::new(),
    };
    loop {
        fm.draw();
        fm.screen.present();
        let Ok(key) = fm.keys.read() else { break };
        if !matches!(key, Key::F(1)) {
            fm.message.clear();
        }
        if !fm.act(&raw, key) {
            break;
        }
    }
    drop(raw);
    term::reset_screen();
    0
}
