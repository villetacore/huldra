//! help [TOPIC|COMMAND] | help commands | help topics | help -k WORD:
//! documentation and command reference inside the system.
//!
//!   help               overview of the system and where to start
//!   help commands      every command with a one-line summary, by area
//!   help topics        the documentation (getting started, packages, ...)
//!   help NAME          a command's help or a documentation page
//!   help -k WORD       search commands and documentation
//!
//! Command help comes from /usr/share/huldra/help (generated from each
//! program's own description when the system is built), documentation
//! from /usr/share/huldra/docs. Long pages open in less. `man` is the same
//! program.

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, format, fs, print, println, process, term, String, ToString, Vec};

huldra_user::main!(main);

const DOCS: &str = "/usr/share/huldra/docs";
const HELP: &str = "/usr/share/huldra/help";

/// Commands by area, for `help commands`. Anything not listed goes under
/// "Other".
const AREAS: &[(&str, &[&str])] = &[
    ("Files", &["ls", "cat", "cp", "mv", "rm", "mkdir", "rmdir", "ln", "readlink", "touch", "find", "tree", "du", "df", "stat", "tar", "fm", "less", "more", "edit", "pwd", "basename", "dirname"]),
    ("Text", &["grep", "sed", "sort", "uniq", "cut", "tr", "wc", "head", "tail", "nl", "rev", "diff", "cmp", "tee", "xargs", "printf", "echo", "base64", "sha256sum", "hexdump", "seq", "expr", "yes"]),
    ("Network", &["ifconfig", "ping", "host", "wget", "nc", "httpd", "git", "browse", "web"]),
    ("Packages", &["pkg"]),
    ("Development", &["cc", "sh", "time", "env", "which", "true", "false", "test"]),
    ("System", &["ps", "top", "kill", "free", "uptime", "uname", "hostname", "date", "cal", "dmesg", "lspci", "mount", "umount", "sync", "reboot", "poweroff", "id", "whoami", "clear", "sleep", "help", "man"]),
    ("Graphics", &["startgui", "display", "boxwm", "tilewm", "panel", "term", "files", "clock", "calc", "paint", "sysinfo", "menu", "screenshot"]),
    ("Games", &["hack", "mines", "blocks", "snake", "2048", "sl", "fortune", "cowsay"]),
];

/// Programs that are not meant to be run by hand.
const HIDDEN: &[&str] = &["init", "guitest", "utest"];

struct Entry {
    name: String,
    synopsis: String,
    summary: String,
}

/// /usr/share/huldra/help/INDEX: `name<TAB>synopsis<TAB>summary` lines.
fn index() -> Vec<Entry> {
    fs::read_to_string(&format!("{}/INDEX", HELP))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut f = l.splitn(3, '\t');
            Some(Entry { name: f.next()?.to_string(), synopsis: f.next()?.to_string(), summary: f.next().unwrap_or("").to_string() })
        })
        .filter(|e| !HIDDEN.contains(&e.name.as_str()))
        .collect()
}

/// Documentation pages: (name, title).
fn topics() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = fs::read_dir(DOCS)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| {
            let name = e.name.strip_suffix(".md")?.to_string();
            if name == "README" {
                return None; // the index of the docs, which this list replaces
            }
            let text = fs::read_to_string(&fs::join(DOCS, &e.name)).ok()?;
            let title = match text.lines().find_map(|l| l.strip_prefix("# ")) {
                Some(t) => t.to_string(),
                None if name == "overview" => "The project README: features at a glance".to_string(),
                None => name.clone(),
            };
            Some((name, title))
        })
        .collect();
    v.sort();
    v
}

fn color() -> bool {
    term::is_tty(huldra_user::io::STDOUT)
}

fn bold(s: &str) -> String {
    if color() { format!("\x1b[1;92m{}\x1b[0m", s) } else { s.to_string() }
}

fn dim(s: &str) -> String {
    if color() { format!("\x1b[32m{}\x1b[0m", s) } else { s.to_string() }
}

/// Shows text: through less if it does not fit on the screen.
fn page(name: &str, text: &str) {
    let (rows, _) = term::size();
    if !color() || text.lines().count() < rows.saturating_sub(1) {
        print!("{}", text);
        if !text.ends_with('\n') {
            println!();
        }
        return;
    }
    let tmp = format!("/tmp/help-{}", name.replace('/', "-"));
    if fs::write(&tmp, text.as_bytes()).is_err() {
        print!("{}", text);
        return;
    }
    let _ = process::run("/bin/less", &["less", &tmp]);
    let _ = fs::remove_file(&tmp);
}

