//! paint: draw with the mouse. Left button paints, right button erases;
//! pick colors and brush sizes from the tool bar. "Save" writes
//! ~/drawing.ppm.

#![no_std]
#![no_main]

use huldra_user::gui::*;
use huldra_user::{env, eprintln, format, fs, Vec};

huldra_user::main!(main);

const BAR: i32 = 34;
const COLORS: [u32; 12] = [0xFF000000, 0xFFFFFFFF, 0xFF808080, 0xFFE53935, 0xFFFB8C00, 0xFFFDD835, 0xFF43A047, 0xFF00ACC1, 0xFF1E88E5, 0xFF8E24AA, 0xFF6D4C41, 0xFFF48FB1];
const SIZES: [i32; 4] = [1, 3, 7, 13];

struct Paint {
    d: Display,
    win: u32,
    font: Font,
    w: i32,
    h: i32,
    image: Canvas,
    color: u32,
    size: i32,
    last: Option<(i32, i32)>,
}

impl Paint {
    fn swatch(i: usize) -> Rect {
        Rect::new(6 + i as i32 * 24, 6, 20, 20)
    }

    fn size_box(i: usize) -> Rect {
        Rect::new(6 + 12 * 24 + 10 + i as i32 * 24, 6, 20, 20)
    }

    fn button(&self, i: i32) -> Rect {
        Rect::new(self.w - 6 - (i + 1) * 60, 6, 54, 20)
    }

    fn draw_bar(&mut self) {
        let mut c = Canvas::new(self.w, BAR);
        c.gradient(c.bounds(), 0xFFE8E8E8, 0xFFCCCCCC);
        c.fill_rect(Rect::new(0, BAR - 1, self.w, 1), 0xFF909090);
        for (i, &col) in COLORS.iter().enumerate() {
            let r = Paint::swatch(i);
            c.fill_rect(r, col);
            c.rect_outline(r.inset(-1), if col == self.color { 0xFF000000 } else { 0xFF9A9A9A });
            if col == self.color {
                c.rect_outline(r.inset(-2), 0xFF1E88E5);
            }
        }
        for (i, &s) in SIZES.iter().enumerate() {
            let r = Paint::size_box(i);
            c.fill_rect(r, if s == self.size { 0xFFB0C8E8 } else { 0xFFF6F6F6 });
            c.rect_outline(r, 0xFF9A9A9A);
            c.fill_circle(r.x + 10, r.y + 10, (s / 2).max(1), 0xFF202020);
        }
        for (i, label) in ["Clear", "Save"].iter().enumerate() {
            let r = self.button(i as i32);
            c.fill_round_rect(r, 4, 0xFFF6F6F6);
            c.rect_outline(r, 0xFF9A9A9A);
            c.draw_text(&self.font, r.x + (r.w - self.font.text_width(label)) / 2, r.y + 2, label, 0xFF202020, None);
        }
        let win = self.win;
        self.d.put_canvas(win, &c, c.bounds(), 0, 0);
    }

    fn show_image(&mut self, r: Rect) {
        let win = self.win;
        let r = r.intersect(&self.image.bounds());
        if !r.is_empty() {
            self.d.put_canvas(win, &self.image, r, r.x, r.y + BAR);
        }
    }

    fn stroke(&mut self, x: i32, y: i32, erase: bool) {
        let color = if erase { 0xFFFFFFFF } else { self.color };
        let size = if erase { self.size.max(9) } else { self.size };
        let (x0, y0) = self.last.unwrap_or((x, y));
        self.image.thick_line(x0, y0, x, y, size, color);
        self.last = Some((x, y));
        let pad = size + 2;
        let r = Rect::new(x0.min(x) - pad, y0.min(y) - pad, (x0 - x).abs() + 2 * pad, (y0 - y).abs() + 2 * pad);
        self.show_image(r);
        self.d.flush();
    }

    fn save(&mut self) -> huldra_user::Result<()> {
        let path = format!("{}/drawing.ppm", env::var("HOME").unwrap_or("/root"));
        let mut data: Vec<u8> = format!("P6\n{} {}\n255\n", self.image.width, self.image.height).into_bytes();
        for p in &self.image.pixels {
            data.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
        }
        fs::write(&path, &data)?;
        let title = format!("Paint - saved {}", path);
        let win = self.win;
        self.d.set_title(win, &title);
        Ok(())
    }
}

fn main() -> i32 {
    let mut d = match Display::connect() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("paint: {}", e);
            return 1;
        }
    };
    let (w, h) = (640, 480);
    let win = d.create_window(0, 0, w, h, KIND_NORMAL);
    d.set_title(win, "Paint");
    d.map(win);
    let mut image = Canvas::new(w, h - BAR);
    image.fill_rect(image.bounds(), 0xFFFFFFFF);
    let mut p = Paint { d, win, font: load_font(), w, h, image, color: 0xFF000000, size: 3, last: None };
    while let Some(ev) = p.d.wait_event(-1) {
        match ev {
            Event::Expose { w, h, .. } => {
                p.w = w;
                p.h = h;
                p.image.resize(w, (h - BAR).max(1), 0xFFFFFFFF);
                p.draw_bar();
                let b = p.image.bounds();
                p.show_image(b);
                p.d.flush();
            }
            Event::Button { x, y, button, pressed: true, .. } if button == BUTTON_LEFT || button == BUTTON_RIGHT => {
                if y < BAR {
                    if let Some(i) = (0..COLORS.len()).find(|&i| Paint::swatch(i).contains(x, y)) {
                        p.color = COLORS[i];
                    } else if let Some(i) = (0..SIZES.len()).find(|&i| Paint::size_box(i).contains(x, y)) {
                        p.size = SIZES[i];
                    } else if p.button(0).contains(x, y) {
                        p.image.fill_rect(p.image.bounds(), 0xFFFFFFFF);
                        let b = p.image.bounds();
                        p.show_image(b);
                    } else if p.button(1).contains(x, y) {
                        if let Err(e) = p.save() {
                            eprintln!("paint: save: {}", e);
                        }
                    }
                    p.draw_bar();
                    p.d.flush();
                } else {
                    p.last = None;
                    p.stroke(x, y - BAR, button == BUTTON_RIGHT);
                }
            }
            Event::Motion { x, y, buttons, .. } => {
                if buttons & (BUTTON_LEFT | BUTTON_RIGHT) != 0 && p.last.is_some() {
                    p.stroke(x, y - BAR, buttons & BUTTON_RIGHT != 0);
                }
            }
            Event::Button { pressed: false, .. } => p.last = None,
            Event::CloseRequest { .. } => return 0,
            _ => {}
        }
    }
    0
}
