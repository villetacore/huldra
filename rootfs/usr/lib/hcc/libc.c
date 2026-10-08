/* The hcc C library. Compiled together with every program (unused
   functions are dropped by the compiler), so there are no object files
   or archives to link. Talks to the kernel with Linux system calls. */

#include <stddef.h>
#include <stdarg.h>
#include <stdint.h>
#include <limits.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#include <math.h>
#include <time.h>
#include <fcntl.h>
#include <unistd.h>
#include <signal.h>
#include <dirent.h>
#include <termios.h>
#include <locale.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <sys/time.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/utsname.h>

#undef sqrt

int errno;
char **environ;

/* ---- system calls ------------------------------------------------------ */

static long __ret(long r) {
    if (r < 0 && r > -4096) {
        errno = (int)-r;
        return -1;
    }
    return r;
}

long syscall(long nr, ...) {
    va_list ap;
    va_start(ap, nr);
    long a = va_arg(ap, long), b = va_arg(ap, long), c = va_arg(ap, long);
    long d = va_arg(ap, long), e = va_arg(ap, long), f = va_arg(ap, long);
    va_end(ap);
    return __ret(__syscall(nr, a, b, c, d, e, f));
}

ssize_t read(int fd, void *buf, size_t n) { return __ret(__syscall(0, fd, buf, n)); }
ssize_t write(int fd, const void *buf, size_t n) { return __ret(__syscall(1, fd, buf, n)); }
ssize_t pread(int fd, void *buf, size_t n, off_t off) { return __ret(__syscall(17, fd, buf, n, off)); }
ssize_t pwrite(int fd, const void *buf, size_t n, off_t off) { return __ret(__syscall(18, fd, buf, n, off)); }

int open(const char *path, int flags, ...) {
    va_list ap;
    va_start(ap, flags);
    int mode = va_arg(ap, int);
    va_end(ap);
    return __ret(__syscall(2, path, flags, (flags & O_CREAT) ? mode : 0));
}

int creat(const char *path, mode_t mode) { return open(path, O_WRONLY | O_CREAT | O_TRUNC, mode); }
int close(int fd) { return __ret(__syscall(3, fd)); }
int stat(const char *path, struct stat *st) { return __ret(__syscall(4, path, st)); }
int fstat(int fd, struct stat *st) { return __ret(__syscall(5, fd, st)); }
int lstat(const char *path, struct stat *st) { return __ret(__syscall(6, path, st)); }
off_t lseek(int fd, off_t off, int whence) { return __ret(__syscall(8, fd, off, whence)); }

void *mmap(void *addr, size_t len, int prot, int flags, int fd, off_t off) {
    long r = __syscall(9, addr, len, prot, flags, fd, off);
    if (r < 0 && r > -4096) {
        errno = (int)-r;
        return MAP_FAILED;
    }
    return (void *)r;
}

int munmap(void *addr, size_t len) { return __ret(__syscall(11, addr, len)); }
int mprotect(void *addr, size_t len, int prot) { return __ret(__syscall(10, addr, len, prot)); }

int ioctl(int fd, unsigned long req, ...) {
    va_list ap;
    va_start(ap, req);
    long arg = va_arg(ap, long);
    va_end(ap);
    return __ret(__syscall(16, fd, req, arg));
}

int access(const char *path, int mode) { return __ret(__syscall(21, path, mode)); }
int pipe(int fds[2]) { return __ret(__syscall(22, fds)); }
int dup(int fd) { return __ret(__syscall(32, fd)); }
int dup2(int fd, int fd2) { return __ret(__syscall(33, fd, fd2)); }
int pause(void) { return __ret(__syscall(34)); }
unsigned alarm(unsigned s) { return (unsigned)__syscall(37, s); }
pid_t getpid(void) { return __syscall(39); }
pid_t fork(void) { return __ret(__syscall(57)); }
pid_t vfork(void) { return __ret(__syscall(57)); }
int execve(const char *path, char *const argv[], char *const envp[]) { return __ret(__syscall(59, path, argv, envp)); }
void _exit(int status) {
    __syscall(231, status);
    for (;;) {}
}
void _Exit(int status) { _exit(status); }
pid_t waitpid(pid_t pid, int *status, int options) { return __ret(__syscall(61, pid, status, options, 0)); }
pid_t wait(int *status) { return waitpid(-1, status, 0); }
int kill(pid_t pid, int sig) { return __ret(__syscall(62, pid, sig)); }
int raise(int sig) { return kill(getpid(), sig); }
int uname(struct utsname *u) { return __ret(__syscall(63, u)); }

int fcntl(int fd, int cmd, ...) {
    va_list ap;
    va_start(ap, cmd);
    long arg = va_arg(ap, long);
    va_end(ap);
    return __ret(__syscall(72, fd, cmd, arg));
}

int fsync(int fd) { return __ret(__syscall(74, fd)); }
int truncate(const char *path, off_t len) { return __ret(__syscall(76, path, len)); }
int ftruncate(int fd, off_t len) { return __ret(__syscall(77, fd, len)); }
int chdir(const char *path) { return __ret(__syscall(80, path)); }
int fchdir(int fd) { return __ret(__syscall(81, fd)); }
int rename(const char *from, const char *to) { return __ret(__syscall(82, from, to)); }
int mkdir(const char *path, mode_t mode) { return __ret(__syscall(83, path, mode)); }
int rmdir(const char *path) { return __ret(__syscall(84, path)); }
int link(const char *from, const char *to) { return __ret(__syscall(86, from, to)); }
int unlink(const char *path) { return __ret(__syscall(87, path)); }
int symlink(const char *from, const char *to) { return __ret(__syscall(88, from, to)); }
ssize_t readlink(const char *path, char *buf, size_t n) { return __ret(__syscall(89, path, buf, n)); }
int chmod(const char *path, mode_t mode) { return __ret(__syscall(90, path, mode)); }
int fchmod(int fd, mode_t mode) { return __ret(__syscall(91, fd, mode)); }
mode_t umask(mode_t mask) { return (mode_t)__syscall(95, mask); }
int gettimeofday(struct timeval *tv, void *tz) { return __ret(__syscall(96, tv, tz)); }
uid_t getuid(void) { return (uid_t)__syscall(102); }
gid_t getgid(void) { return (gid_t)__syscall(104); }
int setuid(uid_t uid) { return __ret(__syscall(105, uid)); }
int setgid(gid_t gid) { return __ret(__syscall(106, gid)); }
uid_t geteuid(void) { return (uid_t)__syscall(107); }
gid_t getegid(void) { return (gid_t)__syscall(108); }
int setpgid(pid_t pid, pid_t pgid) { return __ret(__syscall(109, pid, pgid)); }
pid_t getppid(void) { return __syscall(110); }
pid_t getpgrp(void) { return __syscall(111); }
pid_t setsid(void) { return __ret(__syscall(112)); }
void sync(void) { __syscall(162); }
int clock_gettime(clockid_t id, struct timespec *ts) { return __ret(__syscall(228, id, ts)); }
int nanosleep(const struct timespec *req, struct timespec *rem) { return __ret(__syscall(35, req, rem)); }

char *getcwd(char *buf, size_t n) {
    if (!buf) {
        if (n == 0)
            n = PATH_MAX;
        buf = malloc(n);
        if (!buf)
            return NULL;
    }
    if (__ret(__syscall(79, buf, n)) < 0)
        return NULL;
    return buf;
}

static char *__brk_cur;

int brk(void *addr) {
    long r = __syscall(12, addr);
    __brk_cur = (char *)r;
    if ((char *)r < (char *)addr) {
        errno = ENOMEM;
        return -1;
    }
    return 0;
}

void *sbrk(long inc) {
    if (!__brk_cur)
        __brk_cur = (char *)__syscall(12, 0);
    char *old = __brk_cur;
    if (inc == 0)
        return old;
    if (brk(old + inc) < 0)
        return (void *)-1;
    return old;
}

unsigned sleep(unsigned s) {
    struct timespec ts = {s, 0};
    return nanosleep(&ts, NULL) < 0 ? s : 0;
}

int usleep(useconds_t us) {
    struct timespec ts = {us / 1000000, (us % 1000000) * 1000};
    return nanosleep(&ts, NULL);
}

int isatty(int fd) {
    struct termios t;
    return ioctl(fd, TCGETS, &t) == 0;
}

char *ttyname(int fd) {
    static char name[] = "/dev/tty";
    return isatty(fd) ? name : NULL;
}

int tcgetattr(int fd, struct termios *t) { return ioctl(fd, TCGETS, t); }
int tcsetattr(int fd, int act, const struct termios *t) { return ioctl(fd, TCSETS + act, t); }

int gethostname(char *name, size_t n) {
    struct utsname u;
    if (uname(&u) < 0)
        return -1;
    strncpy(name, u.nodename, n);
    return 0;
}

long sysconf(int name) {
    switch (name) {
    case _SC_PAGESIZE:
        return 4096;
    case _SC_CLK_TCK:
        return 100;
    case _SC_NPROCESSORS_ONLN:
        return 1;
    }
    errno = EINVAL;
    return -1;
}

/* ---- signals -------------------------------------------------------------- */

struct __sigaction {
    sighandler_t handler;
    unsigned long flags;
    void (*restorer)(void);
    unsigned long mask;
};

void __restore_rt(void);
void __hcc_sigtramp(int sig);
static sighandler_t __handlers[NSIG];

void __sig_dispatch(int sig) {
    if (sig > 0 && sig < NSIG && __handlers[sig])
        __handlers[sig](sig);
}

sighandler_t signal(int sig, sighandler_t handler) {
    if (sig <= 0 || sig >= NSIG) {
        errno = EINVAL;
        return SIG_ERR;
    }
    sighandler_t old = __handlers[sig];
    struct __sigaction sa;
    sa.flags = 0x04000000 | 0x10000000; /* SA_RESTORER | SA_RESTART */
    sa.restorer = __restore_rt;
    sa.mask = 0;
    if (handler == SIG_DFL || handler == SIG_IGN) {
        sa.handler = handler;
        __handlers[sig] = NULL;
    } else {
        sa.handler = (sighandler_t)__hcc_sigtramp;
        __handlers[sig] = handler;
    }
    if (__ret(__syscall(13, sig, &sa, NULL, 8)) < 0)
        return SIG_ERR;
    return old ? old : SIG_DFL;
}

/* ---- memory allocation ------------------------------------------------- */

/* Power-of-two size classes carved from brk; big blocks use mmap. The
   header in front of each block records its class. */

#define __NCLASS 20
#define __BIG (1L << 19)

struct __block {
    unsigned long size; /* usable size */
    struct __block *next;
};

static struct __block *__free_lists[__NCLASS];

static int __class_of(size_t n) {
    int c = 0;
    size_t s = 16;
    while (s < n) {
        s <<= 1;
        c++;
    }
    return c;
}

