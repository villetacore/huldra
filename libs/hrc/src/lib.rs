//! hrc: a compiler for a subset of Rust, for Huldra.
//!
//! The pipeline is [`lex`] → [`parse`] (syntax tree, [`ast`]; `macro_rules!`
//! in [`macros`]) → [`resolve`] (modules, items, `use`) → [`check`] (type
//! inference, trait resolution and monomorphization into a typed IR,
//! [`tir`]) → [`cgen`] (C source) → hcc, which compiles the C together with
//! the runtime (`/usr/lib/hrc/rt.c`) and the C library into a static
//! x86-64 ELF executable.
//!
//! The standard library is Rust source too (`/usr/lib/hrc/std.rs`),
//! compiled with every program.

#![no_std]

extern crate alloc;

pub mod ast;
pub mod cgen;
pub mod check;
pub mod lex;
pub mod macros;
pub mod parse;
pub mod resolve;
pub mod tir;
pub mod ty;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

pub use huldra_hcc::FileSource;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Span {
    pub file: u16,
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone)]
pub struct Error {
    pub span: Span,
    pub message: String,
    /// Extra lines (`note: ...`, `help: ...`).
    pub notes: Vec<String>,
    /// Set by [`compile`]: the file name for `span.file`.
    pub file: String,
}

impl Error {
    pub fn new(span: Span, message: String) -> Error {
        Error {
            span,
            message,
            notes: Vec::new(),
            file: String::new(),
        }
    }

    pub fn note(mut self, n: String) -> Error {
        self.notes.push(n);
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "error: {}", self.message)?;
        if !self.file.is_empty() {
            write!(f, "\n --> {}:{}:{}", self.file, self.span.line, self.span.col)?;
        }
        for n in &self.notes {
            write!(f, "\n  = {}", n)?;
        }
        Ok(())
    }
}

pub type Result<T> = core::result::Result<T, Error>;

pub fn err<T>(span: Span, message: String) -> Result<T> {
    Err(Error::new(span, message))
}

pub struct Options {
    /// The standard library source.
    pub std: String,
    /// The C runtime the generated code links against.
    pub runtime: String,
    /// The C library (hcc's `libc.c`) and its headers.
    pub hcc: huldra_hcc::Options,
    /// Integer overflow checks (on unless `-O`).
    pub overflow_checks: bool,
}

impl Default for Options {
    fn default() -> Self {
        let mut hcc = huldra_hcc::Options::default();
        hcc.runtime.insert(0, String::from("/usr/lib/hrc/rt.c"));
        Options {
            std: String::from("/usr/lib/hrc/std/lib.rs"),
            runtime: String::from("/usr/lib/hrc/rt.c"),
            hcc,
            overflow_checks: true,
        }
    }
}

/// The source files of one compilation, by `Span::file`.
pub struct Sources {
    pub names: Vec<String>,
    pub texts: Vec<String>,
}

impl Sources {
    pub fn add(&mut self, name: String, text: String) -> u16 {
        self.names.push(name);
        self.texts.push(text);
        (self.names.len() - 1) as u16
    }
}

fn locate(mut e: Error, sources: &Sources) -> Error {
    if let Some(n) = sources.names.get(e.span.file as usize) {
        e.file = n.clone();
    }
    e
}

/// Translates a program (its crate root `file`) to C.
pub fn to_c(fs: &dyn FileSource, file: &str, options: &Options) -> Result<String> {
    let mut sources = Sources {
        names: Vec::new(),
        texts: Vec::new(),
    };
    let run = |sources: &mut Sources| -> Result<String> {
        let std_text = fs
            .read(&options.std)
            .ok_or_else(|| Error::new(Span::default(), format!("cannot read the standard library {}", options.std)))?;
        let std_file = sources.add(options.std.clone(), std_text);
        let std_items = parse::parse_crate(fs, sources, std_file)?;
        let text = fs
            .read(file)
            .ok_or_else(|| Error::new(Span::default(), format!("couldn't read `{}`", file)))?;
        let main_file = sources.add(String::from(file), text);
        let items = parse::parse_crate(fs, sources, main_file)?;
        let db = resolve::build(std_items, items)?;
        let program = check::check_program(&db, sources, main_file)?;
        Ok(cgen::generate(&db, &program, sources, options))
    };
    run(&mut sources).map_err(|e| locate(e, &sources))
}

/// Serves the generated C file next to the real file system.
struct WithGenerated<'a> {
    fs: &'a dyn FileSource,
    name: &'a str,
    text: &'a str,
}

impl FileSource for WithGenerated<'_> {
    fn read(&self, path: &str) -> Option<String> {
        if path == self.name {
            Some(String::from(self.text))
        } else {
            self.fs.read(path)
        }
    }
}

/// Compiles a program into an ELF executable.
pub fn compile(fs: &dyn FileSource, file: &str, options: &Options) -> Result<Vec<u8>> {
    let c = to_c(fs, file, options)?;
    let name = "/hrc-generated.c";
    let overlay = WithGenerated { fs, name, text: &c };
    huldra_hcc::compile(&overlay, &[name], &options.hcc).map_err(|e| {
        Error::new(
            Span::default(),
            format!("internal compiler error: the generated C does not compile: {}", e),
        )
    })
}

