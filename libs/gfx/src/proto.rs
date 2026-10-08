//! The display protocol, spoken over a stream socket (TCP 127.0.0.1:6000
//! for display `:0`, like X11).
//!
//! Every message is `[u32 length][u8 tag][fields]` in little endian; the
//! length counts the tag and fields. Clients choose window ids themselves
//! (`client << 20 | n`, the client number comes in `Welcome`), so drawing
//! needs no round trips. Window 0 is the root (the desktop background).
//!
//! As in X11, window management is done by a separate client: once one
//! sends `BecomeWm`, mapping and moving other clients' windows becomes a
//! request to it (`MapRequest`, `ConfigureRequest`), and it is told about
//! clicks, titles and destroyed windows so it can draw frames.

use alloc::string::String;
use alloc::vec::Vec;

pub const VERSION: u16 = 1;
pub const PORT_BASE: u16 = 6000;
pub const ROOT: u32 = 0;

/// Window kinds.
pub const KIND_NORMAL: u8 = 0;
/// Panels and bars: never managed, kept on top, reserve screen space.
pub const KIND_DOCK: u8 = 1;
/// Menus and tooltips: not managed, shown where requested.
pub const KIND_POPUP: u8 = 2;
pub const KIND_DIALOG: u8 = 3;

/// Modifier bits.
pub const MOD_SHIFT: u8 = 1;
pub const MOD_CTRL: u8 = 2;
pub const MOD_ALT: u8 = 4;
pub const MOD_SUPER: u8 = 8;

/// Mouse buttons (also bits in `Motion::buttons`).
pub const BUTTON_LEFT: u8 = 1;
pub const BUTTON_RIGHT: u8 = 2;
pub const BUTTON_MIDDLE: u8 = 4;
/// Wheel "buttons" (press only).
pub const WHEEL_UP: u8 = 8;
pub const WHEEL_DOWN: u8 = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Hello { version: u16 },
    CreateWindow { id: u32, x: i32, y: i32, w: i32, h: i32, kind: u8 },
    DestroyWindow { id: u32 },
    Map { id: u32 },
    Unmap { id: u32 },
    Configure { id: u32, x: i32, y: i32, w: i32, h: i32 },
    SetTitle { id: u32, title: String },
    Raise { id: u32 },
    Lower { id: u32 },
    SetFocus { id: u32 },
    Fill { id: u32, x: i32, y: i32, w: i32, h: i32, color: u32 },
    /// `bg` 0 draws only the glyph pixels.
    Text { id: u32, x: i32, y: i32, fg: u32, bg: u32, text: String },
    Line { id: u32, x0: i32, y0: i32, x1: i32, y1: i32, color: u32 },
    Image { id: u32, x: i32, y: i32, w: i32, h: i32, pixels: Vec<u32> },
    Copy { id: u32, x: i32, y: i32, w: i32, h: i32, dx: i32, dy: i32 },
    BecomeWm,
    GrabKey { code: u16, mods: u8 },
    GrabPointer { id: u32 },
    UngrabPointer,
    /// Ask the window's owner to close it (WM_DELETE_WINDOW).
    Close { id: u32 },
    /// Receive window list, focus and status events (panels).
    SelectWindows,
    /// Ask the window manager to show and focus a window.
    Activate { id: u32 },
    /// Status text shown by panels (workspaces, a clock...).
    SetStatus { text: String },
    /// Shut the display server down (log out).
    Quit,
    /// Draw a filled circle.
    Circle { id: u32, x: i32, y: i32, r: i32, color: u32 },
    /// Ask for the pointer position (answered with `Pointer`).
    QueryPointer,
    /// Read pixels back (answered with `ImageData`); window 0 reads the
    /// composed screen.
    GetImage { id: u32, x: i32, y: i32, w: i32, h: i32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Welcome { client: u32, width: i32, height: i32 },
    /// The window became visible or changed size: redraw everything.
    Expose { id: u32, w: i32, h: i32 },
    Configure { id: u32, x: i32, y: i32, w: i32, h: i32 },
    /// `code` is the scancode (+0x100 for extended keys); `ch` the
    /// character it types with the current modifiers, or 0.
    Key { id: u32, code: u16, pressed: bool, mods: u8, ch: u32 },
    /// Coordinates relative to the window, then to the screen.
    Button { id: u32, x: i32, y: i32, rx: i32, ry: i32, button: u8, pressed: bool, mods: u8 },
    Motion { id: u32, x: i32, y: i32, rx: i32, ry: i32, buttons: u8 },
    Focus { id: u32, focused: bool },
    Crossing { id: u32, entered: bool },
    CloseRequest { id: u32 },
    MapRequest { id: u32, client: u32, x: i32, y: i32, w: i32, h: i32, kind: u8, title: String },
    ConfigureRequest { id: u32, x: i32, y: i32, w: i32, h: i32 },
    Destroyed { id: u32 },
    Unmapped { id: u32 },
    TitleChanged { id: u32, title: String },
    ActivateRequest { id: u32 },
    KeyGrabbed { code: u16, mods: u8, pressed: bool },
    /// A button was pressed on a window (told to the WM, for focus).
    Clicked { id: u32, rx: i32, ry: i32, button: u8, mods: u8 },
    WindowListItem { id: u32, title: String, mapped: bool },
    WindowRemoved { id: u32 },
    FocusChanged { id: u32 },
    Status { text: String },
    Pointer { x: i32, y: i32, buttons: u8 },
    ImageData { w: i32, h: i32, pixels: Vec<u32> },
    Error { code: u32, message: String },
}

