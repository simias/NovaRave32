use core::arch::global_asm;

#[repr(u32)]
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub enum PrivilegeMode {
    Kernel = 0,
    User = 1,
}

/// To keep track of what more we're in since RISCV does not let us simply check the current
/// privilege mode
#[no_mangle]
pub static mut _CURRENT_MODE: PrivilegeMode = PrivilegeMode::Kernel;

pub fn current_mode() -> PrivilegeMode {
    unsafe { _CURRENT_MODE }
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

    /* Setup GP */
    .option push
    .option norelax
    la      gp, __global_pointer$
    .option pop

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

    /* Switch mode to kernel */
    la      s0, _CURRENT_MODE
    sw      zero, 0(s0)

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

    /* We may preempt so we save everything except ZERO, GP and SP.
     *
     * Later we could special-case some hardware interrupts to be handled faster if we don't
     * preempt them
     */
    add     sp, sp, -(32 * 4)

    .option push
    /* Not sure why I need to do this, otherwise the assembler seems to believe that we don't
     * support A even though we use an imac toolchain? */
    .option arch, +a

    /* Do a dummy SWC to clear any reservation */
    sc.w    zero, zero, (sp)
    .option pop

    sw      x1,  (31 * 4)(sp)
    /* Skip x2 = SP */
    sw      x3,  (30 * 4)(sp)
    sw      x4,  (29 * 4)(sp)
    sw      x5,  (28 * 4)(sp)
    sw      x6,  (27 * 4)(sp)
    sw      x7,  (26 * 4)(sp)
    sw      x8,  (25 * 4)(sp)
    sw      x9,  (24 * 4)(sp)
    sw      x10, (23 * 4)(sp)
    sw      x11, (22 * 4)(sp)
    sw      x12, (21 * 4)(sp)
    sw      x13, (20 * 4)(sp)
    sw      x14, (19 * 4)(sp)
    sw      x15, (18 * 4)(sp)
    sw      x16, (17 * 4)(sp)
    sw      x17, (16 * 4)(sp)
    sw      x18, (15 * 4)(sp)
    sw      x19, (14 * 4)(sp)
    sw      x20, (13 * 4)(sp)
    sw      x21, (12 * 4)(sp)
    sw      x22, (11 * 4)(sp)
    sw      x23, (10 * 4)(sp)
    sw      x24, (9 * 4)(sp)
    sw      x25, (8 * 4)(sp)
    sw      x26, (7 * 4)(sp)
    sw      x27, (6 * 4)(sp)
    sw      x28, (5 * 4)(sp)
    sw      x29, (4 * 4)(sp)
    sw      x30, (3 * 4)(sp)
    sw      x31, (2 * 4)(sp)

    /* Swap system stack in */
    csrrw   sp, mscratch, sp

    /* Switch mode to kernel */
    la      s0, _CURRENT_MODE
    sw      zero, 0(s0)

    jal     _system_trap

    .global __return_to_user
__return_to_user:

    /* Switch mode to user */
    li      t1, 1
    sw      t1, 0(s0)

    /* Swap to task stack */
    csrrw   sp, mscratch, sp

    lw      x1, (31 * 4)(sp)
    /* Skip x2 = SP */
    lw      x3,  (30 * 4)(sp)
    lw      x4,  (29 * 4)(sp)
    lw      x5,  (28 * 4)(sp)
    lw      x6,  (27 * 4)(sp)
    lw      x7,  (26 * 4)(sp)
    lw      x8,  (25 * 4)(sp)
    lw      x9,  (24 * 4)(sp)
    lw      x10, (23 * 4)(sp)
    lw      x11, (22 * 4)(sp)
    lw      x12, (21 * 4)(sp)
    lw      x13, (20 * 4)(sp)
    lw      x14, (19 * 4)(sp)
    lw      x15, (18 * 4)(sp)
    lw      x16, (17 * 4)(sp)
    lw      x17, (16 * 4)(sp)
    lw      x18, (15 * 4)(sp)
    lw      x19, (14 * 4)(sp)
    lw      x20, (13 * 4)(sp)
    lw      x21, (12 * 4)(sp)
    lw      x22, (11 * 4)(sp)
    lw      x23, (10 * 4)(sp)
    lw      x24, (9 * 4)(sp)
    lw      x25, (8 * 4)(sp)
    lw      x26, (7 * 4)(sp)
    lw      x27, (6 * 4)(sp)
    lw      x28, (5 * 4)(sp)
    lw      x29, (4 * 4)(sp)
    lw      x30, (3 * 4)(sp)
    lw      x31, (2 * 4)(sp)

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

extern "C" {
    pub fn _idle_task();
    pub fn _task_runner() -> !;
}
