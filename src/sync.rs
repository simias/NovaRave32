//! Keep track of how many cycles have been run for every module

use crate::{gpu, spu, systimer, CycleCounter, NoRa32, CPU_FREQ};

/// Tokens used to keep track of the progress of each module individually
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SyncToken {
    SysTimer,
    Gpu,
    Spu,

    NumTokens,
}

pub struct Synchronizer {
    /// Array containing, for each module, the date corresponding to the last sync.
    last_sync: [CycleCounter; SyncToken::NumTokens as usize],
    /// Array containing, for each module, the date at which we should force a resync.
    next_event: [CycleCounter; SyncToken::NumTokens as usize],
    /// The date of the event in `next_event` that occurs first
    first_event: CycleCounter,
}

impl Synchronizer {
    pub fn new() -> Synchronizer {
        Synchronizer {
            last_sync: [0; SyncToken::NumTokens as usize],
            next_event: [0; SyncToken::NumTokens as usize],
            first_event: 0,
        }
    }

    pub fn refresh_first_event(&mut self) {
        // The only way `min()` can return None is if the array is empty which is impossible here.
        self.first_event = *self.next_event.iter().min().unwrap();
    }

    pub fn first_event(&self) -> CycleCounter {
        self.first_event
    }
}

/// Resynchronize `who` with the CPU, returning the number of CPU cycles elapsed since the last
/// sync date
pub fn resync(m: &mut NoRa32, who: SyncToken) -> CycleCounter {
    let who = who as usize;

    let elapsed = m.cycle_counter - m.sync.last_sync[who];

    if elapsed <= 0 {
        // Since we move the timestamp back when we handle an event it's possible in some cases to
        // end up with an event being handled after a refresh that already put us past it.
        debug_assert!(elapsed > -300);
        return 0;
    }

    m.sync.last_sync[who] = m.cycle_counter;

    elapsed
}

/// Fast forward the cycle counter to the time of the next scheduled event
pub fn fast_forward_to_next_event(m: &mut NoRa32) {
    let fe = m.sync.first_event();

    if fe > 0 {
        m.cycle_counter = fe;
    }
}

/// Reset the cycle_counter to 0 by rebasing all the event counters relative to it. This way we
/// don't have to worry about overflows.
pub fn rebase_counters(m: &mut NoRa32) {
    let cc = m.cycle_counter;

    for i in 0..(SyncToken::NumTokens as usize) {
        m.sync.last_sync[i] -= cc;
        m.sync.next_event[i] -= cc;
    }
    m.sync.first_event -= cc;

    m.cycle_counter = 0;
}

/// If `who` couldn't consume all the cycles returned by `resync` it can return the leftover here,
/// we'll move the `last_sync` back by the same number of cycles which means that they'll be
/// returned on the next call to `resync`. Should only be called with *positive* cycle amounts,
/// otherwise it would put the module in the future.
pub fn rewind(m: &mut NoRa32, who: SyncToken, rewind: CycleCounter) {
    debug_assert!(rewind >= 0);

    m.sync.last_sync[who as usize] -= rewind;
}

/// Returns true if an event is pending and should be treated
pub fn is_event_pending(m: &NoRa32) -> bool {
    m.cycle_counter >= m.sync.first_event
}

/// Run event handlers as necessary
pub fn handle_events(m: &mut NoRa32) {
    while is_event_pending(m) {
        // If we've "overshot" the event date (which will almost always happen since CPU
        // instructions usually take more than one cycle to execute) we temporarily rewind to the
        // event date before running the various handlers.
        let event_delta = m.cycle_counter - m.sync.first_event;
        m.cycle_counter -= event_delta;

        if m.sync.first_event >= m.sync.next_event[SyncToken::SysTimer as usize] {
            systimer::run(m);
        }

        if m.sync.first_event >= m.sync.next_event[SyncToken::Gpu as usize] {
            gpu::run(m);
        }

        if m.sync.first_event >= m.sync.next_event[SyncToken::Spu as usize] {
            spu::run(m);
        }

        m.cycle_counter += event_delta;
    }
}

/// Set the next sync for `who` at `delay` cycles from now
pub fn next_event(m: &mut NoRa32, who: SyncToken, delay: CycleCounter) {
    m.sync.next_event[who as usize] = m.sync.last_sync[who as usize] + delay;

    m.sync.refresh_first_event();
}

/// Called when a module has no next event to schedule
pub fn no_next_event(m: &mut NoRa32, who: SyncToken) {
    // Schedule a run in 1 second. Even if there's no event scheduled we still need to call run()
    // from time to time to prevent timers from overflowing
    next_event(m, who, CPU_FREQ);
}
