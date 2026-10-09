//! Execution of the syntax tree.

use crate::parser::{AndOr, Command, List, Node, ParseError, Pipeline, RedirKind, Redir};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use huldra_user::abi::fs::*;
use huldra_user::process::{self, ExitStatus, Pid};
use huldra_user::{env, eprintln, format, fs, io, signal, sys, Errno};

/// Non-local control flow out of a command.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flow {
    Normal,
    Break(usize),
    Continue(usize),
    Return,
}

pub struct Job {
    pub pid: Pid,
}

pub struct Shell {
    /// `$0`, `$1`, ...
    pub params: Vec<String>,
    pub status: i32,
    pub functions: BTreeMap<String, Node>,
    pub interactive: bool,
    /// Process group of the shell itself (interactive mode).
    pub pgid: Pid,
    pub jobs: Vec<Job>,
    pub last_background: Pid,
    /// True inside a forked child (no job control there).
    pub subshell: bool,
    loop_depth: usize,
    pub history: Vec<String>,
}

pub const BUILTINS: &[&str] = &[
    ".", ":", "[", "break", "cd", "continue", "echo", "eval", "exec", "exit", "export", "false", "history",
    "jobs", "pwd", "read", "return", "set", "shift", "source", "test", "true", "type", "unset", "wait",
];

impl Shell {
    pub fn new(params: Vec<String>) -> Shell {
        Shell {
            params,
            status: 0,
            functions: BTreeMap::new(),
            interactive: false,
            pgid: process::getpid(),
            jobs: Vec::new(),
            last_background: 0,
            subshell: false,
            loop_depth: 0,
            history: Vec::new(),
        }
    }

    pub fn get_var(&self, name: &str) -> Option<String> {
        env::var(name).map(String::from)
    }

    /// Parses and runs shell source. Returns the exit status.
    pub fn run_source(&mut self, src: &str) -> i32 {
        let tokens = match crate::lexer::tokenize(src) {
            Ok(t) => t,
            Err(crate::lexer::LexError::Incomplete) => {
                eprintln!("sh: syntax error: unexpected end of input");
                self.status = 2;
                return 2;
            }
            Err(crate::lexer::LexError::Bad(e)) => {
                eprintln!("sh: {}", e);
                self.status = 2;
                return 2;
            }
        };
        match crate::parser::parse(tokens) {
            Ok(list) => {
                self.run_list(&list);
            }
            Err(ParseError::Incomplete) => {
                eprintln!("sh: syntax error: unexpected end of input");
                self.status = 2;
            }
            Err(ParseError::Syntax(e)) => {
                eprintln!("sh: {}", e);
                self.status = 2;
            }
        }
        self.status
    }

    pub fn run_list(&mut self, list: &List) -> Flow {
        for (and_or, background) in &list.items {
            let flow = if *background { self.run_background(and_or) } else { self.run_and_or(and_or) };
            if flow != Flow::Normal {
                return flow;
            }
        }
        Flow::Normal
    }

    fn run_and_or(&mut self, ao: &AndOr) -> Flow {
        let flow = self.run_pipeline(&ao.first);
        if flow != Flow::Normal {
            return flow;
        }
        for (and, p) in &ao.rest {
            if (*and && self.status != 0) || (!*and && self.status == 0) {
                continue;
            }
            let flow = self.run_pipeline(p);
            if flow != Flow::Normal {
                return flow;
            }
        }
        Flow::Normal
    }

    fn run_background(&mut self, ao: &AndOr) -> Flow {
        match process::fork() {
            Ok(None) => {
                let _ = sys::setpgid(0, 0);
                self.subshell = true;
                self.interactive = false;
                let _ = signal::default(signal::SIGINT);
                self.run_and_or(ao);
                process::exit(self.status)
            }
            Ok(Some(pid)) => {
                let _ = sys::setpgid(pid, pid);
                self.last_background = pid;
                if self.interactive {
                    eprintln!("[{}] {}", self.jobs.len() + 1, pid);
                }
                self.jobs.push(Job { pid });
                self.status = 0;
            }
            Err(e) => {
                eprintln!("sh: fork: {}", e);
                self.status = 1;
            }
        }
        Flow::Normal
    }

