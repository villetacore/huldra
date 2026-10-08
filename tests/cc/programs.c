/* Larger programs exercising the compiler: data structures, an
   interpreter, by-value aggregates, mixed varargs. */
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <ctype.h>

/* ---- hash table ---- */
struct entry {
    char *key;
    int value;
    struct entry *next;
};

struct table {
    struct entry *buckets[64];
    int count;
};

static unsigned hash(const char *s) {
    unsigned h = 2166136261u;
    while (*s)
        h = (h ^ (unsigned char)*s++) * 16777619u;
    return h;
}

static void put(struct table *t, const char *key, int value) {
    unsigned b = hash(key) % 64;
    for (struct entry *e = t->buckets[b]; e; e = e->next) {
        if (strcmp(e->key, key) == 0) {
            e->value = value;
            return;
        }
    }
    struct entry *e = malloc(sizeof *e);
    e->key = strdup(key);
    e->value = value;
    e->next = t->buckets[b];
    t->buckets[b] = e;
    t->count++;
}

static int *get(struct table *t, const char *key) {
    for (struct entry *e = t->buckets[hash(key) % 64]; e; e = e->next)
        if (strcmp(e->key, key) == 0)
            return &e->value;
    return NULL;
}

/* ---- expression interpreter ---- */
enum kind { NUM, ADD, SUB, MUL, DIV, NEG, VAR };

struct node {
    enum kind kind;
    long value;
    char name;
    struct node *l, *r;
};

static const char *src;
static long vars[26];

static struct node *mk(enum kind k, struct node *l, struct node *r) {
    struct node *n = calloc(1, sizeof(struct node));
    n->kind = k;
    n->l = l;
    n->r = r;
    return n;
}

static struct node *expr(void);

static struct node *primary(void) {
    while (isspace(*src))
        src++;
    if (*src == '(') {
        src++;
        struct node *n = expr();
        while (isspace(*src))
            src++;
        src++; /* ')' */
        return n;
    }
    if (*src == '-') {
        src++;
        return mk(NEG, primary(), NULL);
    }
    if (isalpha(*src)) {
        struct node *n = mk(VAR, NULL, NULL);
        n->name = *src++;
        return n;
    }
    struct node *n = mk(NUM, NULL, NULL);
    n->value = strtol(src, (char **)&src, 10);
    return n;
}

static struct node *term(void) {
    struct node *n = primary();
    for (;;) {
        while (isspace(*src))
            src++;
        if (*src == '*') {
            src++;
            n = mk(MUL, n, primary());
        } else if (*src == '/') {
            src++;
            n = mk(DIV, n, primary());
        } else {
            return n;
        }
    }
}

static struct node *expr(void) {
    struct node *n = term();
    for (;;) {
        while (isspace(*src))
            src++;
        if (*src == '+') {
            src++;
            n = mk(ADD, n, term());
        } else if (*src == '-') {
            src++;
            n = mk(SUB, n, term());
        } else {
            return n;
        }
    }
}

static long eval(struct node *n) {
    switch (n->kind) {
    case NUM: return n->value;
    case VAR: return vars[n->name - 'a'];
    case NEG: return -eval(n->l);
    case ADD: return eval(n->l) + eval(n->r);
    case SUB: return eval(n->l) - eval(n->r);
    case MUL: return eval(n->l) * eval(n->r);
    case DIV: return eval(n->l) / eval(n->r);
    }
    return 0;
}

/* ---- big structs by value ---- */
struct big {
    long a[6];
    char tag[10];
    double d;
};

static struct big make_big(int seed) {
    struct big b;
    for (int i = 0; i < 6; i++)
        b.a[i] = seed * (i + 1);
    snprintf(b.tag, sizeof b.tag, "big%d", seed);
    b.d = seed / 4.0;
    return b;
}

static struct big twice(struct big b) {
    for (int i = 0; i < 6; i++)
        b.a[i] *= 2;
    b.d *= 2;
    return b;
}

static long big_sum(struct big x, int k, struct big y) {
    long s = k;
    for (int i = 0; i < 6; i++)
        s += x.a[i] - y.a[i];
    return s;
}

/* ---- mixed varargs ---- */
static void logf_(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    for (const char *p = fmt; *p; p++) {
        switch (*p) {
        case 'i': printf("i:%d ", va_arg(ap, int)); break;
        case 'l': printf("l:%ld ", va_arg(ap, long)); break;
        case 'd': printf("d:%.3f ", va_arg(ap, double)); break;
        case 's': printf("s:%s ", va_arg(ap, char *)); break;
        case 'c': printf("c:%c ", va_arg(ap, int)); break;
        }
    }
    va_end(ap);
    printf("\n");
}

/* ---- function pointer tables in static data ---- */
static int op_inc(int x) { return x + 1; }
static int op_dbl(int x) { return x * 2; }
static int op_sq(int x) { return x * x; }

