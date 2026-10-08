/* A small wc(1): counts lines, words and bytes of files or stdin. */
#include <ctype.h>
#include <stdio.h>

static int count(FILE *f, const char *name, long totals[3]) {
    long lines = 0, words = 0, bytes = 0;
    int c, in_word = 0;
    while ((c = fgetc(f)) != EOF) {
        bytes++;
        if (c == '\n')
            lines++;
        if (isspace(c)) {
            in_word = 0;
        } else if (!in_word) {
            in_word = 1;
            words++;
        }
    }
    printf("%7ld %7ld %7ld %s\n", lines, words, bytes, name);
    totals[0] += lines;
    totals[1] += words;
    totals[2] += bytes;
    return 0;
}

int main(int argc, char **argv) {
    long totals[3] = {0, 0, 0};
    int status = 0;
    if (argc < 2)
        return count(stdin, "", totals);
    for (int i = 1; i < argc; i++) {
        FILE *f = fopen(argv[i], "r");
        if (!f) {
            perror(argv[i]);
            status = 1;
            continue;
        }
        count(f, argv[i], totals);
        fclose(f);
    }
    if (argc > 2)
        printf("%7ld %7ld %7ld total\n", totals[0], totals[1], totals[2]);
    return status;
}
