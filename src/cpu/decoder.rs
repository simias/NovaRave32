//! RISC-V instruction decoding and caching

use super::{Extendable, Reg};
use crate::NoRa32;
use crate::simple_rand::SimpleRand;
use nr32_common::memmap::{RAM, ROM};

/// Number of bytes per instruction page as a power of two
const PAGE_LEN_SHIFT: usize = 12;

/// Number of bytes per instruction page
const PAGE_LEN_BYTES: usize = 1 << PAGE_LEN_SHIFT;

/// Number of instructions per page. Since instructions are 16-bit aligned with "C", we have to
/// assume that we can have an instruction every other byte.
const PAGE_LEN_OP: usize = PAGE_LEN_BYTES >> 1;

/// Max number of decoded pages before we start recycling
const PAGE_CACHE_MAX: usize = 64;

/// Total number of possible pages on the system. Since we can only run code from ROM or RAM, we
/// don't have to look further.
const PAGE_TOTAL: usize = ((ROM.len + RAM.len) >> PAGE_LEN_SHIFT) as usize;

pub struct Decoder {
    pages: Vec<Page>,
    page_lut: Vec<Option<u16>>,
    /// Last used page base and its corresponding index
    last_used_page: (u32, u16),
    rand: SimpleRand,
}

impl Decoder {
    pub fn new() -> Decoder {
        info!("PAGE_LEN:       {}B ({}inst)", PAGE_LEN_BYTES, PAGE_LEN_OP);
        info!(
            "PAGE_CACHE_MAX: {}p ({}KiB)",
            PAGE_CACHE_MAX,
            (PAGE_CACHE_MAX * PAGE_LEN_BYTES) / 1024
        );
        info!("SYSTEM_PAGES:   {}p", PAGE_TOTAL);
        info!("INST_SIZE:      {}B", std::mem::size_of::<Instruction>());

        Decoder {
            pages: Vec::new(),
            page_lut: vec![None; PAGE_TOTAL],
            last_used_page: (!0, 0),
            rand: SimpleRand::new(),
        }
    }

    pub fn invalidate(&mut self) {
        info!("Flushing {} decoded pages", self.pages.len());
        self.pages.clear();
        self.page_lut.fill(None);
        self.last_used_page = (!0, 0);
    }

    pub fn expire_pages(&mut self) {
        for p in self.pages.iter_mut() {
            p.hit_score >>= 1;
        }
    }

    /// Eject the least-used page and return its index for reuse
    pub fn evict_page(&mut self) -> usize {
        // Recycle the page with the lowest hit score
        let ip = self
            .pages
            .iter()
            .enumerate()
            .min_by_key(|(_, p)| {
                // Add a small pseudorandom bias to the score to randomize which page gets evicted
                // if they have the same score. This could avoid some pathological cases.
                p.hit_score + (self.rand.next() & 0x1f)
            })
            .map(|(index, _)| index)
            .unwrap();

        info!(
            "Evict page {} [score: {}] {:x}",
            ip, self.pages[ip].hit_score, self.pages[ip].base
        );

        let old_idx = lut_idx(self.pages[ip].base << PAGE_LEN_SHIFT);
        self.page_lut[old_idx] = None;
        self.last_used_page = (!0, 0);

        ip
    }
}

/// Retrieves the page_lut index for the given address.
///
/// Panics if the address is not in ROM or RAM
fn lut_idx(addr: u32) -> usize {
    if let Some(off) = RAM.contains(addr) {
        return (off >> PAGE_LEN_SHIFT) as usize;
    }

    if let Some(off) = ROM.contains(addr) {
        return ((off + RAM.len) >> PAGE_LEN_SHIFT) as usize;
    }

    panic!("Invalid PC {addr:x}");
}

/// Returns the base address of a page from its lut_idx
fn lut_idx_to_base(lidx: usize) -> u32 {
    debug_assert!(lidx < PAGE_TOTAL);

    let lidx = lidx as u32;

    let off = lidx << PAGE_LEN_SHIFT;

    let base = if (lidx as usize) >= lut_idx(ROM.base) {
        off - RAM.len + ROM.base
    } else {
        off + RAM.base
    };

    debug_assert_eq!(lut_idx(base), lidx as usize);

    base
}

