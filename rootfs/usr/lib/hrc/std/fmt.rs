//! Formatting: `Formatter`, the formatting traits and their
//! implementations for builtin types.

use crate::string::String;

pub type Result = crate::result::Result<(), Error>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Error;

impl Display for Error {
    fn fmt(&self, f: &mut Formatter) -> Result {
        f.pad("an error occurred when formatting an argument")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Right,
    Center,
}

pub trait Display {
    fn fmt(&self, f: &mut Formatter) -> Result;
}
pub trait Debug {
    fn fmt(&self, f: &mut Formatter) -> Result;
}
pub trait LowerHex {
    fn fmt(&self, f: &mut Formatter) -> Result;
}
pub trait UpperHex {
    fn fmt(&self, f: &mut Formatter) -> Result;
}
pub trait Octal {
    fn fmt(&self, f: &mut Formatter) -> Result;
}
pub trait Binary {
    fn fmt(&self, f: &mut Formatter) -> Result;
}
pub trait LowerExp {
    fn fmt(&self, f: &mut Formatter) -> Result;
}
pub trait UpperExp {
    fn fmt(&self, f: &mut Formatter) -> Result;
}
pub trait Pointer {
    fn fmt(&self, f: &mut Formatter) -> Result;
}

pub trait Write {
    fn write_str(&mut self, s: &str) -> Result;
    fn write_char(&mut self, c: char) -> Result {
        let mut b = [0u8; 4];
        self.write_str(c.encode_utf8(&mut b))
    }
    fn write_fmt(&mut self, args: Arguments) -> Result {
        self.write_str(args.as_str())
    }
}

impl<W: Write + ?Sized> Write for &mut W {
    fn write_str(&mut self, s: &str) -> Result {
        (**self).write_str(s)
    }
}

/// Already formatted text (`format_args!` formats eagerly).
pub struct Arguments {
    s: String,
}

impl Arguments {
    pub fn new(s: String) -> Arguments {
        Arguments { s }
    }
    pub fn as_str(&self) -> &str {
        self.s.as_str()
    }
}

impl Display for Arguments {
    fn fmt(&self, f: &mut Formatter) -> Result {
        f.write_str(self.s.as_str())
    }
}

impl Debug for Arguments {
    fn fmt(&self, f: &mut Formatter) -> Result {
        f.write_str(self.s.as_str())
    }
}

pub fn format(args: Arguments) -> String {
    args.s
}

/// Where formatted text goes, with the current `{:...}` specification.
pub struct Formatter {
    buf: String,
    fill: char,
    align: u8,
    flags: u32,
    width: Option<usize>,
    precision: Option<usize>,
}

impl Formatter {
    pub fn new() -> Formatter {
        Formatter {
            buf: String::new(),
            fill: ' ',
            align: 0,
            flags: 0,
            width: None,
            precision: None,
        }
    }
    pub fn set_spec(&mut self, fill: char, align: u8, flags: u32, width: Option<usize>, precision: Option<usize>) {
        self.fill = fill;
        self.align = align;
        self.flags = flags;
        self.width = width;
        self.precision = precision;
    }
    pub fn clear_spec(&mut self) {
        self.fill = ' ';
        self.align = 0;
        self.flags = 0;
        self.width = None;
        self.precision = None;
    }
    pub fn into_string(self) -> String {
        self.buf
    }
    /// A formatter with the same specification and an empty buffer.
    fn sub(&self) -> Formatter {
        Formatter {
            buf: String::new(),
            fill: self.fill,
            align: self.align,
            flags: self.flags,
            width: self.width,
            precision: self.precision,
        }
    }
    pub fn write_str(&mut self, s: &str) -> Result {
        self.buf.push_str(s);
        Ok(())
    }
    pub fn write_char(&mut self, c: char) -> Result {
        self.buf.push(c);
        Ok(())
    }
    pub fn write_fmt(&mut self, args: Arguments) -> Result {
        self.buf.push_str(args.as_str());
        Ok(())
    }
    pub fn width(&self) -> Option<usize> {
        self.width
    }
    pub fn precision(&self) -> Option<usize> {
        self.precision
    }
    pub fn fill(&self) -> char {
        self.fill
    }
    pub fn align(&self) -> Option<Alignment> {
        match self.align {
            1 => Some(Alignment::Left),
            2 => Some(Alignment::Center),
            3 => Some(Alignment::Right),
            _ => None,
        }
    }
    pub fn sign_plus(&self) -> bool {
        self.flags & 1 != 0
    }
    pub fn sign_minus(&self) -> bool {
        false
    }
    pub fn alternate(&self) -> bool {
        self.flags & 2 != 0
    }
    pub fn sign_aware_zero_pad(&self) -> bool {
        self.flags & 4 != 0
    }

