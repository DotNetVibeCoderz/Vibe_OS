/* Linker script for Buitenzorg RISC-V user programs (multi-arch M4).
 * Static, no interpreter, same layout as the x86/ARM64 programs (image at
 * 0x400000). No __global_pointer$: gp stays unused (no gp relaxation). */
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
        *(.text._start)
        *(.text .text.*)
        *(.rodata .rodata.* .srodata .srodata.*)
    } :text

    . = ALIGN(0x1000);

    .data : {
        *(.data .data.* .sdata .sdata.*)
        *(.got .got.plt)
    } :data

    .bss : {
        *(.sbss .sbss.* .bss .bss.*)
        *(COMMON)
    } :data

    /DISCARD/ : {
        *(.eh_frame*)
        *(.note.*)
        *(.comment)
        *(.riscv.attributes)
        *(.gnu.hash)
        *(.dynsym .dynstr .dynamic .interp)
    }
}