pub fn fetch_instruction(m: &mut NoRa32, pc: u32) -> (Instruction, u32) {
    if pc & 1 != 0 {
        return (Instruction::InvalidAddress(pc), pc);
    }

    let pc_base = pc >> PAGE_LEN_SHIFT;
    let ipos = (pc >> 1) & ((PAGE_LEN_OP as u32) - 1);

    let (lu_page, lu_idx) = m.cpu.decoder.last_used_page;

    if pc_base == lu_page {
        // We're still on the same page
        let page = &mut m.cpu.decoder.pages[lu_idx as usize];

        debug_assert_eq!(page.base, pc_base);

        return page.instructions[ipos as usize];
    }

    let lut_idx = lut_idx(pc);

    let page_idx = match m.cpu.decoder.page_lut[lut_idx] {
        Some(pi) => usize::from(pi),
        None => decode_page(m, lut_idx),
    };

    let page = &mut m.cpu.decoder.pages[page_idx];
    // Should never overflow in practice
    page.hit_score += PAGE_LEN_OP as u32;

    if page.base != pc_base {
        // That means that `pc` isn't targeting an area we support
        (Instruction::InvalidAddress(pc), pc)
    } else {
        m.cpu.decoder.last_used_page = (pc_base, page_idx as u16);
        page.instructions[ipos as usize]
    }
}

