use std::io::{self, BufRead, Read, Write};
fn main() {
    let stdin = io::stdin();
    let mut first = String::new();
    stdin.read_line(&mut first).unwrap();
    let n: usize = first.trim().parse().unwrap();
    let mut total = 0i64;
    let mut lines = stdin.lock().lines();
    for _ in 0..n {
        let line = lines.next().unwrap().unwrap();
        let nums: Vec<i64> = line.split_whitespace().map(|x| x.parse().unwrap()).collect();
        total += nums.iter().sum::<i64>();
        println!("line sum {}", nums.iter().sum::<i64>());
    }
    drop(lines);
    let mut rest = String::new();
    io::stdin().read_to_string(&mut rest).unwrap();
    let words = rest.split_whitespace().count();
    let out = io::stdout();
    let mut w = io::BufWriter::new(out.lock());
    writeln!(w, "total {}", total).unwrap();
    writeln!(w, "{} more words", words).unwrap();
    w.flush().unwrap();
}