    fn job_control(&self) -> bool {
        self.interactive && !self.subshell
    }

    fn run_pipeline(&mut self, p: &Pipeline) -> Flow {
        let flow = if p.nodes.len() == 1 {
            self.run_node(&p.nodes[0], false)
        } else {
            self.run_multi(p);
            Flow::Normal
        };
        if p.negate {
            self.status = (self.status == 0) as i32;
        }
        flow
    }

    /// Runs a pipeline of several commands, each in its own process.
    fn run_multi(&mut self, p: &Pipeline) {
        let mut pids: Vec<Pid> = Vec::new();
        let mut pgid: Pid = 0;
        let mut prev_read: Option<i32> = None;
        let n = p.nodes.len();
        for (i, node) in p.nodes.iter().enumerate() {
            let pipe = if i + 1 < n { sys::pipe().ok() } else { None };
            match process::fork() {
                Ok(None) => {
                    let me = process::getpid();
                    if self.job_control() {
                        let group = if pgid == 0 { me } else { pgid };
                        let _ = sys::setpgid(0, group);
                        let _ = process::set_foreground(0, group);
                    }
                    self.subshell = true;
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
                    self.run_node(node, true);
                    process::exit(self.status)
                }
                Ok(Some(pid)) => {
                    if pgid == 0 {
                        pgid = pid;
                    }
                    if self.job_control() {
                        let _ = sys::setpgid(pid, pgid);
                    }
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
        self.wait_foreground(pgid, &pids);
    }

    /// Waits for a foreground job; the last pid's status becomes `$?`.
    fn wait_foreground(&mut self, pgid: Pid, pids: &[Pid]) {
        if self.job_control() && pgid != 0 {
            let _ = process::set_foreground(0, pgid);
        }
        let mut last = ExitStatus::Exited(0);
        for (i, pid) in pids.iter().enumerate() {
            if let Ok((_, st)) = process::wait(*pid as i32) {
                if i + 1 == pids.len() {
                    last = st;
                }
            }
        }
        if self.job_control() {
            let _ = process::set_foreground(0, self.pgid);
        }
        if let ExitStatus::Signaled(sig) = last {
            if sig == signal::SIGINT {
                if self.interactive {
                    eprintln!();
                }
            } else if sig != signal::SIGPIPE {
                eprintln!("{}", signal::name(sig));
            }
        }
        self.status = last.code();
    }

    /// Opens the files of `redirs` and installs them on fds 0/1/2. When
    /// `save` is set, returns the original descriptors for [`restore`].
    fn apply_redirs(&mut self, redirs: &[Redir], save: bool) -> Result<Vec<(i32, i32)>, String> {
        let mut saved: Vec<(i32, i32)> = Vec::new();
        for r in redirs {
            let (target_fd, source) = match r.kind {
                RedirKind::ErrToOut => (2, Err(1)),
                RedirKind::OutToErr => (1, Err(2)),
                _ => {
                    let path = self.expand_single(&r.target);
                    let (fd, flags) = match r.kind {
                        RedirKind::In => (0, O_RDONLY),
                        RedirKind::Out => (1, O_WRONLY | O_CREAT | O_TRUNC),
                        RedirKind::Append => (1, O_WRONLY | O_CREAT | O_APPEND),
                        RedirKind::Err => (2, O_WRONLY | O_CREAT | O_TRUNC),
                        _ => (2, O_WRONLY | O_CREAT | O_APPEND),
                    };
                    match sys::open(&path, flags, 0o644) {
                        Ok(f) => (fd, Ok(f)),
                        Err(e) => {
                            restore(&saved);
                            return Err(format!("{}: {}", path, e));
                        }
                    }
                }
            };
            if save && !saved.iter().any(|&(t, _)| t == target_fd) {
                if let Ok(copy) = sys::fcntl(target_fd, F_DUPFD_CLOEXEC, 10) {
                    saved.push((target_fd, copy as i32));
                }
            }
            match source {
                Ok(f) => {
                    let _ = sys::dup2(f, target_fd);
                    let _ = sys::close(f);
                }
                Err(from) => {
                    let _ = sys::dup2(from, target_fd);
                }
            }
        }
        Ok(saved)
    }

    /// Runs one command. With `in_child` the current process may be
    /// replaced (`exec`) instead of forking.
    pub fn run_node(&mut self, node: &Node, in_child: bool) -> Flow {
        match &node.cmd {
            Command::Simple { assigns, words } => return self.run_simple(assigns, words, &node.redirs, in_child),
            Command::Function { name, body } => {
                self.functions.insert(name.clone(), (**body).clone());
                self.status = 0;
                return Flow::Normal;
            }
            _ => {}
        }
        let saved = match self.apply_redirs(&node.redirs, !in_child) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("sh: {}", e);
                self.status = 1;
                return Flow::Normal;
            }
        };
        let flow = match &node.cmd {
            Command::If { branches, otherwise } => self.run_if(branches, otherwise),
            Command::While { cond, body, until } => self.run_while(cond, body, *until),
            Command::For { var, items, body } => self.run_for(var, items, body),
            Command::Group(list) => self.run_list(list),
            Command::Subshell(list) => {
                match process::fork() {
                    Ok(None) => {
                        self.subshell = true;
                        self.run_list(list);
                        process::exit(self.status)
                    }
                    Ok(Some(pid)) => {
                        let st = process::wait(pid as i32).map_or(ExitStatus::Exited(1), |r| r.1);
                        self.status = st.code();
                    }
                    Err(e) => {
                        eprintln!("sh: fork: {}", e);
                        self.status = 1;
                    }
                }
                Flow::Normal
            }
            Command::Simple { .. } | Command::Function { .. } => unreachable!(),
        };
        restore(&saved);
        flow
    }

    fn run_if(&mut self, branches: &[(List, List)], otherwise: &Option<List>) -> Flow {
        for (cond, body) in branches {
            let f = self.run_list(cond);
            if f != Flow::Normal {
                return f;
            }
            if self.status == 0 {
                return self.run_list(body);
            }
        }
        match otherwise {
            Some(list) => self.run_list(list),
            None => {
                self.status = 0;
                Flow::Normal
            }
        }
    }

    /// Handles break/continue inside a loop body; returns Some(flow) to stop.
    fn loop_flow(&mut self, f: Flow) -> Option<Flow> {
        match f {
            Flow::Normal | Flow::Continue(1) => None,
            Flow::Break(1) => Some(Flow::Normal),
            Flow::Break(n) => Some(Flow::Break(n - 1)),
            Flow::Continue(n) => Some(Flow::Continue(n - 1)),
            Flow::Return => Some(Flow::Return),
        }
    }

    fn run_while(&mut self, cond: &List, body: &List, until: bool) -> Flow {
        self.loop_depth += 1;
        let mut last = 0;
        let result = loop {
            let f = self.run_list(cond);
            if f != Flow::Normal {
                break f;
            }
            if (self.status == 0) == until {
                break Flow::Normal;
            }
            let f = self.run_list(body);
            last = self.status;
            if let Some(stop) = self.loop_flow(f) {
                break stop;
            }
        };
        self.loop_depth -= 1;
        self.status = last;
        result
    }

    fn run_for(&mut self, var: &str, items: &Option<Vec<crate::lexer::Word>>, body: &List) -> Flow {
        let values = match items {
            Some(words) => self.expand_words(words),
            None => self.params.get(1..).unwrap_or(&[]).to_vec(),
        };
        self.loop_depth += 1;
        self.status = 0;
        let mut result = Flow::Normal;
        for v in values {
            env::set_var(var, &v);
            let f = self.run_list(body);
            if let Some(stop) = self.loop_flow(f) {
                result = stop;
                break;
            }
        }
        self.loop_depth -= 1;
        result
    }

    fn run_simple(
        &mut self,
        assigns: &[(String, crate::lexer::Word)],
        words: &[crate::lexer::Word],
        redirs: &[Redir],
        in_child: bool,
    ) -> Flow {
        let argv = self.expand_words(words);
        let values: Vec<(String, String)> =
            assigns.iter().map(|(k, w)| (k.clone(), self.expand_single(w))).collect();

        if argv.is_empty() {
            for (k, v) in &values {
                env::set_var(k, v);
            }
            match self.apply_redirs(redirs, true) {
                Ok(saved) => {
                    restore(&saved);
                    self.status = 0;
                }
                Err(e) => {
                    eprintln!("sh: {}", e);
                    self.status = 1;
                }
            }
            return Flow::Normal;
        }

        if let Some(body) = self.functions.get(&argv[0]).cloned() {
            let saved = match self.apply_redirs(redirs, !in_child) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("sh: {}", e);
                    self.status = 1;
                    return Flow::Normal;
                }
            };
            let mut params = alloc::vec![self.params[0].clone()];
            params.extend(argv[1..].iter().cloned());
            let old = core::mem::replace(&mut self.params, params);
            let flow = self.run_node(&body, false);
            self.params = old;
            restore(&saved);
            return if flow == Flow::Return { Flow::Normal } else { flow };
        }

