use crate::utils::format_size;
use anyhow::{Context, Result};
use goblin::elf::Elf;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

pub struct Cart {
    buf: Vec<u8>,
    op_index: usize,
}

impl Cart {
    pub fn new() -> Cart {
        let mut buf = vec![0xff; 0x100];

        buf[..8].clone_from_slice(&CART_MAGIC);

        Cart { buf, op_index: 1 }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
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

                self.copy_data(data, ph.p_paddr)
                    .with_context(|| format!("Copying data from bootloader section {pi}"))?;
            }
        }

        Ok(())
    }

    pub fn load_main<P: AsRef<Path>>(&mut self, elf_path: P, stack_size: u32) -> Result<()> {
        let mut f = File::open(elf_path)?;
        let mut elf_raw = Vec::new();

        f.read_to_end(&mut elf_raw)?;

        let elf = Elf::parse(&elf_raw)?;

        let mut ram_max = RAM.base + 32 * 1024;

        for (pi, ph) in elf.program_headers.iter().enumerate() {
            if ph.p_memsz > 0 {
                let mem_end = ph.p_vaddr + ph.p_memsz;
                if is_addr_in_ram(mem_end).is_ok() && (mem_end as u32) > ram_max {
                    ram_max = mem_end as u32;
                }
            }
            if ph.p_type == goblin::elf::program_header::PT_LOAD && ph.p_memsz > 0 {
                if ph.p_filesz > 0 {
                    let data = &elf_raw[ph.file_range()];

                    debug!("Copying main section {}: {:?}", pi, ph);

                    if ph.p_filesz != ph.p_memsz {
                        bail!("Section {} file size differs from mem size?", pi);
                    }

                    self.copy_data(data, ph.p_paddr)
                        .with_context(|| format!("Copying data from main section {pi}"))?;

                    if ph.p_vaddr != ph.p_paddr {
                        is_range_in_ram(ph.p_vaddr, ph.p_memsz)?;
                        self.add_op(
                            *b"COPY",
                            [ph.p_paddr as u32, ph.p_vaddr as u32, ph.p_filesz as u32],
                        )?;
                    }
                } else {
                    // Size in memory but no size in file -> BSS
                    debug!("Found BSS section {}: {:?}", pi, ph);
                    is_range_in_ram(ph.p_vaddr, ph.p_memsz)?;
                    self.add_op(*b"ZERO", [ph.p_vaddr as u32, ph.p_memsz as u32, 0])?;
                }
            }
        }

        let mut gp = 0u32;

        for sym in elf.syms.iter() {
            if let Some(name) = elf.strtab.get_at(sym.st_name) {
                if name == "__global_pointer$" {
                    is_addr_in_ram_or_rom(sym.st_value)
                        .with_context(|| format!("Loading GP value 0x{:x}", sym.st_value))?;
                    gp = sym.st_value as u32;
                }
            }
        }

        if ram_max > RAM.base + RAM.len {
            bail!("Overflowed RAM");
        }

        let heap_start = (ram_max + 0xf) & !0xf;
        let heap_size = (RAM.base + RAM.len) - heap_start;

        info!(
            "Free memory after static section alloc: {}",
            format_size(heap_size as _)
        );

        if heap_size < stack_size {
            bail!(
                "Not enough memory left to allocate a stack of {}",
                format_size(stack_size as _)
            );
        }

        debug!("GP value is 0x{:x}", gp);
        info!(
            "Main heap allocated at 0x{:x} with a size of {}",
            heap_start,
            format_size(heap_size as _)
        );

        self.add_op(*b"HEAP", [heap_start, heap_size, 0])?;

        is_addr_in_ram_or_rom(elf.entry).context("Main entry point")?;

        debug!("Main program entry @ 0x{:x}", elf.entry);

        self.add_op(*b"EXEC", [elf.entry as u32, stack_size, gp])?;

        Ok(())
    }

    fn copy_data(&mut self, data: &[u8], load_addr: u64) -> Result<()> {
        is_range_in_rom(load_addr, data.len() as u64)?;

        let cart_start = (load_addr as usize) - ROM.base as usize;
        let cart_end = cart_start + data.len();

        if cart_end > self.buf.len() {
            self.buf.resize(cart_end, 0xff);
        }

        for (off, (t, f)) in self.buf[cart_start..cart_end]
            .iter_mut()
            .zip(data.iter())
            .enumerate()
        {
            if *t != 0xff {
                bail!(
                    "Data conflict at ROM address 0x{:x}",
                    ROM.base as usize + cart_start + off
                );
            }

            *t = *f;
        }

        Ok(())
    }

    fn add_op(&mut self, op: [u8; 4], params: [u32; 3]) -> Result<()> {
        let desc = String::from_utf8_lossy(&op);

        let mut off = self.op_index * 16;

        if off >= 0x100 {
            bail!("No space left in header to add {} operation", desc);
        }

        self.op_index += 1;

        self.buf[off..(off + 4)].clone_from_slice(&op);

        for p in params {
            off += 4;
            self.buf[off..(off + 4)].clone_from_slice(&p.to_le_bytes());
        }

        Ok(())
    }
}

fn is_addr_in_ram_or_rom(addr: u64) -> Result<()> {
    if is_addr_in_ram(addr).is_err() && is_addr_in_rom(addr).is_err() {
        bail!("0x{:x} isn't a valid RAM or ROM address!", addr);
    }

    Ok(())
}

fn is_addr_in_ram(addr: u64) -> Result<()> {
    if addr > u32::MAX as u64 || RAM.contains(addr as u32).is_none() {
        bail!("0x{:x} isn't a valid RAM address!", addr);
    }

    Ok(())
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

fn is_range_in_ram(start: u64, len: u64) -> Result<()> {
    is_addr_in_ram(start)?;
    is_addr_in_ram(start + len)
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

const RAM: Range = Range {
    base: 0x0000_0000,
    len: 2 * 1024 * 1024,
};

const CART_MAGIC: [u8; 8] = *b"NR32CRT0";
