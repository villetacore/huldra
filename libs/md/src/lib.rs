//! Markdown to terminal text, for `help` and reading the documentation in
//! the system.
//!
//! Handles what the Huldra docs use: headings, paragraphs (wrapped to the
//! terminal width), bullet and numbered lists, fenced code blocks, block
//! quotes, tables (columns aligned), rules, and inline `code`, **bold**,
//! *emphasis* and [links](url). HTML tags are dropped. With `color` the
//! output uses SGR escape sequences (which `less` understands); without
//! it, plain text.

#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Style {
    Plain,
    Bold,
    Code,
    Link,
}

const RESET: &str = "\x1b[0m";

fn sgr(style: Style) -> &'static str {
    match style {
        Style::Plain => "",
        Style::Bold => "\x1b[1m",
        Style::Code => "\x1b[33m",
        Style::Link => "\x1b[36m",
    }
}

/// Inline markup into (text, style) runs.
fn inline(text: &str) -> Vec<(String, Style)> {
    let mut out: Vec<(String, Style)> = Vec::new();
    let mut cur = String::new();
    let push = |out: &mut Vec<(String, Style)>, cur: &mut String, style: Style| {
        if !cur.is_empty() {
            out.push((core::mem::take(cur), style));
        }
    };
    let chars: Vec<char> = strip_html(text).chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let rest = &chars[i..];
        let find = |from: usize, pat: &[char]| -> Option<usize> {
            (from..chars.len().saturating_sub(pat.len() - 1)).find(|&j| chars[j..j + pat.len()] == *pat)
        };
        if c == '\\' && i + 1 < chars.len() && "\\`*_[]()#|<>".contains(chars[i + 1]) {
            cur.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '`' {
            if let Some(end) = find(i + 1, &['`']) {
                push(&mut out, &mut cur, Style::Plain);
                out.push((chars[i + 1..end].iter().collect(), Style::Code));
                i = end + 1;
                continue;
            }
        }
        if rest.starts_with(&['*', '*']) || rest.starts_with(&['_', '_']) {
            if let Some(end) = find(i + 2, &rest[..2]) {
                push(&mut out, &mut cur, Style::Plain);
                for (t, s) in inline(&chars[i + 2..end].iter().collect::<String>()) {
                    out.push((t, if s == Style::Plain { Style::Bold } else { s }));
                }
                i = end + 2;
                continue;
            }
        }
        if (c == '*' || c == '_') && i + 1 < chars.len() && !chars[i + 1].is_whitespace() && (i == 0 || !chars[i - 1].is_alphanumeric()) {
            if let Some(end) = find(i + 1, &[c]) {
                if end > i + 1 && !chars[end - 1].is_whitespace() {
                    cur.extend(&chars[i + 1..end]);
                    i = end + 1;
                    continue;
                }
            }
        }
        if c == '[' {
            if let Some(close) = find(i + 1, &[']', '(']) {
                if let Some(end) = find(close + 2, &[')']) {
                    push(&mut out, &mut cur, Style::Plain);
                    let label: String = chars[i + 1..close].iter().collect();
                    for (t, s) in inline(&label) {
                        out.push((t, if s == Style::Plain { Style::Link } else { s }));
                    }
                    i = end + 1;
                    continue;
                }
            }
        }
        cur.push(c);
        i += 1;
    }
    push(&mut out, &mut cur, Style::Plain);
    out
}

