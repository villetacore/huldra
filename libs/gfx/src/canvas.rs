//! Pixel buffers (32-bit 0xAARRGGBB, alpha only used for blending) and
//! drawing primitives.

use crate::font::Font;
use alloc::vec;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }

    pub fn intersect(&self, o: &Rect) -> Rect {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = self.right().min(o.right());
        let b = self.bottom().min(o.bottom());
        Rect::new(x, y, (r - x).max(0), (b - y).max(0))
    }

    /// Smallest rectangle containing both (empty ones are ignored).
    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect::new(x, y, self.right().max(o.right()) - x, self.bottom().max(o.bottom()) - y)
    }

    pub fn offset(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    pub fn inset(&self, d: i32) -> Rect {
        Rect::new(self.x + d, self.y + d, self.w - 2 * d, self.h - 2 * d)
    }
}

pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    0xFF00_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32
}

/// `src` over `dst` with `alpha` (0..=255).
pub fn blend(dst: u32, src: u32, alpha: u32) -> u32 {
    let inv = 255 - alpha;
    let ch = |shift: u32| ((((src >> shift) & 0xFF) * alpha + ((dst >> shift) & 0xFF) * inv) / 255) << shift;
    0xFF00_0000 | ch(16) | ch(8) | ch(0)
}

