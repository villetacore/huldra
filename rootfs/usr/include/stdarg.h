#ifndef _STDARG_H
#define _STDARG_H
/* hcc passes every argument in an 8-byte stack slot, so va_list is just
   a pointer walking the slots. */
typedef char *va_list;
typedef char *__gnuc_va_list;
#define __va_slot(T) ((sizeof(T) + 7) & ~7)
#define va_start(ap, last) ((ap) = (char *)&(last) + __va_slot(last))
#define va_arg(ap, T) (*(T *)(((ap) += __va_slot(T)) - __va_slot(T)))
#define va_end(ap) ((void)0)
#define va_copy(d, s) ((d) = (s))
#endif
