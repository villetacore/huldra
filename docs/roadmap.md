# Limitations and roadmap

Huldra is a hobby system: small enough to read, complete enough to use.
These are the known gaps, roughly in order of how much they matter.
Contributions are welcome; see [CONTRIBUTING](../CONTRIBUTING.md).

## Packages

- [ ] **Signed repository indexes** (ed25519). Today checksums come from
      the index, which is only as trustworthy as its transport: serve the
      repository over HTTPS.
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
- [x] HTTPS: TLS 1.3 with certificate verification (`wget`, `git`, `pkg`, browsers).
- [ ] TLS 1.2 for servers that do not speak 1.3; the P-256 key exchange.
- [ ] Keep-alive connections (one request per connection today).

## Git

- [x] clone, fetch, pull (fast-forward), commit, push over smart HTTP(S).
- [ ] Merges (three-way, with conflicts), rebase, stash.
- [ ] Delta compression when pushing; streaming large packs to disk.
- [ ] SSH transport.

## Web browsers

- [x] `browse` and `web`: HTML, links, forms, tables, history, HTTPS.
- [ ] A little CSS (`display: none`, colors), images in `web`, cookies.

## C compiler

- [ ] VLAs, real `long double`, packed bit-fields.
- [ ] An optimization pass (register allocation for locals).

## Graphics

- [ ] More fonts than the 8×16 bitmap font; clipboard; drag and drop between
      applications.
- [ ] Hardware acceleration is out of scope. Programs written for X11 or
      Wayland will not run, since the display protocol is Huldra's own.