/// Mixes two colors (t = 0..=255 from a to b).
pub fn mix(a: u32, b: u32, t: u32) -> u32 {
    blend(a, b, t.min(255))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Canvas {
    pub width: i32,
    pub height: i32,
    pub pixels: Vec<u32>,
    /// Drawing is limited to this rectangle.
    pub clip: Rect,
}

impl Canvas {
    pub fn new(width: i32, height: i32) -> Canvas {
        let (w, h) = (width.max(0), height.max(0));
        Canvas { width: w, height: h, pixels: vec![0xFF00_0000; (w * h) as usize], clip: Rect::new(0, 0, w, h) }
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }

    pub fn reset_clip(&mut self) {
        self.clip = self.bounds();
    }

    pub fn set_clip(&mut self, r: Rect) {
        self.clip = r.intersect(&self.bounds());
    }

    /// Resizes, keeping the overlapping top-left part.
    pub fn resize(&mut self, width: i32, height: i32, fill: u32) {
        let mut n = Canvas::new(width, height);
        n.fill_rect(n.bounds(), fill);
        n.blit(self, self.bounds(), 0, 0);
        *self = n;
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return 0;
        }
        self.pixels[(y * self.width + x) as usize]
    }

    #[inline]
    pub fn put(&mut self, x: i32, y: i32, c: u32) {
        if self.clip.contains(x, y) {
            self.pixels[(y * self.width + x) as usize] = c;
        }
    }

    pub fn put_alpha(&mut self, x: i32, y: i32, c: u32, alpha: u32) {
        if self.clip.contains(x, y) {
            let i = (y * self.width + x) as usize;
            self.pixels[i] = blend(self.pixels[i], c, alpha);
        }
    }

    pub fn fill_rect(&mut self, r: Rect, c: u32) {
        let r = r.intersect(&self.clip);
        if r.is_empty() {
            return;
        }
        for y in r.y..r.bottom() {
            let start = (y * self.width + r.x) as usize;
            self.pixels[start..start + r.w as usize].fill(c);
        }
    }

    /// Translucent fill: `alpha` 0..=255.
    pub fn fill_rect_alpha(&mut self, r: Rect, c: u32, alpha: u32) {
        let r = r.intersect(&self.clip);
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let i = (y * self.width + x) as usize;
                self.pixels[i] = blend(self.pixels[i], c, alpha);
            }
        }
    }

    /// Vertical gradient from `top` to `bottom`.
    pub fn gradient(&mut self, r: Rect, top: u32, bottom: u32) {
        let clipped = r.intersect(&self.clip);
        for y in clipped.y..clipped.bottom() {
            let t = if r.h <= 1 { 0 } else { ((y - r.y) * 255 / (r.h - 1)) as u32 };
            let c = mix(top, bottom, t);
            self.fill_rect(Rect::new(clipped.x, y, clipped.w, 1), c);
        }
    }

    pub fn rect_outline(&mut self, r: Rect, c: u32) {
        self.fill_rect(Rect::new(r.x, r.y, r.w, 1), c);
        self.fill_rect(Rect::new(r.x, r.bottom() - 1, r.w, 1), c);
        self.fill_rect(Rect::new(r.x, r.y, 1, r.h), c);
        self.fill_rect(Rect::new(r.right() - 1, r.y, 1, r.h), c);
    }

    /// 3D-looking frame: light top-left, dark bottom-right.
    pub fn bevel(&mut self, r: Rect, light: u32, dark: u32) {
        self.fill_rect(Rect::new(r.x, r.y, r.w, 1), light);
        self.fill_rect(Rect::new(r.x, r.y, 1, r.h), light);
        self.fill_rect(Rect::new(r.x, r.bottom() - 1, r.w, 1), dark);
        self.fill_rect(Rect::new(r.right() - 1, r.y, 1, r.h), dark);
    }

    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: u32) {
        let (mut x, mut y) = (x0, y0);
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            self.put(x, y, c);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// A line `width` pixels thick.
    pub fn thick_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, width: i32, c: u32) {
        let r = width / 2;
        for oy in -r..=r {
            for ox in -r..=r {
                if ox * ox + oy * oy <= r * r + 1 {
                    self.line(x0 + ox, y0 + oy, x1 + ox, y1 + oy, c);
                }
            }
        }
    }

    pub fn fill_circle(&mut self, cx: i32, cy: i32, radius: i32, c: u32) {
        for y in -radius..=radius {
            let w = isqrt(radius * radius - y * y);
            self.fill_rect(Rect::new(cx - w, cy + y, 2 * w + 1, 1), c);
        }
    }

    pub fn circle(&mut self, cx: i32, cy: i32, radius: i32, c: u32) {
        let (mut x, mut y, mut err) = (radius, 0, 1 - radius);
        while x >= y {
            for (px, py) in [(x, y), (y, x), (-y, x), (-x, y), (-x, -y), (-y, -x), (y, -x), (x, -y)] {
                self.put(cx + px, cy + py, c);
            }
            y += 1;
            if err < 0 {
                err += 2 * y + 1;
            } else {
                x -= 1;
                err += 2 * (y - x) + 1;
            }
        }
    }

    /// Rectangle with rounded corners of radius `rad`.
    pub fn fill_round_rect(&mut self, r: Rect, rad: i32, c: u32) {
        let rad = rad.min(r.w / 2).min(r.h / 2).max(0);
        for y in 0..r.h {
            let inset = if y < rad {
                rad - isqrt(rad * rad - (rad - y) * (rad - y))
            } else if y >= r.h - rad {
                let d = y - (r.h - rad - 1);
                rad - isqrt(rad * rad - d * d)
            } else {
                0
            };
            self.fill_rect(Rect::new(r.x + inset, r.y + y, r.w - 2 * inset, 1), c);
        }
    }

    /// Copies `src_rect` of `src` to (`dx`, `dy`).
    pub fn blit(&mut self, src: &Canvas, src_rect: Rect, dx: i32, dy: i32) {
        let src_rect = src_rect.intersect(&src.bounds());
        let dst = Rect::new(dx, dy, src_rect.w, src_rect.h).intersect(&self.clip);
        if dst.is_empty() {
            return;
        }
        let (ox, oy) = (src_rect.x + (dst.x - dx), src_rect.y + (dst.y - dy));
        for row in 0..dst.h {
            let s = ((oy + row) * src.width + ox) as usize;
            let d = ((dst.y + row) * self.width + dst.x) as usize;
            self.pixels[d..d + dst.w as usize].copy_from_slice(&src.pixels[s..s + dst.w as usize]);
        }
    }

    /// Copies a rectangle within the canvas (scrolling); overlap is fine.
    pub fn copy_within(&mut self, src: Rect, dx: i32, dy: i32) {
        let src = src.intersect(&self.bounds());
        let dst = Rect::new(dx, dy, src.w, src.h).intersect(&self.clip);
        if dst.is_empty() {
            return;
        }
        let (ox, oy) = (src.x + (dst.x - dx), src.y + (dst.y - dy));
        let rows: Vec<i32> = if dst.y > oy { (0..dst.h).rev().collect() } else { (0..dst.h).collect() };
        for row in rows {
            let s = ((oy + row) * self.width + ox) as usize;
            let d = ((dst.y + row) * self.width + dst.x) as usize;
            self.pixels.copy_within(s..s + dst.w as usize, d);
        }
    }

    /// Draws raw pixels (`w * h`, row-major) at (x, y).
    pub fn put_image(&mut self, x: i32, y: i32, w: i32, h: i32, data: &[u32]) {
        if w <= 0 || h <= 0 || data.len() < (w * h) as usize {
            return;
        }
        for row in 0..h {
            for col in 0..w {
                let p = data[(row * w + col) as usize];
                let a = p >> 24;
                if a == 0xFF {
                    self.put(x + col, y + row, p);
                } else if a != 0 {
                    self.put_alpha(x + col, y + row, p, a);
                }
            }
        }
    }

    pub fn draw_char(&mut self, font: &Font, x: i32, y: i32, ch: char, fg: u32, bg: Option<u32>) {
        let glyph = font.glyph(ch);
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..8 {
                let on = bits & (0x80 >> col) != 0;
                if on {
                    self.put(x + col, y + row as i32, fg);
                } else if let Some(b) = bg {
                    self.put(x + col, y + row as i32, b);
                }
            }
        }
    }

    /// Draws text; returns the x after the last character.
    pub fn draw_text(&mut self, font: &Font, x: i32, y: i32, text: &str, fg: u32, bg: Option<u32>) -> i32 {
        let mut cx = x;
        for ch in text.chars() {
            if cx >= self.clip.right() {
                break;
            }
            self.draw_char(font, cx, y, ch, fg, bg);
            cx += font.width;
        }
        cx
    }

    /// Bold text (drawn twice, one pixel apart).
    pub fn draw_text_bold(&mut self, font: &Font, x: i32, y: i32, text: &str, fg: u32) -> i32 {
        self.draw_text(font, x + 1, y, text, fg, None);
        self.draw_text(font, x, y, text, fg, None)
    }
}