void *malloc(size_t n) {
    if (n == 0)
        n = 1;
    if (n > __BIG) {
        size_t len = (n + 16 + 4095) & ~4095UL;
        char *p = mmap(NULL, len, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        if (p == MAP_FAILED)
            return NULL;
        ((struct __block *)p)->size = len - 16;
        return p + 16;
    }
    int c = __class_of(n);
    struct __block *b = __free_lists[c];
    if (b) {
        __free_lists[c] = b->next;
        return (char *)b + 16;
    }
    size_t size = 16UL << c;
    char *p = sbrk(size + 16);
    if (p == (char *)-1) {
        errno = ENOMEM;
        return NULL;
    }
    ((struct __block *)p)->size = size;
    return p + 16;
}

void free(void *ptr) {
    if (!ptr)
        return;
    struct __block *b = (struct __block *)((char *)ptr - 16);
    if (b->size > __BIG) {
        munmap(b, b->size + 16);
        return;
    }
    int c = __class_of(b->size);
    b->next = __free_lists[c];
    __free_lists[c] = b;
}

void *calloc(size_t n, size_t size) {
    if (size && n > (size_t)-1 / size) {
        errno = ENOMEM;
        return NULL;
    }
    void *p = malloc(n * size);
    if (p)
        memset(p, 0, n * size);
    return p;
}

void *realloc(void *ptr, size_t n) {
    if (!ptr)
        return malloc(n);
    if (n == 0) {
        free(ptr);
        return NULL;
    }
    struct __block *b = (struct __block *)((char *)ptr - 16);
    if (n <= b->size)
        return ptr;
    void *q = malloc(n);
    if (!q)
        return NULL;
    memcpy(q, ptr, b->size);
    free(ptr);
    return q;
}

void *reallocarray(void *p, size_t n, size_t size) {
    if (size && n > (size_t)-1 / size) {
        errno = ENOMEM;
        return NULL;
    }
    return realloc(p, n * size);
}

/* ---- strings -------------------------------------------------------------- */

void *memcpy(void *d, const void *s, size_t n) {
    char *dp = d;
    const char *sp = s;
    while (n >= 8) {
        *(long *)dp = *(const long *)sp;
        dp += 8;
        sp += 8;
        n -= 8;
    }
    while (n--)
        *dp++ = *sp++;
    return d;
}

void *memmove(void *d, const void *s, size_t n) {
    char *dp = d;
    const char *sp = s;
    if (dp <= sp || dp >= sp + n)
        return memcpy(d, s, n);
    while (n--)
        dp[n] = sp[n];
    return d;
}

void *memset(void *d, int c, size_t n) {
    unsigned char *p = d;
    while (n--)
        *p++ = (unsigned char)c;
    return d;
}

int memcmp(const void *a, const void *b, size_t n) {
    const unsigned char *x = a, *y = b;
    for (size_t i = 0; i < n; i++)
        if (x[i] != y[i])
            return x[i] - y[i];
    return 0;
}

void *memchr(const void *s, int c, size_t n) {
    const unsigned char *p = s;
    for (size_t i = 0; i < n; i++)
        if (p[i] == (unsigned char)c)
            return (void *)(p + i);
    return NULL;
}

void *memrchr(const void *s, int c, size_t n) {
    const unsigned char *p = s;
    while (n--)
        if (p[n] == (unsigned char)c)
            return (void *)(p + n);
    return NULL;
}

void *memmem(const void *h, size_t hn, const void *n, size_t nn) {
    const char *hp = h;
    if (nn == 0)
        return (void *)h;
    for (size_t i = 0; i + nn <= hn; i++)
        if (memcmp(hp + i, n, nn) == 0)
            return (void *)(hp + i);
    return NULL;
}

size_t strlen(const char *s) {
    const char *p = s;
    while (*p)
        p++;
    return p - s;
}

size_t strnlen(const char *s, size_t n) {
    size_t i = 0;
    while (i < n && s[i])
        i++;
    return i;
}

char *strcpy(char *d, const char *s) {
    char *r = d;
    while ((*d++ = *s++))
        ;
    return r;
}

char *stpcpy(char *d, const char *s) {
    while ((*d = *s++))
        d++;
    return d;
}

char *strncpy(char *d, const char *s, size_t n) {
    size_t i = 0;
    for (; i < n && s[i]; i++)
        d[i] = s[i];
    for (; i < n; i++)
        d[i] = 0;
    return d;
}

size_t strlcpy(char *d, const char *s, size_t n) {
    size_t len = strlen(s);
    if (n) {
        size_t c = len < n - 1 ? len : n - 1;
        memcpy(d, s, c);
        d[c] = 0;
    }
    return len;
}

size_t strlcat(char *d, const char *s, size_t n) {
    size_t dl = strnlen(d, n);
    if (dl == n)
        return n + strlen(s);
    return dl + strlcpy(d + dl, s, n - dl);
}

char *strcat(char *d, const char *s) {
    strcpy(d + strlen(d), s);
    return d;
}

char *strncat(char *d, const char *s, size_t n) {
    char *p = d + strlen(d);
    while (n-- && *s)
        *p++ = *s++;
    *p = 0;
    return d;
}

int strcmp(const char *a, const char *b) {
    while (*a && *a == *b) {
        a++;
        b++;
    }
    return (unsigned char)*a - (unsigned char)*b;
}

int strncmp(const char *a, const char *b, size_t n) {
    for (; n; n--, a++, b++) {
        if (*a != *b || !*a)
            return (unsigned char)*a - (unsigned char)*b;
    }
    return 0;
}

int strcoll(const char *a, const char *b) { return strcmp(a, b); }

size_t strxfrm(char *d, const char *s, size_t n) {
    size_t len = strlen(s);
    if (len < n)
        strcpy(d, s);
    return len;
}

int strcasecmp(const char *a, const char *b) {
    while (*a && tolower(*a) == tolower(*b)) {
        a++;
        b++;
    }
    return tolower((unsigned char)*a) - tolower((unsigned char)*b);
}

int strncasecmp(const char *a, const char *b, size_t n) {
    for (; n; n--, a++, b++) {
        int x = tolower((unsigned char)*a), y = tolower((unsigned char)*b);
        if (x != y || !x)
            return x - y;
    }
    return 0;
}

char *strchr(const char *s, int c) {
    for (;; s++) {
        if (*s == (char)c)
            return (char *)s;
        if (!*s)
            return NULL;
    }
}

char *strrchr(const char *s, int c) {
    const char *r = NULL;
    for (;; s++) {
        if (*s == (char)c)
            r = s;
        if (!*s)
            return (char *)r;
    }
}

char *strstr(const char *h, const char *n) {
    size_t nl = strlen(n);
    if (!nl)
        return (char *)h;
    for (; *h; h++)
        if (*h == *n && strncmp(h, n, nl) == 0)
            return (char *)h;
    return NULL;
}

char *strdup(const char *s) {
    size_t n = strlen(s) + 1;
    char *d = malloc(n);
    if (d)
        memcpy(d, s, n);
    return d;
}

char *strndup(const char *s, size_t n) {
    size_t l = strnlen(s, n);
    char *d = malloc(l + 1);
    if (d) {
        memcpy(d, s, l);
        d[l] = 0;
    }
    return d;
}

size_t strspn(const char *s, const char *accept) {
    size_t i = 0;
    while (s[i] && strchr(accept, s[i]))
        i++;
    return i;
}

size_t strcspn(const char *s, const char *reject) {
    size_t i = 0;
    while (s[i] && !strchr(reject, s[i]))
        i++;
    return i;
}

char *strpbrk(const char *s, const char *accept) {
    s += strcspn(s, accept);
    return *s ? (char *)s : NULL;
}

char *strtok_r(char *s, const char *delim, char **save) {
    if (!s)
        s = *save;
    s += strspn(s, delim);
    if (!*s) {
        *save = s;
        return NULL;
    }
    char *end = s + strcspn(s, delim);
    if (*end)
        *end++ = 0;
    *save = end;
    return s;
}

char *strtok(char *s, const char *delim) {
    static char *save;
    return strtok_r(s, delim, &save);
}

char *strsep(char **sp, const char *delim) {
    char *s = *sp;
    if (!s)
        return NULL;
    char *end = s + strcspn(s, delim);
    if (*end) {
        *end = 0;
        *sp = end + 1;
    } else {
        *sp = NULL;
    }
    return s;
}

char *strerror(int e) {
    switch (e) {
    case 0: return "Success";
    case EPERM: return "Operation not permitted";
    case ENOENT: return "No such file or directory";
    case ESRCH: return "No such process";
    case EINTR: return "Interrupted system call";
    case EIO: return "Input/output error";
    case E2BIG: return "Argument list too long";
    case ENOEXEC: return "Exec format error";
    case EBADF: return "Bad file descriptor";
    case ECHILD: return "No child processes";
    case EAGAIN: return "Resource temporarily unavailable";
    case ENOMEM: return "Cannot allocate memory";
    case EACCES: return "Permission denied";
    case EFAULT: return "Bad address";
    case EBUSY: return "Device or resource busy";
    case EEXIST: return "File exists";
    case EXDEV: return "Invalid cross-device link";
    case ENODEV: return "No such device";
    case ENOTDIR: return "Not a directory";
    case EISDIR: return "Is a directory";
    case EINVAL: return "Invalid argument";
    case EMFILE: return "Too many open files";
    case ENOTTY: return "Inappropriate ioctl for device";
    case EFBIG: return "File too large";
    case ENOSPC: return "No space left on device";
    case ESPIPE: return "Illegal seek";
    case EROFS: return "Read-only file system";
    case EPIPE: return "Broken pipe";
    case EDOM: return "Numerical argument out of domain";
    case ERANGE: return "Numerical result out of range";
    case ENAMETOOLONG: return "File name too long";
    case ENOSYS: return "Function not implemented";
    case ENOTEMPTY: return "Directory not empty";
    case ELOOP: return "Too many levels of symbolic links";
    case ETIMEDOUT: return "Connection timed out";
    case ENOTSOCK: return "Socket operation on non-socket";
    case EMSGSIZE: return "Message too long";
    case EPROTONOSUPPORT: return "Protocol not supported";
    case EAFNOSUPPORT: return "Address family not supported by protocol";
    case EADDRINUSE: return "Address already in use";
    case EADDRNOTAVAIL: return "Cannot assign requested address";
    case ENETUNREACH: return "Network is unreachable";
    case ECONNRESET: return "Connection reset by peer";
    case EISCONN: return "Transport endpoint is already connected";
    case ENOTCONN: return "Transport endpoint is not connected";
    case ECONNREFUSED: return "Connection refused";
    case EHOSTUNREACH: return "No route to host";
    case EINPROGRESS: return "Operation now in progress";
    }
    return "Unknown error";
}

/* ---- ctype ---------------------------------------------------------------- */

int isdigit(int c) { return c >= '0' && c <= '9'; }
int islower(int c) { return c >= 'a' && c <= 'z'; }
int isupper(int c) { return c >= 'A' && c <= 'Z'; }
int isalpha(int c) { return islower(c) || isupper(c); }
int isalnum(int c) { return isalpha(c) || isdigit(c); }
int isxdigit(int c) { return isdigit(c) || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F'); }
int isspace(int c) { return c == ' ' || (c >= '\t' && c <= '\r'); }
int isblank(int c) { return c == ' ' || c == '\t'; }
int iscntrl(int c) { return (c >= 0 && c < 32) || c == 127; }
int isprint(int c) { return c >= 32 && c < 127; }
int isgraph(int c) { return c > 32 && c < 127; }
int ispunct(int c) { return isgraph(c) && !isalnum(c); }
int isascii(int c) { return c >= 0 && c < 128; }
int tolower(int c) { return isupper(c) ? c + 32 : c; }
int toupper(int c) { return islower(c) ? c - 32 : c; }

/* ---- numbers -------------------------------------------------------------- */

static int __digit(int c) {
    if (isdigit(c))
        return c - '0';
    if (c >= 'a' && c <= 'z')
        return c - 'a' + 10;
    if (c >= 'A' && c <= 'Z')
        return c - 'A' + 10;
    return 99;
}

/* Parses an unsigned magnitude; *neg receives the sign. */
static unsigned long __strtou(const char *s, char **end, int base, int *neg, int *overflow) {
    const char *p = s;
    unsigned long v = 0;
    *neg = 0;
    *overflow = 0;
    while (isspace(*p))
        p++;
    if (*p == '-' || *p == '+')
        *neg = *p++ == '-';
    if ((base == 0 || base == 16) && p[0] == '0' && (p[1] == 'x' || p[1] == 'X') && isxdigit(p[2])) {
        p += 2;
        base = 16;
    } else if (base == 0) {
        base = *p == '0' ? 8 : 10;
    }
    const char *start = p;
    int d;
    while ((d = __digit(*p)) < base) {
        if (v > (ULONG_MAX - d) / base)
            *overflow = 1;
        v = v * base + d;
        p++;
    }
    if (end)
        *end = (char *)(p == start ? s : p);
    return v;
}

long strtol(const char *s, char **end, int base) {
    int neg, of;
    unsigned long v = __strtou(s, end, base, &neg, &of);
    if (of || v > (unsigned long)LONG_MAX + neg) {
        errno = ERANGE;
        return neg ? LONG_MIN : LONG_MAX;
    }
    return neg ? -(long)v : (long)v;
}

unsigned long strtoul(const char *s, char **end, int base) {
    int neg, of;
    unsigned long v = __strtou(s, end, base, &neg, &of);
    if (of) {
        errno = ERANGE;
        return ULONG_MAX;
    }
    return neg ? -v : v;
}

long long strtoll(const char *s, char **end, int base) { return strtol(s, end, base); }
unsigned long long strtoull(const char *s, char **end, int base) { return strtoul(s, end, base); }
intmax_t strtoimax(const char *s, char **end, int base) { return strtol(s, end, base); }
uintmax_t strtoumax(const char *s, char **end, int base) { return strtoul(s, end, base); }
int atoi(const char *s) { return (int)strtol(s, NULL, 10); }
long atol(const char *s) { return strtol(s, NULL, 10); }
long long atoll(const char *s) { return strtol(s, NULL, 10); }

static double __pow10(int e) {
    double r = 1, b = 10;
    int neg = e < 0;
    if (neg)
        e = -e;
    while (e) {
        if (e & 1)
            r *= b;
        b *= b;
        e >>= 1;
    }
    return neg ? 1 / r : r;
}

double strtod(const char *s, char **end) {
    const char *p = s;
    int neg = 0;
    while (isspace(*p))
        p++;
    if (*p == '-' || *p == '+')
        neg = *p++ == '-';
    if (strncasecmp(p, "inf", 3) == 0) {
        p += strncasecmp(p, "infinity", 8) == 0 ? 8 : 3;
        if (end)
            *end = (char *)p;
        return neg ? -INFINITY : INFINITY;
    }
    if (strncasecmp(p, "nan", 3) == 0) {
        if (end)
            *end = (char *)p + 3;
        return NAN;
    }
    unsigned long m = 0;
    int digits = 0, exp = 0, any = 0;
    while (isdigit(*p)) {
        if (digits < 19) {
            m = m * 10 + (*p - '0');
            if (m)
                digits++;
        } else {
            exp++;
        }
        p++;
        any = 1;
    }
    if (*p == '.') {
        p++;
        while (isdigit(*p)) {
            if (digits < 19) {
                m = m * 10 + (*p - '0');
                if (m)
                    digits++;
                exp--;
            }
            p++;
            any = 1;
        }
    }
    if (!any) {
        if (end)
            *end = (char *)s;
        return 0;
    }
    if (*p == 'e' || *p == 'E') {
        const char *q = p + 1;
        int eneg = 0, e = 0;
        if (*q == '-' || *q == '+')
            eneg = *q++ == '-';
        if (isdigit(*q)) {
            while (isdigit(*q)) {
                if (e < 10000)
                    e = e * 10 + (*q - '0');
                q++;
            }
            exp += eneg ? -e : e;
            p = q;
        }
    }
    if (end)
        *end = (char *)p;
    double v = (double)m;
    if (exp < -300) {
        v *= __pow10(-300);
        exp += 300;
    }
    v *= __pow10(exp);
    if (v == INFINITY)
        errno = ERANGE;
    return neg ? -v : v;
}

float strtof(const char *s, char **end) { return (float)strtod(s, end); }
long double strtold(const char *s, char **end) { return strtod(s, end); }
double atof(const char *s) { return strtod(s, NULL); }

int abs(int x) { return x < 0 ? -x : x; }
long labs(long x) { return x < 0 ? -x : x; }
long long llabs(long long x) { return x < 0 ? -x : x; }

div_t div(int a, int b) {
    div_t r;
    r.quot = a / b;
    r.rem = a % b;
    return r;
}

ldiv_t ldiv(long a, long b) {
    ldiv_t r;
    r.quot = a / b;
    r.rem = a % b;
    return r;
}

static unsigned long __rand_state = 1;

int rand(void) {
    __rand_state = __rand_state * 6364136223846793005UL + 1442695040888963407UL;
    return (int)(__rand_state >> 33);
}

void srand(unsigned seed) { __rand_state = seed; }
long random(void) { return rand(); }
void srandom(unsigned seed) { srand(seed); }

static void __swap(char *a, char *b, size_t n) {
    while (n--) {
        char t = *a;
        *a++ = *b;
        *b++ = t;
    }
}

static void __qsort(char *base, size_t n, size_t size, int (*cmp)(const void *, const void *)) {
    while (n > 1) {
        if (n < 8) {
            for (size_t i = 1; i < n; i++)
                for (size_t j = i; j > 0 && cmp(base + (j - 1) * size, base + j * size) > 0; j--)
                    __swap(base + (j - 1) * size, base + j * size, size);
            return;
        }
        __swap(base, base + (n / 2) * size, size);
        size_t last = 0;
        for (size_t i = 1; i < n; i++)
            if (cmp(base + i * size, base) < 0)
                __swap(base + ++last * size, base + i * size, size);
        __swap(base, base + last * size, size);
        /* Recurse into the smaller part, loop on the larger. */
        if (last < n - last - 1) {
            __qsort(base, last, size, cmp);
            base += (last + 1) * size;
            n -= last + 1;
        } else {
            __qsort(base + (last + 1) * size, n - last - 1, size, cmp);
            n = last;
        }
    }
}

void qsort(void *base, size_t n, size_t size, int (*cmp)(const void *, const void *)) { __qsort(base, n, size, cmp); }

void *bsearch(const void *key, const void *base, size_t n, size_t size, int (*cmp)(const void *, const void *)) {
    const char *b = base;
    while (n > 0) {
        const char *mid = b + (n / 2) * size;
        int c = cmp(key, mid);
        if (c == 0)
            return (void *)mid;
        if (c > 0) {
            b = mid + size;
            n -= n / 2 + 1;
        } else {
            n /= 2;
        }
    }
    return NULL;
}

/* ---- environment and processes -------------------------------------------- */

char *getenv(const char *name) {
    size_t n = strlen(name);
    if (!environ)
        return NULL;
    for (char **e = environ; *e; e++)
        if (strncmp(*e, name, n) == 0 && (*e)[n] == '=')
            return *e + n + 1;
    return NULL;
}

static int __env_owned;

static int __env_put(char *entry, size_t keylen, int overwrite) {
    size_t n = 0;
    for (char **e = environ; e && *e; e++, n++) {
        if (strncmp(*e, entry, keylen) == 0 && (*e)[keylen] == '=') {
            if (overwrite)
                *e = entry;
            return 0;
        }
    }
    char **ne = malloc((n + 2) * sizeof(char *));
    if (!ne)
        return -1;
    for (size_t i = 0; i < n; i++)
        ne[i] = environ[i];
    ne[n] = entry;
    ne[n + 1] = NULL;
    if (__env_owned)
        free(environ);
    environ = ne;
    __env_owned = 1;
    return 0;
}

int setenv(const char *name, const char *value, int overwrite) {
    size_t n = strlen(name);
    if (!n || strchr(name, '=')) {
        errno = EINVAL;
        return -1;
    }
    char *entry = malloc(n + strlen(value) + 2);
    if (!entry)
        return -1;
    strcpy(entry, name);
    entry[n] = '=';
    strcpy(entry + n + 1, value);
    return __env_put(entry, n, overwrite);
}

int putenv(char *s) {
    char *eq = strchr(s, '=');
    if (!eq)
        return unsetenv(s);
    return __env_put(s, eq - s, 1);
}

int unsetenv(const char *name) {
    size_t n = strlen(name);
    if (!environ)
        return 0;
    char **w = environ;
    for (char **e = environ; *e; e++)
        if (!(strncmp(*e, name, n) == 0 && (*e)[n] == '='))
            *w++ = *e;
    *w = NULL;
    return 0;
}

int execv(const char *path, char *const argv[]) { return execve(path, argv, environ); }

int execvp(const char *file, char *const argv[]) {
    if (strchr(file, '/'))
        return execve(file, argv, environ);
    const char *path = getenv("PATH");
    if (!path)
        path = "/bin:/usr/bin";
    char buf[PATH_MAX];
    while (*path) {
        size_t n = strcspn(path, ":");
        if (n + strlen(file) + 2 <= sizeof(buf)) {
            memcpy(buf, path, n);
            buf[n] = '/';
            strcpy(buf + n + 1, file);
            execve(buf, argv, environ);
        }
        path += n;
        if (*path == ':')
            path++;
    }
    errno = ENOENT;
    return -1;
}

static int __execl(const char *path, const char *arg, va_list ap, int search) {
    char *argv[64];
    int n = 0;
    argv[n++] = (char *)arg;
    while (arg && n < 63) {
        arg = va_arg(ap, char *);
        argv[n++] = (char *)arg;
    }
    argv[n] = NULL;
    return search ? execvp(path, argv) : execv(path, argv);
}

int execl(const char *path, const char *arg, ...) {
    va_list ap;
    va_start(ap, arg);
    return __execl(path, arg, ap, 0);
}

int execlp(const char *file, const char *arg, ...) {
    va_list ap;
    va_start(ap, arg);
    return __execl(file, arg, ap, 1);
}

int system(const char *cmd) {
    if (!cmd)
        return 1;
    fflush(NULL);
    pid_t pid = fork();
    if (pid < 0)
        return -1;
    if (pid == 0) {
        char *argv[] = {"sh", "-c", (char *)cmd, NULL};
        execve("/bin/sh", argv, environ);
        _exit(127);
    }
    int status;
    while (waitpid(pid, &status, 0) < 0)
        if (errno != EINTR)
            return -1;
    return status;
}

static void (*__atexit_fns[32])(void);
static int __atexit_n;

int atexit(void (*fn)(void)) {
    if (__atexit_n == 32)
        return -1;
    __atexit_fns[__atexit_n++] = fn;
    return 0;
}

void exit(int status) {
    while (__atexit_n > 0)
        __atexit_fns[--__atexit_n]();
    fflush(NULL);
    _exit(status);
}

void abort(void) {
    fflush(NULL);
    signal(SIGABRT, SIG_DFL);
    raise(SIGABRT);
    _exit(134);
}

void __assert_fail(const char *expr, const char *file, int line, const char *func) {
    fprintf(stderr, "%s:%d: %s: Assertion `%s' failed.\n", file, line, func, expr);
    abort();
}

char *setlocale(int cat, const char *locale) {
    static char c[] = "C";
    return c;
}

struct lconv *localeconv(void) {
    static struct lconv l = {".", "", ""};
    return &l;
}

char *realpath(const char *path, char *resolved) {
    char buf[PATH_MAX];
    char *out = resolved ? resolved : malloc(PATH_MAX);
    if (!out)
        return NULL;
    if (path[0] == '/') {
        out[0] = 0;
    } else if (!getcwd(out, PATH_MAX)) {
        return NULL;
    }
    strncpy(buf, path, PATH_MAX - 1);
    buf[PATH_MAX - 1] = 0;
    char *save;
    for (char *part = strtok_r(buf, "/", &save); part; part = strtok_r(NULL, "/", &save)) {
        if (strcmp(part, ".") == 0)
            continue;
        if (strcmp(part, "..") == 0) {
            char *slash = strrchr(out, '/');
            if (slash)
                *slash = 0;
            continue;
        }
        size_t n = strlen(out);
        if (n + strlen(part) + 2 > PATH_MAX) {
            errno = ENAMETOOLONG;
            return NULL;
        }
        if (n == 0 || out[n - 1] != '/')
            out[n++] = '/';
        strcpy(out + n, part);
    }
    if (!out[0])
        strcpy(out, "/");
    struct stat st;
    if (stat(out, &st) < 0)
        return NULL;
    return out;
}

static unsigned long __tmp_counter;

char *mktemp(char *tmpl) {
    size_t n = strlen(tmpl);
    if (n < 6 || strcmp(tmpl + n - 6, "XXXXXX") != 0) {
        errno = EINVAL;
        return tmpl;
    }
    unsigned long v = getpid() * 7919UL + __tmp_counter++ * 104729UL + time(NULL);
    for (int i = 0; i < 6; i++) {
        tmpl[n - 6 + i] = "abcdefghijklmnopqrstuvwxyz0123456789"[v % 36];
        v /= 36;
    }
    return tmpl;
}

int mkstemp(char *tmpl) {
    size_t n = strlen(tmpl);
    char saved[8];
    if (n >= 6)
        strcpy(saved, tmpl + n - 6);
    for (int tries = 0; tries < 100; tries++) {
        if (n >= 6)
            strcpy(tmpl + n - 6, saved);
        mktemp(tmpl);
        int fd = open(tmpl, O_RDWR | O_CREAT | O_EXCL, 0600);
        if (fd >= 0 || errno != EEXIST)
            return fd;
    }
    return -1;
}

char *optarg;
int optind = 1, opterr = 1, optopt;
static int __optpos;

int getopt(int argc, char *const argv[], const char *opts) {
    if (optind >= argc || !argv[optind] || argv[optind][0] != '-' || !argv[optind][1])
        return -1;
    if (strcmp(argv[optind], "--") == 0) {
        optind++;
        return -1;
    }
    if (!__optpos)
        __optpos = 1;
    int c = argv[optind][__optpos++];
    const char *spec = strchr(opts, c);
    int last = !argv[optind][__optpos];
    if (!spec || c == ':') {
        optopt = c;
        if (opterr && opts[0] != ':')
            fprintf(stderr, "%s: invalid option -- '%c'\n", argv[0], c);
        if (last) {
            optind++;
            __optpos = 0;
        }
        return '?';
    }
    if (spec[1] == ':') {
        if (!last) {
            optarg = &argv[optind][__optpos];
        } else if (optind + 1 < argc) {
            optarg = argv[++optind];
        } else {
            optopt = c;
            optind++;
            __optpos = 0;
            if (opts[0] == ':')
                return ':';
            if (opterr)
                fprintf(stderr, "%s: option requires an argument -- '%c'\n", argv[0], c);
            return '?';
        }
        optind++;
        __optpos = 0;
        return c;
    }
    if (last) {
        optind++;
        __optpos = 0;
    }
    return c;
}

/* ---- directories ------------------------------------------------------- */

struct _DIR {
    int fd;
    int pos, len;
    char buf[4096];
    struct dirent ent;
};

DIR *opendir(const char *path) {
    int fd = open(path, O_RDONLY | O_DIRECTORY);
    if (fd < 0)
        return NULL;
    DIR *d = calloc(1, sizeof(DIR));
    if (!d) {
        close(fd);
        return NULL;
    }
    d->fd = fd;
    return d;
}

struct dirent *readdir(DIR *d) {
    if (d->pos >= d->len) {
        long n = __ret(__syscall(217, d->fd, d->buf, sizeof(d->buf)));
        if (n <= 0)
            return NULL;
        d->len = (int)n;
        d->pos = 0;
    }
    char *p = d->buf + d->pos;
    unsigned short reclen = *(unsigned short *)(p + 16);
    d->ent.d_ino = *(unsigned long *)p;
    d->ent.d_off = *(long *)(p + 8);
    d->ent.d_reclen = reclen;
    d->ent.d_type = *(unsigned char *)(p + 18);
    strncpy(d->ent.d_name, p + 19, 255);
    d->ent.d_name[255] = 0;
    d->pos += reclen;
    return &d->ent;
}

int closedir(DIR *d) {
    int r = close(d->fd);
    free(d);
    return r;
}

void rewinddir(DIR *d) {
    lseek(d->fd, 0, SEEK_SET);
    d->pos = d->len = 0;
}

int dirfd(DIR *d) { return d->fd; }

/* ---- time --------------------------------------------------------------------- */

time_t time(time_t *t) {
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    if (t)
        *t = ts.tv_sec;
    return ts.tv_sec;
}

static struct timespec __clock_start;

clock_t clock(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (ts.tv_sec - __clock_start.tv_sec) * 1000000L + (ts.tv_nsec - __clock_start.tv_nsec) / 1000;
}

double difftime(time_t a, time_t b) { return (double)(a - b); }

static const char *__wday[] = {"Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"};
static const char *__wday_full[] = {"Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"};
static const char *__mon[] = {"Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"};
static const char *__mon_full[] = {"January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"};

static int __leap(long y) { return (y % 4 == 0 && y % 100 != 0) || y % 400 == 0; }

struct tm *gmtime_r(const time_t *t, struct tm *tm) {
    long days = *t / 86400, secs = *t % 86400;
    if (secs < 0) {
        secs += 86400;
        days--;
    }
    tm->tm_hour = (int)(secs / 3600);
    tm->tm_min = (int)(secs / 60 % 60);
    tm->tm_sec = (int)(secs % 60);
    tm->tm_wday = (int)((days % 7 + 11) % 7); /* 1970-01-01 was a Thursday */
    /* Civil-from-days (Howard Hinnant). */
    long z = days + 719468;
    long era = (z >= 0 ? z : z - 146096) / 146097;
    long doe = z - era * 146097;
    long yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    long y = yoe + era * 400;
    long doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    long mp = (5 * doy + 2) / 153;
    long d = doy - (153 * mp + 2) / 5 + 1;
    long m = mp < 10 ? mp + 3 : mp - 9;
    if (m <= 2)
        y++;
    tm->tm_year = (int)(y - 1900);
    tm->tm_mon = (int)(m - 1);
    tm->tm_mday = (int)d;
    static const int cum[] = {0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334};
    tm->tm_yday = cum[m - 1] + (int)d - 1 + (m > 2 && __leap(y));
    tm->tm_isdst = 0;
    tm->tm_gmtoff = 0;
    tm->tm_zone = "UTC";
    return tm;
}

struct tm *gmtime(const time_t *t) {
    static struct tm tm;
    return gmtime_r(t, &tm);
}

struct tm *localtime_r(const time_t *t, struct tm *tm) { return gmtime_r(t, tm); }
struct tm *localtime(const time_t *t) { return gmtime(t); }

time_t timegm(struct tm *tm) {
    long y = tm->tm_year + 1900L, m = tm->tm_mon + 1;
    /* Normalize the month. */
    y += (m - 1) / 12;
    m = (m - 1) % 12 + 1;
    if (m <= 0) {
        m += 12;
        y--;
    }
    if (m <= 2)
        y--;
    long era = (y >= 0 ? y : y - 399) / 400;
    long yoe = y - era * 400;
    long doy = (153 * (m > 2 ? m - 3 : m + 9) + 2) / 5 + tm->tm_mday - 1;
    long doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    long days = era * 146097 + doe - 719468;
    time_t t = days * 86400 + tm->tm_hour * 3600L + tm->tm_min * 60L + tm->tm_sec;
    gmtime_r(&t, tm);
    return t;
}

time_t mktime(struct tm *tm) { return timegm(tm); }

size_t strftime(char *s, size_t max, const char *fmt, const struct tm *tm) {
    size_t n = 0;
    char tmp[64];
    for (; *fmt; fmt++) {
        if (*fmt != '%') {
            if (n + 1 >= max)
                return 0;
            s[n++] = *fmt;
            continue;
        }
        fmt++;
        tmp[0] = 0;
        switch (*fmt) {
        case 'Y': snprintf(tmp, sizeof tmp, "%d", tm->tm_year + 1900); break;
        case 'y': snprintf(tmp, sizeof tmp, "%02d", tm->tm_year % 100); break;
        case 'C': snprintf(tmp, sizeof tmp, "%02d", (tm->tm_year + 1900) / 100); break;
        case 'm': snprintf(tmp, sizeof tmp, "%02d", tm->tm_mon + 1); break;
        case 'd': snprintf(tmp, sizeof tmp, "%02d", tm->tm_mday); break;
        case 'e': snprintf(tmp, sizeof tmp, "%2d", tm->tm_mday); break;
        case 'H': snprintf(tmp, sizeof tmp, "%02d", tm->tm_hour); break;
        case 'I': snprintf(tmp, sizeof tmp, "%02d", (tm->tm_hour + 11) % 12 + 1); break;
        case 'M': snprintf(tmp, sizeof tmp, "%02d", tm->tm_min); break;
        case 'S': snprintf(tmp, sizeof tmp, "%02d", tm->tm_sec); break;
        case 'j': snprintf(tmp, sizeof tmp, "%03d", tm->tm_yday + 1); break;
        case 'u': snprintf(tmp, sizeof tmp, "%d", tm->tm_wday ? tm->tm_wday : 7); break;
        case 'w': snprintf(tmp, sizeof tmp, "%d", tm->tm_wday); break;
        case 'a': strcpy(tmp, __wday[tm->tm_wday % 7]); break;
        case 'A': strcpy(tmp, __wday_full[tm->tm_wday % 7]); break;
        case 'b': case 'h': strcpy(tmp, __mon[tm->tm_mon % 12]); break;
        case 'B': strcpy(tmp, __mon_full[tm->tm_mon % 12]); break;
        case 'p': strcpy(tmp, tm->tm_hour < 12 ? "AM" : "PM"); break;
        case 'Z': strcpy(tmp, "UTC"); break;
        case 'z': strcpy(tmp, "+0000"); break;
        case 'n': strcpy(tmp, "\n"); break;
        case 't': strcpy(tmp, "\t"); break;
        case '%': strcpy(tmp, "%"); break;
        case 's': {
            struct tm c = *tm;
            snprintf(tmp, sizeof tmp, "%ld", (long)timegm(&c));
            break;
        }
        case 'F': strftime(tmp, sizeof tmp, "%Y-%m-%d", tm); break;
        case 'T': strftime(tmp, sizeof tmp, "%H:%M:%S", tm); break;
        case 'R': strftime(tmp, sizeof tmp, "%H:%M", tm); break;
        case 'D': strftime(tmp, sizeof tmp, "%m/%d/%y", tm); break;
        case 'c': strftime(tmp, sizeof tmp, "%a %b %e %H:%M:%S %Y", tm); break;
        case 'x': strftime(tmp, sizeof tmp, "%m/%d/%y", tm); break;
        case 'X': strftime(tmp, sizeof tmp, "%H:%M:%S", tm); break;
        case 0: fmt--; break;
        default: tmp[0] = '%'; tmp[1] = *fmt; tmp[2] = 0;
        }
        size_t l = strlen(tmp);
        if (n + l >= max)
            return 0;
        memcpy(s + n, tmp, l);
        n += l;
    }
    s[n] = 0;
    return n;
}

char *asctime(const struct tm *tm) {
    static char buf[32];
    strftime(buf, sizeof buf, "%a %b %e %H:%M:%S %Y\n", tm);
    return buf;
}

char *ctime(const time_t *t) { return asctime(localtime(t)); }

/* ---- math ------------------------------------------------------------------- */

union __dbits {
    double d;
    unsigned long u;
};

int __signbit(double x) {
    union __dbits b;
    b.d = x;
    return (int)(b.u >> 63);
}

int __fpclassify(double x) {
    union __dbits b;
    b.d = x;
    int e = (int)(b.u >> 52 & 0x7ff);
    unsigned long m = b.u & 0xfffffffffffffUL;
    if (e == 0x7ff)
        return m ? FP_NAN : FP_INFINITE;
    if (e == 0)
        return m ? FP_SUBNORMAL : FP_ZERO;
    return FP_NORMAL;
}

double sqrt(double x) { return __builtin_sqrt(x); }

double fabs(double x) {
    union __dbits b;
    b.d = x;
    b.u &= ~(1UL << 63);
    return b.d;
}

double copysign(double x, double y) {
    union __dbits a, b;
    a.d = x;
    b.d = y;
    a.u = (a.u & ~(1UL << 63)) | (b.u & (1UL << 63));
    return a.d;
}

double trunc(double x) {
    if (!(fabs(x) < 4503599627370496.0))
        return x;
    double r = (double)(long)x;
    return r == 0 ? copysign(0, x) : r;
}

double floor(double x) {
    double t = trunc(x);
    return t > x ? t - 1 : t;
}

double ceil(double x) {
    double t = trunc(x);
    return t < x ? t + 1 : t;
}

double round(double x) { return x < 0 ? -floor(-x + 0.5) : floor(x + 0.5); }

double rint(double x) {
    double f = floor(x), d = x - f;
    if (d > 0.5 || (d == 0.5 && fmod(f, 2) != 0))
        return f + 1;
    return f;
}

double nearbyint(double x) { return rint(x); }
long lround(double x) { return (long)round(x); }
long lrint(double x) { return (long)rint(x); }

double fmod(double x, double y) {
    if (y == 0 || isinf(x) || isnan(x) || isnan(y))
        return NAN;
    if (isinf(y))
        return x;
    double ay = fabs(y), r = fabs(x);
    /* Subtract shifted multiples of y: exact, unlike x - trunc(x/y)*y. */
    while (r >= ay) {
        double t = ay;
        while (t * 2 <= r)
            t *= 2;
        r -= t;
    }
    return x < 0 ? -r : r;
}

double remainder(double x, double y) { return x - rint(x / y) * y; }

double modf(double x, double *ip) {
    double t = trunc(x);
    *ip = t;
    return x - t;
}

double frexp(double x, int *e) {
    union __dbits b;
    b.d = x;
    int ex = (int)(b.u >> 52 & 0x7ff);
    if (ex == 0) {
        if (x == 0) {
            *e = 0;
            return x;
        }
        b.d = x * 18014398509481984.0; /* 2^54 */
        ex = (int)(b.u >> 52 & 0x7ff) - 54;
    } else if (ex == 0x7ff) {
        *e = 0;
        return x;
    }
    *e = ex - 1022;
    b.u = (b.u & ~(0x7ffUL << 52)) | (1022UL << 52);
    return b.d;
}

double ldexp(double x, int e) {
    while (e > 1000) {
        x *= 1.0715086071862673e301; /* 2^1000 */
        e -= 1000;
    }
    while (e < -1000) {
        x *= 9.332636185032189e-302; /* 2^-1000 */
        e += 1000;
    }
    union __dbits b;
    b.u = (unsigned long)(e + 1023) << 52;
    return x * b.d;
}

double scalbn(double x, int e) { return ldexp(x, e); }

double exp(double x) {
    if (isnan(x))
        return x;
    if (x > 709.8)
        return INFINITY;
    if (x < -745.2)
        return 0;
    double k = round(x / M_LN2);
    double r = x - k * 0.6931471803691238 - k * 1.9082149292705877e-10;
    /* exp(r) for |r| <= ln2/2 by Taylor series. */
    double term = 1, sum = 1;
    for (int i = 1; i < 20; i++) {
        term *= r / i;
        sum += term;
    }
    return ldexp(sum, (int)k);
}

double exp2(double x) { return exp(x * M_LN2); }
double expm1(double x) { return fabs(x) < 1e-5 ? x + x * x / 2 + x * x * x / 6 : exp(x) - 1; }

double log(double x) {
    if (isnan(x) || x < 0)
        return NAN;
    if (x == 0)
        return -INFINITY;
    if (isinf(x))
        return x;
    int e;
    double m = frexp(x, &e); /* x = m * 2^e, m in [0.5, 1) */
    if (m < M_SQRT1_2) {
        m *= 2;
        e--;
    }
    /* log(m) = 2 atanh((m-1)/(m+1)) */
    double s = (m - 1) / (m + 1), s2 = s * s, term = s, sum = 0;
    for (int i = 1; i < 40; i += 2) {
        sum += term / i;
        term *= s2;
    }
    return 2 * sum + e * M_LN2;
}

double log2(double x) { return log(x) / M_LN2; }
double log10(double x) { return log(x) / M_LN10; }
double log1p(double x) { return fabs(x) < 1e-5 ? x - x * x / 2 + x * x * x / 3 : log(1 + x); }

double pow(double x, double y) {
    if (y == 0)
        return 1;
    if (isnan(x) || isnan(y))
        return NAN;
    if (y == trunc(y) && fabs(y) < 1e18) {
        long n = (long)y;
        int neg = n < 0;
        unsigned long u = neg ? -(unsigned long)n : (unsigned long)n;
        double r = 1, b = x;
        while (u) {
            if (u & 1)
                r *= b;
            b *= b;
            u >>= 1;
        }
        return neg ? 1 / r : r;
    }
    if (x < 0)
        return NAN;
    if (x == 0)
        return y > 0 ? 0 : INFINITY;
    return exp(y * log(x));
}

double cbrt(double x) {
    if (x == 0 || isnan(x) || isinf(x))
        return x;
    double r = copysign(exp(log(fabs(x)) / 3), x);
    r = r - (r * r * r - x) / (3 * r * r);
    return r;
}

double hypot(double x, double y) {
    x = fabs(x);
    y = fabs(y);
    if (x < y) {
        double t = x;
        x = y;
        y = t;
    }
    if (x == 0)
        return 0;
    double r = y / x;
    return x * sqrt(1 + r * r);
}

/* sin/cos of |x| <= pi/4 */
static double __sin_k(double x) {
    double x2 = x * x, term = x, sum = x;
    for (int i = 1; i < 12; i++) {
        term *= -x2 / ((2 * i) * (2 * i + 1));
        sum += term;
    }
    return sum;
}

static double __cos_k(double x) {
    double x2 = x * x, term = 1, sum = 1;
    for (int i = 1; i < 12; i++) {
        term *= -x2 / ((2 * i - 1) * (2 * i));
        sum += term;
    }
    return sum;
}

/* Reduces x to r in [-pi/4, pi/4] with x = r + q*pi/2. */
static double __reduce(double x, int *q) {
    double k = round(x / M_PI_2);
    *q = (int)((long)k & 3);
    return x - k * 1.5707963267341256 - k * 6.077100506506192e-11;
}

double sin(double x) {
    if (isnan(x) || isinf(x))
        return NAN;
    int q;
    double r = __reduce(x, &q);
    switch (q) {
    case 0: return __sin_k(r);
    case 1: return __cos_k(r);
    case 2: return -__sin_k(r);
    default: return -__cos_k(r);
    }
}

double cos(double x) {
    if (isnan(x) || isinf(x))
        return NAN;
    int q;
    double r = __reduce(x, &q);
    switch (q) {
    case 0: return __cos_k(r);
    case 1: return -__sin_k(r);
    case 2: return -__cos_k(r);
    default: return __sin_k(r);
    }
}

double tan(double x) { return sin(x) / cos(x); }

double atan(double x) {
    if (isnan(x))
        return x;
    if (x < 0)
        return -atan(-x);
    if (x > 1)
        return M_PI_2 - atan(1 / x);
    if (x > 0.2679491924311227) /* tan(pi/12) */
        return M_PI / 6 + atan((x * 1.7320508075688772 - 1) / (x + 1.7320508075688772));
    double x2 = x * x, term = x, sum = 0;
    for (int i = 1; i < 40; i += 2) {
        sum += term / i;
        term *= -x2;
    }
    return sum;
}

double atan2(double y, double x) {
    if (isnan(x) || isnan(y))
        return NAN;
    if (x > 0)
        return atan(y / x);
    if (x < 0)
        return y >= 0 ? atan(y / x) + M_PI : atan(y / x) - M_PI;
    if (y > 0)
        return M_PI_2;
    if (y < 0)
        return -M_PI_2;
    return 0;
}

double asin(double x) {
    if (x < -1 || x > 1)
        return NAN;
    return atan2(x, sqrt(1 - x * x));
}

double acos(double x) {
    if (x < -1 || x > 1)
        return NAN;
    return atan2(sqrt(1 - x * x), x);
}

double sinh(double x) { return (exp(x) - exp(-x)) / 2; }
double cosh(double x) { return (exp(x) + exp(-x)) / 2; }

double tanh(double x) {
    if (x > 20)
        return 1;
    if (x < -20)
        return -1;
    double e = exp(2 * x);
    return (e - 1) / (e + 1);
}

double fmin(double a, double b) { return isnan(a) ? b : isnan(b) ? a : a < b ? a : b; }
double fmax(double a, double b) { return isnan(a) ? b : isnan(b) ? a : a > b ? a : b; }
float sqrtf(float x) { return (float)__builtin_sqrt(x); }
float fabsf(float x) { return (float)fabs(x); }
float floorf(float x) { return (float)floor(x); }
float ceilf(float x) { return (float)ceil(x); }
float roundf(float x) { return (float)round(x); }
float powf(float x, float y) { return (float)pow(x, y); }
float expf(float x) { return (float)exp(x); }
float logf(float x) { return (float)log(x); }
float sinf(float x) { return (float)sin(x); }
float cosf(float x) { return (float)cos(x); }
float tanf(float x) { return (float)tan(x); }
float atan2f(float y, float x) { return (float)atan2(y, x); }
float fmodf(float x, float y) { return (float)fmod(x, y); }

/* ---- stdio: files ------------------------------------------------------------- */

#define __F_READ 1
#define __F_WRITE 2
#define __F_EOF 4
#define __F_ERR 8
#define __F_LINEBUF 16
#define __F_NOBUF 32
#define __F_OWNBUF 64

struct _FILE {
    int fd;
    int flags;
    char *buf;
    int cap;
    int rpos, rlen; /* read buffer window */
    int wlen;       /* pending output */
    int ungot;
    pid_t pid;      /* popen child */
    struct _FILE *next;
};

static char __stdin_buf[BUFSIZ], __stdout_buf[BUFSIZ];
static struct _FILE __stdin = {0, __F_READ, __stdin_buf, BUFSIZ, 0, 0, 0, -1, 0, NULL};
static struct _FILE __stdout = {1, __F_WRITE | __F_LINEBUF, __stdout_buf, BUFSIZ, 0, 0, 0, -1, 0, NULL};
static struct _FILE __stderr = {2, __F_WRITE | __F_NOBUF, NULL, 0, 0, 0, 0, -1, 0, NULL};
FILE *stdin = &__stdin;
FILE *stdout = &__stdout;
FILE *stderr = &__stderr;
static FILE *__open_files;

static int __flush(FILE *f) {
    int off = 0;
    while (off < f->wlen) {
        long n = write(f->fd, f->buf + off, f->wlen - off);
        if (n <= 0) {
            if (n < 0 && errno == EINTR)
                continue;
            f->flags |= __F_ERR;
            f->wlen = 0;
            return EOF;
        }
        off += (int)n;
    }
    f->wlen = 0;
    return 0;
}

int fflush(FILE *f) {
    if (!f) {
        int r = fflush(stdout) | fflush(stderr);
        for (FILE *p = __open_files; p; p = p->next)
            r |= fflush(p);
        return r;
    }
    if (f->wlen)
        return __flush(f);
    if (f->rpos < f->rlen) {
        /* Give unread input back to the file position. */
        lseek(f->fd, f->rpos - f->rlen, SEEK_CUR);
        f->rpos = f->rlen = 0;
    }
    return 0;
}

static int __parse_mode(const char *mode, int *fl) {
    int acc;
    switch (mode[0]) {
    case 'r': acc = O_RDONLY; *fl = 0; break;
    case 'w': acc = O_WRONLY; *fl = O_CREAT | O_TRUNC; break;
    case 'a': acc = O_WRONLY; *fl = O_CREAT | O_APPEND; break;
    default: return -1;
    }
    if (strchr(mode, '+'))
        acc = O_RDWR;
    return acc;
}

static FILE *__new_file(int fd, int acc) {
    FILE *f = calloc(1, sizeof(FILE));
    if (!f)
        return NULL;
    f->fd = fd;
    f->ungot = -1;
    f->pid = -1;
    f->flags = (acc == O_RDONLY ? __F_READ : acc == O_WRONLY ? __F_WRITE : __F_READ | __F_WRITE);
    f->next = __open_files;
    __open_files = f;
    return f;
}

FILE *fopen(const char *path, const char *mode) {
    int fl;
    int acc = __parse_mode(mode, &fl);
    if (acc < 0) {
        errno = EINVAL;
        return NULL;
    }
    int fd = open(path, acc | fl, 0666);
    if (fd < 0)
        return NULL;
    FILE *f = __new_file(fd, acc);
    if (!f)
        close(fd);
    return f;
}

FILE *fdopen(int fd, const char *mode) {
    int fl;
    int acc = __parse_mode(mode, &fl);
    if (acc < 0) {
        errno = EINVAL;
        return NULL;
    }
    return __new_file(fd, acc);
}

int fclose(FILE *f) {
    int r = fflush(f);
    if (close(f->fd) < 0)
        r = EOF;
    if (f == stdin || f == stdout || f == stderr)
        return r;
    for (FILE **p = &__open_files; *p; p = &(*p)->next) {
        if (*p == f) {
            *p = f->next;
            break;
        }
    }
    if (f->flags & __F_OWNBUF)
        free(f->buf);
    free(f);
    return r;
}

FILE *freopen(const char *path, const char *mode, FILE *f) {
    int fl;
    int acc = __parse_mode(mode, &fl);
    fflush(f);
    int fd = open(path, acc | fl, 0666);
    if (fd < 0)
        return NULL;
    dup2(fd, f->fd);
    close(fd);
    f->flags = (f->flags & ~(__F_READ | __F_WRITE | __F_EOF | __F_ERR)) | (acc == O_RDONLY ? __F_READ : acc == O_WRONLY ? __F_WRITE : __F_READ | __F_WRITE);
    f->rpos = f->rlen = f->wlen = 0;
    f->ungot = -1;
    return f;
}

static int __ensure_buf(FILE *f) {
    if (f->buf || (f->flags & __F_NOBUF))
        return 0;
    f->buf = malloc(BUFSIZ);
    if (!f->buf) {
        f->flags |= __F_NOBUF;
        return 0;
    }
    f->cap = BUFSIZ;
    f->flags |= __F_OWNBUF;
    return 0;
}

int fputc(int c, FILE *f) {
    unsigned char ch = (unsigned char)c;
    if (f->rlen) {
        lseek(f->fd, f->rpos - f->rlen, SEEK_CUR);
        f->rpos = f->rlen = 0;
    }
    __ensure_buf(f);
    if (f->flags & __F_NOBUF) {
        if (write(f->fd, &ch, 1) != 1) {
            f->flags |= __F_ERR;
            return EOF;
        }
        return ch;
    }
    f->buf[f->wlen++] = (char)ch;
    if (f->wlen == f->cap || ((f->flags & __F_LINEBUF) && ch == '\n'))
        if (__flush(f) < 0)
            return EOF;
    return ch;
}

int putc(int c, FILE *f) { return fputc(c, f); }
int putchar(int c) { return fputc(c, stdout); }

size_t fwrite(const void *p, size_t size, size_t n, FILE *f) {
    size_t total = size * n;
    const char *s = p;
    if (!total)
        return 0;
    __ensure_buf(f);
    if ((f->flags & __F_NOBUF) || total >= (size_t)f->cap) {
        if (f->wlen && __flush(f) < 0)
            return 0;
        size_t off = 0;
        while (off < total) {
            long w = write(f->fd, s + off, total - off);
            if (w <= 0) {
                f->flags |= __F_ERR;
                return off / size;
            }
            off += w;
        }
        return n;
    }
    for (size_t i = 0; i < total; i++)
        if (fputc(s[i], f) == EOF)
            return i / size;
    return n;
}

int fputs(const char *s, FILE *f) { return fwrite(s, 1, strlen(s), f) == strlen(s) ? 0 : EOF; }

int puts(const char *s) {
    if (fputs(s, stdout) == EOF)
        return EOF;
    return fputc('\n', stdout) == EOF ? EOF : 0;
}

int fgetc(FILE *f) {
    if (f->ungot >= 0) {
        int c = f->ungot;
        f->ungot = -1;
        return c;
    }
    if (f->rpos >= f->rlen) {
        if (f == stdin)
            fflush(stdout);
        if (f->wlen)
            __flush(f);
        __ensure_buf(f);
        unsigned char ch;
        char *dst = f->buf ? f->buf : (char *)&ch;
        int cap = f->buf ? f->cap : 1;
        long n;
        do
            n = read(f->fd, dst, cap);
        while (n < 0 && errno == EINTR);
        if (n <= 0) {
            f->flags |= n == 0 ? __F_EOF : __F_ERR;
            return EOF;
        }
        if (!f->buf)
            return ch;
        f->rpos = 0;
        f->rlen = (int)n;
    }
    return (unsigned char)f->buf[f->rpos++];
}

int getc(FILE *f) { return fgetc(f); }
int getchar(void) { return fgetc(stdin); }

int ungetc(int c, FILE *f) {
    if (c == EOF)
        return EOF;
    f->ungot = (unsigned char)c;
    f->flags &= ~__F_EOF;
    return c;
}

size_t fread(void *p, size_t size, size_t n, FILE *f) {
    size_t total = size * n;
    char *d = p;
    size_t i = 0;
    while (i < total) {
        /* Large reads bypass the buffer. */
        if (f->ungot < 0 && f->rpos >= f->rlen && total - i >= (size_t)BUFSIZ) {
            long r = read(f->fd, d + i, total - i);
            if (r <= 0) {
                f->flags |= r == 0 ? __F_EOF : __F_ERR;
                break;
            }
            i += r;
            continue;
        }
        int c = fgetc(f);
        if (c == EOF)
            break;
        d[i++] = (char)c;
    }
    return size ? i / size : 0;
}

char *fgets(char *s, int n, FILE *f) {
    int i = 0;
    if (n <= 0)
        return NULL;
    while (i < n - 1) {
        int c = fgetc(f);
        if (c == EOF)
            break;
        s[i++] = (char)c;
        if (c == '\n')
            break;
    }
    if (i == 0)
        return NULL;
    s[i] = 0;
    return s;
}

long getdelim(char **line, size_t *cap, int delim, FILE *f) {
    size_t n = 0;
    if (!*line || !*cap) {
        *cap = 128;
        *line = malloc(*cap);
        if (!*line)
            return -1;
    }
    for (;;) {
        int c = fgetc(f);
        if (c == EOF)
            break;
        if (n + 2 > *cap) {
            char *nl = realloc(*line, *cap * 2);
            if (!nl)
                return -1;
            *line = nl;
            *cap *= 2;
        }
        (*line)[n++] = (char)c;
        if (c == delim)
            break;
    }
    (*line)[n] = 0;
    return n ? (long)n : -1;
}

long getline(char **line, size_t *cap, FILE *f) { return getdelim(line, cap, '\n', f); }

int fseek(FILE *f, long off, int whence) {
    fflush(f);
    if (whence == SEEK_CUR && f->ungot >= 0)
        off--;
    f->rpos = f->rlen = 0;
    f->ungot = -1;
    if (lseek(f->fd, off, whence) < 0)
        return -1;
    f->flags &= ~__F_EOF;
    return 0;
}

long ftell(FILE *f) {
    long pos = lseek(f->fd, 0, SEEK_CUR);
    if (pos < 0)
        return -1;
    return pos - (f->rlen - f->rpos) + f->wlen - (f->ungot >= 0);
}

void rewind(FILE *f) {
    fseek(f, 0, SEEK_SET);
    f->flags &= ~__F_ERR;
}

int fgetpos(FILE *f, fpos_t *pos) {
    *pos = ftell(f);
    return *pos < 0 ? -1 : 0;
}

int fsetpos(FILE *f, const fpos_t *pos) { return fseek(f, *pos, SEEK_SET); }
int feof(FILE *f) { return (f->flags & __F_EOF) != 0; }
int ferror(FILE *f) { return (f->flags & __F_ERR) != 0; }
void clearerr(FILE *f) { f->flags &= ~(__F_EOF | __F_ERR); }
int fileno(FILE *f) { return f->fd; }

int setvbuf(FILE *f, char *buf, int mode, size_t size) {
    fflush(f);
    f->flags &= ~(__F_LINEBUF | __F_NOBUF);
    if (mode == _IONBF) {
        f->flags |= __F_NOBUF;
        return 0;
    }
    if (mode == _IOLBF)
        f->flags |= __F_LINEBUF;
    if (buf && size) {
        if (f->flags & __F_OWNBUF)
            free(f->buf);
        f->flags &= ~__F_OWNBUF;
        f->buf = buf;
        f->cap = (int)size;
    }
    return 0;
}

void setbuf(FILE *f, char *buf) { setvbuf(f, buf, buf ? _IOFBF : _IONBF, BUFSIZ); }

void perror(const char *s) {
    if (s && *s)
        fprintf(stderr, "%s: %s\n", s, strerror(errno));
    else
        fprintf(stderr, "%s\n", strerror(errno));
}

int remove(const char *path) {
    if (unlink(path) == 0)
        return 0;
    if (errno == EISDIR)
        return rmdir(path);
    return -1;
}

FILE *tmpfile(void) {
    char name[] = "/tmp/tmpXXXXXX";
    int fd = mkstemp(name);
    if (fd < 0)
        return NULL;
    unlink(name);
    return fdopen(fd, "w+");
}

char *tmpnam(char *s) {
    static char buf[L_tmpnam];
    if (!s)
        s = buf;
    strcpy(s, "/tmp/tmpXXXXXX");
    return mktemp(s);
}

FILE *popen(const char *cmd, const char *mode) {
    int fds[2];
    int reading = mode[0] == 'r';
    if (pipe(fds) < 0)
        return NULL;
    fflush(NULL);
    pid_t pid = fork();
    if (pid < 0) {
        close(fds[0]);
        close(fds[1]);
        return NULL;
    }
    if (pid == 0) {
        if (reading) {
            dup2(fds[1], 1);
        } else {
            dup2(fds[0], 0);
        }
        close(fds[0]);
        close(fds[1]);
        char *argv[] = {"sh", "-c", (char *)cmd, NULL};
        execve("/bin/sh", argv, environ);
        _exit(127);
    }
    close(reading ? fds[1] : fds[0]);
    FILE *f = fdopen(reading ? fds[0] : fds[1], reading ? "r" : "w");
    if (f)
        f->pid = pid;
    return f;
}

int pclose(FILE *f) {
    pid_t pid = f->pid;
    fclose(f);
    int status;
    if (waitpid(pid, &status, 0) < 0)
        return -1;
    return status;
}

/* ---- formatted output ---------------------------------------------------- */

struct __out {
    FILE *f;
    char *buf;
    size_t cap;
    size_t len;
};

static void __put(struct __out *o, char c) {
    if (o->f)
        fputc(c, o->f);
    else if (o->len + 1 < o->cap)
        o->buf[o->len] = c;
    o->len++;
}

static void __puts(struct __out *o, const char *s, size_t n) {
    for (size_t i = 0; i < n; i++)
        __put(o, s[i]);
}

static void __pad(struct __out *o, char c, int n) {
    while (n-- > 0)
        __put(o, c);
}

/* Writes the decimal digits of a positive double v rounded to `prec`
   fractional digits into buf; returns the length. */
static int __fmt_fixed(char *buf, double v, int prec) {
    if (prec > 40)
        prec = 40;
    int n = 0;
    if (v >= 1e18) {
        /* Integral: print exactly as mantissa * 2^e with base-1e9 limbs. */
        int e;
        double m = frexp(v, &e);
        unsigned long mant = (unsigned long)ldexp(m, 53);
        e -= 53;
        unsigned limbs[40];
        int nl = 0;
        while (mant) {
            limbs[nl++] = (unsigned)(mant % 1000000000);
            mant /= 1000000000;
        }
        while (e-- > 0) {
            unsigned long carry = 0;
            for (int i = 0; i < nl; i++) {
                unsigned long t = (unsigned long)limbs[i] * 2 + carry;
                limbs[i] = (unsigned)(t % 1000000000);
                carry = t / 1000000000;
            }
            if (carry)
                limbs[nl++] = (unsigned)carry;
        }
        n = sprintf(buf, "%u", limbs[nl - 1]);
        for (int i = nl - 2; i >= 0; i--)
            n += sprintf(buf + n, "%09u", limbs[i]);
        if (prec > 0) {
            buf[n++] = '.';
            while (prec--)
                buf[n++] = '0';
        }
        return n;
    }
    unsigned long ip = (unsigned long)v;
    double frac = v - (double)ip;
    char fdig[48];
    int nf = 0;
    for (int i = 0; i < prec; i++) {
        frac *= 10;
        int d = (int)frac;
        if (d > 9)
            d = 9;
        fdig[nf++] = (char)('0' + d);
        frac -= d;
    }
    /* Round to nearest, ties to even (like glibc on exact halves). */
    int last = nf ? fdig[nf - 1] - '0' : (int)(ip % 10);
    if (frac > 0.5 || (frac == 0.5 && (last & 1))) {
        int i = nf - 1;
        while (i >= 0 && fdig[i] == '9')
            fdig[i--] = '0';
        if (i >= 0)
            fdig[i]++;
        else
            ip++;
    }
    char idig[24];
    int ni = 0;
    do {
        idig[ni++] = (char)('0' + ip % 10);
        ip /= 10;
    } while (ip);
    while (ni)
        buf[n++] = idig[--ni];
    if (prec > 0) {
        buf[n++] = '.';
        for (int i = 0; i < nf; i++)
            buf[n++] = fdig[i];
    }
    return n;
}

/* Exponent form: d.ddde+XX */
static int __fmt_exp(char *buf, double v, int prec, int upper) {
    int e = 0;
    if (v != 0) {
        while (v >= 10) {
            v /= 10;
            e++;
        }
        while (v < 1) {
            v *= 10;
            e--;
        }
    }
    char tmp[64];
    int n = __fmt_fixed(tmp, v, prec);
    if (tmp[0] == '1' && tmp[1] == '0') {
        /* Rounded up to 10.000: renormalize. */
        v /= 10;
        e++;
        n = __fmt_fixed(tmp, v, prec);
    }
    memcpy(buf, tmp, n);
    buf[n++] = upper ? 'E' : 'e';
    buf[n++] = e < 0 ? '-' : '+';
    if (e < 0)
        e = -e;
    if (e >= 100)
        buf[n++] = (char)('0' + e / 100);
    buf[n++] = (char)('0' + e / 10 % 10);
    buf[n++] = (char)('0' + e % 10);
    return n;
}

static int __strip_zeros(char *buf, int n) {
    char *dot = memchr(buf, '.', n);
    if (!dot)
        return n;
    char *e = memchr(buf, 'e', n);
    if (!e)
        e = memchr(buf, 'E', n);
    int end = e ? (int)(e - buf) : n;
    int k = end;
    while (k > dot - buf + 1 && buf[k - 1] == '0')
        k--;
    if (k == dot - buf + 1)
        k--;
    if (e) {
        memmove(buf + k, e, n - end);
        return k + (n - end);
    }
    return k;
}

static int __vformat(struct __out *o, const char *fmt, va_list ap) {
    for (; *fmt; fmt++) {
        if (*fmt != '%') {
            __put(o, *fmt);
            continue;
        }
        fmt++;
        int left = 0, plus = 0, space = 0, alt = 0, zero = 0;
        for (;; fmt++) {
            if (*fmt == '-')
                left = 1;
            else if (*fmt == '+')
                plus = 1;
            else if (*fmt == ' ')
                space = 1;
            else if (*fmt == '#')
                alt = 1;
            else if (*fmt == '0')
                zero = 1;
            else
                break;
        }
        int width = 0, prec = -1;
        if (*fmt == '*') {
            width = va_arg(ap, int);
            if (width < 0) {
                left = 1;
                width = -width;
            }
            fmt++;
        } else {
            while (isdigit(*fmt))
                width = width * 10 + (*fmt++ - '0');
        }
        if (*fmt == '.') {
            fmt++;
            prec = 0;
            if (*fmt == '*') {
                prec = va_arg(ap, int);
                fmt++;
            } else {
                while (isdigit(*fmt))
                    prec = prec * 10 + (*fmt++ - '0');
            }
        }
        int size = 0; /* -2 hh, -1 h, 0 int, 1 long */
        for (;; fmt++) {
            if (*fmt == 'l' || *fmt == 'z' || *fmt == 'j' || *fmt == 't' || *fmt == 'q' || *fmt == 'L')
                size = 1;
            else if (*fmt == 'h')
                size = size == -1 ? -2 : -1;
            else
                break;
        }
        char buf[512];
        int n = 0;
        const char *prefix = "";
        char conv = *fmt;
        switch (conv) {
        case 'd':
        case 'i': {
            long v = size == 1 ? va_arg(ap, long) : va_arg(ap, int);
            if (size == -1)
                v = (short)v;
            if (size == -2)
                v = (signed char)v;
            unsigned long u = v < 0 ? -(unsigned long)v : (unsigned long)v;
            prefix = v < 0 ? "-" : plus ? "+" : space ? " " : "";
            char tmp[24];
            int k = 0;
            while (u) {
                tmp[k++] = (char)('0' + u % 10);
                u /= 10;
            }
            while (k < prec)
                tmp[k++] = '0';
            if (k == 0 && prec != 0)
                tmp[k++] = '0';
            while (k)
                buf[n++] = tmp[--k];
            if (prec >= 0)
                zero = 0;
            break;
        }
        case 'u':
        case 'x':
        case 'X':
        case 'o':
        case 'b':
        case 'p': {
            unsigned long u;
            int base = conv == 'o' ? 8 : conv == 'u' ? 10 : conv == 'b' ? 2 : 16;
            if (conv == 'p') {
                u = (unsigned long)va_arg(ap, void *);
                if (!u) {
                    strcpy(buf, "(nil)");
                    n = 5;
                    break;
                }
                prefix = "0x";
            } else {
                u = size == 1 ? va_arg(ap, unsigned long) : va_arg(ap, unsigned int);
                if (size == -1)
                    u = (unsigned short)u;
                if (size == -2)
                    u = (unsigned char)u;
                if (alt && u && conv == 'x')
                    prefix = "0x";
                if (alt && u && conv == 'X')
                    prefix = "0X";
            }
            const char *digits = conv == 'X' ? "0123456789ABCDEF" : "0123456789abcdef";
            char tmp[72];
            int k = 0;
            while (u) {
                tmp[k++] = digits[u % base];
                u /= base;
            }
            while (k < prec)
                tmp[k++] = '0';
            if (k == 0 && prec != 0)
                tmp[k++] = '0';
            if (alt && conv == 'o' && tmp[k - 1] != '0')
                tmp[k++] = '0';
            while (k)
                buf[n++] = tmp[--k];
            if (prec >= 0)
                zero = 0;
            break;
        }
        case 'c':
            buf[n++] = (char)va_arg(ap, int);
            break;
        case 's': {
            const char *s = va_arg(ap, const char *);
            if (!s)
                s = "(null)";
            size_t len = prec >= 0 ? strnlen(s, prec) : strlen(s);
            if (!left)
                __pad(o, ' ', width - (int)len);
            __puts(o, s, len);
            if (left)
                __pad(o, ' ', width - (int)len);
            continue;
        }
        case 'f':
        case 'F':
        case 'e':
        case 'E':
        case 'g':
        case 'G':
        case 'a':
        case 'A': {
            double v = va_arg(ap, double);
            int upper = conv == 'F' || conv == 'E' || conv == 'G' || conv == 'A';
            if (__signbit(v)) {
                prefix = "-";
                v = -v;
            } else if (plus) {
                prefix = "+";
            } else if (space) {
                prefix = " ";
            }
            if (isnan(v) || isinf(v)) {
                strcpy(buf, isnan(v) ? (upper ? "NAN" : "nan") : (upper ? "INF" : "inf"));
                n = 3;
                zero = 0;
                break;
            }
            if (prec < 0)
                prec = 6;
            if (conv == 'f' || conv == 'F') {
                n = __fmt_fixed(buf, v, prec);
            } else if (conv == 'e' || conv == 'E' || conv == 'a' || conv == 'A') {
                n = __fmt_exp(buf, v, prec, upper);
            } else {
                int p = prec == 0 ? 1 : prec;
                n = __fmt_exp(buf, v, p - 1, upper);
                buf[n] = 0;
                char *ep = buf + n - 1;
                while (*ep != 'e' && *ep != 'E')
                    ep--;
                int x = atoi(ep + 1);
                if (x < p && x >= -4)
                    n = __fmt_fixed(buf, v, p - 1 - x);
                if (!alt)
                    n = __strip_zeros(buf, n);
            }
            if (alt && !memchr(buf, '.', n))
                buf[n++] = '.';
            break;
        }
        case 'n':
            *va_arg(ap, int *) = (int)o->len;
            continue;
        case '%':
            __put(o, '%');
            continue;
        case 0:
            fmt--;
            continue;
        default:
            __put(o, '%');
            __put(o, conv);
            continue;
        }
        int plen = (int)strlen(prefix);
        int padn = width - n - plen;
        if (!left && !zero)
            __pad(o, ' ', padn);
        __puts(o, prefix, plen);
        if (!left && zero)
            __pad(o, '0', padn);
        __puts(o, buf, n);
        if (left)
            __pad(o, ' ', padn);
    }
    return (int)o->len;
}

int vfprintf(FILE *f, const char *fmt, va_list ap) {
    struct __out o = {f, NULL, 0, 0};
    return __vformat(&o, fmt, ap);
}

int vsnprintf(char *buf, size_t n, const char *fmt, va_list ap) {
    struct __out o = {NULL, buf, n, 0};
    int r = __vformat(&o, fmt, ap);
    if (n)
        buf[o.len < n ? o.len : n - 1] = 0;
    return r;
}

int vsprintf(char *buf, const char *fmt, va_list ap) { return vsnprintf(buf, (size_t)INT_MAX, fmt, ap); }
int vprintf(const char *fmt, va_list ap) { return vfprintf(stdout, fmt, ap); }

int vdprintf(int fd, const char *fmt, va_list ap) {
    char buf[1024];
    int n = vsnprintf(buf, sizeof buf, fmt, ap);
    write(fd, buf, n < (int)sizeof buf ? n : (int)sizeof buf - 1);
    return n;
}

int printf(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    return vfprintf(stdout, fmt, ap);
}

int fprintf(FILE *f, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    return vfprintf(f, fmt, ap);
}

int sprintf(char *buf, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    return vsprintf(buf, fmt, ap);
}

int snprintf(char *buf, size_t n, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    return vsnprintf(buf, n, fmt, ap);
}

int dprintf(int fd, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    return vdprintf(fd, fmt, ap);
}

/* ---- formatted input ------------------------------------------------------ */

struct __in {
    FILE *f;
    const char *s;
    int count;
};

static int __get(struct __in *in) {
    int c;
    if (in->f)
        c = fgetc(in->f);
    else
        c = *in->s ? (unsigned char)*in->s++ : EOF;
    if (c != EOF)
        in->count++;
    return c;
}

static void __unget(struct __in *in, int c) {
    if (c == EOF)
        return;
    in->count--;
    if (in->f)
        ungetc(c, in->f);
    else
        in->s--;
}

static int __vscan(struct __in *in, const char *fmt, va_list ap) {
    int assigned = 0;
    int c;
    for (; *fmt; fmt++) {
        if (isspace(*fmt)) {
            while (isspace(c = __get(in)))
                ;
            __unget(in, c);
            continue;
        }
        if (*fmt != '%' || fmt[1] == '%') {
            if (*fmt == '%')
                fmt++;
            c = __get(in);
            if (c != *fmt) {
                __unget(in, c);
                return assigned;
            }
            continue;
        }
        fmt++;
        int skip = 0, width = 0, size = 0;
        if (*fmt == '*') {
            skip = 1;
            fmt++;
        }
        while (isdigit(*fmt))
            width = width * 10 + (*fmt++ - '0');
        for (;; fmt++) {
            if (*fmt == 'l' || *fmt == 'z' || *fmt == 'j' || *fmt == 'L')
                size++;
            else if (*fmt == 'h')
                size--;
            else
                break;
        }
        if (width == 0)
            width = INT_MAX;
        char conv = *fmt;
        if (conv == 'n') {
            if (!skip)
                *va_arg(ap, int *) = in->count;
            continue;
        }
        if (conv != 'c' && conv != '[') {
            while (isspace(c = __get(in)))
                ;
            __unget(in, c);
        }
        char buf[512];
        int n = 0;
        switch (conv) {
        case 'd':
        case 'i':
        case 'u':
        case 'x':
        case 'X':
        case 'o':
        case 'p': {
            int base = conv == 'x' || conv == 'X' || conv == 'p' ? 16 : conv == 'o' ? 8 : conv == 'i' ? 0 : 10;
            c = __get(in);
            if ((c == '-' || c == '+') && n < width) {
                buf[n++] = (char)c;
                c = __get(in);
            }
            while (c != EOF && n < width && n < 510 && (isxdigit(c) || ((c == 'x' || c == 'X') && (base == 16 || base == 0)))) {
                if (base == 10 && !isdigit(c))
                    break;
                if (base == 8 && (c < '0' || c > '7'))
                    break;
                buf[n++] = (char)c;
                c = __get(in);
            }
            __unget(in, c);
            buf[n] = 0;
            if (n == 0 || (n == 1 && (buf[0] == '-' || buf[0] == '+')))
                return assigned ? assigned : (c == EOF ? EOF : 0);
            if (skip)
                break;
            if (conv == 'p') {
                *va_arg(ap, void **) = (void *)strtoul(buf, NULL, 16);
            } else if (conv == 'd' || conv == 'i') {
                long v = strtol(buf, NULL, base);
                if (size >= 1)
                    *va_arg(ap, long *) = v;
                else if (size == -1)
                    *va_arg(ap, short *) = (short)v;
                else if (size <= -2)
                    *va_arg(ap, char *) = (char)v;
                else
                    *va_arg(ap, int *) = (int)v;
            } else {
                unsigned long v = strtoul(buf, NULL, base);
                if (size >= 1)
                    *va_arg(ap, unsigned long *) = v;
                else if (size == -1)
                    *va_arg(ap, unsigned short *) = (unsigned short)v;
                else if (size <= -2)
                    *va_arg(ap, unsigned char *) = (unsigned char)v;
                else
                    *va_arg(ap, unsigned *) = (unsigned)v;
            }
            assigned++;
            break;
        }
        case 'f':
        case 'e':
        case 'g':
        case 'E':
        case 'G':
        case 'a': {
            c = __get(in);
            while (c != EOF && n < width && n < 510 && (isdigit(c) || c == '.' || c == 'e' || c == 'E' || c == '-' || c == '+' || c == 'i' || c == 'n' || c == 'f' || c == 'a' || c == 'I' || c == 'N' || c == 'F' || c == 'A')) {
                if ((c == '-' || c == '+') && n > 0 && buf[n - 1] != 'e' && buf[n - 1] != 'E')
                    break;
                buf[n++] = (char)c;
                c = __get(in);
            }
            __unget(in, c);
            buf[n] = 0;
            char *end;
            double v = strtod(buf, &end);
            if (end == buf)
                return assigned ? assigned : (c == EOF ? EOF : 0);
            if (skip)
                break;
            if (size >= 1)
                *va_arg(ap, double *) = v;
            else
                *va_arg(ap, float *) = (float)v;
            assigned++;
            break;
        }
        case 's': {
            char *d = skip ? NULL : va_arg(ap, char *);
            c = __get(in);
            if (c == EOF)
                return assigned ? assigned : EOF;
            while (c != EOF && !isspace(c) && n < width) {
                if (d)
                    d[n] = (char)c;
                n++;
                c = __get(in);
            }
            __unget(in, c);
            if (d) {
                d[n] = 0;
                assigned++;
            }
            break;
        }
        case 'c': {
            char *d = skip ? NULL : va_arg(ap, char *);
            if (width == INT_MAX)
                width = 1;
            for (int i = 0; i < width; i++) {
                c = __get(in);
                if (c == EOF)
                    return assigned ? assigned : EOF;
                if (d)
                    d[i] = (char)c;
            }
            if (d)
                assigned++;
            break;
        }
        case '[': {
            fmt++;
            int negate = 0;
            if (*fmt == '^') {
                negate = 1;
                fmt++;
            }
            char set[256];
            memset(set, 0, sizeof set);
            if (*fmt == ']')
                set[(unsigned char)*fmt++] = 1;
            for (; *fmt && *fmt != ']'; fmt++) {
                if (fmt[1] == '-' && fmt[2] && fmt[2] != ']') {
                    for (int ch = (unsigned char)fmt[0]; ch <= (unsigned char)fmt[2]; ch++)
                        set[ch] = 1;
                    fmt += 2;
                } else {
                    set[(unsigned char)*fmt] = 1;
                }
            }
            char *d = skip ? NULL : va_arg(ap, char *);
            c = __get(in);
            while (c != EOF && n < width && set[c] != negate) {
                if (d)
                    d[n] = (char)c;
                n++;
                c = __get(in);
            }
            __unget(in, c);
            if (n == 0)
                return assigned ? assigned : (c == EOF ? EOF : 0);
            if (d) {
                d[n] = 0;
                assigned++;
            }
            break;
        }
        default:
            return assigned;
        }
    }
    return assigned;
}

int vsscanf(const char *s, const char *fmt, va_list ap) {
    struct __in in = {NULL, s, 0};
    return __vscan(&in, fmt, ap);
}

int vfscanf(FILE *f, const char *fmt, va_list ap) {
    struct __in in = {f, NULL, 0};
    return __vscan(&in, fmt, ap);
}

int vscanf(const char *fmt, va_list ap) { return vfscanf(stdin, fmt, ap); }

int sscanf(const char *s, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    return vsscanf(s, fmt, ap);
}

int fscanf(FILE *f, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    return vfscanf(f, fmt, ap);
}

int scanf(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    return vfscanf(stdin, fmt, ap);
}

/* ---- sockets -------------------------------------------------------------- */

#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <netdb.h>
#include <poll.h>

int poll(struct pollfd *fds, nfds_t n, int timeout) { return __ret(__syscall(7, fds, n, timeout)); }
int socket(int domain, int type, int protocol) { return __ret(__syscall(41, domain, type, protocol)); }
int connect(int fd, const struct sockaddr *a, socklen_t len) { return __ret(__syscall(42, fd, a, len)); }
int accept(int fd, struct sockaddr *a, socklen_t *len) { return __ret(__syscall(43, fd, a, len)); }
int accept4(int fd, struct sockaddr *a, socklen_t *len, int flags) { return __ret(__syscall(288, fd, a, len, flags)); }
ssize_t sendto(int fd, const void *buf, size_t n, int flags, const struct sockaddr *to, socklen_t len) { return __ret(__syscall(44, fd, buf, n, flags, to, len)); }
ssize_t recvfrom(int fd, void *buf, size_t n, int flags, struct sockaddr *from, socklen_t *len) { return __ret(__syscall(45, fd, buf, n, flags, from, len)); }
ssize_t send(int fd, const void *buf, size_t n, int flags) { return sendto(fd, buf, n, flags, NULL, 0); }
ssize_t recv(int fd, void *buf, size_t n, int flags) { return recvfrom(fd, buf, n, flags, NULL, NULL); }
ssize_t sendmsg(int fd, const struct msghdr *m, int flags) { return __ret(__syscall(46, fd, m, flags)); }
ssize_t recvmsg(int fd, struct msghdr *m, int flags) { return __ret(__syscall(47, fd, m, flags)); }
int shutdown(int fd, int how) { return __ret(__syscall(48, fd, how)); }
int bind(int fd, const struct sockaddr *a, socklen_t len) { return __ret(__syscall(49, fd, a, len)); }
int listen(int fd, int backlog) { return __ret(__syscall(50, fd, backlog)); }
int getsockname(int fd, struct sockaddr *a, socklen_t *len) { return __ret(__syscall(51, fd, a, len)); }
int getpeername(int fd, struct sockaddr *a, socklen_t *len) { return __ret(__syscall(52, fd, a, len)); }
int setsockopt(int fd, int level, int name, const void *v, socklen_t len) { return __ret(__syscall(54, fd, level, name, v, len)); }
int getsockopt(int fd, int level, int name, void *v, socklen_t *len) { return __ret(__syscall(55, fd, level, name, v, len)); }

uint16_t htons(uint16_t x) { return (uint16_t)((x << 8) | (x >> 8)); }
uint16_t ntohs(uint16_t x) { return htons(x); }
uint32_t htonl(uint32_t x) { return (x >> 24) | ((x >> 8) & 0xff00) | ((x << 8) & 0xff0000) | (x << 24); }
uint32_t ntohl(uint32_t x) { return htonl(x); }

int inet_aton(const char *s, struct in_addr *a) {
    unsigned long parts[4];
    int n = 0;
    while (n < 4) {
        char *end;
        if (!isdigit(*s))
            return 0;
        parts[n++] = strtoul(s, &end, 10);
        if (parts[n - 1] > 255)
            return 0;
        s = end;
        if (*s != '.')
            break;
        s++;
    }
    if (*s || n != 4)
        return 0;
    a->s_addr = htonl((uint32_t)(parts[0] << 24 | parts[1] << 16 | parts[2] << 8 | parts[3]));
    return 1;
}

in_addr_t inet_addr(const char *s) {
    struct in_addr a;
    return inet_aton(s, &a) ? a.s_addr : INADDR_NONE;
}

char *inet_ntoa(struct in_addr a) {
    static char buf[16];
    unsigned char *b = (unsigned char *)&a.s_addr;
    snprintf(buf, sizeof buf, "%d.%d.%d.%d", b[0], b[1], b[2], b[3]);
    return buf;
}

int inet_pton(int af, const char *s, void *dst) {
    if (af != AF_INET) {
        errno = EAFNOSUPPORT;
        return -1;
    }
    return inet_aton(s, dst);
}

const char *inet_ntop(int af, const void *src, char *dst, socklen_t n) {
    if (af != AF_INET) {
        errno = EAFNOSUPPORT;
        return NULL;
    }
    const unsigned char *b = src;
    if (snprintf(dst, n, "%d.%d.%d.%d", b[0], b[1], b[2], b[3]) >= (int)n) {
        errno = ENOSPC;
        return NULL;
    }
    return dst;
}

int h_errno;

/* /etc/hosts, then DNS servers from /etc/resolv.conf or /proc/net/dns. */
static int __hosts_lookup(const char *name, struct in_addr *out) {
    FILE *f = fopen("/etc/hosts", "r");
    char line[256];
    if (!f)
        return 0;
    while (fgets(line, sizeof line, f)) {
        char *save, *addr = strtok_r(line, " \t\n", &save);
        if (!addr || addr[0] == '#')
            continue;
        for (char *n = strtok_r(NULL, " \t\n", &save); n && n[0] != '#'; n = strtok_r(NULL, " \t\n", &save)) {
            if (strcasecmp(n, name) == 0 && inet_aton(addr, out)) {
                fclose(f);
                return 1;
            }
        }
    }
    fclose(f);
    return 0;
}

static int __nameserver(struct in_addr *out) {
    const char *files[] = {"/etc/resolv.conf", "/proc/net/dns"};
    char line[256];
    for (int i = 0; i < 2; i++) {
        FILE *f = fopen(files[i], "r");
        if (!f)
            continue;
        while (fgets(line, sizeof line, f)) {
            char *save, *key = strtok_r(line, " \t\n", &save);
            char *val = key ? strtok_r(NULL, " \t\n", &save) : NULL;
            if (key && val && strcmp(key, "nameserver") == 0 && inet_aton(val, out)) {
                fclose(f);
                return 1;
            }
        }
        fclose(f);
    }
    return 0;
}

static int __dns_lookup(const char *name, struct in_addr *out) {
    struct in_addr server;
    if (!__nameserver(&server))
        return EAI_FAIL;
    unsigned char q[512], r[1500];
    unsigned short id = (unsigned short)(getpid() * 31 + time(NULL));
    int n = 12;
    memset(q, 0, 12);
    q[0] = id >> 8;
    q[1] = id & 0xff;
    q[2] = 1;
    q[5] = 1;
    const char *p = name;
    while (*p) {
        size_t len = strcspn(p, ".");
        if (len == 0 || len > 63 || n + len + 6 > sizeof q)
            return EAI_NONAME;
        q[n++] = (unsigned char)len;
        memcpy(q + n, p, len);
        n += len;
        p += len;
        if (*p == '.')
            p++;
    }
    q[n++] = 0;
    q[n++] = 0;
    q[n++] = 1;
    q[n++] = 0;
    q[n++] = 1;
    int s = socket(AF_INET, SOCK_DGRAM, 0);
    if (s < 0)
        return EAI_SYSTEM;
    struct timeval tv = {2, 0};
    setsockopt(s, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv);
    struct sockaddr_in sa = {0};
    sa.sin_family = AF_INET;
    sa.sin_port = htons(53);
    sa.sin_addr = server;
    int result = EAI_AGAIN;
    for (int attempt = 0; attempt < 3 && result == EAI_AGAIN; attempt++) {
        sendto(s, q, n, 0, (struct sockaddr *)&sa, sizeof sa);
        ssize_t got = recv(s, r, sizeof r, 0);
        if (got < 12 || r[0] != q[0] || r[1] != q[1])
            continue;
        if ((r[3] & 15) == 3) {
            result = EAI_NONAME;
            break;
        }
        int an = r[6] << 8 | r[7];
        int i = n; /* skip our question, echoed back */
        result = EAI_NONAME;
        for (int k = 0; k < an && i + 12 <= got; k++) {
            while (i < got && r[i] && (r[i] & 0xC0) != 0xC0)
                i += r[i] + 1;
            i += (i < got && r[i]) ? 2 : 1;
            int type = r[i] << 8 | r[i + 1];
            int len = r[i + 8] << 8 | r[i + 9];
            if (type == 1 && len == 4 && i + 14 <= got) {
                memcpy(&out->s_addr, r + i + 10, 4);
                result = 0;
                break;
            }
            i += 10 + len;
        }
    }
    close(s);
    return result;
}

static int __resolve(const char *name, struct in_addr *out) {
    if (inet_aton(name, out))
        return 0;
    if (__hosts_lookup(name, out))
        return 0;
    return __dns_lookup(name, out);
}

struct hostent *gethostbyname(const char *name) {
    static struct hostent h;
    static struct in_addr addr;
    static char *list[2];
    static char hname[256];
    int r = __resolve(name, &addr);
    if (r != 0) {
        h_errno = r == EAI_AGAIN ? TRY_AGAIN : HOST_NOT_FOUND;
        return NULL;
    }
    strncpy(hname, name, sizeof hname - 1);
    list[0] = (char *)&addr;
    list[1] = NULL;
    h.h_name = hname;
    h.h_aliases = list + 1;
    h.h_addrtype = AF_INET;
    h.h_length = 4;
    h.h_addr_list = list;
    return &h;
}

int getaddrinfo(const char *node, const char *service, const struct addrinfo *hints, struct addrinfo **res) {
    struct in_addr addr;
    int socktype = hints ? hints->ai_socktype : 0;
    if (hints && hints->ai_family != AF_UNSPEC && hints->ai_family != AF_INET)
        return EAI_FAMILY;
    if (node) {
        int r = (hints && (hints->ai_flags & AI_NUMERICHOST)) ? (inet_aton(node, &addr) ? 0 : EAI_NONAME) : __resolve(node, &addr);
        if (r)
            return r;
    } else {
        addr.s_addr = (hints && (hints->ai_flags & AI_PASSIVE)) ? htonl(INADDR_ANY) : htonl(INADDR_LOOPBACK);
    }
    int port = 0;
    if (service) {
        char *end;
        port = (int)strtol(service, &end, 10);
        if (*end) {
            if (strcmp(service, "http") == 0)
                port = 80;
            else if (strcmp(service, "https") == 0)
                port = 443;
            else
                return EAI_SERVICE;
        }
    }
    struct addrinfo *ai = calloc(1, sizeof(struct addrinfo) + sizeof(struct sockaddr_in));
    if (!ai)
        return EAI_MEMORY;
    struct sockaddr_in *sa = (struct sockaddr_in *)(ai + 1);
    sa->sin_family = AF_INET;
    sa->sin_port = htons((uint16_t)port);
    sa->sin_addr = addr;
    ai->ai_family = AF_INET;
    ai->ai_socktype = socktype ? socktype : SOCK_STREAM;
    ai->ai_protocol = hints ? hints->ai_protocol : 0;
    ai->ai_addrlen = sizeof(struct sockaddr_in);
    ai->ai_addr = (struct sockaddr *)sa;
    *res = ai;
    return 0;
}

void freeaddrinfo(struct addrinfo *res) {
    while (res) {
        struct addrinfo *next = res->ai_next;
        free(res);
        res = next;
    }
}

const char *gai_strerror(int err) {
    switch (err) {
    case EAI_NONAME: return "Name or service not known";
    case EAI_AGAIN: return "Temporary failure in name resolution";
    case EAI_FAIL: return "Non-recoverable failure in name resolution";
    case EAI_FAMILY: return "Address family not supported";
    case EAI_SERVICE: return "Servname not supported";
    case EAI_MEMORY: return "Memory allocation failure";
    }
    return "Unknown error";
}

/* ---- program entry -------------------------------------------------------- */

int main();

void __libc_start(long *sp) {
    int argc = (int)sp[0];
    char **argv = (char **)(sp + 1);
    environ = argv + argc + 1;
    clock_gettime(CLOCK_MONOTONIC, &__clock_start);
    exit(main(argc, argv, environ));
}
