pub mod rom;
pub use rom::{RomGenerationType, Rom, RomDigest};

use cryptoxide::{
    hashing::blake2b::{self, Blake2b},
    kdf::argon2,
};

// 1 byte operator
// 3 bytes operands (src1, src2, dst)
// 28 bytes data
const INSTR_SIZE: usize = 20;
const NB_REGS: usize = 1 << REGS_BITS;
const REGS_BITS: usize = 5;
const REGS_INDEX_MASK: u8 = NB_REGS as u8 - 1;

type Register = u64;

const REGISTER_SIZE: usize = std::mem::size_of::<Register>();

struct VM {
    program: Program,
    regs: [Register; NB_REGS],
    ip: u32,
    prog_digest: blake2b::Context<512>,
    mem_digest: blake2b::Context<512>,
    prog_seed: [u8; 64],
    memory_counter: u32,
    loop_counter: u32,
}

#[derive(Clone, Copy)]
enum Instr {
    Op3(Op3),
    Op2(Op2),
}

#[derive(Clone, Copy)]
enum Op3 {
    Add,
    Mul,
    MulH,
    Xor,
    Div,
    Mod,
    And,
    Hash(u8),
}

#[derive(Clone, Copy)]
enum Op2 {
    ISqrt,
    Neg,
    BitRev,
    RotL,
    RotR,
}

// special encoding

impl From<u8> for Instr {
    fn from(value: u8) -> Self {
        match value {
            0..40 => Instr::Op3(Op3::Add),                   // 40
            40..80 => Instr::Op3(Op3::Mul),                  // 40
            80..96 => Instr::Op3(Op3::MulH),                 // 16
            96..112 => Instr::Op3(Op3::Div),                 // 16
            112..128 => Instr::Op3(Op3::Mod),                // 16
            128..138 => Instr::Op2(Op2::ISqrt),              // 10
            138..148 => Instr::Op2(Op2::BitRev),             // 10
            148..188 => Instr::Op3(Op3::Xor),                // 40
            188..204 => Instr::Op2(Op2::RotL),               // 16
            204..220 => Instr::Op2(Op2::RotR),               // 16
            220..240 => Instr::Op2(Op2::Neg),                // 20
            240..248 => Instr::Op3(Op3::And),                // 8
            248..=255 => Instr::Op3(Op3::Hash(value - 248)), // 8
        }
    }
}

#[derive(Clone, Copy)]
enum Operand {
    Reg,
    Memory,
    Literal,
    Special1,
    Special2,
}

impl From<u8> for Operand {
    fn from(value: u8) -> Self {
        assert!(value <= 0x0f);
        match value {
            0..5 => Self::Reg,
            5..9 => Self::Memory,
            9..13 => Self::Literal,
            13..14 => Self::Special1,
            14.. => Self::Special2,
        }
    }
}

impl VM {
    /// Create a new VM which is specific to the ROM by using the RomDigest,
    /// but mainly dependent on the salt which is an arbitrary byte content
    pub fn new(rom_digest: &RomDigest, nb_instrs: u32, salt: &[u8]) -> Self {
        const DIGEST_INIT_SIZE: usize = 64;
        const REGS_CONTENT_SIZE: usize = REGISTER_SIZE * NB_REGS;

        let mut init_buffer = [0; REGS_CONTENT_SIZE + 3 * DIGEST_INIT_SIZE];

        let mut init_buffer_input = rom_digest.0.to_vec();
        init_buffer_input.extend_from_slice(salt);
        argon2::hprime(&mut init_buffer, &init_buffer_input);

        let (init_buffer_regs, init_buffer_digests) = init_buffer.split_at(REGS_CONTENT_SIZE);

        let mut regs = [0; NB_REGS];
        for (reg, reg_bytes) in regs.iter_mut().zip(init_buffer_regs.chunks(REGISTER_SIZE)) {
            *reg = u64::from_le_bytes(*<&[u8; 8]>::try_from(reg_bytes).unwrap());
        }

        let mut digests = init_buffer_digests.chunks(DIGEST_INIT_SIZE);
        let prog_digest = Blake2b::<512>::new().update(&digests.next().unwrap());
        let mem_digest = Blake2b::<512>::new().update(&digests.next().unwrap());
        let prog_seed = *<&[u8; 64]>::try_from(digests.next().unwrap()).unwrap();

        assert_eq!(digests.next(), None);

        let program = Program::new(nb_instrs);

        Self {
            program,
            regs,
            prog_digest,
            mem_digest,
            prog_seed,
            ip: 0,
            loop_counter: 0,
            memory_counter: 0,
        }
    }

