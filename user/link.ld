/* Static user programs are loaded at 4 MiB. */
ENTRY(_start)

SECTIONS
{
    . = 0x400000 + SIZEOF_HEADERS;

    .text : { *(.text .text.*) }

    . = ALIGN(4K);
    .rodata : { *(.rodata .rodata.*) }

    . = ALIGN(4K);
    .data : { *(.data .data.*) *(.got .got.*) }
    .bss : { *(.bss .bss.*) *(COMMON) }

    /DISCARD/ : { *(.eh_frame) *(.eh_frame_hdr) *(.comment) }
}
