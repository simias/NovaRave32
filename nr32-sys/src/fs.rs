//! NR32 Cartridge filesystem handling

use crate::adler32::adler32;
use crate::syscall::{SysError, SysResult};
use core::fmt;
use core::slice;
use core::str;
use nr32_common::bootscript;
use nr32_common::memmap::ROM;

pub struct Fs {
    mem: &'static [u8],
}

impl Fs {
    pub fn from_bootscript_with_code(code: [u8; 4]) -> SysResult<Fs> {
        match bootscript::get().find(|e| e.code == code) {
            Some(entry) => {
                let fs_off = entry.params[0] as usize;
                let fs_len = entry.params[1] as usize;

                Fs::from_rom(fs_off, fs_len)
            }
            None => Err(SysError::NoEnt),
        }
    }

    pub fn from_bootscript() -> SysResult<Fs> {
        Fs::from_bootscript_with_code(*b"%FSM")
    }

    pub fn from_rom(fs_off: usize, fs_len: usize) -> SysResult<Fs> {
        let fs_start = ROM.base as usize + fs_off;
        let fs_end = fs_start + fs_len;

        if ROM.contains(fs_start as u32).is_none() || ROM.contains(fs_end as u32).is_none() {
            error!("Filesystem 0x{:x}:{} is off of bounds", fs_off, fs_len);
            return Err(SysError::Invalid);
        }

        if fs_len < 32 {
            error!("Filesystem 0x{:x}:{} is too small", fs_off, fs_len);
            return Err(SysError::Invalid);
        }

        let hdr = unsafe { slice::from_raw_parts(fs_start as *const u8, fs_len) };

        if &hdr[0..4] != b"NRFS" {
            error!("Filesystem 0x{:x}:{} has bad magic", fs_off, fs_len);
            return Err(SysError::Invalid);
        }

        let l = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]) as usize + 16;

        if l > fs_len {
            error!("Filesystem 0x{:x}:{} is truncated", fs_off, fs_len);
            return Err(SysError::Invalid);
        }

        let mem = unsafe { slice::from_raw_parts(fs_start as *const u8, l) };

        Ok(Fs { mem })
    }

    pub fn fsck(&self) -> SysResult<FsckStats> {
        // Start with the full FS checksum check
        let fs_csum = u32::from_le_bytes([self.mem[8], self.mem[9], self.mem[10], self.mem[11]]);

        let csum = adler32(&self.mem[12..]);

        if csum != fs_csum {
            error!("Bad FS CSUM! (expected {:x} got {:x})", csum, fs_csum);
            return Err(SysError::Invalid);
        }

        let mut fsck_st = FsckStats {
            dir_count: 0,
            file_count: 0,
            bad_files: 0,
            file_bytes: 0,
        };

        self.visit(|e| {
            match e.ty()? {
                EntryType::Directory => fsck_st.dir_count += 1,
                EntryType::File => {
                    fsck_st.file_count += 1;
                    let expected_csum = e.csum();
                    let contents = e.contents()?;

                    let csum = adler32(contents);

                    if csum != expected_csum {
                        error!("Invalid CSUM for {}", e.name_str()?);
                        fsck_st.bad_files += 1;
                    }
                    fsck_st.file_bytes += contents.len();
                }
            }
            Ok(FsVisitRet::Recurse)
        })?;

        Ok(fsck_st)
    }

    pub fn visit<V>(&self, mut visitor: V) -> SysResult<Option<FsEntry>>
    where
        V: FnMut(FsEntry) -> SysResult<FsVisitRet>,
    {
        self.visit_r(&mut visitor, 16)
    }

    pub fn visit_r<V>(&self, visitor: &mut V, mut pos: usize) -> SysResult<Option<FsEntry>>
    where
        V: FnMut(FsEntry) -> SysResult<FsVisitRet>,
    {
        while pos > 0 {
            let entry = self.entry_at(pos)?;

            pos = entry.next();

            match visitor(entry)? {
                FsVisitRet::End => return Ok(Some(entry)),
                FsVisitRet::Ignore => (),
                FsVisitRet::Recurse => {
                    if entry.is_dir() {
                        if let Some(e) = self.visit_r(visitor, entry.pos() + 32)? {
                            return Ok(Some(e));
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    pub fn entry_at(&self, pos: usize) -> SysResult<FsEntry> {
        if pos == 0 || pos & 0xf != 0 {
            // Entries are always 16-byte aligned
            return Err(SysError::Invalid);
        }

        if pos + 32 > self.mem.len() {
            return Err(SysError::Invalid);
        }

        Ok(FsEntry { pos, mem: self.mem })
    }
}

#[derive(Copy, Clone)]
pub struct FsEntry {
    pos: usize,
    mem: &'static [u8],
}

impl FsEntry {
    pub fn next(&self) -> usize {
        let n = u32::from_le_bytes([
            self.mem[self.pos + 0],
            self.mem[self.pos + 1],
            self.mem[self.pos + 2],
            self.mem[self.pos + 3],
        ]);

        (n & !0xf) as usize
    }

    pub fn length(&self) -> usize {
        u32::from_le_bytes([
            self.mem[self.pos + 4],
            self.mem[self.pos + 5],
            self.mem[self.pos + 6],
            self.mem[self.pos + 7],
        ]) as usize
    }

    pub fn contents(&self) -> SysResult<&'static [u8]> {
        let file_start = self.pos + 32;
        let file_end = file_start + self.length();

        if !self.is_file() || file_end > self.mem.len() {
            return Err(SysError::Invalid);
        }

        Ok(&self.mem[file_start..file_end])
    }

    pub fn csum(&self) -> u32 {
        u32::from_le_bytes([
            self.mem[self.pos + 8],
            self.mem[self.pos + 9],
            self.mem[self.pos + 10],
            self.mem[self.pos + 11],
        ])
    }

    pub fn name(&self) -> &[u8] {
        let n = &self.mem[(self.pos + 16)..(self.pos + 32)];

        match n.iter().position(|&c| c == 0) {
            Some(p) => &n[0..p],
            None => n,
        }
    }

    pub fn name_str(&self) -> SysResult<&str> {
        str::from_utf8(self.name()).map_err(|_| SysError::Invalid)
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn ty(&self) -> SysResult<EntryType> {
        let ty = match self.mem[self.pos] & 0xf {
            1 => EntryType::Directory,
            2 => EntryType::File,
            _ => return Err(SysError::Invalid),
        };

        Ok(ty)
    }

    pub fn is_dir(&self) -> bool {
        self.ty() == Ok(EntryType::Directory)
    }

    pub fn is_file(&self) -> bool {
        self.ty() == Ok(EntryType::File)
    }
}

#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub enum EntryType {
    Directory = 1,
    File = 2,
}

pub enum FsVisitRet {
    /// Stop the visit and return the current entry
    End,
    /// If the currently visited entry is a directory, visit its contents. If it's a file does the
    /// same thing as Ignore.
    Recurse,
    /// Move on to the next entry. If the current entry is a directory, its contents won't be
    /// visited.
    Ignore,
}

#[derive(Debug)]
pub struct FsckStats {
    /// Total dir count
    pub dir_count: usize,
    /// Total file count
    pub file_count: usize,
    /// Total dir count
    pub bad_files: usize,
    /// Sum of every file size
    pub file_bytes: usize,
}

impl fmt::Display for FsckStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} files ({}B) in {} directories [{} bad checksums]",
            self.file_count, self.file_bytes, self.dir_count, self.bad_files
        )
    }
}
