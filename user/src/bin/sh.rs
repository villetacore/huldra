//! /bin/sh: a small POSIX-flavoured shell.
//!
//! Supports pipelines (`|`), lists (`;`, `&&`, `||`), background jobs (`&`),
//! redirections (`<`, `>`, `>>`, `2>`, `2>&1`), quoting, `$VAR`/`$?`/`$$`
//! expansion, `#` comments, scripts (`sh file`, `sh -c cmd`) and builtins.
//! Each pipeline runs in its own process group, which becomes the terminal's
//! foreground group, so ^C stops the job and not the shell.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use huldra_user::abi::fs::*;
use huldra_user::io::{Reader, STDIN};
use huldra_user::process::{self, ExitStatus, Pid};
use huldra_user::{env, eprintln, format, fs, print, println, signal, sys, Errno};

huldra_user::main!(main);

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Word(String),
    Pipe,
    And,
    Or,
    Semi,
    Amp,
    Less,
    Great,
    DGreat,
    ErrGreat,
    ErrToOut,
}

struct Shell {
    status: i32,
    /// $0, $1, ...
    params: Vec<String>,
    interactive: bool,
    jobs: Vec<Pid>,
    pgid: Pid,
}

/// Splits a line into tokens, expanding variables (not inside '...').
fn tokenize(line: &str, status: i32, params: &[String]) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut word = String::new();
    let mut in_word = false;

    let expand = |chars: &[char], i: &mut usize, out: &mut String| {
        // chars[*i] == '$'
        *i += 1;
        if *i >= chars.len() {
            out.push('$');
            return;
        }
        let c = chars[*i];
        if c == '?' {
            out.push_str(&status.to_string());
            *i += 1;
        } else if c == '$' {
            out.push_str(&process::getpid().to_string());
            *i += 1;
        } else if c == '#' {
            out.push_str(&params.len().saturating_sub(1).to_string());
            *i += 1;
        } else if c == '@' || c == '*' {
            out.push_str(&params.get(1..).unwrap_or(&[]).join(" "));
            *i += 1;
        } else if c.is_ascii_digit() {
            out.push_str(
                params
                    .get(c as usize - '0' as usize)
                    .map_or("", String::as_str),
            );
            *i += 1;
        } else if c == '{' {
            let start = *i + 1;
            let mut end = start;
            while end < chars.len() && chars[end] != '}' {
                end += 1;
            }
            let name: String = chars[start..end].iter().collect();
            out.push_str(env::var(&name).unwrap_or(""));
            *i = end + 1;
        } else if c.is_ascii_alphanumeric() || c == '_' {
            let start = *i;
            while *i < chars.len() && (chars[*i].is_ascii_alphanumeric() || chars[*i] == '_') {
                *i += 1;
            }
            let name: String = chars[start..*i].iter().collect();
            out.push_str(env::var(&name).unwrap_or(""));
        } else {
            out.push('$');
        }
    };

    macro_rules! flush {
        () => {
            #[allow(unused_assignments)]
            if in_word {
                tokens.push(Token::Word(core::mem::take(&mut word)));
                in_word = false;
            }
        };
    }

    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' => {
                flush!();
                i += 1;
            }
            '#' if !in_word => break,
            '\'' => {
                in_word = true;
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    word.push(chars[i]);
                    i += 1;
                }
                if i >= chars.len() {
                    return Err("unterminated quote".into());
                }
                i += 1;
            }
            '"' => {
                in_word = true;
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\'
                        && i + 1 < chars.len()
                        && matches!(chars[i + 1], '"' | '\\' | '$')
                    {
                        word.push(chars[i + 1]);
                        i += 2;
                    } else if chars[i] == '$' {
                        expand(&chars, &mut i, &mut word);
                    } else {
                        word.push(chars[i]);
                        i += 1;
                    }
                }
                if i >= chars.len() {
                    return Err("unterminated quote".into());
                }
                i += 1;
            }
            '\\' if i + 1 < chars.len() => {
                in_word = true;
                word.push(chars[i + 1]);
                i += 2;
            }
            '$' => {
                in_word = true;
                expand(&chars, &mut i, &mut word);
            }
            '|' | '&' | ';' | '<' | '>' => {
                if c == '>' && in_word && word == "2" {
                    word.clear();
                    in_word = false;
                    if chars.get(i + 1) == Some(&'&') && chars.get(i + 2) == Some(&'1') {
                        tokens.push(Token::ErrToOut);
                        i += 3;
                    } else {
                        tokens.push(Token::ErrGreat);
                        i += 1;
                    }
                    continue;
                }
                flush!();
                let next = chars.get(i + 1).copied();
                let (tok, len) = match (c, next) {
                    ('|', Some('|')) => (Token::Or, 2),
                    ('|', _) => (Token::Pipe, 1),
                    ('&', Some('&')) => (Token::And, 2),
                    ('&', _) => (Token::Amp, 1),
                    (';', _) => (Token::Semi, 1),
                    ('<', _) => (Token::Less, 1),
                    ('>', Some('>')) => (Token::DGreat, 2),
                    _ => (Token::Great, 1),
                };
                tokens.push(tok);
                i += len;
            }
            _ => {
                in_word = true;
                word.push(c);
                i += 1;
            }
        }
    }
    flush!();
    Ok(tokens)
}

