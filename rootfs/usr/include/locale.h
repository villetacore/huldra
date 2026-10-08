#ifndef _LOCALE_H
#define _LOCALE_H
#define LC_ALL 6
#define LC_CTYPE 0
#define LC_NUMERIC 1
#define LC_TIME 2
#define LC_COLLATE 3
#define LC_MONETARY 4
#define LC_MESSAGES 5
struct lconv { char *decimal_point; char *thousands_sep; char *grouping; };
char *setlocale(int cat, const char *locale);
struct lconv *localeconv(void);
#endif
