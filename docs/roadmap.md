# Limitations and roadmap

Huldra is a hobby system: small enough to read, complete enough to use.
These are the known gaps, roughly in order of how much they matter.
Contributions are welcome; see [CONTRIBUTING](../CONTRIBUTING.md).

## Packages

- [ ] **Signed repository indexes** (ed25519). Today checksums come from
      an index fetched over plain HTTP.
- [ ] Version constraints in `depends` (`libfoo>=1.2`), plus `provides`
      and `conflicts`.
- [ ] Manage the base system (`/bin`, the kernel) as packages, so that a
      generation describes the whole machine, with generations to choose
      from at boot.
- [ ] Per-user profiles (`~/.pkg/profile`).
- [ ] A richer build recipe: several programs per package, Rust programs.

## Kernel

- [ ] SMP (one CPU today).
- [ ] Copy-on-write `fork` and `MAP_SHARED`.
- [ ] Users, groups and permission checks.
- [ ] Hard links.
- [ ] Dynamic linking (static Linux binaries only today).
- [ ] USB, sound.

## Networking

- [ ] IPv6.
- [ ] TCP congestion control and reassembly of out-of-order segments.
- [ ] TLS (an HTTPS client for `wget` and `pkg`).

## C compiler

- [ ] VLAs, real `long double`, packed bit-fields.
- [ ] An optimization pass (register allocation for locals).

## Graphics

- [ ] More fonts than the 8×16 bitmap font; clipboard; drag and drop between
      applications.
- [ ] Hardware acceleration is out of scope. Programs written for X11 or
      Wayland will not run, since the display protocol is Huldra's own.
