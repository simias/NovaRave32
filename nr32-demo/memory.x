MEMORY
{
    /* The first 128K of the ROM are reserved for the kernel */
    ROM (xr) : ORIGIN = 0x20000000 + 128K, LENGTH = 16M - 128K
    /* The first 32K of RAM are reserved for the kernel */
    RAM (xrw) : ORIGIN = 0x40000000 + 32K, LENGTH = 2M
}

PROVIDE(__estack = ORIGIN(RAM) + LENGTH(RAM));
PROVIDE(__stack_len = 2K);
PROVIDE(__sstack = __estack - __stack_len);

ENTRY(nr32_main);

SECTIONS {
    /* Move everything to RAM for simplicity (and make it more PlayStation-like) */
    .text : ALIGN(4)
    {
        *(.text .text.*);
    } > RAM

    /* Since we don't have virtual memory there's no point in splitting rodata
     * away */
    .data : ALIGN(4)
    {
        . = ALIGN(4);
        /* For GP in order to make some address calculations faster */
        PROVIDE(__global_pointer$ = . + 0x800);

        *(.sdata .sdata.* .sdata2 .sdata2.*);
        *(.srodata .srodata.*);
        *(.data .data.*);
        *(.rodata .rodata.*);
    } > RAM

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
