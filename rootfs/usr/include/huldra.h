#ifndef _HULDRA_H
#define _HULDRA_H
/* Raw system calls: __syscall(number, args...) is built into hcc. */
long syscall(long nr, ...);
#endif