    fn write_fill(&mut self, n: usize) {
        let mut i = 0;
        while i < n {
            self.buf.push(self.fill);
            i += 1;
        }
    }

    /// Fill before and after for `n` padding characters.
    fn padding(&self, n: usize, default: u8) -> (usize, usize) {
        let a = if self.align == 0 { default } else { self.align };
        match a {
            1 => (0, n),
            2 => (n / 2, (n + 1) / 2),
            _ => (n, 0),
        }
    }

    /// Writes a string with width, alignment and precision (truncation).
    pub fn pad(&mut self, s: &str) -> Result {
        let s = match self.precision {
            Some(p) => match s.char_indices().nth(p) {
                Some((i, _)) => &s[..i],
                None => s,
            },
            None => s,
        };
        match self.width {
            None => self.write_str(s),
            Some(w) => {
                let len = s.chars().count();
                if len >= w {
                    self.write_str(s)
                } else {
                    let (pre, post) = self.padding(w - len, 1);
                    self.write_fill(pre);
                    self.buf.push_str(s);
                    self.write_fill(post);
                    Ok(())
                }
            }
        }
    }

    /// Writes a number: sign, `#` prefix, zero padding.
    pub fn pad_integral(&mut self, is_nonnegative: bool, prefix: &str, digits: &str) -> Result {
        let mut width = digits.len();
        let sign = if !is_nonnegative {
            "-"
        } else if self.sign_plus() {
            "+"
        } else {
            ""
        };
        width += sign.len();
        let prefix = if self.alternate() { prefix } else { "" };
        width += prefix.len();
        match self.width {
            Some(min) if width < min => {
                if self.sign_aware_zero_pad() {
                    self.buf.push_str(sign);
                    self.buf.push_str(prefix);
                    let mut i = width;
                    while i < min {
                        self.buf.push('0');
                        i += 1;
                    }
                    self.buf.push_str(digits);
                } else {
                    let (pre, post) = self.padding(min - width, 3);
                    self.write_fill(pre);
                    self.buf.push_str(sign);
                    self.buf.push_str(prefix);
                    self.buf.push_str(digits);
                    self.write_fill(post);
                }
            }
            _ => {
                self.buf.push_str(sign);
                self.buf.push_str(prefix);
                self.buf.push_str(digits);
            }
        }
        Ok(())
    }

    pub fn debug_struct(&mut self, name: &str) -> DebugStruct<'_> {
        self.buf.push_str(name);
        DebugStruct { fmt: self, has_fields: false }
    }
    pub fn debug_tuple(&mut self, name: &str) -> DebugTuple<'_> {
        self.buf.push_str(name);
        DebugTuple {
            fmt: self,
            fields: 0,
            empty_name: name.is_empty(),
        }
    }
    pub fn debug_list(&mut self) -> DebugList<'_> {
        self.buf.push_str("[");
        DebugList {
            inner: DebugInner { fmt: self, has_fields: false },
        }
    }
    pub fn debug_set(&mut self) -> DebugSet<'_> {
        self.buf.push_str("{");
        DebugSet {
            inner: DebugInner { fmt: self, has_fields: false },
        }
    }
    pub fn debug_map(&mut self) -> DebugMap<'_> {
        self.buf.push_str("{");
        DebugMap { fmt: self, has_fields: false }
    }

    /// Formats `value` with this specification into a string, indented
    /// for pretty printing.
    fn indented(&self, value: &dyn Debug) -> String {
        let mut sub = self.sub();
        let _ = Debug::fmt(value, &mut sub);
        sub.buf.replace("\n", "\n    ")
    }
}

