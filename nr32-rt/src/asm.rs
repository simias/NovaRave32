use core::arch::global_asm;

#[repr(u32)]
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub enum PrivilegeMode {
    Kernel = 0,
    User = 1,
}

// Entry point. This is the first thing executed by the CPU.
global_asm!(
    ".section .init, \"ax\"
    .global _start
_start:
    .cfi_startproc
    .cfi_undefined ra

    /* Disable all interrupts */
    csrw    mie, 0

    /* Setup SP to the top of the system stack space */
    la      t0, __estack
    /* 16-byte aligned */
    andi    sp, t0, -16

    /* Setup trap handler */
    la      t0, _trap_handler
    csrw    mtvec, t0

    /* Copy .data from ROM to RAM */
    la      t0, __s_ram_copy
    la      t1, __e_ram_copy
    bgeu    t0, t1, .data_copy_done
    la      t2, __s_rom_copy

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

    fence.i

    la      t0, _system_entry
    jalr    t0

    la      t0, __return_to_user
    jr      t0

    .cfi_endproc
    "
);

// Trap handler (not vectored)
global_asm!(
    ".section .trap_handler, \"ax\"
    .global _trap_handler
_trap_handler:
    .cfi_startproc
    .cfi_undefined ra

    /* Allocate stack space to potentially bank all registers */
    add     sp, sp, -(32 * 4)

    .option push
    /* Not sure why I need to do this, otherwise the assembler seems to believe that we don't
     * support A even though we use an imac toolchain? */
    .option arch, +a

    /* Do a dummy SWC to clear any reservation */
    sc.w    zero, zero, (sp)
    .option pop

    /* See if this is an ECALL from U-Mode (MCAUSE = 8). If it's the case we don't have to bank
     * anything since this works like a normal C function call. */
    sw      s0, (24 * 4)(sp)
    csrr    s0, mcause
    addi    s0, s0, -8
    bnez    s0, .L_not_ecall

    /* Swap system stack in, but save caller SP in case the caller gets preempted */
    mv      s0, sp
    csrrw   sp, mscratch, sp

    jal     _system_ecall

    srl     t0, a0, 16
    bnez    t0, .L_task_changed

    /* Same task, we can return immediately */
    csrrw   sp, mscratch, sp

    lw      s0, (24 * 4)(sp)
    add     sp, sp, (32 * 4)
    mret

.L_task_changed:
    /* Clear the high bits of A0 since the caller shouldn't see them */
    sll     a0, a0, 16
    srl     a0, a0, 16

    addi    t0, t0, -1

    /* Previous task has been killed, we don't have to worry about its registers */
    bnez    t0, __return_to_user

    /* Previous task has been preempted, save its registers */

    /* Retrieve previous task SP */
    mv      t0, sp
    mv      sp, s0

    /* We only have to save callee-preserved registers + a0 (syscall result) and a1 (syscall return value)*/
    sw      gp,  (29 * 4)(sp)
    sw      tp,  (28 * 4)(sp)
    /* S0 banked above */
    sw      s1,  (23 * 4)(sp)
    sw      a0,  (22 * 4)(sp)
    sw      a1,  (21 * 4)(sp)
    sw      s2,  (14 * 4)(sp)
    sw      s3,  (13 * 4)(sp)
    sw      s4,  (12 * 4)(sp)
    sw      s5,  (11 * 4)(sp)
    sw      s6,  (10 * 4)(sp)
    sw      s7,  (9 * 4)(sp)
    sw      s8,  (8 * 4)(sp)
    sw      s9,  (7 * 4)(sp)
    sw      s10, (6 * 4)(sp)
    sw      s11, (5 * 4)(sp)

    /* Return to sys stack */
    mv      sp, t0

    j       __return_to_user

.L_not_ecall:

    /* We may preempt so we save everything except ZERO and SP. */

    sw      ra,  (31 * 4)(sp)
    /* Skip SP */
    sw      gp,  (29 * 4)(sp)
    sw      tp,  (28 * 4)(sp)
    sw      t0,  (27 * 4)(sp)
    sw      t1,  (26 * 4)(sp)
    sw      t2,  (25 * 4)(sp)
    /* S0 banked above */
    sw      s1,  (23 * 4)(sp)
    sw      a0,  (22 * 4)(sp)
    sw      a1,  (21 * 4)(sp)
    sw      a2,  (20 * 4)(sp)
    sw      a3,  (19 * 4)(sp)
    sw      a4,  (18 * 4)(sp)
    sw      a5,  (17 * 4)(sp)
    sw      a6,  (16 * 4)(sp)
    sw      a7,  (15 * 4)(sp)
    sw      s2,  (14 * 4)(sp)
    sw      s3,  (13 * 4)(sp)
    sw      s4,  (12 * 4)(sp)
    sw      s5,  (11 * 4)(sp)
    sw      s6,  (10 * 4)(sp)
    sw      s7,  (9 * 4)(sp)
    sw      s8,  (8 * 4)(sp)
    sw      s9,  (7 * 4)(sp)
    sw      s10, (6 * 4)(sp)
    sw      s11, (5 * 4)(sp)
    sw      t3,  (4 * 4)(sp)
    sw      t4,  (3 * 4)(sp)
    sw      t5,  (2 * 4)(sp)
    sw      t6,  (1 * 4)(sp)

    /* Swap system stack in */
    csrrw   sp, mscratch, sp

    jal     _system_trap

    .global __return_to_user
__return_to_user:

    /* Swap to task stack */
    csrrw   sp, mscratch, sp

    lw      ra,  (31 * 4)(sp)
    /* Skip x2 = SP */
    lw      gp,  (29 * 4)(sp)
    lw      tp,  (28 * 4)(sp)
    lw      t0,  (27 * 4)(sp)
    lw      t1,  (26 * 4)(sp)
    lw      t2,  (25 * 4)(sp)
    lw      s0,  (24 * 4)(sp)
    lw      s1,  (23 * 4)(sp)
    lw      a0,  (22 * 4)(sp)
    lw      a1,  (21 * 4)(sp)
    lw      a2,  (20 * 4)(sp)
    lw      a3,  (19 * 4)(sp)
    lw      a4,  (18 * 4)(sp)
    lw      a5,  (17 * 4)(sp)
    lw      a6,  (16 * 4)(sp)
    lw      a7,  (15 * 4)(sp)
    lw      s2,  (14 * 4)(sp)
    lw      s3,  (13 * 4)(sp)
    lw      s4,  (12 * 4)(sp)
    lw      s5,  (11 * 4)(sp)
    lw      s6,  (10 * 4)(sp)
    lw      s7,  (9 * 4)(sp)
    lw      s8,  (8 * 4)(sp)
    lw      s9,  (7 * 4)(sp)
    lw      s10, (6 * 4)(sp)
    lw      s11, (5 * 4)(sp)
    lw      t3,  (4 * 4)(sp)
    lw      t4,  (3 * 4)(sp)
    lw      t5,  (2 * 4)(sp)
    lw      t6,  (1 * 4)(sp)

    add     sp, sp, (32 * 4)

    mret

    .cfi_endproc
    "
);

// Task running when there's nothing else to do.
//
// This task will not be given a proper stack, so it can't push or pop anything, or call anything
// for that matter, hence the use of assembly to prevent bad surprises at lower optim levels.
global_asm!(
    ".section .text, \"ax\"
    .global _idle_task
_idle_task:
    .cfi_startproc

1:
    wfi
    j       1b

    .cfi_endproc
    "
);

// Trampoline when spawning a task that takes care of calling SYS_EXIT once it returns
//
// The task start address is in a1. The task's data argument is in a0.
global_asm!(
    ".section .text, \"ax\"
    .global _task_runner
_task_runner:
    .cfi_startproc
    .cfi_undefined ra

    jalr    a1
    li      a7, 0x04
    ecall

    .cfi_endproc
    "
);

unsafe extern "C" {
    pub fn _idle_task();
    pub fn _task_runner() -> !;
}
