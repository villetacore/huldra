#ifndef _SETJMP_H
#define _SETJMP_H
/* Return address, frame pointer and stack pointer (built into hcc). */
typedef long jmp_buf[3];
typedef long sigjmp_buf[3];
int setjmp(jmp_buf env);
void longjmp(jmp_buf env, int val);
#define _setjmp setjmp
#define _longjmp longjmp
#define sigsetjmp(env, save) setjmp(env)
#define siglongjmp longjmp
#endif
