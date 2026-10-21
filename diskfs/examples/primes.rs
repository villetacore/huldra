// The sieve of Eratosthenes: rustc -run primes.rs 100
fn sieve(n: usize) -> Vec<usize> {
    let mut composite = vec![false; n + 1];
    let mut primes = Vec::new();
    for i in 2..=n {
        if !composite[i] {
            primes.push(i);
            let mut j = i * i;
            while j <= n {
                composite[j] = true;
                j += i;
            }
        }
    }
    primes
}

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(50);
    let primes = sieve(n);
    println!("{} primes up to {}:", primes.len(), n);
    let line: Vec<String> = primes.iter().map(|p| p.to_string()).collect();
    println!("{}", line.join(" "));
}
