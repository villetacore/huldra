//! Interactive line editor: cursor movement, history and completion.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use huldra_user::io::{write_all, STDOUT};
use huldra_user::term::{self, Key, Keys, RawMode};
use huldra_user::{env, fs};

const HISTORY_MAX: usize = 500;

fn history_path() -> String {
    fs::join(env::var("HOME").unwrap_or("/root"), ".sh_history")
}

pub fn load_history() -> Vec<String> {
    fs::read_to_string(&history_path())
        .map(|t| t.lines().map(String::from).collect())
        .unwrap_or_default()
}

pub fn save_history(history: &[String]) {
    let start = history.len().saturating_sub(HISTORY_MAX);
    let mut text = history[start..].join("\n");
    text.push('\n');
    let _ = fs::write(&history_path(), text.as_bytes());
}

pub enum ReadResult {
    Line(String),
    Interrupted,
    Eof,
}

/// Visible width of a prompt (ANSI escapes excluded).
fn visible_width(s: &str) -> usize {
    let mut w = 0;
    let mut esc = false;
    for c in s.chars() {
        if esc {
            if c.is_ascii_alphabetic() {
                esc = false;
            }
        } else if c == '\x1b' {
            esc = true;
        } else {
            w += 1;
        }
    }
    w
}

struct Editor<'a> {
    prompt: &'a str,
    prompt_width: usize,
    line: Vec<char>,
    cursor: usize,
    /// Row (relative to the prompt's first row) the terminal cursor is on.
    cursor_row: usize,
    cols: usize,
}

impl Editor<'_> {
    fn out(&self, s: &str) {
        let _ = write_all(STDOUT, s.as_bytes());
    }

    /// Redraws prompt and line, leaving the cursor at `self.cursor`.
    fn redraw(&mut self) {
        let mut s = String::new();
        if self.cursor_row > 0 {
            s.push_str(&alloc::format!("\x1b[{}A", self.cursor_row));
        }
        s.push('\r');
        s.push_str(self.prompt);
        s.extend(self.line.iter());
        s.push_str("\x1b[J");
        let total = self.prompt_width + self.line.len();
        if total > 0 && total % self.cols == 0 {
            s.push_str("\r\n");
        }
        let end_row = total / self.cols;
        let target = self.prompt_width + self.cursor;
        let (row, col) = (target / self.cols, target % self.cols);
        if end_row > row {
            s.push_str(&alloc::format!("\x1b[{}A", end_row - row));
        }
        s.push('\r');
        if col > 0 {
            s.push_str(&alloc::format!("\x1b[{}C", col));
        }
        self.cursor_row = row;
        self.out(&s);
    }

    fn set_line(&mut self, s: &str) {
        self.line = s.chars().collect();
        self.cursor = self.line.len();
        self.redraw();
    }
}

