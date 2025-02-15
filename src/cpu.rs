//! Implementation of the RISC-rfdV RV32IMAC ISA

use crate::{Machine, RAM, ROM};
use std::fmt;
use log::info;

pub struct Cpu {
    /// Program Counter
    pc: u32,
    /// 32 general purpose registers (x0 must always be 0)
    x: [u32; 32],
    /// Machine status
    mstatus: u32,
    /// Machine Interrupt Enable
    mie: u32,
    /// Machine Interrupt Pending
    mip: u32,
    /// Machine Trap Vector base address
    mtvec: u32,
    /// Contains the address of the last "Load-Reserved" instruction as long as it remains valid
    reservation: Option<u32>,
}

impl Cpu {
    pub fn new() -> Cpu {
        Cpu {
            pc: ROM.base,
            x: [0; 32],
            mstatus: 0,
            mie: 0,
            mip: 0,
            mtvec: ROM.base,
            reservation: None,
        }
    }

    /// Set register value (does nothing for x0). Panics if the register index is >= 32.
    fn xset(&mut self, reg: Reg, v: u32) {
        self.x[reg.0 as usize] = v;
        self.x[0] = 0;
    }

    /// Get register value. Panics if the register index is >= 32.
    fn xget(&mut self, reg: Reg) -> u32 {
        self.x[reg.0 as usize]
    }

