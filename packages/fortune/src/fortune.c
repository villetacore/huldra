/* fortune [file]: prints a random entry from a %-separated file. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#define DEFAULT "/pkg/system/sw/share/fortune/fortunes"

int main(int argc, char **argv) {
    const char *path = argc > 1 ? argv[1] : DEFAULT;
    FILE *f = fopen(path, "r");
    if (!f) {
        perror(path);
        return 1;
    }
    char *text = NULL;
    size_t cap = 0, len = 0;
    char buf[1024];
    size_t n;
    while ((n = fread(buf, 1, sizeof buf, f)) > 0) {
        if (len + n + 1 > cap) {
            cap = (len + n + 1) * 2;
            text = realloc(text, cap);
        }
        memcpy(text + len, buf, n);
        len += n;
    }
    fclose(f);
    if (!text)
        return 1;
    text[len] = 0;

    char *entries[1024];
    int count = 0;
    char *save;
    for (char *e = strtok_r(text, "%", &save); e && count < 1024; e = strtok_r(NULL, "%", &save)) {
        while (*e == '\n')
            e++;
        if (*e)
            entries[count++] = e;
    }
    if (count == 0)
        return 1;
    srand((unsigned)(time(NULL) ^ (getpid() << 8) ^ clock()));
    char *e = entries[rand() % count];
    size_t l = strlen(e);
    while (l > 0 && e[l - 1] == '\n')
        e[--l] = 0;
    puts(e);
    return 0;
}
