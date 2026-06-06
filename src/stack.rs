use crate::cpu::Cpu;
use crate::mem::{GuestUSize, Memory, MutPtr, Ptr};

pub fn prep_stack_for_start(mem: &mut Memory, cpu: &mut Cpu) {
    let stack_base: usize = 1 << 32;

    let mut reversed_data = Vec::<u8>::new();
    let magic: &str = "skymrp";
    reversed_data.reserve(magic.bytes().len());
    for &c in magic.as_bytes().iter().rev() {
        reversed_data.push(c);
    }

    let stack_ptr: MutPtr<u8> =
        Ptr::from_bits((stack_base - reversed_data.len()).try_into().unwrap());
    let stack_height: GuestUSize = reversed_data.len().try_into().unwrap();

    assert!(stack_height < Memory::STACK_SIZE);

    cpu.regs_mut()[Cpu::SP] = stack_ptr.to_bits();
}