// Decode page at `lut_idx`, store it in the cache and returns its page_idx
fn decode_page(m: &mut NoRa32, lidx: usize) -> usize {
    let base = lut_idx_to_base(lidx);

    let (mem, mem_off) = {
        if let Some(off) = ROM.contains(base) {
            (&*m.rom as &[u32], off >> 2)
        } else if let Some(off) = RAM.contains(base) {
            (&*m.ram as &[u32], off >> 2)
        } else {
            panic!("Can't decode page at {base:x}");
        }
    };

    let mut instructions = [(Instruction::Unknown16(0), 0); PAGE_LEN_OP];

    for (op_off, (inst, npc)) in instructions.iter_mut().enumerate() {
        // We decode one instruction every other byte
        let woff = (mem_off as usize) + (op_off >> 1);

        let w = mem.get(woff).cloned().unwrap_or(!0);

        let pc = base + ((op_off as u32) << 1);

        let op = if op_off & 1 == 0 {
            // 32bit-aligned
            w
        } else {
            let ilo = w >> 16;

            // Warning: this will fetch one-past the end of the page for the last instruction. This
            // is by design, but we need to make sure that we handle it correctly when we reach the
            // end of `mem`
            let ihi = mem.get(woff + 1).unwrap_or(&!0);
            ihi << 16 | ilo
        };

        if op & 3 == 3 {
            // 32bit instruction

            let unkn = Instruction::Unknown32(op);

            *npc = pc.wrapping_add(4);

            let funct3 = (op >> 12) & 7;
            let funct5 = op >> 27;
            let funct7 = op >> 25;

            // Ignore the two LSB since they're always 0b11 here.
            *inst = match (op >> 2) & 0x1f {
                0b0_0000 => {
                    let rd = r_7(op).out();
                    let rs1 = r_15(op);
                    let off = imm_20_se(op);

                    match funct3 {
                        0b000 => Instruction::Lb { rd, rs1, off },
                        // LH
                        0b001 => Instruction::Lh { rd, rs1, off },
                        // LW
                        0b010 => Instruction::Lw { rd, rs1, off },
                        // LBU
                        0b100 => Instruction::Lbu { rd, rs1, off },
                        // LHU
                        0b101 => Instruction::Lhu { rd, rs1, off },
                        _ => unkn,
                    }
                }
                0b0_0011 => {
                    match funct3 {
                        // FENCE
                        0b000 => Instruction::nop(),
                        // FENCE.I
                        0b001 => Instruction::FenceI,
                        _ => unkn,
                    }
                }
                0b0_0100 => {
                    let imm = imm_20_se(op);
                    let rs1 = r_15(op);
                    let rd = r_7(op).out();

                    match funct3 {
                        // ADDI
                        0b000 => {
                            if rs1 == Reg::ZERO {
                                // ADDI rd, r0, imm is used to implement LI rd, imm
                                Instruction::Li {
                                    rd,
                                    imm: imm.extend(),
                                }
                            } else if imm == 0 {
                                // ADDI rd, rs1, 0 is used to implement MV rd, rs1
                                Instruction::Move { rd, rs1 }
                            } else {
                                Instruction::AddImm { rd, rs1, imm }
                            }
                        }
                        0b001 => match funct7 {
                            // SLLI
                            0b0000000 => Instruction::SllImm {
                                rd,
                                rs1,
                                shamt: shamt(op),
                            },
                            _ => unkn,
                        },
                        // SLTI
                        0b010 => Instruction::Slti { rd, rs1, imm },
                        // SLTIU
                        0b011 => Instruction::Sltiu { rd, rs1, imm },
                        // XORI
                        0b100 => Instruction::XorImm { rd, rs1, imm },
                        0b101 => match funct7 {
                            // SRLI
                            0b0000000 => Instruction::SrlImm {
                                rd,
                                rs1,
                                shamt: shamt(op),
                            },
                            // SRAI
                            0b0100000 => Instruction::SraImm {
                                rd,
                                rs1,
                                shamt: shamt(op),
                            },
                            _ => unkn,
                        },
                        // ANDI
                        0b110 => Instruction::OrImm { rd, rs1, imm },
                        // ANDI
                        0b111 => Instruction::AndImm { rd, rs1, imm },
                        _ => unkn,
                    }
                }
                // AUIPC
                0b0_0101 => Instruction::Li {
                    rd: r_7(op).out(),
                    imm: pc.wrapping_add(op & 0xffff_f000),
                },
                0b0_1000 => {
                    let rs1 = r_15(op);
                    let rs2 = r_20(op);
                    let off = store_off(op);

                    match funct3 {
                        // SB
                        0b000 => Instruction::Sb { rs1, rs2, off },
                        // SH
                        0b001 => Instruction::Sh { rs1, rs2, off },
                        // SW
                        0b010 => Instruction::Sw { rs1, rs2, off },
                        _ => unkn,
                    }
                }
                0b0_1011 => {
                    let rd = r_7(op).out();
                    let rs1 = r_15(op);
                    let rs2 = r_20(op);

                    match (funct5, funct3, rs2) {
                        (0b00010, 0b010, Reg::ZERO) => Instruction::Lrw { rd, rs1 },
                        (0b00011, 0b010, _) => Instruction::Scw { rd, rs1, rs2 },
                        (0b01000, 0b010, _) => Instruction::AmoorW { rd, rs1, rs2 },
                        (0b00000, 0b010, _) => Instruction::AmoaddW { rd, rs1, rs2 },
                        _ => unkn,
                    }
                }
                0b0_1100 => {
                    let rd = r_7(op).out();
                    let rs1 = r_15(op);
                    let rs2 = r_20(op);

                    match funct7 {
                        0b000_0000 => match funct3 {
                            // ADD
                            0b000 => Instruction::Add { rd, rs1, rs2 },
                            // SLL
                            0b001 => Instruction::Sll { rd, rs1, rs2 },
                            // SLTU
                            0b010 => Instruction::Slt { rd, rs1, rs2 },
                            // SLTU
                            0b011 => Instruction::Sltu { rd, rs1, rs2 },
                            // XOR
                            0b100 => Instruction::Xor { rd, rs1, rs2 },
                            // SRL
                            0b101 => Instruction::Srl { rd, rs1, rs2 },
                            // OR
                            0b110 => Instruction::Or { rd, rs1, rs2 },
                            // AND
                            0b111 => Instruction::And { rd, rs1, rs2 },
                            _ => unkn,
                        },
                        0b000_0001 => match funct3 {
                            // MUL
                            0b000 => Instruction::Mul { rd, rs1, rs2 },
                            // MULH
                            0b001 => Instruction::Mulh { rd, rs1, rs2 },
                            // DIV
                            0b100 => Instruction::Div { rd, rs1, rs2 },
                            // DIVU
                            0b101 => Instruction::Divu { rd, rs1, rs2 },
                            // MULHU
                            0b011 => Instruction::Mulhu { rd, rs1, rs2 },
                            // REMU
                            0b111 => Instruction::Remu { rd, rs1, rs2 },
                            _ => unkn,
                        },
                        0b010_0000 => match funct3 {
                            // SUB
                            0b000 => Instruction::Sub { rd, rs1, rs2 },
                            // SRA
                            0b101 => Instruction::Sra { rd, rs1, rs2 },
                            _ => unkn,
                        },
                        _ => unkn,
                    }
                }
                // LUI
                0b0_1101 => Instruction::Li {
                    rd: r_7(op).out(),
                    imm: op & 0xffff_f000,
                },
                0b1_1000 => {
                    let rs1 = r_15(op);
                    let rs2 = r_20(op);
                    let off = boff(op);
                    let tpc = pc.wrapping_add(off.extend());

                    match funct3 {
                        // BEQ
                        0b000 => Instruction::Beq { rs1, rs2, tpc },
                        // BNE
                        0b001 => Instruction::Bne { rs1, rs2, tpc },
                        // BLT
                        0b100 => Instruction::Blt { rs1, rs2, tpc },
                        // BGE
                        0b101 => Instruction::Bge { rs1, rs2, tpc },
                        // BLTU
                        0b110 => Instruction::Bltu { rs1, rs2, tpc },
                        // BGEU
                        0b111 => Instruction::Bgeu { rs1, rs2, tpc },
                        _ => unkn,
                    }
                }
                0b1_1001 => match funct3 {
                    // JALR
                    0b000 => Instruction::Jalr {
                        rd: r_7(op).out(),
                        rs1: r_15(op),
                        off: imm_20_se(op),
                    },
                    _ => unkn,
                },
                // JAL
                0b1_1011 => Instruction::Jal {
                    rd: r_7(op).out(),
                    tpc: pc.wrapping_add(jal_off(op).extend()),
                },
                0b1_1100 => {
                    let csr = (op >> 20) as u16;
                    let rd = r_7(op).out();
                    let rs1 = r_15(op);
                    let imm = ((op >> 15) & 0x1f) as i8;

                    match funct3 {
                        0b000 => {
                            match op {
                                // ECALL
                                0x0000_0073 => Instruction::Ecall,
                                // WFI
                                0x1050_0073 => Instruction::Wfi,
                                // MRET
                                0x3020_0073 => Instruction::MRet,
                                _ => unkn,
                            }
                        }
                        // CSRRW
                        0b001 => Instruction::CsrSet { rd, csr, rs1 },
                        // CSRRS
                        0b010 => Instruction::CsrSetBits { rd, csr, rs1 },
                        // CSRRC
                        0b011 => Instruction::CsrClearBits { rd, csr, rs1 },
                        // CSRRWI
                        0b101 => Instruction::CsrManipImm {
                            rd,
                            csr,
                            and_mask: 0,
                            or_mask: imm,
                        },
                        // CSRRCI
                        0b111 => Instruction::CsrManipImm {
                            rd,
                            csr,
                            and_mask: !imm,
                            or_mask: 0,
                        },
                        _ => unkn,
                    }
                }
                _ => unkn,
            };
        } else {
            // 16bit instruction
            let op = op as u16;

            let unkn = Instruction::Unknown16(op);

            *npc = pc.wrapping_add(2);

            *inst = match op & 3 {
                0b00 => match op >> 13 {
                    // C.ADDI4SPN
                    0b000 => {
                        let imm = c_addi4spn_off(op);

                        if imm != 0 {
                            Instruction::AddImm {
                                rd: cr_2x(op),
                                rs1: Reg::SP,
                                imm,
                            }
                        } else {
                            // This encoding is invalid. It's worth special-casing because this is
                            // where we end up if we try to execute full-zero
                            Instruction::Invalid16(op)
                        }
                    }
                    // C.LW
                    0b010 => Instruction::Lw {
                        rd: cr_2x(op).out(),
                        rs1: cr_7x(op),
                        off: c_sw_lw_off(op),
                    },
                    // C.SW
                    0b110 => Instruction::Sw {
                        rs1: cr_7x(op),
                        rs2: cr_2x(op),
                        off: c_sw_lw_off(op),
                    },
                    _ => unkn,
                },
                0b01 => match op >> 13 {
                    // C.ADDI
                    0b000 => Instruction::AddImm {
                        rd: cr_7(op).out(),
                        rs1: cr_7(op),
                        imm: c_li_imm(op) as i16,
                    },
                    // C.JAL
                    0b001 => Instruction::Jal {
                        rd: Reg::RA,
                        tpc: pc.wrapping_add(c_joff(op).extend()),
                    },
                    // C.LI
                    0b010 => Instruction::Li {
                        rd: cr_7(op).out(),
                        imm: c_li_imm(op),
                    },
                    0b011 => {
                        let rd = cr_7(op).out();

                        if rd == Reg::SP {
                            // C.ADDI16SP
                            Instruction::AddImm {
                                rd: Reg::SP,
                                rs1: Reg::SP,
                                imm: c_addisp_off(op),
                            }
                        } else {
                            // C.LUI
                            Instruction::Li {
                                rd,
                                imm: c_li_imm(op) << 12,
                            }
                        }
                    }
                    0b100 => match (op >> 10, (op >> 5) & 3) {
                        // C.SRLI
                        (0b10_0000, _) | (0b10_0100, _) => Instruction::SrlImm {
                            rd: cr_7x(op).out(),
                            rs1: cr_7x(op),
                            shamt: c_shamt(op),
                        },
                        // C.SRAI
                        (0b10_0001, _) | (0b10_0101, _) => Instruction::SraImm {
                            rd: cr_7x(op).out(),
                            rs1: cr_7x(op),
                            shamt: c_shamt(op),
                        },
                        // C.ANDI
                        (0b10_0010, _) | (0b10_0110, _) => Instruction::AndImm {
                            rd: cr_7x(op).out(),
                            rs1: cr_7x(op),
                            imm: c_li_imm(op) as i16,
                        },
                        // C.SUB
                        (0b10_0011, 0b00) => Instruction::Sub {
                            rd: cr_7x(op).out(),
                            rs1: cr_7x(op),
                            rs2: cr_2x(op),
                        },
                        // C.XOR
                        (0b10_0011, 0b01) => Instruction::Xor {
                            rd: cr_7x(op).out(),
                            rs1: cr_7x(op),
                            rs2: cr_2x(op),
                        },
                        // C.OR
                        (0b10_0011, 0b10) => Instruction::Or {
                            rd: cr_7x(op).out(),
                            rs1: cr_7x(op),
                            rs2: cr_2x(op),
                        },
                        // C.AND
                        (0b10_0011, 0b11) => Instruction::And {
                            rd: cr_7x(op).out(),
                            rs1: cr_7x(op),
                            rs2: cr_2x(op),
                        },
                        _ => unkn,
                    },
                    // C.J
                    //
                    // Since this has no side effect and we can compute the target statically, we
                    // could technically just override `*npc` with the target and return a NOP. In
                    // practice it won't really change anything performance-wise and it will make
                    // debug dumps harder to understand.
                    0b101 => Instruction::Jal {
                        rd: Reg::DUMMY,
                        tpc: pc.wrapping_add(c_joff(op).extend()),
                    },
                    // C.BEQZ
                    0b110 => Instruction::Beq {
                        rs1: cr_7x(op),
                        rs2: Reg::ZERO,
                        tpc: pc.wrapping_add(c_boff(op).extend()),
                    },
                    // C.BNEZ
                    0b111 => Instruction::Bne {
                        rs1: cr_7x(op),
                        rs2: Reg::ZERO,
                        tpc: pc.wrapping_add(c_boff(op).extend()),
                    },
                    _ => unkn,
                },
                0b10 => match op >> 13 {
                    // C.SLLI
                    0b000 => Instruction::SllImm {
                        rd: cr_7(op).out(),
                        rs1: cr_7(op),
                        shamt: c_shamt(op),
                    },
                    0b010 => {
                        let rd = cr_7(op);

                        if rd != Reg::ZERO {
                            // C.LWSP
                            Instruction::Lw {
                                rd,
                                rs1: Reg::SP,
                                off: c_lwsp_off(op),
                            }
                        } else {
                            unkn
                        }
                    }
                    // C.SWSP
                    0b110 => Instruction::Sw {
                        rs1: Reg::SP,
                        rs2: cr_2(op),
                        off: c_swsp_off(op),
                    },
                    0b100 => {
                        let r7 = cr_7(op);
                        let r2 = cr_2(op);

                        match op >> 12 {
                            // C.MV
                            0b1000 if r2 != Reg::ZERO && r7 != Reg::ZERO => Instruction::Move {
                                rd: cr_7(op).out(),
                                rs1: r2,
                            },
                            // C.JR
                            0b1000 if r2 == Reg::ZERO && r7 != Reg::ZERO => Instruction::Jalr {
                                rd: Reg::DUMMY,
                                rs1: r7,
                                off: 0,
                            },
                            // C.JALR
                            0b1001 if r2 == Reg::ZERO && r7 != Reg::ZERO => Instruction::Jalr {
                                rd: Reg::RA,
                                rs1: r7,
                                off: 0,
                            },
                            // C.ADD
                            0b1001 if r2 != Reg::ZERO && r7 != Reg::ZERO => Instruction::Add {
                                rd: r7.out(),
                                rs1: r7,
                                rs2: r2,
                            },
                            _ => unkn,
                        }
                    }
                    _ => unkn,
                },
                _ => unkn,
            };
        }
    }

    let page = Page {
        base: base >> PAGE_LEN_SHIFT,
        // We start with an artificially high hit count to give the page a chance to show its use
        // before it's evicted
        hit_score: (PAGE_LEN_OP << 4) as u32,
        instructions,
    };

    let pl = m.cpu.decoder.pages.len();

    if pl < PAGE_CACHE_MAX {
        m.cpu.decoder.pages.push(page);
        m.cpu.decoder.page_lut[lidx] = Some(pl as u16);
        pl
    } else {
        let ip = m.cpu.decoder.evict_page();
        m.cpu.decoder.pages[ip] = page;
        m.cpu.decoder.page_lut[lidx] = Some(ip as u16);
        ip
    }
}

