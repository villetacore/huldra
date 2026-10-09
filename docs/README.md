# Huldra documentation

| | |
|---|---|
| [Getting started](getting-started.md) | Run a release, build from source, the `xtask` commands, first steps, debugging |
| [Architecture](architecture.md) | The big picture: boot, memory, processes, system calls, file systems, devices, libraries |
| [Packages](packages.md) | Declarative system configuration: `/etc/system.conf`, generations, rollback, writing packages, repositories |
| [Graphics](graphics.md) | The display server, its protocol, the window managers, writing GUI programs in Rust and C |
| [C compiler](c-compiler.md) | hcc: how it works, the language, the C library |
| [Networking](networking.md) | TCP/IP, HTTP and HTTPS (TLS 1.3), certificates, `wget` and the other tools |
| [Git](git.md) | The git client: clone, commit, push to GitHub, how it works |
| [Web browsers](browser.md) | `browse` (terminal) and `web` (window): keys, what pages work, the engine |
| [Testing](testing.md) | `cargo xtask test`, session scripts, CI, releases |
| [Roadmap](roadmap.md) | Limitations and what comes next |

Inside the system, the same pages are in `/usr/share/huldra/docs`: `help`
lists them, `help NAME` shows one (`help packages`, `help git`), and
`help commands` lists every program with what it does.
