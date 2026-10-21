// Strings, chars, parsing and formatting.

fn main() {
    let mut s = String::from("Hello");
    s.push_str(", world");
    s.push('!');
    println!("{} ({} bytes)", s, s.len());
    println!("{}", s.to_uppercase());
    println!("{}", s.to_lowercase());
    println!("{}", s.replace("l", "L"));
    println!("{:?}", s.find("world"));
    println!("{:?}", s.find('z'));
    println!("{}", s.contains("lo, w"));
    println!("{}", s.starts_with("Hell"));
    println!("{}", s.ends_with('!'));
    println!("{}", &s[7..12]);
    let words: Vec<&str> = "  the quick  brown fox ".split_whitespace().collect();
    println!("{:?}", words);
    let parts: Vec<&str> = "a,b,,c".split(',').collect();
    println!("{:?}", parts);
    println!("{:?}", "k=v=w".split_once('='));
    println!("{:?}", "one two three".splitn(2, ' ').collect::<Vec<_>>());
    println!("[{}]", "  padded  ".trim());
    println!("[{}]", "xxabcxx".trim_matches('x'));
    println!("{:?}", "line1\nline2\r\nline3".lines().collect::<Vec<_>>());
    let rev: String = "stressed".chars().rev().collect();
    println!("{}", rev);
    let caps: String = "hello world".split(' ').map(|w| {
        let mut c = w.chars();
        match c.next() {
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            None => String::new(),
        }
    }).collect::<Vec<_>>().join(" ");
    println!("{}", caps);
    let text = "Привет, мир";
    println!("{} chars, {} bytes", text.chars().count(), text.len());
    for (i, c) in text.char_indices().take(3) {
        println!("{} {}", i, c);
    }
    println!("{:?}", text.chars().nth(8));
    let n: i32 = "  -42 ".trim().parse().unwrap();
    println!("{}", n + 1);
    println!("{:?}", "3.75".parse::<f64>());
    println!("{:?}", "abc".parse::<i32>());
    println!("{:?}", "".parse::<u8>());
    println!("{:?}", "300".parse::<u8>());
    println!("{:?}", "true".parse::<bool>());
    let num = 255;
    println!("{} {:x} {:X} {:o} {:b} {:#x} {:#b}", num, num, num, num, num, num, num);
    println!("[{:5}] [{:<5}] [{:^5}] [{:>5}] [{:05}]", 42, 42, 42, 42, 42);
    println!("[{:*^9}] [{:-<6}] [{:+}]", "mid", "ab", 7);
    println!("[{:8.3}] [{:<8.2}] [{:e}] [{:.0}]", 3.14159, 2.71828, 1234.5, 2.5);
    println!("{} {} {} {}", 1.0, 0.1 + 0.2, 1e21, 1.5e-7);
    println!("{:?} {:?} {:?} {:?}", 1.0, 0.1f32, 1e16, -0.0);
    println!("{} {} {}", f64::NAN, f64::INFINITY, -f64::INFINITY);
    println!("{} {}", i64::MAX, i64::MIN);
    println!("{} {}", u64::MAX, i8::MIN);
    let width = 10;
    let prec = 2;
    println!("[{:width$.prec$}]", 1.23456);
    println!("[{:>1$}]", "x", 4);
    println!("{0} {1} {0}", "a", "b");
    let name = "Ann";
    println!("{name} is {age} years", age = 30);
    println!("{:?}", "quote\"tab\t");
    println!("{:?}", 'a');
    println!("{:?}", '\'');
    let c = 'ß';
    println!("{} {} {}", c.is_alphabetic(), c.len_utf8(), c as u32);
    println!("{}", 'Z'.to_ascii_lowercase());
    println!("{:?}", '7'.to_digit(10));
    println!("{:?}", std::char::from_digit(5, 10));
    let bytes = "AZ".as_bytes();
    println!("{:?}", bytes);
    let from = String::from_utf8(vec![104, 105]).unwrap();
    println!("{}", from);
    let mut t = String::new();
    for i in 0..3 {
        t += &format!("{}-", i);
    }
    t.pop();
    println!("{}", t);
    t.insert(0, '[');
    t.insert_str(t.len(), "]");
    println!("{} {}", t, t.is_empty());
    println!("{}", "ab".repeat(3));
    println!("{}", "Hello".eq_ignore_ascii_case("hELLO"));
    let csv = "name,age\nbob,31\namy,27";
    for line in csv.lines().skip(1) {
        let fields: Vec<&str> = line.split(',').collect();
        let age: u32 = fields[1].parse().unwrap();
        println!("{:>6}|{:<4}|", fields[0], age);
    }
    let s2 = s.clone();
    println!("{}", s == s2);
    println!("{}", "apple" < "banana");
    println!("{:?}", "a-b_c".split(|c: char| c == '-' || c == '_').collect::<Vec<_>>());
    println!("{}", "x".to_string() + "y" + &"z".to_string());
    let opt: Option<String> = Some("hi".into());
    println!("{:?} {:?}", opt, opt.as_deref());
    println!("{:>width$}", "right", width = 8);
    println!("{:#?}", (1, "two", [3.0, 4.5]));
}
