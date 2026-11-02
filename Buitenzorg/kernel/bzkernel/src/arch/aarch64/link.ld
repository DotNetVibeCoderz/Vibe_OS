/* ARM64 kernel image: linked in the higher half for Limine (executables are
   loaded at or above 0xffffffff80000000). One PT_LOAD per permission.
   .text.rpi_head comes first: flattened to kernel8.img, the file's first
   instruction is the Raspberry Pi entry (arch/aarch64/rpi.rs). */
ENTRY(bz_arm_start)

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
        KEEP(*(.text.rpi_head))
        *(.text .text.*)
    } :text

    . = ALIGN(4096);
    .rodata : {
        *(.rodata .rodata.*)
    } :rodata

    . = ALIGN(4096);
    .data : {
        *(.data .data.*)
    } :data

    __bz_data_end = .;

    .bss : {
        *(.bss .bss.*)
        *(COMMON)
    } :data

    __bz_kernel_end = .;

    /DISCARD/ : {
        *(.eh_frame*)
        *(.note .note.*)
        *(.comment)
    }
}
