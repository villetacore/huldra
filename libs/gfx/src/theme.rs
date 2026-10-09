//! The system look: an old phosphor CRT terminal. Not black and pure
//! green: the "glass" is a dark green-grey, text is a soft phosphor green
//! with a brighter "hot" shade for emphasis, and warm amber and rust are
//! the only other accents (warnings, the close button, highlights).
//!
//! Shared by the display server, window managers, the panel, the
//! terminal and applications, so the whole session looks the same.

use crate::canvas::{mix, Canvas, Rect};

/// The glass: desktop and terminal background.
pub const BG: u32 = 0xFF0C1712;
/// Window bodies and the panel.
pub const SURFACE: u32 = 0xFF112019;
/// Buttons, fields, title bars.
pub const RAISED: u32 = 0xFF172B21;
/// Hover on raised elements.
pub const HOVER: u32 = 0xFF1F3B2C;
/// Faint lines: separators, inactive borders, grids.
pub const LINE_DIM: u32 = 0xFF26402F;
/// Normal lines: borders of the active window, outlines.
pub const LINE: u32 = 0xFF3E8A58;
/// Secondary text.
pub const TEXT_DIM: u32 = 0xFF5C9A70;
/// Normal text.
pub const TEXT: u32 = 0xFF96E6A8;
/// Emphasis: titles, the cursor, selected items.
pub const BRIGHT: u32 = 0xFFCCFFD6;
/// The phosphor itself: selections (drawn under `BG`-colored text).
pub const ACCENT: u32 = 0xFF4BD97C;
/// Warm accent: status, warnings, numbers.
pub const AMBER: u32 = 0xFFF0B450;
/// Danger: close, errors.
pub const RUST: u32 = 0xFFD9694C;
/// A cool accent for variety (directories, links).
pub const TEAL: u32 = 0xFF5CC6AE;

/// A box in the terminal style: filled `SURFACE`, with a 1-pixel outline.
pub fn panel(c: &mut Canvas, r: Rect, outline: u32) {
    c.fill_rect(r, SURFACE);
    c.rect_outline(r, outline);
}

/// A button: raised fill, outline, centered label; `hot` inverts it like a
/// selected item in a terminal menu.
pub fn button(c: &mut Canvas, font: &crate::Font, r: Rect, label: &str, hot: bool) {
    let (bg, fg, line) = if hot { (ACCENT, BG, ACCENT) } else { (RAISED, TEXT, LINE) };
    c.fill_rect(r, bg);
    c.rect_outline(r, line);
    let tx = r.x + (r.w - font.text_width(label)) / 2;
    let ty = r.y + (r.h - font.height) / 2;
    c.draw_text(font, tx, ty, label, fg, None);
}

/// Faint horizontal scanlines over `r`, every other row darkened a little.
pub fn scanlines(c: &mut Canvas, r: Rect) {
    let r = r.intersect(&c.clip);
    let mut y = r.y + (r.y & 1 ^ 1);
    while y < r.bottom() {
        for x in r.x..r.right() {
            let p = c.get(x, y);
            c.put(x, y, darken(p));
        }
        y += 2;
    }
}

/// The color 1/8 darker (the scanline shade).
#[inline]
pub fn darken(p: u32) -> u32 {
    p - ((p >> 3) & 0x001F_1F1F)
}

/// A soft glow: `c` mixed toward `BG` by `t` (0..=255).
pub fn dim(c: u32, t: u32) -> u32 {
    mix(c, BG, t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn darken_keeps_alpha() {
        assert_eq!(darken(0xFF808080), 0xFF707070);
        assert_eq!(darken(0xFF000000), 0xFF000000);
        assert_eq!(darken(0xFFFFFFFF) >> 24, 0xFF);
    }
}
