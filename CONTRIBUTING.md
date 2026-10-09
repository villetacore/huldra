# Contributing to Huldra

Thanks for your interest! Huldra is a hobby OS meant to be read as much as
run, so clear code and good tests matter more than features. Bug reports,
fixes, new programs, packages, documentation and ideas are all welcome.

## Getting set up

```bash
git clone https://github.com/villetacore/huldra && cd huldra
cargo xtask run      # build and boot in QEMU
cargo xtask test     # everything CI runs
```

See [getting started](docs/getting-started.md) for the prerequisites (Rust
stable and QEMU; gcc, e2fsprogs and GRUB tools are optional) and for
Windows notes. [Architecture](docs/architecture.md) is the map of the code.

## Workflow

1. For anything bigger than a small fix, open an issue first, so we can agree
   on the approach before you spend time on it.
2. Fork, then create a branch from `main`.
3. Make the change **with tests** (see below).
4. Run `cargo xtask test`. CI runs exactly that, in QEMU on Ubuntu.
5. Add a line to the `[Unreleased]` section of [CHANGELOG.md](CHANGELOG.md)
   if users will notice the change.
6. Open a pull request. Describe what changed and why, and how you tested it.

## Where tests go

| what you changed | test it with |
|---|---|
| logic that needs no hardware (formats, protocols, algorithms) | `#[test]` in the `libs/` crate, run by `cargo test -p <crate>` |
| kernel behavior | a test in the subsystem's `TESTS` list (e.g. `kernel/src/fs/mod.rs`) |
| a program or anything user-visible | a step in a `tests/*.txt` session script ([format](docs/testing.md#session-scripts)) |
| the C compiler | a program in `tests/cc/` whose output must match gcc's |
| system calls used by Linux programs | `tests/linux/*.c` and `tests/linux.txt` |

If a piece of logic is hard to test, it probably belongs in a `libs/` crate
with the I/O kept outside, the way `libs/net` and `libs/pkg` are built.

## Code style

- **Match the surrounding code**: its naming, comment density and idiom.
- **No dependencies from crates.io.** Everything is written here, and that is
  part of the point of the project. If you need an algorithm, implement it
  in the smallest reasonable way, with tests.
- `no_std` everywhere except `xtask`.
- Every file and public item gets a doc comment that says *what it is
  for*. Comments explain why, not what.
- Errors: the kernel returns `Errno`, user programs print
  `program: what: why` to stderr and exit non-zero.
- Keep the user-visible behavior close to POSIX and Linux unless there is a
  reason not to, and write the reason down.
- Commits: one logical change each, with a subject in the imperative
  (`pkg: add rollback`) and a body that explains why.

## Adding things

- **A program**: add `user/src/bin/NAME.rs`. It is built and installed into
  `/bin` automatically. Mention it in the docs, and test it in
  `tests/shell.txt` if it can be tested.
- **A package**: add `packages/NAME/` with `PKGINFO`, `src/*.c` and/or
  `files/`. See [writing packages](docs/packages.md#writing-packages).
- **A system call**: add the number in `libs/abi`, a handler in
  `kernel/src/syscall/`, and a test (`utest` or a Linux program).

## Reporting bugs

Use the [bug report form](https://github.com/villetacore/huldra/issues/new/choose).
The most useful reports include the exact command, what you expected, what
happened, the kernel log (`dmesg`, or the serial output of `cargo xtask run`)
and your host OS and QEMU version.

Security problems: please follow [SECURITY.md](SECURITY.md) instead of
opening a public issue.

## License

By contributing you agree that your contributions are licensed under the
[MIT license](LICENSE), like the rest of the project.
