//! menu: a run dialog (like dmenu). Type a command; the matching programs
//! from PATH are listed. Tab or the arrows choose, Enter runs, Esc closes.

#![no_std]
#![no_main]

use huldra_gfx::keymap;
use huldra_user::gui::*;
use huldra_user::{env, eprintln, fs, String, Vec};

huldra_user::main!(main);

const HEIGHT: i32 = 22;

fn programs() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for dir in env::var("PATH").unwrap_or("/bin:/usr/bin").split(':') {
        for e in fs::read_dir(dir).unwrap_or_default() {
            if !e.is_dir() && !v.contains(&e.name) {
                v.push(e.name.clone());
            }
        }
    }
    v.sort();
    v
}

fn main() -> i32 {
    let mut d = match Display::connect() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("menu: {}", e);
            return 1;
        }
    };
    let font = load_font();
    let w = d.width;
    let win = d.create_window(0, 0, w, HEIGHT, KIND_POPUP);
    d.map(win);
    d.send(Request::Raise { id: win });
    d.send(Request::SetFocus { id: win });
    let all = programs();
    let mut input = String::new();
    let mut sel = 0usize;
    loop {
        let matches: Vec<&String> = all.iter().filter(|p| p.starts_with(input.split_whitespace().next().unwrap_or(""))).collect();
        sel = sel.min(matches.len().saturating_sub(1));
        // Draw: prompt, input, then the matches like dmenu.
        let mut c = Canvas::new(w, HEIGHT);
        c.fill_rect(c.bounds(), 0xFF222222);
        c.fill_rect(Rect::new(0, 0, 60, HEIGHT), 0xFF285577);
        c.draw_text_bold(&font, 10, 3, "run:", 0xFFFFFFFF);
        let x = c.draw_text(&font, 70, 3, &input, 0xFFFFFFFF, None);
        c.fill_rect(Rect::new(x + 1, 4, 2, 14), 0xFFFFFFFF);
        let mut mx = 260.max(x + 20);
        for (i, m) in matches.iter().enumerate().take(40) {
            let tw = font.text_width(m) + 16;
            if mx + tw > w {
                break;
            }
            if i == sel {
                c.fill_rect(Rect::new(mx, 0, tw, HEIGHT), 0xFF285577);
            }
            c.draw_text(&font, mx + 8, 3, m, if i == sel { 0xFFFFFFFF } else { 0xFFBBBBBB }, None);
            mx += tw;
        }
        d.put_canvas(win, &c, c.bounds(), 0, 0);
        d.flush();
        let Some(ev) = d.wait_event(-1) else { return 0 };
        match ev {
            Event::Key { code, pressed: true, ch, .. } => match code {
                keymap::ESC => return 0,
                keymap::ENTER | keymap::KP_ENTER => {
                    let cmd = if input.contains(' ') || matches.is_empty() { input.clone() } else { matches[sel].clone() };
                    if !cmd.trim().is_empty() {
                        if !spawn(&cmd) {
                            spawn(&huldra_user::format!("term -e {}", cmd));
                        }
                    }
                    return 0;
                }
                keymap::TAB | keymap::RIGHT => {
                    if code == keymap::TAB && matches.len() == 1 {
                        input = matches[0].clone();
                    } else {
                        sel += 1;
                    }
                }
                keymap::LEFT => sel = sel.saturating_sub(1),
                keymap::BACKSPACE => {
                    input.pop();
                    sel = 0;
                }
                _ => {
                    if let Some(c) = char::from_u32(ch).filter(|c| !c.is_control()) {
                        input.push(c);
                        sel = 0;
                    }
                }
            },
            Event::Button { pressed: true, .. } => {}
            Event::Focus { focused: false, .. } => return 0,
            _ => {}
        }
    }
}
