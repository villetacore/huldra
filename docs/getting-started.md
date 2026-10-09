# Getting started

## Run a release

Every [release](https://github.com/villetacore/huldra/releases) contains
a bootable ISO, the kernel and initrd for `qemu -kernel`, a disk image, the
package repository and `SHA256SUMS`.

```bash
gunzip huldra-0.4.0-x86_64-disk.img.gz
qemu-system-x86_64 -m 256M -nic user,model=e1000 \
  -kernel huldra-0.4.0-x86_64-kernel -initrd huldra-0.4.0-x86_64-initrd.cpio \
  -drive file=huldra-0.4.0-x86_64-disk.img,format=raw,if=ide \
  -append "root=/dev/hda gui"
```

Drop `gui` to boot into the text console. The ISO boots through GRUB and
runs entirely from the initrd, without a disk.

## Build from source

You need:

- **Rust**, stable. `rust-toolchain.toml` installs the `x86_64-unknown-none`
  target automatically.
- **QEMU** (`qemu-system-x86_64`).

These are optional; the steps that need them are skipped when they are
missing:

| tool | used for |
|---|---|
| `gcc` | comparing hcc's output with gcc; the Linux test programs |
| `e2fsck` | checking the disk image the guest wrote |
| `grub-mkrescue`, `xorriso`, `mtools` | `cargo xtask iso` |

On Windows, gcc, e2fsck and the GRUB tools are taken from WSL.

```bash
git clone https://github.com/villetacore/huldra
cd huldra
cargo xtask run
```

This builds the kernel, the programs, the initrd and the disk, starts the
package repository on port 8800, and boots QEMU with an e1000 network
card. The console appears both in the QEMU window and in your terminal.

## xtask commands

| command | what it does |
|---|---|
| `cargo xtask build [--release]` | kernel, programs, `target/initrd.cpio`, `target/disk.img` |
| `cargo xtask run [--release] [--headless] [--append "..."]` | boot in QEMU (+ package repository on port 8800, host `localhost:8080` → guest port 80) |
| `cargo xtask run --gui` | boot straight into the graphical session |
| `cargo xtask test` | all tests (see [testing](testing.md)); leaves `disk.img` alone, so it can run beside `run` |
| `cargo xtask session FILE` | run one scenario (`tests/*.txt`) on a fresh disk |
| `cargo xtask cc file.c -o prog` | compile C on the host with hcc |
| `cargo xtask cc-test` | `tests/cc/*.c`: hcc and gcc must print the same |
| `cargo xtask repo` / `serve` | build the package repository / serve it over HTTP |
| `cargo xtask fsck` | build an ext2 image and check it with the real `e2fsck` |
| `cargo xtask iso` | bootable ISO with GRUB |

Options: `--release` (optimized), `--headless` (no window, serial only),
`--gdb` (wait for gdb on `localhost:1234`), `--append S` (kernel command
line), `--gui`.

`target/disk.img` is the root file system and keeps your files between
runs. A build only refreshes `/bin`, `/sbin` and `/usr` on it. Delete the
file to start from scratch.

## First steps in the system

```sh
ls /usr/share/huldra             # docs and examples
less /usr/share/huldra/docs/packages.md

pkg add fortune cowsay           # declare packages and switch
fortune | cowsay
pkg generations; pkg rollback    # every change can be undone

cc -run /usr/share/huldra/examples/primes.c 100
edit hello.c                     # Ctrl+S saves, Ctrl+Q quits
fm                               # two-panel file manager

ifconfig; ping 10.0.2.2
httpd &                          # then open http://localhost:8080 on the host

startgui                         # or: startgui tilewm
```

## Debugging the kernel

```bash
cargo xtask run --gdb --headless
gdb target/x86_64-unknown-none/debug/huldra -ex "target remote :1234"
```

`--append debug` logs at debug level; `dmesg` shows the kernel log inside
the system. A kernel panic prints the message and halts. Under `ktest` it
also exits QEMU with a failure code.