/// Little-endian writer.
#[derive(Default)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        let b = &s.as_bytes()[..s.len().min(4096)];
        self.u16(b.len() as u16);
        self.buf.extend_from_slice(b);
    }
    fn bool(&mut self, v: bool) {
        self.u8(v as u8);
    }

    /// Starts a message; `end` fills in its length.
    fn begin(&mut self, tag: u8) -> usize {
        let at = self.buf.len();
        self.u32(0);
        self.u8(tag);
        at
    }
    fn end(&mut self, at: usize) {
        let len = (self.buf.len() - at - 4) as u32;
        self.buf[at..at + 4].copy_from_slice(&len.to_le_bytes());
    }
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.pos..self.pos + n)?;
        self.pos += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(self.u32()? as i32)
    }
    fn str(&mut self) -> Option<String> {
        let n = self.u16()? as usize;
        Some(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn bool(&mut self) -> Option<bool> {
        Some(self.u8()? != 0)
    }
}

/// Splits one complete message off the front of `buf`: (tag, payload,
/// bytes used). `None` if more data is needed.
pub fn frame(buf: &[u8]) -> Option<(u8, &[u8], usize)> {
    let len = u32::from_le_bytes(buf.get(..4)?.try_into().ok()?) as usize;
    let body = buf.get(4..4 + len)?;
    let (&tag, payload) = body.split_first()?;
    Some((tag, payload, 4 + len))
}

/// A message length above this is a protocol error.
pub const MAX_MESSAGE: usize = 16 << 20;

pub fn message_too_long(buf: &[u8]) -> bool {
    buf.len() >= 4 && u32::from_le_bytes(buf[..4].try_into().unwrap()) as usize > MAX_MESSAGE
}

impl Request {
    pub fn encode(&self, w: &mut Writer) {
        use Request::*;
        let tag = match self {
            Hello { .. } => 1,
            CreateWindow { .. } => 2,
            DestroyWindow { .. } => 3,
            Map { .. } => 4,
            Unmap { .. } => 5,
            Configure { .. } => 6,
            SetTitle { .. } => 7,
            Raise { .. } => 8,
            Lower { .. } => 9,
            SetFocus { .. } => 10,
            Fill { .. } => 11,
            Text { .. } => 12,
            Line { .. } => 13,
            Image { .. } => 14,
            Copy { .. } => 15,
            BecomeWm => 16,
            GrabKey { .. } => 17,
            GrabPointer { .. } => 18,
            UngrabPointer => 19,
            Close { .. } => 20,
            SelectWindows => 21,
            Activate { .. } => 22,
            SetStatus { .. } => 23,
            Quit => 24,
            Circle { .. } => 25,
            QueryPointer => 26,
            GetImage { .. } => 27,
        };
        let at = w.begin(tag);
        match self {
            Hello { version } => w.u16(*version),
            CreateWindow { id, x, y, w: ww, h, kind } => {
                w.u32(*id);
                w.i32(*x);
                w.i32(*y);
                w.i32(*ww);
                w.i32(*h);
                w.u8(*kind);
            }
            DestroyWindow { id } | Map { id } | Unmap { id } | Raise { id } | Lower { id } | SetFocus { id } | GrabPointer { id } | Close { id } | Activate { id } => w.u32(*id),
            Configure { id, x, y, w: ww, h } => {
                w.u32(*id);
                w.i32(*x);
                w.i32(*y);
                w.i32(*ww);
                w.i32(*h);
            }
            SetTitle { id, title } => {
                w.u32(*id);
                w.str(title);
            }
            Fill { id, x, y, w: ww, h, color } => {
                w.u32(*id);
                w.i32(*x);
                w.i32(*y);
                w.i32(*ww);
                w.i32(*h);
                w.u32(*color);
            }
            Text { id, x, y, fg, bg, text } => {
                w.u32(*id);
                w.i32(*x);
                w.i32(*y);
                w.u32(*fg);
                w.u32(*bg);
                w.str(text);
            }
            Line { id, x0, y0, x1, y1, color } => {
                w.u32(*id);
                w.i32(*x0);
                w.i32(*y0);
                w.i32(*x1);
                w.i32(*y1);
                w.u32(*color);
            }
            Image { id, x, y, w: ww, h, pixels } => {
                w.u32(*id);
                w.i32(*x);
                w.i32(*y);
                w.i32(*ww);
                w.i32(*h);
                for p in pixels {
                    w.u32(*p);
                }
            }
            Copy { id, x, y, w: ww, h, dx, dy } => {
                w.u32(*id);
                w.i32(*x);
                w.i32(*y);
                w.i32(*ww);
                w.i32(*h);
                w.i32(*dx);
                w.i32(*dy);
            }
            BecomeWm | UngrabPointer | SelectWindows | Quit | QueryPointer => {}
            GrabKey { code, mods } => {
                w.u16(*code);
                w.u8(*mods);
            }
            SetStatus { text } => w.str(text),
            Circle { id, x, y, r, color } => {
                w.u32(*id);
                w.i32(*x);
                w.i32(*y);
                w.i32(*r);
                w.u32(*color);
            }
            GetImage { id, x, y, w: ww, h } => {
                w.u32(*id);
                w.i32(*x);
                w.i32(*y);
                w.i32(*ww);
                w.i32(*h);
            }
        }
        w.end(at);
    }

    pub fn decode(tag: u8, p: &[u8]) -> Option<Request> {
        use Request::*;
        let mut r = Reader { b: p, pos: 0 };
        Some(match tag {
            1 => Hello { version: r.u16()? },
            2 => CreateWindow { id: r.u32()?, x: r.i32()?, y: r.i32()?, w: r.i32()?, h: r.i32()?, kind: r.u8()? },
            3 => DestroyWindow { id: r.u32()? },
            4 => Map { id: r.u32()? },
            5 => Unmap { id: r.u32()? },
            6 => Configure { id: r.u32()?, x: r.i32()?, y: r.i32()?, w: r.i32()?, h: r.i32()? },
            7 => SetTitle { id: r.u32()?, title: r.str()? },
            8 => Raise { id: r.u32()? },
            9 => Lower { id: r.u32()? },
            10 => SetFocus { id: r.u32()? },
            11 => Fill { id: r.u32()?, x: r.i32()?, y: r.i32()?, w: r.i32()?, h: r.i32()?, color: r.u32()? },
            12 => Text { id: r.u32()?, x: r.i32()?, y: r.i32()?, fg: r.u32()?, bg: r.u32()?, text: r.str()? },
            13 => Line { id: r.u32()?, x0: r.i32()?, y0: r.i32()?, x1: r.i32()?, y1: r.i32()?, color: r.u32()? },
            14 => {
                let (id, x, y, w, h) = (r.u32()?, r.i32()?, r.i32()?, r.i32()?, r.i32()?);
                let n = (w.max(0) as usize).checked_mul(h.max(0) as usize)?;
                let raw = r.take(n.checked_mul(4)?)?;
                let pixels = raw.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
                Image { id, x, y, w, h, pixels }
            }
            15 => Copy { id: r.u32()?, x: r.i32()?, y: r.i32()?, w: r.i32()?, h: r.i32()?, dx: r.i32()?, dy: r.i32()? },
            16 => BecomeWm,
            17 => GrabKey { code: r.u16()?, mods: r.u8()? },
            18 => GrabPointer { id: r.u32()? },
            19 => UngrabPointer,
            20 => Close { id: r.u32()? },
            21 => SelectWindows,
            22 => Activate { id: r.u32()? },
            23 => SetStatus { text: r.str()? },
            24 => Quit,
            25 => Circle { id: r.u32()?, x: r.i32()?, y: r.i32()?, r: r.i32()?, color: r.u32()? },
            26 => QueryPointer,
            27 => GetImage { id: r.u32()?, x: r.i32()?, y: r.i32()?, w: r.i32()?, h: r.i32()? },
            _ => return None,
        })
    }
}

impl Event {
    pub fn encode(&self, w: &mut Writer) {
        use Event::*;
        let tag = match self {
            Welcome { .. } => 1,
            Expose { .. } => 2,
            Configure { .. } => 3,
            Key { .. } => 4,
            Button { .. } => 5,
            Motion { .. } => 6,
            Focus { .. } => 7,
            Crossing { .. } => 8,
            CloseRequest { .. } => 9,
            MapRequest { .. } => 20,
            ConfigureRequest { .. } => 21,
            Destroyed { .. } => 22,
            Unmapped { .. } => 23,
            TitleChanged { .. } => 24,
            ActivateRequest { .. } => 25,
            KeyGrabbed { .. } => 26,
            Clicked { .. } => 27,
            WindowListItem { .. } => 30,
            WindowRemoved { .. } => 31,
            FocusChanged { .. } => 32,
            Status { .. } => 33,
            Pointer { .. } => 34,
            ImageData { .. } => 35,
            Error { .. } => 40,
        };
        let at = w.begin(tag);
        match self {
            Welcome { client, width, height } => {
                w.u32(*client);
                w.i32(*width);
                w.i32(*height);
            }
            Expose { id, w: ww, h } => {
                w.u32(*id);
                w.i32(*ww);
                w.i32(*h);
            }
            Configure { id, x, y, w: ww, h } | ConfigureRequest { id, x, y, w: ww, h } => {
                w.u32(*id);
                w.i32(*x);
                w.i32(*y);
                w.i32(*ww);
                w.i32(*h);
            }
            Key { id, code, pressed, mods, ch } => {
                w.u32(*id);
                w.u16(*code);
                w.bool(*pressed);
                w.u8(*mods);
                w.u32(*ch);
            }
            Button { id, x, y, rx, ry, button, pressed, mods } => {
                w.u32(*id);
                w.i32(*x);
                w.i32(*y);
                w.i32(*rx);
                w.i32(*ry);
                w.u8(*button);
                w.bool(*pressed);
                w.u8(*mods);
            }
            Motion { id, x, y, rx, ry, buttons } => {
                w.u32(*id);
                w.i32(*x);
                w.i32(*y);
                w.i32(*rx);
                w.i32(*ry);
                w.u8(*buttons);
            }
            Focus { id, focused } => {
                w.u32(*id);
                w.bool(*focused);
            }
            Crossing { id, entered } => {
                w.u32(*id);
                w.bool(*entered);
            }
            CloseRequest { id } | Destroyed { id } | Unmapped { id } | ActivateRequest { id } | WindowRemoved { id } | FocusChanged { id } => w.u32(*id),
            MapRequest { id, client, x, y, w: ww, h, kind, title } => {
                w.u32(*id);
                w.u32(*client);
                w.i32(*x);
                w.i32(*y);
                w.i32(*ww);
                w.i32(*h);
                w.u8(*kind);
                w.str(title);
            }
            TitleChanged { id, title } => {
                w.u32(*id);
                w.str(title);
            }
            KeyGrabbed { code, mods, pressed } => {
                w.u16(*code);
                w.u8(*mods);
                w.bool(*pressed);
            }
            Clicked { id, rx, ry, button, mods } => {
                w.u32(*id);
                w.i32(*rx);
                w.i32(*ry);
                w.u8(*button);
                w.u8(*mods);
            }
            WindowListItem { id, title, mapped } => {
                w.u32(*id);
                w.str(title);
                w.bool(*mapped);
            }
            Status { text } => w.str(text),
            Pointer { x, y, buttons } => {
                w.i32(*x);
                w.i32(*y);
                w.u8(*buttons);
            }
            ImageData { w: ww, h, pixels } => {
                w.i32(*ww);
                w.i32(*h);
                for p in pixels {
                    w.u32(*p);
                }
            }
            Error { code, message } => {
                w.u32(*code);
                w.str(message);
            }
        }
        w.end(at);
    }

    pub fn decode(tag: u8, p: &[u8]) -> Option<Event> {
        use Event::*;
        let mut r = Reader { b: p, pos: 0 };
        Some(match tag {
            1 => Welcome { client: r.u32()?, width: r.i32()?, height: r.i32()? },
            2 => Expose { id: r.u32()?, w: r.i32()?, h: r.i32()? },
            3 => Configure { id: r.u32()?, x: r.i32()?, y: r.i32()?, w: r.i32()?, h: r.i32()? },
            4 => Key { id: r.u32()?, code: r.u16()?, pressed: r.bool()?, mods: r.u8()?, ch: r.u32()? },
            5 => Button { id: r.u32()?, x: r.i32()?, y: r.i32()?, rx: r.i32()?, ry: r.i32()?, button: r.u8()?, pressed: r.bool()?, mods: r.u8()? },
            6 => Motion { id: r.u32()?, x: r.i32()?, y: r.i32()?, rx: r.i32()?, ry: r.i32()?, buttons: r.u8()? },
            7 => Focus { id: r.u32()?, focused: r.bool()? },
            8 => Crossing { id: r.u32()?, entered: r.bool()? },
            9 => CloseRequest { id: r.u32()? },
            20 => MapRequest { id: r.u32()?, client: r.u32()?, x: r.i32()?, y: r.i32()?, w: r.i32()?, h: r.i32()?, kind: r.u8()?, title: r.str()? },
            21 => ConfigureRequest { id: r.u32()?, x: r.i32()?, y: r.i32()?, w: r.i32()?, h: r.i32()? },
            22 => Destroyed { id: r.u32()? },
            23 => Unmapped { id: r.u32()? },
            24 => TitleChanged { id: r.u32()?, title: r.str()? },
            25 => ActivateRequest { id: r.u32()? },
            26 => KeyGrabbed { code: r.u16()?, mods: r.u8()?, pressed: r.bool()? },
            27 => Clicked { id: r.u32()?, rx: r.i32()?, ry: r.i32()?, button: r.u8()?, mods: r.u8()? },
            30 => WindowListItem { id: r.u32()?, title: r.str()?, mapped: r.bool()? },
            31 => WindowRemoved { id: r.u32()? },
            32 => FocusChanged { id: r.u32()? },
            33 => Status { text: r.str()? },
            34 => Pointer { x: r.i32()?, y: r.i32()?, buttons: r.u8()? },
            35 => {
                let (w, h) = (r.i32()?, r.i32()?);
                let n = (w.max(0) as usize).checked_mul(h.max(0) as usize)?;
                let raw = r.take(n.checked_mul(4)?)?;
                ImageData { w, h, pixels: raw.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect() }
            }
            40 => Error { code: r.u32()?, message: r.str()? },
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn round_trip() {
        let reqs = vec![
            Request::Hello { version: VERSION },
            Request::CreateWindow { id: 1 << 20 | 1, x: -5, y: 10, w: 640, h: 480, kind: KIND_NORMAL },
            Request::Text { id: 3, x: 1, y: 2, fg: 0xFFFFFFFF, bg: 0, text: "привет".into() },
            Request::Image { id: 3, x: 0, y: 0, w: 2, h: 1, pixels: vec![1, 2] },
            Request::Quit,
            Request::GrabKey { code: 0x11C, mods: MOD_ALT },
        ];
        let mut w = Writer::default();
        for r in &reqs {
            r.encode(&mut w);
        }
        let mut buf = &w.buf[..];
        for r in &reqs {
            let (tag, p, used) = frame(buf).unwrap();
            assert_eq!(&Request::decode(tag, p).unwrap(), r);
            buf = &buf[used..];
        }
        assert!(buf.is_empty());

        let evs = vec![
            Event::Key { id: 9, code: 0x1E, pressed: true, mods: MOD_SHIFT, ch: 'A' as u32 },
            Event::MapRequest { id: 2, client: 1, x: 0, y: 0, w: 10, h: 10, kind: 0, title: "term".into() },
            Event::Button { id: 1, x: 2, y: 3, rx: 4, ry: 5, button: BUTTON_LEFT, pressed: false, mods: 0 },
        ];
        let mut w = Writer::default();
        for e in &evs {
            e.encode(&mut w);
        }
        let (tag, p, used) = frame(&w.buf).unwrap();
        assert_eq!(Event::decode(tag, p).unwrap(), evs[0]);
        assert!(frame(&w.buf[used..used + 3]).is_none());
    }
}
