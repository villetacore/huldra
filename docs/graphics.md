# Graphics

The graphical session is built like X11: a **display server** owns the
screen and the input devices, every application is a client talking a
small protocol over a socket, and the **window manager is just another
client**.

```text
 /dev/fb0  /dev/kbd  /dev/mouse
     ▲         │         │
     │         ▼         ▼
 ┌──────────────────────────────┐   TCP 127.0.0.1:6000 (DISPLAY=:0)
 │ display                      │◀───────────┬─────────────┬──────────────┐
 │  windows + backing store     │            │             │              │
 │  damage → recompose → blit   │      boxwm / tilewm    panel      term, files,
 └──────────────────────────────┘      (BecomeWm)                  paint, games…
```

## Starting a session

- `startgui`, or `startgui tilewm`;
- `cargo xtask run --gui`, or `gui` on the kernel command line.

`/etc/gui.conf` chooses the window manager, the programs to autostart, the
screen mode and the CRT effect. With [declarative packages](packages.md) it
can also be managed from `/etc/system.conf` as an `[etc/gui.conf]` section.

```ini
wm = boxwm            # or tilewm
autostart = term      # programs separated by ";"
#mode = 1024x768
crt = on              # faint scanlines; screenshots never have them
```

To leave the session, use **Exit** in the desktop menu (or Alt+Shift+E in
tilewm). Ctrl+Alt+Backspace kills it.

## The display server

`display` maps the framebuffer and reads raw keyboard and mouse events.
Every window has its own pixel buffer (backing store). When something
changes, only the damaged rectangles are recomposed and copied to the
screen, and the cursor is drawn in software. Clients choose their own
window ids (`client << 20 | n`), so creating a window and drawing into it
never waits for a reply.

### The protocol

Defined in [`libs/gfx/src/proto.rs`](../libs/gfx/src/proto.rs). Each
message is `[u32 length][u8 tag][fields]`, little endian.

| requests (client → server) | events (server → client) |
|---|---|
| `Hello`, `CreateWindow`, `DestroyWindow` | `Welcome` (client number, screen size) |
| `Map`, `Unmap`, `Configure`, `Raise`, `Lower`, `SetFocus`, `SetTitle` | `Expose`, `Configure`, `Focus`, `Crossing`, `CloseRequest` |
| `Fill`, `Text`, `Line`, `Circle`, `Image`, `Copy`, `GetImage` | `Key`, `Button`, `Motion`, `Pointer`, `ImageData` |
| `BecomeWm`, `GrabKey`, `GrabPointer`, `Activate`, `Close`, `SetStatus` | `MapRequest`, `ConfigureRequest`, `Destroyed`, `Unmapped`, `TitleChanged`, `Clicked`, `KeyGrabbed`, `WindowListItem`, `FocusChanged`, `Status`, `Error` |

Window kinds: normal, **dock** (panels: unmanaged, on top, reserve screen
space), **popup** (menus: unmanaged, shown where requested), dialog.

### Window management

Once a client sends `BecomeWm`, a request from any other client to map or
move one of its windows turns into a `MapRequest` or `ConfigureRequest` to
the window manager. The window manager decides where the window goes and
draws its frame with windows of its own.

- **boxwm** (like Openbox): title bars with minimize, maximize and close;
  dragging and resizing (also Alt+drag); double-click to maximize; a
  desktop menu on right click; Alt+Tab; Alt+F4; Ctrl+Alt+T for a terminal;
  Alt+F2 to run a command.
- **tilewm** (like i3, with `$mod` = Alt): a tree of splits. Alt+Enter opens a
  terminal and Alt+d the launcher. Alt+j/k/l/; or the arrows move focus,
  with Shift they move the window. Alt+h and Alt+v set the next split,
  Alt+e toggles the direction, Alt+f goes fullscreen. Alt+1…9 switches
  workspace, Alt+Shift+1…9 moves the window there, Alt+Shift+Q closes it,
  and Alt+Shift+B switches to boxwm.

## Programs

| program | |
|---|---|
| `term` | terminal emulator on a pseudo-terminal: xterm-256 colors, alternate screen, scrollback (Shift+PgUp, wheel) |
| `panel` | task bar: application menu, window list, workspaces, clock |
| `files` | file manager |
| `clock`, `calc`, `paint` (saves PPM), `sysinfo` | small applications |
| `menu` | dmenu-like launcher (lists programs on `PATH`) |
| `screenshot` | saves the screen |
| `mines`, `blocks`, `snake` (package) | games |

## The look

The whole system looks like an old green phosphor terminal. The
background is dark green-grey "glass" rather than black. Text is soft
green, highlights are bright green, and the accents are amber and rust.
There is one palette for everything:
[`libs/gfx/src/theme.rs`](../libs/gfx/src/theme.rs). `term` and the kernel's
VGA console use the same 16 colors.

## Writing GUI programs

**Rust**, with `huldra_user::gui`:

```rust
let mut d = Display::connect()?;
let w = d.create_window(100, 100, 300, 200, KIND_NORMAL);
d.set_title(w, "Hello");
d.map(w);
while let Some(ev) = d.wait_event(-1) {
    if let Event::Expose { w: width, h: height, .. } = ev {
        d.fill(w, Rect::new(0, 0, width, height), theme::BG);
        d.text(w, 10, 10, theme::TEXT, theme::BG, "Hello, world");
        d.flush();
    }
}
```

**C**, with `#include <gui.h>` (part of the libc, so there is nothing to link):

```c
#include <gui.h>

int main(void) {
    gui_connect();
    unsigned w = gui_window(100, 100, 320, 200, "Hello");
    struct gui_event e;
    while (gui_next_event(&e, -1) > 0) {
        if (e.type == GUI_EXPOSE) {
            gui_fill(w, 0, 0, e.w, e.h, 0x203040);
            gui_text(w, 10, 10, 0xFFFFFF, 0, "Hello, world");
            gui_flush();
        }
        if (e.type == GUI_CLOSE) break;
    }
    return 0;
}
```

Compile it in the system with `cc -run window.c`. A complete example is in
`/usr/share/huldra/examples/window.c`, and a whole game in
[`packages/snake`](../packages/snake).
