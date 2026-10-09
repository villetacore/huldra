# Testing, CI and releases

`cargo xtask test` runs everything, from unit tests to a full boot in
QEMU. It uses its own disk image and initrd, so you can keep a
`cargo xtask run` session open while it runs.

| # | layer | what |
|---|---|---|
| 1 | host unit tests | every crate in `libs/`: allocators, ELF, cpio, ext2 (+ consistency checks), tar/SHA-256, the C compiler, TCP/IP (lossy wire, slow reader), DHCP, terminal emulator, tiling, display protocol, package model |
| 2 | `tests/cc/*.c` | compiled by hcc and gcc and run on Linux; the outputs must match |
| 3 | in-kernel tests | `ktest` on the kernel command line: VFS, symlinks, pipes, devices, procfs, scheduler, memory… |
| 4 | `tests/shell.txt` | the shell language, utilities, full-screen programs, networking (DHCP, ping, httpd + wget, nc), compiling C inside the system, `utest` (system calls from user space) |
| 5 | `tests/persist.txt` | reboot: files on the root disk survive |
| 6 | `tests/gui.txt` | graphics via `guitest`: drawing reaches the screen, text, moving windows, window list, window manager frames, back to the text console |
| 7 | `tests/pkg.txt` | declarative packages against the repository xtask serves: add, pins, `/etc` files, remove, rollback, gc |
| 8 | `tests/linux.txt` | real static glibc programs: files, processes, threads, sockets |
| 9 | disk check | the disk the guest wrote is read on the host and checked with `e2fsck -fn` |

## Session scripts

`tests/*.txt` drive the system through the serial console. Run one with
`cargo xtask session tests/pkg.txt`.

```text
# comment
> command            run a command and wait for the prompt
< text               the output must contain text
! text               the output must not contain text
= keys               send raw keystrokes (\r, \e, \xNN escapes), for full-screen programs
.                    wait for the prompt to come back
```

Failures are collected and reported together, each with the full output of
its command.

## Adding tests

- Logic that does not need hardware belongs in a `libs/` crate with a
  `#[test]`.
- Kernel behavior goes in a `TESTS` list next to the code (see the end of
  `kernel/src/fs/mod.rs`).
- User-visible behavior goes in a session script. Prefer extending an
  existing one over adding a new boot.

## CI

[`.github/workflows/ci.yml`](../.github/workflows/ci.yml) runs on every push
to `main` and on every pull request. It builds debug and release and runs
the whole `cargo xtask test` in QEMU on Ubuntu (without KVM, so it takes a
while). If a run fails, the initrd, the test disk and the C test output are
uploaded as an artifact.

## Releases

[`.github/workflows/release.yml`](../.github/workflows/release.yml) runs when
a `vX.Y.Z` tag is pushed. It runs CI, checks that the tag matches the
version in `Cargo.toml`, and builds the release with the ISO, repository
and fsck steps. It then publishes a GitHub release containing:

- `huldra-X.Y.Z-x86_64.iso`: boots with GRUB and runs from the initrd;
- `huldra-X.Y.Z-x86_64-kernel` and `-initrd.cpio`, for `qemu -kernel`;
- `huldra-X.Y.Z-x86_64-disk.img.gz`, the root file system;
- `huldra-X.Y.Z-repo.tar.gz`, the package repository;
- `SHA256SUMS`.

The release notes are the version's section of
[`CHANGELOG.md`](../CHANGELOG.md).

To cut a release:

1. Set `version` in `Cargo.toml` and `VERSION`/`PRETTY_NAME` in
   `rootfs/etc/os-release`.
2. Move the `[Unreleased]` entries in `CHANGELOG.md` into a new section.
3. Commit, then `git tag vX.Y.Z && git push origin main vX.Y.Z`.
