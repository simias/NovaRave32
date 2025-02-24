use crate::{syscalls, MTIME_1MS, MTIME_HZ};
use alloc::vec;
use spin::{Mutex, MutexGuard};

pub struct Scheduler {
    tasks: [Task; MAX_TASKS],
    cur_task: usize,
}

static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler {
    tasks: [Task {
        state: TaskState::Dead,
        ra: 0,
        sp: 0,
        prio: -10,
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
        self.tasks[0].prio = -1000;

        self.tasks[1].sp = main_stack - BANKED_REGISTER_LEN;
        self.tasks[1].ra = main_task as *const u8 as usize;
        self.tasks[1].state = TaskState::Running;
        self.tasks[1].prio = 0;

        // Spawn the idle task first to set it up properly and make sure our task switching code is
        // working correctly
        schedule_preempt(MTIME_1MS);
        self.switch_to_task(0);
    }

    pub fn preempt_current_task(&mut self) {
        // Start by saving the state of the current task
        {
            let t = &mut self.tasks[usize::from(self.cur_task)];

            let task_ra = riscv::register::mepc::read();
            let task_sp = riscv::register::mscratch::read();

            t.ra = task_ra;
            t.sp = task_sp;
        }

        self.maybe_wake_up_tasks();

        // Find a runnable task, falling back on idle if nothing is found
        let mut next_task = 0;

        // We loop starting from the current task so that we "round-robin" the threads with equal
        // priority.
        let mut task = self.cur_task;
        loop {
            task = task.wrapping_add(1) % MAX_TASKS;

            if task == self.cur_task {
                // We wrapped around
                break;
            }

            let t = self.tasks[task];

            // Skip over task 0 which is always idle
            if task == 0 || !t.runnable() {
                continue;
            }

            if self.tasks[next_task].prio < t.prio {
                next_task = task;
            }
        }

        // We could run tickless if we only have one runnable task
        schedule_preempt(TASK_SLOT_MAX_TICKS);

        if next_task != self.cur_task {
            self.switch_to_task(next_task);
        }
    }

    fn maybe_wake_up_tasks(&mut self) {
        let now = mtime_get();

        for t in &mut self.tasks {
            if let TaskState::Sleeping { until } = t.state {
                if now >= until {
                    t.state = TaskState::Running;
                }
            }
        }
    }

    pub fn got_vsync(&mut self) {
        let mut task_awoken = false;

        for t in &mut self.tasks {
            if let TaskState::WaitingForVSync = t.state {
                task_awoken = true;
                t.state = TaskState::Running;
            }
        }

        if task_awoken {
            self.preempt_current_task();
        }
    }

    pub fn sleep_current_task(&mut self, ticks: u64) {
        if ticks > 0 {
            let t = &mut self.tasks[self.cur_task];

            let now = mtime_get();

            t.state = TaskState::Sleeping {
                until: now.saturating_add(ticks),
            };
        }

        self.preempt_current_task();
    }

    pub fn wait_event_current_task(&mut self, ev: usize) -> usize {
        let t = &mut self.tasks[self.cur_task];

        t.state = match ev {
            syscalls::events::EV_VSYNC => TaskState::WaitingForVSync,
            _ => {
                error!("Can't waiting for unknown event {}", ev);
                return !0;
            }
        };

        self.preempt_current_task();

        0
    }

    fn switch_to_task(&mut self, task_id: usize) {
        assert!(task_id < MAX_TASKS);

        let task = &self.tasks[task_id];

        assert!(!task.is_dead());

        self.cur_task = task_id;

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
    /// Task priority (higher values means higher priority)
    prio: i32,
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
    Sleeping {
        /// MTIME value
        until: u64,
    },
    WaitingForVSync,
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
const MTIME_L: *mut usize = 0xffff_fff0 as *mut usize;
/// MTIME[63:32]
const MTIME_H: *mut usize = 0xffff_fff4 as *mut usize;
/// MTIMECMP[31:0]
const MTIMECMP_L: *mut usize = 0xffff_fff8 as *mut usize;
/// MTIMECMP[63:32]
const MTIMECMP_H: *mut usize = 0xffff_fffc as *mut usize;

fn mtime_get() -> u64 {
    loop {
        unsafe {
            let h = MTIME_H.read_volatile();
            let l = MTIME_L.read_volatile();
            let c = MTIME_H.read_volatile();

            // Make sure that the counter didn't wrap as we were reading it
            if h == c {
                return ((h as u64) << 32) | (l as u64);
            }
        }
    }
}

fn mtimecmp_set(cmp: u64) {
    let l = cmp as usize;
    let h = (cmp >> 32) as usize;

    unsafe {
        // Set full 1 to the low word so that we don't trigger an interrupt by mistake. Not that it
        // matters since we should call this with IRQ disabled, but still.
        MTIMECMP_L.write_volatile(!0);
        MTIMECMP_H.write_volatile(h);
        MTIMECMP_L.write_volatile(l);
    }
}
