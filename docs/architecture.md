# Architecture

Huldra is a monolithic kernel with a Linux-compatible system call
interface, a user space written mostly in Rust, and a set of `no_std`
libraries shared by both. Everything is built by `cargo xtask` from this
repository, with no dependencies from crates.io.

```text
┌──────────────────────────────── user space ────────────────────────────────┐
│ init   sh   edit  fm  less  pkg  cc  httpd …   display  boxwm/tilewm  panel │
│        Linux static binaries (glibc)           term  files  paint  games    │
│ ─────────── huldra-user (runtime: syscalls, I/O, net, gui) ────── libc.c ── │
└──────────────────────────────┬─────────────────────────────────────────────┘
                     syscall / int 0x80 (Linux x86_64 numbers)
┌──────────────────────────────┴───────────── kernel ────────────────────────┐
│ syscall/   process, fs, memory, signal, net, misc, linux-compat            │
│ proc/      exec (ELF, #!), signals, futex, alarms, user memory (VMAs)      │
│ task/      round-robin preemptive scheduler, wait queues                   │
│ fs/        VFS + symlinks, ext2, tmpfs, devfs, procfs, pipes, initrd       │
│ net/       e1000 driver, sockets  ──  huldra-net (TCP/IP, DHCP, DNS)       │
│ mm/        buddy frames, slab heap, kernel page tables, kernel stacks      │
│ drivers/   ATA, PCI, PS/2, serial, VGA console, framebuffer, PTYs, RTC     │
│ arch/      boot (Multiboot2, PVH), GDT/IDT, APIC/IOAPIC, ACPI, paging, FPU │
└────────────────────────────────────────────────────────────────────────────┘
```

## Contents

