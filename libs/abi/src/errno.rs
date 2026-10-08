//! Error numbers (Linux values).

use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Errno {
    EPERM = 1,
    ENOENT = 2,
    ESRCH = 3,
    EINTR = 4,
    EIO = 5,
    ENXIO = 6,
    E2BIG = 7,
    ENOEXEC = 8,
    EBADF = 9,
    ECHILD = 10,
    EAGAIN = 11,
    ENOMEM = 12,
    EACCES = 13,
    EFAULT = 14,
    EBUSY = 16,
    EEXIST = 17,
    EXDEV = 18,
    ENODEV = 19,
    ENOTDIR = 20,
    EISDIR = 21,
    EINVAL = 22,
    ENFILE = 23,
    EMFILE = 24,
    ENOTTY = 25,
    EFBIG = 27,
    ENOSPC = 28,
    ESPIPE = 29,
    EROFS = 30,
    EMLINK = 31,
    EPIPE = 32,
    ERANGE = 34,
    ENAMETOOLONG = 36,
    ENOSYS = 38,
    ENOTEMPTY = 39,
    ELOOP = 40,
    ETIMEDOUT = 110,
    ENOTSOCK = 88,
    EDESTADDRREQ = 89,
    EMSGSIZE = 90,
    EPROTOTYPE = 91,
    ENOPROTOOPT = 92,
    EPROTONOSUPPORT = 93,
    EOPNOTSUPP = 95,
    EAFNOSUPPORT = 97,
    EADDRINUSE = 98,
    EADDRNOTAVAIL = 99,
    ENETUNREACH = 101,
    ECONNRESET = 104,
    EISCONN = 106,
    ENOTCONN = 107,
    ECONNREFUSED = 111,
    EHOSTUNREACH = 113,
    EALREADY = 114,
    EINPROGRESS = 115,
}

impl Errno {
    pub const fn code(self) -> i32 {
        self as i32
    }

    pub fn from_code(code: i32) -> Option<Errno> {
        use Errno::*;
        const ALL: [Errno; 54] = [
            EPERM,
            ENOENT,
            ESRCH,
            EINTR,
            EIO,
            ENXIO,
            E2BIG,
            ENOEXEC,
            EBADF,
            ECHILD,
            EAGAIN,
            ENOMEM,
            EACCES,
            EFAULT,
            EBUSY,
            EEXIST,
            EXDEV,
            ENODEV,
            ENOTDIR,
            EISDIR,
            EINVAL,
            ENFILE,
            EMFILE,
            ENOTTY,
            EFBIG,
            ENOSPC,
            ESPIPE,
            EROFS,
            EMLINK,
            EPIPE,
            ERANGE,
            ENAMETOOLONG,
            ENOSYS,
            ENOTEMPTY,
            ELOOP,
            ETIMEDOUT,
            ENOTSOCK,
            EDESTADDRREQ,
            EMSGSIZE,
            EPROTOTYPE,
            ENOPROTOOPT,
            EPROTONOSUPPORT,
            EOPNOTSUPP,
            EAFNOSUPPORT,
            EADDRINUSE,
            EADDRNOTAVAIL,
            ENETUNREACH,
            ECONNRESET,
            EISCONN,
            ENOTCONN,
            ECONNREFUSED,
            EHOSTUNREACH,
            EALREADY,
            EINPROGRESS,
        ];
        ALL.iter().copied().find(|e| e.code() == code)
    }

    pub const fn message(self) -> &'static str {
        use Errno::*;
        match self {
            EPERM => "Operation not permitted",
            ENOENT => "No such file or directory",
            ESRCH => "No such process",
            EINTR => "Interrupted system call",
            EIO => "Input/output error",
            ENXIO => "No such device or address",
            E2BIG => "Argument list too long",
            ENOEXEC => "Exec format error",
            EBADF => "Bad file descriptor",
            ECHILD => "No child processes",
            EAGAIN => "Resource temporarily unavailable",
            ENOMEM => "Cannot allocate memory",
            EACCES => "Permission denied",
            EFAULT => "Bad address",
            EBUSY => "Device or resource busy",
            EEXIST => "File exists",
            EXDEV => "Invalid cross-device link",
            ENODEV => "No such device",
            ENOTDIR => "Not a directory",
            EISDIR => "Is a directory",
            EINVAL => "Invalid argument",
            ENFILE => "Too many open files in system",
            EMFILE => "Too many open files",
            ENOTTY => "Inappropriate ioctl for device",
            EFBIG => "File too large",
            ENOSPC => "No space left on device",
            ESPIPE => "Illegal seek",
            EROFS => "Read-only file system",
            EMLINK => "Too many links",
            EPIPE => "Broken pipe",
            ERANGE => "Numerical result out of range",
            ENAMETOOLONG => "File name too long",
            ENOSYS => "Function not implemented",
            ENOTEMPTY => "Directory not empty",
            ELOOP => "Too many levels of symbolic links",
            ETIMEDOUT => "Connection timed out",
            ENOTSOCK => "Socket operation on non-socket",
            EDESTADDRREQ => "Destination address required",
            EMSGSIZE => "Message too long",
            EPROTOTYPE => "Protocol wrong type for socket",
            ENOPROTOOPT => "Protocol not available",
            EPROTONOSUPPORT => "Protocol not supported",
            EOPNOTSUPP => "Operation not supported",
            EAFNOSUPPORT => "Address family not supported by protocol",
            EADDRINUSE => "Address already in use",
            EADDRNOTAVAIL => "Cannot assign requested address",
            ENETUNREACH => "Network is unreachable",
            ECONNRESET => "Connection reset by peer",
            EISCONN => "Transport endpoint is already connected",
            ENOTCONN => "Transport endpoint is not connected",
            ECONNREFUSED => "Connection refused",
            EHOSTUNREACH => "No route to host",
            EALREADY => "Operation already in progress",
            EINPROGRESS => "Operation now in progress",
        }
    }
}

impl fmt::Display for Errno {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.message())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        for code in 1..=39 {
            if let Some(e) = Errno::from_code(code) {
                assert_eq!(e.code(), code);
            }
        }
        assert_eq!(Errno::from_code(2), Some(Errno::ENOENT));
        assert_eq!(Errno::from_code(0), None);
    }
}
