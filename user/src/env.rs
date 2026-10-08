//! Command-line arguments and environment variables.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::UnsafeCell;

struct Env {
    args: Vec<String>,
    vars: Vec<(String, String)>,
}

struct Global(UnsafeCell<Env>);
unsafe impl Sync for Global {}

static ENV: Global = Global(UnsafeCell::new(Env {
    args: Vec::new(),
    vars: Vec::new(),
}));

fn env() -> &'static mut Env {
    unsafe { &mut *ENV.0.get() }
}

unsafe fn cstr(p: *const u8) -> String {
    let mut len = 0;
    while *p.add(len) != 0 {
        len += 1;
    }
    String::from_utf8_lossy(core::slice::from_raw_parts(p, len)).into_owned()
}

pub(crate) unsafe fn init(argc: usize, argv: *const *const u8, mut envp: *const *const u8) {
    let e = env();
    for i in 0..argc {
        e.args.push(cstr(*argv.add(i)));
    }
    while !(*envp).is_null() {
        let s = cstr(*envp);
        if let Some((k, v)) = s.split_once('=') {
            e.vars.push((k.to_string(), v.to_string()));
        }
        envp = envp.add(1);
    }
}

pub fn args() -> &'static [String] {
    &env().args
}

pub fn program_name() -> &'static str {
    env()
        .args
        .first()
        .map(|a| a.rsplit('/').next().unwrap_or(a))
        .unwrap_or("?")
}

pub fn var(key: &str) -> Option<&'static str> {
    env()
        .vars
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

pub fn set_var(key: &str, value: &str) {
    let vars = &mut env().vars;
    match vars.iter_mut().find(|(k, _)| k == key) {
        Some(entry) => entry.1 = value.to_string(),
        None => vars.push((key.to_string(), value.to_string())),
    }
}

pub fn remove_var(key: &str) {
    env().vars.retain(|(k, _)| k != key);
}

pub fn vars() -> &'static [(String, String)] {
    &env().vars
}

/// The environment as `KEY=value` strings (for `execve`).
pub fn envp() -> Vec<String> {
    env()
        .vars
        .iter()
        .map(|(k, v)| alloc::format!("{}={}", k, v))
        .collect()
}