impl Write for Formatter {
    fn write_str(&mut self, s: &str) -> Result {
        self.buf.push_str(s);
        Ok(())
    }
}

pub struct DebugStruct<'a> {
    fmt: &'a mut Formatter,
    has_fields: bool,
}

impl<'a> DebugStruct<'a> {
    pub fn field(&mut self, name: &str, value: &dyn Debug) -> &mut DebugStruct<'a> {
        if self.fmt.alternate() {
            if !self.has_fields {
                self.fmt.buf.push_str(" {\n");
            }
            let s = self.fmt.indented(value);
            self.fmt.buf.push_str("    ");
            self.fmt.buf.push_str(name);
            self.fmt.buf.push_str(": ");
            self.fmt.buf.push_str(&s);
            self.fmt.buf.push_str(",\n");
        } else {
            self.fmt.buf.push_str(if self.has_fields { ", " } else { " { " });
            self.fmt.buf.push_str(name);
            self.fmt.buf.push_str(": ");
            let _ = Debug::fmt(value, self.fmt);
        }
        self.has_fields = true;
        self
    }
    pub fn finish_non_exhaustive(&mut self) -> Result {
        if self.fmt.alternate() {
            if !self.has_fields {
                self.fmt.buf.push_str(" {\n");
            }
            self.fmt.buf.push_str("    ..\n}");
        } else if self.has_fields {
            self.fmt.buf.push_str(", .. }");
        } else {
            self.fmt.buf.push_str(" { .. }");
        }
        Ok(())
    }
    pub fn finish(&mut self) -> Result {
        if self.has_fields {
            self.fmt.buf.push_str(if self.fmt.alternate() { "}" } else { " }" });
        }
        Ok(())
    }
}

pub struct DebugTuple<'a> {
    fmt: &'a mut Formatter,
    fields: usize,
    empty_name: bool,
}

impl<'a> DebugTuple<'a> {
    pub fn field(&mut self, value: &dyn Debug) -> &mut DebugTuple<'a> {
        if self.fmt.alternate() {
            if self.fields == 0 {
                self.fmt.buf.push_str("(\n");
            }
            let s = self.fmt.indented(value);
            self.fmt.buf.push_str("    ");
            self.fmt.buf.push_str(&s);
            self.fmt.buf.push_str(",\n");
        } else {
            self.fmt.buf.push_str(if self.fields == 0 { "(" } else { ", " });
            let _ = Debug::fmt(value, self.fmt);
        }
        self.fields += 1;
        self
    }
    pub fn finish(&mut self) -> Result {
        if self.fields > 0 {
            if self.fields == 1 && self.empty_name && !self.fmt.alternate() {
                self.fmt.buf.push_str(",");
            }
            self.fmt.buf.push_str(")");
        }
        Ok(())
    }
}

struct DebugInner<'a> {
    fmt: &'a mut Formatter,
    has_fields: bool,
}

impl<'a> DebugInner<'a> {
    fn entry(&mut self, value: &dyn Debug) {
        if self.fmt.alternate() {
            if !self.has_fields {
                self.fmt.buf.push_str("\n");
            }
            let s = self.fmt.indented(value);
            self.fmt.buf.push_str("    ");
            self.fmt.buf.push_str(&s);
            self.fmt.buf.push_str(",\n");
        } else {
            if self.has_fields {
                self.fmt.buf.push_str(", ");
            }
            let _ = Debug::fmt(value, self.fmt);
        }
        self.has_fields = true;
    }
}

pub struct DebugList<'a> {
    inner: DebugInner<'a>,
}

impl<'a> DebugList<'a> {
    pub fn entry(&mut self, value: &dyn Debug) -> &mut DebugList<'a> {
        self.inner.entry(value);
        self
    }
    pub fn entries<D: Debug, I: IntoIterator<Item = D>>(&mut self, entries: I) -> &mut DebugList<'a> {
        for e in entries {
            self.inner.entry(&e);
        }
        self
    }
    pub fn finish(&mut self) -> Result {
        self.inner.fmt.buf.push_str("]");
        Ok(())
    }
}