        if BUILTINS.contains(&argv[0].as_str()) {
            for (k, v) in &values {
                env::set_var(k, v);
            }
            let saved = match self.apply_redirs(redirs, !in_child) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("sh: {}", e);
                    self.status = 1;
                    return Flow::Normal;
                }
            };
            let flow = self.builtin(&argv);
            io::flush_stdout();
            restore(&saved);
            return flow;
        }

        if in_child {
            self.exec_external(&argv, &values, redirs);
        }
        match process::fork() {
            Ok(None) => {
                if self.job_control() {
                    let _ = sys::setpgid(0, 0);
                    let _ = process::set_foreground(0, process::getpid());
                }
                let _ = signal::default(signal::SIGINT);
                let _ = signal::default(signal::SIGQUIT);
                self.exec_external(&argv, &values, redirs)
            }
            Ok(Some(pid)) => {
                if self.job_control() {
                    let _ = sys::setpgid(pid, pid);
                }
                self.wait_foreground(pid, &[pid]);
            }
            Err(e) => {
                eprintln!("sh: fork: {}", e);
                self.status = 1;
            }
        }
        Flow::Normal
    }

    /// Replaces the current process with an external program.
    fn exec_external(&mut self, argv: &[String], values: &[(String, String)], redirs: &[Redir]) -> ! {
        if let Err(e) = self.apply_redirs(redirs, false) {
            eprintln!("sh: {}", e);
            process::exit(1);
        }
        for (k, v) in values {
            env::set_var(k, v);
        }
        let Some(path) = process::find_in_path(&argv[0]) else {
            eprintln!("sh: {}: command not found", argv[0]);
            process::exit(127)
        };
        let args: Vec<&str> = argv.iter().map(String::as_str).collect();
        let e = process::exec(&path, &args, &env::envp());
        eprintln!("sh: {}: {}", argv[0], e);
        process::exit(if e == Errno::ENOENT { 127 } else { 126 })
    }

    /// Runs `source` in a child and returns what it printed.
    pub fn command_substitution(&mut self, source: &str) -> String {
        let Ok((r, w)) = sys::pipe() else { return String::new() };
        match process::fork() {
            Ok(None) => {
                let _ = sys::close(r);
                let _ = sys::dup2(w, 1);
                let _ = sys::close(w);
                self.subshell = true;
                self.interactive = false;
                let status = self.run_source(source);
                process::exit(status)
            }
            Ok(Some(pid)) => {
                let _ = sys::close(w);
                let out = io::Reader::new(r).read_to_end().unwrap_or_default();
                let _ = sys::close(r);
                if let Ok((_, st)) = process::wait(pid as i32) {
                    self.status = st.code();
                }
                String::from_utf8_lossy(&out).into_owned()
            }
            Err(_) => {
                let _ = sys::close(r);
                let _ = sys::close(w);
                String::new()
            }
        }
    }

    pub fn reap_jobs(&mut self) {
        while let Ok(Some((pid, status))) = process::try_wait(-1) {
            if let Some(i) = self.jobs.iter().position(|j| j.pid == pid) {
                self.jobs.remove(i);
                if self.interactive {
                    eprintln!("[{}] done ({})", pid, status.code());
                }
            }
        }
    }

    fn builtin(&mut self, argv: &[String]) -> Flow {
        let arg = |i: usize| argv.get(i).map(String::as_str);
        let mut flow = Flow::Normal;
        self.status = match argv[0].as_str() {
            ":" | "true" => 0,
            "false" => 1,
            "cd" => {
                let dir = match arg(1) {
                    Some("-") => self.get_var("OLDPWD").unwrap_or_else(|| "/".into()),
                    Some(d) => d.to_string(),
                    None => self.get_var("HOME").unwrap_or_else(|| "/".into()),
                };
                let old = fs::current_dir().unwrap_or_default();
                match fs::set_current_dir(&dir) {
                    Ok(()) => {
                        env::set_var("OLDPWD", &old);
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
                huldra_user::println!("{}", fs::current_dir().unwrap_or_default());
                0
            }
            "echo" => builtin_echo(&argv[1..]),
            "exit" => {
                crate::readline::save_history(&self.history);
                process::exit(arg(1).and_then(|c| c.parse().ok()).unwrap_or(self.status))
            }
            "export" | "set" if argv.len() == 1 => {
                let mut vars: Vec<_> = env::vars().to_vec();
                vars.sort();
                for (k, v) in vars {
                    huldra_user::println!("{}={}", k, v);
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
            "set" => {
                // `set -- a b c` replaces the positional parameters.
                let start = if arg(1) == Some("--") { 2 } else { 1 };
                let mut params = alloc::vec![self.params[0].clone()];
                params.extend(argv[start..].iter().cloned());
                self.params = params;
                0
            }
            "unset" => {
                for a in &argv[1..] {
                    env::remove_var(a);
                    self.functions.remove(a);
                }
                0
            }
            "shift" => {
                let n = arg(1).and_then(|s| s.parse().ok()).unwrap_or(1usize);
                if n < self.params.len() {
                    self.params.drain(1..=n);
                    0
                } else {
                    1
                }
            }
            "break" | "continue" if self.loop_depth > 0 => {
                let n = arg(1).and_then(|s| s.parse().ok()).unwrap_or(1usize).clamp(1, self.loop_depth);
                flow = if argv[0] == "break" { Flow::Break(n) } else { Flow::Continue(n) };
                0
            }
            "break" | "continue" => 0,
            "return" => {
                flow = Flow::Return;
                arg(1).and_then(|s| s.parse().ok()).unwrap_or(self.status)
            }
            "test" | "[" => {
                let mut args: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
                if argv[0] == "[" {
                    if args.last() != Some(&"]") {
                        eprintln!("[: missing ']'");
                        self.status = 2;
                        return Flow::Normal;
                    }
                    args.pop();
                }
                match crate::test::evaluate(&args) {
                    Ok(true) => 0,
                    Ok(false) => 1,
                    Err(e) => {
                        eprintln!("test: {}", e);
                        2
                    }
                }
            }
            "read" => self.builtin_read(&argv[1..]),
            "source" | "." => match arg(1).map(fs::read_to_string) {
                Some(Ok(src)) => {
                    let old = core::mem::replace(&mut self.params[0], argv[1].clone());
                    let st = self.run_source(&src);
                    self.params[0] = old;
                    st
                }
                Some(Err(e)) => {
                    eprintln!("source: {}: {}", argv[1], e);
                    1
                }
                None => 2,
            },
            "eval" => {
                let src = argv[1..].join(" ");
                self.run_source(&src)
            }
            "exec" if argv.len() > 1 => self.exec_external(&argv[1..], &[], &[]),
            "exec" => 0,
            "jobs" => {
                for (i, j) in self.jobs.iter().enumerate() {
                    huldra_user::println!("[{}] {} running", i + 1, j.pid);
                }
                0
            }
            "wait" => {
                if let Some(pid) = arg(1).and_then(|p| p.parse::<i32>().ok()) {
                    let st = process::wait(pid).map_or(127, |r| r.1.code());
                    self.jobs.retain(|j| j.pid as i32 != pid);
                    st
                } else {
                    let mut st = 0;
                    while let Some(j) = self.jobs.pop() {
                        st = process::wait(j.pid as i32).map_or(127, |r| r.1.code());
                    }
                    st
                }
            }
            "type" => {
                let mut st = 0;
                for name in &argv[1..] {
                    if BUILTINS.contains(&name.as_str()) {
                        huldra_user::println!("{} is a shell builtin", name);
                    } else if self.functions.contains_key(name) {
                        huldra_user::println!("{} is a function", name);
                    } else if let Some(p) = process::find_in_path(name) {
                        huldra_user::println!("{} is {}", name, p);
                    } else {
                        eprintln!("type: {}: not found", name);
                        st = 1;
                    }
                }
                st
            }
            "history" => {
                for (i, h) in self.history.iter().enumerate() {
                    huldra_user::println!("{:>5}  {}", i + 1, h);
                }
                0
            }
            _ => 127,
        };
        flow
    }

    fn builtin_read(&mut self, args: &[String]) -> i32 {
        let mut names: Vec<&str> = Vec::new();
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "-r" => {}
                "-p" if i + 1 < args.len() => {
                    huldra_user::print!("{}", args[i + 1]);
                    io::flush_stdout();
                    i += 1;
                }
                n => names.push(n),
            }
            i += 1;
        }
        // Byte by byte, so nothing after the newline is consumed.
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        let mut eof = false;
        loop {
            match sys::read(0, &mut byte) {
                Ok(0) => {
                    eof = true;
                    break;
                }
                Ok(_) if byte[0] == b'\n' => break,
                Ok(_) => line.push(byte[0]),
                Err(Errno::EINTR) => return 130,
                Err(_) => {
                    eof = true;
                    break;
                }
            }
        }
        let text = String::from_utf8_lossy(&line).into_owned();
        if names.is_empty() {
            names.push("REPLY");
        }
        let mut rest = text.trim_start();
        for (k, name) in names.iter().enumerate() {
            if k + 1 == names.len() {
                env::set_var(name, rest.trim_end());
            } else {
                let (field, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                env::set_var(name, field);
                rest = tail.trim_start();
            }
        }
        if eof && line.is_empty() {
            1
        } else {
            0
        }
    }
}

fn restore(saved: &[(i32, i32)]) {
    io::flush_stdout();
    for &(target, copy) in saved.iter().rev() {
        let _ = sys::dup2(copy, target);
        let _ = sys::close(copy);
    }
}

fn builtin_echo(args: &[String]) -> i32 {
    let mut newline = true;
    let mut escapes = false;
    let mut i = 0;
    while i < args.len() && args[i].starts_with('-') && args[i].len() > 1 && args[i][1..].chars().all(|c| "neE".contains(c)) {
        for c in args[i][1..].chars() {
            match c {
                'n' => newline = false,
                'e' => escapes = true,
                _ => escapes = false,
            }
        }
        i += 1;
    }
    let mut out = args[i..].join(" ");
    if escapes {
        out = unescape(&out);
    }
    if newline {
        out.push('\n');
    }
    huldra_user::print!("{}", out);
    0
}

/// Interprets backslash escapes (`\n`, `\t`, `\e`, `\\`, `\0NNN`).
pub fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('e') => out.push('\x1b'),
            Some('a') => out.push('\x07'),
            Some('\\') => out.push('\\'),
            Some('0') => {
                let mut v = 0u32;
                for _ in 0..3 {
                    match chars.peek().and_then(|d| d.to_digit(8)) {
                        Some(d) => {
                            v = v * 8 + d;
                            chars.next();
                        }
                        None => break,
                    }
                }
                out.push(char::from_u32(v).unwrap_or('?'));
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}