struct Page {
    /// (Instruction, next_pc)
    instructions: [(Instruction, u32); PAGE_LEN_OP],
    /// Start address, shifted by PAGE_LEN_SHIFT
    base: u32,
    hit_score: u32,
}

#[derive(Copy, Clone, Debug)]
pub enum Instruction {
    InvalidAddress(u32),
    Unknown32(u32),
    Unknown16(u16),
    Invalid16(u16),

    // ALU/register manipulation
    Li {
        rd: Reg,
        imm: u32,
    },
    Move {
        rd: Reg,
        rs1: Reg,
    },
    Add {
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },
    Slt {
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },
    Sltu {
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },
    Xor {
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },
    Or {
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },
    Sub {
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },
    And {
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },
    Mul {
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },
    Mulh {
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },
    Mulhu {
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },
    Div {
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },
    Divu {
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },
    Remu {
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },
    AddImm {
        rd: Reg,
        rs1: Reg,
        imm: i16,
    },
    Slti {
        rd: Reg,
        rs1: Reg,
        imm: i16,
    },
    Sltiu {
        rd: Reg,
        rs1: Reg,
        imm: i16,
    },
    XorImm {
        rd: Reg,
        rs1: Reg,
        imm: i16,
    },
    OrImm {
        rd: Reg,
        rs1: Reg,
        imm: i16,
    },
    AndImm {
        rd: Reg,
        rs1: Reg,
        imm: i16,
    },
    SraImm {
        rd: Reg,
        rs1: Reg,
        shamt: u8,
    },
    SrlImm {
        rd: Reg,
        rs1: Reg,
        shamt: u8,
    },
    Sll {
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },
    Srl {
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },
    Sra {
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },
    SllImm {
        rd: Reg,
        rs1: Reg,
        shamt: u8,
    },

