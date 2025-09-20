INCLUDE ../nr32-common/common.ld

MEMORY
{
    ROM (xr) : ORIGIN = __nr32_rom_base + __nr32_sys_rom_len, LENGTH = __nr32_rom_len - __nr32_sys_rom_len
    RAM (xrw) : ORIGIN = __nr32_ram_base + __nr32_sys_ram_len, LENGTH = __nr32_ram_len - __nr32_sys_ram_len
}

ENTRY(nr32_main);

SECTIONS {
    /* Move everything to RAM for simplicity (and make it more PlayStation-like) */
    .text : ALIGN(4)
    {
        *(.text .text.*);
    } > RAM AT > ROM

    /* Since we don't have virtual memory there's no point in splitting rodata
     * away */
    .data : ALIGN(4)
    {
        . = ALIGN(4);
        /* For GP in order to make some address calculations faster */
        PROVIDE(__global_pointer$ = . + 0x7f0);

        *(.sdata .sdata.* .sdata2 .sdata2.*);
        *(.srodata .srodata.*);
        *(.data .data.*);
        *(.rodata .rodata.*);
    } > RAM AT > ROM

    .bss (NOLOAD) : ALIGN(4)
    {
        . = ALIGN(4);
        *(.sbss .sbss.* .bss .bss.*);
    } > RAM

    /DISCARD/ : {
        /* We don't unwind */
        *(.eh_frame);
        *(.eh_frame_hdr);
    }
}
