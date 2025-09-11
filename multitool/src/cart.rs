use crate::utils::format_size;
use adler32::adler32;
use anyhow::{Context, Result};
use goblin::elf::Elf;
use nr32_common::memmap::{RAM, ROM};
use std::cmp::Ordering;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

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

    pub fn load_fs<P: AsRef<Path>>(&mut self, fs_path: P) -> Result<()> {
        let root = FsEntry::from_path(fs_path)?;

        let fs_start = align_up(self.buf.len(), 1024);
        self.buf.resize(fs_start, 0xff);

        info!("Dumping cart filesystem at offset {}", fs_start);

        self.buf.extend_from_slice(b"NRFS");

        // Length
        self.buf.extend_from_slice(b"\0\0\0\0");
        // Checksum
        self.buf.extend_from_slice(b"\0\0\0\0");
        // Reserved
        self.buf.extend_from_slice(b"\0\0\0\0");

        let top_header = self.buf.len();
        root.dump(&mut self.buf, fs_start)?;

        // Erase the next pointer for the top header
        self.buf[top_header] &= 0xf;
        self.buf[top_header + 1] = 0;
        self.buf[top_header + 2] = 0;
        self.buf[top_header + 3] = 0;

        let fs = &mut self.buf[fs_start..];
        let fs_len = (fs.len() - 16) as u32;

        let csum = adler32(&fs[12..])?;
        fs[4..8].copy_from_slice(&fs_len.to_le_bytes());
        fs[8..12].copy_from_slice(&csum.to_le_bytes());

        self.add_op(*b"%FSM", [fs_start as u32, fs_len + 16, 0])?;

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

const CART_MAGIC: [u8; 8] = *b"NR32CRT0";

#[derive(Debug)]
struct FsEntry {
    path: PathBuf,
    payload: FsPayload,
}

#[derive(Debug)]
enum FsPayload {
    File { size: u64 },
    Directory { entries: Vec<FsEntry> },
}

impl FsEntry {
    fn from_path<P: AsRef<Path>>(fs_path: P) -> Result<FsEntry> {
        let path = fs_path.as_ref().to_path_buf();

        let meta = path.metadata()?;

        let payload = if meta.is_dir() {
            let mut entries = fs::read_dir(fs_path)?
                .map(|e| -> Result<FsEntry> {
                    let e = e?;
                    FsEntry::from_path(e.path())
                })
                .collect::<Result<Vec<FsEntry>>>()?;

            // Put directories first, then files
            entries.sort_by(|a, b| match a.is_file().cmp(&b.is_file()) {
                Ordering::Equal => a.path.file_name().cmp(&b.path.file_name()),
                c => c,
            });

            FsPayload::Directory { entries }
        } else if meta.is_file() {
            FsPayload::File { size: meta.len() }
        } else {
            bail!(
                "Found non-file, non-dir entry {}: {:?}",
                path.display(),
                meta
            );
        };

        Ok(FsEntry { path, payload })
    }

    fn is_file(&self) -> bool {
        matches!(self.payload, FsPayload::File { .. })
    }

    fn dump(&self, buf: &mut Vec<u8>, fs_start: usize) -> Result<()> {
        let start = buf.len();

        // Next header
        buf.extend_from_slice(&[0; 4]);

        let etype;

        match self.payload {
            FsPayload::File { size } => {
                etype = 2;
                // File length
                buf.extend_from_slice(&(size as u32).to_le_bytes());

                // CSUM
                buf.extend_from_slice(&[0; 4]);

                // PAD
                buf.extend_from_slice(&[0; 4]);

                self.write_filename(buf)?;

                let mut f = File::open(&self.path)?;

                let fstart = buf.len();
                f.read_to_end(buf)?;

                let flen = buf.len() - fstart;

                if flen as u64 != size {
                    bail!("File {} changed while dumping!", self.path.display());
                }

                let csum = adler32(&buf[fstart..])?;
                buf[(start + 8)..(start + 12)].copy_from_slice(&csum.to_le_bytes());
            }
            FsPayload::Directory { ref entries } => {
                etype = 1;

                // Number of entries
                buf.extend_from_slice(&(entries.len() as u32).to_le_bytes());
                // PAD
                buf.extend_from_slice(&[0; 8]);

                self.write_filename(buf)?;

                let mut last_header = 0;

                for e in entries {
                    last_header = buf.len();

                    e.dump(buf, fs_start)?;
                }

                if last_header != 0 {
                    // Erase the next pointer for the last header
                    buf[last_header] &= 0xf;
                    buf[last_header + 1] = 0;
                    buf[last_header + 2] = 0;
                    buf[last_header + 3] = 0;
                }
            }
        }

        let next_start = align_up(buf.len(), 16);

        buf.resize(next_start, 0);

        let nt = ((next_start - fs_start) as u32) | etype;

        buf[start..(start + 4)].copy_from_slice(&nt.to_le_bytes());

        Ok(())
    }

    fn write_filename(&self, buf: &mut Vec<u8>) -> Result<()> {
        let fname = self.path.file_name().unwrap_or_else(|| OsStr::new(""));

        let bytes = fname.as_encoded_bytes();

        if bytes.len() > 16 {
            bail!("File name is longer than 16 chars: {}", self.path.display());
        }

        buf.extend_from_slice(bytes);

        for _ in bytes.len()..16 {
            buf.push(0);
        }

        Ok(())
    }
}

/// Align `addr` to `alignment` (which should be a power of 2), rounding up
const fn align_up(addr: usize, align: usize) -> usize {
    align_down(addr.wrapping_add(align - 1), align)
}

/// Align `addr` to `alignment` (which should be a power of 2), rounding down
const fn align_down(addr: usize, align: usize) -> usize {
    addr & !(align - 1)
}