    // Jumps and branches
    Jalr {
        rd: Reg,
        rs1: Reg,
        off: i16,
    },
    Jal {
        rd: Reg,
        tpc: u32,
    },
    Beq {
        rs1: Reg,
        rs2: Reg,
        tpc: u32,
    },
    Bne {
        rs1: Reg,
        rs2: Reg,
        tpc: u32,
    },
    Blt {
        rs1: Reg,
        rs2: Reg,
        tpc: u32,
    },
    Bltu {
        rs1: Reg,
        rs2: Reg,
        tpc: u32,
    },
    Bge {
        rs1: Reg,
        rs2: Reg,
        tpc: u32,
    },
    Bgeu {
        rs1: Reg,
        rs2: Reg,
        tpc: u32,
    },
    MRet,

    // Memory access
    Lb {
        rd: Reg,
        rs1: Reg,
        off: i16,
    },
    Lbu {
        rd: Reg,
        rs1: Reg,
        off: i16,
    },
    Lh {
        rd: Reg,
        rs1: Reg,
        off: i16,
    },
    Lhu {
        rd: Reg,
        rs1: Reg,
        off: i16,
    },
    Lw {
        rd: Reg,
        rs1: Reg,
        off: i16,
    },
    Lrw {
        rd: Reg,
        rs1: Reg,
    },
    Sb {
        rs1: Reg,
        rs2: Reg,
        off: i16,
    },
    Sh {
        rs1: Reg,
        rs2: Reg,
        off: i16,
    },
    Sw {
        rs1: Reg,
        rs2: Reg,
        off: i16,
    },
    Scw {
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },
    AmoorW {
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },
    AmoaddW {
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },

