use crate::mem::{Memory, MutPtr, Ptr};
use crate::mrp::Mrp;
pub struct Syscall {}

fn encode_a32_svc(imm: u32) -> u32 {
    assert!(imm & 0xff000000 == 0);
    imm | 0xef000000
}
fn encode_a32_ret() -> u32 {
    0xe12fff1e
}
fn encode_a32_trap() -> u32 {
    0xe7ffdefe
}

impl Syscall {
    pub fn new() -> Syscall {
        Syscall {}
    }

    pub fn setup_stubs(&self, stub_addr: u32, mem: &mut Memory) {
        let ptr: MutPtr<u32> = Ptr::from_bits(stub_addr);
        mem.write(ptr + 0, encode_a32_svc(0));
        mem.write(ptr + 1, encode_a32_ret());
        mem.write(ptr + 2, encode_a32_trap());
    }

    /// Handle an SVC instruction encountered during CPU emulation.
    pub fn handle_svc(&mut self, bin: &Mrp, current_instruction: u32, svc: u32) {
        match svc {
            _ => {
                panic!("Unexpected SVC #{} at {:#x}", svc, current_instruction);
            }
        }
    }
}