pub struct DebugSet<'a> {
    inner: DebugInner<'a>,
}

impl<'a> DebugSet<'a> {
    pub fn entry(&mut self, value: &dyn Debug) -> &mut DebugSet<'a> {
        self.inner.entry(value);
        self
    }
    pub fn entries<D: Debug, I: IntoIterator<Item = D>>(&mut self, entries: I) -> &mut DebugSet<'a> {
        for e in entries {
            self.inner.entry(&e);
        }
        self
    }
    pub fn finish(&mut self) -> Result {
        self.inner.fmt.buf.push_str("}");
        Ok(())
    }
}

pub struct DebugMap<'a> {
    fmt: &'a mut Formatter,
    has_fields: bool,
}

impl<'a> DebugMap<'a> {
    pub fn entry(&mut self, key: &dyn Debug, value: &dyn Debug) -> &mut DebugMap<'a> {
        if self.fmt.alternate() {
            if !self.has_fields {
                self.fmt.buf.push_str("\n");
            }
            let k = self.fmt.indented(key);
            let v = self.fmt.indented(value);
            self.fmt.buf.push_str("    ");
            self.fmt.buf.push_str(&k);
            self.fmt.buf.push_str(": ");
            self.fmt.buf.push_str(&v);
            self.fmt.buf.push_str(",\n");
        } else {
            if self.has_fields {
                self.fmt.buf.push_str(", ");
            }
            let _ = Debug::fmt(key, self.fmt);
            self.fmt.buf.push_str(": ");
            let _ = Debug::fmt(value, self.fmt);
        }
        self.has_fields = true;
        self
    }
    pub fn entries<K: Debug, V: Debug, I: IntoIterator<Item = (K, V)>>(&mut self, entries: I) -> &mut DebugMap<'a> {
        for (k, v) in entries {
            self.entry(&k, &v);
        }
        self
    }
    pub fn finish(&mut self) -> Result {
        self.fmt.buf.push_str("}");
        Ok(())
    }
}

// ---- implementations for builtin types ----

impl<T: Display + ?Sized> Display for &T {
    fn fmt(&self, f: &mut Formatter) -> Result {
        Display::fmt(&**self, f)
    }
}
impl<T: Display + ?Sized> Display for &mut T {
    fn fmt(&self, f: &mut Formatter) -> Result {
        Display::fmt(&**self, f)
    }
}
impl<T: Debug + ?Sized> Debug for &T {
    fn fmt(&self, f: &mut Formatter) -> Result {
        Debug::fmt(&**self, f)
    }
}
impl<T: Debug + ?Sized> Debug for &mut T {
    fn fmt(&self, f: &mut Formatter) -> Result {
        Debug::fmt(&**self, f)
    }
}
impl<T: LowerHex + ?Sized> LowerHex for &T {
    fn fmt(&self, f: &mut Formatter) -> Result {
        LowerHex::fmt(&**self, f)
    }
}
impl<T: UpperHex + ?Sized> UpperHex for &T {
    fn fmt(&self, f: &mut Formatter) -> Result {
        UpperHex::fmt(&**self, f)
    }
}
impl<T: Binary + ?Sized> Binary for &T {
    fn fmt(&self, f: &mut Formatter) -> Result {
        Binary::fmt(&**self, f)
    }
}
impl<T: LowerExp + ?Sized> LowerExp for &T {
    fn fmt(&self, f: &mut Formatter) -> Result {
        LowerExp::fmt(&**self, f)
    }
}

impl Display for str {
    fn fmt(&self, f: &mut Formatter) -> Result {
        f.pad(self)
    }
}

fn escape_char(f: &mut Formatter, c: char, quote: char) {
    match c {
        '\t' => f.buf.push_str("\\t"),
        '\r' => f.buf.push_str("\\r"),
        '\n' => f.buf.push_str("\\n"),
        '\\' => f.buf.push_str("\\\\"),
        '\0' => f.buf.push_str("\\0"),
        _ if c == quote => {
            f.buf.push('\\');
            f.buf.push(c);
        }
        _ => {
            let v = c as u32;
            if v < 0x20 || v == 0x7f || (v >= 0x80 && v < 0xa0) {
                f.buf.push_str("\\u{");
                let digits = hex_digits(v as u64, false);
                f.buf.push_str(&digits);
                f.buf.push('}');
            } else {
                f.buf.push(c);
            }
        }
    }
}

