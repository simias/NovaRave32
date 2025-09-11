//! Parse the bootscript section at the top of ROM

use crate::memmap::ROM;
use core::slice;

const BOOTSCRIPT_START: usize = ROM.base as usize + 0x10;
const BOOTSCRIPT_END: usize = BOOTSCRIPT_START + 0x100 - 0x10;

pub struct ScriptEntry {
    pub code: [u8; 4],
    pub params: [u32; 3],
}

pub struct ScriptIter {
    pos: usize,
}

impl Iterator for ScriptIter {
    type Item = ScriptEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= BOOTSCRIPT_END {
            None
        } else {
            let b = unsafe { slice::from_raw_parts(self.pos as *const u8, 0x10) };

            let code = [b[0], b[1], b[2], b[3]];

            if code == [0xff; 4] {
                return None;
            }

            let mut params = [0; 3];

            for (i, b) in b[4..16].chunks_exact(4).enumerate() {
                params[i] = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            }

            self.pos += 0x10;

            Some(ScriptEntry {
                code, params,
            })
        }
    }
}

pub fn get() -> ScriptIter {
    ScriptIter {
        pos: BOOTSCRIPT_START,
    }
}