/// Drops HTML tags (`<div align="center">`, `<img ...>`, `<br>`).
fn strip_html(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        let is_tag = after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
        match (is_tag, after.find('>')) {
            (true, Some(end)) => {
                out.push_str(&rest[..start]);
                rest = &after[end + 1..];
            }
            _ => {
                out.push_str(&rest[..=start]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn width_of(s: &str) -> usize {
    s.chars().count()
}

/// The output being built.
struct Out {
    text: String,
    color: bool,
    width: usize,
}

impl Out {
    fn styled(&mut self, text: &str, style: Style) {
        if self.color && style != Style::Plain {
            self.text.push_str(sgr(style));
            self.text.push_str(text);
            self.text.push_str(RESET);
        } else {
            self.text.push_str(text);
        }
    }

    fn raw(&mut self, color: &str, text: &str) {
        if self.color {
            self.text.push_str(color);
            self.text.push_str(text);
            self.text.push_str(RESET);
        } else {
            self.text.push_str(text);
        }
    }

    /// Word-wraps runs: the first line starts with `first`, the others with
    /// `rest` (both plain, same width).
    fn wrap(&mut self, runs: &[(String, Style)], first: &str, rest: &str) {
        let indent = width_of(first);
        let avail = self.width.saturating_sub(indent).max(20);
        // Words keep their style; a word may consist of several runs.
        let mut words: Vec<Vec<(String, Style)>> = Vec::new();
        let mut glue = false;
        for (text, style) in runs {
            for (k, piece) in text.split(' ').enumerate() {
                if k > 0 {
                    glue = false;
                }
                if piece.is_empty() {
                    continue;
                }
                if glue {
                    words.last_mut().unwrap().push((piece.to_string(), *style));
                } else {
                    words.push(alloc::vec![(piece.to_string(), *style)]);
                }
                glue = true;
            }
            glue = glue && !text.ends_with(' ');
        }
        self.text.push_str(first);
        let mut col = 0;
        for w in words {
            let len: usize = w.iter().map(|(t, _)| width_of(t)).sum();
            if col > 0 && col + 1 + len > avail {
                self.text.push('\n');
                self.text.push_str(rest);
                col = 0;
            } else if col > 0 {
                self.text.push(' ');
                col += 1;
            }
            for (t, s) in &w {
                self.styled(t, *s);
            }
            col += len;
        }
        self.text.push('\n');
    }
}

fn is_table_rule(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.chars().all(|c| "|-: ".contains(c))
}

fn table_cells(line: &str) -> Vec<String> {
    let t = line.trim().trim_start_matches('|').trim_end_matches('|');
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut code = false;
    let mut prev = ' ';
    for c in t.chars() {
        if c == '`' {
            code = !code;
        }
        if c == '|' && !code && prev != '\\' {
            cells.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(c);
        }
        prev = c;
    }
    cells.push(cur.trim().to_string());
    cells
}

fn plain(runs: &[(String, Style)]) -> String {
    runs.iter().map(|(t, _)| t.as_str()).collect()
}

fn table(out: &mut Out, rows: &[Vec<String>], header: bool) {
    let parsed: Vec<Vec<Vec<(String, Style)>>> = rows.iter().map(|r| r.iter().map(|c| inline(c)).collect()).collect();
    let ncols = parsed.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut widths = alloc::vec![0usize; ncols];
    for r in &parsed {
        for (i, c) in r.iter().enumerate() {
            widths[i] = widths[i].max(width_of(&plain(c)));
        }
    }
    // Too wide: give the last column what is left and wrap it.
    let fixed: usize = widths.iter().take(ncols.saturating_sub(1)).map(|w| w + 2).sum::<usize>() + 2;
    let last_width = out.width.saturating_sub(fixed).max(16);
    for (n, r) in parsed.iter().enumerate() {
        let mut first = String::from("  ");
        for (i, c) in r.iter().enumerate().take(ncols.saturating_sub(1)) {
            let text = plain(c);
            let pad = widths[i] - width_of(&text);
            let mut cell = Out { text: String::new(), color: out.color, width: usize::MAX };
            for (t, s) in c {
                cell.styled(t, if n == 0 && header { Style::Bold } else { *s });
            }
            first.push_str(&cell.text);
            first.extend(core::iter::repeat_n(' ', pad + 2));
        }
        let last = r.get(ncols - 1).cloned().unwrap_or_default();
        let hanging: String = core::iter::repeat_n(' ', fixed).collect();
        let mut o = Out { text: String::new(), color: out.color, width: fixed + last_width };
        let runs: Vec<(String, Style)> = if n == 0 && header { last.iter().map(|(t, _)| (t.clone(), Style::Bold)).collect() } else { last };
        // `first` contains escapes: wrap with a plain indent of the right width.
        o.wrap(&runs, &hanging, &hanging);
        out.text.push_str(&first);
        out.text.push_str(o.text.trim_start_matches(' '));
        if n == 0 && header {
            let total: usize = widths.iter().map(|w| w + 2).sum::<usize>().min(out.width.saturating_sub(2));
            let rule: String = core::iter::repeat_n('─', total.saturating_sub(2)).collect();
            out.text.push_str("  ");
            out.raw("\x1b[32m", &rule);
            out.text.push('\n');
        }
    }
}

/// Renders markdown for a terminal `width` columns wide.
pub fn render(md: &str, width: usize, color: bool) -> String {
    let mut out = Out { text: String::new(), color, width: width.max(30) };
    let lines: Vec<&str> = md.lines().collect();
    let mut i = 0;
    let mut para: Vec<&str> = Vec::new();
    let flush = |out: &mut Out, para: &mut Vec<&str>| {
        if !para.is_empty() {
            let text = para.iter().map(|l| l.trim()).collect::<Vec<_>>().join(" ");
            let runs = inline(&text);
            if !plain(&runs).trim().is_empty() {
                out.wrap(&runs, "", "");
                out.text.push('\n');
            }
            para.clear();
        }
    };
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim();
        // Fenced code.
        if t.starts_with("```") {
            flush(&mut out, &mut para);
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with("```") {
                out.text.push_str("    ");
                out.raw("\x1b[33m", lines[i].trim_end());
                out.text.push('\n');
                i += 1;
            }
            out.text.push('\n');
            i += 1;
            continue;
        }
        if t.is_empty() {
            flush(&mut out, &mut para);
            i += 1;
            continue;
        }
        // Headings.
        if let Some(level) = t.split(' ').next().filter(|h| !h.is_empty() && h.chars().all(|c| c == '#')).map(|h| h.len()) {
            flush(&mut out, &mut para);
            let title = plain(&inline(t[level..].trim()));
            match level {
                1 => {
                    out.raw("\x1b[1;92m", &title);
                    out.text.push('\n');
                    let rule: String = core::iter::repeat_n('═', width_of(&title).min(out.width)).collect();
                    out.raw("\x1b[32m", &rule);
                }
                2 => {
                    out.raw("\x1b[1;92m", &title);
                    out.text.push('\n');
                    let rule: String = core::iter::repeat_n('─', width_of(&title).min(out.width)).collect();
                    out.raw("\x1b[32m", &rule);
                }
                _ => out.raw("\x1b[1m", &title),
            }
            out.text.push_str("\n\n");
            i += 1;
            continue;
        }
        // Rules.
        if t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*')) {
            flush(&mut out, &mut para);
            let rule: String = core::iter::repeat_n('─', out.width.min(72)).collect();
            out.raw("\x1b[32m", &rule);
            out.text.push_str("\n\n");
            i += 1;
            continue;
        }
        // Tables.
        if t.starts_with('|') {
            flush(&mut out, &mut para);
            let mut rows = Vec::new();
            while i < lines.len() && lines[i].trim().starts_with('|') {
                if !is_table_rule(lines[i]) {
                    rows.push(table_cells(lines[i]));
                }
                i += 1;
            }
            // A header row of empty cells (`| | |`) means no header.
            let blank = rows.first().is_some_and(|r| r.iter().all(|c| c.is_empty()));
            if blank {
                rows.remove(0);
            }
            table(&mut out, &rows, !blank && rows.len() > 1);
            out.text.push('\n');
            continue;
        }
        // Block quotes.
        if let Some(q) = t.strip_prefix('>') {
            flush(&mut out, &mut para);
            out.raw("\x1b[32m", "  │ ");
            out.styled(&plain(&inline(q.trim())), Style::Plain);
            out.text.push('\n');
            i += 1;
            continue;
        }
        // Lists: "- x", "* x", "1. x", with continuation lines indented.
        let indent = line.len() - line.trim_start().len();
        let bullet = if t.starts_with("- ") || t.starts_with("* ") {
            Some((2, String::from("•")))
        } else {
            let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
            (digits > 0 && t[digits..].starts_with(". ")).then(|| (digits + 2, String::from(&t[..digits + 1])))
        };
        if let Some((skip, mark)) = bullet {
            flush(&mut out, &mut para);
            let mut text = String::from(&t[skip..]);
            i += 1;
            while i < lines.len() {
                let next = lines[i];
                let nt = next.trim();
                let next_indent = next.len() - next.trim_start().len();
                let is_item = nt.starts_with("- ") || nt.starts_with("* ") || nt.chars().next().is_some_and(|c| c.is_ascii_digit()) && nt.contains(". ");
                if nt.is_empty() || next_indent <= indent || is_item && next_indent <= indent + 2 || nt.starts_with("```") {
                    break;
                }
                text.push(' ');
                text.push_str(nt);
                i += 1;
            }
            let lead: String = core::iter::repeat_n(' ', 2 + indent).collect();
            let first = format!("{}{} ", lead, mark);
            let rest: String = core::iter::repeat_n(' ', width_of(&first)).collect();
            // The bullet itself in green.
            let mut o = Out { text: String::new(), color, width: out.width };
            o.wrap(&inline(&text), &rest, &rest);
            out.text.push_str(&lead);
            out.raw("\x1b[32m", &mark);
            out.text.push(' ');
            out.text.push_str(&o.text[rest.len()..]);
            if i >= lines.len() || lines[i].trim().is_empty() {
                out.text.push('\n');
            }
            continue;
        }
        para.push(line);
        i += 1;
    }
    flush(&mut out, &mut para);
    while out.text.ends_with("\n\n") {
        out.text.pop();
    }
    out.text
}

/// The text with all escape sequences removed (tests, plain output).
pub fn strip_escapes(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            for d in it.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_render(md: &str, w: usize) -> String {
        render(md, w, false)
    }

    #[test]
    fn headings_and_paragraphs() {
        let r = plain_render("# Title\n\nSome *words* and `code` and **bold**\nacross lines.\n", 80);
        assert_eq!(r, "Title\n═════\n\nSome words and code and bold across lines.\n");
        let c = render("Use `pkg` now", 80, true);
        assert!(c.contains("\x1b[33mpkg\x1b[0m"));
        assert_eq!(strip_escapes(&c), "Use pkg now\n");
    }

    #[test]
    fn wrapping() {
        let r = plain_render("aaa bbb ccc ddd eee fff ggg hhh iii jjj kkk lll mmm nnn ooo ppp", 30);
        for l in r.lines() {
            assert!(l.chars().count() <= 30, "{:?}", l);
        }
        assert_eq!(r.split_whitespace().count(), 16);
    }

    #[test]
    fn lists_links_html() {
        let r = plain_render("<div align=\"center\">\n\n- one [link](http://x) here\n- two\n  continued\n1. first\n\n</div>\n", 80);
        assert_eq!(r, "  • one link here\n  • two continued\n  1. first\n");
    }

    #[test]
    fn code_blocks_and_tables() {
        let r = plain_render("```sh\nls -l\n```\n\n| a | longer header |\n|---|---|\n| `x|y` | v |\n", 80);
        assert!(r.starts_with("    ls -l\n\n"), "{}", r);
        assert!(r.contains("  a    longer header\n"), "{}", r);
        assert!(r.contains("  x|y  v\n"), "{}", r);
        let r = plain_render("| | |\n|---|---|\n| k | v |\n", 80);
        assert_eq!(r.trim_end(), "  k  v");
    }

    #[test]
    fn escapes_and_odd_input() {
        assert_eq!(plain_render("a \\* b < c", 80), "a * b < c\n");
        assert_eq!(plain_render("unclosed `tick and *star", 80), "unclosed `tick and *star\n");
        assert_eq!(plain_render("", 80), "");
        // Every doc in the repository renders without panicking.
        let _ = render(include_str!("../../../docs/packages.md"), 72, true);
        let _ = render(include_str!("../../../README.md"), 40, true);
    }
}