impl Debug for str {
    fn fmt(&self, f: &mut Formatter) -> Result {
        f.buf.push('"');
        for c in self.chars() {
            escape_char(f, c, '"');
        }
        f.buf.push('"');
        Ok(())
    }
}

impl Display for char {
    fn fmt(&self, f: &mut Formatter) -> Result {
        let mut b = [0u8; 4];
        let s = self.encode_utf8(&mut b);
        f.pad(s)
    }
}

impl Debug for char {
    fn fmt(&self, f: &mut Formatter) -> Result {
        f.buf.push('\'');
        escape_char(f, *self, '\'');
        f.buf.push('\'');
        Ok(())
    }
}

impl Display for bool {
    fn fmt(&self, f: &mut Formatter) -> Result {
        f.pad(if *self { "true" } else { "false" })
    }
}

impl Debug for bool {
    fn fmt(&self, f: &mut Formatter) -> Result {
        f.pad(if *self { "true" } else { "false" })
    }
}

impl Debug for () {
    fn fmt(&self, f: &mut Formatter) -> Result {
        f.pad("()")
    }
}

impl Debug for crate::convert::Infallible {
    fn fmt(&self, _f: &mut Formatter) -> Result {
        Ok(())
    }
}

/// Digits of an unsigned value in a radix.
fn radix_digits(mut v: u64, radix: u64, upper: bool) -> String {
    let mut buf = [0u8; 64];
    let mut i = 64;
    loop {
        let d = (v % radix) as u8;
        i -= 1;
        buf[i] = if d < 10 {
            b'0' + d
        } else if upper {
            b'A' + d - 10
        } else {
            b'a' + d - 10
        };
        v /= radix;
        if v == 0 {
            break;
        }
    }
    let mut s = String::new();
    while i < 64 {
        s.push(buf[i] as char);
        i += 1;
    }
    s
}

pub fn hex_digits(v: u64, upper: bool) -> String {
    radix_digits(v, 16, upper)
}

