/* Language features; the output must match gcc's. */
#include <stdio.h>
#include <stdarg.h>
#include <string.h>

struct point {
    int x, y;
};

struct rect {
    struct point a, b;
    char name[8];
};

union bits {
    double d;
    unsigned long u;
    unsigned char bytes[8];
};

enum color { RED, GREEN = 5, BLUE };

typedef int (*binop)(int, int);

static int add(int a, int b) { return a + b; }
static int mul(int a, int b) { return a * b; }

int counter;
int table[5] = {1, 2, 3};
const char *names[] = {"zero", "one", "two"};
struct point origin = {.y = 7};
struct rect rects[2] = {{{1, 2}, {3, 4}, "first"}, {.name = "second"}};
char greeting[] = "hi there";
int *ptr_to_table = &table[2];
static int matrix[3][4];

int fib(int n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }

struct point make_point(int x, int y) {
    struct point p = {x, y};
    return p;
}

int sum_points(struct point p, struct point q) { return p.x + p.y + q.x + q.y; }

int sum(int n, ...) {
    va_list ap;
    va_start(ap, n);
    int s = 0;
    for (int i = 0; i < n; i++)
        s += va_arg(ap, int);
    va_end(ap);
    return s;
}

double average(int n, ...) {
    va_list ap;
    va_start(ap, n);
    double s = 0;
    for (int i = 0; i < n; i++)
        s += va_arg(ap, double);
    va_end(ap);
    return s / n;
}

int next_id(void) {
    static int id = 100;
    return id++;
}

float half(float f) { return f / 2; }

const char *classify(int c) {
    switch (c) {
    case 0:
        return "zero";
    case 1:
    case 2:
        return "small";
    case 'a' ... 'z':
        return "letter";
    default:
        return "other";
    }
}

int main(void) {
    /* integers */
    int a = 17, b = 5;
    printf("arith %d %d %d %d %d\n", a + b, a - b, a * b, a / b, a % b);
    printf("neg %d %d %d\n", -a / b, -a % b, a / -b);
    printf("bits %d %d %d %d %d %d\n", a & b, a | b, a ^ b, ~a, a << 3, -a >> 1);
    unsigned u = 0xFFFFFFFF;
    printf("unsigned %u %u %u\n", u, u + 1, u >> 4);
    long big = 1L << 40;
    printf("long %ld %ld\n", big, big * 3 - 1);
    unsigned long ul = 18446744073709551615UL;
    printf("ulong %lu %lu\n", ul, ul / 3);
    char c = (char)200;
    unsigned char uc = 200;
    short s = (short)70000;
    printf("narrow %d %d %d\n", c, uc, s);
    printf("compare %d %d %d %d\n", a < b, a > b, -1 < 0u, (unsigned char)255 == 255);
    printf("logic %d %d %d\n", a && 0, a || 0, !a);
    int x = 3;
    x += 4;
    x *= 2;
    x -= 1;
    x /= 3;
    x <<= 2;
    x |= 1;
    x %= 7;
    printf("compound %d\n", x);
    int i = 5;
    int j = i++;
    j += ++i;
    printf("incdec %d %d %d\n", i, j, i--);
    printf("ternary %s %d\n", a > b ? "yes" : "no", a ?: 9);
    printf("comma %d\n", (a = 1, a + 1));
    printf("sizeof %zu %zu %zu %zu %zu\n", sizeof(char), sizeof(int), sizeof(long), sizeof(struct rect), sizeof(union bits));

    /* control flow */
    int total = 0;
    for (int k = 0; k < 10; k++) {
        if (k == 3)
            continue;
        if (k == 8)
            break;
        total += k;
    }
    int w = 0;
    while (w < 100)
        w = w * 2 + 1;
    int d = 0;
    do
        d++;
    while (d < 5);
    printf("loops %d %d %d\n", total, w, d);
    int n = 0;
again:
    n++;
    if (n < 3)
        goto again;
    printf("goto %d\n", n);
    printf("switch %s %s %s %s\n", classify(0), classify(2), classify('q'), classify(99));
    printf("fib %d\n", fib(20));

    /* pointers and arrays */
    int arr[6] = {5, 3, 9};
    int *p = arr;
    p[3] = 4;
    *(p + 4) = 7;
    printf("array %d %d %d %d %d %d %ld\n", arr[0], arr[1], arr[2], arr[3], arr[4], arr[5], (long)(&arr[5] - p));
    for (int r = 0; r < 3; r++)
        for (int k = 0; k < 4; k++)
            matrix[r][k] = r * 10 + k;
    printf("matrix %d %d %zu\n", matrix[2][3], *(*(matrix + 1) + 2), sizeof matrix);
    char buf[32];
    strcpy(buf, greeting);
    buf[0] = 'H';
    printf("string %s %zu %c\n", buf, strlen(buf), "abc"[1]);
    int **pp = &p;
    **pp = 42;
    printf("ptrptr %d\n", arr[0]);

    /* structs and unions */
    struct point pt = make_point(3, 4);
    struct point copy = pt;
    copy.x = 10;
    struct point *pptr = &copy;
    pptr->y += 1;
    printf("struct %d %d %d %d %d\n", pt.x, pt.y, copy.x, copy.y, sum_points(pt, copy));
    printf("globals %d %d %d %s %d %d %s %s\n", table[1], table[4], origin.x, names[2], origin.y, rects[0].b.y, rects[0].name, rects[1].name);
    printf("ptrinit %d\n", *ptr_to_table);
    union bits ub;
    ub.d = 1.0;
    printf("union %lx %d\n", ub.u, ub.bytes[7]);
    struct rect r2 = rects[0];
    r2.a = make_point(8, 9);
    printf("nested %d %d %s\n", r2.a.x, r2.b.x, r2.name);
    struct point pts[3] = {{1, 2}, {3, 4}};
    printf("compound literal %d\n", ((struct point){5, 6}).y + pts[1].x);

    /* enums, function pointers, varargs, statics */
    enum color col = BLUE;
    printf("enum %d %d %d\n", RED, GREEN, col);
    binop ops[2] = {add, mul};
    printf("fnptr %d %d %d\n", ops[0](6, 7), ops[1](6, 7), (*ops[1])(2, 3));
    printf("varargs %d %.2f\n", sum(4, 1, 2, 3, 4), average(3, 1.5, 2.5, 3.5));
    next_id();
    printf("static %d\n", next_id());
    counter++;
    printf("counter %d\n", counter);

    /* floating point */
    double f = 3.75;
    float g = 1.5f;
    printf("float %.3f %.3f %.3f %.3f\n", f + g, f - g, f * g, f / g);
    printf("convert %d %ld %.1f %.2f\n", (int)f, (long)-f, (double)7, (double)half(5));
    printf("fcompare %d %d %d\n", f > g, f == 3.75, g != 1.5);
    int fi = 7;
    fi *= 1.5;
    double acc = 0;
    for (int k = 1; k <= 10; k++)
        acc += 1.0 / k;
    printf("fmix %d %.6f %.3e %g\n", fi, acc, acc * 1e10, 0.0001234);
    unsigned long bigu = 18446744073709551615UL;
    printf("u2d %.0f\n", (double)bigu);

    /* statement expression, offsetof-style */
    int se = ({ int t = 4; t * t; });
    printf("stmtexpr %d %lu\n", se, (unsigned long)&((struct rect *)0)->name);
    return 0;
}