#[derive(Default, Debug)]
struct Command {
    argv: Vec<String>,
    stdin: Option<String>,
    stdout: Option<(String, bool)>,
    stderr: Option<String>,
    err_to_out: bool,
}

#[derive(Debug)]
struct Pipeline {
    commands: Vec<Command>,
    background: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Connector {
    Always,
    IfOk,
    IfFailed,
}

fn parse(tokens: Vec<Token>) -> Result<Vec<(Connector, Pipeline)>, String> {
    let mut list = Vec::new();
    let mut connector = Connector::Always;
    let mut commands = Vec::new();
    let mut cmd = Command::default();
    let mut it = tokens.into_iter().peekable();

    fn target(
        it: &mut core::iter::Peekable<alloc::vec::IntoIter<Token>>,
    ) -> Result<String, String> {
        match it.next() {
            Some(Token::Word(w)) => Ok(w),
            _ => Err("syntax error: expected a file name".into()),
        }
    }

    loop {
        let tok = it.next();
        match tok {
            Some(Token::Word(w)) => cmd.argv.push(w),
            Some(Token::Less) => cmd.stdin = Some(target(&mut it)?),
            Some(Token::Great) => cmd.stdout = Some((target(&mut it)?, false)),
            Some(Token::DGreat) => cmd.stdout = Some((target(&mut it)?, true)),
            Some(Token::ErrGreat) => cmd.stderr = Some(target(&mut it)?),
            Some(Token::ErrToOut) => cmd.err_to_out = true,
            Some(Token::Pipe) => {
                if cmd.argv.is_empty() {
                    return Err("syntax error near '|'".into());
                }
                commands.push(core::mem::take(&mut cmd));
            }
            Some(Token::And) | Some(Token::Or) | Some(Token::Semi) | Some(Token::Amp) | None => {
                if !cmd.argv.is_empty() {
                    commands.push(core::mem::take(&mut cmd));
                } else if !commands.is_empty() {
                    return Err("syntax error: missing command".into());
                }
                if !commands.is_empty() {
                    let background = tok == Some(Token::Amp);
                    list.push((
                        connector,
                        Pipeline {
                            commands: core::mem::take(&mut commands),
                            background,
                        },
                    ));
                } else if matches!(tok, Some(Token::And) | Some(Token::Or)) {
                    return Err("syntax error: missing command".into());
                }
                connector = match tok {
                    Some(Token::And) => Connector::IfOk,
                    Some(Token::Or) => Connector::IfFailed,
                    _ => Connector::Always,
                };
                if tok.is_none() {
                    break;
                }
            }
        }
    }
    Ok(list)
}

const HELP: &str = "\
Huldra shell builtins:
  cd [dir]          change directory          exit [code]       leave the shell
  pwd               print working directory   export K=V        set a variable
  unset K           remove a variable         set               list variables
  jobs              list background jobs      help              this help
Syntax: a | b, a ; b, a && b, a || b, a &, > file, >> file, < file, 2> file, 2>&1, $VAR, $?
Programs live in /bin and /sbin: ls /bin";

impl Shell {
    fn builtin(&mut self, argv: &[String]) -> Option<i32> {
        let arg = |i: usize| argv.get(i).map(String::as_str);
        Some(match argv[0].as_str() {
            "cd" => {
                let dir = arg(1)
                    .map(String::from)
                    .unwrap_or_else(|| String::from(env::var("HOME").unwrap_or("/")));
                match fs::set_current_dir(&dir) {
                    Ok(()) => {
                        env::set_var("PWD", &fs::current_dir().unwrap_or_default());
                        0
                    }
                    Err(e) => {
                        eprintln!("cd: {}: {}", dir, e);
                        1
                    }
                }
            }
            "pwd" => {
                println!("{}", fs::current_dir().unwrap_or_default());
                0
            }
            "exit" => process::exit(arg(1).and_then(|c| c.parse().ok()).unwrap_or(self.status)),
            "export" | "set" if argv.len() == 1 => {
                for (k, v) in env::vars() {
                    println!("{}={}", k, v);
                }
                0
            }
            "export" => {
                for a in &argv[1..] {
                    if let Some((k, v)) = a.split_once('=') {
                        env::set_var(k, v);
                    }
                }
                0
            }
            "unset" => {
                for a in &argv[1..] {
                    env::remove_var(a);
                }
                0
            }
            "jobs" => {
                for pid in &self.jobs {
                    println!("[{}] running", pid);
                }
                0
            }
            "help" => {
                println!("{}", HELP);
                0
            }
            _ if argv[0].contains('=') && !argv[0].starts_with('=') && argv.len() == 1 => {
                let (k, v) = argv[0].split_once('=').unwrap();
                env::set_var(k, v);
                0
            }
            _ => return None,
        })
    }

