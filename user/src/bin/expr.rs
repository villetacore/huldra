//! expr: evaluate an expression given as separate arguments.
//! `expr 1 + 2 \* 3`, comparisons (= != < <= > >=), `|` `&`, `length STR`.

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, println, String};

huldra_user::main!(main);

struct P<'a> {
    t: &'a [String],
    i: usize,
}

impl P<'_> {
    fn peek(&self) -> Option<&str> {
        self.t.get(self.i).map(String::as_str)
    }

    fn atom(&mut self) -> Result<String, String> {
        match self.peek() {
            Some("(") => {
                self.i += 1;
                let v = self.or()?;
                if self.peek() != Some(")") {
                    return Err("missing )".into());
                }
                self.i += 1;
                Ok(v)
            }
            Some("length") => {
                self.i += 1;
                let v = self.atom()?;
                Ok(huldra_user::format!("{}", v.chars().count()))
            }
            Some(s) => {
                let v = String::from(s);
                self.i += 1;
                Ok(v)
            }
            None => Err("missing argument".into()),
        }
    }

    fn num(s: &str) -> Result<i64, String> {
        s.parse().map_err(|_| huldra_user::format!("non-integer argument '{}'", s))
    }

    fn mul(&mut self) -> Result<String, String> {
        let mut l = self.atom()?;
        while let Some(op @ ("*" | "/" | "%")) = self.peek() {
            let op = String::from(op);
            self.i += 1;
            let r = self.atom()?;
            let (a, b) = (Self::num(&l)?, Self::num(&r)?);
            if b == 0 && op != "*" {
                return Err("division by zero".into());
            }
            l = huldra_user::format!("{}", match op.as_str() { "*" => a * b, "/" => a / b, _ => a % b });
        }
        Ok(l)
    }

    fn add(&mut self) -> Result<String, String> {
        let mut l = self.mul()?;
        while let Some(op @ ("+" | "-")) = self.peek() {
            let plus = op == "+";
            self.i += 1;
            let r = self.mul()?;
            let (a, b) = (Self::num(&l)?, Self::num(&r)?);
            l = huldra_user::format!("{}", if plus { a + b } else { a - b });
        }
        Ok(l)
    }

    fn cmp(&mut self) -> Result<String, String> {
        let l = self.add()?;
        if let Some(op @ ("=" | "!=" | "<" | "<=" | ">" | ">=")) = self.peek() {
            let op = String::from(op);
            self.i += 1;
            let r = self.add()?;
            let ord = match (l.parse::<i64>(), r.parse::<i64>()) {
                (Ok(a), Ok(b)) => a.cmp(&b),
                _ => l.cmp(&r),
            };
            let res = match op.as_str() {
                "=" => ord.is_eq(),
                "!=" => ord.is_ne(),
                "<" => ord.is_lt(),
                "<=" => ord.is_le(),
                ">" => ord.is_gt(),
                _ => ord.is_ge(),
            };
            return Ok(String::from(if res { "1" } else { "0" }));
        }
        Ok(l)
    }

    fn and(&mut self) -> Result<String, String> {
        let mut l = self.cmp()?;
        while self.peek() == Some("&") {
            self.i += 1;
            let r = self.cmp()?;
            l = if truthy(&l) && truthy(&r) { l } else { String::from("0") };
        }
        Ok(l)
    }

    fn or(&mut self) -> Result<String, String> {
        let mut l = self.and()?;
        while self.peek() == Some("|") {
            self.i += 1;
            let r = self.and()?;
            if !truthy(&l) {
                l = if truthy(&r) { r } else { String::from("0") };
            }
        }
        Ok(l)
    }
}

fn truthy(s: &str) -> bool {
    !s.is_empty() && s != "0"
}

fn main() -> i32 {
    let args = &env::args()[1..];
    let mut p = P { t: args, i: 0 };
    match p.or() {
        Ok(v) if p.i == args.len() => {
            println!("{}", v);
            (!truthy(&v)) as i32
        }
        Ok(_) => {
            eprintln!("expr: syntax error");
            2
        }
        Err(e) => {
            eprintln!("expr: {}", e);
            2
        }
    }
}
