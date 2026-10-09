# Packages: a declarative system

Huldra manages software the way NixOS does. You do not install and remove
packages one by one, leaving the system in whatever state that history
produced. You **describe** the system in one file, `/etc/system.conf`, and
`pkg switch` makes the running system match it. Every switch produces a new
**generation**, so any change can be rolled back with one command.

```text
root@huldra:~# pkg add fortune cowsay
fetching http://10.0.2.2:8800/INDEX
  7 packages
  fetched fortune-data 1.0-2
  fetched fortune 1.2-2
  fetched cowsay 3.0-2
  + fortune-data 1.0-2
  + fortune 1.2-2
  + cowsay 3.0-2
switched to generation 1
root@huldra:~# fortune | cowsay
root@huldra:~# pkg remove fortune
  - fortune 1.2-2
  - fortune-data 1.0-2
switched to generation 2
root@huldra:~# pkg rollback
  + fortune-data 1.0-2
  + fortune 1.2-2
switched to generation 1
```

## Contents

- [The configuration](#the-configuration)
- [Commands](#commands)
- [How it works](#how-it-works)
- [Guarantees](#guarantees)
- [Writing packages](#writing-packages)
- [Repositories](#repositories)
- [Compared with NixOS](#compared-with-nixos)

## The configuration

`/etc/system.conf` has settings, then optional file sections:

```ini
# Package repositories, tried in order.
repo = http://10.0.2.2:8800

# Packages in the system profile, with their dependencies.
packages = fortune cowsay 2048
packages = sl@5.0-2 /root/mytool-1.0-1.pkg

# The host name (becomes a managed /etc/hostname).
hostname = huldra

# Files under /etc: the contents up to the next [etc/...] line.
[etc/motd]
Welcome to Huldra.

[etc/gui.conf]
wm = tilewm
autostart = term
crt = off
```

| setting | meaning |
|---|---|
| `repo = URL` | A repository (see [Repositories](#repositories)). May repeat. If there is none, `http://10.0.2.2:8800` is used. |
| `packages = ...` | Space-separated package requests. May repeat; the lists are joined. |
| `hostname = NAME` | Shorthand for an `[etc/hostname]` section containing `NAME`. |
| `[etc/PATH]` | A file at `/etc/PATH` with the lines that follow as its contents. Nested paths such as `[etc/net/hosts]` work. |

A package request is one of:

| form | meaning |
|---|---|
| `fortune` | the newest version in the repositories |
| `fortune@1.2-2` | exactly this version (a *pin*) |
| `/root/fortune-1.2-2.pkg` | a local package archive (any request containing `/` or ending in `.pkg`) |

Dependencies are added automatically and do not belong in `packages`.

Unknown settings are errors. A typo like `pakages = ...` is reported at
once instead of being silently ignored.

## Commands

| command | what it does |
|---|---|
| `pkg switch` | Builds a generation from `/etc/system.conf` and activates it. Does nothing if the result would be identical to the running generation. |
| `pkg switch -n` | Shows what `switch` would change, without changing anything. |
| `pkg add NAME...` | Adds requests to the `packages` line and switches. The file is only rewritten if the switch succeeds. |
| `pkg remove NAME...` | Removes requests (by name, pinned or not) and switches. |
| `pkg rollback` | Activates the generation before the running one. |
| `pkg rollback N` | Activates generation `N`. This works forwards too. |
| `pkg generations` | Lists generations; `*` marks the running one. |
| `pkg gc` | Deletes store paths that no generation uses. |
| `pkg gc -d` | Also deletes every generation except the running one. |
| `pkg update` | Fetches the package index again. `switch` fetches it by itself when there is none. |
| `pkg search [WORD]` | Lists available packages. |
| `pkg info NAME` | Shows a package and where it lives in the store. |
| `pkg list` | Lists the packages of the running system and marks dependencies. |
| `pkg files NAME` | Lists the files a package contributes to the profile. |

`pkg add` and `pkg remove` only edit the configuration and switch. Editing
the file by hand and running `pkg switch` is exactly equivalent.

`rollback` changes the running system but leaves `/etc/system.conf` alone.
Every generation keeps a copy of the configuration it was built from, in
`/pkg/generations/N/system.conf`. To make a rollback permanent, copy that
file back.

## How it works

```text
/etc/system.conf ──pkg switch──▶ /pkg/generations/7/
                                   manifest            what is in it
                                   system.conf         the config it came from
                                   sw/bin/fortune ───▶ /pkg/store/1f0c…-fortune-1.2-2/bin/fortune
                                   sw/share/fortune/…▶ /pkg/store/a93b…-fortune-data-1.0-2/share/…
                                   etc/motd

/pkg/system ──▶ generations/7          (one symbolic link: the running system)
/etc/motd   ──▶ /pkg/system/etc/motd   (links to the running generation)
PATH        ... :/pkg/system/sw/bin
```

**The store.** `/pkg/store/HASH-NAME-VERSION` holds one unpacked package.
`HASH` is the start of the package archive's SHA-256, so the name pins the
exact bytes. Two builds of the same version never mix, and a package that
is already in the store is never downloaded again. Store paths are never
modified after they are created.

**Generations.** `pkg switch` resolves the configuration (requests, pins,
dependencies) against the repository index and fetches whatever is missing
into the store. It then writes a new directory `/pkg/generations/N`:

- `sw/` is the *profile*: the union of all packages' files, as symbolic links
  into the store. If two packages provide the same path, the switch fails and
  names both packages.
- `etc/` holds the generated `/etc` files.
- `manifest` lists every package (name, version, store path) and every
  managed `/etc` file.
- `system.conf` is the configuration the generation was built from.

Generation numbers only grow. They are not reused, even after `gc -d`.

**Activation** changes the `/pkg/system` link to point at the new
generation. Programs run from `/pkg/system/sw/bin` (it is on `PATH`) and
read their data from `/pkg/system/sw/share`, so they switch along with the
link. For each managed `/etc` file, `/etc/NAME` becomes a link to
`/pkg/system/etc/NAME`. Its contents therefore follow the running generation
too, and links are only added or removed when the *set* of managed files
changes.

**Files that were already there.** If `/etc/NAME` exists and pkg did not
create it, pkg moves it to `/etc/NAME.before-pkg` before linking. When no
generation manages the file any more, pkg moves the old file back.

**Garbage collection.** Nothing is deleted on switch, so every generation
stays complete. `pkg gc` removes store paths that no generation refers to;
`pkg gc -d` deletes the old generations first.

## Guarantees

- **Atomic switches.** A package is unpacked into `NAME.tmp` and renamed
  into place. A generation is written as `N.tmp` and renamed. Activation is
  the rename of a new link over `/pkg/system`. If the power goes out at any
  moment, the previous system is still the running one, and leftover `.tmp`
  files are cleaned up by `pkg gc`.
- **Verified downloads.** Every archive's size and SHA-256 are checked
  against the index before it is unpacked. Paths inside archives are
  sanitized, so `..` and absolute paths are rejected.
- **One writer.** `/pkg/lock` holds the pid of the running pkg. A second pkg
  refuses to run, and a lock left behind by a pkg that died is taken over.
- **Reproducible generations.** A generation names exact store paths, so a
  rollback brings back exactly the same bytes, whatever the repository
  holds today. Pins (`name@version`) do the same for the configuration
  itself.

Not yet: package signatures. Today the checksums come from the index, which
is fetched over plain HTTP. See the [roadmap](roadmap.md).

## Writing packages

A package's source lives in `packages/NAME/`:

```text
packages/fortune/
  PKGINFO          name, version, description, depends
  src/*.c          compiled with hcc into bin/NAME (optional)
  files/...        installed as is, relative to the profile (optional)
```

```ini
name = fortune
version = 1.2-2
description = Prints a random adage
depends = fortune-data
```

Paths in a package are relative to the profile root: `bin/fortune`,
`share/fortune/fortunes`. Installed, they appear as
`/pkg/system/sw/bin/fortune`, and so on. A program that needs its data
should look under `/pkg/system/sw/share/...`. Never use `/usr/share`; that
belongs to the base system.

Versions are compared like `1.10-2 > 1.9-3`: runs of digits numerically,
everything else character by character. By convention the part after `-`
is the package release. Bump it whenever the package changes but the
upstream version does not.

`cargo xtask repo` builds every package into `target/repo/`: `NAME-VERSION.pkg`
archives plus an `INDEX`. `cargo xtask run` and `cargo xtask serve` serve
that directory on port 8800, which the guest reaches as
`http://10.0.2.2:8800`.

### The archive format

A `.pkg` file is a ustar archive. Its first member is `.PKGINFO` (the
same `key = value` format as above), followed by directories and files.
Only regular files and directories are allowed. pkg copies `.PKGINFO`
into the store path, so `pkg info` works without the repository.

## Repositories

A repository is any directory served over HTTP that contains an `INDEX`
and the archives. `INDEX` is a list of stanzas separated by blank lines:

```ini
name = fortune
version = 1.2-2
description = Prints a random adage
depends = fortune-data
file = fortune-1.2-2.pkg
size = 58368
sha256 = 1f0c9e…
```

Several versions of a package may be listed; `pkg` picks the newest one
unless a request pins a version. With several `repo` lines the indexes are
merged.

Every GitHub release attaches the repository as `huldra-VERSION-repo.tar.gz`.
Unpack it anywhere and serve it with any static HTTP server.

## Compared with NixOS

| | NixOS | Huldra |
|---|---|---|
| Description | the Nix language, `configuration.nix` | a flat `key = value` file |
| Store | `/nix/store/HASH-name`, hash of the build inputs | `/pkg/store/HASH-name-version`, hash of the binary archive |
| Building | from source, binary cache as an optimization | binary packages only (built on the host by `cargo xtask repo`) |
| Generations, rollback, gc | yes | yes |
| `/etc` | generated, linked from `/etc/static` | generated, linked from `/pkg/system/etc` |
| Dependencies | exact store paths compiled in | resolved by name, found through the profile |
| Per-user profiles | yes | not yet |
| Boot menu with generations | yes | not yet: the base system (`/bin`, the kernel) is not managed by pkg |