    fn reap_jobs(&mut self) {
        while let Ok(Some((pid, status))) = process::try_wait(-1) {
            if self.jobs.contains(&pid) {
                self.jobs.retain(|&p| p != pid);
                println!("[{}] done ({})", pid, status.code());
            }
        }
    }

    /// Sets up redirections in a child process.
    fn redirect(cmd: &Command) -> Result<(), String> {
        let open = |path: &str, flags: u32| {
            sys::open(path, flags, 0o644).map_err(|e| format!("{}: {}", path, e))
        };
        if let Some(p) = &cmd.stdin {
            let fd = open(p, O_RDONLY)?;
            let _ = sys::dup2(fd, 0);
            let _ = sys::close(fd);
        }
        if let Some((p, append)) = &cmd.stdout {
            let flags = O_WRONLY | O_CREAT | if *append { O_APPEND } else { O_TRUNC };
            let fd = open(p, flags)?;
            let _ = sys::dup2(fd, 1);
            let _ = sys::close(fd);
        }
        if let Some(p) = &cmd.stderr {
            let fd = open(p, O_WRONLY | O_CREAT | O_TRUNC)?;
            let _ = sys::dup2(fd, 2);
            let _ = sys::close(fd);
        }
        if cmd.err_to_out {
            let _ = sys::dup2(1, 2);
        }
        Ok(())
    }

    fn run_pipeline(&mut self, p: &Pipeline) -> i32 {
        // A lone builtin runs inside the shell (so `cd` works).
        if p.commands.len() == 1 && !p.background {
            let c = &p.commands[0];
            if c.stdin.is_none() && c.stdout.is_none() && c.stderr.is_none() {
                if let Some(code) = self.builtin(&c.argv) {
                    return code;
                }
            }
        }

        let mut pids: Vec<Pid> = Vec::new();
        let mut pgid: Pid = 0;
        let mut prev_read: Option<i32> = None;
        let n = p.commands.len();
        for (i, cmd) in p.commands.iter().enumerate() {
            let pipe = if i + 1 < n {
                match sys::pipe() {
                    Ok(fds) => Some(fds),
                    Err(e) => {
                        eprintln!("sh: pipe: {}", e);
                        break;
                    }
                }
            } else {
                None
            };
            match process::fork() {
                Ok(None) => {
                    let me = process::getpid();
                    let group = if pgid == 0 { me } else { pgid };
                    let _ = sys::setpgid(0, group);
                    if self.interactive && !p.background {
                        let _ = process::set_foreground(0, group);
                    }
                    let _ = signal::default(signal::SIGINT);
                    let _ = signal::default(signal::SIGQUIT);
                    if let Some(fd) = prev_read {
                        let _ = sys::dup2(fd, 0);
                        let _ = sys::close(fd);
                    }
                    if let Some((r, w)) = pipe {
                        let _ = sys::close(r);
                        let _ = sys::dup2(w, 1);
                        let _ = sys::close(w);
                    }
                    if let Err(e) = Self::redirect(cmd) {
                        eprintln!("sh: {}", e);
                        process::exit(1);
                    }
                    if let Some(code) = self.builtin(&cmd.argv) {
                        process::exit(code);
                    }
                    let Some(path) = process::find_in_path(&cmd.argv[0]) else {
                        eprintln!("sh: {}: command not found", cmd.argv[0]);
                        process::exit(127)
                    };
                    let args: Vec<&str> = cmd.argv.iter().map(String::as_str).collect();
                    let e = process::exec(&path, &args, &env::envp());
                    eprintln!("sh: {}: {}", cmd.argv[0], e);
                    process::exit(if e == Errno::ENOENT { 127 } else { 126 })
                }
                Ok(Some(pid)) => {
                    if pgid == 0 {
                        pgid = pid;
                    }
                    let _ = sys::setpgid(pid, pgid);
                    pids.push(pid);
                }
                Err(e) => {
                    eprintln!("sh: fork: {}", e);
                    break;
                }
            }
            if let Some(fd) = prev_read.take() {
                let _ = sys::close(fd);
            }
            if let Some((r, w)) = pipe {
                let _ = sys::close(w);
                prev_read = Some(r);
            }
        }
        if let Some(fd) = prev_read {
            let _ = sys::close(fd);
        }

        if p.background {
            if let Some(&last) = pids.last() {
                println!("[{}]", last);
                self.jobs.push(last);
            }
            return 0;
        }

        if self.interactive && pgid != 0 {
            let _ = process::set_foreground(0, pgid);
        }
        let mut last_status = ExitStatus::Exited(127);
        for (i, pid) in pids.iter().enumerate() {
            if let Ok((_, status)) = process::wait(*pid as i32) {
                if i + 1 == pids.len() {
                    last_status = status;
                }
            }
        }
        if self.interactive {
            let _ = process::set_foreground(0, self.pgid);
        }
        if let ExitStatus::Signaled(sig) = last_status {
            if sig != signal::SIGINT && sig != signal::SIGPIPE {
                eprintln!("{}", signal::name(sig));
            }
        }
        last_status.code()
    }

