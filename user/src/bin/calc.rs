//! calc: a desktop calculator (mouse or keyboard).

#![no_std]
#![no_main]

use huldra_gfx::keymap;
use huldra_user::gui::*;
use huldra_user::{eprintln, format, String};

huldra_user::main!(main);

const KEYS: [[&str; 4]; 5] = [["C", "+/-", "%", "/"], ["7", "8", "9", "*"], ["4", "5", "6", "-"], ["1", "2", "3", "+"], ["0", ".", "<", "="]];
const BW: i32 = 56;
const BH: i32 = 40;
const GAP: i32 = 6;
const TOP: i32 = 70;

struct Calc {
    display: String,
    acc: Option<f64>,
    op: Option<char>,
    fresh: bool,
    pressed: Option<(usize, usize)>,
}

fn format_num(v: f64) -> String {
    if !v.is_finite() {
        return String::from("Error");
    }
    if v == (v as i64) as f64 && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{:.10}", v);
    String::from(s.trim_end_matches('0').trim_end_matches('.'))
}

fn apply(a: f64, op: char, b: f64) -> f64 {
    match op {
        '+' => a + b,
        '-' => a - b,
        '*' => a * b,
        '/' => a / b,
        _ => b,
    }
}

impl Calc {
    fn value(&self) -> f64 {
        self.display.parse().unwrap_or(0.0)
    }

    fn press(&mut self, key: &str) {
        match key {
            "C" => {
                self.display = String::from("0");
                self.acc = None;
                self.op = None;
                self.fresh = true;
            }
            "<" => {
                if !self.fresh {
                    self.display.pop();
                    if self.display.is_empty() || self.display == "-" {
                        self.display = String::from("0");
                        self.fresh = true;
                    }
                }
            }
            "+/-" => {
                if self.display.starts_with('-') {
                    self.display.remove(0);
                } else if self.display != "0" {
                    self.display.insert(0, '-');
                }
            }
            "%" => self.display = format_num(self.value() / 100.0),
            "." => {
                if self.fresh {
                    self.display = String::from("0.");
                    self.fresh = false;
                } else if !self.display.contains('.') {
                    self.display.push('.');
                }
            }
            "+" | "-" | "*" | "/" | "=" => {
                let v = self.value();
                let result = match (self.acc, self.op) {
                    (Some(a), Some(op)) if !self.fresh => apply(a, op, v),
                    _ => v,
                };
                self.display = format_num(result);
                if key == "=" {
                    self.acc = None;
                    self.op = None;
                } else {
                    self.acc = Some(result);
                    self.op = key.chars().next();
                }
                self.fresh = true;
            }
            digit => {
                if self.fresh || self.display == "0" {
                    self.display = String::from(digit);
                    self.fresh = false;
                } else if self.display.len() < 16 {
                    self.display.push_str(digit);
                }
            }
        }
    }
}

fn button_rect(row: usize, col: usize) -> Rect {
    Rect::new(GAP + col as i32 * (BW + GAP), TOP + row as i32 * (BH + GAP), BW, BH)
}

fn draw(d: &mut Display, win: u32, font: &Font, calc: &Calc, w: i32, h: i32) {
    let mut c = Canvas::new(w, h);
    c.gradient(c.bounds(), 0xFF2B2F36, 0xFF1C1F24);
    let disp = Rect::new(GAP, GAP, w - 2 * GAP, TOP - 2 * GAP);
    c.fill_round_rect(disp, 6, 0xFFD8E4C8);
    if let Some(op) = calc.op {
        c.draw_text(font, disp.x + 8, disp.y + 6, &format!("{} {}", format_num(calc.acc.unwrap_or(0.0)), op), 0xFF607050, None);
    }
    let tw = font.text_width(&calc.display) * 2;
    // Big digits: draw each glyph scaled 2x.
    let mut small = Canvas::new(font.text_width(&calc.display), 16);
    small.fill_rect(small.bounds(), 0xFFD8E4C8);
    small.draw_text(font, 0, 0, &calc.display, 0xFF1A2410, None);
    let (x0, y0) = (disp.right() - tw - 10, disp.y + 22);
    for y in 0..32 {
        for x in 0..tw {
            c.put(x0 + x, y0 + y, small.get(x / 2, y / 2));
        }
    }
    for (r, row) in KEYS.iter().enumerate() {
        for (k, label) in row.iter().enumerate() {
            let b = button_rect(r, k);
            let op = "+-*/=".contains(label) && label.len() == 1;
            let (top, bottom) = if calc.pressed == Some((r, k)) {
                (0xFF3C4F6E, 0xFF2D3D57)
            } else if op {
                (0xFFF0A040, 0xFFD07F20)
            } else if label.chars().next().is_some_and(|ch| ch.is_ascii_digit()) || *label == "." {
                (0xFF5A606A, 0xFF454A52)
            } else {
                (0xFF7A808A, 0xFF62676F)
            };
            c.fill_round_rect(b, 6, bottom);
            c.gradient(Rect::new(b.x + 2, b.y + 1, b.w - 4, b.h / 2), top, bottom);
            let lw = font.text_width(label);
            c.draw_text_bold(font, b.x + (b.w - lw) / 2, b.y + (b.h - 16) / 2, label, 0xFFFFFFFF);
        }
    }
    d.put_canvas(win, &c, c.bounds(), 0, 0);
    d.flush();
}

fn main() -> i32 {
    let mut d = match Display::connect() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("calc: {}", e);
            return 1;
        }
    };
    let font = load_font();
    let w = 4 * BW + 5 * GAP;
    let h = TOP + 5 * (BH + GAP);
    let win = d.create_window(0, 0, w, h, KIND_NORMAL);
    d.set_title(win, "Calculator");
    d.map(win);
    let mut calc = Calc { display: String::from("0"), acc: None, op: None, fresh: true, pressed: None };
    let (mut cw, mut ch) = (w, h);
    while let Some(ev) = d.wait_event(-1) {
        match ev {
            Event::Expose { w, h, .. } => {
                cw = w;
                ch = h;
            }
            Event::Button { x, y, button: BUTTON_LEFT, pressed, .. } => {
                let hit = (0..5).flat_map(|r| (0..4).map(move |k| (r, k))).find(|&(r, k)| button_rect(r, k).contains(x, y));
                if pressed {
                    calc.pressed = hit;
                } else {
                    if let (Some(h), Some(p)) = (hit, calc.pressed) {
                        if h == p {
                            calc.press(KEYS[h.0][h.1]);
                        }
                    }
                    calc.pressed = None;
                }
            }
            Event::Key { code, pressed: true, ch: c, .. } => {
                let key = match (code, char::from_u32(c).unwrap_or('\0')) {
                    (keymap::ENTER | keymap::KP_ENTER, _) | (_, '=') => "=",
                    (keymap::BACKSPACE, _) => "<",
                    (keymap::ESC, _) => "C",
                    (_, '+') => "+",
                    (_, '-') => "-",
                    (_, '*') => "*",
                    (_, '/') => "/",
                    (_, '.') | (_, ',') => ".",
                    (_, '%') => "%",
                    (_, ch @ '0'..='9') => KEYS.iter().flatten().find(|k| k.starts_with(ch)).copied().unwrap_or(""),
                    _ => "",
                };
                if !key.is_empty() {
                    calc.press(key);
                }
            }
            Event::CloseRequest { .. } => return 0,
            _ => continue,
        }
        draw(&mut d, win, &font, &calc, cw, ch);
    }
    0
}