pub fn isqrt(v: i32) -> i32 {
    if v <= 0 {
        return 0;
    }
    let mut x = v;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rects() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 10, 10);
        assert_eq!(a.intersect(&b), Rect::new(5, 5, 5, 5));
        assert_eq!(a.union(&b), Rect::new(0, 0, 15, 15));
        assert!(a.intersect(&Rect::new(20, 20, 1, 1)).is_empty());
        assert_eq!(Rect::default().union(&b), b);
    }

    #[test]
    fn drawing() {
        let mut c = Canvas::new(20, 10);
        c.fill_rect(Rect::new(-5, -5, 10, 10), 0xFFFF0000);
        assert_eq!(c.get(4, 4), 0xFFFF0000);
        assert_eq!(c.get(5, 5), 0xFF000000);
        c.set_clip(Rect::new(10, 0, 10, 10));
        c.fill_rect(c.bounds(), 0xFF00FF00);
        assert_eq!(c.get(9, 0), 0xFF000000);
        assert_eq!(c.get(10, 0), 0xFF00FF00);
        c.reset_clip();
        c.line(0, 9, 19, 9, 0xFFFFFFFF);
        assert!((0..20).all(|x| c.get(x, 9) == 0xFFFFFFFF));
        let mut d = Canvas::new(4, 4);
        d.blit(&c, Rect::new(8, 0, 4, 4), 0, 0);
        assert_eq!((d.get(1, 0), d.get(2, 0)), (0xFF000000, 0xFF00FF00));
        // Scrolling up by one row.
        let mut s = Canvas::new(3, 3);
        for y in 0..3 {
            s.fill_rect(Rect::new(0, y, 3, 1), y as u32);
        }
        s.copy_within(Rect::new(0, 1, 3, 2), 0, 0);
        assert_eq!((s.get(0, 0), s.get(0, 1), s.get(0, 2)), (1, 2, 2));
        s.copy_within(Rect::new(0, 0, 3, 2), 0, 1);
        assert_eq!((s.get(0, 1), s.get(0, 2)), (1, 2));
        assert_eq!(blend(0xFF000000, 0xFFFFFFFF, 255), 0xFFFFFFFF);
        assert_eq!(isqrt(17), 4);
    }
}