static struct {
    const char *name;
    int (*fn)(int);
} ops[] = {{"inc", op_inc}, {"dbl", op_dbl}, {"sq", op_sq}};

static const int primes[][4] = {{2, 3, 5, 7}, {11, 13}, {17}};

/* ---- linked list sort ---- */
struct item {
    int v;
    struct item *next;
};

static struct item *merge(struct item *a, struct item *b) {
    struct item head, *t = &head;
    while (a && b) {
        if (a->v <= b->v) {
            t->next = a;
            a = a->next;
        } else {
            t->next = b;
            b = b->next;
        }
        t = t->next;
    }
    t->next = a ? a : b;
    return head.next;
}

static struct item *msort(struct item *l) {
    if (!l || !l->next)
        return l;
    struct item *slow = l, *fast = l->next;
    while (fast && fast->next) {
        slow = slow->next;
        fast = fast->next->next;
    }
    struct item *r = slow->next;
    slow->next = NULL;
    return merge(msort(l), msort(r));
}

/* ---- sieve and bit tricks ---- */
static int popcount(unsigned long x) {
    int n = 0;
    while (x) {
        x &= x - 1;
        n++;
    }
    return n;
}

int main(void) {
    struct table t = {0};
    const char *words = "the quick brown fox jumps over the lazy dog the end";
    char buf[128];
    strcpy(buf, words);
    for (char *w = strtok(buf, " "); w; w = strtok(NULL, " ")) {
        int *v = get(&t, w);
        put(&t, w, v ? *v + 1 : 1);
    }
    printf("words %d the=%d fox=%d cat=%p\n", t.count, *get(&t, "the"), *get(&t, "fox"), (void *)get(&t, "cat"));

    vars['x' - 'a'] = 7;
    vars['y' - 'a'] = -3;
    const char *exprs[] = {"1 + 2 * 3", "(1 + 2) * 3", "x * x - y", "-(x + y) / 2", "100 / 7 / 2 - -4"};
    for (int i = 0; i < 5; i++) {
        src = exprs[i];
        printf("%s = %ld\n", exprs[i], eval(expr()));
    }

    struct big b = twice(make_big(3));
    struct big c = make_big(1);
    printf("big %ld %ld %s %.2f %ld\n", b.a[0], b.a[5], b.tag, b.d, big_sum(b, 100, c));
    struct big arr[3];
    for (int i = 0; i < 3; i++)
        arr[i] = make_big(i + 10);
    printf("big array %s %ld\n", arr[2].tag, arr[1].a[3]);

    logf_("ildsc", -5, 1L << 35, 2.5, "str", 'Z');
    float fv = 0.25f;
    char ch = 'q';
    short sh = -300;
    logf_("dci", fv, ch, sh);

    for (int i = 0; i < 3; i++)
        printf("%s(5)=%d ", ops[i].name, ops[i].fn(5));
    printf("| %d %d %d %zu\n", primes[0][3], primes[1][1], primes[2][1], sizeof primes);

    struct item items[10];
    int seedv[10] = {9, 3, 7, 1, 8, 2, 6, 0, 5, 4};
    for (int i = 0; i < 10; i++) {
        items[i].v = seedv[i];
        items[i].next = i < 9 ? &items[i + 1] : NULL;
    }
    for (struct item *p = msort(items); p; p = p->next)
        printf("%d", p->v);
    printf("\n");

    char sieve[100];
    memset(sieve, 1, sizeof sieve);
    int count = 0;
    for (int i = 2; i < 100; i++) {
        if (!sieve[i])
            continue;
        count++;
        for (int j = i * i; j < 100; j += i)
            sieve[j] = 0;
    }
    printf("primes<100 %d popcount %d %d\n", count, popcount(0xF0F0F0F0F0UL), popcount(~0UL));

    unsigned char wrap = 250;
    wrap += 10;
    unsigned short us = 65535;
    us++;
    int neg = -7;
    unsigned long shifted = 1UL << 63;
    printf("wrap %d %d %ld %ld %lu %d\n", wrap, us, (long)neg % 3, (long)neg / 2, shifted >> 62, (signed char)0x80);
    double dd = 1e300;
    printf("conv %d %u %ld %.0f %d\n", (int)-3.99, (unsigned)3.99, (long)1e18, dd * 1e-290, (int)(char)300);
    int matrix[3][3] = {{1, 2, 3}, {4, 5, 6}, {7, 8, 9}};
    int trace = 0;
    for (int i = 0; i < 3; i++)
        trace += matrix[i][i];
    int (*row)[3] = matrix + 1;
    printf("matrix trace %d row %d %d\n", trace, (*row)[2], row[1][0]);
    int depth = 0;
    for (int i = 0; i < 5; i++) {
        do {
            depth++;
            if (depth % 2)
                continue;
            depth += 10;
        } while (depth < i * 10);
    }
    printf("loops %d\n", depth);
    return 0;
}
