//! Implementation of the RISC-rfdV RV32IMAC ISA

mod decoder;

use crate::{CycleCounter, NoRa32, RAM, ROM, sync};
use decoder::{Decoder, Instruction};
use std::fmt;

pub struct Cpu {
    /// Instruction decoder
    decoder: Decoder,
    /// Program Counter
    pc: u32,
    /// 32 general purpose registers (x0 must always be 0). The additional register at the end is
    /// used as a target for writes to x0
    x: [u32; 33],
    /// Machine mode
    mode: Mode,
    /// Machine status
    mstatus: u32,
    /// Machine Interrupt Enable
    mie: u32,
    /// Machine Interrupt Pending
    mip: u32,
    /// Machine Cause register
    mcause: u32,
    /// Machine Trap Vector base address
    mtvec: u32,
    /// Matchine scratch register
    mscratch: u32,
    /// Machine Exception Program Counter
    mepc: u32,
    /// Contains the address of the last "Load-Reserved" instruction as long as it remains valid
    reservation: Option<u32>,
    /// Set to true if the CPU is halted until an IRQ occurs
    wfi: bool,
    /// Instruction cache timing emulation. No caching is actually implemented here, it's just to
    /// get more realistic timings.
    icache: ICache,
}

impl Cpu {
    pub fn new() -> Cpu {
        Cpu {
            decoder: Decoder::new(),
            // The first 0x100 bytes of the ROM are reserved for metadata/header
            pc: ROM.base + 0x100,
            x: [0; 33],
            mode: Mode::Machine,
            mstatus: 0,
            mscratch: 0,
            mie: 0,
            mip: 0,
            mcause: 0,
            mtvec: 0,
            mepc: 0,
            reservation: None,
            wfi: false,
            icache: ICache::new(),
        }
    }

