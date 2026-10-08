/* Built with the host's gcc -static: a real Linux binary. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

int main(int argc, char **argv) {
    printf("hello from glibc! argc=%d argv[0]=%s\n", argc, argv[0]);
    double x = 2.0;
    printf("sqrt(2) = %.6f, pi = %.5f\n", sqrt(x), 4.0 * atan(1.0));
    char *buf = malloc(1 << 20);
    memset(buf, 'x', 1 << 20);
    printf("malloc ok: %c, HOME=%s\n", buf[12345], getenv("HOME"));
    free(buf);
    return 0;
}
