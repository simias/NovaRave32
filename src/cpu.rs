//! Implementation of the RISC-rfdV RV32IMAC ISA

mod decoder;

use crate::{Machine, RAM, ROM};
use decoder::{Decoder, Instruction};
use std::fmt;

pub struct Cpu {
    /// Instruction decoder
    decoder: Decoder,
    /// Program Counter
    pc: u32,
    /// 32 general purpose registers (x0 must always be 0). The additional register at the end is
    /// used as a target for writes to R0
    x: [u32; 33],
    /// Machine status
    mstatus: u32,
    /// Machine Interrupt Enable
    mie: u32,
    /// Machine Interrupt Pending
    mip: u32,
    /// Machine Trap Vector base address
    mtvec: u32,
    /// Machine Exception Program Counter
    mepc: u32,
    /// Contains the address of the last "Load-Reserved" instruction as long as it remains valid
    reservation: Option<u32>,
}

impl Cpu {
    pub fn new() -> Cpu {
        Cpu {
            decoder: Decoder::new(),
            pc: ROM.base,
            x: [0; 33],
            mstatus: 0,
            mie: 0,
            mip: 0,
            mtvec: ROM.base,
            mepc: 0,
            reservation: None,
        }
    }

    /// Set register value. Panics if the register is out of range.
    fn xset(&mut self, reg: Reg, v: u32) {
        debug_assert!(reg != Reg::ZERO || v == 0);
        self.x[reg.0 as usize] = v;
    }

    /// Get register value. Panics if the register index is out of range
    fn xget(&mut self, reg: Reg) -> u32 {
        debug_assert!(reg != Reg::DUMMY);
        self.x[reg.0 as usize]
    }

    /// Set a new value for the given Control and Status Register, returning the previous value
    fn csr_and_or(&mut self, csr: u16, and_mask: u32, or_mask: u32) -> u32 {
        // TODO Check CSR privileges and raise illegal-instruction if there's a violation
        // See "2.1. CSR Address Mapping Conventions" in the privileged architecture manual.

        debug!("CSR SET *{:x} & {:x} | {:x}", csr, and_mask, or_mask);

        let update_csr = |reg: &mut u32| -> u32 {
            let prev = *reg;

            *reg &= and_mask;
            *reg |= or_mask;

            prev
        };

        match csr {
            CSR_MSTATUS => {
                if or_mask != 0 {
                    panic!("MSTATUS set {:x}", or_mask)
                }

                update_csr(&mut self.mstatus)
            }
            CSR_MIE => {
                if or_mask != 0 {
                    panic!("IRQ en {:x}", or_mask);
                }
                update_csr(&mut self.mie)
            }
            CSR_MEPC => update_csr(&mut self.mepc),
            CSR_MIP => update_csr(&mut self.mip),
            CSR_MTVEC => update_csr(&mut self.mtvec),
            _ => panic!("Unhandled CSR {:x} {:?}", csr, self),
        }
    }

    fn csr_set(&mut self, csr: u16, v: u32) -> u32 {
        self.csr_and_or(csr, 0, v)
    }
}

impl fmt::Debug for Cpu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f)?;
        writeln!(f, "pc : {:08x}", self.pc)?;
        for r in 0..16 {
            writeln!(
                f,
                "x{:<2} / {:<3}: {:>8x}      x{:<2} / {:<3}: {:>8x}",
                r,
                REGNAMES[r],
                self.x[r],
                r + 16,
                REGNAMES[r + 16],
                self.x[r + 16]
            )?;
        }

        Ok(())
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
struct Reg(u8);

impl Reg {
    const ZERO: Reg = Reg(0);
    const RA: Reg = Reg(1);
    const SP: Reg = Reg(2);
    const DUMMY: Reg = Reg(32);

    fn out(self) -> Reg {
        if self == Reg::ZERO {
            // Can't write to R0
            Reg::DUMMY
        } else {
            self
        }
    }
}

