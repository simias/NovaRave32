use crate::{MTIME_1MS, MTIME_HZ};
use alloc::vec;
use spin::{Mutex, MutexGuard};

pub struct Scheduler {
    tasks: [Task; MAX_TASKS],
    cur_task: u8,
}

static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler {
    tasks: [Task {
        state: TaskState::Dead,
        ra: 0,
        sp: 0,
    }; MAX_TASKS],
    cur_task: !0,
});

pub fn get() -> MutexGuard<'static, Scheduler> {
    // There should never be contention on the scheduler since we're running with IRQs disabled
    match SCHEDULER.try_lock() {
        Some(lock) => lock,
        None => {
            panic!("Couldn't lock scheduler!")
        }
    }
}

impl Scheduler {
    pub fn start(&mut self, idle_task: fn() -> !, main_task: fn() -> !) {
        let idle_stack = stack_alloc(128);
        let main_stack = stack_alloc(2048);

        self.tasks[0].sp = idle_stack - BANKED_REGISTER_LEN;
        self.tasks[0].ra = idle_task as *const u8 as usize;
        self.tasks[0].state = TaskState::Running;

        self.tasks[1].sp = main_stack - BANKED_REGISTER_LEN;
        self.tasks[1].ra = main_task as *const u8 as usize;
        self.tasks[1].state = TaskState::Running;

        // Spawn the idle task first to set it up properly and make sure our task switching code is
        // working correctly
        schedule_preempt(MTIME_1MS);
        self.switch_to_task(0);
    }

    pub fn preempt_current_task(&mut self) {
        let t = &mut self.tasks[usize::from(self.cur_task)];

        let task_ra = riscv::register::mepc::read();
        let task_sp = riscv::register::mscratch::read();

        t.ra = task_ra;
        t.sp = task_sp;

        // Find a runnable task
        let cur_task = usize::from(self.cur_task);
        let mut next_task = cur_task;

        loop {
            next_task = next_task.wrapping_add(1) % MAX_TASKS;

            let nt = self.tasks[next_task];

            // Skip over task 0 which is always idle
            if next_task != 0 && nt.runnable() {
                break;
            }

            if next_task == cur_task {
                // We wrapped around and didn't find anything to run, run idle task
                next_task = 0;
                break;
            }
        }

        // We could run tickless if we only have one runnable task
        schedule_preempt(TASK_SLOT_MAX_TICKS);
        self.switch_to_task(next_task);
    }

    fn switch_to_task(&mut self, task_id: usize) {
        let task = &self.tasks[task_id];

        assert!(!task.is_dead());

        self.cur_task = task_id as u8;

        riscv::register::mscratch::write(task.sp);
        riscv::register::mepc::write(task.ra);

        let mpp_user = riscv::register::mstatus::MPP::User;

        unsafe {
            // Switch to user mode upon mret
            riscv::register::mstatus::set_mpp(mpp_user);
            // Enable interrupts upon mret
            riscv::register::mstatus::set_mpie();
        }
    }
}

/// Needs at least 2 tasks: "idle" and "main"
const MAX_TASKS: usize = 4;

/// How much space is saved on the task stack when banking the registers
const BANKED_REGISTER_LEN: usize = 32 * 4;

#[derive(Copy, Clone)]
struct Task {
    state: TaskState,
    ra: usize,
    sp: usize,
}

impl Task {
    fn is_dead(&self) -> bool {
        matches!(self.state, TaskState::Dead)
    }

    fn runnable(&self) -> bool {
        matches!(self.state, TaskState::Running)
    }
}

#[derive(Copy, Clone)]
enum TaskState {
    Dead,
    Running,
}

/// Use MTIMECMP to schedule an interrupt
fn schedule_preempt(delay_ticks: u32) {
    let now = mtime_get();

    // Should never overflow given that it's a 64bit counter running at 48kHz
    mtimecmp_set(now + u64::from(delay_ticks));

    // Make sure the MTIE is set
    unsafe {
        riscv::register::mie::set_mtimer();
    }
}

/// Allocate a `stack_size`-byte long, 0-initialized stack and return a 16-byte aligned pointer to
/// the top
fn stack_alloc(stack_size: usize) -> usize {
    let stack_size = (stack_size + 0xf) & !0xf;

    /// Type used to force the right alignment for the stack alloc
    #[repr(align(16))]
    #[derive(Copy, Clone)]
    #[allow(dead_code)]
    struct StackWord(u32);

    let sw_size = core::mem::size_of::<StackWord>();

    let mut stack = vec![StackWord(0); stack_size / sw_size].into_boxed_slice();

    let ptr = stack.as_mut_ptr() as usize;

    let top = ptr + stack_size;

    debug!(
        "Allocated stack of {}B starting at {:x}",
        stack.len() * sw_size,
        top
    );

    assert!(top & 0xf == 0, "Allocated stack is not correctly aligned!");

    top
}

/// How long is a task allowed to hog the CPU if others are also runnable. Expressed in MTIME timer
/// ticks
const TASK_SLOT_MAX_TICKS: u32 = MTIME_HZ / 10;

/// MTIME[31:0]
const MTIME_L: *mut u32 = 0xffff_fff0 as *mut u32;
/// MTIME[63:32]
const MTIME_H: *mut u32 = 0xffff_fff4 as *mut u32;
/// MTIMECMP[31:0]
const MTIMECMP_L: *mut u32 = 0xffff_fff8 as *mut u32;
/// MTIMECMP[63:32]
const MTIMECMP_H: *mut u32 = 0xffff_fffc as *mut u32;

fn mtime_get() -> u64 {
    loop {
        unsafe {
            let h = MTIME_H.read_volatile();
            let l = MTIME_L.read_volatile();
            let c = MTIME_H.read_volatile();

            // Make sure that the counter didn't wrap as we were reading it
            if h == c {
                return (u64::from(h) << 32) | u64::from(l);
            }
        }
    }
}

fn mtimecmp_set(cmp: u64) {
    let l = cmp as u32;
    let h = (cmp >> 32) as u32;

    unsafe {
        // Set full 1 to the low word so that we don't trigger an interrupt by mistake
        MTIMECMP_L.write_volatile(!0);
        MTIMECMP_H.write_volatile(h);
        MTIMECMP_L.write_volatile(l);
    }
}
