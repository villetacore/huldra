//! cal [[month] year]: a calendar (current month by default).

#![no_std]
#![no_main]

use huldra_user::time::{self, DateTime};
use huldra_user::{env, print, println};

huldra_user::main!(main);

const NAMES: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

fn days_in(month: u32, year: i64) -> u32 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Day of week (0 = Sunday) of the 1st of the month (Zeller-style).
fn first_weekday(month: u32, year: i64) -> u32 {
    let t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if month < 3 { year - 1 } else { year };
    ((y + y / 4 - y / 100 + y / 400 + t[(month - 1) as usize] + 1).rem_euclid(7)) as u32
}

fn main() -> i32 {
    let now = DateTime::from_unix(time::now());
    let args = &env::args()[1..];
    let (month, year) = match args.len() {
        1 => (0, args[0].parse().unwrap_or(now.year)),
        2 => (args[0].parse().unwrap_or(now.month), args[1].parse().unwrap_or(now.year)),
        _ => (now.month, now.year),
    };
    let months: huldra_user::Vec<u32> = if month == 0 { (1..=12).collect() } else { huldra_user::Vec::from([month]) };
    for m in months {
        let title = huldra_user::format!("{} {}", NAMES[(m - 1) as usize], year);
        println!("{:^20}", title);
        println!("Su Mo Tu We Th Fr Sa");
        let start = first_weekday(m, year);
        for _ in 0..start {
            print!("   ");
        }
        for d in 1..=days_in(m, year) {
            let today = m == now.month && year == now.year && d == now.day;
            if today {
                print!("\x1b[7m{:>2}\x1b[0m", d);
            } else {
                print!("{:>2}", d);
            }
            if (start + d) % 7 == 0 {
                println!();
            } else {
                print!(" ");
            }
        }
        println!("\n");
    }
    0
}
