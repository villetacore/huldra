//! Process control.

pub fn exit(code: i32) -> ! {
    unsafe { crate::sys::hrc_exit(code) }
}

pub fn abort() -> ! {
    unsafe { crate::sys::hrc_exit(134) }
}

extern "C" {
    fn getpid() -> i32;
}

pub fn id() -> u32 {
    unsafe { getpid() as u32 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitCode(i32);

impl ExitCode {
    pub const SUCCESS: ExitCode = ExitCode(0);
    pub const FAILURE: ExitCode = ExitCode(1);
}

impl From<u8> for ExitCode {
    fn from(c: u8) -> ExitCode {
        ExitCode(c as i32)
    }
}
