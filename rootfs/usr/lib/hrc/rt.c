/* The runtime of programs compiled by hrc (the Huldra Rust compiler):
 * process start and exit, the stdout buffer, panics with rustc's messages,
 * and exact float formatting and parsing. Compiled together with the
 * generated code and the C library by hcc. */

#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>

typedef struct {
    uint8_t *ptr;
    uint64_t len;
} hrc_str;

/* core::panic::Location */
typedef struct {
    hrc_str file;
    uint32_t line;
    uint32_t col;
} hrc_loc;

static int hrc_argc;
static char **hrc_argv;

void hrc_rt_init(int argc, char **argv) {
    hrc_argc = argc;
    hrc_argv = argv;
}

int hrc_errno(void) { return errno; }

uint64_t hrc_arg_count(void) { return (uint64_t)hrc_argc; }

hrc_str hrc_arg(uint64_t i) {
    hrc_str s;
    s.ptr = (uint8_t *)hrc_argv[i];
    s.len = strlen(hrc_argv[i]);
    return s;
}

/* ---- output ---- */

static uint8_t hrc_obuf[8192];
static uint64_t hrc_olen;

static void hrc_write_all(int fd, const uint8_t *p, uint64_t n) {
    while (n > 0) {
        long w = write(fd, p, n);
        if (w <= 0)
            return;
        p += w;
        n -= (uint64_t)w;
    }
}

void hrc_flush_stdout(void) {
    hrc_write_all(1, hrc_obuf, hrc_olen);
    hrc_olen = 0;
}

/* Like Rust's LineWriter: complete lines go out at once. */
void hrc_write_stdout(const uint8_t *p, uint64_t n) {
    uint64_t nl = 0;
    for (uint64_t i = n; i > 0; i--) {
        if (p[i - 1] == 10) {
            nl = i;
            break;
        }
    }
    if (nl > 0) {
        if (hrc_olen + nl <= sizeof hrc_obuf) {
            memcpy(hrc_obuf + hrc_olen, p, nl);
            hrc_olen += nl;
            hrc_flush_stdout();
        } else {
            hrc_flush_stdout();
            hrc_write_all(1, p, nl);
        }
        p += nl;
        n -= nl;
    }
    if (n == 0)
        return;
    if (hrc_olen + n > sizeof hrc_obuf) {
        hrc_flush_stdout();
        if (n > sizeof hrc_obuf) {
            hrc_write_all(1, p, n);
            return;
        }
    }
    memcpy(hrc_obuf + hrc_olen, p, n);
    hrc_olen += n;
}

void hrc_write_stderr(const uint8_t *p, uint64_t n) { hrc_write_all(2, p, n); }

void hrc_exit(int code) {
    hrc_flush_stdout();
    _exit(code);
}

/* ---- panics ---- */

static void hrc_err(const char *s) { hrc_write_all(2, (const uint8_t *)s, strlen(s)); }

static void hrc_err_u64(uint64_t v) {
    char buf[24];
    int i = 23;
    buf[i] = 0;
    do {
        buf[--i] = (char)('0' + v % 10);
        v /= 10;
    } while (v);
    hrc_err(buf + i);
}

static int hrc_panicking;

/* Prints "thread 'main' panicked at file:line:col:" and the message. */
void hrc_panic_start(const void *l) {
    const hrc_loc *loc = l;
    if (hrc_panicking) {
        hrc_err("thread panicked while processing panic. aborting.\n");
        hrc_exit(134);
    }
    hrc_panicking = 1;
    /* Like rustc 1.87+: the thread id (the pid for the main thread). */
    hrc_err("\nthread 'main' (");
    hrc_err_u64((uint64_t)getpid());
    hrc_err(") panicked at ");
    hrc_write_all(2, loc->file.ptr, loc->file.len);
    hrc_err(":");
    hrc_err_u64(loc->line);
    hrc_err(":");
    hrc_err_u64(loc->col);
    hrc_err(":\n");
}

