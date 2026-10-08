/* cowsay [-t] [message...]: a cow says (or with -t thinks) the message. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define WIDTH 40

static char text[4096];
static char *lines[128];
static int nlines;

static void wrap(void) {
    char *p = text;
    while (*p && nlines < 128) {
        while (*p == ' ')
            p++;
        if (!*p)
            break;
        size_t len = strlen(p);
        size_t take = len;
        if (take > WIDTH) {
            take = WIDTH;
            while (take > 0 && p[take] != ' ')
                take--;
            if (take == 0)
                take = WIDTH;
        }
        char *line = malloc(take + 1);
        memcpy(line, p, take);
        line[take] = 0;
        lines[nlines++] = line;
        p += take;
    }
}

int main(int argc, char **argv) {
    int think = 0, first = 1;
    for (; first < argc && argv[first][0] == '-'; first++)
        if (strcmp(argv[first], "-t") == 0)
            think = 1;
    size_t n = 0;
    if (first < argc) {
        for (int i = first; i < argc && n < sizeof text - 2; i++) {
            n += snprintf(text + n, sizeof text - n, "%s%s", i > first ? " " : "", argv[i]);
        }
    } else {
        int c;
        while ((c = getchar()) != EOF && n < sizeof text - 1)
            text[n++] = (c == '\n' || c == '\t') ? ' ' : (char)c;
        text[n] = 0;
    }
    wrap();
    if (nlines == 0)
        lines[nlines++] = "";
    int w = 0;
    for (int i = 0; i < nlines; i++)
        if ((int)strlen(lines[i]) > w)
            w = (int)strlen(lines[i]);
    printf(" ");
    for (int i = 0; i < w + 2; i++)
        putchar('_');
    printf("\n");
    for (int i = 0; i < nlines; i++) {
        char l = '|', r = '|';
        if (think) {
            l = '(';
            r = ')';
        } else if (nlines == 1) {
            l = '<';
            r = '>';
        } else if (i == 0) {
            l = '/';
            r = '\\';
        } else if (i == nlines - 1) {
            l = '\\';
            r = '/';
        }
        printf("%c %-*s %c\n", l, w, lines[i], r);
    }
    printf(" ");
    for (int i = 0; i < w + 2; i++)
        putchar('-');
    printf("\n");
    const char *t = think ? "o" : "\\";
    printf("        %s   ^__^\n", t);
    printf("         %s  (oo)\\_______\n", t);
    printf("            (__)\\       )\\/\\\n");
    printf("                ||----w |\n");
    printf("                ||     ||\n");
    return 0;
}