pub fn read_line(prompt: &str, history: &mut Vec<String>, completer: &dyn Fn(&str, usize) -> (usize, Vec<String>)) -> ReadResult {
    let Ok(_raw) = RawMode::enable() else {
        return ReadResult::Eof;
    };
    let (_, cols) = term::size();
    let mut ed = Editor { prompt, prompt_width: visible_width(prompt), line: Vec::new(), cursor: 0, cursor_row: 0, cols };
    ed.out(prompt);
    let mut keys = Keys::new();
    let mut hist_pos = history.len();
    let mut draft = String::new();
    let mut last_was_tab = false;
    loop {
        let Ok(key) = keys.read() else { return ReadResult::Eof };
        let tab = key == Key::Tab;
        match key {
            Key::Enter => {
                if ed.cursor != ed.line.len() {
                    ed.cursor = ed.line.len();
                    ed.redraw();
                }
                ed.out("\r\n");
                let line: String = ed.line.iter().collect();
                if !line.trim().is_empty() && history.last() != Some(&line) {
                    history.push(line.clone());
                }
                return ReadResult::Line(line);
            }
            Key::Ctrl('c') => {
                ed.cursor = ed.line.len();
                ed.redraw();
                ed.out("^C\r\n");
                return ReadResult::Interrupted;
            }
            Key::Ctrl('d') if ed.line.is_empty() => {
                ed.out("\r\n");
                return ReadResult::Eof;
            }
            Key::Ctrl('d') | Key::Delete => {
                if ed.cursor < ed.line.len() {
                    ed.line.remove(ed.cursor);
                    ed.redraw();
                }
            }
            Key::Backspace | Key::Ctrl('h') => {
                if ed.cursor > 0 {
                    ed.cursor -= 1;
                    ed.line.remove(ed.cursor);
                    ed.redraw();
                }
            }
            Key::Left | Key::Ctrl('b') => {
                if ed.cursor > 0 {
                    ed.cursor -= 1;
                    ed.redraw();
                }
            }
            Key::Right | Key::Ctrl('f') => {
                if ed.cursor < ed.line.len() {
                    ed.cursor += 1;
                    ed.redraw();
                }
            }
            Key::Home | Key::Ctrl('a') => {
                ed.cursor = 0;
                ed.redraw();
            }
            Key::End | Key::Ctrl('e') => {
                ed.cursor = ed.line.len();
                ed.redraw();
            }
            Key::Ctrl('u') => {
                ed.line.drain(..ed.cursor);
                ed.cursor = 0;
                ed.redraw();
            }
            Key::Ctrl('k') => {
                ed.line.truncate(ed.cursor);
                ed.redraw();
            }
            Key::Ctrl('w') => {
                let mut start = ed.cursor;
                while start > 0 && ed.line[start - 1] == ' ' {
                    start -= 1;
                }
                while start > 0 && ed.line[start - 1] != ' ' {
                    start -= 1;
                }
                ed.line.drain(start..ed.cursor);
                ed.cursor = start;
                ed.redraw();
            }
            Key::Ctrl('l') => {
                ed.out("\x1b[2J\x1b[H");
                ed.cursor_row = 0;
                ed.redraw();
            }
            Key::Up | Key::Ctrl('p') => {
                if hist_pos > 0 {
                    if hist_pos == history.len() {
                        draft = ed.line.iter().collect();
                    }
                    hist_pos -= 1;
                    let h = history[hist_pos].clone();
                    ed.set_line(&h);
                }
            }
            Key::Down | Key::Ctrl('n') => {
                if hist_pos < history.len() {
                    hist_pos += 1;
                    let h = if hist_pos == history.len() { draft.clone() } else { history[hist_pos].clone() };
                    ed.set_line(&h);
                }
            }
            Key::Tab => {
                let text: String = ed.line.iter().collect();
                let byte_cursor: usize = ed.line[..ed.cursor].iter().map(|c| c.len_utf8()).sum();
                let (start, candidates) = completer(&text, byte_cursor);
                let word: String = text[start..byte_cursor].to_string();
                if candidates.len() == 1 {
                    let add: Vec<char> = candidates[0][word.len()..].chars().collect();
                    let n = add.len();
                    for (i, c) in add.into_iter().enumerate() {
                        ed.line.insert(ed.cursor + i, c);
                    }
                    ed.cursor += n;
                    if !candidates[0].ends_with('/') {
                        ed.line.insert(ed.cursor, ' ');
                        ed.cursor += 1;
                    }
                    ed.redraw();
                } else if candidates.len() > 1 {
                    let prefix = common_prefix(&candidates);
                    if prefix.len() > word.len() {
                        let add: Vec<char> = prefix[word.len()..].chars().collect();
                        let n = add.len();
                        for (i, c) in add.into_iter().enumerate() {
                            ed.line.insert(ed.cursor + i, c);
                        }
                        ed.cursor += n;
                        ed.redraw();
                    } else if last_was_tab {
                        let mut list = String::from("\r\n");
                        let width = candidates.iter().map(|c| display_name(c).len()).max().unwrap_or(0) + 2;
                        let per_row = (cols / width).max(1);
                        for (i, c) in candidates.iter().enumerate() {
                            list.push_str(&alloc::format!("{:<w$}", display_name(c), w = width));
                            if (i + 1) % per_row == 0 {
                                list.push_str("\r\n");
                            }
                        }
                        if candidates.len() % per_row != 0 {
                            list.push_str("\r\n");
                        }
                        ed.finish_with(&list);
                    } else {
                        ed.out("\x07");
                    }
                } else {
                    ed.out("\x07");
                }
            }
            Key::Char(c) => {
                ed.line.insert(ed.cursor, c);
                ed.cursor += 1;
                if ed.cursor == ed.line.len() && (ed.prompt_width + ed.line.len()) % ed.cols != 0 {
                    // Fast path: appending at the end.
                    let mut buf = [0u8; 4];
                    ed.out(c.encode_utf8(&mut buf));
                    ed.cursor_row = (ed.prompt_width + ed.cursor) / ed.cols;
                } else {
                    ed.redraw();
                }
            }
            _ => {}
        }
        last_was_tab = tab;
    }
}

impl Editor<'_> {
    /// Prints `text` below the line, then redraws the prompt and line.
    fn finish_with(&mut self, text: &str) {
        let total = self.prompt_width + self.line.len();
        let end_row = total / self.cols;
        if end_row > self.cursor_row {
            self.out(&alloc::format!("\x1b[{}B", end_row - self.cursor_row));
        }
        self.out(text);
        self.cursor_row = 0;
        self.out(self.prompt);
        let line: String = self.line.iter().collect();
        self.out(&line);
        self.cursor_row = total / self.cols;
        self.redraw();
    }
}

fn display_name(c: &str) -> &str {
    let trimmed = c.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(i) => &c[i + 1..],
        None => c,
    }
}

fn common_prefix(items: &[String]) -> String {
    let mut prefix = items[0].clone();
    for s in &items[1..] {
        while !s.starts_with(&prefix) {
            prefix.pop();
        }
    }
    prefix
}
