use super::InputDevice;

pub struct TouchScreen {
    /// Touchscreen input (None if there's no input currently)
    position: Option<[u16; 2]>,
    /// Position latched at the beginning of a transmission
    latched_position: [u16; 2],
    /// Set to true if a transaction is in progress
    selected: bool,
}

impl TouchScreen {
    pub fn new() -> TouchScreen {
        TouchScreen {
            position: None,
            latched_position: [0xffff; 2],
            selected: false,
        }
    }

    pub fn set_touch(&mut self, position: Option<[u16; 2]>) {
        self.position = position;
    }
}

impl InputDevice for TouchScreen {
    fn xmit(&mut self, seq: u8, tx_byte: u8) -> u8 {
        match (seq, tx_byte, self.selected) {
            (0, b'T', _) => {
                self.selected = true;
                0xff
            }
            (1, b'S', true) => {
                self.latched_position = self.position.unwrap_or([0xffff, 0xffff]);
                b'a'
            }
            (2, _, true) => (self.latched_position[0] >> 8) as u8,
            (3, _, true) => self.latched_position[0] as u8,
            (4, _, true) => (self.latched_position[1] >> 8) as u8,
            (5, _, true) => self.latched_position[1] as u8,
            _ => {
                self.selected = false;
                0xff
            }
        }
    }
}