    // CSR/system stuff
    CsrSet {
        rd: Reg,
        csr: u16,
        rs1: Reg,
    },
    CsrSetBits {
        rd: Reg,
        csr: u16,
        rs1: Reg,
    },
    CsrClearBits {
        rd: Reg,
        csr: u16,
        rs1: Reg,
    },
    CsrManipImm {
        rd: Reg,
        csr: u16,
        and_mask: i8,
        or_mask: i8,
    },
    Ecall,
    Wfi,
    FenceI,
}

impl Instruction {
    fn nop() -> Instruction {
        Instruction::Li {
            rd: Reg::DUMMY,
            imm: 0x504f4e,
        }
    }
}

const fn r_7(op: u32) -> Reg {
    Reg(((op >> 7) & 0x1f) as u8)
}

const fn r_15(op: u32) -> Reg {
    Reg(((op >> 15) & 0x1f) as u8)
}

const fn r_20(op: u32) -> Reg {
    Reg(((op >> 20) & 0x1f) as u8)
}

const fn imm_20_se(op: u32) -> i16 {
    let op = op as i32;

    (op >> 20) as i16
}

const fn cr_7(op: u16) -> Reg {
    Reg(((op >> 7) & 0x1f) as u8)
}

const fn cr_2x(op: u16) -> Reg {
    Reg((((op >> 2) & 7) + 8) as u8)
}

const fn cr_7x(op: u16) -> Reg {
    Reg((((op >> 7) & 7) + 8) as u8)
}

const fn cr_2(op: u16) -> Reg {
    Reg(((op >> 2) & 0x1f) as u8)
}

const fn store_off(op: u32) -> i16 {
    let mut off = ((op as i32) >> 20) as u32;

    off &= !0x1f;
    off |= (op >> 7) & 0x1f;

    off as i16
}