    /// Set a new value for the given Control and Status Register, returning the previous value
    fn csr_and_or(&mut self, csr: u32, and_mask: u32, or_mask: u32) -> u32 {
        // TODO Check CSR privileges and raise illegal-instruction if there's a violation
        // See "2.1. CSR Address Mapping Conventions" in the privileged architecture manual.

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
            CSR_MIP => update_csr(&mut self.mip),
            CSR_MTVEC => update_csr(&mut self.mtvec),
            _ => panic!("Unhandled CSR {:x} {:?}", csr, self),
        }
    }

    fn csr_set(&mut self, csr: u32, v: u32) -> u32 {
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

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct Reg(u8);

impl Reg {
    const ZERO: Reg = Reg(0);
    const RA: Reg = Reg(1);
    const SP: Reg = Reg(2);
}

pub fn step(m: &mut Machine) {
    let pc = m.cpu.pc;

    if pc & 1 != 0 {
        panic!("Misaligned PC! {:?}", m.cpu);
    }

    // println!("{:?}", m.cpu);

    let op = m.fetch_instruction(pc);

    if op & 3 == 3 {
        // 32bit instruction
        m.cpu.pc += 4;

        // We take [op][funct3] as jump index
        let code = ((op >> 12) & 7) | ((op & 0x7c) << 1);

        INSTRUCTIONS_32[code as usize](m, op);
    } else {
        // Compressed instruction
        let op = op as u16;
        m.cpu.pc += 2;

        // We take [op][funct4] as jump index
        let code = ((op >> 10) & 0x3f) | ((op & 3) << 6);

        INSTRUCTIONS_16[code as usize](m, op);
    }
}

fn check_sw(m: &mut Machine, addr: u32) -> bool {
    if addr & 3 == 0 {
        true
    } else {
        // "Behaviour dependent on the EEI". In practice we should probably raise
        // an address-misaligned trap
        panic!("Misaligned store {:x} {:?}", addr, m.cpu);
    }
}

fn check_lw(m: &mut Machine, addr: u32) -> bool {
    if addr & 3 == 0 {
        true
    } else {
        // "Behaviour dependent on the EEI". In practice we should probably raise
        // an address-misaligned trap
        panic!("Misaligned load {:x} {:?}", addr, m.cpu);
    }
}

/// Destination register
fn rd(op: u32) -> Reg {
    Reg(((op >> 7) & 0x1f) as u8)
}

/// Source register 1
fn rs1(op: u32) -> Reg {
    Reg(((op >> 15) & 0x1f) as u8)
}

/// Source register 2
fn rs2(op: u32) -> Reg {
    Reg(((op >> 20) & 0x1f) as u8)
}

/// Signed immediate value in bits [31:20]
///
/// Returns the sign-extended 32bit value
fn imm_20_se(op: u32) -> u32 {
    let op = op as i32;

    (op >> 20) as u32
}

/// Unsigned immediate value in bits [31:20]
fn imm_20(op: u32) -> u32 {
    op >> 20
}

/// Store offset
///
/// Returns the sign-extended 32bit value
fn store_off(op: u32) -> u32 {
    let mut off = ((op as i32) >> 20) as u32;

    off &= !0x1f;
    off |= (op >> 7) & 0x1f;

    off
}

/// Branch offset, sign-extended
fn boff(op: u32) -> u32 {
    // Sign bit
    let mut off = ((op as i32) >> 19) as u32;
    off &= !0xfff;
    // Bit 11
    off |= (op << 4) & (1 << 11);
    // Bits 10:5
    off |= (op >> 20) & (0x3f << 5);
    // Bits 4:1
    off |= (op >> 7) & (0xf << 1);

    off
}

/// JAL offset, sign-extended
fn jal_off(op: u32) -> u32 {
    // Sign bit
    let mut off = ((op as i32) >> 11) as u32;
    off &= !0xfffff;
    off |= op & (0xff << 12);
    // Bit 11
    off |= (op >> 9) & (1 << 11);
    // Bits 10:1
    off |= (op >> 20) & (0x3ff << 1);

    off
}

fn funct5(op: u32) -> u32 {
    op >> 27
}

fn shamt(op: u32) -> u32 {
    (op >> 20) & 0x1f
}

fn i_xxx(m: &mut Machine, op: u32) {
    let code = ((op >> 12) & 7) | ((op & 0x7c) << 1);

    panic!("i_xxx {:x} [{:x}] {:?}", op, code, m.cpu);
}

fn i_fence(_m: &mut Machine, _op: u32) {
    // NOP
}

fn i_addi(m: &mut Machine, op: u32) {
    let v = m.cpu.xget(rs1(op));
    let i = imm_20_se(op);

    m.cpu.xset(rd(op), v.wrapping_add(i));
}

fn i_slli(m: &mut Machine, op: u32) {
    assert_eq!(op >> 25, 0);

    let v = m.cpu.xget(rs1(op));
    let s = shamt(op);

    m.cpu.xset(rd(op), v.checked_shl(s).unwrap_or(0));
}

fn i_srli(m: &mut Machine, op: u32) {
    assert_eq!(op >> 25, 0);

    let v = m.cpu.xget(rs1(op));
    let s = shamt(op);

    m.cpu.xset(rd(op), v.checked_shr(s).unwrap_or(0));
}

fn i_add(m: &mut Machine, op: u32) {
    let v1 = m.cpu.xget(rs1(op));
    let v2 = m.cpu.xget(rs2(op));
    let sub = op >> 25;

    let r = match sub {
        // add
        0x00 => v1.wrapping_add(v2),
        // mul
        0x01 => v1.wrapping_mul(v2),
        // sub
        0x20 => v1.wrapping_sub(v2),
        _ => panic!("Unsupported op {:x}", op),
    };

    m.cpu.xset(rd(op), r)
}

fn i_mulhu(m: &mut Machine, op: u32) {
    assert_eq!(op >> 25, 1);

    let v1 = m.cpu.xget(rs1(op)) as u64;
    let v2 = m.cpu.xget(rs2(op)) as u64;

    let p = v1 * v2;

    m.cpu.xset(rd(op), (p >> 32) as u32);
}

fn i_lui(m: &mut Machine, op: u32) {
    m.cpu.xset(rd(op), op & 0xffff_f000)
}

fn i_xori(m: &mut Machine, op: u32) {
    let v = m.cpu.xget(rs1(op));
    let i = imm_20_se(op);

    m.cpu.xset(rd(op), v ^ i);
}

fn i_ori(m: &mut Machine, op: u32) {
    let v = m.cpu.xget(rs1(op));
    let i = imm_20_se(op);

    m.cpu.xset(rd(op), v | i);
}

fn i_andi(m: &mut Machine, op: u32) {
    let v = m.cpu.xget(rs1(op));
    let i = imm_20_se(op);

    m.cpu.xset(rd(op), v & i);
}

fn i_auipc(m: &mut Machine, op: u32) {
    let pc = m.cpu.pc.wrapping_sub(4);
    let off = op & 0xffff_f000;

    m.cpu.xset(rd(op), pc.wrapping_add(off));
}

fn i_lw(m: &mut Machine, op: u32) {
    let b = m.cpu.xget(rs1(op));
    let off = imm_20_se(op);

    let s = b.wrapping_add(off);

    if check_lw(m, s) {
        let v = m.load_word(s);
        m.cpu.xset(rd(op), v)
    }
}

fn i_lbu(m: &mut Machine, op: u32) {
    let b = m.cpu.xget(rs1(op));
    let off = imm_20_se(op);

    let s = b.wrapping_add(off);

    let v = m.load_byte(s);
    m.cpu.xset(rd(op), u32::from(v))
}

fn i_lr_w(m: &mut Machine, op: u32) {
    // Invalidate any previous reservation
    m.cpu.reservation = None;

    let addr = m.cpu.xget(rs1(op));

    if addr & 3 != 0 {
        // "Behaviour dependent on the EEI". In practice we should probably raise
        // an address-misaligned trap
        panic!("Misaligned load {:x} {:?}", addr, m.cpu);
    }

    if !check_lw(m, addr) {
        return;
    }

    match RAM.contains(addr) {
        Some(off) => {
            let v = m.ram[(off >> 2) as usize];

            m.cpu.reservation = Some(addr);

            m.cpu.xset(rd(op), v)
        }
        None => panic!("LR.W not targeting RAM! {:x} {:?}", addr, m.cpu),
    }
}

fn i_sr_w(m: &mut Machine, op: u32) {
    // Invalidate any previous reservation
    let reservation = m.cpu.reservation.take();
    let addr = m.cpu.xget(rs1(op));

    let r_valid = match reservation {
        Some(r_addr) => r_addr == addr,
        None => false,
    };

    let mut result = 1;

    if r_valid {
        if let Some(off) = RAM.contains(addr) {
            m.ram[(off >> 2) as usize] = m.cpu.xget(rs2(op));
            // Success
            result = 0;
        }
    }

    m.cpu.xset(rd(op), result)
}

fn i_atomic_w(m: &mut Machine, op: u32) {
    match funct5(op) {
        0b00010 => i_lr_w(m, op),
        0b00011 => i_sr_w(m, op),
        f5 => panic!("Unimplemented atomic operation {:x} {:x}", op, f5),
    }
}

fn i_sb(m: &mut Machine, op: u32) {
    let s = m.cpu.xget(rs1(op));
    let v = m.cpu.xget(rs2(op));
    let off = store_off(op);

    let d = s.wrapping_add(off);

    m.store_byte(d, v as u8)
}

fn i_sw(m: &mut Machine, op: u32) {
    let s = m.cpu.xget(rs1(op));
    let v = m.cpu.xget(rs2(op));
    let off = store_off(op);

    let d = s.wrapping_add(off);

    if check_sw(m, d) {
        m.store_word(d, v)
    }
}

fn i_beq(m: &mut Machine, op: u32) {
    let v1 = m.cpu.xget(rs1(op));
    let v2 = m.cpu.xget(rs2(op));
    let pc = m.cpu.pc.wrapping_sub(4);

    if v1 == v2 {
        let off = boff(op);
        m.cpu.pc = pc.wrapping_add(off);
    }
}

fn i_bne(m: &mut Machine, op: u32) {
    let v1 = m.cpu.xget(rs1(op));
    let v2 = m.cpu.xget(rs2(op));
    let pc = m.cpu.pc.wrapping_sub(4);

    if v1 != v2 {
        let off = boff(op);
        m.cpu.pc = pc.wrapping_add(off);
    }
}

fn i_bltu(m: &mut Machine, op: u32) {
    let v1 = m.cpu.xget(rs1(op));
    let v2 = m.cpu.xget(rs2(op));
    let pc = m.cpu.pc.wrapping_sub(4);

    if v1 < v2 {
        let off = boff(op);
        m.cpu.pc = pc.wrapping_add(off);
    }
}

fn i_bgeu(m: &mut Machine, op: u32) {
    let v1 = m.cpu.xget(rs1(op));
    let v2 = m.cpu.xget(rs2(op));
    let pc = m.cpu.pc.wrapping_sub(4);

    if v1 >= v2 {
        let off = boff(op);
        m.cpu.pc = pc.wrapping_add(off);
    }
}

fn i_jal(m: &mut Machine, op: u32) {
    let off = jal_off(op);
    let pc = m.cpu.pc.wrapping_sub(4);

    // Store return address
    m.cpu.xset(rd(op), m.cpu.pc);

    m.cpu.pc = pc.wrapping_add(off);
}

fn i_jalr(m: &mut Machine, op: u32) {
    let base = m.cpu.xget(rs1(op));

    // Store return address
    m.cpu.xset(rd(op), m.cpu.pc);

    let target = base.wrapping_add(imm_20_se(op));

    m.cpu.pc = target & !1;
}

fn i_csrrw(m: &mut Machine, op: u32) {
    let v = m.cpu.xget(rs1(op));
    let csr = imm_20(op);

    let prev = m.cpu.csr_set(csr, v);

    m.cpu.xset(rd(op), prev);
}

fn i_csrrs(m: &mut Machine, op: u32) {
    let v = m.cpu.xget(rs1(op));

    if v == 0 {
        // We're not setting any bits, therefore it's just a read. This is worth optimizing because
        // that's how the CSRR pseudo-instruction is implemented, by setting rd to x0
        panic!("CSR read {:?}", m.cpu);
    } else {
        panic!("CSR bit set");
    }
}

fn i_csrrci(m: &mut Machine, op: u32) {
    let v = rs1(op).0 as u32;
    let csr = imm_20(op);

    let prev = m.cpu.csr_and_or(csr, !v, 0);

    m.cpu.xset(rd(op), prev);
}

fn i_csrrwi(m: &mut Machine, op: u32) {
    let v = rs1(op).0 as u32;
    let csr = imm_20(op);

    let prev = m.cpu.csr_set(csr, v);

    m.cpu.xset(rd(op), prev);
}

static INSTRUCTIONS_32: [fn(&mut Machine, op: u32); 256] = [
    i_xxx, i_xxx, i_lw, i_xxx, i_lbu, i_xxx, i_xxx, i_xxx, // 07
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // 0F
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // 17
    i_fence, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // 1F
    i_addi, i_slli, i_xxx, i_xxx, i_xori, i_srli, i_ori, i_andi, // 27
    i_auipc, i_auipc, i_auipc, i_auipc, i_auipc, i_auipc, i_auipc, i_auipc, // 2F
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // 37
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // 3F
    i_sb, i_xxx, i_sw, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // 47
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // 4F
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // 57
    i_xxx, i_xxx, i_atomic_w, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // 5F
    i_add, i_xxx, i_xxx, i_mulhu, i_xxx, i_xxx, i_xxx, i_xxx, // 67
    i_lui, i_lui, i_lui, i_lui, i_lui, i_lui, i_lui, i_lui, // 6F
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // 77
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // 7F
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // 87
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // 8F
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // 97
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // 9F
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // A7
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // AF
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // B7
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // BF
    i_beq, i_bne, i_xxx, i_xxx, i_xxx, i_xxx, i_bltu, i_bgeu, // C7
    i_jalr, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // CF
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // D7
    i_jal, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // DF
    i_xxx, i_csrrw, i_csrrs, i_xxx, i_xxx, i_csrrwi, i_xxx, i_csrrci, // E7
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // EF
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // F7
    i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, i_xxx, // FF
];

/// Compressed source register 1
fn c_rs1(op: u16) -> Reg {
    Reg((((op >> 7) & 7) + 8) as u8)
}

/// Compressed source register 2
fn c_rs2(op: u16) -> Reg {
    Reg((((op >> 2) & 7) + 8) as u8)
}

/// Compressed destination register (full range)
fn c_frd(op: u16) -> Reg {
    Reg(((op >> 7) & 0x1f) as u8)
}

/// Compressed source register 2 (full range)
fn c_frs2(op: u16) -> Reg {
    Reg(((op >> 2) & 0x1f) as u8)
}

fn c_sw_lw_off(op: u16) -> u32 {
    let op = op as u32;
    let mut off = 0;

    off |= (op >> 7) & (7 << 3);
    off |= (op >> 4) & (1 << 2);
    off |= (op << 1) & (1 << 6);

    off
}

fn c_li_imm(op: u16) -> u32 {
    let op = op as u32;
    let mut off = (((op << 19) as i32) >> 26) as u32;
    off &= !0x1f;
    off |= (op >> 2) & 0x1f;

    off
}

fn c_boff(op: u16) -> u32 {
    let op = op as u32;
    let mut off = (((op << 19) as i32) >> 24) as u32;
    off &= !0x7f;
    off |= (op >> 8) & (3 << 2);
    off |= op & (3 << 5);
    off |= (op >> 3) & 3;
    off |= (op << 2) & (1 << 4);

    off << 1
}

fn c_joff(op: u16) -> u32 {
    let op = op as u32;
    let mut off = (((op << 19) as i32) >> 20) as u32;
    off &= !0x7ff;
    off |= (op >> 7) & (1 << 4);
    off |= (op >> 1) & (3 << 8);
    off |= (op << 2) & (1 << 10);
    off |= (op >> 1) & (1 << 6);
    off |= (op << 1) & (1 << 7);
    off |= (op >> 2) & (7 << 1);
    off |= (op << 3) & (1 << 5);

    off
}

fn c_addisp_off(op: u16) -> u32 {
    let op = op as u32;
    let mut off = (((op << 19) as i32) >> 22) as u32;
    off &= !0x1ff;
    off |= (op >> 2) & (1 << 4);
    off |= (op << 1) & (1 << 6);
    off |= (op << 4) & (3 << 7);
    off |= (op << 3) & (1 << 5);

    off
}

fn c_swsp_off(op: u16) -> u32 {
    let op = op as u32;
    let mut off = 0;

    off |= (op >> 7) & (0xf << 2);
    off |= (op >> 1) & (3 << 6);

    off
}

fn c_lwsp_off(op: u16) -> u32 {
    let op = op as u32;
    let mut off = 0;

    off |= (op >> 7) & (1 << 5);
    off |= (op >> 2) & (7 << 2);
    off |= (op << 4) & (3 << 6);

    off
}

fn c_addi4spn_off(op: u16) -> u32 {
    let op = op as u32;
    let mut off = 0;

    off |= (op >> 7) & (3 << 4);
    off |= (op >> 1) & (0xf << 6);
    off |= (op >> 4) & (1 << 2);
    off |= (op >> 2) & (1 << 3);

    off
}

fn c_shamt(op: u16) -> u32 {
    let s = (op >> 2) & 0x1f;

    u32::from(s | ((op >> 7) & (1 << 5)))
}

fn c_xxx(m: &mut Machine, op: u16) {
    let code = ((op >> 10) & 0x3f) | ((op & 3) << 6);

    panic!("c_xxx {:x} [{:x}] {:?}", op, code, m.cpu);
}

fn c_beqz(m: &mut Machine, op: u16) {
    let v = m.cpu.xget(c_rs1(op));

    if v == 0 {
        let off = c_boff(op);
        let pc = m.cpu.pc.wrapping_sub(2);

        m.cpu.pc = pc.wrapping_add(off);
    }
}

fn c_bnez(m: &mut Machine, op: u16) {
    let v = m.cpu.xget(c_rs1(op));

    if v != 0 {
        let off = c_boff(op);
        let pc = m.cpu.pc.wrapping_sub(2);

        m.cpu.pc = pc.wrapping_add(off);
    }
}

fn c_swsp(m: &mut Machine, op: u16) {
    let v = m.cpu.xget(c_frs2(op));
    let off = c_swsp_off(op);
    let sp = m.cpu.xget(Reg::SP);

    let d = sp.wrapping_add(off);

    if check_sw(m, d) {
        m.store_word(d, v)
    }
}

fn c_lwsp(m: &mut Machine, op: u16) {
    let off = c_lwsp_off(op);
    let sp = m.cpu.xget(Reg::SP);
    let rd = c_frd(op);

    assert_ne!(rd, Reg::ZERO);

    let d = sp.wrapping_add(off);

    if check_lw(m, d) {
        let v = m.load_word(d);
        m.cpu.xset(rd, v);
    }
}

// If RD is 0, this encodes a NOP
fn c_addi(m: &mut Machine, op: u16) {
    let r = c_frd(op);
    let v = m.cpu.xget(r);
    let imm = c_li_imm(op);

    assert_ne!(imm, 0);

    m.cpu.xset(r, v.wrapping_add(imm));
}

fn c_andi(m: &mut Machine, op: u16) {
    let r = c_frd(op);
    let v = m.cpu.xget(r);
    let imm = c_li_imm(op);

    m.cpu.xset(r, v & imm);
}

fn c_li(m: &mut Machine, op: u16) {
    let r = c_frd(op);

    assert_ne!(r, Reg::ZERO);

    m.cpu.xset(r, c_li_imm(op))
}

fn c_lui(m: &mut Machine, op: u16) {
    let r = c_frd(op);

    // If the target is SP, then this is c.addi16sp
    if r == Reg::SP {
        let imm = c_addisp_off(op);
        let v = m.cpu.xget(r);

        m.cpu.xset(r, v.wrapping_add(imm));
    } else {
        let imm = c_li_imm(op) << 12;
        m.cpu.xset(r, imm)
    }
}

fn c_j(m: &mut Machine, op: u16) {
    let off = c_joff(op);
    let pc = m.cpu.pc.wrapping_sub(2);

    m.cpu.pc = pc.wrapping_add(off);
}

fn c_srli(m: &mut Machine, op: u16) {
    let rd = c_rs1(op);
    let s = c_shamt(op);

    let v = m.cpu.xget(rd);
    m.cpu.xset(rd, v.checked_shr(s).unwrap_or(0));
}

fn c_sub(m: &mut Machine, op: u16) {
    let rd = c_rs1(op);
    let rs2 = c_rs2(op);
    let a = m.cpu.xget(rd);
    let b = m.cpu.xget(rs2);

    m.cpu.xset(rd, a.wrapping_sub(b))
}

fn c_slli(m: &mut Machine, op: u16) {
    let rd = c_frd(op);
    let s = c_shamt(op);

    assert_ne!(rd, Reg::ZERO);

    let v = m.cpu.xget(rd);
    m.cpu.xset(rd, v.checked_shl(s).unwrap_or(0));
}

fn c_add_jalr(m: &mut Machine, op: u16) {
    let rs = c_frs2(op);
    let rd = c_frd(op);
    let d = m.cpu.xget(rd);

    if rs == Reg::ZERO {
        // JALR

        // Store return address
        m.cpu.xset(Reg::RA, m.cpu.pc);

        m.cpu.pc = d & !1;
    } else {
        // ADD
        let v = m.cpu.xget(rs);

        m.cpu.xset(rd, v.wrapping_add(d));
    }
}

fn c_mv_jr(m: &mut Machine, op: u16) {
    let rs2 = c_frs2(op);
    let rd = c_frd(op);

    assert_ne!(rd, Reg::ZERO);

    if rs2 == Reg::ZERO {
        // JR
        m.cpu.pc = m.cpu.xget(rd);
    } else {
        // MV
        let v = m.cpu.xget(rs2);
        m.cpu.xset(rd, v);
    }
}

fn c_addi4spn(m: &mut Machine, op: u16) {
    let off = c_addi4spn_off(op);
    let sp = m.cpu.xget(Reg::SP);

    m.cpu.xset(c_rs2(op), sp.wrapping_add(off));
}

fn c_lw(m: &mut Machine, op: u16) {
    let s = m.cpu.xget(c_rs1(op));
    let off = c_sw_lw_off(op);

    let d = s.wrapping_add(off);

    if check_lw(m, d) {
        let v = m.load_word(d);
        m.cpu.xset(c_rs2(op), v);
    }
}

fn c_sw(m: &mut Machine, op: u16) {
    let s = m.cpu.xget(c_rs1(op));
    let v = m.cpu.xget(c_rs2(op));
    let off = c_sw_lw_off(op);

    let d = s.wrapping_add(off);

    if check_sw(m, d) {
        m.store_word(d, v)
    }
}

static INSTRUCTIONS_16: [fn(&mut Machine, op: u16); 256] = [
    c_addi4spn, c_addi4spn, c_addi4spn, c_addi4spn, c_addi4spn, c_addi4spn, c_addi4spn,
    c_addi4spn, // 07
    c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, // 0F
    c_lw, c_lw, c_lw, c_lw, c_lw, c_lw, c_lw, c_lw, // 17
    c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, // 1F
    c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, // 27
    c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, // 2F
    c_sw, c_sw, c_sw, c_sw, c_sw, c_sw, c_sw, c_sw, // 37
    c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, // 3F
    c_addi, c_addi, c_addi, c_addi, c_addi, c_addi, c_addi, c_addi, // 47
    c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, // 4F
    c_li, c_li, c_li, c_li, c_li, c_li, c_li, c_li, // 57
    c_lui, c_lui, c_lui, c_lui, c_lui, c_lui, c_lui, c_lui, // 5F
    c_srli, c_xxx, c_andi, c_sub, c_srli, c_xxx, c_andi, c_xxx, // 67
    c_j, c_j, c_j, c_j, c_j, c_j, c_j, c_j, // 6F
    c_beqz, c_beqz, c_beqz, c_beqz, c_beqz, c_beqz, c_beqz, c_beqz, // 77
    c_bnez, c_bnez, c_bnez, c_bnez, c_bnez, c_bnez, c_bnez, c_bnez, // 7F
    c_slli, c_slli, c_slli, c_slli, c_slli, c_slli, c_slli, c_slli, // 87
    c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, // 8F
    c_lwsp, c_lwsp, c_lwsp, c_lwsp, c_lwsp, c_lwsp, c_lwsp, c_lwsp, // 97
    c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, // 9F
    c_mv_jr, c_mv_jr, c_mv_jr, c_mv_jr, c_add_jalr, c_add_jalr, c_add_jalr, c_add_jalr, // A7
    c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, // AF
    c_swsp, c_swsp, c_swsp, c_swsp, c_swsp, c_swsp, c_swsp, c_swsp, // B7
    c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, // BF
    c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, // C7
    c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, // CF
    c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, // D7
    c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, // DF
    c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, // E7
    c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, // EF
    c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, // F7
    c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, c_xxx, // FF
];

const CSR_MSTATUS: u32 = 0x300;
const CSR_MIE: u32 = 0x304;
const CSR_MTVEC: u32 = 0x305;
const CSR_MIP: u32 = 0x344;

static REGNAMES: [&str; 32] = [
    "z0", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2", "a3", "a4", "a5",
    "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11", "t3", "t4", "t5",
    "t6",
];

#[test]
fn test_rd_rs1_rs2() {
    let add_x8_x11_x27 = 0x01b58433;

    assert_eq!(rd(add_x8_x11_x27), Reg(8));
    assert_eq!(rs1(add_x8_x11_x27), Reg(11));
    assert_eq!(rs2(add_x8_x11_x27), Reg(27));

    let add_x31_x31_x31 = 0x01ff8fb3;

    assert_eq!(rd(add_x31_x31_x31), Reg(31));
    assert_eq!(rs1(add_x31_x31_x31), Reg(31));
    assert_eq!(rs2(add_x31_x31_x31), Reg(31));

    let sltu_a7_a7_a7 = 0x0118b8b3;

    assert_eq!(rd(sltu_a7_a7_a7), Reg(17));
    assert_eq!(rs1(sltu_a7_a7_a7), Reg(17));
    assert_eq!(rs2(sltu_a7_a7_a7), Reg(17));
}

#[test]
fn test_imm_20_se() {
    // addi s0, a1, 0x7ff
    assert_eq!(imm_20_se(0x7ff58413), 0x7ff);
    // ori x31, x31, -0x800
    assert_eq!(imm_20_se(0x800fef93), -2048i32 as u32);
    // andi x31, x31, -1
    assert_eq!(imm_20_se(0xffffff93), -1i32 as u32);
    // xori x31, x31, 0x5d9
    assert_eq!(imm_20_se(0x5d9fcf93), 0x5d9);
    // xori x31, x31, 1
    assert_eq!(imm_20_se(0x001fcf93), 1);
    // xori x31, x31, 1 << 1
    assert_eq!(imm_20_se(0x002fcf93), 1 << 1);
    // xori x31, x31, 1 << 2
    assert_eq!(imm_20_se(0x004fcf93), 1 << 2);
    // xori x31, x31, 1 << 3
    assert_eq!(imm_20_se(0x008fcf93), 1 << 3);
    // xori x31, x31, 1 << 4
    assert_eq!(imm_20_se(0x010fcf93), 1 << 4);
    // xori x31, x31, 1 << 5
    assert_eq!(imm_20_se(0x020fcf93), 1 << 5);
    // xori x31, x31, 1 << 6
    assert_eq!(imm_20_se(0x040fcf93), 1 << 6);
    // xori x31, x31, 1 << 7
    assert_eq!(imm_20_se(0x080fcf93), 1 << 7);
    // xori x31, x31, 1 << 8
    assert_eq!(imm_20_se(0x100fcf93), 1 << 8);
    // xori x31, x31, 1 << 9
    assert_eq!(imm_20_se(0x200fcf93), 1 << 9);
    // xori x31, x31, 1 << 10
    assert_eq!(imm_20_se(0x400fcf93), 1 << 10);
}

#[test]
fn test_imm_20() {
    // csrrw x31, 0, x31
    assert_eq!(imm_20(0x000f9ff3), 0);
    // csrrw x31, 1, x31
    assert_eq!(imm_20(0x001f9ff3), 1);
    // csrrw x31, 0xaaa, x31
    assert_eq!(imm_20(0xaaaf9ff3), 0xaaa);
    // csrrw x31, 0xfff, x31
    assert_eq!(imm_20(0xffff9ff3), 0xfff);
}

#[test]
fn test_store_off() {
    // sw a0, 0x0(sp)
    assert_eq!(store_off(0x00a12023), 0);
    // sw a1, 0x4(sp)
    assert_eq!(store_off(0x00b12223), 4);
    // sw a2, 0x4(sp)
    assert_eq!(store_off(0x00c12423), 8);
    // sw x31, 1(x31)
    assert_eq!(store_off(0x01ffa0a3), 1);
    // sh x31, 1(x31)
    assert_eq!(store_off(0x01ff90a3), 1);
    // sb x31, 1(x31)
    assert_eq!(store_off(0x01ff80a3), 1);
    // sw x0, -1(x0)
    assert_eq!(store_off(0xfe002fa3), -1i32 as u32);
    // sw x0, 0x7ff(x0)
    assert_eq!(store_off(0x7e002fa3), 0x7ff);
    // sw x0, -0x800(x0)
    assert_eq!(store_off(0x80002023), -0x800i32 as u32);
    // sw x0, 0x555(x0)
    assert_eq!(store_off(0x54002aa3), 0x555);
}

#[test]
fn test_boff() {
    // bltu t0, t2, 0
    assert_eq!(boff(0x0072e063), 0);
    // bltu x31, x31, 0
    assert_eq!(boff(0x01ffe063), 0);
    // bltu t0, t2, -2
    assert_eq!(boff(0xfe72efe3) as i32, -2i32);
    // bltu t0, t2, -32
    assert_eq!(boff(0xfe72e0e3) as i32, -32i32);
    // bltu t0, t2, 4
    assert_eq!(boff(0x0072e263), 4);
    // bltu t0, t2, 68
    assert_eq!(boff(0x0472e263), 68);
    // bltu x31, x31, 68
    assert_eq!(boff(0x05ffe263), 68);
    // bltu x0, x0, 0xffe
    assert_eq!(boff(0x7e006fe3), 0xffe);
    // bltu x0, x0, 0xaaa
    assert_eq!(boff(0x2a0065e3), 0xaaa);
    // bltu x31, x31, 0xaaa
    assert_eq!(boff(0x2bffe5e3), 0xaaa);
}

#[test]
fn test_jal() {
    // jal x0, 0
    assert_eq!(jal_off(0x0000006f), 0);
    // jal x0, -2
    assert_eq!(jal_off(0xfffff06f) as i32, -2);
    // jal x0, -64
    assert_eq!(jal_off(0xfc1ff06f) as i32, -64);
    // jal x0, +68
    assert_eq!(jal_off(0x0440006f), 68);
    // jal x0, +392
    assert_eq!(jal_off(0x1880006f), 392);
    // jal x0, 2
    assert_eq!(jal_off(0x0020006f), 2);
    // jal x0, 4
    assert_eq!(jal_off(0x0040006f), 4);
    // jal x0, 8
    assert_eq!(jal_off(0x0080006f), 8);
    // jal x0, 16
    assert_eq!(jal_off(0x0100006f), 16);
    // jal x0, 32
    assert_eq!(jal_off(0x0200006f), 32);
    // jal x0, 64
    assert_eq!(jal_off(0x0400006f), 64);
    // jal x0, 128
    assert_eq!(jal_off(0x0800006f), 128);
    // jal x0, 256
    assert_eq!(jal_off(0x1000006f), 256);
    // jal x0, 512
    assert_eq!(jal_off(0x2000006f), 512);
    // jal x0, 1024
    assert_eq!(jal_off(0x4000006f), 1024);
    // jal x0, 2048
    assert_eq!(jal_off(0x0010006f), 2048);
    // jal x0, 4096
    assert_eq!(jal_off(0x0000106f), 4096);
    // jal x0, 0x4000
    assert_eq!(jal_off(0x0000406f), 0x4000);
    // jal x0, 0x8000
    assert_eq!(jal_off(0x0000806f), 0x8000);
    // jal x0, 0x10000
    assert_eq!(jal_off(0x0001006f), 0x10000);
    // jal x0, 0x20000
    assert_eq!(jal_off(0x0002006f), 0x20000);
    // jal x0, 0x40000
    assert_eq!(jal_off(0x0004006f), 0x40000);
    // jal x0, -0x100000
    assert_eq!(jal_off(0x8000006f) as i32, -0x100000);
}

#[test]
fn test_c_frd() {
    // li a0, 0x1
    assert_eq!(c_frd(0x4505), Reg(10));
    // li x31, 0x1
    assert_eq!(c_frd(0x4f85), Reg(31));
    // li x1, 0x1
    assert_eq!(c_frd(0x4085), Reg(1));
    // lw a5, 0x0(a1)
    assert_eq!(c_rs1(0x419c), Reg(11));
    // sw x8, 0(x15)
    assert_eq!(c_rs2(0xc380), Reg(8));
    assert_eq!(c_rs1(0xc380), Reg(15));
}

#[test]
fn test_c_li_imm() {
    // li x1, 0
    assert_eq!(c_li_imm(0x4081), 0);
    // li x1, 1
    assert_eq!(c_li_imm(0x4085), 1);
    // li x1, -1
    assert_eq!(c_li_imm(0x50fd) as i32, -1);
    // li x1, 0x1f
    assert_eq!(c_li_imm(0x40fd), 0x1f);
    // li x1, -0x20
    assert_eq!(c_li_imm(0x5081) as i32, -0x20);
    // li x1, 0x15
    assert_eq!(c_li_imm(0x40d5), 0x15);
    // c.andi x13, -4
    assert_eq!(c_li_imm(0x9af1) as i32, -4);
}

#[test]
fn test_c_boff() {
    // c.bnez x11, 24
    assert_eq!(c_boff(0xed81), 24);
    // c.bnez x10, 0
    assert_eq!(c_boff(0xe101), 0);
    // c.bnez x15, 0
    assert_eq!(c_boff(0xe381), 0);
    // c.bnez x10, -2
    assert_eq!(c_boff(0xfd7d) as i32, -2);
    // c.bnez x10, -256
    assert_eq!(c_boff(0xf101) as i32, -256);
    // c.bnez x15, -256
    assert_eq!(c_boff(0xf381) as i32, -256);
    // c.bnez x15, -2
    assert_eq!(c_boff(0xf381) as i32, -256);
    // c.bnez x15, -2
    assert_eq!(c_boff(0xfffd) as i32, -2);
    // c.bnez x8, -2
    assert_eq!(c_boff(0xfc7d) as i32, -2);
    // c.bnez x15, 0xaa
    assert_eq!(c_boff(0xe7cd), 0xaa);
    // c.bnez x8, 0xaa
    assert_eq!(c_boff(0xe44d), 0xaa);
    // c.bnez x10, 0
    assert_eq!(c_boff(0xe101), 0);
    // c.bnez x10, 2
    assert_eq!(c_boff(0xe109), 2);
    // c.bnez x10, 4
    assert_eq!(c_boff(0xe111), 4);
    // c.bnez x10, 8
    assert_eq!(c_boff(0xe501), 8);
    // c.bnez x10, 16
    assert_eq!(c_boff(0xe901), 16);
    // c.bnez x10, 32
    assert_eq!(c_boff(0xe105), 32);
    // c.bnez x10, 64
    assert_eq!(c_boff(0xe121), 64);
    // c.bnez x10, 128
    assert_eq!(c_boff(0xe141), 128);
    // c.bnez x10, -256
    assert_eq!(c_boff(0xf101) as i32, -256);
    // c.bnez x10, 254
    assert_eq!(c_boff(0xed7d), 254);
}

#[test]
fn test_c_joff() {
    // c.j 0
    assert_eq!(c_joff(0xa001), 0);
    // c.j 2
    assert_eq!(c_joff(0xa009), 2);
    // c.j 4
    assert_eq!(c_joff(0xa011), 4);
    // c.j 8
    assert_eq!(c_joff(0xa021), 8);
    // c.j 0x10
    assert_eq!(c_joff(0xa801), 0x10);
    // c.j 0x20
    assert_eq!(c_joff(0xa005), 0x20);
    // c.j 0x40
    assert_eq!(c_joff(0xa081), 0x40);
    // c.j 0x80
    assert_eq!(c_joff(0xa041), 0x80);
    // c.j 0x200
    assert_eq!(c_joff(0xa401), 0x200);
    // c.j 0x400
    assert_eq!(c_joff(0xa101), 0x400);
    // c.j -0x800
    assert_eq!(c_joff(0xb001) as i32, -0x800);
    // c.j -2
    assert_eq!(c_joff(0xbffd) as i32, -2);
    // c.j 0x4aa
    assert_eq!(c_joff(0xa16d), 0x4aa);
}

#[test]
fn test_c_addisp_off() {
    // c.addi16sp -64
    assert_eq!(c_addisp_off(0x7139) as i32, -64);
    // c.addi16sp -16
    assert_eq!(c_addisp_off(0x717d) as i32, -16);
    // c.addi16sp 16
    assert_eq!(c_addisp_off(0x6141), 16);
    // c.addi16sp 32
    assert_eq!(c_addisp_off(0x6105), 32);
    // c.addi16sp 64
    assert_eq!(c_addisp_off(0x6121), 64);
    // c.addi16sp 128
    assert_eq!(c_addisp_off(0x6109), 128);
    // c.addi16sp 256
    assert_eq!(c_addisp_off(0x6111), 256);
    // c.addi16sp -512
    assert_eq!(c_addisp_off(0x7101) as i32, -512);
    // c.addi16sp -512
    assert_eq!(c_addisp_off(0x7101) as i32, -512);
    // c.addi16sp 496
    assert_eq!(c_addisp_off(0x617d) as i32, 496);
}

#[test]
fn test_c_swsp_off() {
    // c.swsp x1, 60
    assert_eq!(c_swsp_off(0xde06), 60);
    // c.swsp x0, 0
    assert_eq!(c_swsp_off(0xc002), 0);
    // c.swsp x31, 0
    assert_eq!(c_swsp_off(0xc07e), 0);
    // c.swsp x0, 4
    assert_eq!(c_swsp_off(0xc202), 4);
    // c.swsp x0, 8
    assert_eq!(c_swsp_off(0xc402), 8);
    // c.swsp x0, 16
    assert_eq!(c_swsp_off(0xc802), 16);
    // c.swsp x0, 32
    assert_eq!(c_swsp_off(0xd002), 32);
    // c.swsp x0, 64
    assert_eq!(c_swsp_off(0xc082), 64);
    // c.swsp x0, 128
    assert_eq!(c_swsp_off(0xc102), 128);
    // c.swsp x0, 252
    assert_eq!(c_swsp_off(0xdf82), 252);
}

#[test]
fn test_c_lwsp_off() {
    // c.lwsp x1, 44
    assert_eq!(c_lwsp_off(0x50b2), 44);
    // c.lwsp x4, 0
    assert_eq!(c_lwsp_off(0x4202), 0);
    // c.lwsp x31, 0
    assert_eq!(c_lwsp_off(0x4f82), 0);
    // c.lwsp x4, 4
    assert_eq!(c_lwsp_off(0x4212), 4);
    // c.lwsp x4, 8
    assert_eq!(c_lwsp_off(0x4222), 8);
    // c.lwsp x4, 16
    assert_eq!(c_lwsp_off(0x4242), 16);
    // c.lwsp x4, 32
    assert_eq!(c_lwsp_off(0x5202), 32);
    // c.lwsp x4, 64
    assert_eq!(c_lwsp_off(0x4206), 64);
    // c.lwsp x4, 128
    assert_eq!(c_lwsp_off(0x420a), 128);
    // c.lwsp x4, 252
    assert_eq!(c_lwsp_off(0x527e), 252);
}

#[test]
fn test_c_sw_lw_off() {
    // c.lw x12, 16(x11)
    assert_eq!(c_sw_lw_off(0x4990), 16);
    // c.lw x8, 16(x8)
    assert_eq!(c_sw_lw_off(0x4800), 16);
    // c.lw x8, 0(x8)
    assert_eq!(c_sw_lw_off(0x4000), 0);
    // c.lw x15, 0(x15)
    assert_eq!(c_sw_lw_off(0x439c), 0);
    // c.lw x8, 4(x8)
    assert_eq!(c_sw_lw_off(0x4040), 4);
    // c.lw x8, 16(x8)
    assert_eq!(c_sw_lw_off(0x4800), 16);
    // c.lw x8, 32(x8)
    assert_eq!(c_sw_lw_off(0x5000), 32);
    // c.lw x8, 64(x8)
    assert_eq!(c_sw_lw_off(0x4020), 64);
    // c.lw x8, 124(x8)
    assert_eq!(c_sw_lw_off(0x5c60), 124);
}

#[test]
fn test_c_addi4spn_off() {
    // c.addi4spn x11, 4
    assert_eq!(c_addi4spn_off(0x004c), 4);
    // c.addi4spn x8, 4
    assert_eq!(c_addi4spn_off(0x0040), 4);
    // c.addi4spn x15, 4
    assert_eq!(c_addi4spn_off(0x005c), 4);
    // c.addi4spn x8, 8
    assert_eq!(c_addi4spn_off(0x0020), 8);
    // c.addi4spn x8, 16
    assert_eq!(c_addi4spn_off(0x0800), 16);
    // c.addi4spn x8, 32
    assert_eq!(c_addi4spn_off(0x1000), 32);
    // c.addi4spn x8, 64
    assert_eq!(c_addi4spn_off(0x0080), 64);
    // c.addi4spn x8, 128
    assert_eq!(c_addi4spn_off(0x0100), 128);
    // c.addi4spn x8, 256
    assert_eq!(c_addi4spn_off(0x0200), 256);
    // c.addi4spn x8, 512
    assert_eq!(c_addi4spn_off(0x0400), 512);
    // c.addi4spn x8, 1020
    assert_eq!(c_addi4spn_off(0x1fe0), 1020);
}

#[test]
fn test_c_shamt() {
    // c.slli x10, 3
    assert_eq!(c_shamt(0x050e), 3);
    // c.slli x4, 1
    assert_eq!(c_shamt(0x0206), 1);
    // c.slli x4, 2
    assert_eq!(c_shamt(0x020a), 2);
    // c.slli x4, 4
    assert_eq!(c_shamt(0x0212), 4);
    // c.slli x4, 8
    assert_eq!(c_shamt(0x0222), 8);
    // c.slli x4, 16
    assert_eq!(c_shamt(0x0242), 16);
    // c.slli x4, 32
    assert_eq!(c_shamt(0x1202), 32);
    // c.slli x4, 63
    assert_eq!(c_shamt(0x127e), 63);
}
