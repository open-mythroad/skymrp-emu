use crate::cpu::Cpu;
use crate::mem::{GuestUSize, Memory, MutPtr, Ptr};

pub fn prep_stack_for_start(mem: &mut Memory, cpu: &mut Cpu) {
    let stack_base: usize = 1 << 32 - 1;

    let stack_ptr: MutPtr<u8> = Ptr::from_bits((stack_base).try_into().unwrap());

    cpu.regs_mut()[Cpu::SP] = stack_ptr.to_bits();
}
