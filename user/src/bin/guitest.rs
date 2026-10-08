//! guitest: checks the display server end to end (run with a display;
//! a window manager is started for the second half), then shuts the
//! display down. Prints "guitest: all checks passed" or the failures;
//! exits nonzero on failure.

#![no_std]
#![no_main]

use huldra_user::gui::*;
use huldra_user::{eprintln, println, process, signal, time, Vec};

huldra_user::main!(main);

struct T {
    failed: usize,
}

impl T {
    fn check(&mut self, ok: bool, what: &str) {
        if ok {
            println!("ok   {}", what);
        } else {
            println!("FAIL {}", what);
            self.failed += 1;
        }
    }
}

/// Waits for an event matching `f` (other events are skipped).
fn expect(d: &mut Display, ms: u64, mut f: impl FnMut(&Event) -> bool) -> Option<Event> {
    let end = time::uptime_ms() + ms;
    while time::uptime_ms() < end {
        match d.wait_event(100) {
            Some(e) if f(&e) => return Some(e),
            Some(_) => {}
            None if d.closed => return None,
            None => {}
        }
    }
    None
}

fn pixels(d: &mut Display, id: u32, r: Rect) -> Vec<u32> {
    d.send(Request::GetImage { id, x: r.x, y: r.y, w: r.w, h: r.h });
    match expect(d, 3000, |e| matches!(e, Event::ImageData { .. })) {
        Some(Event::ImageData { pixels, .. }) => pixels,
        _ => Vec::new(),
    }
}

fn main() -> i32 {
    let mut t = T { failed: 0 };
    let mut d = match Display::connect() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("guitest: cannot connect to the display: {}", e);
            return 1;
        }
    };
    t.check(d.width >= 640 && d.height >= 480 && d.client > 0, "connect and Welcome");

    // A window without a window manager appears where asked.
    let w = d.create_window(100, 120, 200, 100, KIND_NORMAL);
    d.set_title(w, "guitest");
    d.map(w);
    let exposed = expect(&mut d, 3000, |e| matches!(e, Event::Expose { id, .. } if *id == w));
    t.check(matches!(exposed, Some(Event::Expose { w: 200, h: 100, .. })), "map gives Expose with the size");
    d.fill(w, Rect::new(0, 0, 200, 100), 0xFF112233);
    d.fill(w, Rect::new(10, 10, 20, 20), 0xFFFF0000);
    let screen = pixels(&mut d, ROOT, Rect::new(110, 130, 1, 1));
    t.check(screen.first() == Some(&0xFFFF0000), "drawing reaches the screen");
    let screen = pixels(&mut d, ROOT, Rect::new(150, 150, 1, 1));
    t.check(screen.first() == Some(&0xFF112233), "window background on screen");

    // Text rendering with the VGA font: some pixels in the glyph box lit.
    d.text(w, 40, 40, 0xFFFFFFFF, 0xFF000000, "Hi");
    let glyphs = pixels(&mut d, w, Rect::new(40, 40, 16, 16));
    let lit = glyphs.iter().filter(|&&p| p == 0xFFFFFFFF).count();
    t.check(lit > 10 && lit < 200, "text is drawn with the font");

    // Moving and resizing.
    d.configure(w, Rect::new(300, 200, 260, 140));
    let conf = expect(&mut d, 3000, |e| matches!(e, Event::Configure { id, .. } if *id == w));
    t.check(matches!(conf, Some(Event::Configure { x: 300, y: 200, w: 260, h: 140, .. })), "configure moves and resizes");
    let moved = pixels(&mut d, ROOT, Rect::new(310, 210, 1, 1));
    t.check(moved.first() == Some(&0xFFFF0000), "content follows the window");
    let old = pixels(&mut d, ROOT, Rect::new(110, 130, 1, 1));
    t.check(old.first() != Some(&0xFFFF0000), "old area is repainted");

    // A second client sees the window list.
    let mut panel = Display::connect().expect("second client");
    panel.send(Request::SelectWindows);
    let item = expect(&mut panel, 3000, |e| matches!(e, Event::WindowListItem { .. }));
    t.check(matches!(&item, Some(Event::WindowListItem { id, title, mapped: true }) if *id == w && title == "guitest"), "window list for panels");

    // With a window manager: frames and MapRequest.
    let wm = match process::fork() {
        Ok(None) => {
            let e = process::exec("/bin/boxwm", &["boxwm"], &huldra_user::env::envp());
            eprintln!("guitest: boxwm: {}", e);
            process::exit(127)
        }
        Ok(Some(pid)) => pid,
        Err(_) => 0,
    };
    time::sleep_ms(1000);
    let w2 = d.create_window(0, 0, 160, 90, KIND_NORMAL);
    d.set_title(w2, "managed");
    d.map(w2);
    let conf = expect(&mut d, 5000, |e| matches!(e, Event::Configure { id, .. } if *id == w2));
    let placed = match conf {
        Some(Event::Configure { x, y, .. }) => (x, y),
        _ => (-1, -1),
    };
    t.check(placed.0 > 0 && placed.1 > 20, "the window manager places the window");
    let exposed = expect(&mut d, 3000, |e| matches!(e, Event::Expose { id, .. } if *id == w2));
    t.check(exposed.is_some(), "managed window is shown");
    // The title bar is drawn above the window.
    let title = pixels(&mut d, ROOT, Rect::new(placed.0 + 40, placed.1 - 12, 1, 1));
    let bg = pixels(&mut d, ROOT, Rect::new(5, 5, 1, 1));
    t.check(!title.is_empty() && title != bg, "frame with a title bar");
    d.send(Request::Close { id: w2 });
    let close = expect(&mut d, 3000, |e| matches!(e, Event::CloseRequest { id } if *id == w2));
    t.check(close.is_some(), "close requests reach the owner");
    if wm > 0 {
        let _ = signal::kill(wm as i32, huldra_user::abi::signal::SIGTERM);
        let _ = process::wait(wm as i32);
    }

    d.send(Request::Quit);
    d.flush();
    if t.failed == 0 {
        println!("guitest: all checks passed");
        0
    } else {
        println!("guitest: {} checks failed", t.failed);
        1
    }
}