void hrc_panic_end(void) {
    hrc_err("\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n");
    hrc_exit(101);
}

void hrc_panic_str(const uint8_t *msg, uint64_t len, const void *loc) {
    hrc_panic_start(loc);
    hrc_write_all(2, msg, len);
    hrc_panic_end();
}

void hrc_panic_bounds(uint64_t index, uint64_t len, const void *loc) {
    hrc_panic_start(loc);
    hrc_err("index out of bounds: the len is ");
    hrc_err_u64(len);
    hrc_err(" but the index is ");
    hrc_err_u64(index);
    hrc_panic_end();
}

void hrc_slice_check(uint64_t lo, uint64_t hi, uint64_t len, const void *loc) {
    if (lo <= hi && hi <= len)
        return;
    hrc_panic_start(loc);
    if (lo > len && hi == len) {
        hrc_err("range start index ");
        hrc_err_u64(lo);
        hrc_err(" out of range for slice of length ");
        hrc_err_u64(len);
    } else if (lo > hi) {
        hrc_err("slice index starts at ");
        hrc_err_u64(lo);
        hrc_err(" but ends at ");
        hrc_err_u64(hi);
    } else {
        hrc_err("range end index ");
        hrc_err_u64(hi);
        hrc_err(" out of range for slice of length ");
        hrc_err_u64(len);
    }
    hrc_panic_end();
}

static int hrc_is_boundary(hrc_str s, uint64_t i) {
    if (i == 0 || i == s.len)
        return 1;
    if (i > s.len)
        return 0;
    return (s.ptr[i] & 0xC0) != 0x80;
}

static void hrc_err_str_trunc(hrc_str s) {
    uint64_t n = s.len;
    int cut = 0;
    if (n > 256) {
        n = 256;
        while (n > 0 && (s.ptr[n] & 0xC0) == 0x80)
            n--;
        cut = 1;
    }
    hrc_err("`");
    hrc_write_all(2, s.ptr, n);
    hrc_err("`");
    if (cut)
        hrc_err("[...]");
}

void hrc_str_slice_check(hrc_str s, uint64_t lo, uint64_t hi, const void *loc) {
    if (lo <= hi && hi <= s.len && hrc_is_boundary(s, lo) && hrc_is_boundary(s, hi))
        return;
    hrc_panic_start(loc);
    if (lo > s.len || hi > s.len) {
        hrc_err("byte index ");
        hrc_err_u64(lo > s.len ? lo : hi);
        hrc_err(" is out of bounds of ");
        hrc_err_str_trunc(s);
    } else if (lo > hi) {
        hrc_err("begin <= end (");
        hrc_err_u64(lo);
        hrc_err(" <= ");
        hrc_err_u64(hi);
        hrc_err(") when slicing ");
        hrc_err_str_trunc(s);
    } else {
        uint64_t i = hrc_is_boundary(s, lo) ? hi : lo;
        uint64_t start = i;
        while (start > 0 && (s.ptr[start] & 0xC0) == 0x80)
            start--;
        uint64_t end = start + 1;
        while (end < s.len && (s.ptr[end] & 0xC0) == 0x80)
            end++;
        hrc_err("byte index ");
        hrc_err_u64(i);
        hrc_err(" is not a char boundary; it is inside '");
        hrc_write_all(2, s.ptr + start, end - start);
        hrc_err("' (bytes ");
        hrc_err_u64(start);
        hrc_err("..");
        hrc_err_u64(end);
        hrc_err(") of ");
        hrc_err_str_trunc(s);
    }
    hrc_panic_end();
}

void hrc_unreachable(const void *loc) {
    hrc_panic_start(loc);
    hrc_err("internal error: entered unreachable code");
    hrc_panic_end();
}

