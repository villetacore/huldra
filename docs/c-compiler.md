# The C compiler (hcc)

`cc` compiles C inside Huldra. The same compiler,
[`libs/hcc`](../libs/hcc), runs on the build machine as `cargo xtask cc`
and builds the C packages in the repository.

```sh
cc -run /usr/share/huldra/examples/primes.c 100   # compile and run
cc -o life life.c && ./life
cc -E file.c                                      # preprocess only
```

```text
cc [-o out] [-E] [-I dir] [-D name[=value]] [-run] file.c... [-- args]
```

## How it works

hcc compiles **the whole program at once**: your sources together with the
C library, which is shipped in source form (`/usr/lib/hcc/libc.c`). The
output is a static x86-64 ELF executable. There is no assembler, no object
files and no linker, so `-c` does not exist.

```text
sources + libc.c ─▶ lex ─▶ pp ─▶ parse (typed AST) ─▶ gen ─▶ asm ─▶ elf
```

| stage | file | |
|---|---|---|
| lexer | `lex.rs` | tokens with positions |
| preprocessor | `pp.rs` | `#include`, object and function macros, `#`/`##`, `#if` arithmetic, `__VA_ARGS__` |
| parser | `parse.rs`, `ast.rs`, `ty.rs` | declarations, types, a typed expression tree |
| code generator | `gen.rs` | a stack machine over the typed AST; every expression leaves its value in `rax` (`float`/`double` in SSE registers) |
| assembler | `asm.rs` | x86-64 instruction encoding, labels, relocations |
| ELF writer | `elf.rs` | a static executable with text, data and bss |

The executables use **Linux system calls only**, so they run on Huldra and on
Linux alike. That is how `cargo xtask cc-test` checks the compiler: every
program in `tests/cc/` is built by both hcc and gcc and run on Linux (in
WSL on Windows), and the two outputs must be identical.

## The language

- C99/C11: structs, unions, enums, bit-fields, function pointers, varargs,
  compound literals, designated initializers, `_Bool`, `static_assert`
- GNU extensions: statement expressions, `case 1 ... 5`, `a ?: b`,
  `typeof`, `__attribute__` (accepted)
- `float` and `double` on SSE; `setjmp`/`longjmp`

Not supported: VLAs. `long double` is the same as `double`, bit-fields are
not packed, and the code is not optimized.

## The C library

The headers are in `/usr/include` and the implementation in
`/usr/lib/hcc/libc.c` (from `rootfs/` in the repository):

stdio (`printf`/`scanf` families, buffered `FILE`), stdlib (`malloc`,
`qsort`, `strtol`…), string, ctype, math, time, `dirent`, processes
(`fork`, `exec*`, `waitpid`), signals, termios, sockets, `getaddrinfo`
(DNS), `poll`, and the GUI library `gui.h` (see [graphics](graphics.md)).

## Examples

`/usr/share/huldra/examples/` (from [`diskfs/examples`](../diskfs/examples)):
`hello.c`, `primes.c`, `life.c`, `wc.c`, `fetch.c` (HTTP over sockets),
`fbdemo.c` (drawing straight to `/dev/fb0`), `window.c` (a GUI window).

Packages written in C (`packages/*/src/*.c`) are compiled by
`cargo xtask repo` with the same compiler.
