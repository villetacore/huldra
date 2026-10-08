#![no_std]
#![no_main]

use huldra_user::{env, eprintln, fs, String};

huldra_user::main!(main);

fn main() -> i32 {
    let args = &env::args()[1..];
    if args.len() < 2 {
        eprintln!("usage: mv source... dest");
        return 2;
    }
    let (sources, dest) = args.split_at(args.len() - 1);
    let dest = dest[0].as_str();
    let dest_is_dir = fs::metadata(dest).map(|s| fs::is_dir(&s)).unwrap_or(false);
    let mut status = 0;
    for src in sources {
        let target = if dest_is_dir {
            fs::join(dest, src.rsplit('/').next().unwrap_or(src))
        } else {
            String::from(dest)
        };
        if let Err(e) = fs::rename(src, &target) {
            eprintln!("mv: {}: {}", src, e);
            status = 1;
        }
    }
    status
}
