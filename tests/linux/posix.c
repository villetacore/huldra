/* POSIX/libc API coverage test, built with the host gcc -static (glibc). */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/random.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/utsname.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static int failures = 0;
#define CHECK(name, cond)                                                     \
    do {                                                                      \
        if (cond) {                                                           \
            printf("ok   %s\n", name);                                        \
        } else {                                                              \
            printf("FAIL %s (errno %d: %s)\n", name, errno, strerror(errno)); \
            failures++;                                                       \
        }                                                                     \
    } while (0)

static volatile sig_atomic_t got_signal = 0;
static void on_usr1(int sig) { got_signal = sig; }

static int cmp_int(const void *a, const void *b) { return *(const int *)a - *(const int *)b; }

int main(void) {
    struct utsname u;
    CHECK("uname", uname(&u) == 0 && strcmp(u.sysname, "Huldra") == 0);

    /* stdio on files */
    FILE *f = fopen("/tmp/posix-test.txt", "w");
    CHECK("fopen(w)", f != NULL);
    for (int i = 0; i < 100; i++) fprintf(f, "line %d\n", i);
    CHECK("fclose", fclose(f) == 0);
    f = fopen("/tmp/posix-test.txt", "r");
    char line[64];
    int count = 0;
    while (fgets(line, sizeof line, f)) count++;
    fclose(f);
    CHECK("fgets 100 lines", count == 100);

    struct stat st;
    CHECK("stat size", stat("/tmp/posix-test.txt", &st) == 0 && st.st_size == 790 && S_ISREG(st.st_mode));
    CHECK("rename", rename("/tmp/posix-test.txt", "/tmp/posix-moved.txt") == 0);
    CHECK("access", access("/tmp/posix-moved.txt", F_OK) == 0);
    CHECK("unlink", unlink("/tmp/posix-moved.txt") == 0 && access("/tmp/posix-moved.txt", F_OK) != 0);

    /* directories */
    CHECK("mkdir", mkdir("/tmp/posix-dir", 0755) == 0);
    close(open("/tmp/posix-dir/a", O_CREAT | O_WRONLY, 0644));
    close(open("/tmp/posix-dir/b", O_CREAT | O_WRONLY, 0644));
    DIR *d = opendir("/tmp/posix-dir");
    int entries = 0;
    struct dirent *e;
    while (d && (e = readdir(d))) entries++;
    if (d) closedir(d);
    CHECK("opendir/readdir", entries == 4);
    unlink("/tmp/posix-dir/a");
    unlink("/tmp/posix-dir/b");
    CHECK("rmdir", rmdir("/tmp/posix-dir") == 0);

    char cwd[256];
    CHECK("chdir/getcwd", chdir("/etc") == 0 && getcwd(cwd, sizeof cwd) && strcmp(cwd, "/etc") == 0);

    /* processes */
    pid_t pid = fork();
    if (pid == 0) _exit(42);
    int status = 0;
    CHECK("fork/waitpid", pid > 0 && waitpid(pid, &status, 0) == pid && WIFEXITED(status) && WEXITSTATUS(status) == 42);

    int fds[2];
    CHECK("pipe", pipe(fds) == 0);
    pid = fork();
    if (pid == 0) {
        close(fds[0]);
        dup2(fds[1], 1);
        execl("/bin/echo", "echo", "from exec", (char *)NULL);
        _exit(127);
    }
    close(fds[1]);
    char buf[64] = {0};
    ssize_t n = read(fds[0], buf, sizeof buf - 1);
    close(fds[0]);
    waitpid(pid, &status, 0);
    CHECK("fork+exec+pipe", n > 0 && strcmp(buf, "from exec\n") == 0);

    CHECK("system()", system("exit 3") >= 0 && WEXITSTATUS(system("exit 3")) == 3);
    FILE *p = popen("echo popen works", "r");
    char pbuf[64] = {0};
    if (p) {
        fgets(pbuf, sizeof pbuf, p);
        pclose(p);
    }
    CHECK("popen", strcmp(pbuf, "popen works\n") == 0);

    /* signals */
    struct sigaction sa = {0};
    sa.sa_handler = on_usr1;
    sigaction(SIGUSR1, &sa, NULL);
    raise(SIGUSR1);
    CHECK("sigaction/raise", got_signal == SIGUSR1);

    pid = fork();
    if (pid == 0) {
        pause();
        _exit(0);
    }
    usleep(20000);
    kill(pid, SIGTERM);
    waitpid(pid, &status, 0);
    CHECK("kill/WIFSIGNALED", WIFSIGNALED(status) && WTERMSIG(status) == SIGTERM);

    /* time */
    struct timespec t0, t1;
    clock_gettime(CLOCK_MONOTONIC, &t0);
    usleep(50000);
    clock_gettime(CLOCK_MONOTONIC, &t1);
    long ms = (t1.tv_sec - t0.tv_sec) * 1000 + (t1.tv_nsec - t0.tv_nsec) / 1000000;
    CHECK("usleep/clock_gettime", ms >= 40 && ms < 1000);
    time_t now = time(NULL);
    struct tm tm;
    gmtime_r(&now, &tm);
    CHECK("time/gmtime", now > 1600000000 && tm.tm_year > 120);
    struct timeval tv;
    CHECK("gettimeofday", gettimeofday(&tv, NULL) == 0 && tv.tv_sec > 1600000000);

    /* memory and misc */
    int nums[1000];
    for (int i = 0; i < 1000; i++) nums[i] = (i * 7919) % 1000;
    qsort(nums, 1000, sizeof(int), cmp_int);
    CHECK("qsort", nums[0] == 0 && nums[999] == 999);
    char *big = malloc(32 << 20);
    memset(big, 1, 32 << 20);
    CHECK("malloc 32 MiB", big && big[(32 << 20) - 1] == 1);
    big = realloc(big, 64 << 20);
    CHECK("realloc 64 MiB", big && big[(32 << 20) - 1] == 1);
    free(big);
    unsigned char rnd[16];
    CHECK("getrandom", getrandom(rnd, sizeof rnd, 0) == sizeof rnd);
    CHECK("getpid/getppid", getpid() > 1 && getppid() >= 1);
    CHECK("isatty", isatty(1) == 1);
    char *s = strdup("snprintf");
    char out[32];
    snprintf(out, sizeof out, "%s %d %.2f", s, 42, 3.14159);
    CHECK("snprintf", strcmp(out, "snprintf 42 3.14") == 0);
    free(s);

    printf("posix: %d failed\n", failures);
    return failures != 0;
}
