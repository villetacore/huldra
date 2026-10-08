#ifndef _STDDEF_H
#define _STDDEF_H
#define NULL ((void *)0)
typedef unsigned long size_t;
typedef long ssize_t;
typedef long ptrdiff_t;
typedef int wchar_t;
typedef long max_align_t;
#define offsetof(T, m) __builtin_offsetof(T, m)
#endif
