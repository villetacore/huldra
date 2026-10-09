<div align="center">

<img src="docs/logo.svg" alt="Huldra" width="560">

**Небольшая Unix-подобная операционная система для x86_64, написанная на Rust с нуля.**

[![CI](https://github.com/villetacore/huldra/actions/workflows/ci.yml/badge.svg)](https://github.com/villetacore/huldra/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/villetacore/huldra?include_prereleases&color=3e8a58)](https://github.com/villetacore/huldra/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-3e8a58)](LICENSE)
[![Rust stable](https://img.shields.io/badge/rust-stable-e0a84a?logo=rust)](rust-toolchain.toml)
[![No dependencies](https://img.shields.io/badge/crates.io%20deps-0-3e8a58)](Cargo.toml)

[Быстрый старт](docs/getting-started.md) ·
[Документация](docs/README.md) ·
[Пакеты](docs/packages.md) ·
[Git](docs/git.md) ·
[Браузер](docs/browser.md) ·
[Устройство](docs/architecture.md) ·
[Как помочь](CONTRIBUTING.md) ·
[English](README.md)

</div>

---

Huldra — это монолитное ядро с системными вызовами Linux, корневая ФС
ext2, свой стек TCP/IP с **HTTPS (TLS 1.3)**, **клиент git**, который
пушит на GitHub, **веб-браузеры** для терминала и рабочего стола,
компилятор Си прямо внутри системы, **декларативный пакетный менеджер в
духе NixOS** и графика, устроенная как X11, с плавающим и тайловым
оконными менеджерами. Всё оформлено как
старый терминал с зелёным фосфорным экраном.

Huldra запускает статические Linux-программы (glibc) без изменений.
Собирается на **stable** Rust и не тянет **ни одной зависимости с
crates.io**: весь код, от аллокатора страниц до SHA-256, лежит в этом
репозитории.

<table>
<tr>
<td><img src="docs/screenshots/boxwm.png" alt="boxwm"></td>
<td><img src="docs/screenshots/tilewm.png" alt="tilewm"></td>
</tr>
<tr>
<td align="center"><b>boxwm</b> — плавающие окна, как в Openbox</td>
<td align="center"><b>tilewm</b> — тайлинг, как в i3</td>
</tr>
</table>

## Попробовать

```bash
git clone https://github.com/villetacore/huldra && cd huldra
cargo xtask run          # нужны Rust и QEMU; с --gui — сразу рабочий стол
```

Готовые ISO и образы диска лежат в
[Releases](https://github.com/villetacore/huldra/releases). Подробности — в
[быстром старте](docs/getting-started.md).

## Как это выглядит

```text
root@huldra:~# cat /etc/system.conf
repo = http://10.0.2.2:8800
packages = fortune cowsay
hostname = huldra
root@huldra:~# pkg switch
  + fortune-data 1.0-2
  + fortune 1.2-2
  + cowsay 3.0-2
switched to generation 1
root@huldra:~# fortune | cowsay
 ________________________________________
/ Talk is cheap. Show me the code.       \
\ -- Linus Torvalds                      /
 ----------------------------------------
        \   ^__^
         \  (oo)\_______
            (__)\       )\/\
                ||----w |
                ||     ||
root@huldra:~# pkg remove fortune && pkg rollback    # любое изменение откатывается
root@huldra:~# git clone --depth 1 https://github.com/villetacore/huldra.git
Cloning into 'huldra'...
Received 658 objects, 882 KiB in 6.7s
Checked out 'main' (457 files)
root@huldra:~# browse news.ycombinator.com              # или `web` на рабочем столе
root@huldra:~# echo 'int main(void){ printf("%d\n", 6*7); }' > a.c && cc -run a.c
42
root@huldra:~# help commands                           # справка по всем командам прямо в системе
```

## Возможности

<table>
<tr><td width="50%" valign="top">

**Ядро**
- Загрузка через Multiboot2 и PVH, higher half, W^X/NX
- Вытесняющий планировщик, `fork`/`clone`-потоки, futex, сигналы, job control
- ~130 системных вызовов Linux: статические программы на glibc работают как есть
- VFS с символическими ссылками: ext2 (чтение и запись, проходит
  `e2fsck`), tmpfs, devfs, procfs, pipe
- Драйверы: e1000, ATA, PS/2, консоль VGA, кадровый буфер, псевдотерминалы, ACPI/APIC

</td><td width="50%" valign="top">

**Декларативные пакеты**
- Вся система описана в [`/etc/system.conf`](rootfs/etc/system.conf)
- Хранилище, адресуемое по содержимому, атомарные поколения, `pkg rollback`, `pkg gc`
- Управляемые файлы в `/etc`, закреплённые версии, локальные пакеты
- Бинарный репозиторий по HTTP, собирается из [`packages/`](packages)
- [Подробнее →](docs/packages.md)

</td></tr>
<tr><td valign="top">

**Программы**
- `sh` с функциями, `$(…)`, glob, историей, автодополнением и фоновыми задачами
- `edit` (подсветка синтаксиса), `less`, `fm` (в духе Midnight Commander), `top`
- Около 80 утилит, от `ls` и `grep` до `tar` и `sha256sum`
- Сеть: `ifconfig ping host wget nc httpd`

</td><td valign="top">

**Компилятор Си (`cc`)**
- Компилирует сразу в статический ELF: без ассемблера, объектных файлов и линковщика
- C99/C11 с расширениями GNU, плавающая точка на SSE, `setjmp`
- Своя libc: stdio, malloc, math, сокеты, DNS, графика
- На тестах печатает то же, что gcc. [Подробнее →](docs/c-compiler.md)

</td></tr>
<tr><td valign="top">

**Графика**
- Дисплейный сервер с протоколом, похожим на X11; оконные менеджеры — обычные клиенты
- `term` (xterm-256), `panel`, `files`, `paint`, `calc`, `clock`
- Игры: `hack`, `mines`, `blocks`, `snake`
- Свои программы с окнами — на Rust или на Си. [Подробнее →](docs/graphics.md)

</td><td valign="top">

**Сеть**
- Свой TCP/IP: ARP, IPv4, ICMP, UDP, TCP с повторной передачей, DHCP, DNS
- Стек не делает ввода-вывода, поэтому тестируется на хосте поверх «провода» с потерями
- BSD-сокеты с номерами Linux. [Подробнее →](docs/networking.md)

</td></tr>
<tr><td valign="top">

**Интернет с нуля**
- HTTPS: TLS 1.3 с X25519, ChaCha20-Poly1305/AES-GCM, ECDSA/RSA и проверкой
  цепочки сертификатов; вся криптография своя и сверена с OpenSSL
- `wget` с докачкой и прогрессом; криптостойкий генератор случайных чисел в ядре
- [Подробнее →](docs/networking.md)

</td><td valign="top">

**git и веб-браузеры**
- `git` клонирует с GitHub, коммитит и пушит, совместим с настоящим git
  (pack-файлы, индекс, smart HTTP). [Подробнее →](docs/git.md)
- `browse` (терминал) и `web` (окно): HTML, таблицы, формы, история.
  [Подробнее →](docs/browser.md)
- `help` и `man`: документация прямо в системе

</td></tr>
</table>

## Тесты

`cargo xtask test` загружает настоящую систему в QEMU и управляет ею
через последовательный порт. В набор входят unit-тесты на хосте,
сравнение hcc с gcc, тесты внутри ядра, сценарии в shell, перезагрузка с
сохранением данных, графика и пакетный менеджер. Дальше идут HTTP/HTTPS,
git и браузер против серверов на хосте, причём push из Huldra должен
пройти `git fsck --strict` на хосте. Затем Linux-программы, а в конце —
`e2fsck` диска, на который писала гостевая система. CI гоняет всё
это на каждый push. Подробнее — в [testing](docs/testing.md).

## Структура

| | |
|---|---|
| [`kernel/`](kernel) | ядро |
| [`libs/`](libs) | `no_std`-библиотеки: ext2, сеть, tls, crypto, http, git, web, hcc, pkg, gfx, flate, md, elf, архивы, аллокаторы |
| [`user/`](user) | рантайм и все программы |
| [`rootfs/`](rootfs) | базовая система: `/etc`, `/usr/include`, libc |
| [`packages/`](packages) | исходники пакетов репозитория |
| [`docs/`](docs) | документация (в системе — `/usr/share/huldra/docs`) |
| [`tests/`](tests) | сценарии для QEMU, тесты компилятора и Linux-программ |
| [`xtask/`](xtask) | сборка |

## Статус

Это хобби-проект и учебный материал. Им вполне можно пользоваться для
развлечения, и он достаточно мал, чтобы прочитать его целиком, но до
продакшена ему далеко. Система работает на одном процессоре, в ней нет
пользователей и прав доступа, а `fork` обходится без copy-on-write. Что
ещё не готово и что планируется — в [roadmap](docs/roadmap.md).

Документация в `docs/` написана на английском.

## Как помочь

Issues и pull requests приветствуются. Порядок работы и стиль кода
описаны в [CONTRIBUTING.md](CONTRIBUTING.md), а как сообщить об уязвимости —
в [SECURITY.md](SECURITY.md). От всех участников ожидается соблюдение
[кодекса поведения](CODE_OF_CONDUCT.md).

## Лицензия

[MIT](LICENSE)