    /// True if the CPU is currently stopped and waiting for an interrupt
    pub fn wfi(&self) -> bool {
        self.wfi
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

    /// Get the value of Machine Interrupt Enable in mstatus
    fn mstatus_mie(&self) -> bool {
        self.mstatus & (1 << 3) != 0
    }

    /// Set the value of Machine Interrupt Enable in mstatus
    fn mstatus_mie_set(&mut self, set: bool) {
        self.mstatus &= !(1 << 3);
        self.mstatus |= u32::from(set) << 3;
    }

    /// Get the value of Machine Previous Interrupt Enable in mstatus
    fn mstatus_mpie(&self) -> bool {
        self.mstatus & (1 << 7) != 0
    }

    /// Set the value of Machine Previous Interrupt Enable in mstatus
    fn mstatus_mpie_set(&mut self, set: bool) {
        self.mie &= !(1 << 7);
        self.mie |= u32::from(set) << 7;
    }

    /// Get the previous privilege mode in mstatus
    fn mstatus_mpp(&self) -> Mode {
        match (self.mstatus >> 11) & 3 {
            0 => Mode::User,
            3 => Mode::Machine,
            mpp => panic!("Unexpected MPP value: {mpp}"),
        }
    }

    /// Set the previous privilege mode in mstatus
    fn mstatus_mpp_set(&mut self, mode: Mode) {
        self.mstatus &= !(3 << 11);
        self.mstatus |= (mode as u32) << 11;
    }

    /// Get the value of Timeout Wait in mstatus
    fn mstatus_tw(&self) -> bool {
        self.mstatus & (1 << 21) != 0
    }

    /// Get the value of Machine Timer Interrupt Pending in mip
    fn mip_mtip(&self) -> bool {
        self.mip & (1 << 7) != 0
    }

    /// Set the value of Machine Timer Interrupt Pending in mip
    fn mip_mtip_set(&mut self, set: bool) {
        self.mip &= !(1 << 7);
        self.mip |= u32::from(set) << 7;
    }

    /// Get the value of Machine External Interrupt Pending in mip
    fn mip_meip(&self) -> bool {
        self.mip & (1 << 11) != 0
    }

    /// Set the value of Machine External Interrupt Pending in mip
    fn mip_meip_set(&mut self, set: bool) {
        self.mip &= !(1 << 11);
        self.mip |= u32::from(set) << 11;
    }

    /// Set a new value for the given Control and Status Register, returning the previous value
    #[cold]
    fn csr_and_or(&mut self, csr: u16, and_mask: u32, or_mask: u32) -> u32 {
        let mode_min = (csr >> 8) & 3;
        let read_only = ((csr >> 10) & 3) == 0b11;

        if mode_min > self.mode as u16 {
            panic!(
                "Attempt to access CSR {:x} in {:?} mode @ {:x}",
                csr, self.mode, self.pc
            );
        }

        if read_only && (and_mask != !0 || or_mask != 0) {
            panic!("Attempt to write read-only CSR {csr:x}");
        }

        // debug!("CSR SET *{:x} & {:x} | {:x}", csr, and_mask, or_mask);

        let update_csr = |reg: &mut u32| -> u32 {
            let prev = *reg;

            *reg &= and_mask;
            *reg |= or_mask;

            prev
        };

        match csr {
            CSR_MSTATUS => {
                let prev = update_csr(&mut self.mstatus);

                // We only support U and M modes
                let mpp = (self.mstatus >> 11) & 3;

                if mpp != 0 && mpp != 3 {
                    // Default to machine mode. I think that's fine per the spec:
                    //
                    // "M-mode software can determine whether a privilege mode is implemented by
                    // writing that mode to MPP then reading it back."
                    self.mstatus |= 3 << 11;
                }

                prev
            }
            CSR_MIE => {
                let prev = update_csr(&mut self.mie);

                // We only support timer and external (device) machine-mode interrupts
                self.mie &= (1 << 7) | (1 << 11);

                prev
            }
            CSR_MTVEC => update_csr(&mut self.mtvec),
            CSR_MSCRATCH => update_csr(&mut self.mscratch),
            CSR_MEPC => update_csr(&mut self.mepc),
            CSR_MCAUSE => update_csr(&mut self.mcause),
            CSR_MIP => {
                // Since we only have timer and external interrupts available, we can't actually
                // ack anything here:
                //
                // - the timer IRQ is ack'd by setting mtimecmp into the future
                // - the external IRQ are ack'd on the external controller
                self.mip
            }
            _ => panic!("Unhandled CSR {csr:x} {self:?}"),
        }
    }

    fn csr_set(&mut self, csr: u16, v: u32) -> u32 {
        self.csr_and_or(csr, 0, v)
    }

    pub fn ram_write(&mut self, addr: u32) {
        // Make sure to invalidate the reservation if it hits the same memory cell
        if let Some(r_addr) = self.reservation {
            if r_addr >> 4 == addr >> 4 {
                self.reservation = None;
            }
        }
    }
}

impl fmt::Debug for Cpu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f)?;
        writeln!(
            f,
            "pc : {:08x}   MODE: {:?}   MIE: {}",
            self.pc,
            self.mode,
            self.mstatus_mie()
        )?;
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

fn check_for_irq(m: &mut NoRa32) {
    let irq = m.cpu.mie & m.cpu.mip;
    if irq == 0 {
        return;
    }

    // Even if MIE is set we have to wake up in case of WFI
    m.cpu.wfi = false;

    if !m.cpu.mstatus_mie() {
        return;
    }

    // "Multiple simultaneous interrupts destined for M-mode are handled in the following
    // decreasing priority order: MEI, MSI, MTI, SEI, SSI, STI, LCOFI."
    let cause = if (irq & (1 << 11)) != 0 {
        cause::MACHINE_EXTERNAL_IRQ
    } else if (irq & (1 << 7)) != 0 {
        cause::MACHINE_TIMER_IRQ
    } else {
        unreachable!("Unexpected IRQ {:x}", irq)
    };

    trigger_trap(m, cause, 0);
}

