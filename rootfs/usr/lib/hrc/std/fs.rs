//! Files.

use crate::io::{self, Read, Write};
use crate::string::String;
use crate::vec::Vec;

mod c {
    extern "C" {
        pub fn open(path: *const u8, flags: i32, mode: i32) -> i32;
        pub fn close(fd: i32) -> i32;
        pub fn unlink(path: *const u8) -> i32;
        pub fn mkdir(path: *const u8, mode: i32) -> i32;
        pub fn rmdir(path: *const u8) -> i32;
        pub fn rename(a: *const u8, b: *const u8) -> i32;
        pub fn hrc_errno() -> i32;
    }
}

fn errno() -> i32 {
    unsafe { c::hrc_errno() }
}

fn c_path<P: AsRef<str>>(p: P) -> String {
    let mut s = String::from(<P as AsRef<str>>::as_ref(&p));
    s.push('\0');
    s
}

fn os_err() -> io::Error {
    io::Error::from_raw_os_error(errno())
}

pub struct File {
    fd: i32,
}

const O_RDONLY: i32 = 0;
const O_WRONLY: i32 = 1;
const O_RDWR: i32 = 2;
const O_CREAT: i32 = 64;
const O_TRUNC: i32 = 512;
const O_APPEND: i32 = 1024;

impl File {
    fn open_flags<P: AsRef<str>>(path: P, flags: i32) -> io::Result<File> {
        let p = c_path(path);
        let fd = unsafe { c::open(p.as_ptr(), flags, 0o644) };
        if fd < 0 {
            Err(os_err())
        } else {
            Ok(File { fd })
        }
    }
    pub fn open<P: AsRef<str>>(path: P) -> io::Result<File> {
        File::open_flags(path, O_RDONLY)
    }
    pub fn create<P: AsRef<str>>(path: P) -> io::Result<File> {
        File::open_flags(path, O_WRONLY | O_CREAT | O_TRUNC)
    }
    pub fn options() -> OpenOptions {
        OpenOptions::new()
    }
    pub fn sync_all(&self) -> io::Result<()> {
        Ok(())
    }
    pub fn close(self) {
        unsafe {
            c::close(self.fd);
        }
    }
}

impl Read for File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = unsafe { crate::sys::read(self.fd, buf.as_mut_ptr(), buf.len()) };
        if n < 0 {
            Err(os_err())
        } else {
            Ok(n as usize)
        }
    }
}

impl Write for File {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = unsafe { crate::sys::write(self.fd, buf.as_ptr(), buf.len()) };
        if n < 0 {
            Err(os_err())
        } else {
            Ok(n as usize)
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct OpenOptions {
    read: bool,
    write: bool,
    append: bool,
    truncate: bool,
    create: bool,
}

impl OpenOptions {
    pub fn new() -> OpenOptions {
        OpenOptions {
            read: false,
            write: false,
            append: false,
            truncate: false,
            create: false,
        }
    }
    pub fn read(&mut self, v: bool) -> &mut OpenOptions {
        self.read = v;
        self
    }
    pub fn write(&mut self, v: bool) -> &mut OpenOptions {
        self.write = v;
        self
    }
    pub fn append(&mut self, v: bool) -> &mut OpenOptions {
        self.append = v;
        self
    }
    pub fn truncate(&mut self, v: bool) -> &mut OpenOptions {
        self.truncate = v;
        self
    }
    pub fn create(&mut self, v: bool) -> &mut OpenOptions {
        self.create = v;
        self
    }
    pub fn open<P: AsRef<str>>(&self, path: P) -> io::Result<File> {
        let mut flags = if self.write || self.append {
            if self.read {
                O_RDWR
            } else {
                O_WRONLY
            }
        } else {
            O_RDONLY
        };
        if self.append {
            flags |= O_APPEND;
        }
        if self.truncate {
            flags |= O_TRUNC;
        }
        if self.create {
            flags |= O_CREAT;
        }
        File::open_flags(path, flags)
    }
}

pub fn read<P: AsRef<str>>(path: P) -> io::Result<Vec<u8>> {
    let mut f = File::open(path)?;
    let mut v = Vec::new();
    f.read_to_end(&mut v)?;
    f.close();
    Ok(v)
}

pub fn read_to_string<P: AsRef<str>>(path: P) -> io::Result<String> {
    let mut f = File::open(path)?;
    let mut s = String::new();
    f.read_to_string(&mut s)?;
    f.close();
    Ok(s)
}

pub fn write<P: AsRef<str>, C: AsRef<[u8]>>(path: P, contents: C) -> io::Result<()> {
    let mut f = File::create(path)?;
    f.write_all(<C as AsRef<[u8]>>::as_ref(&contents))?;
    f.close();
    Ok(())
}

pub fn remove_file<P: AsRef<str>>(path: P) -> io::Result<()> {
    let p = c_path(path);
    if unsafe { c::unlink(p.as_ptr()) } < 0 {
        Err(os_err())
    } else {
        Ok(())
    }
}

pub fn create_dir<P: AsRef<str>>(path: P) -> io::Result<()> {
    let p = c_path(path);
    if unsafe { c::mkdir(p.as_ptr(), 0o755) } < 0 {
        Err(os_err())
    } else {
        Ok(())
    }
}

pub fn remove_dir<P: AsRef<str>>(path: P) -> io::Result<()> {
    let p = c_path(path);
    if unsafe { c::rmdir(p.as_ptr()) } < 0 {
        Err(os_err())
    } else {
        Ok(())
    }
}

pub fn rename<P: AsRef<str>, Q: AsRef<str>>(from: P, to: Q) -> io::Result<()> {
    let a = c_path(from);
    let b = c_path(to);
    if unsafe { c::rename(a.as_ptr(), b.as_ptr()) } < 0 {
        Err(os_err())
    } else {
        Ok(())
    }
}
