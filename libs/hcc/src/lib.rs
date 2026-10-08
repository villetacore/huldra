//! hcc: a small C compiler for Huldra.
//!
//! It compiles a whole program at once (the user's sources plus the C
//! library in source form) straight to a static x86-64 ELF executable:
//! no assembler, object files or linker. The pipeline is
//! [`lex`] → [`pp`] → [`parse`] (typed AST, [`ast`]/[`ty`]) → [`gen`]
//! (machine code via [`asm`]) → [`elf`].
//!
//! Generated code uses Linux system calls only, so the executables run on
//! both Huldra and Linux.

#![no_std]

extern crate alloc;

pub mod asm;
pub mod ast;
pub mod elf;
pub mod gen;
pub mod lex;
pub mod parse;
pub mod pp;
pub mod ty;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

#[derive(Debug, Clone)]
pub struct Error {
    pub file: String,
    pub line: u32,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match (self.file.is_empty(), self.line) {
            (true, _) => write!(f, "error: {}", self.message),
            (false, 0) => write!(f, "{}: error: {}", self.file, self.message),
            _ => write!(f, "{}:{}: error: {}", self.file, self.line, self.message),
        }
    }
}

/// Where the compiler reads sources and headers from.
pub trait FileSource {
    fn read(&self, path: &str) -> Option<String>;
}

pub struct Options {
    pub include_dirs: Vec<String>,
    pub defines: Vec<(String, String)>,
    /// C sources compiled together with the program (the C library).
    pub runtime: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            include_dirs: alloc::vec![String::from("/usr/include")],
            defines: Vec::new(),
            runtime: alloc::vec![String::from("/usr/lib/hcc/libc.c")],
        }
    }
}

fn preprocessor<'a>(fs: &'a dyn FileSource, options: &Options) -> pp::Preprocessor<'a> {
    let mut pp = pp::Preprocessor::new(fs, options.include_dirs.clone());
    for (k, v) in &options.defines {
        pp.define_str(k, v);
    }
    pp
}

/// Runs only the preprocessor (`cc -E`).
pub fn preprocess(fs: &dyn FileSource, file: &str, options: &Options) -> Result<String, Error> {
    let mut pp = preprocessor(fs, options);
    let toks = pp.run_file(file)?;
    let mut out = String::new();
    let mut line = 0;
    let mut file_idx = u16::MAX;
    for t in &toks {
        if matches!(t.tok, lex::Tok::Eof) {
            break;
        }
        if t.file != file_idx || t.line != line {
            if !out.is_empty() {
                out.push('\n');
            }
            file_idx = t.file;
            line = t.line;
        } else if t.space {
            out.push(' ');
        }
        out.push_str(&t.spelling());
    }
    out.push('\n');
    Ok(out)
}

/// Compiles `files` (plus the runtime sources) into an ELF executable.
pub fn compile(fs: &dyn FileSource, files: &[&str], options: &Options) -> Result<Vec<u8>, Error> {
    let mut program = ast::Program::default();
    let all = files
        .iter()
        .map(|f| String::from(*f))
        .chain(options.runtime.iter().cloned());
    for (unit, file) in all.enumerate() {
        let mut pp = preprocessor(fs, options);
        let toks = pp.run_file(&file)?;
        parse::parse(toks, pp.files, unit, &mut program)?;
    }
    let image = gen::generate(&program)?;
    Ok(elf::write(&image))
}