void hrc_overflow(int op, const void *loc) {
    static const char *msgs[] = {
        "",
        "attempt to add with overflow",
        "attempt to subtract with overflow",
        "attempt to multiply with overflow",
        "attempt to divide by zero",
        "attempt to calculate the remainder with a divisor of zero",
        "attempt to divide with overflow",
        "attempt to calculate the remainder with overflow",
        "",
        "attempt to negate with overflow",
        "attempt to shift left with overflow",
        "attempt to shift right with overflow",
    };
    hrc_panic_start(loc);
    hrc_err(msgs[op]);
    hrc_panic_end();
}

int hrc_str_eq(hrc_str a, hrc_str b) { return a.len == b.len && memcmp(a.ptr, b.ptr, a.len) == 0; }

/* ---- numeric conversions ---- */

long hrc_f2i(double v, long lo, long hi) {
    if (v != v)
        return 0;
    if (v <= (double)lo)
        return lo;
    if (v >= (double)hi)
        return hi;
    return (long)v;
}

unsigned long hrc_f2u(double v, unsigned long hi) {
    if (v != v || v <= 0)
        return 0;
    if (v >= 18446744073709551616.0)
        return hi;
    unsigned long r;
    if (v >= 9223372036854775808.0)
        r = (unsigned long)(long)(v - 9223372036854775808.0) + 9223372036854775808UL;
    else
        r = (unsigned long)(long)v;
    return r > hi ? hi : r;
}

double hrc_u2f(unsigned long v) {
    if ((long)v >= 0)
        return (double)(long)v;
    /* Halve with the low bit kept sticky so the rounding stays correct. */
    unsigned long h = (v >> 1) | (v & 1);
    return (double)(long)h * 2.0;
}

/* ---- big integers (for exact float conversions) ---- */

#define BIG_LIMBS 80

typedef struct {
    uint32_t d[BIG_LIMBS];
    int n;
} big;

static void big_set(big *a, uint64_t v) {
    a->n = 0;
    while (v) {
        a->d[a->n++] = (uint32_t)v;
        v >>= 32;
    }
}

static void big_mul_small(big *a, uint32_t m) {
    uint64_t carry = 0;
    for (int i = 0; i < a->n; i++) {
        uint64_t t = (uint64_t)a->d[i] * m + carry;
        a->d[i] = (uint32_t)t;
        carry = t >> 32;
    }
    if (carry)
        a->d[a->n++] = (uint32_t)carry;
}

static void big_add(big *a, const big *b) {
    uint64_t carry = 0;
    int n = a->n > b->n ? a->n : b->n;
    for (int i = 0; i < n; i++) {
        uint64_t t = carry;
        if (i < a->n)
            t += a->d[i];
        if (i < b->n)
            t += b->d[i];
        a->d[i] = (uint32_t)t;
        carry = t >> 32;
    }
    a->n = n;
    if (carry)
        a->d[a->n++] = (uint32_t)carry;
}

/* a -= b (a >= b) */
static void big_sub(big *a, const big *b) {
    int64_t borrow = 0;
    for (int i = 0; i < a->n; i++) {
        int64_t t = (int64_t)a->d[i] - borrow - (i < b->n ? (int64_t)b->d[i] : 0);
        borrow = t < 0;
        a->d[i] = (uint32_t)(t + (borrow << 32));
    }
    while (a->n > 0 && a->d[a->n - 1] == 0)
        a->n--;
}

static int big_cmp(const big *a, const big *b) {
    if (a->n != b->n)
        return a->n < b->n ? -1 : 1;
    for (int i = a->n - 1; i >= 0; i--) {
        if (a->d[i] != b->d[i])
            return a->d[i] < b->d[i] ? -1 : 1;
    }
    return 0;
}

static void big_shl(big *a, int s) {
    if (a->n == 0)
        return;
    int words = s / 32, bits = s % 32;
    if (bits) {
        uint32_t carry = 0;
        for (int i = 0; i < a->n; i++) {
            uint32_t v = a->d[i];
            a->d[i] = (v << bits) | carry;
            carry = v >> (32 - bits);
        }
        if (carry)
            a->d[a->n++] = carry;
    }
    if (words) {
        for (int i = a->n - 1; i >= 0; i--)
            a->d[i + words] = a->d[i];
        for (int i = 0; i < words; i++)
            a->d[i] = 0;
        a->n += words;
    }
}

