//! readlink [-f] PATH: print the target of a symbolic link; with -f the
//! fully resolved path.

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, fs, println, String, Vec};

huldra_user::main!(main);

/// Resolves every symbolic link in `path` (which must exist).
fn canonical(path: &str) -> Result<String, huldra_user::Errno> {
    let mut path = if path.starts_with('/') { String::from(path) } else { fs::join(&fs::current_dir()?, path) };
    for _ in 0..40 {
        let mut out: Vec<String> = Vec::new();
        let mut changed = false;
        let comps: Vec<&str> = path.split('/').filter(|c| !c.is_empty() && *c != ".").collect();
        for (i, c) in comps.iter().enumerate() {
            if *c == ".." {
                out.pop();
                continue;
            }
            out.push(String::from(*c));
            let here = huldra_user::format!("/{}", out.join("/"));
            let st = fs::symlink_metadata(&here)?;
            if fs::is_symlink(&st) {
                let target = fs::read_link(&here)?;
                out.pop();
                let base = if target.starts_with('/') { String::new() } else { huldra_user::format!("/{}", out.join("/")) };
                let rest = comps[i + 1..].join("/");
                path = huldra_user::format!("{}/{}/{}", base, target, rest);
                changed = true;
                break;
            }
        }
        if !changed {
            return Ok(huldra_user::format!("/{}", out.join("/")));
        }
    }
    Err(huldra_user::Errno::ELOOP)
}

fn main() -> i32 {
    let args = env::args();
    let follow = args.iter().any(|a| a == "-f");
    let Some(path) = args[1..].iter().find(|a| !a.starts_with('-')) else {
        eprintln!("usage: readlink [-f] PATH");
        return 2;
    };
    let r = if follow { canonical(path) } else { fs::read_link(path) };
    match r {
        Ok(t) => {
            println!("{}", t);
            0
        }
        Err(_) => 1,
    }
}
