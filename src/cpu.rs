use crate::mem::{ConstPtr, Memory, MutPtr, Ptr, SafeRead};

// Import functions from C++
use skymrp_dynarmic_wrapper::*;

type VAddr = u32;

fn skymrp_cpu_read_impl<T: SafeRead>(mem: *mut skymrp_Memory, addr: VAddr) -> T {
    let mem = unsafe { &mut *mem.cast::<Memory>() };
    let ptr: ConstPtr<T> = Ptr::from_bits(addr);
    mem.read(ptr)
}

fn skymrp_cpu_write_impl<T>(mem: *mut skymrp_Memory, addr: VAddr, value: T) {
    let mem = unsafe { &mut *mem.cast::<Memory>() };
    let ptr: MutPtr<T> = Ptr::from_bits(addr);
    mem.write(ptr, value)
}

// Export functions for use by C++
#[no_mangle]
extern "C" fn skymrp_cpu_read_u8(mem: *mut skymrp_Memory, addr: VAddr) -> u8 {
    skymrp_cpu_read_impl(mem, addr)
}
#[no_mangle]
extern "C" fn skymrp_cpu_read_u16(mem: *mut skymrp_Memory, addr: VAddr) -> u16 {
    skymrp_cpu_read_impl(mem, addr)
}
#[no_mangle]
extern "C" fn skymrp_cpu_read_u32(mem: *mut skymrp_Memory, addr: VAddr) -> u32 {
    skymrp_cpu_read_impl(mem, addr)
}
#[no_mangle]
extern "C" fn skymrp_cpu_read_u64(mem: *mut skymrp_Memory, addr: VAddr) -> u64 {
    skymrp_cpu_read_impl(mem, addr)
}
#[no_mangle]
extern "C" fn skymrp_cpu_write_u8(mem: *mut skymrp_Memory, addr: VAddr, value: u8) {
    skymrp_cpu_write_impl(mem, addr, value);
}
#[no_mangle]
extern "C" fn skymrp_cpu_write_u16(mem: *mut skymrp_Memory, addr: VAddr, value: u16) {
    skymrp_cpu_write_impl(mem, addr, value);
}
#[no_mangle]
extern "C" fn skymrp_cpu_write_u32(mem: *mut skymrp_Memory, addr: VAddr, value: u32) {
    skymrp_cpu_write_impl(mem, addr, value);
}
#[no_mangle]
extern "C" fn skymrp_cpu_write_u64(mem: *mut skymrp_Memory, addr: VAddr, value: u64) {
    skymrp_cpu_write_impl(mem, addr, value);
}

pub struct Cpu {
    dynarmic_wrapper: *mut skymrp_DynarmicWrapper,
}

impl Drop for Cpu {
    fn drop(&mut self) {
        unsafe { skymrp_DynarmicWrapper_delete(self.dynarmic_wrapper) }
    }
}

/// Why CPU execution ended.
#[derive(Debug)]
pub enum CpuState {
    /// Execution halted due to using up all remaining ticks.
    Normal,
    /// SVC instruction encountered.
    Svc(u32),
}

impl Cpu {
    /// The register number of the stack pointer.
    pub const SP: usize = 13;
    /// The register number of the link register.
    #[allow(unused)]
    pub const LR: usize = 14;
    /// The register number of the program counter.
    pub const PC: usize = 15;

    pub fn new() -> Cpu {
        let dynarmic_wrapper = unsafe { skymrp_DynarmicWrapper_new() };
        Cpu { dynarmic_wrapper }
    }

    pub fn regs(&self) -> &[u32; 16] {
        unsafe {
            let ptr = skymrp_DynarmicWrapper_regs_const(self.dynarmic_wrapper);
            &*(ptr as *const [u32; 16])
        }
    }
    pub fn regs_mut(&mut self) -> &mut [u32; 16] {
        unsafe {
            let ptr = skymrp_DynarmicWrapper_regs_mut(self.dynarmic_wrapper);
            &mut *(ptr as *mut [u32; 16])
        }
    }

    #[must_use]
    pub fn run(&mut self, mem: &mut Memory, ticks: &mut u64) -> CpuState {
        unsafe {
            let res = skymrp_DynarmicWrapper_run(
                self.dynarmic_wrapper,
                mem as *mut Memory as *mut skymrp_Memory,
                ticks,
            );
            match res {
                -1 => {
                    assert!(*ticks == 0);
                    CpuState::Normal
                }
                _ if res < -1 => panic!("Unexpected CPU execution result"),
                svc => CpuState::Svc(svc as u32),
            }
        }
    }
}