static void big_pow10(big *a, int k) {
    while (k >= 9) {
        big_mul_small(a, 1000000000u);
        k -= 9;
    }
    static const uint32_t p[] = {1, 10, 100, 1000, 10000, 100000, 1000000, 10000000, 100000000};
    if (k > 0)
        big_mul_small(a, p[k]);
}

static int big_bits(const big *a) {
    if (a->n == 0)
        return 0;
    uint32_t top = a->d[a->n - 1];
    int b = 0;
    while (top) {
        b++;
        top >>= 1;
    }
    return (a->n - 1) * 32 + b;
}

static int big_bit(const big *a, int i) {
    int w = i / 32;
    if (w >= a->n)
        return 0;
    return (a->d[w] >> (i % 32)) & 1;
}

/* ---- float to decimal ---- */

static void decompose(double v, int is_f32, uint64_t *f, int *e, int *min_e, int *p) {
    union {
        double d;
        uint64_t u;
    } x;
    x.d = v;
    uint64_t bits = x.u;
    int be = (int)((bits >> 52) & 0x7FF);
    uint64_t m = bits & ((1UL << 52) - 1);
    if (be == 0) {
        *f = m;
        *e = -1074;
    } else {
        *f = m | (1UL << 52);
        *e = be - 1075;
    }
    *min_e = -1074;
    *p = 53;
    if (is_f32) {
        /* Re-derive from the float's own bits. */
        union {
            float f;
            uint32_t u;
        } y;
        y.f = (float)v;
        uint32_t b = y.u;
        int fe = (int)((b >> 23) & 0xFF);
        uint32_t fm = b & ((1u << 23) - 1);
        if (fe == 0) {
            *f = fm;
            *e = -149;
        } else {
            *f = fm | (1u << 23);
            *e = fe - 150;
        }
        *min_e = -149;
        *p = 24;
    }
}

/* Shortest digits that read back as the same value (Burger & Dybvig).
 * Returns the number of digits; the value is 0.DIGITS * 10^k. */
