/*
 * Huldra kernel layout.
 *
 * The boot stub (.boot.*) runs at its physical address (1 MiB). Everything
 * else is linked in the higher half at KERNEL_VMA + physical address and
 * loaded right after the boot stub (AT = physical load address).
 */
ENTRY(_start)

KERNEL_VMA = 0xFFFFFFFF80000000;

SECTIONS
{
    . = 1M;

    /* Must stay within the first 32 KiB of the file for GRUB. */
    .multiboot2 : { KEEP(*(.multiboot2)) }
    /* PVH entry note: lets `qemu -kernel` boot the ELF directly. */
    .note.Xen : { KEEP(*(.note.Xen)) }

    .boot.text : { *(.boot.text) }
    .boot.data : { *(.boot.data) }
    .boot.bss (NOLOAD) : ALIGN(4K) {
        __boot_bss_start = .;
        *(.boot.bss)
        . = ALIGN(4K);
        __boot_bss_end = .;
    }

    . = ALIGN(4K);
    . += KERNEL_VMA;
    __kernel_start = .;

    .text : AT(ADDR(.text) - KERNEL_VMA) {
        __text_start = .;
        *(.text .text.*)
        . = ALIGN(4K);
        __text_end = .;
    }

    .rodata : AT(ADDR(.rodata) - KERNEL_VMA) {
        __rodata_start = .;
        *(.rodata .rodata.*)
        . = ALIGN(4K);
        __rodata_end = .;
    }

    .data : AT(ADDR(.data) - KERNEL_VMA) {
        __data_start = .;
        *(.data .data.*)
        *(.got .got.*)
    }

    .bss (NOLOAD) : AT(ADDR(.bss) - KERNEL_VMA) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(4K);
        __bss_end = .;
    }

    __kernel_end = .;

    /DISCARD/ : { *(.eh_frame) *(.eh_frame_hdr) *(.comment) }
}
