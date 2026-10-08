# Huldra

Небольшое Unix-подобное ядро для x86_64 на Rust. Работает на **stable** Rust
(target `x86_64-unknown-none`), без внешних crate-зависимостей.

## Что уже есть

- **Загрузка**: Multiboot2 (GRUB) и PVH (`qemu -kernel` напрямую, без ISO).
  32-битный вход в [boot.S](src/arch/boot.S): identity-mapping первых 4 GiB
  страницами по 2 MiB, переход в long mode.
- **CPU**: GDT с TSS (отдельный IST-стек для double fault), IDT, общая точка
  входа прерываний с `TrapFrame` ([trap.S](src/arch/trap.S), [trap.rs](src/arch/trap.rs)),
  дамп регистров при исключениях.
- **Прерывания**: 8259 PIC (IRQ 32–47), таймер PIT на 100 Hz, PS/2-клавиатура.
- **Системные вызовы**: `int 0x80` с нумерацией как в Linux (`write`, `getpid`).
- **Память**: парсинг карты памяти от загрузчика, аллокатор физических фреймов,
  куча ядра 8 MiB (first-fit со слиянием блоков) — работают `Vec`, `String`, `BTreeMap`.
- **Консоль**: VGA text mode + дублирование в COM1 (с ANSI-цветами).
- **ФС**: in-memory tmpfs на `/` (`/etc`, `/root`, `/tmp`, …).
- **Shell**: `ls cd pwd cat touch mkdir rm echo uname uptime free bootinfo cpuinfo
  syscall clear reboot poweroff`, перенаправление `>` и `>>`.

## Сборка

```bash
cargo build --release
```

Нужный target ставится автоматически через `rust-toolchain.toml`.
Ядро: `target/x86_64-unknown-none/release/huldra`.

## Запуск в QEMU

Установить QEMU (Windows):

```bash
winget install SoftwareFreedomConservancy.QEMU
```

Запустить (PowerShell):

```powershell
.\scripts\run.ps1
```

или, если `qemu-system-x86_64` есть в `PATH`, просто `cargo run --release`.
Shell доступен и в окне QEMU, и в терминале через serial.

Загрузочный ISO через GRUB (Linux/WSL):

```bash
./scripts/make-iso.sh
```

## Структура

```
src/
  main.rs          kernel_main, panic handler
  arch/            boot.S, trap.S, GDT/TSS, IDT, PIC, PIT, порты
  drivers/         vga, serial (COM1), keyboard (PS/2)
  mm/              frame (физические фреймы), heap (global allocator)
  fs/              VFS-обёртка + tmpfs
  bootinfo.rs      разбор Multiboot2 / PVH
  syscall.rs       int 0x80
  shell.rs         встроенный shell
  sync.rs          SpinLock с отключением прерываний
linker.ld          ядро линкуется на 1 MiB
```

## Дальше по плану

1. Страничная память: свои таблицы страниц, higher-half ядро, `map/unmap`.
2. Процессы: планировщик (round-robin по таймеру), переключение контекста.
3. User mode: переход в ring 3, `syscall`/`sysret`, загрузка ELF.
4. VFS с файловыми дескрипторами: `open/read/write/close`, `/dev/console`.
5. `fork`/`exec`/`wait`, init и shell в user space.
6. Драйверы: APIC, ATA/virtio-blk, файловая система на диске (ext2).
