#ifndef _UNISTD_H
#define _UNISTD_H
#include <sys/types.h>
#define STDIN_FILENO 0
#define STDOUT_FILENO 1
#define STDERR_FILENO 2
#define F_OK 0
#define X_OK 1
#define W_OK 2
#define R_OK 4
#ifndef SEEK_SET
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
#endif
extern char **environ;
extern char *optarg;
extern int optind, opterr, optopt;
ssize_t read(int fd, void *buf, size_t n);
ssize_t write(int fd, const void *buf, size_t n);
ssize_t pread(int fd, void *buf, size_t n, off_t off);
ssize_t pwrite(int fd, const void *buf, size_t n, off_t off);
int close(int fd);
off_t lseek(int fd, off_t off, int whence);
int dup(int fd);
int dup2(int fd, int fd2);
int pipe(int fds[2]);
int unlink(const char *path);
int rmdir(const char *path);
int link(const char *from, const char *to);
int symlink(const char *from, const char *to);
ssize_t readlink(const char *path, char *buf, size_t n);
int chdir(const char *path);
int fchdir(int fd);
char *getcwd(char *buf, size_t n);
int access(const char *path, int mode);
int truncate(const char *path, off_t len);
int ftruncate(int fd, off_t len);
int fsync(int fd);
void sync(void);
pid_t getpid(void);
pid_t getppid(void);
pid_t getpgrp(void);
int setpgid(pid_t pid, pid_t pgid);
pid_t setsid(void);
uid_t getuid(void);
uid_t geteuid(void);
gid_t getgid(void);
gid_t getegid(void);
int setuid(uid_t uid);
int setgid(gid_t gid);
pid_t fork(void);
pid_t vfork(void);
int execve(const char *path, char *const argv[], char *const envp[]);
int execv(const char *path, char *const argv[]);
int execvp(const char *file, char *const argv[]);
int execl(const char *path, const char *arg, ...);
int execlp(const char *file, const char *arg, ...);
void _exit(int status);
unsigned sleep(unsigned s);
int usleep(useconds_t us);
unsigned alarm(unsigned s);
int pause(void);
int isatty(int fd);
char *ttyname(int fd);
int gethostname(char *name, size_t n);
int getopt(int argc, char *const argv[], const char *opts);
long sysconf(int name);
void *sbrk(long inc);
int brk(void *addr);
#define _SC_PAGESIZE 30
#define _SC_PAGE_SIZE 30
#define _SC_CLK_TCK 2
#define _SC_NPROCESSORS_ONLN 84
#endif