fn decimal(f: &mut Formatter, nonneg: bool, abs: u64) -> Result {
    let mut buf = [0u8; 24];
    let mut i = 24;
    let mut v = abs;
    loop {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let s = unsafe { crate::str::from_utf8_unchecked(&buf[i..]) };
    f.pad_integral(nonneg, "", s)
}

macro_rules! int_fmt {
    ($($t:ty, $u:ty, $signed:expr;)*) => {$(
        impl Display for $t {
            fn fmt(&self, f: &mut Formatter) -> Result {
                let v = *self;
                if $signed && (v as i64) < 0 {
                    decimal(f, false, (!(v as i64 as u64)).wrapping_add(1))
                } else {
                    decimal(f, true, v as u64)
                }
            }
        }
        impl Debug for $t {
            fn fmt(&self, f: &mut Formatter) -> Result {
                Display::fmt(self, f)
            }
        }
        impl LowerHex for $t {
            fn fmt(&self, f: &mut Formatter) -> Result {
                let s = radix_digits(*self as $u as u64, 16, false);
                f.pad_integral(true, "0x", &s)
            }
        }
        impl UpperHex for $t {
            fn fmt(&self, f: &mut Formatter) -> Result {
                let s = radix_digits(*self as $u as u64, 16, true);
                f.pad_integral(true, "0x", &s)
            }
        }
        impl Octal for $t {
            fn fmt(&self, f: &mut Formatter) -> Result {
                let s = radix_digits(*self as $u as u64, 8, false);
                f.pad_integral(true, "0o", &s)
            }
        }
        impl Binary for $t {
            fn fmt(&self, f: &mut Formatter) -> Result {
                let s = radix_digits(*self as $u as u64, 2, false);
                f.pad_integral(true, "0b", &s)
            }
        }
        impl LowerExp for $t {
            fn fmt(&self, f: &mut Formatter) -> Result {
                LowerExp::fmt(&(*self as f64), f)
            }
        }
    )*};
}
int_fmt! {
    i8, u8, true;
    i16, u16, true;
    i32, u32, true;
    i64, u64, true;
    isize, usize, true;
    u8, u8, false;
    u16, u16, false;
    u32, u32, false;
    u64, u64, false;
    usize, usize, false;
}

fn float_fmt(v: f64, is_f32: bool, kind: i32, f: &mut Formatter) -> Result {
    let mut buf = [0u8; 1200];
    let prec = match f.precision {
        Some(p) => p as i64,
        None => -1,
    };
    let n = unsafe { crate::sys::hrc_fmt_float(v, is_f32 as i32, kind, prec, &mut buf[0] as *mut u8) };
    let s = unsafe { crate::str::from_utf8_unchecked(&buf[..n]) };
    let neg = v.is_sign_negative() && !v.is_nan();
    let saved = f.flags;
    if v.is_nan() {
        // NaN never has a sign and is not zero-padded.
        f.flags = f.flags & !5;
    } else if v.is_infinite() {
        f.flags = f.flags & !4;
    }
    let r = f.pad_integral(!neg, "", s);
    f.flags = saved;
    r
}

impl Display for f64 {
    fn fmt(&self, f: &mut Formatter) -> Result {
        float_fmt(*self, false, 0, f)
    }
}
impl Debug for f64 {
    fn fmt(&self, f: &mut Formatter) -> Result {
        float_fmt(*self, false, 1, f)
    }
}
impl LowerExp for f64 {
    fn fmt(&self, f: &mut Formatter) -> Result {
        float_fmt(*self, false, 2, f)
    }
}
impl UpperExp for f64 {
    fn fmt(&self, f: &mut Formatter) -> Result {
        float_fmt(*self, false, 3, f)
    }
}
impl Display for f32 {
    fn fmt(&self, f: &mut Formatter) -> Result {
        float_fmt(*self as f64, true, 0, f)
    }
}
impl Debug for f32 {
    fn fmt(&self, f: &mut Formatter) -> Result {
        float_fmt(*self as f64, true, 1, f)
    }
}
impl LowerExp for f32 {
    fn fmt(&self, f: &mut Formatter) -> Result {
        float_fmt(*self as f64, true, 2, f)
    }
}
impl UpperExp for f32 {
    fn fmt(&self, f: &mut Formatter) -> Result {
        float_fmt(*self as f64, true, 3, f)
    }
}

impl<T: Debug> Debug for [T] {
    fn fmt(&self, f: &mut Formatter) -> Result {
        let mut l = f.debug_list();
        for x in self.iter() {
            l.entry(x);
        }
        l.finish()
    }
}

impl<T: Debug, const N: usize> Debug for [T; N] {
    fn fmt(&self, f: &mut Formatter) -> Result {
        Debug::fmt(&self[..], f)
    }
}

impl<T> Debug for *const T {
    fn fmt(&self, f: &mut Formatter) -> Result {
        let s = hex_digits(*self as usize as u64, false);
        f.write_str("0x")?;
        f.write_str(&s)
    }
}

impl<T> Debug for *mut T {
    fn fmt(&self, f: &mut Formatter) -> Result {
        let s = hex_digits(*self as usize as u64, false);
        f.write_str("0x")?;
        f.write_str(&s)
    }
}

macro_rules! tuple_debug {
    ($(($($T:ident $i:tt),+))*) => {$(
        impl<$($T: Debug),+> Debug for ($($T,)+) {
            fn fmt(&self, f: &mut Formatter) -> Result {
                let mut t = f.debug_tuple("");
                $(t.field(&self.$i);)+
                t.finish()
            }
        }
    )*};
}
tuple_debug! {
    (A 0)
    (A 0, B 1)
    (A 0, B 1, C 2)
    (A 0, B 1, C 2, D 3)
    (A 0, B 1, C 2, D 3, E 4)
    (A 0, B 1, C 2, D 3, E 4, F 5)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7)
}
