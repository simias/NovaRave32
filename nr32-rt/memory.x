MEMORY
{
    ROM (xr) : ORIGIN = 0x20000000, LENGTH = 128K
    RAM (xrw) : ORIGIN = 0x40000000, LENGTH = 32K
}

PROVIDE(__stack_len = 2K);

SECTIONS {
    .text.init : ALIGN(4)
    {
        . = . + 0x100;
        KEEP(*(.init));
    } > ROM

    .text.fast : ALIGN(4)
    {
        . = ALIGN(4);
        /* Start of data section in RAM */
        PROVIDE(__s_ram_copy = .);
        KEEP(*(.trap_handler));
        *(.text.*memcpy*);
        *(.text.*memset*);
        *(.text.*memset*);
        *(.text.*compiler_builtins*);
        *(.text.*__div*);
        *(.text.*__udiv*);
        *(.text.fast)
    } > RAM AT > ROM

    . = ALIGN(4);
    /* End of copy section in RAM */
    PROVIDE(__e_ram_copy = .);
    /* Start of copy section in ROM pre-relocation */
    PROVIDE(__s_rom_copy = LOADADDR(.text.fast));

    .text :
    {
        . = ALIGN(4);
        *(.text .text.*)
    } > ROM

    .rodata :
    {
        . = ALIGN(4);
        *(.srodata .srodata.*);
        *(.rodata .rodata.*);
    } > ROM

    .data :
    {
        . = ALIGN(4);
        /* For GP in order to make some address calculations faster */
        PROVIDE(__global_pointer$ = . + 0x800);

        *(.sdata .sdata.* .sdata2 .sdata2.*);
        *(.data .data.*);
    } > RAM AT > ROM

    .bss (NOLOAD) :
    {
        . = ALIGN(4);
        PROVIDE(__sbss = .);

        *(.sbss .sbss.* .bss .bss.*);
    } > RAM

    PROVIDE(__ebss = .);

    /* System stack */
    .stack (NOLOAD) :
    {
        . = ALIGN(16);
        PROVIDE(__sstack = .);
        . = . + 2K;
        PROVIDE(__estack = .);
        PROVIDE(__sheap = .);
        PROVIDE(__eheap = ORIGIN(RAM) + 2M);
    }

    /DISCARD/ : {
        /* We don't unwind */
        *(.eh_frame);
        *(.eh_frame_hdr);
    }
}

_hart_stack_size = 1K;
_heap_size = 1M;
