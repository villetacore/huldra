# Interrupt entry stubs. Every stub pushes (error code, vector) so that
# isr_common builds a uniform TrapFrame and calls trap_dispatch(frame).

.section .text
.code64

.macro ISR_NOERR num
.global isr_stub_\num
isr_stub_\num:
    push 0
    push \num
    jmp isr_common
.endm

.macro ISR_ERR num
.global isr_stub_\num
isr_stub_\num:
    push \num
    jmp isr_common
.endm

.irp num, 0,1,2,3,4,5,6,7,9,15,16,18,19,20,22,23,24,25,26,27,28,31
ISR_NOERR \num
.endr

.irp num, 8,10,11,12,13,14,17,21,29,30
ISR_ERR \num
.endr

.irp num, 32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47
ISR_NOERR \num
.endr

ISR_NOERR 128

isr_common:
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    mov rdi, rsp
    cld
    call trap_dispatch

    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax
    add rsp, 16                     # vector + error code
    iretq

.section .rodata
.balign 8
.global isr_stub_table
isr_stub_table:
.irp num, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47
    .quad isr_stub_\num
.endr

.section .text