static int shortest(double v, int is_f32, char *digits, int *k_out) {
    uint64_t f;
    int e, min_e, p;
    decompose(v, is_f32, &f, &e, &min_e, &p);
    int even = (f & 1) == 0;
    big r, s, mp, mm;
    uint64_t hidden = 1UL << (p - 1);
    if (e >= 0) {
        if (f != hidden) {
            big_set(&r, f);
            big_shl(&r, e + 1);
            big_set(&s, 2);
            big_set(&mp, 1);
            big_shl(&mp, e);
            big_set(&mm, 1);
            big_shl(&mm, e);
        } else {
            big_set(&r, f);
            big_shl(&r, e + 2);
            big_set(&s, 4);
            big_set(&mp, 1);
            big_shl(&mp, e + 1);
            big_set(&mm, 1);
            big_shl(&mm, e);
        }
    } else {
        if (e == min_e || f != hidden) {
            big_set(&r, f);
            big_shl(&r, 1);
            big_set(&s, 1);
            big_shl(&s, 1 - e);
            big_set(&mp, 1);
            big_set(&mm, 1);
        } else {
            big_set(&r, f);
            big_shl(&r, 2);
            big_set(&s, 1);
            big_shl(&s, 2 - e);
            big_set(&mp, 2);
            big_set(&mm, 1);
        }
    }
    /* Estimate k, then fix it up exactly. */
    int bits = big_bits(&r) - big_bits(&s);
    int k = (int)((double)bits * 0.30102999566398114);
    if (k > 0) {
        big_pow10(&s, k);
    } else if (k < 0) {
        big_pow10(&r, -k);
        big_pow10(&mp, -k);
        big_pow10(&mm, -k);
    }
    for (;;) {
        big t = r;
        big_add(&t, &mp);
        int c = big_cmp(&t, &s);
        if (even ? c >= 0 : c > 0) {
            big_mul_small(&s, 10);
            k++;
            continue;
        }
        big t10 = t;
        big_mul_small(&t10, 10);
        int c10 = big_cmp(&t10, &s);
        if (even ? c10 < 0 : c10 <= 0) {
            big_mul_small(&r, 10);
            big_mul_small(&mp, 10);
            big_mul_small(&mm, 10);
            k--;
            continue;
        }
        break;
    }
    int n = 0;
    for (;;) {
        big_mul_small(&r, 10);
        big_mul_small(&mp, 10);
        big_mul_small(&mm, 10);
        int d = 0;
        while (big_cmp(&r, &s) >= 0) {
            big_sub(&r, &s);
            d++;
        }
        int c1 = big_cmp(&r, &mm);
        int tc1 = even ? c1 <= 0 : c1 < 0;
        big t = r;
        big_add(&t, &mp);
        int c2 = big_cmp(&t, &s);
        int tc2 = even ? c2 >= 0 : c2 > 0;
        if (!tc1 && !tc2) {
            digits[n++] = (char)('0' + d);
            continue;
        }
        if (tc1 && tc2) {
            big r2 = r;
            big_shl(&r2, 1);
            int c = big_cmp(&r2, &s);
            if (c > 0 || (c == 0 && (d & 1)))
                d++;
        } else if (tc2) {
            d++;
        }
        digits[n++] = (char)('0' + d);
        break;
    }
    *k_out = k;
    return n;
}

/* The exact decimal expansion: integer digits and fraction digits. */
static void exact(double v, int is_f32, char *ip, int *ilen, char *fp, int *flen) {
    uint64_t f;
    int e, min_e, p;
    decompose(v, is_f32, &f, &e, &min_e, &p);
    big a;
    big_set(&a, f);
    big frac;
    frac.n = 0;
    int fbits = 0;
    if (e >= 0) {
        big_shl(&a, e);
    } else {
        fbits = -e;
        /* frac = low fbits bits of f, a = f >> fbits */
        big_set(&frac, f);
        if (fbits >= 64) {
            big_set(&a, 0);
        } else {
            big_set(&a, f >> fbits);
            uint64_t mask = fbits == 64 ? ~0UL : ((1UL << fbits) - 1);
            big_set(&frac, f & mask);
        }
    }
    /* integer digits */
    char tmp[400];
    int t = 0;
    big q = a;
    while (q.n > 0) {
        uint64_t rem = 0;
        for (int i = q.n - 1; i >= 0; i--) {
            uint64_t cur = (rem << 32) | q.d[i];
            q.d[i] = (uint32_t)(cur / 10);
            rem = cur % 10;
        }
        while (q.n > 0 && q.d[q.n - 1] == 0)
            q.n--;
        tmp[t++] = (char)('0' + rem);
    }
    if (t == 0)
        tmp[t++] = '0';
    for (int i = 0; i < t; i++)
        ip[i] = tmp[t - 1 - i];
    *ilen = t;
    /* fraction digits: frac / 2^fbits */
    int n = 0;
    while (frac.n > 0) {
        big_mul_small(&frac, 10);
        int d = 0;
        /* digit = frac >> fbits; frac &= mask */
        int top = big_bits(&frac);
        for (int i = top - 1; i >= fbits; i--)
            d = d * 2 + big_bit(&frac, i);
        int words = fbits / 32;
        int rb = fbits % 32;
        if (frac.n > words) {
            frac.d[words] &= rb ? ((1u << rb) - 1) : 0;
            frac.n = words + 1;
        }
        while (frac.n > 0 && frac.d[frac.n - 1] == 0)
            frac.n--;
        fp[n++] = (char)('0' + d);
    }
    *flen = n;
}