    fn run_line(&mut self, line: &str) {
        let list = match tokenize(line, self.status, &self.params).and_then(parse) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("sh: {}", e);
                self.status = 2;
                return;
            }
        };
        for (connector, pipeline) in list {
            let run = match connector {
                Connector::Always => true,
                Connector::IfOk => self.status == 0,
                Connector::IfFailed => self.status != 0,
            };
            if run {
                self.status = self.run_pipeline(&pipeline);
            }
        }
    }

    fn prompt(&self) {
        let host = fs::read_to_string("/etc/hostname").unwrap_or_default();
        let cwd = fs::current_dir().unwrap_or_default();
        let home = env::var("HOME").unwrap_or("/root");
        let shown = if cwd == home {
            String::from("~")
        } else if let Some(rest) = cwd.strip_prefix(home).filter(|r| r.starts_with('/')) {
            format!("~{}", rest)
        } else {
            cwd
        };
        print!(
            "\x1b[1;32mroot@{}\x1b[0m:\x1b[1;34m{}\x1b[0m# ",
            host.trim(),
            shown
        );
        huldra_user::io::flush_stdout();
    }
}

extern "C" fn on_interrupt(_sig: i32) {}

fn main() -> i32 {
    let args = env::args();
    let mut sh = Shell {
        status: 0,
        params: Vec::from([args[0].clone()]),
        interactive: false,
        jobs: Vec::new(),
        pgid: process::getpid(),
    };

    if args.len() >= 3 && args[1] == "-c" {
        sh.params = args[2..].to_vec();
        sh.run_line(&args[2]);
        return sh.status;
    }

    let script_fd = if args.len() >= 2 && !args[1].starts_with('-') {
        sh.params = args[1..].to_vec();
        match sys::open(&args[1], O_RDONLY | O_CLOEXEC, 0) {
            Ok(fd) => Some(fd),
            Err(e) => {
                eprintln!("sh: {}: {}", args[1], e);
                return 127;
            }
        }
    } else {
        None
    };

    if script_fd.is_none() {
        sh.interactive = true;
        sh.pgid = sys::getpgrp();
        // ^C at the prompt just interrupts the read.
        let _ = signal::handle_interrupting(signal::SIGINT, on_interrupt);
        let _ = signal::ignore(signal::SIGQUIT);
        if args.first().is_some_and(|a| a.starts_with('-')) {
            if let Ok(motd) = fs::read_to_string("/etc/motd") {
                print!("{}", motd);
            }
        }
    }

    let mut input = Reader::new(script_fd.unwrap_or(STDIN));
    loop {
        if sh.interactive {
            sh.reap_jobs();
            sh.prompt();
        }
        match input.read_line() {
            Ok(Some(line)) => sh.run_line(&line),
            Ok(None) => {
                if sh.interactive {
                    println!("exit");
                }
                return sh.status;
            }
            Err(Errno::EINTR) => input.reset(),
            Err(e) => {
                eprintln!("sh: read: {}", e);
                return 1;
            }
        }
    }
}
