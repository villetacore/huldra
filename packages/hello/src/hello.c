#include <stdio.h>
#include <string.h>

int main(int argc, char **argv) {
    const char *who = argc > 1 ? argv[1] : "world";
    printf("Hello, %s! (installed with pkg)\n", who);
    return 0;
}