impl fmt::Debug for Reg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if *self != Reg::DUMMY {
            write!(f, "x{}/{}", self.0, REGNAMES[self.0 as usize])
        } else {
            write!(f, "{}", REGNAMES[self.0 as usize])
        }
    }
}

static REGNAMES: [&str; 33] = [
    "z0", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2", "a3", "a4", "a5",
    "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11", "t3", "t4", "t5",
    "t6", "r_",
];

pub fn step(m: &mut Machine) {
    let pc = m.cpu.pc;

    let (inst, npc) = decoder::fetch_instruction(m, pc);
    m.cpu.pc = npc;

    // info!("{:x} {:x?}", pc, inst);

    match inst {
        Instruction::InvalidAddress(add) => panic!("Can't fetch instruction at {:x}", add),
        Instruction::Li { rd, imm } => m.cpu.xset(rd, imm),
        Instruction::Move { rd, rs1 } => {
            let v = m.cpu.xget(rs1);
            m.cpu.xset(rd, v);
        }
        Instruction::Add { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            m.cpu.xset(rd, a.wrapping_add(b));
        }
        Instruction::Sub { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            m.cpu.xset(rd, a.wrapping_sub(b));
        }
        Instruction::Mul { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            m.cpu.xset(rd, a.wrapping_mul(b));
        }
        Instruction::Mulhu { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            let p = u64::from(a) * u64::from(b);

            m.cpu.xset(rd, (p >> 32) as u32);
        }
        Instruction::Divu { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            let d = if b == 0 { !0 } else { a / b };

            m.cpu.xset(rd, d);
        }
        Instruction::AddImm { rd, rs1, imm } => {
            let v = m.cpu.xget(rs1);

            m.cpu.xset(rd, v.wrapping_add(imm.extend()));
        }
        Instruction::OrImm { rd, rs1, imm } => {
            let v = m.cpu.xget(rs1);

            m.cpu.xset(rd, v | imm.extend());
        }
        Instruction::AndImm { rd, rs1, imm } => {
            let v = m.cpu.xget(rs1);

            m.cpu.xset(rd, v & imm.extend());
        }
        Instruction::SrlImm { rd, rs1, shamt } => {
            let v = m.cpu.xget(rs1);

            m.cpu.xset(rd, v.checked_shr(u32::from(shamt)).unwrap_or(0));
        }
        Instruction::SllImm { rd, rs1, shamt } => {
            let v = m.cpu.xget(rs1);

            m.cpu.xset(rd, v.checked_shl(u32::from(shamt)).unwrap_or(0));
        }
        Instruction::Jal { rd, tpc } => {
            m.cpu.xset(rd, m.cpu.pc);
            m.cpu.pc = tpc;
        }
        Instruction::Jalr { rd, rs1, off } => {
            let base = m.cpu.xget(rs1);

            let target = base.wrapping_add(off.extend()) & !1;

            m.cpu.xset(rd, m.cpu.pc);
            m.cpu.pc = target;
        }
        Instruction::Beq { rs1, rs2, tpc } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            if a == b {
                m.cpu.pc = tpc;
            }
        }
        Instruction::Bne { rs1, rs2, tpc } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            if a != b {
                m.cpu.pc = tpc;
            }
        }
        Instruction::Bltu { rs1, rs2, tpc } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            if a < b {
                m.cpu.pc = tpc;
            }
        }
        Instruction::Bgeu { rs1, rs2, tpc } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            if a >= b {
                m.cpu.pc = tpc;
            }
        }
        Instruction::Lbu { rd, rs1, off } => {
            let base = m.cpu.xget(rs1);
            let addr = base.wrapping_add(off.extend());

            let v = m.load_byte(addr);
            m.cpu.xset(rd, v as u32)
        }
        Instruction::Lw { rd, rs1, off } => {
            let base = m.cpu.xget(rs1);
            let addr = base.wrapping_add(off.extend());

            if addr & 3 == 0 {
                let v = m.load_word(addr);
                m.cpu.xset(rd, v)
            } else {
                panic!("Misaligned store {:x} {:?}", addr, m.cpu);
            }
        }
        Instruction::Sb { rs1, rs2, off } => {
            let base = m.cpu.xget(rs1);
            let v = m.cpu.xget(rs2);

            let addr = base.wrapping_add(off.extend());

            m.store_byte(addr, v as u8);
        }
        Instruction::Sh { rs1, rs2, off } => {
            let base = m.cpu.xget(rs1);
            let v = m.cpu.xget(rs2);

            let addr = base.wrapping_add(off.extend());

            if addr & 1 == 0 {
                m.store_halfword(addr, v as u16);
            } else {
                panic!("Misaligned SH {:x} {:?}", addr, m.cpu);
            }
        }
        Instruction::Sw { rs1, rs2, off } => {
            let base = m.cpu.xget(rs1);
            let v = m.cpu.xget(rs2);

            let addr = base.wrapping_add(off.extend());

            if addr & 3 == 0 {
                m.store_word(addr, v);
            } else {
                panic!("Misaligned SW {:x} {:?}", addr, m.cpu);
            }
        }
        Instruction::Lrw { rd, rs1 } => {
            let addr = m.cpu.xget(rs1);

            // Invalidate any previous reservation
            m.cpu.reservation = None;

            if addr & 3 == 0 {
                match RAM.contains(addr) {
                    Some(off) => {
                        let v = m.ram[(off >> 2) as usize];

                        m.cpu.reservation = Some(addr);

                        m.cpu.xset(rd, v)
                    }
                    None => panic!("LR.W not targeting RAM! {:x} {:?}", addr, m.cpu),
                }
            } else {
                panic!("Misaligned store {:x} {:?}", addr, m.cpu);
            }
        }
        Instruction::Scw { rd, rs1, rs2 } => {
            // Invalidate any previous reservation
            let reservation = m.cpu.reservation.take();
            let addr = m.cpu.xget(rs1);

            let r_valid = match reservation {
                Some(r_addr) => r_addr == addr,
                None => false,
            };

            let mut result = 1;

            if r_valid {
                if let Some(off) = RAM.contains(addr) {
                    m.ram[(off >> 2) as usize] = m.cpu.xget(rs2);
                    // Success
                    result = 0;
                }
            }

            m.cpu.xset(rd, result)
        }
        Instruction::CsrSet { rd, csr, rs1 } => {
            let v = m.cpu.xget(rs1);

            let prev = m.cpu.csr_set(csr, v);

            m.cpu.xset(rd, prev);
        }
        Instruction::CsrClearBits { rd, csr, rs1 } => {
            let v = m.cpu.xget(rs1);

            let prev = m.cpu.csr_and_or(csr, !v, 0);

            m.cpu.xset(rd, prev);
        }
        Instruction::CsrManipImm {
            rd,
            csr,
            and_mask,
            or_mask,
        } => {
            let prev = m.cpu.csr_and_or(csr, and_mask.extend(), or_mask.extend());

            m.cpu.xset(rd, prev);
        }
        Instruction::MRet => {
            // XXX handle mode stuff
            m.cpu.pc = m.cpu.mepc;
        }
        Instruction::Unknown32(op) => {
            panic!("Encountered unknown instruction {:x} {:?}", op, m.cpu)
        }
        Instruction::Unknown16(op) => panic!(
            "Encountered unknown compressed instruction {:x} {:?}",
            op, m.cpu
        ),
    }
}

const CSR_MSTATUS: u16 = 0x300;
const CSR_MIE: u16 = 0x304;
const CSR_MTVEC: u16 = 0x305;
const CSR_MEPC: u16 = 0x341;
const CSR_MIP: u16 = 0x344;

/// Trait used to sign-extend various types to 32bits
trait Extendable {
    fn extend(self) -> u32;
}

impl Extendable for i8 {
    fn extend(self) -> u32 {
        (self as i32) as u32
    }
}

impl Extendable for i16 {
    fn extend(self) -> u32 {
        (self as i32) as u32
    }
}

impl Extendable for i32 {
    fn extend(self) -> u32 {
        self as u32
    }
}