/* Rounds a digit string at `keep` digits (round half to even on the exact
 * value); returns 1 when it carried into a new leading digit. */
static int round_digits(char *d, int len, int keep) {
    if (keep >= len)
        return 0;
    int up = 0;
    if (d[keep] > '5') {
        up = 1;
    } else if (d[keep] == '5') {
        int rest = 0;
        for (int i = keep + 1; i < len; i++) {
            if (d[i] != '0') {
                rest = 1;
                break;
            }
        }
        if (rest)
            up = 1;
        else
            up = keep > 0 ? ((d[keep - 1] - '0') & 1) : 0;
    }
    if (!up)
        return 0;
    for (int i = keep - 1; i >= 0; i--) {
        if (d[i] == '9') {
            d[i] = '0';
        } else {
            d[i]++;
            return 0;
        }
    }
    return 1;
}

static int put(uint8_t *out, int n, const char *s, int len) {
    for (int i = 0; i < len; i++)
        out[n + i] = (uint8_t)s[i];
    return n + len;
}

static int put_int(uint8_t *out, int n, int v) {
    char b[16];
    int i = 15;
    int neg = v < 0;
    if (neg)
        v = -v;
    do {
        b[i--] = (char)('0' + v % 10);
        v /= 10;
    } while (v);
    if (neg)
        b[i--] = '-';
    return put(out, n, b + i + 1, 15 - i);
}

/* Formats |v| like Rust. kind: 0 Display, 1 Debug, 2 LowerExp, 3 UpperExp.
 * prec < 0: shortest. Returns the length written to out (>= 1100 bytes). */
uint64_t hrc_fmt_float(double v, int is_f32, int kind, long prec, uint8_t *out) {
    if (v != v)
        return (uint64_t)put(out, 0, "NaN", 3);
    if (v < 0)
        v = -v;
    if (v == 1.0 / 0.0)
        return (uint64_t)put(out, 0, "inf", 3);
    int n = 0;
    static char digits[1200];
    int exp_form = kind >= 2;
    if (kind == 1 && prec < 0 && v != 0 && (v < 1e-4 || v >= 1e16))
        exp_form = 1;
    if (exp_form) {
        int nd, k;
        if (v == 0) {
            digits[0] = '0';
            nd = 1;
            k = 1;
        } else if (prec < 0) {
            nd = shortest(v, is_f32, digits, &k);
        } else {
            static char ip[400], fp[1200];
            int il, fl;
            exact(v, is_f32, ip, &il, fp, &fl);
            /* all significant digits */
            int pos = 0;
            nd = 0;
            int lead = 1;
            k = il;
            for (int i = 0; i < il; i++) {
                if (lead && ip[i] == '0') {
                    k--;
                    continue;
                }
                lead = 0;
                digits[nd++] = ip[i];
            }
            for (int i = 0; i < fl; i++) {
                if (lead && fp[i] == '0') {
                    k--;
                    continue;
                }
                lead = 0;
                digits[nd++] = fp[i];
            }
            (void)pos;
            if (round_digits(digits, nd, (int)prec + 1)) {
                digits[0] = '1';
                for (int i = 1; i <= prec; i++)
                    digits[i] = '0';
                k++;
            }
            nd = (int)prec + 1;
            for (int i = 0; i < nd; i++)
                if (digits[i] < '0' || digits[i] > '9')
                    digits[i] = '0';
        }
        out[n++] = (uint8_t)digits[0];
        int frac = prec < 0 ? nd - 1 : (int)prec;
        if (frac > 0) {
            out[n++] = '.';
            for (int i = 1; i <= frac; i++)
                out[n++] = (uint8_t)(i < nd ? digits[i] : '0');
        }
        out[n++] = kind == 3 ? 'E' : 'e';
        n = put_int(out, n, v == 0 ? 0 : k - 1);
        return (uint64_t)n;
    }
    if (prec < 0) {
        int nd, k;
        if (v == 0) {
            n = put(out, n, "0", 1);
            if (kind == 1)
                n = put(out, n, ".0", 2);
            return (uint64_t)n;
        }
        nd = shortest(v, is_f32, digits, &k);
        /* value = 0.d1..dn * 10^k */
        if (k <= 0) {
            n = put(out, n, "0.", 2);
            for (int i = 0; i < -k; i++)
                out[n++] = '0';
            n = put(out, n, digits, nd);
        } else if (k >= nd) {
            n = put(out, n, digits, nd);
            for (int i = nd; i < k; i++)
                out[n++] = '0';
            if (kind == 1)
                n = put(out, n, ".0", 2);
        } else {
            n = put(out, n, digits, k);
            out[n++] = '.';
            n = put(out, n, digits + k, nd - k);
        }
        return (uint64_t)n;
    }
    /* Fixed precision: round the exact expansion. */
    static char ip[400], fp[1200];
    int il, fl;
    exact(v, is_f32, ip, &il, fp, &fl);
    int keep = il + (int)prec;
    static char all[1600];
    int tot = 0;
    for (int i = 0; i < il; i++)
        all[tot++] = ip[i];
    for (int i = 0; i < fl; i++)
        all[tot++] = fp[i];
    while (tot < keep)
        all[tot++] = '0';
    int carry = round_digits(all, tot, keep);
    if (carry)
        out[n++] = '1';
    n = put(out, n, all, il);
    if (prec > 0) {
        out[n++] = '.';
        n = put(out, n, all + il, (int)prec);
    }
    return (uint64_t)n;
}

