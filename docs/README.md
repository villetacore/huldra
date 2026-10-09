# Huldra documentation

| | |
|---|---|
| [Getting started](getting-started.md) | Run a release, build from source, the `xtask` commands, first steps, debugging |
| [Architecture](architecture.md) | The big picture: boot, memory, processes, system calls, file systems, devices, libraries |
| [Packages](packages.md) | Declarative system configuration: `/etc/system.conf`, generations, rollback, writing packages, repositories |
| [Graphics](graphics.md) | The display server, its protocol, the window managers, writing GUI programs in Rust and C |
| [C compiler](c-compiler.md) | hcc: how it works, the language, the C library |
| [Networking](networking.md) | The TCP/IP stack, configuration, tools |
| [Testing](testing.md) | `cargo xtask test`, session scripts, CI, releases |
| [Roadmap](roadmap.md) | Limitations and what comes next |

Inside the system, the same files are in `/usr/share/huldra/docs`. Read them
with `less`.
