use crate::lock::{Mutex, MutexGuard};
use crate::{
    MTIME_HZ, SysError, SysResult,
    asm::{_idle_task, _task_runner},
};
use alloc::vec::Vec;
use core::ptr::NonNull;

type TaskId = usize;

pub struct Scheduler {
    tasks: Vec<Task>,
    /// Which task of `tasks` is currently running
    cur_task: usize,
    /// MTIME of the last task change
    last_task_change: u64,
    /// MTIME of last stat dump
    last_stat_dump: u64,
}

static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler {
    tasks: Vec::new(),
    cur_task: 0,
    last_task_change: 0,
    last_stat_dump: 0,
});

#[unsafe(link_section = ".text.fast")]
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
        self.spawn_task(TaskType::System, idle_task as usize, 0, i32::MIN, 0, 0)
            .unwrap();
        self.switch_to_task(0);
    }

    pub fn cur_task_id(&self) -> usize {
        self.cur_task
    }

    pub fn spawn_task(
        &mut self,
        ty: TaskType,
        entry: usize,
        data: usize,
        prio: i32,
        stack_size: usize,
        gp: usize,
    ) -> SysResult<TaskId> {
        let (stack, sp) = stack_alloc(ty, stack_size + BANKED_REGISTER_LEN)?;

        let new_task = Task {
            sp: sp - BANKED_REGISTER_LEN,
            ra: _task_runner as usize,
            state: TaskState::Running,
            prio,
            ty,
            stack,
            run_ticks: 0,
        };

        new_task.set_banked_reg(Reg::A0, data);
        new_task.set_banked_reg(Reg::A1, entry);
        new_task.set_banked_reg(Reg::Gp, gp);

        for (i, t) in self.tasks.iter_mut().enumerate() {
            if t.is_dead() {
                *t = new_task;
                return Ok(i);
            }
        }

        // No dead task, create a new one
        self.tasks.push(new_task);

        Ok(self.tasks.len() - 1)
    }

    pub fn exit_current_task(&mut self) {
        assert_ne!(self.cur_task, 0, "Attempted to kill the idle task!");

        let t = &mut self.tasks[self.cur_task];
        t.state = TaskState::Dead;
        t.prio = i32::MIN;

        stack_free(t.ty, t.stack);

        self.schedule()
    }

    #[unsafe(link_section = ".text.fast")]
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
                TaskState::Sleeping { until, .. } => {
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
            let t = &mut self.tasks[self.cur_task];
            if !t.is_dead() {
                t.run_ticks += now - self.last_task_change;
            }
            self.last_task_change = now;

            self.switch_to_task(next_task);
        }
    }

    #[unsafe(link_section = ".text.fast")]
    fn maybe_wake_up_tasks(&mut self) {
        let now = mtime_get();

        for t in &mut self.tasks {
            if let TaskState::Sleeping { until, .. } = t.state {
                if now >= until {
                    t.state = TaskState::Running;
                    t.set_banked_reg(Reg::A0, SysError::TimeOut as usize);
                }
            }
        }
    }

    #[unsafe(link_section = ".text.fast")]
    fn futex_wake_one(&mut self, wake_futex_addr: usize) -> Option<TaskId> {
        // Look for the highest-prio thread blocking on the futex
        let mut candidate = None;
        let mut candidate_prio = i32::MIN;

        for (i, t) in self.tasks.iter().enumerate() {
            if let TaskState::Sleeping { futex_addr, .. } = t.state {
                if futex_addr == wake_futex_addr && t.prio >= candidate_prio {
                    candidate = Some(i);
                    candidate_prio = t.prio;
                }
            }
        }

        if let Some(i) = candidate {
            self.tasks[i].state = TaskState::Running;
        }

        candidate
    }

    #[unsafe(link_section = ".text.fast")]
    pub fn futex_wake(&mut self, wake_futex_addr: usize, nwakeup: usize) -> SysResult<usize> {
        if nwakeup == 0 {
            return Err(SysError::Invalid);
        }

        let cur_prio = self.tasks[self.cur_task].prio;

        let mut needs_schedule = false;
        let mut nawoken = 0;

        for _ in 0..nwakeup {
            match self.futex_wake_one(wake_futex_addr) {
                Some(t) => {
                    nawoken += 1;
                    if self.tasks[t].prio > cur_prio {
                        needs_schedule = true;
                    }
                }
                None => break,
            }
        }

        if needs_schedule {
            self.schedule();
        }

        Ok(nawoken)
    }

    #[unsafe(link_section = ".text.fast")]
    pub fn wake_up_state(&mut self, state: TaskState) {
        let cur_prio = self.tasks[self.cur_task].prio;
        let mut needs_schedule = false;

        for t in &mut self.tasks {
            if t.state == state {
                if t.prio > cur_prio {
                    needs_schedule = true;
                }
                t.state = TaskState::Running;
            }
        }

        if needs_schedule {
            self.schedule();
        }

        if state == TaskState::WaitingForVSync {
            self.dump_task_stats();
        }
    }

    pub fn dump_task_stats(&mut self) {
        // For more accuracy (and simplicity) we use data from the last task change since that's
        // when the counters were last updated. That means that this code won't work if a single
        // task hogs 100% of the CPU without ever yielding or being preempted.
        let lts = self.last_task_change;

        let span = lts - self.last_stat_dump;

        if span < u64::from(MTIME_HZ * 5) {
            return;
        }

        let mut tot = 0;

        for (i, t) in self.tasks.iter_mut().enumerate() {
            let pcent = (t.run_ticks * 100 + span / 2) / span;
            info!("Task #{i} CPU: {pcent}%");
            tot += t.run_ticks;
            t.run_ticks = 0;
        }

        let pcent = ((span - tot) * 100 + span / 2) / span;
        info!("System CPU: {pcent}%");

        self.last_stat_dump = lts;
    }

    #[unsafe(link_section = ".text.fast")]
    pub fn sleep_current_task(&mut self, futex_addr: usize, ticks: u64) {
        if ticks > 0 {
            let t = &mut self.tasks[self.cur_task];

            let now = mtime_get();

            t.state = TaskState::Sleeping {
                futex_addr,
                until: now.saturating_add(ticks),
            };
        }
        self.schedule();
    }

    #[unsafe(link_section = ".text.fast")]
    pub fn current_task_set_state(&mut self, state: TaskState) {
        let t = &mut self.tasks[self.cur_task];

        t.state = state;

        self.schedule();
    }

    #[unsafe(link_section = ".text.fast")]
    fn switch_to_task(&mut self, task_id: TaskId) {
        let task = &self.tasks[task_id];

        self.cur_task = task_id;

        riscv::register::mscratch::write(task.sp);
        riscv::register::mepc::write(task.ra);

        let mpp_ret = match task.ty {
            TaskType::System => riscv::register::mstatus::MPP::Machine,
            TaskType::User => riscv::register::mstatus::MPP::User,
        };

        unsafe {
            riscv::register::mstatus::set_mpp(mpp_ret);
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
    ty: TaskType,
    /// Pointer to the stack buffer
    stack: NonNull<u8>,
    /// Total number of MTIME ticks this time has been running since the last stat dump
    run_ticks: u64,
}

unsafe impl Send for Task {}

impl Task {
    #[unsafe(link_section = ".text.fast")]
    fn runnable(&self) -> bool {
        matches!(self.state, TaskState::Running)
    }

    #[unsafe(link_section = ".text.fast")]
    fn is_dead(&self) -> bool {
        matches!(self.state, TaskState::Dead)
    }

    fn set_banked_reg(&self, reg: Reg, v: usize) {
        let sp = self.sp as *mut usize;

        // The layout should match the banking scheme in asm.rs
        unsafe { *(sp.add(32 - reg as usize)) = v };
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum TaskState {
    Dead,
    Running,
    Sleeping {
        /// MTIME value
        until: u64,
        /// If this task is waiting on a futex, this is its address.
        futex_addr: usize,
    },
    WaitingForVSync,
    WaitingForInputDev,
}

/// Use MTIMECMP to schedule an interrupt
#[unsafe(link_section = ".text.fast")]
fn schedule_preempt(until: u64) {
    mtimecmp_set(until);

    // Make sure the MTIE is set
    unsafe {
        riscv::register::mie::set_mtimer();
    }
}

/// Allocate a `stack_size`-byte long, 0-initialized stack and return a 16-byte aligned pointer to
/// the top
fn stack_alloc(ty: TaskType, stack_size: usize) -> SysResult<(NonNull<u8>, usize)> {
    let stack_size = (stack_size + 0xf) & !0xf;

    let heap = match ty {
        TaskType::System => crate::ALLOCATOR.system_heap(),
        TaskType::User => crate::ALLOCATOR.user_heap(),
    };

    let ptr = heap.raw_alloc(stack_size, 16);

    let ptr = match NonNull::new(ptr) {
        Some(p) => p,
        None => return Err(SysError::NoMem),
    };

    let top = (ptr.as_ptr() as usize) + stack_size;

    debug!("Allocated stack of {}B starting at {:x}", stack_size, top);

    assert!(top & 0xf == 0, "Allocated stack is not correctly aligned!");

    Ok((ptr, top))
}

fn stack_free(ty: TaskType, stack: NonNull<u8>) {
    let heap = match ty {
        TaskType::System => crate::ALLOCATOR.system_heap(),
        TaskType::User => crate::ALLOCATOR.user_heap(),
    };

    heap.raw_free(stack.as_ptr()).unwrap();
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum TaskType {
    /// Kernel task
    System,
    /// User task
    User,
}

/// If two or more tasks with equal priority want to run at the same time, how long should they be
/// allowed to run before being preempted?
const TASK_SLOT_ROUND_ROBBIN: u32 = MTIME_HZ / 120;

/// MTIME[31:0]
const MTIME_L: *mut usize = 0xffff_ffe0 as *mut usize;
/// MTIME[63:32]
const MTIME_H: *mut usize = 0xffff_ffe4 as *mut usize;
/// MTIMECMP[31:0]
const MTIMECMP_L: *mut usize = 0xffff_ffe8 as *mut usize;
/// MTIMECMP[63:32]
const MTIMECMP_H: *mut usize = 0xffff_ffec as *mut usize;

#[unsafe(link_section = ".text.fast")]
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

#[unsafe(link_section = ".text.fast")]
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

#[allow(dead_code)]
#[derive(PartialEq, Eq, Debug, Copy, Clone)]
enum Reg {
    Zero = 0,
    Ra = 1,
    Sp = 2,
    Gp = 3,
    Tp = 4,
    T0 = 5,
    T1 = 6,
    T2 = 7,
    S0 = 8,
    S1 = 9,
    A0 = 10,
    A1 = 11,
    A2 = 12,
    A3 = 13,
    A4 = 14,
    A5 = 15,
    A6 = 16,
    A7 = 17,
    S2 = 18,
    S3 = 19,
    S4 = 20,
    S5 = 21,
    S6 = 22,
    S7 = 23,
    S8 = 24,
    S9 = 25,
    S10 = 26,
    S11 = 27,
    T3 = 28,
    T4 = 29,
    T5 = 30,
    T6 = 31,
}
