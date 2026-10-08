# Low-level kernel entry points: interrupt/exception stubs, the common
# trap path, context switching and task start trampolines.
#
# Every stub pushes (error code, vector) so isr_common can build a uniform
# TrapFrame (see trap.rs) and call trap_dispatch(frame). GS is swapped when
# entering from or returning to user mode (per-CPU data lives in GS).

.section .text
.code64

.macro ISR_NOERR num
isr_stub_\num:
    push 0
    push \num
    jmp isr_common
.endm

.macro ISR_ERR num
isr_stub_\num:
    push \num
    jmp isr_common
.endm
.irp num, 0,1,2,3,4,5,6,7,9,15,16,18,19,20,22,23,24,25,26,27,28,31,32,33
ISR_NOERR \num
.endr
.irp num, 34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57
ISR_NOERR \num
.endr
.irp num, 58,59,60,61,62,63,64,65,66,67,68,69,70,71,72,73,74,75,76,77,78,79,80,81
ISR_NOERR \num
.endr
.irp num, 82,83,84,85,86,87,88,89,90,91,92,93,94,95,96,97,98,99,100,101,102,103,104,105
ISR_NOERR \num
.endr
.irp num, 106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,126,127,128,129
ISR_NOERR \num
.endr
.irp num, 130,131,132,133,134,135,136,137,138,139,140,141,142,143,144,145,146,147,148,149,150,151,152,153
ISR_NOERR \num
.endr
.irp num, 154,155,156,157,158,159,160,161,162,163,164,165,166,167,168,169,170,171,172,173,174,175,176,177
ISR_NOERR \num
.endr
.irp num, 178,179,180,181,182,183,184,185,186,187,188,189,190,191,192,193,194,195,196,197,198,199,200,201
ISR_NOERR \num
.endr
.irp num, 202,203,204,205,206,207,208,209,210,211,212,213,214,215,216,217,218,219,220,221,222,223,224,225
ISR_NOERR \num
.endr
.irp num, 226,227,228,229,230,231,232,233,234,235,236,237,238,239,240,241,242,243,244,245,246,247,248,249
ISR_NOERR \num
.endr
.irp num, 250,251,252,253,254,255
ISR_NOERR \num
.endr
.irp num, 8,10,11,12,13,14,17,21,29,30
ISR_ERR \num
.endr

isr_common:
    # [rsp] = vector, [rsp+8] = error, [rsp+16] = rip, [rsp+24] = cs
    test qword ptr [rsp + 24], 3
    jz .Lisr_from_kernel
    swapgs
.Lisr_from_kernel:
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

# Restores a TrapFrame at rsp and returns to the interrupted context.
.global trap_return
trap_return:
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
    test qword ptr [rsp + 8], 3     # returning to user mode?
    jz .Lreturn_to_kernel
    swapgs
.Lreturn_to_kernel:
    iretq

# switch_context(save_rsp: *mut u64, new_rsp: u64)
# Saves callee-saved state on the current stack, switches stacks and
# restores the next task's state. RFLAGS travels with the task.
.global switch_context
switch_context:
    pushfq
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15
    mov [rdi], rsp
    mov rsp, rsi
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    popfq
    ret

# First code run by a new kernel thread: r12 = argument, r13 = entry.
.global kthread_trampoline
kthread_trampoline:
    sti
    mov rdi, r12
    call r13
    mov edi, eax
    call kthread_exit
    ud2

# First code run by a new (or forked) user task: a TrapFrame describing
# the user context sits at the top of its kernel stack.
.global user_trampoline
user_trampoline:
    jmp trap_return

.section .rodata
.balign 8
.global isr_stub_table
isr_stub_table:
.irp num, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23
    .quad isr_stub_\num
.endr
.irp num, 24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47
    .quad isr_stub_\num
.endr
.irp num, 48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63,64,65,66,67,68,69,70,71
    .quad isr_stub_\num
.endr
.irp num, 72,73,74,75,76,77,78,79,80,81,82,83,84,85,86,87,88,89,90,91,92,93,94,95
    .quad isr_stub_\num
.endr
.irp num, 96,97,98,99,100,101,102,103,104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119
    .quad isr_stub_\num
.endr
.irp num, 120,121,122,123,124,125,126,127,128,129,130,131,132,133,134,135,136,137,138,139,140,141,142,143
    .quad isr_stub_\num
.endr
.irp num, 144,145,146,147,148,149,150,151,152,153,154,155,156,157,158,159,160,161,162,163,164,165,166,167
    .quad isr_stub_\num
.endr
.irp num, 168,169,170,171,172,173,174,175,176,177,178,179,180,181,182,183,184,185,186,187,188,189,190,191
    .quad isr_stub_\num
.endr
.irp num, 192,193,194,195,196,197,198,199,200,201,202,203,204,205,206,207,208,209,210,211,212,213,214,215
    .quad isr_stub_\num
.endr
.irp num, 216,217,218,219,220,221,222,223,224,225,226,227,228,229,230,231,232,233,234,235,236,237,238,239
    .quad isr_stub_\num
.endr
.irp num, 240,241,242,243,244,245,246,247,248,249,250,251,252,253,254,255
    .quad isr_stub_\num
.endr

.section .text
