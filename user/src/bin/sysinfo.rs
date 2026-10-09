//! sysinfo: system information and live usage in a window.

#![no_std]
#![no_main]

use huldra_user::gui::*;
use huldra_user::{eprintln, format, fs, sys, String, Vec};

huldra_user::main!(main);

fn meminfo() -> (u64, u64) {
    let text = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let field = |name: &str| text.lines().find(|l| l.starts_with(name)).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse().ok()).unwrap_or(0u64);
    (field("MemTotal:"), field("MemFree:"))
}

fn lines() -> Vec<(String, String)> {
    let mut v = Vec::new();
    if let Ok(u) = sys::uname() {
        let s = |b: &[u8]| String::from_utf8_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]).into_owned();
        v.push((String::from("System"), format!("{} {} ({})", s(&u.sysname), s(&u.release), s(&u.machine))));
        v.push((String::from("Host"), s(&u.nodename)));
    }
    let cpu = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    if let Some(model) = cpu.lines().find(|l| l.starts_with("model name")).and_then(|l| l.split_once(':')) {
        v.push((String::from("CPU"), String::from(model.1.trim())));
    }
    let up = fs::read_to_string("/proc/uptime").unwrap_or_default();
    let secs: u64 = up.split(['.', ' ']).next().and_then(|s| s.parse().ok()).unwrap_or(0);
    v.push((String::from("Uptime"), format!("{}h {:02}m {:02}s", secs / 3600, secs / 60 % 60, secs % 60)));
    let (total, free) = meminfo();
    v.push((String::from("Memory"), format!("{} MiB used of {} MiB", (total - free) / 1024, total / 1024)));
    if let Ok(st) = sys::statfs("/") {
        let bs = st.f_bsize as u64;
        v.push((String::from("Disk /"), format!("{} KiB free of {} KiB", st.f_bfree * bs / 1024, st.f_blocks * bs / 1024)));
    }
    let net = fs::read_to_string("/proc/net/if").unwrap_or_default();
    if let Some(eth) = net.lines().find(|l| l.contains("inet") && l.starts_with("eth0")) {
        let addr = eth.split_whitespace().skip_while(|w| *w != "inet").nth(1).unwrap_or("-");
        v.push((String::from("Network"), format!("eth0 {}", addr)));
    }
    let procs = fs::read_dir("/proc").map(|d| d.iter().filter(|e| e.name.chars().all(|c| c.is_ascii_digit())).count()).unwrap_or(0);
    v.push((String::from("Processes"), format!("{}", procs)));
    v
}

fn draw(d: &mut Display, win: u32, font: &Font, w: i32, h: i32) {
    let mut c = Canvas::new(w, h);
    c.fill_rect(c.bounds(), theme::SURFACE);
    // Logo.
    c.fill_rect(Rect::new(14, 14, 56, 56), theme::BG);
    c.rect_outline(Rect::new(14, 14, 56, 56), theme::ACCENT);
    c.rect_outline(Rect::new(17, 17, 50, 50), theme::LINE_DIM);
    c.draw_text_bold(font, 34, 34, "H", theme::ACCENT);
    c.draw_text_bold(font, 84, 18, "HULDRA INDUSTRIES", theme::BRIGHT);
    c.draw_text(font, 84, 40, "Unified Operating System, written in Rust", theme::TEXT_DIM, None);
    let mut y = 86;
    for (k, v) in lines() {
        c.draw_text_bold(font, 16, y, &k, theme::TEXT);
        c.draw_text(font, 120, y, &v, theme::BRIGHT, None);
        y += 22;
    }
    let (total, free) = meminfo();
    if total > 0 {
        let bar = Rect::new(120, y + 4, w - 140, 12);
        c.fill_rect(bar, theme::BG);
        c.rect_outline(bar, theme::LINE);
        let used = ((total - free) * bar.w as u64 / total) as i32;
        // A segmented bar, like an old VU meter.
        let mut x = bar.x + 2;
        while x + 4 <= bar.x + 2 + used.max(4).min(bar.w - 4) {
            c.fill_rect(Rect::new(x, bar.y + 2, 4, bar.h - 4), theme::ACCENT);
            x += 6;
        }
    }
    d.put_canvas(win, &c, c.bounds(), 0, 0);
    d.flush();
}

fn main() -> i32 {
    let mut d = match Display::connect() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("sysinfo: {}", e);
            return 1;
        }
    };
    let font = load_font();
    let (mut w, mut h) = (480, 300);
    let win = d.create_window(0, 0, w, h, KIND_NORMAL);
    d.set_title(win, "System information");
    d.map(win);
    loop {
        match d.wait_event(2000) {
            Some(Event::Expose { w: nw, h: nh, .. }) => {
                w = nw;
                h = nh;
            }
            Some(Event::CloseRequest { .. }) => return 0,
            Some(_) => continue,
            None if d.closed => return 0,
            None => {}
        }
        draw(&mut d, win, &font, w, h);
    }
}