fn proc_value(path: &str, key: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let line = text.lines().find(|l| l.starts_with(key))?;
    Some(line[key.len()..].trim().to_string())
}

/// A few facts about the running system for the overview.
fn system_info() -> Vec<(String, String)> {
    let mut v = Vec::new();
    let release = fs::read_to_string("/etc/os-release").unwrap_or_default();
    let name = release.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=")).unwrap_or("Huldra").trim_matches('"').to_string();
    v.push(("system".into(), name));
    if let Ok(up) = fs::read_to_string("/proc/uptime") {
        let secs: u64 = up.split(['.', ' ']).next().and_then(|s| s.parse().ok()).unwrap_or(0);
        v.push(("uptime".into(), format!("{}h {:02}m {:02}s", secs / 3600, secs / 60 % 60, secs % 60)));
    }
    if let (Some(free), Some(total)) = (proc_value("/proc/meminfo", "MemFree:"), proc_value("/proc/meminfo", "MemTotal:")) {
        v.push(("memory".into(), format!("{} free of {}", free, total)));
    }
    if let Ok(net) = fs::read_to_string("/proc/net/if") {
        if let Some(eth) = net.lines().find(|l| l.starts_with("eth0")) {
            let ip = eth.split_whitespace().skip_while(|w| *w != "inet").nth(1).unwrap_or("-");
            v.push(("network".into(), format!("eth0 {}", ip)));
        }
    }
    if let Ok(gen) = fs::read_link("/pkg/system") {
        let n = gen.rsplit('/').next().unwrap_or("?").to_string();
        let count = fs::read_to_string(&format!("/pkg/{}/manifest", gen)).map(|m| m.lines().filter(|l| l.starts_with("package")).count()).unwrap_or(0);
        v.push(("packages".into(), format!("{} (generation {})", count, n)));
    } else {
        v.push(("packages".into(), "none yet (pkg add NAME)".into()));
    }
    v
}

fn overview() {
    let mut s = String::new();
    s.push_str(&bold("Huldra"));
    s.push_str(" — a small Unix-like system. This is the built-in help.\n\n");
    for (k, v) in system_info() {
        s.push_str(&format!("  {} {}\n", dim(&format!("{:<9}", k)), v));
    }
    s.push_str(&format!("\n{}\n", bold("Getting around")));
    for (cmd, what) in [
        ("help commands", "every command, by area"),
        ("help topics", "the documentation: packages, graphics, networking, ..."),
        ("help NAME", "help for a command (help ls) or a page (help packages)"),
        ("help -k WORD", "search commands and documentation"),
        ("ls /bin", "programs of the base system; Tab completes names"),
        ("pkg search", "more software; pkg add NAME installs it"),
        ("startgui", "the graphical desktop"),
    ] {
        s.push_str(&format!("  {}  {}\n", if color() { format!("\x1b[33m{:<15}\x1b[0m", cmd) } else { format!("{:<15}", cmd) }, what));
    }
    s.push_str(&format!("\n{}\n", bold("Documentation")));
    for (name, title) in topics() {
        s.push_str(&format!("  {:<17}{}\n", name, title));
    }
    page("overview", &s);
}

fn list_commands() {
    let idx = index();
    let mut s = String::new();
    let mut shown: Vec<&str> = Vec::new();
    let line = |e: &Entry| {
        let text = if e.summary.is_empty() { e.synopsis.clone() } else { e.summary.clone() };
        let (_, cols) = term::size();
        let mut t: String = text.chars().take(cols.saturating_sub(18).max(20)).collect();
        if t.chars().count() < text.chars().count() {
            t.pop();
            t.push('…');
        }
        format!("  {} {}\n", if color() { format!("\x1b[33m{:<13}\x1b[0m", e.name) } else { format!("{:<13}", e.name) }, t)
    };
    for (area, names) in AREAS {
        let entries: Vec<&Entry> = names.iter().filter_map(|n| idx.iter().find(|e| e.name == *n)).collect();
        if entries.is_empty() {
            continue;
        }
        s.push_str(&format!("{}\n", bold(area)));
        for e in entries {
            s.push_str(&line(e));
            shown.push(&e.name);
        }
        s.push('\n');
    }
    let rest: Vec<&Entry> = idx.iter().filter(|e| !shown.contains(&e.name.as_str())).collect();
    if !rest.is_empty() {
        s.push_str(&format!("{}\n", bold("Other")));
        for e in rest {
            s.push_str(&line(e));
        }
        s.push('\n');
    }
    s.push_str("help NAME shows more about a command.\n");
    page("commands", &s);
}

