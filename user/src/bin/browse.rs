//! browse [-k] [URL|FILE|search words]: a text web browser for the terminal
//! (like Lynx). HTTPS, links, forms, history; no JavaScript or CSS.
//!
//!   Tab / Shift+Tab   next / previous link or field
//!   Enter / Right     follow the link, edit or press the field
//!   b / Left / Bksp   back            f   forward
//!   g                 go to an address (words search the web)
//!   r                 reload          /   find text, n again
//!   s                 save the page or the selected link
//!   u                 show the address of the page
//!   Arrows, PgUp/PgDn, Space, Home/End scroll; q quits.
//!
//! -k skips certificate checks. Without a URL it opens a start page.

#![no_std]
#![no_main]

use huldra_user::browser::{Browser, Target};
use huldra_user::term::{self, Key, Keys, RawMode, Screen, Style};
use huldra_user::{env, eprintln, format, String, ToString};
use huldra_web::layout::{BOLD, CODE, DIM, FIELD, HEADING, ITALIC, LINK};

huldra_user::main!(main);

struct Ui {
    b: Browser,
    screen: Screen,
    keys: Keys,
    search: String,
}

fn style_of(style: u8, selected: bool) -> Style {
    let mut s = if style & HEADING != 0 {
        Style::fg(term::GREEN | term::BRIGHT).bold()
    } else if style & FIELD != 0 {
        Style::fg(term::YELLOW | term::BRIGHT)
    } else if style & LINK != 0 {
        Style::fg(term::CYAN | term::BRIGHT)
    } else if style & CODE != 0 {
        Style::fg(term::YELLOW)
    } else if style & DIM != 0 {
        Style::fg(term::GREEN)
    } else if style & ITALIC != 0 {
        Style::fg(term::MAGENTA | term::BRIGHT)
    } else {
        Style::NORMAL
    };
    if style & BOLD != 0 {
        s = s.bold();
    }
    if selected {
        s.reverse = true;
    }
    s
}

impl Ui {
    fn rows(&self) -> usize {
        self.screen.rows.saturating_sub(2)
    }

    fn draw(&mut self, status: &str) {
        let cols = self.screen.cols;
        self.screen.clear();
        // Title bar.
        let title = format!(" {} ", self.b.doc.page.title);
        self.screen.fill_row(0, 0, Style::REVERSE);
        self.screen.text(0, 0, &title.chars().take(cols).collect::<String>(), Style::REVERSE.bold());
        let rows = self.rows();
        let lines = &self.b.doc.page.lines;
        for r in 0..rows {
            let Some(line) = lines.get(self.b.top + r) else { break };
            let mut col = 0;
            for span in line {
                let sel = match self.b.selected {
                    Some(Target::Link(n)) => span.link == Some(n) && span.field.is_none(),
                    Some(Target::Field(n)) => span.field == Some(n),
                    None => false,
                };
                let st = style_of(span.style, sel);
                for c in span.text.chars() {
                    if col >= cols {
                        break;
                    }
                    self.screen.put(r + 1, col, c, st);
                    col += 1;
                }
            }
        }
        // Status line.
        let last = self.screen.rows - 1;
        let total = lines.len().max(1);
        let pct = ((self.b.top + rows).min(total) * 100) / total;
        let right = format!(" {}% ", pct);
        let left = if status.is_empty() { if self.b.message.is_empty() { self.b.doc.location.to_string() } else { self.b.message.clone() } } else { status.to_string() };
        self.screen.fill_row(last, 0, Style::REVERSE);
        let room = cols.saturating_sub(right.len() + 1);
        self.screen.text(last, 0, &format!(" {}", left.chars().take(room).collect::<String>()), Style::REVERSE);
        self.screen.text(last, cols.saturating_sub(right.len()), &right, Style::REVERSE);
        self.screen.set_cursor(last, cols - 1);
        self.screen.present();
    }

    fn prompt(&mut self, label: &str, initial: &str) -> Option<String> {
        let mut input = String::from(initial);
        loop {
            let shown = format!("{}{}", label, input);
            self.draw(&shown);
            let n = shown.chars().count() + 1;
            self.screen.set_cursor(self.screen.rows - 1, n.min(self.screen.cols - 1));
            self.screen.present();
            match self.keys.read().ok()? {
                Key::Enter => return Some(input),
                Key::Escape | Key::Ctrl('c') | Key::Ctrl('g') => return None,
                Key::Backspace => {
                    input.pop();
                }
                Key::Ctrl('u') => input.clear(),
                Key::Char(c) => input.push(c),
                _ => {}
            }
        }
    }

    fn loading(&mut self, what: &str) {
        self.draw(&format!("loading {} ...", what));
    }