    pub fn step(&mut self, rom: &Rom) {
        execute_one_instruction(self, rom);
        self.ip = self.ip.wrapping_add(1);
    }

    fn sum_regs(&self) -> u64 {
        self.regs.iter().fold(0, |acc, r| acc.wrapping_add(*r))
    }

    pub fn post_instructions(&mut self) {
        let sum_regs = self.sum_regs();

        let prog_value = self
            .prog_digest
            .clone()
            .update(&sum_regs.to_le_bytes())
            .finalize();
        let mem_value = self
            .mem_digest
            .clone()
            .update(&sum_regs.to_le_bytes())
            .finalize();

        let mixing_value = Blake2b::<512>::new()
            .update(&prog_value)
            .update(&mem_value)
            .update(&self.loop_counter.to_le_bytes())
            .finalize();
        let mut mixing_out = vec![0; NB_REGS * REGISTER_SIZE * 32];
        argon2::hprime(&mut mixing_out, &mixing_value);

        for mem_chunks in mixing_out.chunks(NB_REGS * REGISTER_SIZE) {
            for (reg, reg_chunk) in self.regs.iter_mut().zip(mem_chunks.chunks(8)) {
                *reg ^= u64::from_le_bytes(*<&[u8; 8]>::try_from(reg_chunk).unwrap())
            }
        }

        self.prog_seed = prog_value;
        self.loop_counter = self.loop_counter.wrapping_add(1)
    }

    pub fn execute(&mut self, rom: &Rom, instr: u32) {
        self.program.shuffle(&self.prog_seed);
        for _ in 0..instr {
            self.step(rom)
        }
        self.post_instructions()
    }

    pub fn finalize(self) -> [u8; 64] {
        let prog_digest = self.prog_digest.finalize();
        let mem_digest = self.mem_digest.finalize();
        let mut context = Blake2b::<512>::new()
            .update(&prog_digest)
            .update(&mem_digest)
            .update(&self.memory_counter.to_le_bytes());
        for r in self.regs {
            context.update_mut(&r.to_le_bytes());
        }
        context.finalize()
    }

    #[allow(dead_code)]
    pub(crate) fn debug(&self) -> String {
        let mut out = String::new();
        for (i, r) in self.regs.iter().enumerate() {
            out.push_str(&format!("[{i:02x}] {r:016x} "));
            if (i % 4) == 3 {
                out.push('\n');
            }
        }
        out.push_str(&format!("ip {:08x}\n", self.ip,));
        out
    }
}

struct Program {
    instructions: Vec<u8>,
}

impl Program {
    pub fn new(nb_instrs: u32) -> Self {
        let size = nb_instrs as usize * INSTR_SIZE;
        let instructions = vec![0; size];
        Self { instructions }
    }

    pub fn at(&self, i: u32) -> &[u8; INSTR_SIZE] {
        let start = (i as usize).wrapping_mul(INSTR_SIZE) % self.instructions.len();
        <&[u8; INSTR_SIZE]>::try_from(&self.instructions[start..start + INSTR_SIZE]).unwrap()
    }

    pub fn shuffle(&mut self, seed: &[u8; 64]) {
        argon2::hprime(&mut self.instructions, seed)
    }
}

