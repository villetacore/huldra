// Word frequencies of a file (or standard input):
//   rustc -run wordfreq.rs /usr/share/huldra/docs/overview.md
use std::collections::HashMap;
use std::io::Read;

fn main() {
    let text = match std::env::args().nth(1) {
        Some(path) => std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("{}: {}", path, e);
            std::process::exit(1);
        }),
        None => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).unwrap();
            s
        }
    };
    let mut counts: HashMap<String, usize> = HashMap::new();
    for word in text.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2) {
        *counts.entry(word.to_lowercase()).or_insert(0) += 1;
    }
    let mut top: Vec<(String, usize)> = counts.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for (word, n) in top.iter().take(10) {
        println!("{:>5} {}", n, word);
    }
}
