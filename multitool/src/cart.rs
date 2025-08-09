use goblin::elf::Elf;
use std::path::Path;
use std::fs::File;
use std::io::{Write, Read};
use anyhow::{Context, Result};

pub struct Cart {
    buf: Vec<u8>,
}

impl Cart {
    pub fn new() -> Cart {
        let mut buf = vec![0xff; 0x100];

        buf[..8].clone_from_slice(&CART_MAGIC);

        Cart { buf }
    }

    pub fn len(&self) -> usize {
        return self.buf.len()
    }

    pub fn dump<W: Write>(&self, w: &mut W) -> Result<()> {
        w.write_all(&self.buf)?;

        Ok(())
    }

    pub fn load_bootloader<P: AsRef<Path>>(&mut self, elf_path: P) -> Result<()> {
        let mut f = File::open(elf_path)?;
        let mut elf_raw = Vec::new();

        f.read_to_end(&mut elf_raw)?;

        let elf = Elf::parse(&elf_raw)?;

        if elf.entry != 0x2000_0100 {
            bail!("Bootloader entry point isn't 0x20000100: 0x{:x}", elf.entry);
        }

        for (pi, ph) in elf.program_headers.iter().enumerate() {
            if ph.p_filesz > 0 && ph.p_type == goblin::elf::program_header::PT_LOAD {
                let data = &elf_raw[ph.file_range()];

                debug!("Copying bootloader section {}: {:?}", pi, ph);

                self.copy_data(data, ph.p_paddr).with_context(|| format!("Copying data from bootloader section {}", pi))?;
            }
        }

        Ok(())
    }

    fn copy_data(&mut self, data: &[u8], load_addr: u64) -> Result<()> {
        is_range_in_rom(load_addr, data.len() as u64)?;

        let cart_start = (load_addr as usize) - ROM.base as usize;
        let cart_end = cart_start + data.len();

        if cart_end > self.buf.len() {
            self.buf.resize(cart_end, 0xff);
        }

        for (off, (t, f)) in self.buf[cart_start..cart_end].iter_mut().zip(data.iter()).enumerate() {
            if *t != 0xff {
                bail!("Data conflict at ROM address 0x{:x}", ROM.base as usize + cart_start + off);
            }

            *t = *f;
        }

        Ok(())
    }
}

fn is_addr_in_rom(addr: u64) -> Result<()> {
    if addr > u32::MAX as u64 || ROM.contains(addr as u32).is_none() {
        bail!("0x{:x} isn't a valid ROM address!", addr);
    }

    Ok(())
}

fn is_range_in_rom(start: u64, len: u64) -> Result<()> {
    is_addr_in_rom(start)?;
    is_addr_in_rom(start + len)
}

pub struct Range {
    base: u32,
    len: u32,
}

impl Range {
    /// Return `Some(offset)` if addr is contained in `self`
    pub fn contains(self, addr: u32) -> Option<u32> {
        if addr >= self.base && addr <= self.base + (self.len - 1) {
            Some(addr - self.base)
        } else {
            None
        }
    }
}

const ROM: Range = Range {
    base: 0x2000_0000,
    len: 64 * 1024 * 1024,
};

const CART_MAGIC: [u8; 8] = *b"NR32CRT0";