fn list_topics() {
    for (name, title) in topics() {
        println!("  {:<17}{}", name, title);
    }
    println!("\nhelp NAME opens a page, e.g. help packages.");
}

/// Commands whose documentation page is worth pointing at.
fn see_also(name: &str) -> Option<&'static str> {
    Some(match name {
        "pkg" => "packages",
        "cc" => "c-compiler",
        "startgui" | "display" | "boxwm" | "tilewm" | "term" | "panel" => "graphics",
        "ifconfig" | "ping" | "wget" | "nc" | "httpd" | "host" => "networking",
        "git" => "git",
        "browse" | "web" => "browser",
        _ => return None,
    })
}

fn show(name: &str) -> bool {
    // Aliases for documentation pages.
    let topic = match name {
        "start" | "intro" => "getting-started",
        "packages" | "package" | "pkgs" => "packages",
        "cc-compiler" | "c" | "compiler" => "c-compiler",
        "net" | "network" => "networking",
        "gui" => "graphics",
        "tests" => "testing",
        other => other,
    };
    if !fs::exists(&format!("{}/{}", HELP, name)) || topic != name {
        if let Ok(md) = fs::read_to_string(&format!("{}/{}.md", DOCS, topic)) {
            let (_, cols) = term::size();
            page(topic, &huldra_md::render(&md, cols.saturating_sub(2).min(100), color()));
            return true;
        }
    }
    let Ok(text) = fs::read_to_string(&format!("{}/{}", HELP, name)) else {
        return false;
    };
    let mut lines = text.lines();
    let mut s = String::new();
    if let Some(first) = lines.next() {
        s.push_str(&bold(first));
        s.push('\n');
    }
    for l in lines {
        s.push_str(l);
        s.push('\n');
    }
    if let Some(t) = see_also(name).filter(|t| fs::exists(&format!("{}/{}.md", DOCS, t))) {
        s.push_str(&format!("\nSee also: help {}\n", t));
    }
    page(name, &s);
    true
}

fn search(word: &str) {
    let w = word.to_lowercase();
    let mut found = false;
    for e in index() {
        if e.name.contains(&w) || e.summary.to_lowercase().contains(&w) || e.synopsis.to_lowercase().contains(&w) {
            println!("  {:<13} {}", e.name, if e.summary.is_empty() { &e.synopsis } else { &e.summary });
            found = true;
        }
    }
    for (name, _) in topics() {
        let text = fs::read_to_string(&format!("{}/{}.md", DOCS, name)).unwrap_or_default();
        let hits: Vec<&str> = text.lines().filter(|l| l.to_lowercase().contains(&w)).take(3).collect();
        if !hits.is_empty() {
            println!("{}", bold(&format!("help {}", name)));
            for h in hits {
                let h: String = huldra_md::strip_escapes(&huldra_md::render(h.trim_start_matches(['#', '|', '-', ' ']), 200, false)).trim().chars().take(76).collect();
                println!("    {}", h);
            }
            found = true;
        }
    }
    if !found {
        println!("nothing found for '{}'", word);
    }
}

fn main() -> i32 {
    let args = env::args();
    let rest: Vec<&str> = args[1..].iter().map(|s| s.as_str()).collect();
    match rest[..] {
        [] => overview(),
        ["commands"] | ["-a"] => list_commands(),
        ["topics"] | ["docs"] => list_topics(),
        ["-k", w] => search(w),
        [name] if !name.starts_with('-') => {
            if !show(name) {
                eprintln!("help: no help for '{}' (try help commands, help topics or help -k {})", name, name);
                return 1;
            }
        }
        _ => {
            eprintln!("usage: help [NAME] | help commands | help topics | help -k WORD");
            return 2;
        }
    }
    0
}
