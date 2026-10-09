//! ln -s [-f] TARGET LINK: create a symbolic link (hard links are not
//! supported). With -f an existing LINK is replaced atomically.

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, format, fs, Vec};

huldra_user::main!(main);

fn main() -> i32 {
    let args = env::args();
    let (mut symbolic, mut force) = (false, false);
    let mut operands: Vec<&str> = Vec::new();
    for a in &args[1..] {
        match a.as_str() {
            "-s" => symbolic = true,
            "-f" => force = true,
            "-sf" | "-fs" => (symbolic, force) = (true, true),
            _ => operands.push(a),
        }
    }
    if !symbolic {
        eprintln!("ln: only symbolic links are supported (use ln -s)");
        return 1;
    }
    let [target, link] = operands[..] else {
        eprintln!("usage: ln -s [-f] TARGET LINK");
        return 2;
    };
    // `ln -s /x dir` creates dir/x.
    let link = match fs::metadata(link) {
        Ok(st) if fs::is_dir(&st) && fs::symlink_metadata(link).is_ok_and(|s| !fs::is_symlink(&s)) => {
            fs::join(link, target.trim_end_matches('/').rsplit('/').next().unwrap_or(target))
        }
        _ => link.into(),
    };
    let result = if force {
        let tmp = format!("{}.ln-tmp", link);
        let _ = fs::remove_file(&tmp);
        fs::symlink(target, &tmp).and_then(|_| fs::rename(&tmp, &link))
    } else {
        fs::symlink(target, &link)
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("ln: {}: {}", link, e);
            1
        }
    }
}
