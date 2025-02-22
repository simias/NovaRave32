use core::arch::global_asm;

// Entry point. This is the first thing executed by the CPU.
global_asm!(
    ".section .init, \"ax\"
    .global _start
_start:
    .cfi_startproc
    .cfi_undefined ra

    /* Disable all interrupts */
    csrw    mie, 0

    /* Setup GP */
    .option push
    .option norelax
    la      gp, __global_pointer$
    .option pop

    /* Setup SP to the top of the system stack space */
    la      t0, __estack
    /* 16-byte aligned */
    andi    sp, t0, -16

    /* Copy .data from ROM to RAM */
    la      t0, __sdata
    la      t1, __edata
    bgeu    t0, t1, .data_copy_done
    la      t2, __sdata_rom

.data_copy_word:
    lw      t3, 0(t2)
    addi    t2, t2, 4
    sw      t3, 0(t0)
    add     t0, t0, 4
    bltu    t0, t1, .data_copy_word

.data_copy_done:

    /* Zero-out BSS */
    la      t0, __sbss
    la      t1, __ebss
    bgeu    t0, t1, .bss_zero_done

.bss_zero_loop:
    sw      zero, 0(t0)
    add     t0, t0, 4
    bltu    t0, t1, .bss_zero_loop

.bss_zero_done:

    jal     _system_entry

    /* Should not be reached */
1:
    wfi
    j       1b;

    .cfi_endproc
    "
);
