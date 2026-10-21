# The Rust compiler (hrc)

`rustc` compiles Rust inside Huldra. It is not the real rustc: hrc is a
compiler for a large subset of Rust written for this system,
[`libs/hrc`](../libs/hrc), and the same compiler runs on the build machine
as `cargo xtask rs`.

```sh
rustc -run /usr/share/huldra/examples/primes.rs 100   # compile and run
rustc shapes.rs && ./shapes                           # like rustc: ./shapes
rustc -O -o fast main.rs                              # no overflow checks
rustc --emit c main.rs                                # show the C it makes
```

```text
rustc [-o out] [-O] [--emit c] [-run] file.rs [-- args]
```

Programs are ordinary Rust 2021: a `main.rs` with `mod name;` files next
to it, `use std::...`, `#[derive(...)]`, `macro_rules!`. The test suite
compiles every program in [`tests/rs`](../tests/rs) with both hrc and
rustc, and the two executables must print the same standard output and
standard error and exit with the same status.

## How it works

hrc translates Rust to C and hands it to [hcc](c-compiler.md), which
compiles it together with the runtime and the C library into a static
x86-64 executable.

```text
main.rs + std ─▶ lex ─▶ parse (+ macro_rules!) ─▶ resolve ─▶ check ─▶ C ─▶ hcc ─▶ ELF
```

| stage | file | |
|---|---|---|
| lexer | `lex.rs` | tokens with line and column |
| parser | `parse.rs`, `ast.rs` | the syntax tree; built-in macros (`println!`, `vec!`, `assert_eq!`, ...) become their own nodes |
| macros | `macros.rs` | `macro_rules!`: matching with fragments (`expr`, `ty`, `ident`, `tt`, ...) and repetitions |
| names | `resolve.rs` | modules, `use` (globs, groups, `as`), the prelude, items in blocks |
| types | `check/` | inference (unification with integer and float variables), traits and impl selection, associated types, method lookup with auto-deref and auto-ref, coercions, closures, patterns |
| monomorphization | `check/mono.rs` | every generic function is checked and compiled per set of type arguments; trait methods resolve to impls or to vtables |
| C | `cgen.rs` | each function becomes C statements over temporaries; `break`, `?` and `match` arms are `goto`s |

Because generic code is checked per instantiation (like C++ templates),
an error in a generic function shows up only where it is used, with the
concrete types.

The standard library is Rust too: [`/usr/lib/hrc/std`](../rootfs/usr/lib/hrc/std)
is compiled with every program, and only what a program uses ends up in the
executable. The runtime in [`rt.c`](../rootfs/usr/lib/hrc/rt.c) does what
needs C: process start, the stdout buffer, panics and exact float printing
and parsing (shortest round-trip digits like Rust's).

## The language

- Structs (named, tuple, unit), enums with data, `impl` blocks, methods,
  associated functions and constants
- Generics with trait bounds and `where` clauses, const generics
  (`[T; N]`, `Matrix<4>`)
- Traits: default methods, supertraits, associated types, generic traits,
  blanket impls, operator overloading (`Add`, `Index`, `Neg`, ...),
  `impl Trait` arguments and return types
- Trait objects: `&dyn Trait`, `Box<dyn Trait>`, `Box<dyn Fn(i32) -> i32>`
- Closures capturing by reference or by `move`, passed to generics and
  returned; function pointers
- Patterns: tuples, structs, enums, slices (`[first, .., last]`,
  `rest @ ..`), ranges, `|`, guards, `@`, references, `ref`/`ref mut`, match
  ergonomics; `if let`/`while let` chains, `let ... else`
- `?` on `Option` and `Result` (with `From` conversion), `main` returning
  `Result`
- Integer overflow, division by zero and out-of-bounds indexing panic with
  rustc's messages and source locations (`#[track_caller]` works)
- Modules in files, `use`, `pub`, `macro_rules!`, `#[derive(Debug, Clone,
  Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]`, `#[cfg(...)]`

## The standard library

`Option`, `Result`, `Box`, `Vec`, `String`, `&str` with patterns (`split`,
`find`, `trim_matches`, ...), slices (sorting, searching, `windows`,
`chunks`), iterators with the usual adapters (`map`, `filter`, `zip`,
`flat_map`, `peekable`, `rev`, `sum`, `collect`, ...), formatting (width,
alignment, precision, `{:?}`, `{:#?}`, hex, `{:e}`), `HashMap`, `HashSet`,
`BTreeMap`, `BTreeSet`, `VecDeque`, `BinaryHeap`, `Rc`, `Cell`, `RefCell`,
`io` (`stdin().read_line`, `lines()`, `Write`, `BufReader`, `BufWriter`),
`fs` (`read_to_string`, `write`, `File`), `env::args`, `process::exit`,
`time::Instant`, `f64` math.

## Limitations

hrc compiles correct programs; it does not reject every incorrect one.

- There is no borrow checker and lifetimes are ignored.
- Destructors do not run: `Drop` impls are never called, memory is freed
  when the program exits, `RefCell` does not track borrows, `Rc` counts
  only go up.
- No threads (`std::thread::spawn`), `async`, `unsafe` checks, `u128`/`i128`,
  procedural macros or specialization.
- `HashMap` iterates in insertion order (rustc's order is random anyway).
- Generic functions are checked when used, not where they are defined.

## Examples

`/usr/share/huldra/examples/` (from [`diskfs/examples`](../diskfs/examples)):
`hello.rs`, `primes.rs`, `shapes.rs` (traits and trait objects),
`wordfreq.rs` (a `HashMap` of words in a file).
