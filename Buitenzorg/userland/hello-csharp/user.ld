/* Linker script for Buitenzorg ring-3 user programs (v0.4 "Tunas").
 * Static, no interpreter; loaded by the kernel ELF loader at a fixed base
 * well below the kernel's higher-half mapping. */
ENTRY(_start)

PHDRS
{
    text PT_LOAD FLAGS(5);   /* R-X */
    data PT_LOAD FLAGS(6);   /* RW- */
}

SECTIONS
{
    . = 0x400000;

    .text : {
        *(.text .text.*)
        *(.rodata .rodata.*)
    } :text

    . = ALIGN(0x1000);

    .data : {
        *(.data .data.*)
        *(.got .got.plt)
    } :data

    .bss : {
        *(.bss .bss.*)
        *(COMMON)
    } :data

    /DISCARD/ : {
        *(.eh_frame*)
        *(.note.*)
        *(.comment)
        *(.gnu.hash)
        *(.dynsym .dynstr .dynamic .interp)
    }
}
