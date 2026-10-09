//! The software mouse cursor.

use huldra_gfx::canvas::{Canvas, Rect};
use huldra_gfx::theme;

#[rustfmt::skip]
const ARROW: [&str; 19] = [
    "X           ",
    "XX          ",
    "X.X         ",
    "X..X        ",
    "X...X       ",
    "X....X      ",
    "X.....X     ",
    "X......X    ",
    "X.......X   ",
    "X........X  ",
    "X.........X ",
    "X......XXXXX",
    "X...X..X    ",
    "X..XX..X    ",
    "X.X  X..X   ",
    "XX   X..X   ",
    "X     X..X  ",
    "      X..X  ",
    "       XX   ",
];

pub fn rect(x: i32, y: i32) -> Rect {
    Rect::new(x, y, 12, 19)
}

pub fn draw(c: &mut Canvas, x: i32, y: i32) {
    for (row, line) in ARROW.iter().enumerate() {
        for (col, p) in line.bytes().enumerate() {
            let color = match p {
                b'X' => theme::BG,
                b'.' => theme::BRIGHT,
                _ => continue,
            };
            c.put(x + col as i32, y + row as i32, color);
        }
    }
}
