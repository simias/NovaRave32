use super::{CPU_FREQ, CycleCounter, NoRa32, fifo::Fifo, irq, sync};

mod touchscreen;

pub struct InputDev {
    tx_fifo: Fifo<16, u8>,
    rx_fifo: Fifo<16, u8>,
    /// IRQ high on TX empty
    tx_complete_irq: bool,
    /// Selected port
    port: u8,
    /// Baud rate divider
    clk_div: CycleCounter,
    /// Baud rate divider counter
    clk_count: CycleCounter,
    /// Bitpos within a transaction
    seq: u8,
    /// Touchscreen interface
    touchscreen: touchscreen::TouchScreen,
}

impl InputDev {
    pub fn new() -> InputDev {
        InputDev {
            tx_fifo: Fifo::new(),
            rx_fifo: Fifo::new(),
            tx_complete_irq: false,
            port: PORT_SELECT_NONE,
            clk_div: 1,
            clk_count: 0,
            seq: 0,
            touchscreen: touchscreen::TouchScreen::new(),
        }
    }

    pub fn touchscreen_mut(&mut self) -> &mut touchscreen::TouchScreen {
        &mut self.touchscreen
    }
}

pub fn run(m: &mut NoRa32) {
    let elapsed = sync::resync(m, IDEVSYNC);

    if m.input_dev.tx_fifo.is_empty() {
        // Idle
        m.input_dev.clk_count = 0;
        sync::next_event(m, IDEVSYNC, CPU_FREQ);
        return;
    }

    m.input_dev.clk_count += elapsed;

    while m.input_dev.clk_count >= m.input_dev.clk_div {
        m.input_dev.clk_count -= m.input_dev.clk_div;

        // Full byte transmitted
        let b = m.input_dev.tx_fifo.pop().unwrap_or(0xff);

        let mut rx_byte = 0xff;

        if m.input_dev.port == 0 && m.input_dev.clk_div >= 512 {
            // Touchscreen port
            rx_byte &= m.input_dev.touchscreen.xmit(m.input_dev.seq, b);
        }

        m.input_dev.rx_fifo.push(rx_byte);
        m.input_dev.seq += 1;

        if m.input_dev.tx_fifo.is_empty() && m.input_dev.tx_complete_irq {
            irq::trigger(m, irq::Interrupt::InputDev);
        }
    }

    let next_event = match m.input_dev.tx_fifo.len() {
        // Idle
        0 => CPU_FREQ,
        // Refresh at the end of the full transmit
        n => m.input_dev.clk_div * (n as CycleCounter) - m.input_dev.clk_count,
    };

    sync::next_event(m, IDEVSYNC, next_event);
}

pub fn store_word(m: &mut NoRa32, addr: u32, val: u32) {
    run(m);

    match addr >> 2 {
        // CONF
        0 => {
            if val & 1 != 0 {
                m.input_dev.tx_fifo.clear();
                m.input_dev.rx_fifo.clear();
                m.input_dev.port = PORT_SELECT_NONE;
                m.input_dev.seq = 0;
            }
            m.input_dev.tx_complete_irq = (val & (1 << 1)) != 0;
            m.input_dev.clk_div = (((val >> 16) & 0xffff) + 1) as CycleCounter;
            // We run at CPU_FREQ / 16
            m.input_dev.clk_div *= 16;
            // We only care about full byte transmits
            m.input_dev.clk_div *= 8;
            m.input_dev.clk_count = 0;
        }
        // PORT
        1 => {
            let port = val as u8;

            if port != m.input_dev.port {
                m.input_dev.port = port;
                m.input_dev.seq = 0;
            }
        }
        // TX
        2 => {
            let b = val as u8;

            m.input_dev.tx_fifo.push(b);
        }
        n => panic!("Unknown input dev register {n:x}"),
    }

    run(m);
}

pub fn load_word(m: &mut NoRa32, addr: u32) -> u32 {
    run(m);

    match addr >> 2 {
        // RX
        2 => u32::from(m.input_dev.rx_fifo.pop().unwrap_or(0xff)),
        _ => !0,
    }
}

trait InputDevice {
    // Exchange a byte through the serial interface.
    fn xmit(&mut self, seq: u8, tx_byte: u8) -> u8;
}

const PORT_SELECT_NONE: u8 = 0xff;

const IDEVSYNC: sync::SyncToken = sync::SyncToken::InputDev;
