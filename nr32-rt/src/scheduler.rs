use crate::{
    asm::{_idle_task, _task_runner},
    syscalls, MTIME_HZ,
};
use alloc::{boxed::Box, vec, vec::Vec};
use spin::{Mutex, MutexGuard};

pub struct Scheduler {
    tasks: Vec<Task>,
    /// Which task of `tasks` is currently running
    cur_task: usize,
}

static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler {
    tasks: Vec::new(),
    cur_task: 0,
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
    pub fn start(&mut self) {
        self.tasks = Vec::with_capacity(4);

        // Create the idle task.
        let idle_task = unsafe { core::mem::transmute::<usize, fn()>(_idle_task as usize) };
        self.spawn_task(idle_task as usize, i32::MIN, 0);
        self.switch_to_task(0);
    }

    pub fn spawn_task(&mut self, entry: usize, prio: i32, stack_size: usize) {
        let (stack, sp) = stack_alloc(stack_size + BANKED_REGISTER_LEN);
        // Put function in banked a0
        unsafe {
            let p = sp - BANKED_REGISTER_LEN + 23 * 4;

            let p = p as *mut usize;

            *p = entry;
        };

        let new_task = Task {
            sp: sp - BANKED_REGISTER_LEN,
            ra: _task_runner as usize,
            state: TaskState::Running,
            prio,
            stack: stack,
        };

        for t in self.tasks.iter_mut() {
            if matches!(t.state, TaskState::Dead) {
                *t = new_task;
                return;
            }
        }

        // No dead task, create a new one
        self.tasks.push(new_task);
    }

    pub fn exit_current_task(&mut self) {
        assert_ne!(self.cur_task, 0, "Attempted to kill the idle task!");

        let t = &mut self.tasks[self.cur_task];
        t.state = TaskState::Dead;
        t.prio = i32::MIN;
        t.stack = Box::new([]);

        self.schedule()
    }

    pub fn schedule(&mut self) {
        {
            // Start by saving the state of the current task
            let t = &mut self.tasks[self.cur_task];

            let task_ra = riscv::register::mepc::read();
            let task_sp = riscv::register::mscratch::read();

            t.ra = task_ra;
            t.sp = task_sp;
        }

        self.maybe_wake_up_tasks();

        // Find a runnable task, falling back on idle if nothing is found
        let mut next_task = 0;

        // We loop starting from the next task so that we "round-robin" the threads with equal
        // priority.
        let mut task = self.cur_task;
        let ntasks = self.tasks.len();
        loop {
            task = task.wrapping_add(1);
            if task >= ntasks {
                task = 0;
            }

            let t = &self.tasks[task];

            if t.runnable() && self.tasks[next_task].prio < t.prio {
                next_task = task;
            }

            if task == self.cur_task {
                // We wrapped around
                break;
            }
        }

        // Now we figure out when we want to schedule the next timer IRQ
        let now = mtime_get();
        // If we have no other task to run we can just delay the preemption forever.
        let mut run_until = now + u64::from(MTIME_HZ);
        let next_prio = self.tasks[next_task].prio;
        let contention_until = now + u64::from(TASK_SLOT_ROUND_ROBBIN);

        for (tid, t) in self.tasks.iter().enumerate() {
            if tid == next_task {
                // We can't preempt ourselves...
                continue;
            }

            if t.prio < next_prio {
                // Don't allow lower priority tasks from preempting us
                continue;
            }

            match t.state {
                TaskState::Running => {
                    // This task must have the same priority as us (otherwise it would have been picked
                    // above) so we're going to force a preemption in a short while
                    run_until = run_until.min(contention_until);
                }
                TaskState::Sleeping { until } => {
                    run_until = if t.prio > next_prio {
                        run_until.min(until)
                    } else {
                        // If priority is the same we don't have to wake up when the sleep elapses,
                        // we can keep going and delay the other task
                        run_until.min(contention_until)
                    };
                }
                // Task is waiting for something else, no point
                _ => continue,
            };
        }

        schedule_preempt(run_until);

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
            self.schedule();
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

        self.schedule();
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

        self.schedule();

        0
    }

    fn switch_to_task(&mut self, task_id: usize) {
        let task = &self.tasks[task_id];

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

/// How much space is saved on the task stack when banking the registers
const BANKED_REGISTER_LEN: usize = 32 * 4;

#[derive(Clone)]
struct Task {
    state: TaskState,
    ra: usize,
    sp: usize,
    /// Task priority (higher values means higher priority)
    prio: i32,
    stack: Box<[StackWord]>,
}

impl Task {
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
fn schedule_preempt(until: u64) {
    mtimecmp_set(until);

    // Make sure the MTIE is set
    unsafe {
        riscv::register::mie::set_mtimer();
    }
}

/// Type used to force the right alignment for the stack alloc
#[repr(align(16))]
#[derive(Copy, Clone)]
#[allow(dead_code)]
struct StackWord(u32);

/// Allocate a `stack_size`-byte long, 0-initialized stack and return a 16-byte aligned pointer to
/// the top
fn stack_alloc(stack_size: usize) -> (Box<[StackWord]>, usize) {
    let stack_size = (stack_size + 0xf) & !0xf;

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

    (stack, top)
}

/// If two or more tasks with equal priority want to run at the same time, how long should they be
/// allowed to run before being preempted?
const TASK_SLOT_ROUND_ROBBIN: u32 = MTIME_HZ / 120;

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