- [Repository layout](#repository-layout)
- [Boot](#boot)
- [Memory](#memory)
- [Tasks and processes](#tasks-and-processes)
- [System calls](#system-calls)
- [File systems](#file-systems)
- [Devices](#devices)
- [User space](#user-space)
- [Libraries](#libraries)
- [Design rules](#design-rules)

## Repository layout

```text
kernel/src/   arch/ mm/ task/ proc/ syscall/ fs/ net/ drivers/
libs/         no_std libraries, unit-tested on the host with `cargo test`
user/         the user space runtime (huldra-user) and every program
rootfs/       files of the base system: /etc, /usr/include, /usr/lib/hcc
diskfs/       examples and notes installed in /usr/share/huldra
packages/     sources of the packages in the repository
docs/         this documentation (also installed in /usr/share/huldra/docs)
tests/        QEMU session scripts, C compiler tests, Linux test programs
xtask/        the build tool: images, QEMU, tests, ISO, package repository
```

## Boot

1. A loader starts the kernel in 32-bit mode. GRUB uses **Multiboot2**;
   `qemu -kernel` uses **PVH**. `arch/x86_64/boot.S` handles both: it builds
   early page tables, enables long mode and jumps to the higher half.
2. `kernel_main` (`kernel/src/main.rs`) parses the boot information
   (memory map, modules, command line), then brings up the architecture
   (GDT, IDT, PIT at 100 Hz), the frame allocator, the kernel page tables,
   the local APIC and IOAPIC, the scheduler, and the drivers.
3. `fs::init` mounts the root file system. With `root=/dev/hda`, that is the
   ext2 disk. Otherwise it is a tmpfs filled from the initrd (a cpio
   archive, like Linux initramfs). It then mounts `/dev`, `/proc` and `/tmp`.
4. `net::init` configures the e1000 card (`ip=dhcp` by default).
5. PID 1 runs `/sbin/init`, or the program given by `init=`. With `ktest`
   on the command line, the in-kernel tests run instead.
6. `init` mounts `/etc/fstab`, prints the banner, starts the graphical
   session if `gui` was given, and keeps a shell on the console.

Kernel parameters: `root=`, `init=`, `ip=dhcp|none|ADDR/BITS,GW,DNS`, `gui`,
`debug`, `quiet`, `ktest`.

## Memory

- **Physical memory**: a buddy allocator (`huldra-buddy`). Its per-frame
  metadata is carved out of the first large usable region.
- **Kernel heap**: slab classes for small objects on top of the buddy
  allocator (`huldra-kalloc`).
- **Kernel address space**: a direct map of all physical memory plus the
  kernel image in the upper half, mapped section by section with W^X and NX.
  Kernel stacks have guard pages.
- **User address spaces** (`proc/mm.rs`): a page table plus a list of VMAs.
  Anonymous memory (heap, stack, `mmap`) is allocated on first touch by the
  page fault handler. `fork` copies every page; there is no copy-on-write yet.

## Tasks and processes

- **Scheduler** (`task/sched.rs`): round robin and preemptive, with a time
  slice of 5 ticks (50 ms). Kernel code is preemptible whenever interrupts
  are enabled. Spinlocks disable interrupts, so a task holding a lock is
  never switched out.
- **Wait queues** with timeouts underlie sleeping, pipes, sockets, `poll`
  and futexes.
- **Processes**: `fork`, `vfork` and `clone` (threads, TLS, the `CLONE_*`
  flags glibc needs), thread groups, sessions and process groups, job
  control.
- **exec** loads static ELF executables, including static-pie, and `#!`
  scripts.
- **Signals**: handlers, masks, `SA_RESTART`, `sigreturn`, `alarm`.
- **FPU/SSE** state is saved per task.

## System calls

System calls use the **Linux x86_64 numbers, structures and errno
values** (`libs/abi`). They are entered with `syscall` or `int 0x80`: the
number goes in RAX, the arguments in RDI, RSI, RDX, R10, R8 and R9, and RAX
returns the result or a negated errno. About 130 calls are implemented.
The first use of an unknown call is logged, which helps when porting.

Because the interface is Linux's, statically linked Linux programs
(glibc, `gcc -static`) run unmodified, threads included, and hcc's output
runs on Linux as well.

## File systems

`fs/vfs.rs` defines the `Inode` and `FileSystem` traits and the mount
table.

- **Path resolution**: callers normalize a path lexically against the
  working directory (`.` and `..`). `lookup` then walks it from the root,
  crossing mount points and following **symbolic links**. A link target is
  spliced into the rest of the path, so `..` inside a target is physical.
  More than 40 links in one lookup gives `ELOOP`. `lstat`, `readlink`,
  `unlink` and `rename` act on the link itself.
- **ext2** (`libs/ext2`, glued in `fs/ext2.rs`): read and write support,
  directories, indirect blocks, sparse files, multiple block groups, and
  fast (in-inode) and slow symlinks. Images pass `e2fsck -fn`. Dirty blocks
  are written back every five seconds by the `flushd` kernel thread, and on
  `sync`.
- **tmpfs**, **devfs** (`/dev`), **procfs** (`/proc`: processes, mounts,
  uptime, `/proc/net/*`), **pipes**.

## Devices

| device | driver |
|---|---|
| `/dev/hda`… | ATA PIO disks |
| `/dev/console`, `/dev/tty` | VGA text console with VT100 emulation, plus COM1 |
| `/dev/ptmx`, `/dev/pts/N` | pseudo-terminals (used by `term`) |
| `/dev/fb0` | Bochs/QEMU VGA framebuffer with Linux fbdev ioctls and `mmap` |
| `/dev/kbd`, `/dev/mouse` | raw keyboard events; PS/2 mouse with wheel; VMware/QEMU absolute pointer |
| `/dev/null`, `/dev/zero`, `/dev/random`, `/dev/font` | the usual, plus the console font |
| network | Intel e1000 (DMA rings, interrupts); see [networking](networking.md) |

## User space

`huldra-user` (`user/src/lib.rs`) is the runtime for programs written in
Rust. It provides system call wrappers, buffered I/O, files, processes,
terminals, sockets, DNS, an HTTP client and the GUI client library. Every
program is a file in `user/src/bin/`.

C programs use hcc and its libc, `/usr/lib/hcc/libc.c`. See the
[C compiler](c-compiler.md).

The base system lives in `/bin`, `/sbin` and `/usr`, and `cargo xtask build`
refreshes it on the disk. Packages live in `/pkg` and are managed
declaratively; see [packages](packages.md).

## Libraries

| crate | purpose |
|---|---|
| `huldra-abi` | system call numbers, errno, Linux structures |
| `huldra-buddy`, `huldra-kalloc` | page and heap allocators |
| `huldra-elf`, `huldra-cpio`, `huldra-ext2` | ELF loading, initrd, ext2 (+ mkfs) |
| `huldra-archive` | tar and SHA-256 |
| `huldra-hcc` | the C compiler |
| `huldra-net` | TCP/IP without I/O, tested with two stacks on a lossy "wire" |
| `huldra-pkg` | package format, index, versions, dependency resolution, declarative system model |
| `huldra-crypto` | SHA-1/2, HMAC, HKDF, ChaCha20-Poly1305, AES-GCM, X25519, RSA and ECDSA verification |
| `huldra-tls` | TLS 1.3 client and X.509 chain verification |
| `huldra-http` | URLs, HTTP/1.1 requests and an incremental response parser |
| `huldra-flate` | DEFLATE, zlib and gzip |
| `huldra-git` | git objects, packs, the index, smart HTTP, diffs |
| `huldra-web` | HTML parser and text layout for the browsers |
| `huldra-md` | Markdown to terminal text (for `help`) |
| `huldra-gfx` | drawing, fonts, display protocol, keymaps, terminal emulator core, tiling, theme |

## Design rules

- **No external crates.** Everything, from the allocator to SHA-256, is
  part of the repository. The point is to be readable from top to bottom.
- **Logic lives in libraries.** Anything that can be tested without
  hardware goes in a `no_std` crate in `libs/` with host unit tests. The
  kernel and programs supply the I/O. Examples: TCP state machines,
  ext2 layout, dependency resolution, the terminal emulator.
- **Linux compatibility at the boundary.** Real programs are the best test
  of the system call layer.
- **Everything is tested end to end.** `cargo xtask test` boots the real
  system in QEMU and drives it through the serial console; see
  [testing](testing.md).
