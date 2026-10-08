/* The C library; the output must match glibc's. */
#include <ctype.h>
#include <errno.h>
#include <math.h>
#include <setjmp.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <sys/stat.h>
#include <sys/wait.h>

static int cmp_int(const void *a, const void *b) { return *(const int *)a - *(const int *)b; }
static int cmp_str(const void *a, const void *b) { return strcmp(*(char *const *)a, *(char *const *)b); }

static jmp_buf jb;
static void deep(int n) {
    if (n == 0)
        longjmp(jb, 42);
    deep(n - 1);
}

static volatile int got_signal;
static void on_signal(int sig) { got_signal = sig; }

int main(void) {
    /* printf */
    printf("[%5d] [%-5d] [%05d] [%+d] [% d] [%x] [%#X] [%o] [%#o]\n", 42, 42, 42, 42, 42, 255, 255, 8, 8);
    printf("[%s] [%10s] [%-10s] [%.3s] [%c] [%%]\n", "abc", "right", "left", "truncate", 'z');
    printf("[%ld] [%lu] [%lx] [%hd] [%hhu] [%zu]\n", -1234567890123L, 18446744073709551615UL, 0xdeadbeefUL, (short)-5, (unsigned char)300, (size_t)77);
    printf("[%f] [%.2f] [%10.3f] [%-10.1f] [%e] [%.3E] [%g] [%g] [%g] [%G]\n", 3.14159, 2.675, -1.5, 9.99, 12345.678, 0.000123, 100000.0, 1000000.0, 0.5, 1e-10);
    printf("[%.0f] [%.0f] [%#.0f] [%f] [%g] [%5.1f%%]\n", 0.5, 1.5, 3.0, -0.0, 123.456, 99.5);
    printf("[%*d] [%-*d] [%.*f]\n", 6, 7, 4, 8, 2, 1.23456);
    char buf[64];
    int n = snprintf(buf, 8, "%s", "overflowing");
    printf("snprintf %d [%s]\n", n, buf);
    sprintf(buf, "%d-%s-%c", 1, "two", '3');
    printf("sprintf [%s]\n", buf);

    /* strings */
    char s[64] = "Hello";
    strcat(s, ", World");
    printf("%s %zu %d %d %d\n", s, strlen(s), strcmp("abc", "abd") < 0, strncmp("abcd", "abce", 3), strcasecmp("HeLLo", "hello"));
    printf("%s %s %s\n", strchr(s, 'o'), strrchr(s, 'o'), strstr(s, "Wor"));
    printf("%zu %zu %s\n", strspn("aabbcc", "ab"), strcspn("hello", "lo"), strpbrk("hello", "xyl"));
    char tok[] = "a,b;;c,d";
    for (char *t = strtok(tok, ",;"); t; t = strtok(NULL, ",;"))
        printf("tok(%s) ", t);
    printf("\n");
    char *dup = strdup("duplicate");
    memmove(dup + 2, dup, 4);
    printf("%s %d\n", dup, memcmp("abc", "abd", 3) < 0);
    free(dup);
    memset(buf, 'x', 5);
    buf[5] = 0;
    printf("%s %s\n", buf, strerror(ENOENT));
    printf("%d %d %d %d %c %c\n", isalpha('a') != 0, isdigit('5') != 0, isspace('\t') != 0, ispunct('!') != 0, toupper('q'), tolower('Q'));

    /* numbers */
    char *end;
    printf("%ld %ld %ld %lu\n", strtol("  -123abc", &end, 10), strtol("0x1f", NULL, 16), strtol("0777", NULL, 0), strtoul("ff", NULL, 16));
    printf("rest [%s] %d %ld\n", end, atoi("99"), atol("-5"));
    printf("%.4f %.4f %.4f %g\n", strtod("3.25", NULL), atof("-1e3"), strtod("  .5e1x", &end), strtod("1e-5", NULL));
    printf("rest [%s]\n", end);
    printf("%d %ld %d\n", abs(-4), labs(-5L), div(17, 5).rem);

    /* sorting and searching */
    int nums[] = {5, 2, 9, 1, 5, 6, 0, -3, 11, 4};
    qsort(nums, 10, sizeof(int), cmp_int);
    for (int i = 0; i < 10; i++)
        printf("%d ", nums[i]);
    int key = 6;
    int *found = bsearch(&key, nums, 10, sizeof(int), cmp_int);
    printf("| found at %ld\n", found ? (long)(found - nums) : -1L);
    char *words[] = {"pear", "apple", "fig", "banana"};
    qsort(words, 4, sizeof(char *), cmp_str);
    printf("%s %s %s %s\n", words[0], words[1], words[2], words[3]);

    /* memory */
    long checksum = 0;
    char *blocks[200];
    for (int i = 0; i < 200; i++) {
        blocks[i] = malloc(i * 37 + 1);
        memset(blocks[i], i & 0x7f, i * 37 + 1);
    }
    for (int i = 0; i < 200; i += 2) {
        free(blocks[i]);
        blocks[i] = NULL;
    }
    for (int i = 1; i < 200; i += 2) {
        blocks[i] = realloc(blocks[i], i * 50 + 10);
        checksum += blocks[i][i * 37];
    }
    int *zeros = calloc(1000, sizeof(int));
    for (int i = 0; i < 1000; i++)
        checksum += zeros[i];
    char *huge = malloc(1 << 22);
    huge[(1 << 22) - 1] = 7;
    checksum += huge[(1 << 22) - 1];
    free(huge);
    printf("malloc checksum %ld\n", checksum);

    /* math */
    printf("%.6f %.6f %.6f %.6f\n", sqrt(2.0), pow(2.0, 10.5), exp(1.0), log(10.0));
    printf("%.6f %.6f %.6f %.6f\n", sin(1.0), cos(2.0), tan(0.5), atan2(1.0, -1.0));
    printf("%.1f %.1f %.1f %.1f %.6f %.6f\n", floor(-2.5), ceil(-2.5), round(2.5), trunc(-2.7), fmod(10.5, 3.0), log10(12345.0));
    printf("%.6f %.6f %.6f %d %d\n", asin(0.5), acos(0.5), hypot(3, 4), isnan(NAN), isinf(-INFINITY));

    /* sscanf */
    int a, b;
    char word[16];
    double dv;
    int got = sscanf("12 34 hello 2.5", "%d %d %15s %lf", &a, &b, word, &dv);
    printf("sscanf %d %d %d %s %.2f\n", got, a, b, word, dv);
    unsigned hx;
    char rest[16];
    got = sscanf("key=0x1A;tail", "key=%x;%[a-z]", &hx, rest);
    printf("sscanf %d %u %s\n", got, hx, rest);

    /* files */
    const char *path = "/tmp/hcc-libc-test.txt";
    FILE *f = fopen(path, "w");
    for (int i = 0; i < 100; i++)
        fprintf(f, "line %d\n", i);
    fclose(f);
    f = fopen(path, "r");
    char line[64];
    int lines = 0;
    long sumv = 0;
    while (fgets(line, sizeof line, f)) {
        int v;
        if (sscanf(line, "line %d", &v) == 1)
            sumv += v;
        lines++;
    }
    printf("file %d lines, sum %ld, eof %d\n", lines, sumv, feof(f));
    fseek(f, 5, SEEK_SET);
    printf("seek [%c] tell %ld\n", fgetc(f), ftell(f));
    fclose(f);
    struct stat st;
    stat(path, &st);
    printf("stat size %ld regular %d\n", (long)st.st_size, S_ISREG(st.st_mode));
    f = fopen(path, "a+");
    fputs("appended\n", f);
    fclose(f);
    f = fopen(path, "r");
    char *ln = NULL;
    size_t cap = 0;
    long len, last = 0;
    char lastline[64] = "";
    while ((len = getline(&ln, &cap, f)) > 0) {
        last = len;
        strcpy(lastline, ln);
    }
    printf("last line len %ld [%s]", last, lastline);
    fclose(f);
    free(ln);
    printf("unlink %d access %d\n", unlink(path), access(path, F_OK));

    /* setjmp / longjmp */
    int r = setjmp(jb);
    if (r == 0) {
        deep(10);
        printf("not reached\n");
    } else {
        printf("longjmp returned %d\n", r);
    }

    /* signals */
    signal(SIGUSR1, on_signal);
    raise(SIGUSR1);
    printf("signal %d\n", got_signal);

    /* processes */
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0)
        _exit(7);
    int status;
    waitpid(pid, &status, 0);
    printf("child exited %d %d\n", WIFEXITED(status), WEXITSTATUS(status));
    fflush(stdout);
    int sys = system("echo from the shell");
    printf("system %d\n", WEXITSTATUS(sys));
    FILE *pp = popen("echo piped", "r");
    fgets(line, sizeof line, pp);
    printf("popen [%s] %d\n", strtok(line, "\n"), pclose(pp));

    /* time */
    time_t t = 1700000000;
    struct tm *tm = gmtime(&t);
    strftime(buf, sizeof buf, "%Y-%m-%d %H:%M:%S %a %b %j", tm);
    printf("time %s %ld\n", buf, (long)timegm(tm));
    printf("environ %d\n", getenv("PATH") != NULL);
    return 0;
}