const fn boff(op: u32) -> i16 {
    let mut off = ((op as i32) >> 19) as u32;
    off &= !0xfff;
    off |= (op << 4) & (1 << 11);
    off |= (op >> 20) & (0x3f << 5);
    off |= (op >> 7) & (0xf << 1);

    off as i16
}

const fn jal_off(op: u32) -> i32 {
    // Sign bit
    let mut off = ((op as i32) >> 11) as u32;
    off &= !0xfffff;
    off |= op & (0xff << 12);
    // Bit 11
    off |= (op >> 9) & (1 << 11);
    // Bits 10:1
    off |= (op >> 20) & (0x3ff << 1);

    off as i32
}

const fn shamt(op: u32) -> u8 {
    ((op >> 20) & 0x1f) as u8
}

const fn c_addisp_off(op: u16) -> i16 {
    let op = op as u32;
    let mut off = (((op << 19) as i32) >> 22) as u32;
    off &= !0x1ff;
    off |= (op >> 2) & (1 << 4);
    off |= (op << 1) & (1 << 6);
    off |= (op << 4) & (3 << 7);
    off |= (op << 3) & (1 << 5);

    off as i16
}

const fn c_swsp_off(op: u16) -> i16 {
    // Not sign-extended
    let mut off = 0;

    off |= (op >> 7) & (0xf << 2);
    off |= (op >> 1) & (3 << 6);

    off as i16
}

const fn c_li_imm(op: u16) -> u32 {
    let op = op as u32;
    let mut off = (((op << 19) as i32) >> 26) as u32;
    off &= !0x1f;
    off |= (op >> 2) & 0x1f;

    off
}

const fn c_boff(op: u16) -> i16 {
    let op = op as u32;
    let mut off = (((op << 19) as i32) >> 24) as u32;
    off &= !0x7f;
    off |= (op >> 8) & (3 << 2);
    off |= op & (3 << 5);
    off |= (op >> 3) & 3;
    off |= (op << 2) & (1 << 4);

    (off << 1) as i16
}

const fn c_addi4spn_off(op: u16) -> i16 {
    // Not sign-extended
    let mut off = 0;

    off |= (op >> 7) & (3 << 4);
    off |= (op >> 1) & (0xf << 6);
    off |= (op >> 4) & (1 << 2);
    off |= (op >> 2) & (1 << 3);

    off as i16
}

const fn c_sw_lw_off(op: u16) -> i16 {
    // Not sign-extended
    let mut off = 0;

    off |= (op >> 7) & (7 << 3);
    off |= (op >> 4) & (1 << 2);
    off |= (op << 1) & (1 << 6);

    off as i16
}

const fn c_shamt(op: u16) -> u8 {
    let s = (op >> 2) & 0x1f;

    (s | ((op >> 7) & (1 << 5))) as u8
}

const fn c_lwsp_off(op: u16) -> i16 {
    // Not sign-extended
    let op = op as u32;
    let mut off = 0;

    off |= (op >> 7) & (1 << 5);
    off |= (op >> 2) & (7 << 2);
    off |= (op << 4) & (3 << 6);

    off as i16
}

const fn c_joff(op: u16) -> i16 {
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

    off as i16
}

#[test]
fn test_lut_idx() {
    let plen = PAGE_LEN_BYTES as u32;

    assert_eq!(lut_idx(RAM.base) as u32, 0);
    assert_eq!(lut_idx(RAM.base + 10) as u32, 0);
    assert_eq!(lut_idx(RAM.base + plen - 1) as u32, 0);
    assert_eq!(lut_idx(RAM.base + plen) as u32, 1);
    assert_eq!(lut_idx(RAM.base + plen + 4) as u32, 1);
    assert_eq!(lut_idx(RAM.base + plen * 10 + plen / 2) as u32, 10);

    let rom_off = RAM.len >> PAGE_LEN_SHIFT;

    assert_eq!(lut_idx(ROM.base) as u32, rom_off);
    assert_eq!(lut_idx(ROM.base + 10) as u32, rom_off);
    assert_eq!(lut_idx(ROM.base + plen - 1) as u32, rom_off);
    assert_eq!(lut_idx(ROM.base + plen) as u32, rom_off + 1);
    assert_eq!(lut_idx(ROM.base + plen + 4) as u32, rom_off + 1);
    assert_eq!(
        lut_idx(ROM.base + plen * 10 + plen / 2) as u32,
        rom_off + 10
    );
}

#[test]
fn test_lut_idx_max() {
    assert!(lut_idx(ROM.base + ROM.len - 1) < PAGE_TOTAL);
    assert!(lut_idx(RAM.base + RAM.len - 1) < PAGE_TOTAL);
}

