/* The managed-runtime host: a static Buitenzorg user program at 0x400000
 * (the layout of every user program -- x86-64, ARM64 and RISC-V alike). No
 * __global_pointer$: gp stays unused on RISC-V. */
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
    } :text

    .rodata : {
        *(.rodata .rodata.* .srodata .srodata.*)
    } :text

    . = ALIGN(0x1000);

    .data : {
        *(.data .data.* .sdata .sdata.*)
        *(.got .got.plt)
        *(.init_array .init_array.*)
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
