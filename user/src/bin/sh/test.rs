//! The `test` / `[` builtin.

use alloc::string::String;
use huldra_user::abi::fs::*;
use huldra_user::sys;

fn file_test(op: &str, path: &str) -> bool {
    let Ok(st) = sys::stat(path) else { return false };
    let kind = st.st_mode & S_IFMT;
    match op {
        "-e" | "-r" | "-w" => true,
        "-f" => kind == S_IFREG,
        "-d" => kind == S_IFDIR,
        "-c" => kind == S_IFCHR,
        "-b" => kind == S_IFBLK,
        "-p" => kind == S_IFIFO,
        "-s" => st.st_size > 0,
        "-x" => st.st_mode & 0o111 != 0,
        _ => false,
    }
}

fn int(s: &str) -> Result<i64, String> {
    s.trim().parse().map_err(|_| alloc::format!("{}: integer expected", s))
}

fn primary(args: &[&str]) -> Result<bool, String> {
    match args {
        [] => Ok(false),
        [s] => Ok(!s.is_empty()),
        ["!", rest @ ..] => primary(rest).map(|b| !b),
        ["-n", s] => Ok(!s.is_empty()),
        ["-z", s] => Ok(s.is_empty()),
        [op, path] if op.len() == 2 && op.starts_with('-') => Ok(file_test(op, path)),
        [a, "=", b] | [a, "==", b] => Ok(a == b),
        [a, "!=", b] => Ok(a != b),
        [a, op, b] => {
            let (x, y) = (int(a)?, int(b)?);
            match *op {
                "-eq" => Ok(x == y),
                "-ne" => Ok(x != y),
                "-lt" => Ok(x < y),
                "-le" => Ok(x <= y),
                "-gt" => Ok(x > y),
                "-ge" => Ok(x >= y),
                _ => Err(alloc::format!("unknown operator {}", op)),
            }
        }
        _ => Err(String::from("too many arguments")),
    }
}

/// Evaluates a test expression with `-a` / `-o` (lowest precedence).
pub fn evaluate(args: &[&str]) -> Result<bool, String> {
    if let Some(i) = args.iter().position(|&a| a == "-o") {
        return Ok(evaluate(&args[..i])? || evaluate(&args[i + 1..])?);
    }
    if let Some(i) = args.iter().position(|&a| a == "-a") {
        return Ok(evaluate(&args[..i])? && evaluate(&args[i + 1..])?);
    }
    primary(args)
}
