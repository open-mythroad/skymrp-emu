use crate::cpu::Cpu;
use crate::mem::{GuestUSize, Memory, MutPtr, Ptr};

pub fn prep_stack_for_start(_mem: &mut Memory, cpu: &mut Cpu) {
    let stack_base: usize = Memory::STACK_HIGH_END as usize;

    let mut reversed_data = Vec::<u8>::new();
    let magic: &str = "skymrp";
    reversed_data.reserve(magic.bytes().len());
    for &c in magic.as_bytes().iter().rev() {
        reversed_data.push(c);
    }
    // Pad to ensure stack is 4-byte aligned
    let misaligned_by = reversed_data.len() % 4;
    let pad_by = if misaligned_by != 0 {
        4 - misaligned_by
    } else {
        0
    };
    reversed_data.resize(reversed_data.len() + pad_by, 0);

    let stack_ptr: MutPtr<u8> =
        Ptr::from_bits((stack_base - reversed_data.len()).try_into().unwrap());
    let stack_height: GuestUSize = reversed_data.len().try_into().unwrap();

    assert!(stack_height < Memory::STACK_SIZE);
    assert!(stack_height % 4 == 0);
    cpu.regs_mut()[Cpu::SP] = stack_ptr.to_bits();
}
