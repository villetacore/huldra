/* Compile and run on Huldra:
 *     cc hello.c -o hello && ./hello
 * or in one step:
 *     cc -run hello.c
 */
#include <stdio.h>
#include <sys/utsname.h>

int main(int argc, char **argv) {
    struct utsname u;
    uname(&u);
    printf("Hello from C on %s %s!\n", u.sysname, u.release);
    for (int i = 1; i < argc; i++)
        printf("argv[%d] = %s\n", i, argv[i]);
    return 0;
}