#[cold]
fn trigger_trap(m: &mut NoRa32, cause: u32, _mtval: u32) {
    m.cpu.mcause = cause;
    m.cpu.mepc = m.cpu.pc;
    m.cpu.mstatus_mpp_set(m.cpu.mode);
    m.cpu.mode = Mode::Machine;

    let mie = m.cpu.mstatus_mie();
    m.cpu.mstatus_mpie_set(mie);
    m.cpu.mstatus_mie_set(false);

    let handler_base = m.cpu.mtvec & (!3);
    let vec_mode = (m.cpu.mtvec & 1) != 0;

    let is_irq = (cause & (1 << 31)) != 0;

    let handler = if is_irq && vec_mode {
        // Vectored IRQ: address is base + IRQ * 4
        handler_base + (cause & !(1 << 31)) * 4
    } else {
        handler_base
    };

    m.cpu.pc = handler;
}

pub mod cause {
    pub const MACHINE_TIMER_IRQ: u32 = (1 << 31) | 7;
    pub const MACHINE_EXTERNAL_IRQ: u32 = (1 << 31) | 11;

    pub const ECALL_FROM_M_MODE: u32 = 11;
    pub const ECALL_FROM_U_MODE: u32 = 8;
}

pub fn set_mtip(m: &mut NoRa32, mtip: bool) {
    if mtip == m.cpu.mip_mtip() {
        return;
    }

    m.cpu.mip_mtip_set(mtip);

    check_for_irq(m);
}

pub fn set_meip(m: &mut NoRa32, meip: bool) {
    if meip == m.cpu.mip_meip() {
        return;
    }

    m.cpu.mip_meip_set(meip);

    check_for_irq(m);
}

