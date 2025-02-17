use alloc::vec;

pub fn start(main_task: fn(u32) -> !) {
    let stack = stack_alloc(2048);

    // return_to_user_stack will pop registers, we need to allocate enough room for them
    let sp = stack - 48;
    let ra = main_task as *const u8 as u32;

    info!("Jumping to {:x}", ra);
    unsafe { asm::start_user_task(ra, sp, 42) }
}

/// Allocate a `stack_size`-byte long, 0-initialized stack and return a 16-byte aligned pointer to
/// the top along with the size effectively allocated
fn stack_alloc(stack_size: usize) -> u32 {
    let mut stack = vec![0u8; stack_size].into_boxed_slice();

    let ptr = stack.as_mut_ptr() as u32;

    let top = ptr + (stack_size as u32);

    debug!("Allocated stack of {}B starting at {:x}", stack.len(), top);

    // Stack must be 16-byte aligned
    top & !0xf
}

mod asm {
    use core::arch::global_asm;

    // Jump to task whose address is in a0, and stack in s0 and passing `ret` as 1st argument
    //
    // The task will be run in U-mode with interrupts enabled.
    extern "C" {
        pub fn start_user_task(ra: u32, sp: u32, ret: u32) -> !;
    }

    global_asm!(
        r#"
    .option push
    .option rvc
    .section .text
    .global start_user_task
start_user_task:
    .cfi_startproc

    /* Clear MPP to switch to user mode when mret is called */
    li t0, 3 << 11
    csrc mstatus, t0
    /* Set MPIE to enable interrupts upon mret */
    li t0, 1 << 7
    csrs mstatus, t0

    /* Put task return address in mepc */
    csrw mepc, a0

    /* Restore SP from a1 */
    mv sp, a1

    /* Set parameter from a2 */
    mv a0, a2

    mret

    .cfi_endproc
    .option pop
    "#
    );
}
