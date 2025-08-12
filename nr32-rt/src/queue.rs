use alloc::vec::Vec;
use spin::{Mutex, MutexGuard};

static QUEUE_MANAGER: Mutex<QueueManager> = Mutex::new(QueueManager {
    qs: Vec::new(),
});

#[unsafe(link_section = ".text.fast")]
pub fn get() -> MutexGuard<'static, QueueManager> {
    // There should never be contention on the scheduler since we're running with IRQs disabled
    match QUEUE_MANAGER.try_lock() {
        Some(lock) => lock,
        None => {
            panic!("Couldn't lock queue manager!")
        }
    }
}

pub type QueueId = u8;
pub type MsgId = u32;

pub struct QueueManager {
    qs: Vec<Queue>,
}

impl QueueManager {
    /// Alloc a new queue and returns its identifier
    pub fn alloc_queue(&mut self) -> QueueId {
        for (i, t) in self.qs.iter_mut().enumerate() {
            if t.is_free() {
                *t = Queue::empty();
                return (i + 1) as QueueId;
            }
        }

        if self.qs.len() >= QueueId::MAX as usize {
            panic!("No more queues available!");
        }

        // Alloc new entry
        self.qs.push(Queue::empty());

        return self.qs.len() as QueueId
    }
}

struct Queue {
    read_idx: u8,
    write_idx: u8,
    /// Set to non-zero if the queue is available for reuse
    free: u8,
    /// Counter that increments for every message added in order to provide an ID.
    last_id: MsgId,
    q: [Message; QUEUE_LEN],
}

impl Queue {
    fn empty() -> Queue {
        Queue {
            read_idx: 0,
            write_idx: 0,
            free: 0,
            last_id: 0,
            q: [Message { ty: 0, what: 0 }; QUEUE_LEN]
        }
    }

    fn is_free(&self) -> bool {
        self.free != 0
    }
}

#[derive(Copy, Clone)]
#[repr(C)]
struct Message {
    /// ID for this message, allocated by the kernel.
    id: MsgId,
    /// Metadata for the message type. Values >= 0x80 are reserved for the operating system
    /// messaging
    ty: u8,
    /// Opaque message payload.
    what: usize,
}

/// Message queue length is hardcoded
const QUEUE_LEN: usize = 8;

/// Message type used for system events (interrupts, etc).
///
/// `what` contains the event type.
pub const SYS_MSG_EVENT: u8 = 0xff;
