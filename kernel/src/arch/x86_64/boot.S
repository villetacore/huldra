# Huldra boot code.
#
# Entered in 32-bit protected mode with paging disabled, either by a
# Multiboot2 loader (GRUB: EAX = magic, EBX = info) or through the PVH
# entry point (QEMU -kernel: EBX = hvm_start_info).
#
# Boot page tables map the first 4 GiB three times:
#   identity            (PML4[0])    - so this code keeps running
#   direct map          (PML4[256])  - PHYS_OFFSET + phys, 0..4 GiB
#   kernel image        (PML4[511])  - KERNEL_VMA + phys, 0..1 GiB
# The kernel replaces them with its own tables early in kernel_main.


# ---------------------------------------------------------------- Multiboot2
.section .multiboot2, "a", @progbits
.balign 8
mb2_header:
    .long 0xE85250D6                                  # magic
    .long 0                                           # architecture: i386
    .long mb2_header_end - mb2_header                 # header length
    .long 0x100000000 - (0xE85250D6 + (mb2_header_end - mb2_header))
    .short 0                                          # end tag
    .short 0
    .long 8
mb2_header_end:

# ---------------------------------------------------------------- PVH note
.section .note.Xen, "a", @note
.balign 4
    .long 4                                           # namesz
    .long 8                                           # descsz
    .long 18                                          # XEN_ELFNOTE_PHYS32_ENTRY
    .asciz "Xen"
    .balign 4
    .quad _start
    .balign 4

# ---------------------------------------------------------------- 32-bit entry
.section .boot.text, "ax", @progbits
.code32
.global _start
_start:
    cli
    cld
    mov ebp, eax                    # boot magic
    mov esi, ebx                    # boot info (physical)

    # Zero the low boot .bss (page tables, early stack).
    mov edi, offset __boot_bss_start
    mov ecx, offset __boot_bss_end
    sub ecx, edi
    shr ecx, 2
    xor eax, eax
    rep stosd

    mov esp, offset boot_stack32_top

    # Long mode supported?
    mov eax, 0x80000000
    cpuid
    cmp eax, 0x80000001
    jb .Lno_long_mode
    mov eax, 0x80000001
    cpuid
    test edx, 0x20000000
    jz .Lno_long_mode

    # PML4 -> PDPTs
    mov eax, offset boot_pdpt_low
    or eax, 3
    mov dword ptr [boot_pml4], eax              # identity
    mov dword ptr [boot_pml4 + 256 * 8], eax    # direct map
    mov eax, offset boot_pdpt_high
    or eax, 3
    mov dword ptr [boot_pml4 + 511 * 8], eax    # kernel image

    # Low PDPT: four page directories covering 0..4 GiB
    mov eax, offset boot_pd
    or eax, 3
    mov dword ptr [boot_pdpt_low], eax
    mov dword ptr [boot_pdpt_high + 510 * 8], eax   # -2 GiB -> 0..1 GiB
    add eax, 0x1000
    mov dword ptr [boot_pdpt_low + 8], eax
    add eax, 0x1000
    mov dword ptr [boot_pdpt_low + 16], eax
    add eax, 0x1000
    mov dword ptr [boot_pdpt_low + 24], eax

    # 2048 x 2 MiB pages: present | writable | huge
    mov edi, offset boot_pd
    xor ecx, ecx
.Lmap_pd:
    mov eax, ecx
    shl eax, 21
    or eax, 0x83
    mov dword ptr [edi + ecx*8], eax
    mov dword ptr [edi + ecx*8 + 4], 0
    inc ecx
    cmp ecx, 2048
    jne .Lmap_pd

    mov eax, offset boot_pml4
    mov cr3, eax

    mov eax, cr4
    or eax, 0x20                    # CR4.PAE
    mov cr4, eax

    mov eax, 0x80000001
    cpuid
    mov edi, edx                    # extended features
    mov ecx, 0xC0000080             # IA32_EFER
    rdmsr
    or eax, 0x100                   # EFER.LME
    test edi, 0x100000              # NX supported?
    jz .Lno_nx
    or eax, 0x800                   # EFER.NXE
.Lno_nx:
    wrmsr

    mov eax, cr0
    or eax, 0x80010001              # CR0.PG | CR0.WP | CR0.PE
    mov cr0, eax

    lgdt [boot_gdt_ptr]
    push 0x08
    mov eax, offset long_mode_low
    push eax
    retf

.Lno_long_mode:
    mov dword ptr [0xB8000], 0x4F4F4F4E   # "NO64" in white on red
    mov dword ptr [0xB8004], 0x4F344F36
.Lhalt32:
    hlt
    jmp .Lhalt32

.code64
long_mode_low:
    movabs rax, offset long_mode_high
    jmp rax

# ---------------------------------------------------------------- boot data
.section .boot.data, "aw", @progbits
.balign 8
boot_gdt:
    .quad 0
    .quad 0x00AF9A000000FFFF        # 64-bit kernel code
    .quad 0x00CF92000000FFFF        # kernel data
boot_gdt_end:
boot_gdt_ptr:
    .short boot_gdt_end - boot_gdt - 1
    .long boot_gdt

.section .boot.bss, "aw", @nobits
.balign 4096
boot_pml4:      .skip 4096
boot_pdpt_low:  .skip 4096
boot_pdpt_high: .skip 4096
boot_pd:        .skip 4096 * 4
                .skip 256
boot_stack32_top:

# ---------------------------------------------------------------- higher half
.section .text
.code64
long_mode_high:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    xor eax, eax
    mov fs, ax
    mov gs, ax

    # Zero .bss (the boot stack lives there, nothing is on it yet).
    lea rdi, [rip + __bss_start]
    lea rcx, [rip + __bss_end]
    sub rcx, rdi
    shr rcx, 3
    xor eax, eax
    rep stosq

    lea rsp, [rip + boot_stack_top]
    mov edi, ebp                    # arg0: magic (zero-extended)
    mov esi, esi                    # arg1: info  (physical, zero-extended)
    xor ebp, ebp
    call kernel_main
.Lhalt64:
    cli
    hlt
    jmp .Lhalt64

.section .bss
.balign 4096
.global boot_stack_bottom
boot_stack_bottom:
    .skip 65536
.global boot_stack_top
boot_stack_top:

.section .text
