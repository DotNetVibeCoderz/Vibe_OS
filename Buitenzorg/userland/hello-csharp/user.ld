/* Linker script for Buitenzorg ring-3 user programs (v0.4 "Tunas").
 * Static, no interpreter; loaded by the kernel ELF loader at a fixed base
 * well below the kernel's higher-half mapping. */
ENTRY(_start)

PHDRS
{
    text PT_LOAD FLAGS(5);   /* R-X */
    data PT_LOAD FLAGS(6);   /* RW- */
    /* ILC emits an (empty) thread-static anchor in .tdata; the linker wants a
     * PT_TLS segment for it. Thread statics are not supported (the loader
     * ignores this segment). */
    tls PT_TLS;
}

SECTIONS
{
    . = 0x400000;

    .text : {
        *(.text .text.*)
        *(__managedcode)
        *(.rodata .rodata.*)
        *(.dotnet_eh_table)
    } :text

    /* unwind tables: the exception dispatcher (bzeh.rs) walks them */
    .eh_frame : {
        __bz_eh_frame_start = .;
        KEEP(*(.eh_frame))
        __bz_eh_frame_end = .;
        LONG(0)
    } :text

    . = ALIGN(0x1000);

    /* the collector (bzgc.rs) scans __bz_data_start .. __bz_bss_end for roots */
    .data : {
        __bz_data_start = .;
        *(.data .data.*)
        *(__modules)
        *(.got .got.plt)
    } :data

    .tdata : { *(.tdata .tdata.*) } :data :tls
    .tbss : { *(.tbss .tbss.*) } :data :tls

    .bss : {
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(8);
        __bz_bss_end = .;
    } :data

    /DISCARD/ : {
        *(.eh_frame_hdr)
        *(.note.*)
        *(.comment)
        *(.gnu.hash)
        *(.dynsym .dynstr .dynamic .interp)
    }
}
