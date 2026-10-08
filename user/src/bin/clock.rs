//! clock: an analog clock (like xclock).

#![no_std]
#![no_main]

use huldra_user::gui::*;
use huldra_user::time::{self, DateTime, MONTHS, WEEKDAYS};
use huldra_user::{eprintln, format};

huldra_user::main!(main);

/// sin and cos of `deg` degrees, scaled by 1000 (no floating point).
fn sin_cos(deg: i32) -> (i32, i32) {
    // Table of sin for 0..=90 degrees in 6-degree steps is enough for a
    // clock face; interpolate the rest.
    const SIN: [i32; 16] = [0, 105, 208, 309, 407, 500, 588, 669, 743, 809, 866, 914, 951, 978, 995, 1000];
    let s = |d: i32| -> i32 {
        let d = d.rem_euclid(360);
        let (q, r) = (d / 90, d % 90);
        let at = |a: i32| {
            let i = (a / 6) as usize;
            let f = a % 6;
            if i >= 15 {
                SIN[15]
            } else {
                SIN[i] + (SIN[i + 1] - SIN[i]) * f / 6
            }
        };
        match q {
            0 => at(r),
            1 => at(90 - r),
            2 => -at(r),
            _ => -at(90 - r),
        }
    };
    (s(deg), s(deg + 90))
}

fn hand(c: &mut Canvas, cx: i32, cy: i32, deg: i32, len: i32, width: i32, color: u32) {
    let (s, co) = sin_cos(deg);
    c.thick_line(cx, cy, cx + len * s / 1000, cy - len * co / 1000, width, color);
}

fn draw(d: &mut Display, win: u32, font: &Font, w: i32, h: i32) {
    let mut c = Canvas::new(w, h);
    c.gradient(c.bounds(), 0xFFF4F1EA, 0xFFDCD6C8);
    let face_h = h - 40;
    let (cx, cy) = (w / 2, face_h / 2 + 6);
    let r = (w.min(face_h) / 2 - 10).max(20);
    c.fill_circle(cx + 3, cy + 3, r, 0xFFB8B0A0);
    c.fill_circle(cx, cy, r, 0xFFFFFFFF);
    c.circle(cx, cy, r, 0xFF404040);
    for m in 0..60 {
        let (s, co) = sin_cos(m * 6);
        let inner = if m % 5 == 0 { r - 12 } else { r - 5 };
        let color = if m % 5 == 0 { 0xFF202020 } else { 0xFF909090 };
        c.thick_line(cx + inner * s / 1000, cy - inner * co / 1000, cx + (r - 2) * s / 1000, cy - (r - 2) * co / 1000, if m % 5 == 0 { 3 } else { 1 }, color);
    }
    let t = DateTime::from_unix(time::now());
    let sec = t.second as i32;
    let min = t.minute as i32;
    let hour = (t.hour % 12) as i32;
    hand(&mut c, cx, cy, hour * 30 + min / 2, r * 5 / 10, 5, 0xFF202020);
    hand(&mut c, cx, cy, min * 6 + sec / 10, r * 8 / 10, 3, 0xFF202020);
    hand(&mut c, cx, cy, sec * 6, r * 9 / 10, 1, 0xFFC0392B);
    c.fill_circle(cx, cy, 4, 0xFFC0392B);
    let text = format!("{} {} {}  {:02}:{:02}:{:02}", WEEKDAYS[t.weekday as usize], t.day, MONTHS[(t.month - 1) as usize], t.hour, t.minute, t.second);
    let tw = font.text_width(&text);
    c.draw_text(font, (w - tw) / 2, h - 28, &text, 0xFF303030, None);
    d.put_canvas(win, &c, c.bounds(), 0, 0);
    d.flush();
}

fn main() -> i32 {
    let mut d = match Display::connect() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("clock: {}", e);
            return 1;
        }
    };
    let font = load_font();
    let (mut w, mut h) = (220, 250);
    let win = d.create_window(0, 0, w, h, KIND_NORMAL);
    d.set_title(win, "Clock");
    d.map(win);
    let mut last = 0;
    loop {
        match d.wait_event(250) {
            Some(Event::Expose { w: nw, h: nh, .. }) => {
                w = nw;
                h = nh;
                draw(&mut d, win, &font, w, h);
            }
            Some(Event::CloseRequest { .. }) => return 0,
            Some(_) => {}
            None if d.closed => return 0,
            None => {}
        }
        let now = time::now();
        if now != last {
            last = now;
            draw(&mut d, win, &font, w, h);
        }
    }
}