    fn scroll(&mut self, delta: isize) {
        let max = self.b.doc.page.lines.len().saturating_sub(self.rows().min(self.b.doc.page.lines.len()));
        self.b.top = (self.b.top as isize + delta).clamp(0, max as isize) as usize;
    }

    fn run(&mut self) {
        loop {
            self.draw("");
            let Ok(key) = self.keys.read() else { return };
            let rows = self.rows() as isize;
            self.b.message.clear();
            match key {
                Key::Char('q') | Key::Ctrl('c') => return,
                Key::Down | Key::Char('j') => self.scroll(1),
                Key::Up | Key::Char('k') => self.scroll(-1),
                Key::PageDown | Key::Char(' ') => self.scroll(rows - 1),
                Key::PageUp => self.scroll(-(rows - 1)),
                Key::Home => self.b.top = 0,
                Key::End => self.scroll(isize::MAX / 2),
                Key::Tab => {
                    self.b.select_next(true, self.rows());
                }
                Key::BackTab => {
                    self.b.select_next(false, self.rows());
                }
                Key::Enter | Key::Right => {
                    if self.b.selected.is_none() {
                        self.b.select_next(true, self.rows());
                        continue;
                    }
                    if let Some(Target::Link(_)) = self.b.selected {
                        let msg = self.b.message.clone();
                        self.loading(&msg);
                    }
                    if let Some(f) = self.b.activate() {
                        let current = self.b.doc.page.fields[f].value.clone();
                        let name = self.b.doc.page.fields[f].name.clone();
                        if let Some(v) = self.prompt(&format!("{}: ", if name.is_empty() { "value" } else { &name }), &current) {
                            self.b.set_field(f, v);
                            // Enter in a text field submits its form, like browsers.
                            self.loading("form");
                            self.b.submit(f);
                        }
                    }
                }
                Key::Char('b') | Key::Left | Key::Backspace => {
                    self.loading("previous page");
                    self.b.go_back();
                }
                Key::Char('f') => {
                    self.loading("next page");
                    self.b.go_forward();
                }
                Key::Char('r') => {
                    self.loading("page");
                    self.b.reload();
                }
                Key::Char('g') | Key::Char('o') => {
                    if let Some(input) = self.prompt("Go to (address or search words): ", "") {
                        match self.b.parse_input(&input) {
                            Ok(loc) => {
                                self.loading(&loc.to_string());
                                self.b.open(loc);
                            }
                            Err(e) => self.b.message = e,
                        }
                    }
                }
                Key::Char('/') => {
                    if let Some(t) = self.prompt("Find: ", "") {
                        self.search = t.clone();
                        self.b.find(&t);
                    }
                }
                Key::Char('n') if !self.search.is_empty() => {
                    let s = self.search.clone();
                    self.b.find(&s);
                }
                Key::Char('s') => {
                    self.loading("download");
                    self.b.message = self.b.save().unwrap_or_else(|e| e);
                }
                Key::Char('u') => self.b.message = self.b.doc.location.to_string(),
                _ => {}
            }
        }
    }
}

fn main() -> i32 {
    let args = env::args();
    let insecure = args.iter().any(|a| a == "-k" || a == "--insecure");
    let words: huldra_user::Vec<&str> = args[1..].iter().map(|s| s.as_str()).filter(|a| !a.starts_with('-')).collect();
    let (_, cols) = term::size();
    let mut b = Browser::new(cols.saturating_sub(1));
    b.insecure = insecure;
    if !term::is_tty(huldra_user::io::STDOUT) {
        // Not a terminal: print the page as text (handy in pipes).
        if !words.is_empty() {
            match b.parse_input(&words.join(" ")) {
                Ok(loc) => b.open(loc),
                Err(e) => {
                    eprintln!("browse: {}", e);
                    return 1;
                }
            }
            if !b.message.is_empty() && b.doc.page.lines.is_empty() {
                eprintln!("browse: {}", b.message);
                return 1;
            }
        }
        b.set_width(80);
        for i in 0..b.doc.page.lines.len() {
            huldra_user::println!("{}", b.doc.page.line_text(i));
        }
        if !b.message.is_empty() {
            eprintln!("browse: {}", b.message);
        }
        return 0;
    }
    let Ok(_raw) = RawMode::enable() else {
        eprintln!("browse: cannot use the terminal");
        return 1;
    };
    let mut ui = Ui { b, screen: Screen::new(), keys: Keys::new(), search: String::new() };
    if !words.is_empty() {
        let input = words.join(" ");
        ui.loading(&input);
        match ui.b.parse_input(&input) {
            Ok(loc) => ui.b.open(loc),
            Err(e) => ui.b.message = e.to_string(),
        }
    }
    ui.run();
    term::reset_screen();
    0
}
