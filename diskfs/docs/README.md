# Huldra

Небольшая Unix-подобная операционная система для x86_64 на Rust: монолитное
модульное ядро, системные вызовы с номерами Linux, user space со своим shell
и утилитами, ext2 на IDE-диске. Собирается на **stable** Rust, без внешних
зависимостей от crates.io.

```text
Huldra 0.2.0 (init: pid 1)
root@huldra:~# ls /mnt/scripts | grep sh && /mnt/scripts/hello.sh world
hello.sh
Hello from an ext2 disk, world
Huldra huldra 0.2.0 #1 SMP x86_64
root@huldra:~# ps
  PID  PPID  PGID S     TIME     VSZ  CMD
    0     0     0 R     0:00       0  [idle]
    1     0     1 S     0:00    8292  /sbin/init
    2     1     2 S     0:00    8296  -sh
    7     2     7 R     0:00    8284  ps
```

## Быстрый старт

Нужны Rust (stable, target ставится сам через `rust-toolchain.toml`) и QEMU.

```bash
cargo xtask run
```

Собирает ядро, user space, initrd и диск и запускает QEMU; консоль доступна
и в окне QEMU, и в терминале (serial). Другие команды:

| команда | что делает |
|---|---|
| `cargo xtask build [--release]` | ядро + программы + `target/initrd.cpio` + `target/disk.img` |
| `cargo xtask run [--release] [--headless] [--append "debug"]` | запуск в QEMU |
| `cargo xtask test` | все тесты (см. ниже) |
| `cargo xtask fsck` | создать образ ext2 и проверить его настоящим `e2fsck` |
| `cargo xtask iso` | загрузочный ISO с GRUB (нужен `grub-mkrescue`, например в WSL) |
| `cargo xtask run --gdb` | ждать отладчик на `localhost:1234` |

Параметры ядра (`--append`): `init=/bin/sh`, `debug`, `quiet`, `ktest`.
Диск `target/disk.img` сохраняет содержимое между запусками; удалите его,
чтобы пересоздать из `diskfs/`.

## Что внутри

### Ядро (`kernel/`)

- **Загрузка**: Multiboot2 (GRUB) и PVH (`qemu -kernel`). Ядро в higher half
  (`0xFFFFFFFF80000000`), секции отображены с правами W^X и NX.
- **Память**: прямое отображение всей физической памяти, buddy-аллокатор
  страниц, slab-куча, стеки ядра с guard-страницами, адресные пространства
  процессов с VMA, подкачка по требованию (heap, стек, `mmap`).
- **Процессы**: вытесняющий round-robin планировщик, очереди ожидания без
  потерянных пробуждений, спящий `Mutex`, `fork`/`execve`/`exit`/`wait4`,
  группы процессов и сессии, загрузка статических ELF, static-pie и `#!`-скриптов.
- **Системные вызовы**: `syscall`/`sysret` (и `int 0x80`), около 70 вызовов
  с номерами и структурами Linux x86_64. Указатели из user space проверяются
  по VMA, плохой указатель даёт `EFAULT`, а не панику.
- **Сигналы**: `sigaction`, `sigprocmask`, `kill`, обработчики в user space
  через `rt_sigreturn`, `SA_RESTART`, `EINTR`; исключения CPU превращаются
  в `SIGSEGV`/`SIGILL`/`SIGFPE`.
- **VFS**: трейты `Inode`/`FileSystem`, точки монтирования, общие смещения
  открытых файлов, таблицы дескрипторов, `pipe`.
  Файловые системы: tmpfs, devfs, procfs, ext2.
- **TTY**: канонический и raw режимы, эхо, редактирование строки, `^C` → `SIGINT`
  группе переднего плана, `termios` через `ioctl`.
- **Драйверы**: ACPI (MADT), локальный APIC (таймер) и I/O APIC (откат на
  8259 PIC и PIT), PCI, ATA PIO (LBA28/48) с кэшем секторов, PS/2-клавиатура,
  COM1, VGA с ANSI-цветами, CMOS RTC.