pub fn step(m: &mut NoRa32) {
    debug_assert!(!m.cpu.wfi);

    let pc = m.cpu.pc;

    let nticks = match m.cpu.icache.fetch(pc) {
        ICacheFetchResult::Hit => 1,
        ICacheFetchResult::Miss => {
            // Cache miss timings
            if RAM.contains(pc).is_some() {
                // Fetch from RAM
                4
            } else {
                // Fetch from cartridge
                60
            }
        }
    };

    m.tick(nticks);

    let (inst, npc) = decoder::fetch_instruction(m, pc);

    // info!("{:?} {:?}", inst, m.cpu);

    m.cpu.pc = npc;

    match inst {
        Instruction::InvalidAddress(add) => panic!("Can't fetch instruction at {add:x}"),
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
        Instruction::Slt { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1) as i32;
            let b = m.cpu.xget(rs2) as i32;

            m.cpu.xset(rd, if a < b { 1 } else { 0 });
        }
        Instruction::Sltu { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            m.cpu.xset(rd, if a < b { 1 } else { 0 });
        }
        Instruction::Xor { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            m.cpu.xset(rd, a ^ b);
        }
        Instruction::Or { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            m.cpu.xset(rd, a | b);
        }
        Instruction::Sub { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            m.cpu.xset(rd, a.wrapping_sub(b));
        }
        Instruction::And { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            m.cpu.xset(rd, a & b);
        }
        Instruction::Mul { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            // Add a slight penalty for multiplications
            m.tick(1);

            m.cpu.xset(rd, a.wrapping_mul(b));
        }
        Instruction::Mulhu { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            let p = u64::from(a) * u64::from(b);

            // Add a slight penalty for multiplications
            m.tick(1);

            m.cpu.xset(rd, (p >> 32) as u32);
        }
        Instruction::Mulh { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1) as i32;
            let b = m.cpu.xget(rs2) as i32;

            let p = i64::from(a) * i64::from(b);

            // Add a slight penalty for multiplications
            m.tick(1);

            m.cpu.xset(rd, (p >> 32) as u32);
        }
        Instruction::Div { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1) as i32;
            let b = m.cpu.xget(rs2) as i32;

            let d = match (a, b) {
                // Division by 0
                (_, 0) => !0,
                // i32::MIN / -1 (signed overflow)
                (i32::MIN, -1) => i32::MIN,
                _ => a / b,
            };

            // Having divisions take one cycle feels wrong to me, so I'm using a weird heuristic to
            // penalize them here.
            //
            // I tried looking for a more realistic model online but it seems difficult to find any
            // sound default, especially since we don't implement any pipelining or out-of-order
            // execution in this CPU model. For instance the PlayStation CPU's division takes
            // dozens of cycles but it can execute in parallel with other instructions as long as
            // there's no register dependency.
            let hamming_res = d.min(b).unsigned_abs().count_ones();

            m.tick(hamming_res as CycleCounter);

            m.cpu.xset(rd, d as u32);
        }
        Instruction::Divu { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            let d = if b == 0 { !0 } else { a / b };

            // See Div
            let hamming_res = d.min(b).count_ones();

            m.tick(hamming_res as CycleCounter);

            m.cpu.xset(rd, d);
        }
        Instruction::Remu { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            let d = if b == 0 { a } else { a % b };

            // See Div
            let hamming_res = d.min(b).count_ones();

            m.tick(hamming_res as CycleCounter);

            m.cpu.xset(rd, d);
        }
        Instruction::AddImm { rd, rs1, imm } => {
            let v = m.cpu.xget(rs1);

            m.cpu.xset(rd, v.wrapping_add(imm.extend()));
        }
        Instruction::Slti { rd, rs1, imm } => {
            let v = m.cpu.xget(rs1) as i32;
            let i = imm.extend() as i32;

            m.cpu.xset(rd, if v < i { 1 } else { 0 });
        }
        Instruction::Sltiu { rd, rs1, imm } => {
            let v = m.cpu.xget(rs1);
            let i = imm.extend();

            m.cpu.xset(rd, if v < i { 1 } else { 0 });
        }
        Instruction::XorImm { rd, rs1, imm } => {
            let v = m.cpu.xget(rs1);

            m.cpu.xset(rd, v ^ imm.extend());
        }
        Instruction::OrImm { rd, rs1, imm } => {
            let v = m.cpu.xget(rs1);

            m.cpu.xset(rd, v | imm.extend());
        }
        Instruction::AndImm { rd, rs1, imm } => {
            let v = m.cpu.xget(rs1);

            m.cpu.xset(rd, v & imm.extend());
        }
        Instruction::SraImm { rd, rs1, shamt } => {
            let v = m.cpu.xget(rs1) as i32;

            m.cpu.xset(
                rd,
                v.checked_shr(u32::from(shamt)).unwrap_or(v >> 31) as u32,
            );
        }
        Instruction::SrlImm { rd, rs1, shamt } => {
            let v = m.cpu.xget(rs1);

            m.cpu.xset(rd, v.checked_shr(u32::from(shamt)).unwrap_or(0));
        }
        Instruction::Sll { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2) & 0x1f;

            m.cpu.xset(rd, a << b)
        }
        Instruction::Srl { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2) & 0x1f;

            m.cpu.xset(rd, a >> b)
        }
        Instruction::Sra { rd, rs1, rs2 } => {
            let a = m.cpu.xget(rs1) as i32;
            let b = m.cpu.xget(rs2) & 0x1f;

            m.cpu.xset(rd, (a >> b) as u32)
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
                m.cpu.pc = tpc
            }
        }
        Instruction::Bne { rs1, rs2, tpc } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            if a != b {
                m.cpu.pc = tpc
            }
        }
        Instruction::Blt { rs1, rs2, tpc } => {
            let a = m.cpu.xget(rs1) as i32;
            let b = m.cpu.xget(rs2) as i32;

            if a < b {
                m.cpu.pc = tpc
            }
        }
        Instruction::Bge { rs1, rs2, tpc } => {
            let a = m.cpu.xget(rs1) as i32;
            let b = m.cpu.xget(rs2) as i32;

            if a >= b {
                m.cpu.pc = tpc
            }
        }
        Instruction::Bltu { rs1, rs2, tpc } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            if a < b {
                m.cpu.pc = tpc
            }
        }
        Instruction::Bgeu { rs1, rs2, tpc } => {
            let a = m.cpu.xget(rs1);
            let b = m.cpu.xget(rs2);

            if a >= b {
                m.cpu.pc = tpc
            }
        }
        Instruction::Lb { rd, rs1, off } => {
            let base = m.cpu.xget(rs1);
            let addr = base.wrapping_add(off.extend());

            let v = m.load_byte(addr);
            m.cpu.xset(rd, v as i8 as u32)
        }
        Instruction::Lbu { rd, rs1, off } => {
            let base = m.cpu.xget(rs1);
            let addr = base.wrapping_add(off.extend());

            let v = m.load_byte(addr);
            m.cpu.xset(rd, v as u32)
        }
        Instruction::Lh { rd, rs1, off } => {
            let base = m.cpu.xget(rs1);
            let addr = base.wrapping_add(off.extend());

            if addr & 1 == 0 {
                let v = m.load_halfword(addr);
                m.cpu.xset(rd, v as i16 as u32)
            } else {
                panic!("Misaligned LH {:x} {:?}", addr, m.cpu);
            }
        }
        Instruction::Lhu { rd, rs1, off } => {
            let base = m.cpu.xget(rs1);
            let addr = base.wrapping_add(off.extend());

            if addr & 1 == 0 {
                let v = m.load_halfword(addr);
                m.cpu.xset(rd, v as u32)
            } else {
                panic!("Misaligned LHU {:x} {:?}", addr, m.cpu);
            }
        }
        Instruction::Lw { rd, rs1, off } => {
            let base = m.cpu.xget(rs1);
            let addr = base.wrapping_add(off.extend());

            if addr & 3 == 0 {
                let v = m.load_word(addr);
                m.cpu.xset(rd, v)
            } else {
                panic!("Misaligned LW {:x} {:?}", addr, m.cpu);
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

                        m.cpu.xset(rd, v);
                        m.tick(1);
                    }
                    None => panic!("LR.W not targeting RAM! {:x} {:?}", addr, m.cpu),
                }
            } else {
                panic!("Misaligned store {:x} {:?}", addr, m.cpu);
            }
        }
        Instruction::Scw { rd, rs1, rs2 } => {
            // Invalidate any previous reservation:
            //
            // "Regardless of success or failure, executing an SC.W instruction invalidates any
            // reservation held by this hart."
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
        Instruction::AmoorW { rd, rs1, rs2 } => {
            let addr = m.cpu.xget(rs1);
            let or = m.cpu.xget(rs2);

            if addr & 3 == 0 {
                let v = m.load_word(addr);
                m.cpu.xset(rd, v);
                m.store_word(addr, v | or);
            } else {
                panic!("Misaligned load {:x} {:?}", addr, m.cpu);
            }
        }
        Instruction::AmoaddW { rd, rs1, rs2 } => {
            let addr = m.cpu.xget(rs1);
            let inc = m.cpu.xget(rs2);

            if addr & 3 == 0 {
                let v = m.load_word(addr);
                m.cpu.xset(rd, v);
                m.store_word(addr, v.wrapping_add(inc));
            } else {
                panic!("Misaligned load {:x} {:?}", addr, m.cpu);
            }
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

            check_for_irq(m);
        }
        Instruction::CsrSetBits { rd, csr, rs1 } => {
            let v = m.cpu.xget(rs1);

            let prev = m.cpu.csr_and_or(csr, !0, v);

            m.cpu.xset(rd, prev);
            check_for_irq(m);
        }
        Instruction::CsrManipImm {
            rd,
            csr,
            and_mask,
            or_mask,
        } => {
            let prev = m.cpu.csr_and_or(csr, and_mask.extend(), or_mask.extend());

            m.cpu.xset(rd, prev);
            check_for_irq(m);
        }
        Instruction::MRet => {
            // Update mode
            m.cpu.mode = m.cpu.mstatus_mpp();
            m.cpu.mstatus_mpp_set(Mode::User);

            // Update MIE/MPIE
            let mpie = m.cpu.mstatus_mpie();
            m.cpu.mstatus_mie_set(mpie);
            m.cpu.mstatus_mpie_set(true);

            m.cpu.pc = m.cpu.mepc;

            check_for_irq(m);
        }
        Instruction::Wfi => {
            if m.cpu.mode != Mode::Machine && m.cpu.mstatus_tw() {
                // We're not in machine-mode and WFI has been called while Timeout Wait is set.
                //
                // This means that if the WFI "does not complete within an implementation-specific,
                // bounded time limit, the WFI instruction causes an illegal-instruction
                // exception".
                //
                // Instead of implementing an actual timeout, we immediately raise the exception
                // (i.e. timeout = 0) which is explicitly allowed by the spec:
                //
                // "An implementation may have WFI always raise an illegal-instruction exception in
                // less-privileged modes when TW=1, even if there are pending globally-disabled
                // interrupts when the instruction is executed."
                panic!("WFI with TW=1");
            } else {
                m.cpu.wfi = true;
                sync::fast_forward_to_next_event(m);
            }
        }
        Instruction::Ecall => {
            let cause = match m.cpu.mode {
                Mode::Machine => cause::ECALL_FROM_M_MODE,
                Mode::User => cause::ECALL_FROM_U_MODE,
            };

            // MEPC takes the current instruction address, not the next
            m.cpu.pc = pc;
            trigger_trap(m, cause, 0);
        }
        Instruction::FenceI => {
            // Instruction fence. A very expensive instruction for us since it clears the decoder,
            // so penalize it heavily to disincentivize overuse
            m.tick(10_000);
            m.cpu.icache.invalidate();
            m.cpu.decoder.invalidate();
        }
        Instruction::Unknown32(op) => {
            panic!("Encountered unknown instruction {:x} {:?}", op, m.cpu)
        }
        Instruction::Unknown16(op) => panic!(
            "Encountered unknown compressed instruction {:x} {:?}",
            op, m.cpu
        ),
        Instruction::Invalid16(op) => {
            panic!("Encountered invalid instruction {:x} {:?}", op, m.cpu)
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Mode {
    User = 0,
    Machine = 3,
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

const CSR_MSTATUS: u16 = 0x300;
const CSR_MIE: u16 = 0x304;
const CSR_MTVEC: u16 = 0x305;
const CSR_MSCRATCH: u16 = 0x340;
const CSR_MEPC: u16 = 0x341;
const CSR_MCAUSE: u16 = 0x342;
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

/// Dummy instruction cache implementation that's only used for timing emulation.
///
/// Doing proper cacheline-level emulation would add some complexity due to the interplay with the
/// RISC decoder and it's probably not useful to worry about that at this point.
struct ICache {
    /// 256 4-word cachelines. Since we don't actually emulate the caching, we just need to keep
    /// track of the addresses to decide if it's a cache hit or miss.
    tags: [u32; 0x100],
}

impl ICache {
    fn new() -> ICache {
        ICache { tags: [!0; 0x100] }
    }

    fn fetch(&mut self, addr: u32) -> ICacheFetchResult {
        let tag = addr >> 4;

        let bucket = (tag & 0xff) as usize;

        if self.tags[bucket] == tag {
            ICacheFetchResult::Hit
        } else {
            self.tags[bucket] = tag;
            ICacheFetchResult::Miss
        }
    }

    fn invalidate(&mut self) {
        self.tags = [!0; 0x100];
    }
}

#[derive(PartialEq, Eq, Copy, Clone)]
enum ICacheFetchResult {
    Hit,
    Miss,
}