#[test]
fn test_lut_idx_to_base() {
    let plen = PAGE_LEN_BYTES as u32;

    assert_eq!(lut_idx_to_base(lut_idx(ROM.base)) as u32, ROM.base);
    assert_eq!(
        lut_idx_to_base(lut_idx(ROM.base + plen)) as u32,
        ROM.base + plen
    );
    assert_eq!(
        lut_idx_to_base(lut_idx(ROM.base + plen * 10)) as u32,
        ROM.base + plen * 10
    );
    assert_eq!(
        lut_idx_to_base(lut_idx(ROM.base + plen * 10 + plen - 1)) as u32,
        ROM.base + plen * 10
    );

    assert_eq!(
        lut_idx_to_base(lut_idx(RAM.base + plen * 10 + plen - 1)) as u32,
        RAM.base + plen * 10
    );
}

#[test]
fn test_rd_rs1_rs2() {
    let add_x8_x11_x27 = 0x01b58433;

    assert_eq!(r_7(add_x8_x11_x27), Reg(8));
    assert_eq!(r_15(add_x8_x11_x27), Reg(11));
    assert_eq!(r_20(add_x8_x11_x27), Reg(27));

    let add_x31_x31_x31 = 0x01ff8fb3;

    assert_eq!(r_7(add_x31_x31_x31), Reg(31));
    assert_eq!(r_15(add_x31_x31_x31), Reg(31));
    assert_eq!(r_20(add_x31_x31_x31), Reg(31));

    let sltu_a7_a7_a7 = 0x0118b8b3;

    assert_eq!(r_7(sltu_a7_a7_a7), Reg(17));
    assert_eq!(r_15(sltu_a7_a7_a7), Reg(17));
    assert_eq!(r_20(sltu_a7_a7_a7), Reg(17));

    let jalr_r0_8_ra = 0x00808067;
    assert_eq!(r_15(jalr_r0_8_ra), Reg::RA);
    assert_eq!(r_7(jalr_r0_8_ra), Reg::ZERO);
    assert_eq!(r_7(jalr_r0_8_ra).out(), Reg::DUMMY);
}

#[test]
fn test_imm_20_se() {
    // addi s0, a1, 0x7ff
    assert_eq!(imm_20_se(0x7ff58413), 0x7ff);
    // ori x31, x31, -0x800
    assert_eq!(imm_20_se(0x800fef93), -2048);
    // andi x31, x31, -1
    assert_eq!(imm_20_se(0xffffff93), -1);
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
    assert_eq!(store_off(0xfe002fa3), -1);
    // sw x0, 0x7ff(x0)
    assert_eq!(store_off(0x7e002fa3), 0x7ff);
    // sw x0, -0x800(x0)
    assert_eq!(store_off(0x80002023), -0x800);
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
    assert_eq!(boff(0xfe72efe3), -2);
    // bltu t0, t2, -32
    assert_eq!(boff(0xfe72e0e3), -32);
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
fn test_jal_off() {
    // jal x0, 0
    assert_eq!(jal_off(0x0000006f), 0);
    // jal x0, -2
    assert_eq!(jal_off(0xfffff06f), -2);
    // jal x0, -64
    assert_eq!(jal_off(0xfc1ff06f), -64);
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
    assert_eq!(jal_off(0x8000006f), -0x100000);
}

#[test]
fn test_c_addisp_off() {
    // c.addi16sp -64
    assert_eq!(c_addisp_off(0x7139), -64);
    // c.addi16sp -16
    assert_eq!(c_addisp_off(0x717d), -16);
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
    assert_eq!(c_addisp_off(0x7101), -512);
    // c.addi16sp -512
    assert_eq!(c_addisp_off(0x7101), -512);
    // c.addi16sp 496
    assert_eq!(c_addisp_off(0x617d), 496);
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
    assert_eq!(c_boff(0xfd7d), -2);
    // c.bnez x10, -256
    assert_eq!(c_boff(0xf101), -256);
    // c.bnez x15, -256
    assert_eq!(c_boff(0xf381), -256);
    // c.bnez x15, -2
    assert_eq!(c_boff(0xf381), -256);
    // c.bnez x15, -2
    assert_eq!(c_boff(0xfffd), -2);
    // c.bnez x8, -2
    assert_eq!(c_boff(0xfc7d), -2);
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
    assert_eq!(c_boff(0xf101), -256);
    // c.bnez x10, 254
    assert_eq!(c_boff(0xed7d), 254);
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
    assert_eq!(c_joff(0xb001), -0x800);
    // c.j -2
    assert_eq!(c_joff(0xbffd), -2);
    // c.j 0x4aa
    assert_eq!(c_joff(0xa16d), 0x4aa);
}