- **Диагностика**: журнал ядра с уровнями (`dmesg`, `/proc/kmsg`),
  `/proc/{meminfo,interrupts,pci,mounts,cpuinfo,<pid>/...}`.

### Библиотеки (`libs/`)

Части без зависимости от железа, тестируемые обычным `cargo test`:

| crate | назначение |
|---|---|
| `huldra-abi` | номера вызовов, errno, `stat`, `termios`, `sigaction` (раскладки Linux) |
| `huldra-buddy` | buddy-аллокатор физических страниц |
| `huldra-kalloc` | slab-аллокатор (куча ядра и user space) |
| `huldra-elf` | разбор ELF64 |
| `huldra-cpio` | чтение и запись cpio newc (initrd) |
| `huldra-ext2` | ext2: чтение, запись, `mkfs` |

### User space (`user/`)

`huldra-user` — маленькая «libc»: точка входа, системные вызовы, буферизованный
ввод-вывод, файлы, процессы, сигналы, куча на `mmap`. Программы:

`init`, `sh` (пайпы, `;` `&&` `||`, `&`, перенаправления `< > >> 2> 2>&1`,
переменные, `$?`, `$1`, скрипты), `ls cat echo mkdir rm rmdir touch cp mv pwd
uname ps kill free uptime dmesg mount umount sleep clear reboot poweroff head
tail wc grep env true false date stat hexdump tee sync yes lspci utest`.

Корневая ФС — initrd из `rootfs/` + собранные программы. `init` монтирует
`/etc/fstab` (`/dev/hda` → `/mnt`), печатает приветствие и держит `sh`
на консоли.

## Тесты

`cargo xtask test` запускает:

1. unit-тесты библиотек на хосте (buddy, slab, ELF, cpio, ext2, ABI);
2. тесты внутри ядра (`ktest`): память, планировщик, VFS, пайпы, procfs;
3. сценарий shell по serial (`tests/shell.txt`), включая `utest` — 20 тестов
   системных вызовов из user space;
4. проверку диска после сеанса: файл, записанный гостем на ext2, читается
   на хосте, а образ проходит `e2fsck -fn` (через WSL на Windows).

## Структура

```text
kernel/src/
  arch/x86_64/   boot.S, entry.S, GDT/IDT, APIC, ACPI, paging, context switch
  mm/            buddy, slab-куча, таблицы ядра, стеки ядра
  task/          задачи, планировщик, очереди ожидания
  proc/          адресные пространства, exec, fork/exit/wait, сигналы, uaccess
  syscall/       системные вызовы
  fs/            VFS, tmpfs, devfs, procfs, ext2, pipe, initrd
  drivers/       tty, vga, serial, keyboard, rtc, pci, ata, block
libs/            переиспользуемые no_std-библиотеки
user/            libc-замена и программы (src/bin)
xtask/           сборка, образы, запуск QEMU, тесты
rootfs/          содержимое initrd
diskfs/          содержимое ext2-диска
```

## Ограничения и дальнейшие шаги

- Один процессор. Данные per-CPU и APIC уже есть, нет запуска остальных
  ядер (AP) и блокировок, рассчитанных на SMP.
- `fork` копирует память целиком: копирования при записи (COW) пока нет.
- Нет символических ссылок, прав доступа и пользователей (всё от root),
  кэша путей (dentry cache) и общих (`MAP_SHARED`) отображений файлов.
- Job control неполный: `SIGTSTP`/`SIGSTOP` игнорируются.
- Нет динамической линковки; совместимость со статическими бинарниками
  musl — цель ABI, но не проверялась.
- Загрузка через GRUB (`cargo xtask iso`) собрана, но в этой среде не
  тестировалась; путь PVH проверяется автотестами.
- Нет сети и USB.