/* ---- decimal to float ---- */

/* Parses a float like Rust's `str::parse::<f64>` (no surrounding spaces).
 * Returns 0 on error. */
int hrc_parse_float(const uint8_t *s, uint64_t len, int is_f32, double *out) {
    uint64_t i = 0;
    int neg = 0;
    if (i < len && (s[i] == '+' || s[i] == '-')) {
        neg = s[i] == '-';
        i++;
    }
    uint64_t rest = len - i;
    const uint8_t *r = s + i;
    if ((rest == 3 || rest == 8) && (r[0] | 32) == 'i' && (r[1] | 32) == 'n' && (r[2] | 32) == 'f') {
        if (rest == 8) {
            const char *w = "infinity";
            for (int k = 0; k < 8; k++)
                if ((r[k] | 32) != w[k])
                    return 0;
        }
        *out = neg ? -1.0 / 0.0 : 1.0 / 0.0;
        return 1;
    }
    if (rest == 3 && (r[0] | 32) == 'n' && (r[1] | 32) == 'a' && (r[2] | 32) == 'n') {
        *out = 0.0 / 0.0;
        return 1;
    }
    static char dig[800];
    int nd = 0;
    long dexp = 0;
    int any = 0, sticky = 0;
    while (i < len && s[i] >= '0' && s[i] <= '9') {
        any = 1;
        if (nd == 0 && s[i] == '0') {
            i++;
            continue;
        }
        if (nd < 780)
            dig[nd++] = (char)s[i];
        else {
            dexp++;
            if (s[i] != '0')
                sticky = 1;
        }
        i++;
    }
    if (i < len && s[i] == '.') {
        i++;
        while (i < len && s[i] >= '0' && s[i] <= '9') {
            any = 1;
            if (nd == 0 && s[i] == '0') {
                dexp--;
                i++;
                continue;
            }
            if (nd < 780) {
                dig[nd++] = (char)s[i];
                dexp--;
            } else if (s[i] != '0') {
                sticky = 1;
            }
            i++;
        }
    }
    if (!any)
        return 0;
    if (i < len && (s[i] == 'e' || s[i] == 'E')) {
        i++;
        int eneg = 0;
        if (i < len && (s[i] == '+' || s[i] == '-')) {
            eneg = s[i] == '-';
            i++;
        }
        if (i >= len)
            return 0;
        long ev = 0;
        while (i < len && s[i] >= '0' && s[i] <= '9') {
            if (ev < 100000)
                ev = ev * 10 + (s[i] - '0');
            i++;
        }
        dexp += eneg ? -ev : ev;
    }
    if (i != len)
        return 0;
    if (nd == 0) {
        *out = neg ? -0.0 : 0.0;
        return 1;
    }
    int p = is_f32 ? 24 : 53;
    int min_e = is_f32 ? -149 : -1074;
    int max_e = is_f32 ? 104 : 971; /* exponent of the last mantissa bit */
    if (dexp + nd > 330) {
        *out = neg ? -1.0 / 0.0 : 1.0 / 0.0;
        return 1;
    }
    if (dexp + nd < -360) {
        *out = neg ? -0.0 : 0.0;
        return 1;
    }
    /* value = D * 10^dexp = num / den */
    big num, den;
    big_set(&num, 0);
    for (int k = 0; k < nd; k++) {
        big_mul_small(&num, 10);
        big d1;
        big_set(&d1, (uint64_t)(dig[k] - '0'));
        big_add(&num, &d1);
    }
    big_set(&den, 1);
    if (dexp >= 0)
        big_pow10(&num, (int)dexp);
    else
        big_pow10(&den, (int)-dexp);
    /* Scale so that num / den has exactly p + 2 integer bits. */
    int sh = big_bits(&num) - big_bits(&den) - (p + 2);
    if (sh > 0)
        big_shl(&den, sh);
    else
        big_shl(&num, -sh);
    /* e2: value = (num/den) * 2^sh */
    uint64_t q = 0;
    int e2 = sh;
    /* compute q = floor(num/den) bit by bit (at most p + 3 bits) */
    for (;;) {
        big d2 = den;
        big_shl(&d2, p + 2);
        if (big_cmp(&num, &d2) >= 0) {
            big_shl(&den, 1);
            e2++;
            continue;
        }
        big d3 = den;
        big_shl(&d3, p + 1);
        if (big_cmp(&num, &d3) < 0) {
            big_shl(&num, 1);
            e2--;
            continue;
        }
        break;
    }
    for (int b = p + 1; b >= 0; b--) {
        big db = den;
        big_shl(&db, b);
        q <<= 1;
        if (big_cmp(&num, &db) >= 0) {
            big_sub(&num, &db);
            q |= 1;
        }
    }
    if (num.n > 0)
        sticky = 1;
    /* q has p + 2 bits: value = q * 2^e2. Handle subnormals. */
    int shift = 2;
    if (e2 + shift < min_e)
        shift = min_e - e2;
    if (shift > 63) {
        *out = neg ? -0.0 : 0.0;
        return 1;
    }
    uint64_t mant = q >> shift;
    uint64_t lost = q & ((1UL << shift) - 1);
    uint64_t half = 1UL << (shift - 1);
    if (lost > half || (lost == half && (sticky || (mant & 1))))
        mant++;
    int me = e2 + shift;
    if (mant >> p) {
        mant >>= 1;
        me++;
    }
    if (me > max_e) {
        *out = neg ? -1.0 / 0.0 : 1.0 / 0.0;
        return 1;
    }
    double r;
    if (is_f32) {
        union {
            float f;
            uint32_t u;
        } y;
        uint32_t bits;
        if (mant < (1UL << 23))
            bits = (uint32_t)mant;
        else
            bits = ((uint32_t)(me + 150) << 23) | ((uint32_t)mant & ((1u << 23) - 1));
        y.u = bits;
        r = y.f;
    } else {
        union {
            double d;
            uint64_t u;
        } y;
        uint64_t bits;
        if (mant < (1UL << 52))
            bits = mant;
        else
            bits = ((uint64_t)(me + 1075) << 52) | (mant & ((1UL << 52) - 1));
        y.u = bits;
        r = y.d;
    }
    *out = neg ? -r : r;
    return 1;
}
