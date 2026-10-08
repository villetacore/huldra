//! Minimal syntax highlighting: keywords, types, strings, numbers and
//! comments for a few languages.

use alloc::vec;
use alloc::vec::Vec;
use huldra_user::term::{self, Style};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Plain,
    Rust,
    C,
    Shell,
    Python,
    Markdown,
}

#[derive(Clone, Copy, Default)]
pub struct State {
    in_block_comment: bool,
}

const RUST: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in",
    "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super",
    "trait", "true", "type", "unsafe", "use", "where", "while", "dyn", "async", "await",
];
const C: &[&str] = &[
    "auto", "break", "case", "char", "const", "continue", "default", "do", "double", "else", "enum", "extern", "float",
    "for", "goto", "if", "int", "long", "register", "return", "short", "signed", "sizeof", "static", "struct", "switch",
    "typedef", "union", "unsigned", "void", "volatile", "while", "#include", "#define", "#ifdef", "#ifndef", "#endif",
    "#if", "#else", "NULL",
];
const SHELL: &[&str] = &[
    "if", "then", "elif", "else", "fi", "for", "in", "do", "done", "while", "until", "case", "esac", "function",
    "return", "exit", "export", "local", "echo", "cd", "read", "set", "unset", "shift", "test",
];
const PYTHON: &[&str] = &[
    "and", "as", "assert", "break", "class", "continue", "def", "del", "elif", "else", "except", "False", "finally",
    "for", "from", "global", "if", "import", "in", "is", "lambda", "None", "nonlocal", "not", "or", "pass", "raise",
    "return", "True", "try", "while", "with", "yield", "self", "print",
];

const KEYWORD: Style = Style { fg: Some(term::YELLOW | term::BRIGHT), bg: None, bold: false, reverse: false };
const TYPE: Style = Style { fg: Some(term::GREEN | term::BRIGHT), bg: None, bold: false, reverse: false };
const STRING: Style = Style { fg: Some(term::MAGENTA | term::BRIGHT), bg: None, bold: false, reverse: false };
const NUMBER: Style = Style { fg: Some(term::RED | term::BRIGHT), bg: None, bold: false, reverse: false };
const COMMENT: Style = Style { fg: Some(term::BLACK | term::BRIGHT), bg: None, bold: false, reverse: false };
const HEADING: Style = Style { fg: Some(term::CYAN | term::BRIGHT), bg: None, bold: true, reverse: false };

impl Lang {
    pub fn detect(path: &str, first_line: Option<&str>) -> Lang {
        let ext = path.rsplit('.').next().unwrap_or("");
        match ext {
            "rs" => Lang::Rust,
            "c" | "h" | "cc" | "cpp" | "hpp" => Lang::C,
            "sh" => Lang::Shell,
            "py" => Lang::Python,
            "md" => Lang::Markdown,
            _ => match first_line {
                Some(l) if l.starts_with("#!") && l.contains("sh") => Lang::Shell,
                Some(l) if l.starts_with("#!") && l.contains("python") => Lang::Python,
                _ => Lang::Plain,
            },
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Lang::Plain => "text",
            Lang::Rust => "Rust",
            Lang::C => "C",
            Lang::Shell => "sh",
            Lang::Python => "Python",
            Lang::Markdown => "Markdown",
        }
    }

    fn keywords(&self) -> &'static [&'static str] {
        match self {
            Lang::Rust => RUST,
            Lang::C => C,
            Lang::Shell => SHELL,
            Lang::Python => PYTHON,
            _ => &[],
        }
    }

    fn line_comment(&self) -> &'static str {
        match self {
            Lang::Rust | Lang::C => "//",
            Lang::Shell | Lang::Python => "#",
            _ => "",
        }
    }

    fn block_comments(&self) -> bool {
        matches!(self, Lang::Rust | Lang::C)
    }

    /// Comment state after `line` (cheap version of `highlight`).
    pub fn scan_state(&self, line: &[char], state: State) -> State {
        self.highlight(line, state).1
    }

    pub fn highlight(&self, line: &[char], mut state: State) -> (Vec<Style>, State) {
        let mut styles = vec![Style::NORMAL; line.len()];
        if *self == Lang::Plain {
            return (styles, state);
        }
        if *self == Lang::Markdown {
            let s: alloc::string::String = line.iter().collect();
            let style = if s.starts_with('#') {
                HEADING
            } else if s.starts_with("```") || s.starts_with("    ") {
                STRING
            } else {
                Style::NORMAL
            };
            styles.fill(style);
            return (styles, state);
        }
        let lc: Vec<char> = self.line_comment().chars().collect();
        let mut i = 0;
        while i < line.len() {
            if state.in_block_comment {
                styles[i] = COMMENT;
                if line[i] == '*' && line.get(i + 1) == Some(&'/') {
                    styles[i + 1] = COMMENT;
                    state.in_block_comment = false;
                    i += 2;
                } else {
                    i += 1;
                }
                continue;
            }
            let c = line[i];
            if self.block_comments() && c == '/' && line.get(i + 1) == Some(&'*') {
                state.in_block_comment = true;
                styles[i] = COMMENT;
                styles[i + 1] = COMMENT;
                i += 2;
                continue;
            }
            if !lc.is_empty() && line[i..].starts_with(&lc) {
                for s in &mut styles[i..] {
                    *s = COMMENT;
                }
                break;
            }
            if c == '"' || (c == '\'' && self.is_quote_char(line, i)) {
                let start = i;
                i += 1;
                while i < line.len() && line[i] != c {
                    if line[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
                let end = (i + 1).min(line.len());
                for s in &mut styles[start..end] {
                    *s = STRING;
                }
                i = end;
                continue;
            }
            if c.is_ascii_digit() && (i == 0 || !is_word(line[i - 1])) {
                let start = i;
                while i < line.len() && (line[i].is_ascii_alphanumeric() || line[i] == '.' || line[i] == '_') {
                    i += 1;
                }
                for s in &mut styles[start..i] {
                    *s = NUMBER;
                }
                continue;
            }
            if is_word(c) || c == '#' {
                let start = i;
                i += 1;
                while i < line.len() && is_word(line[i]) {
                    i += 1;
                }
                let word: alloc::string::String = line[start..i].iter().collect();
                let style = if self.keywords().contains(&word.as_str()) {
                    KEYWORD
                } else if *self == Lang::Rust && word.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                    TYPE
                } else if *self == Lang::Rust && line.get(i) == Some(&'!') {
                    TYPE // macro
                } else {
                    Style::NORMAL
                };
                for s in &mut styles[start..i] {
                    *s = style;
                }
                continue;
            }
            i += 1;
        }
        (styles, state)
    }

    /// In Rust, `'` starts a char literal only like `'a'` or `'\n'` (not a lifetime).
    fn is_quote_char(&self, line: &[char], i: usize) -> bool {
        match self {
            Lang::Rust => line.get(i + 2) == Some(&'\'') || (line.get(i + 1) == Some(&'\\')),
            _ => true,
        }
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}
