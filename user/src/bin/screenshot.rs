//! screenshot [-d SECONDS] [FILE]: saves the screen as a PPM image
//! (default ~/screenshot.ppm).

#![no_std]
#![no_main]

use huldra_user::gui::*;
use huldra_user::{env, eprintln, format, fs, println, time, Vec};

huldra_user::main!(main);

fn main() -> i32 {
    let args = env::args();
    let mut delay = 0;
    let mut file = format!("{}/screenshot.ppm", env::var("HOME").unwrap_or("/root"));
    let mut i = 1;
    while i < args.len() {
        if args[i] == "-d" {
            i += 1;
            delay = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(0);
        } else {
            file = args[i].clone();
        }
        i += 1;
    }
    time::sleep_ms(delay * 1000);
    let mut d = match Display::connect() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("screenshot: cannot connect to the display: {}", e);
            return 1;
        }
    };
    let (w, h) = (d.width, d.height);
    d.send(Request::GetImage { id: ROOT, x: 0, y: 0, w, h });
    loop {
        match d.wait_event(10_000) {
            Some(Event::ImageData { w, h, pixels }) => {
                let mut out: Vec<u8> = format!("P6\n{} {}\n255\n", w, h).into_bytes();
                for p in pixels {
                    out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
                }
                if let Err(e) = fs::write(&file, &out) {
                    eprintln!("screenshot: {}: {}", file, e);
                    return 1;
                }
                println!("saved {}x{} to {}", w, h, file);
                return 0;
            }
            Some(_) => {}
            None => {
                eprintln!("screenshot: no reply from the display");
                return 1;
            }
        }
    }
}
