//! paint: draw with the mouse. Left button paints, right button erases;
//! pick colors and brush sizes from the tool bar. "Save" writes
//! ~/drawing.ppm.

#![no_std]
#![no_main]

use huldra_user::gui::*;
use huldra_user::{env, eprintln, format, fs, Vec};

huldra_user::main!(main);

const BAR: i32 = 34;
/// The paper: the screen's own glass color, so pictures match the desktop.
const PAPER: u32 = theme::BG;
const COLORS: [u32; 12] = [theme::BG, theme::BRIGHT, theme::ACCENT, theme::AMBER, theme::RUST, theme::TEAL, 0xFF2E7D4F, 0xFF8A6A3A, 0xFFE0E0D0, 0xFF808880, 0xFF4F7FC0, 0xFFC05A8A];
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
        c.fill_rect(c.bounds(), theme::SURFACE);
        c.fill_rect(Rect::new(0, BAR - 1, self.w, 1), theme::LINE);
        for (i, &col) in COLORS.iter().enumerate() {
            let r = Paint::swatch(i);
            c.fill_rect(r, col);
            c.rect_outline(r.inset(-1), if col == self.color { theme::BRIGHT } else { theme::LINE_DIM });
            if col == self.color {
                c.rect_outline(r.inset(-2), theme::ACCENT);
            }
        }
        for (i, &s) in SIZES.iter().enumerate() {
            let r = Paint::size_box(i);
            let hot = s == self.size;
            c.fill_rect(r, if hot { theme::ACCENT } else { theme::RAISED });
            c.rect_outline(r, theme::LINE);
            c.fill_circle(r.x + 10, r.y + 10, (s / 2).max(1), if hot { theme::BG } else { theme::TEXT });
        }
        for (i, label) in ["Clear", "Save"].iter().enumerate() {
            let r = self.button(i as i32);
            theme::button(&mut c, &self.font, r, label, false);
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
        let color = if erase { PAPER } else { self.color };
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
    image.fill_rect(image.bounds(), PAPER);
    let mut p = Paint { d, win, font: load_font(), w, h, image, color: theme::ACCENT, size: 3, last: None };
    while let Some(ev) = p.d.wait_event(-1) {
        match ev {
            Event::Expose { w, h, .. } => {
                p.w = w;
                p.h = h;
                p.image.resize(w, (h - BAR).max(1), PAPER);
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
                        p.image.fill_rect(p.image.bounds(), PAPER);
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