#[derive(Clone)]
pub struct Instruction {
    opcode: Instr,
    op1: Operand,
    op2: Operand,
    r1: u8,
    r2: u8,
    r3: u8,
    lit1: u64,
    lit2: u64,
}

#[inline]
fn decode_instruction(instruction: &[u8; INSTR_SIZE]) -> Instruction {
    let opcode = Instr::from(instruction[0]);
    let op1 = Operand::from(instruction[1] >> 4);
    let op2 = Operand::from(instruction[1] & 0x0f);

    let rs = ((instruction[2] as u16) << 8) | (instruction[3] as u16);
    let r1 = ((rs >> (2 * REGS_BITS)) as u8) & REGS_INDEX_MASK;
    let r2 = ((rs >> REGS_BITS) as u8) & REGS_INDEX_MASK;
    let r3 = (rs as u8) & REGS_INDEX_MASK;

    let lit1 = u64::from_le_bytes(*<&[u8; 8]>::try_from(&instruction[4..12]).unwrap());
    let lit2 = u64::from_le_bytes(*<&[u8; 8]>::try_from(&instruction[12..20]).unwrap());

    Instruction {
        opcode,
        op1,
        op2,
        r1,
        r2,
        r3,
        lit1,
        lit2,
    }
}

fn execute_one_instruction(vm: &mut VM, rom: &Rom) {
    let prog_chunk = *vm.program.at(vm.ip);

    macro_rules! mem_access64 {
        ($vm:ident, $rom:ident, $addr:ident) => {{
            let mem = rom.at($addr as u32);
            $vm.mem_digest.update_mut(mem);
            $vm.memory_counter = $vm.memory_counter.wrapping_add(1);

            // divide memory access into 8 chunks of 8 bytes
            let idx = (($vm.memory_counter % (64 / 8)) as usize) * 8;
            u64::from_le_bytes(*<&[u8; 8]>::try_from(&mem[idx..idx + 8]).unwrap())
        }};
    }

    macro_rules! special1_value64 {
        ($vm:ident) => {{
            let r = $vm.prog_digest.clone().finalize();
            u64::from_le_bytes(*<&[u8; 8]>::try_from(&r[0..8]).unwrap())
        }};
    }

    macro_rules! special2_value64 {
        ($vm:ident) => {{
            let r = $vm.mem_digest.clone().finalize();
            u64::from_le_bytes(*<&[u8; 8]>::try_from(&r[0..8]).unwrap())
        }};
    }

    let Instruction {
        opcode,
        op1,
        op2,
        r1,
        r2,
        r3,
        lit1,
        lit2,
    } = decode_instruction(&prog_chunk);

    match opcode {
        Instr::Op3(operator) => {
            let src1 = match op1 {
                Operand::Reg => vm.regs[r1 as usize],
                Operand::Memory => mem_access64!(vm, rom, lit1),
                Operand::Literal => lit1,
                Operand::Special1 => special1_value64!(vm),
                Operand::Special2 => special2_value64!(vm),
            };
            let src2 = match op2 {
                Operand::Reg => vm.regs[r2 as usize],
                Operand::Memory => mem_access64!(vm, rom, lit2),
                Operand::Literal => lit2,
                Operand::Special1 => special1_value64!(vm),
                Operand::Special2 => special2_value64!(vm),
            };

            let result = match operator {
                Op3::Add => src1.wrapping_add(src2),
                Op3::Mul => src1.wrapping_mul(src2),
                Op3::MulH => ((src1 as u128 * src2 as u128) >> 64) as u64,
                Op3::Xor => src1 ^ src2,
                Op3::Div => {
                    if src2 == 0 {
                        special1_value64!(vm)
                    } else {
                        src1 / src2
                    }
                }
                Op3::Mod => {
                    if src2 == 0 {
                        special1_value64!(vm)
                    } else {
                        src1 / src2
                    }
                }
                Op3::And => src1 & src2,
                Op3::Hash(v) => {
                    assert!(v < 8);
                    let out = Blake2b::<512>::new()
                        .update(&src1.to_le_bytes())
                        .update(&src2.to_le_bytes())
                        .finalize();
                    if let Some(chunk) = out.chunks(8).nth(v as usize) {
                        u64::from_le_bytes(*<&[u8; 8]>::try_from(chunk).unwrap())
                    } else {
                        panic!("chunk doesn't exist")
                    }
                }
            };

            vm.regs[r3 as usize] = result;
        }
        Instr::Op2(operator) => {
            let src1 = match op1 {
                Operand::Reg => vm.regs[r1 as usize],
                Operand::Memory => mem_access64!(vm, rom, lit1),
                Operand::Literal => lit1,
                Operand::Special1 => special1_value64!(vm),
                Operand::Special2 => special2_value64!(vm),
            };

            let result = match operator {
                Op2::Neg => !src1,
                Op2::RotL => src1.rotate_left(r1 as u32),
                Op2::RotR => src1.rotate_right(r1 as u32),
                Op2::ISqrt => src1.isqrt(),
                Op2::BitRev => src1.reverse_bits(),
            };
            vm.regs[r3 as usize] = result;
        }
    }
    vm.prog_digest.update_mut(&prog_chunk);
}

