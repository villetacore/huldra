// HashMap, HashSet, BTreeMap, BTreeSet, VecDeque, BinaryHeap, Rc, RefCell.
use std::cell::{Cell, RefCell};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque};
use std::rc::Rc;

#[derive(Debug)]
struct Node {
    value: i32,
    children: Vec<Rc<RefCell<Node>>>,
}

fn sum_tree(n: &Rc<RefCell<Node>>) -> i32 {
    let node = n.borrow();
    node.value + node.children.iter().map(sum_tree).sum::<i32>()
}

fn main() {
    let text = "the cat sat on the mat the end";
    let mut freq: HashMap<&str, usize> = HashMap::new();
    for w in text.split_whitespace() {
        *freq.entry(w).or_insert(0) += 1;
    }
    let mut entries: Vec<(&str, usize)> = freq.iter().map(|(k, v)| (*k, *v)).collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    println!("{:?}", entries);
    println!("{} {:?} {:?}", freq.len(), freq.get("cat"), freq.get("dog"));
    if let Some(v) = freq.get_mut("cat") {
        *v += 10;
    }
    println!("{}", freq["cat"]);
    freq.remove("the");
    println!("{} {}", freq.contains_key("the"), freq.len());

    let mut scores: HashMap<String, Vec<u32>> = HashMap::new();
    scores.entry("ann".to_string()).or_default().push(90);
    scores.entry("ann".to_string()).or_default().push(80);
    scores.entry("bo".to_string()).or_insert_with(Vec::new).push(70);
    let mut names: Vec<&String> = scores.keys().collect();
    names.sort();
    for n in names {
        let v = &scores[n];
        println!("{} {:?} avg {}", n, v, v.iter().sum::<u32>() / v.len() as u32);
    }

    let a: HashSet<i32> = [1, 2, 3, 4].into_iter().collect();
    let b: HashSet<i32> = HashSet::from([3, 4, 5]);
    let mut inter: Vec<_> = a.intersection(&b).copied().collect();
    inter.sort();
    let mut uni: Vec<_> = a.union(&b).copied().collect();
    uni.sort();
    let mut diff: Vec<_> = a.difference(&b).collect();
    diff.sort();
    println!("{:?} {:?} {:?}", inter, uni, diff);
    println!("{} {}", a.contains(&2), b.is_subset(&a));

    let mut bt = BTreeMap::new();
    for (i, w) in ["delta", "alpha", "charlie", "bravo"].iter().enumerate() {
        bt.insert(*w, i);
    }
    println!("{:?}", bt);
    println!("{:?} {:?}", bt.first_key_value(), bt.keys().next_back());
    for (k, v) in bt.range("b".."d") {
        println!("{} {}", k, v);
    }
    let set: BTreeSet<char> = "mississippi".chars().collect();
    println!("{:?}", set);

    let mut dq: VecDeque<i32> = VecDeque::new();
    dq.push_back(1);
    dq.push_back(2);
    dq.push_front(0);
    dq.push_front(-1);
    println!("{:?} {:?} {:?}", dq, dq.front(), dq.back());
    dq.pop_front();
    dq.pop_back();
    println!("{:?} len {} [1]={}", dq, dq.len(), dq[1]);

    let mut heap = BinaryHeap::new();
    for x in [5, 1, 8, 3, 9, 2] {
        heap.push(x);
    }
    let mut out = vec![];
    while let Some(x) = heap.pop() {
        out.push(x);
    }
    println!("{:?}", out);
    let mut minheap = BinaryHeap::new();
    for x in [5, 1, 8] {
        minheap.push(Reverse(x));
    }
    println!("{:?}", minheap.pop().map(|Reverse(x)| x));
    println!("{:?}", minheap.peek());

    let leaf1 = Rc::new(RefCell::new(Node { value: 3, children: vec![] }));
    let leaf2 = Rc::new(RefCell::new(Node { value: 4, children: vec![] }));
    let root = Rc::new(RefCell::new(Node {
        value: 1,
        children: vec![leaf1.clone(), leaf2.clone()],
    }));
    println!("sum = {}", sum_tree(&root));
    leaf1.borrow_mut().value = 30;
    println!("sum = {}", sum_tree(&root));
    println!("rc count = {}", Rc::strong_count(&leaf1));
    let shared = Rc::new(vec![1, 2, 3]);
    let other = Rc::clone(&shared);
    println!("{:?} {}", other, Rc::ptr_eq(&shared, &other));
    let cell = Cell::new(5);
    cell.set(cell.get() * 2);
    println!("{}", cell.get());
    let log = RefCell::new(Vec::new());
    log.borrow_mut().push("a");
    log.borrow_mut().push("b");
    println!("{:?} {}", log.borrow(), log.borrow().len());
}
