MEMORY
{
    RAM (xrw) : ORIGIN = 0x40000000, LENGTH = 2M
    ROM (xrw) : ORIGIN = 0x20000000, LENGTH = 2M
}

PROVIDE(__sstack= ORIGIN(RAM) + LENGTH(RAM));
/* The system stack is only used at bootup and for the idle task, no point in
 * wasting too much RAM */
PROVIDE(__stack_len = 1K);

SECTIONS {
    .text :
    {
        KEEP(*(.init));
        . = ALIGN(4);
        *(.text .text.*)
    } > ROM

    .rodata : ALIGN(4)
    {
        . = ALIGN(4);
        *(.srodata .srodata.*);
        *(.rodata .rodata.*);
    } > ROM

    .data : ALIGN(4)
    {
        . = ALIGN(4);
        /* Start of data section in RAM */
        PROVIDE(__sdata = .);
        /* For GP in order to make some address calculations faster */
        PROVIDE(__global_pointer$ = . + 0x800);

        *(.sdata .sdata.* .sdata2 .sdata2.*);
        *(.data .data.*);
    } > RAM AT > ROM

    . = ALIGN(4);
    /* End of data section in RAM */
    PROVIDE(__edata = .);
    /* Start data section in ROM pre-relocation */
    PROVIDE(__sdata_rom = LOADADDR(.data));

    .bss (NOLOAD) : ALIGN(4)
    {
        . = ALIGN(4);
        PROVIDE(__sbss = .);

        *(.sbss .sbss.* .bss .bss.*);
    } > RAM

    . = ALIGN(4);
    PROVIDE(__ebss = .);
    PROVIDE(__sheap = .);

    /* Reserve space for the stack */
    .stack (NOLOAD) :
    {
    PROVIDE(__estack = .);
    . = ABSOLUTE(__sstack);
    PROVIDE(__eheap = .);
    __estack = .;
    } > RAM
}

_hart_stack_size = 1K;
_heap_size = 1M;