pub fn hash(salt: &[u8], rom: &Rom, nb_loops: u32, nb_instrs: u32) -> [u8; 64] {
    assert!(nb_loops >= 2);
    assert!(nb_instrs >= 256);
    let mut vm = VM::new(&rom.digest, nb_instrs, salt);
    for _ in 0..nb_loops {
        vm.execute(rom, nb_instrs);
    }
    vm.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instruction_count_diff() {
        let rom = Rom::new(
            b"password1",
            RomGenerationType::TwoStep {
                pre_size: 1024,
                mixing_numbers: 4,
            },
            10_240,
        );

        let h1 = hash(&0u128.to_be_bytes(), &rom, 8, 256);
        let h2 = hash(&0u128.to_be_bytes(), &rom, 8, 257);

        assert_ne!(h1, h2);
    }

    /*
    #[test]
    fn check_ip_stale() {
        let rom = Rom::new(b"password1", 1024, 10_240);

        let salt = &0u128.to_be_bytes();
        let nb_instrs = 100_000;
        let mut vm = VM::new(&rom.digest, nb_instrs, salt);
        for i in 0..nb_instrs {
            let prev = vm.debug();
            vm.step(&rom);
        }
    }
    */

    #[test]
    fn test() {
        const PRE_SIZE: usize = 16 * 1024;
        const SIZE: usize = 10 * 1024 * 1024;
        const NB_INSTR: u32 = 256;

        let rom = Rom::new(
            b"123",
            RomGenerationType::TwoStep {
                pre_size: PRE_SIZE,
                mixing_numbers: 4,
            },
            SIZE,
        );

        let h = hash(b"hello", &rom, 8, NB_INSTR);
        println!("{:?}", h);
    }

    #[test]
    fn test_eq() {
        const PRE_SIZE: usize = 16 * 1024;
        const SIZE: usize = 10 * 1024 * 1024;
        const NB_INSTR: u32 = 256;

        const EXPECTED: [u8; 64] = [
            56, 148, 1, 228, 59, 96, 211, 173, 9, 98, 68, 61, 89, 171, 124, 171, 124, 183, 200,
            196, 29, 43, 133, 168, 218, 217, 255, 71, 234, 182, 97, 158, 231, 156, 56, 230, 61, 54,
            248, 199, 150, 15, 66, 0, 149, 185, 85, 177, 192, 220, 237, 77, 195, 106, 140, 223,
            175, 93, 238, 220, 57, 159, 180, 243,
        ];

        let rom = Rom::new(
            b"123",
            RomGenerationType::TwoStep {
                pre_size: PRE_SIZE,
                mixing_numbers: 4,
            },
            SIZE,
        );

        let h = hash(b"hello", &rom, 8, NB_INSTR);
        assert_eq!(h, EXPECTED);
    }
}
