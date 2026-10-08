//! 8x16 bitmap fonts in the VGA layout (256 glyphs of 16 bytes, code page
//! 437), as read from `/dev/font`.

use alloc::vec::Vec;

pub struct Font {
    pub width: i32,
    pub height: i32,
    data: Vec<u8>,
}

/// Unicode characters with a CP437 glyph (besides ASCII).
const CP437_HIGH: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ', 'Æ', 'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', 'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐', '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧', '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐', '▀', 'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩', '≡', '±', '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{a0}',
];

impl Font {
    /// A font from VGA-layout data (`len / 256` bytes per glyph).
    pub fn from_vga(data: Vec<u8>) -> Font {
        let height = (data.len() / 256).max(1) as i32;
        Font { width: 8, height, data }
    }

    /// A crude fallback when no font is available: boxes for everything
    /// but space.
    pub fn fallback() -> Font {
        let mut data = alloc::vec![0u8; 4096];
        for c in 33..127usize {
            for row in 3..13 {
                data[c * 16 + row] = if row == 3 || row == 12 { 0x7E } else { 0x42 };
            }
        }
        Font::from_vga(data)
    }

    pub fn index(ch: char) -> usize {
        let c = ch as u32;
        if (0x20..0x7F).contains(&c) {
            return c as usize;
        }
        if let Some(i) = CP437_HIGH.iter().position(|&h| h == ch) {
            return 128 + i;
        }
        match ch {
            '•' => 7,
            '◘' => 8,
            '○' => 9,
            '♪' => 13,
            '►' | '▶' => 16,
            '◄' | '◀' => 17,
            '↕' => 18,
            '↑' => 24,
            '↓' => 25,
            '→' => 26,
            '←' => 27,
            '▲' => 30,
            '▼' => 31,
            '⌂' => 127,
            '─' | '━' => 196,
            '│' | '┃' => 179,
            _ => b'?' as usize,
        }
    }

    pub fn glyph(&self, ch: char) -> &[u8] {
        let h = self.height as usize;
        let i = Font::index(ch);
        self.data.get(i * h..(i + 1) * h).unwrap_or(&[])
    }

    pub fn text_width(&self, text: &str) -> i32 {
        text.chars().count() as i32 * self.width
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping() {
        assert_eq!(Font::index('A'), 65);
        assert_eq!(Font::index('┌'), 218);
        assert_eq!(Font::index('█'), 219);
        assert_eq!(Font::index('é'), 130);
        assert_eq!(Font::index('漢'), b'?' as usize);
        let f = Font::fallback();
        assert_eq!(f.glyph('A').len(), 16);
        assert_eq!(f.text_width("abc"), 24);
    }
}
