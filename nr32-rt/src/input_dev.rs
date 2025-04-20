/// Input device (touchscreen, pad, ...) handling
use spin::{Mutex, MutexGuard};

pub struct InputDev {
    /// If a transfer is ongoing, this is the target buffer for the RX data
    xfer_target: Option<&'static mut [u8]>,
}

static INPUT_DEV: Mutex<InputDev> = Mutex::new(InputDev { xfer_target: None });

pub fn get() -> MutexGuard<'static, InputDev> {
    // There should never be contention on the scheduler since we're running with IRQs disabled
    match INPUT_DEV.try_lock() {
        Some(lock) => lock,
        None => {
            panic!("Couldn't lock input_dev!")
        }
    }
}

impl InputDev {
    /// Attempt to start a transfer.
    pub fn xmit(&mut self, port: u8, data_in_out: &'static mut [u8]) -> Result<(), ()> {
        if self.xfer_target.is_some() {
            // For now we only handle one transfer at a time
            warn!("Attempted to start two input_dev xmit concurrently");
            return Err(());
        }

        if data_in_out.is_empty() {
            return Err(());
        }

        if data_in_out.len() > TX_RX_FIFO_DEPTH {
            // We would have to chunk it
            return Err(());
        }

        let mut conf = 0;

        // TX/RX FIFO + port clear
        conf |= 1;
        // TX complete IRQ
        conf |= 1 << 1;
        // Baud rate divider: (CPU_FREQ / 16) / 5 -> ~280kHz
        conf |= (5 - 1) << 16;

        unsafe {
            INPUT_DEV_CONF.write_volatile(conf);

            // Port selection: touchscreen
            INPUT_DEV_PORT.write_volatile(port);

            for b in data_in_out.iter() {
                INPUT_DEV_TX_RX.write_volatile(*b);
            }
        }

        self.xfer_target = Some(data_in_out);

        input_dev_enable_irq(true);

        Ok(())
    }

    /// Called on IRQ
    pub fn xmit_done(&mut self) {
        let xfer_target = match self.xfer_target.take() {
            Some(t) => t,
            None => {
                warn!("Input dev IRQ with no buffer?");
                return;
            }
        };

        for b in xfer_target.iter_mut() {
            unsafe {
                *b = INPUT_DEV_TX_RX.read_volatile();
            }
        }
    }
}

fn input_dev_enable_irq(enable: bool) {
    unsafe {
        let mut irq = super::IRQ_PENDING.read_volatile();
        if enable {
            irq |= 1 << 1;
        } else {
            irq &= !(1 << 1);
        }

        super::IRQ_PENDING.write_volatile(irq);
    }
}

const INPUT_DEV_BASE: usize = 0x1003_0000;
const INPUT_DEV_CONF: *mut u32 = INPUT_DEV_BASE as *mut u32;
const INPUT_DEV_PORT: *mut u8 = (INPUT_DEV_BASE + 4) as *mut u8;
const INPUT_DEV_TX_RX: *mut u8 = (INPUT_DEV_BASE + 4 * 2) as *mut u8;

const TX_RX_FIFO_DEPTH: usize = 16;
