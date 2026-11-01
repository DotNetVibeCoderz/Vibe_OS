/* RISC-V kernel image: linked in the higher half for Limine (executables are
   loaded at or above 0xffffffff80000000). One PT_LOAD per permission. The
   small-data sections are listed explicitly; no __global_pointer$ is
   defined, so the linker never relaxes accesses to gp. */
ENTRY(bz_riscv_start)

PHDRS
{
    text   PT_LOAD FLAGS(5);  /* R-X */
    rodata PT_LOAD FLAGS(4);  /* R-- */
    data   PT_LOAD FLAGS(6);  /* RW- */
}

SECTIONS
{
    . = 0xffffffff80000000;

    .text : {
        *(.text .text.*)
    } :text

    . = ALIGN(4096);
    .rodata : {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    } :rodata

    . = ALIGN(4096);
    .data : {
        *(.data .data.*)
        *(.sdata .sdata.*)
    } :data

    .bss : {
        *(.sbss .sbss.*)
        *(.bss .bss.*)
        *(COMMON)
    } :data

    /DISCARD/ : {
        *(.eh_frame*)
        *(.note .note.*)
        *(.comment)
        *(.riscv.attributes)
    }
}
