#![no_std]
#![no_main]

use huldra_user::time::{self, DateTime, MONTHS, WEEKDAYS};
use huldra_user::{env, println};

huldra_user::main!(main);

fn main() -> i32 {
    let t = time::now();
    if env::args().get(1).map(|a| a.as_str()) == Some("+%s") {
        println!("{}", t);
        return 0;
    }
    let d = DateTime::from_unix(t);
    println!(
        "{} {} {:>2} {:02}:{:02}:{:02} UTC {}",
        WEEKDAYS[d.weekday as usize],
        MONTHS[(d.month - 1) as usize],
        d.day,
        d.hour,
        d.minute,
        d.second,
        d.year
    );
    0
}
