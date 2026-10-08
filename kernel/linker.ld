/* Kernel is loaded and identity-mapped at 1 MiB. */
ENTRY(_start)

SECTIONS
{
    . = 1M;
    __kernel_start = .;

    /* Must stay within the first 32 KiB of the file for GRUB. */
    .multiboot2 : { KEEP(*(.multiboot2)) }
    /* PVH entry note: lets `qemu -kernel` boot the ELF directly. */
    .note.Xen : { KEEP(*(.note.Xen)) }

    .text : ALIGN(16) { *(.text.boot) *(.text .text.*) }

    .rodata : ALIGN(4K) { *(.rodata .rodata.*) }

    .data : ALIGN(4K) { *(.data .data.*) *(.got .got.*) }

    .bss : ALIGN(4K) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(4K);
        __bss_end = .;
    }

    __kernel_end = .;

    /DISCARD/ : { *(.eh_frame) *(.eh_frame_hdr) *(.comment) }
}
