//! /bin/sh: a POSIX-flavoured shell.
//!
//! Pipelines, `;` `&&` `||`, background jobs, redirections, quoting,
//! parameter and command substitution (`$VAR`, `$(cmd)`), globbing,
//! `if`/`while`/`until`/`for`, functions, `{ }` and `( )` groups, and an
//! interactive line editor with history and tab completion.

#![no_std]
#![no_main]

extern crate alloc;

mod arith;
mod exec;
mod expand;
mod lexer;
mod parser;
mod readline;
mod test;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use exec::Shell;
use huldra_user::{env, eprintln, fs, io, print, process, signal, sys, term};

huldra_user::main!(main);

pub const HELP: &str = "\
Huldra shell
  Syntax:   a | b    a ; b    a && b    a || b    a &    ( list )    { list; }
            > file   >> file   < file   2> file   2>&1   $VAR  ${VAR}  $?  $1  $(cmd)  *.txt
            if c; then ...; elif c; then ...; else ...; fi
            while c; do ...; done     until c; do ...; done
            for x in a b c; do ...; done      name() { ...; }
  Builtins: cd pwd echo export unset set shift read test [ source . eval exec
            exit return break continue jobs wait type history help true false
  Keys:     arrows/Home/End edit the line, Up/Down history, Tab completes,
            Ctrl+A/E/U/K/W/L, Ctrl+C cancels, Ctrl+D exits
Programs are in /bin and /sbin (ls /bin).";

extern "C" fn on_interrupt(_sig: i32) {}

fn prompt() -> String {
    let host = fs::read_to_string("/etc/hostname").unwrap_or_default();
    let cwd = fs::current_dir().unwrap_or_default();
    let home = env::var("HOME").unwrap_or("/root");
    let shown = if cwd == home {
        String::from("~")
    } else if let Some(rest) = cwd.strip_prefix(home).filter(|r| r.starts_with('/')) {
        alloc::format!("~{}", rest)
    } else {
        cwd
    };
    alloc::format!("\x1b[1;32mroot@{}\x1b[0m:\x1b[1;34m{}\x1b[0m# ", host.trim(), shown)
}

/// Tab completion: commands for the first word, file names otherwise.
fn complete(sh: &Shell, line: &str, cursor: usize) -> (usize, Vec<String>) {
    let before = &line[..cursor];
    let start = before.rfind([' ', '\t', '|', ';', '&', '<', '>', '(']).map_or(0, |i| i + 1);
    let word = &before[start..];
    let prefix_text = before[..start].trim_end();
    let first_word = prefix_text.is_empty() || prefix_text.ends_with(['|', ';', '&', '(']);
    let mut out: Vec<String> = Vec::new();
    if first_word && !word.contains('/') {
        for b in exec::BUILTINS {
            if b.starts_with(word) {
                out.push(String::from(*b));
            }
        }
        for f in sh.functions.keys() {
            if f.starts_with(word) {
                out.push(f.clone());
            }
        }
        for dir in env::var("PATH").unwrap_or("/bin:/sbin:/usr/bin:/usr/local/bin").split(':') {
            for e in fs::read_dir(dir).unwrap_or_default() {
                if e.name.starts_with(word) {
                    out.push(e.name);
                }
            }
        }
    } else {
        let (dir, file_prefix) = match word.rfind('/') {
            Some(i) => (&word[..=i], &word[i + 1..]),
            None => ("", word),
        };
        let listed = if dir.is_empty() { "." } else { dir };
        let home_dir = if listed.starts_with('~') {
            listed.replacen('~', env::var("HOME").unwrap_or("/root"), 1)
        } else {
            String::from(listed)
        };
        for e in fs::read_dir(&home_dir).unwrap_or_default() {
            if e.name.starts_with(file_prefix) && (file_prefix.starts_with('.') || !e.name.starts_with('.')) {
                let mut s = alloc::format!("{}{}", dir, e.name);
                if e.is_dir() {
                    s.push('/');
                }
                out.push(s);
            }
        }
    }
    out.sort();
    out.dedup();
    (start, out)
}

fn interactive(sh: &mut Shell) -> i32 {
    sh.interactive = true;
    sh.pgid = sys::getpgrp();
    let _ = signal::handle_interrupting(signal::SIGINT, on_interrupt);
    let _ = signal::ignore(signal::SIGQUIT);
    let _ = signal::ignore(huldra_user::abi::signal::SIGTTOU);
    sh.history = readline::load_history();
    let mut pending = String::new();
    loop {
        sh.reap_jobs();
        let p = if pending.is_empty() { prompt() } else { String::from("> ") };
        let mut history = core::mem::take(&mut sh.history);
        let result = readline::read_line(&p, &mut history, &|line, cur| complete(sh, line, cur));
        sh.history = history;
        let line = match result {
            readline::ReadResult::Line(l) => l,
            readline::ReadResult::Interrupted => {
                pending.clear();
                sh.status = 130;
                continue;
            }
            readline::ReadResult::Eof => {
                if !pending.is_empty() {
                    pending.clear();
                    continue;
                }
                print!("exit\n");
                readline::save_history(&sh.history);
                return sh.status;
            }
        };
        pending.push_str(&line);
        pending.push('\n');
        // Ask for more input while a quote or compound command is open.
        let complete_input = match lexer::tokenize(&pending) {
            Err(lexer::LexError::Incomplete) => false,
            Ok(tokens) => !matches!(parser::parse(tokens), Err(parser::ParseError::Incomplete)),
            Err(_) => true,
        };
        if !complete_input {
            continue;
        }
        let src = core::mem::take(&mut pending);
        sh.run_source(&src);
        io::flush_stdout();
        if sh.history.len() % 10 == 0 {
            readline::save_history(&sh.history);
        }
    }
}

fn main() -> i32 {
    let args = env::args();
    let name = args.first().cloned().unwrap_or_else(|| String::from("sh"));
    let login = name.starts_with('-');

    if args.len() >= 3 && args[1] == "-c" {
        // `sh -c [--] command [name [args...]]`
        let start = if args[2] == "--" { 3 } else { 2 };
        let Some(command) = args.get(start) else { return 0 };
        let mut params: Vec<String> = alloc::vec![args.get(start + 1).cloned().unwrap_or_else(|| name.clone())];
        params.extend(args.iter().skip(start + 2).cloned());
        let mut sh = Shell::new(params);
        return sh.run_source(command);
    }

    if args.len() >= 2 && !args[1].starts_with('-') {
        let mut sh = Shell::new(args[1..].to_vec());
        return match fs::read_to_string(&args[1]) {
            Ok(src) => {
                // Skip a #! line.
                let body = if src.starts_with("#!") { src.split_once('\n').map_or("", |(_, b)| b) } else { &src };
                sh.run_source(body)
            }
            Err(e) => {
                eprintln!("sh: {}: {}", args[1], e);
                127
            }
        };
    }

    let mut sh = Shell::new(alloc::vec![name.clone()]);
    if !term::is_tty(0) {
        let input = io::Reader::new(0).read_to_end().unwrap_or_default();
        return sh.run_source(&String::from_utf8_lossy(&input));
    }
    if login {
        for profile in ["/etc/profile".to_string(), fs::join(env::var("HOME").unwrap_or("/root"), ".profile")] {
            if let Ok(src) = fs::read_to_string(&profile) {
                sh.run_source(&src);
            }
        }
        if let Ok(motd) = fs::read_to_string("/etc/motd") {
            print!("{}", motd);
        }
    }
    if let Ok(src) = fs::read_to_string(&fs::join(env::var("HOME").unwrap_or("/root"), ".shrc")) {
        sh.run_source(&src);
    }
    let _ = process::getpid();
    interactive(&mut sh)
}
