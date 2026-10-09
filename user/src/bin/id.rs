//! id: print the user and group ids (always root: there are no users yet).

#![no_std]
#![no_main]

huldra_user::main!(main);

fn main() -> i32 {
    huldra_user::println!("uid=0(root) gid=0(root) groups=0(root)");
    0
}
