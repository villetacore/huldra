/* Sieve of Eratosthenes: primes below N (default 100). */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv) {
    int n = argc > 1 ? atoi(argv[1]) : 100;
    if (n < 2) {
        fprintf(stderr, "usage: primes N (N >= 2)\n");
        return 1;
    }
    char *composite = calloc(n, 1);
    int count = 0;
    for (int i = 2; i < n; i++) {
        if (composite[i])
            continue;
        printf("%d%c", i, ++count % 10 ? ' ' : '\n');
        for (long j = (long)i * i; j < n; j += i)
            composite[j] = 1;
    }
    printf("\n%d primes below %d\n", count, n);
    free(composite);
    return 0;
}
