// Modules, macro_rules!, generics with associated types, supertraits,
// constants and statics.

mod geometry {
    pub mod shapes {
        #[derive(Debug)]
        pub struct Square(pub f64);

        impl Square {
            pub fn area(&self) -> f64 {
                self.0 * self.0
            }
        }
    }

    pub fn double(x: f64) -> f64 {
        x * 2.0
    }

    pub use self::shapes::Square;
}

use geometry::shapes;
use geometry::{double, Square};

macro_rules! square {
    ($x:expr) => {
        $x * $x
    };
}

macro_rules! maximum {
    ($x:expr) => { $x };
    ($x:expr, $($rest:expr),+) => {{
        let a = $x;
        let b = maximum!($($rest),+);
        if a > b { a } else { b }
    }};
}

macro_rules! make_struct {
    ($name:ident { $($field:ident: $ty:ty),* }) => {
        #[derive(Debug, Default)]
        struct $name {
            $($field: $ty),*
        }
    };
}

make_struct!(Settings { width: u32, title: String });

macro_rules! hashmap {
    ($($k:expr => $v:expr),* $(,)?) => {{
        let mut m = std::collections::BTreeMap::new();
        $(m.insert($k, $v);)*
        m
    }};
}

const LIMIT: usize = 3;
static GREETING: &str = "hi";
static mut COUNTER: u32 = 0;

trait Animal {
    fn name(&self) -> String;
    fn sound(&self) -> &'static str;
    fn speak(&self) -> String {
        format!("{} says {}", self.name(), self.sound())
    }
}

trait Pet: Animal {
    fn owner(&self) -> &str;
    fn tag(&self) -> String {
        format!("{} (owned by {})", self.name(), self.owner())
    }
}

struct Dog {
    owner: String,
}

impl Animal for Dog {
    fn name(&self) -> String {
        "Dog".into()
    }
    fn sound(&self) -> &'static str {
        "woof"
    }
}

impl Pet for Dog {
    fn owner(&self) -> &str {
        &self.owner
    }
}

trait Container {
    type Item;
    fn get(&self, i: usize) -> Option<&Self::Item>;
    fn first(&self) -> Option<&Self::Item> {
        self.get(0)
    }
    fn size(&self) -> usize;
}

struct MyVec<T> {
    data: Vec<T>,
}

impl<T> Container for MyVec<T> {
    type Item = T;
    fn get(&self, i: usize) -> Option<&T> {
        self.data.get(i)
    }
    fn size(&self) -> usize {
        self.data.len()
    }
}

fn describe<C: Container>(c: &C) -> String
where
    C::Item: std::fmt::Debug,
{
    format!("{} items, first {:?}", c.size(), c.first())
}

fn show_pet(p: &dyn Pet) {
    println!("{} / {}", p.speak(), p.tag());
}

struct Matrix<const N: usize> {
    m: [[i32; N]; N],
}

impl<const N: usize> Matrix<N> {
    fn identity() -> Self {
        let mut m = [[0; N]; N];
        for i in 0..N {
            m[i][i] = 1;
        }
        Matrix { m }
    }
    fn trace(&self) -> i32 {
        (0..N).map(|i| self.m[i][i]).sum()
    }
}

fn bump() -> u32 {
    unsafe {
        COUNTER += 1;
        COUNTER
    }
}

fn main() {
    let s = Square(3.0);
    println!("{:?} {} {}", s, s.area(), double(s.area()));
    let t = shapes::Square(1.5);
    println!("{}", geometry::double(t.0));
    println!("{}", square!(7));
    println!("{}", square!(1 + 2));
    println!("{}", maximum!(3, 9, 4, 1));
    let st = Settings { width: 80, ..Default::default() };
    println!("{:?}", st);
    let m = hashmap! { "a" => 1, "b" => 2, };
    println!("{:?}", m);
    for i in 0..LIMIT {
        print!("{} ", i);
    }
    println!("{}", GREETING);
    bump();
    bump();
    println!("counter {}", bump());
    let d = Dog { owner: "Sam".to_string() };
    show_pet(&d);
    let mv = MyVec { data: vec!['x', 'y'] };
    println!("{}", describe(&mv));
    let empty: MyVec<u8> = MyVec { data: vec![] };
    println!("{}", describe(&empty));
    let id = Matrix::<4>::identity();
    println!("trace {}", id.trace());
}
