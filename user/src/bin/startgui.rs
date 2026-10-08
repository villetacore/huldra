//! startgui [boxwm|tilewm] [WIDTHxHEIGHT]: starts the graphical session
//! (like startx): the display server, a window manager, the panel and a
//! terminal. Defaults come from /etc/gui.conf. The session ends when the
//! display server exits (menu "Exit", or Ctrl+Alt+Backspace).

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, fs, gui, println, process, signal, time, String, Vec};

huldra_user::main!(main);

fn conf(key: &str) -> Option<String> {
    let text = fs::read_to_string("/etc/gui.conf").ok()?;
    text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == key).then(|| String::from(v.trim()))
    })
}

fn start(cmd: &str) -> Option<u32> {
    let args: Vec<&str> = cmd.split_whitespace().collect();
    let path = process::find_in_path(args.first()?)?;
    match process::fork() {
        Ok(None) => {
            let e = process::exec(&path, &args, &env::envp());
            eprintln!("startgui: {}: {}", path, e);
            process::exit(127)
        }
        Ok(Some(pid)) => Some(pid),
        Err(_) => None,
    }
}

fn main() -> i32 {
    let args = env::args();
    let mut wm = conf("wm").unwrap_or_else(|| String::from("boxwm"));
    let mut mode = conf("mode").unwrap_or_default();
    for a in &args[1..] {
        if a.contains('x') && a.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            mode = a.clone();
        } else {
            wm = a.clone();
        }
    }
    if gui::Display::connect().is_ok() {
        eprintln!("startgui: a display server is already running");
        return 1;
    }
    println!("startgui: starting the display ({}), {}", if mode.is_empty() { "1024x768" } else { &mode }, wm);
    let Some(display) = start(&if mode.is_empty() { String::from("display") } else { huldra_user::format!("display {}", mode) }) else {
        eprintln!("startgui: cannot start display");
        return 1;
    };
    // Wait until it accepts connections.
    let mut up = false;
    for _ in 0..100 {
        time::sleep_ms(100);
        if gui::Display::connect().is_ok() {
            up = true;
            break;
        }
        if let Ok(Some(_)) = process::try_wait(display as i32) {
            break;
        }
    }
    if !up {
        eprintln!("startgui: the display server did not start");
        return 1;
    }
    let mut children = Vec::new();
    children.extend(start(&wm));
    time::sleep_ms(200);
    children.extend(start("panel"));
    let autostart = conf("autostart").unwrap_or_else(|| String::from("term"));
    for cmd in autostart.split(';').map(str::trim).filter(|c| !c.is_empty()) {
        children.extend(start(cmd));
    }
    // The session lasts as long as the display server.
    let _ = process::wait(display as i32);
    for pid in children {
        let _ = signal::kill(pid as i32, huldra_user::abi::signal::SIGTERM);
    }
    while process::try_wait(-1).is_ok_and(|r| r.is_some()) {}
    println!("startgui: session ended");
    0
}
