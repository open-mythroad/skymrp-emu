/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use crate::abi::GuestFunction;
use crate::mem::{
    ConstPtr, GuestUSize, Memory, MutPtr, Ptr, SafeRead, SafeWrite, LINEAR_MEMORY_SIZE, PAGE_SIZE,
};

// Import functions from C++
use skymrp_dynarmic_wrapper::*;

type VAddr = u32;

fn skymrp_cpu_read_impl<T: SafeRead + Default>(
    mem: *mut skymrp_Memory,
    addr: VAddr,
    error: *mut bool,
) -> T {
    // TODO: Disable this in debug mode?
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mem = unsafe { &mut *mem.cast::<Memory>() };
        let ptr: ConstPtr<T> = Ptr::from_bits(addr);
        mem.read(ptr)
    }));
    unsafe {
        error.write(res.is_err());
    }
    res.unwrap_or_default()
}

fn skymrp_cpu_write_impl<T: SafeWrite>(mem: *mut skymrp_Memory, addr: VAddr, value: T) -> bool {
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mem = unsafe { &mut *mem.cast::<Memory>() };
        let ptr: MutPtr<T> = Ptr::from_bits(addr);
        mem.write(ptr, value)
    }));
    res.is_err()
}

// Export functions for use by C++
#[no_mangle]
extern "C" fn skymrp_cpu_read_u8(mem: *mut skymrp_Memory, addr: VAddr, error: *mut bool) -> u8 {
    skymrp_cpu_read_impl(mem, addr, error)
}
#[no_mangle]
extern "C" fn skymrp_cpu_read_u16(mem: *mut skymrp_Memory, addr: VAddr, error: *mut bool) -> u16 {
    skymrp_cpu_read_impl(mem, addr, error)
}
#[no_mangle]
extern "C" fn skymrp_cpu_read_u32(mem: *mut skymrp_Memory, addr: VAddr, error: *mut bool) -> u32 {
    skymrp_cpu_read_impl(mem, addr, error)
}
#[no_mangle]
extern "C" fn skymrp_cpu_read_u64(mem: *mut skymrp_Memory, addr: VAddr, error: *mut bool) -> u64 {
    skymrp_cpu_read_impl(mem, addr, error)
}
#[no_mangle]
extern "C" fn skymrp_cpu_write_u8(mem: *mut skymrp_Memory, addr: VAddr, value: u8) -> bool {
    skymrp_cpu_write_impl(mem, addr, value)
}
#[no_mangle]
extern "C" fn skymrp_cpu_write_u16(mem: *mut skymrp_Memory, addr: VAddr, value: u16) -> bool {
    skymrp_cpu_write_impl(mem, addr, value)
}
#[no_mangle]
extern "C" fn skymrp_cpu_write_u32(mem: *mut skymrp_Memory, addr: VAddr, value: u32) -> bool {
    skymrp_cpu_write_impl(mem, addr, value)
}
#[no_mangle]
extern "C" fn skymrp_cpu_write_u64(mem: *mut skymrp_Memory, addr: VAddr, value: u64) -> bool {
    skymrp_cpu_write_impl(mem, addr, value)
}

pub struct Cpu {
    dynarmic_wrapper: *mut skymrp_DynarmicWrapper,
    executable_pages: Vec<bool>,
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
    /// When this bit is set in CPSR, the CPU is in Thumb mode.
    pub const CPSR_THUMB: u32 = 0x00000020;

    /// When this bit is set in CPSR, the CPU is in user mode.
    pub const CPSR_USER_MODE: u32 = 0x00000010;

    pub fn new() -> Cpu {
        let dynarmic_wrapper = unsafe { skymrp_DynarmicWrapper_new() };
        Cpu {
            dynarmic_wrapper,
            executable_pages: vec![false; (LINEAR_MEMORY_SIZE / PAGE_SIZE) as usize],
        }
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

    pub fn cpsr(&self) -> u32 {
        unsafe { skymrp_DynarmicWrapper_cpsr(self.dynarmic_wrapper) }
    }
    pub fn set_cpsr(&mut self, cpsr: u32) {
        unsafe { skymrp_DynarmicWrapper_set_cpsr(self.dynarmic_wrapper, cpsr) }
    }

    pub fn clear_cache(&mut self) {
        unsafe { skymrp_DynarmicWrapper_clear_cache(self.dynarmic_wrapper) }
    }

    fn invalidate_cache_range_inner(&mut self, start_address: u32, length: usize) {
        unsafe {
            skymrp_DynarmicWrapper_invalidate_cache_range(
                self.dynarmic_wrapper,
                start_address,
                length,
            )
        }
    }

    pub fn invalidate_cache_range(&mut self, start_address: VAddr, length: usize) {
        if length != 0 && start_address < LINEAR_MEMORY_SIZE {
            let size = length.min(u32::MAX as usize) as u32;
            let end = start_address.saturating_add(size).min(LINEAR_MEMORY_SIZE);
            let first_page = (start_address / PAGE_SIZE) as usize;
            let last_page = ((end - 1) / PAGE_SIZE) as usize;
            self.executable_pages[first_page..=last_page].fill(true);
        }

        self.invalidate_cache_range_inner(start_address, length);
    }

    /// Notify the CPU backend after host code writes guest memory.
    /// JIT backends invalidate translated code; interpreter backends may ignore this.
    pub fn notify_memory_write(&mut self, start: VAddr, size: GuestUSize) {
        if size == 0 || start >= LINEAR_MEMORY_SIZE {
            return;
        }

        let end = start.saturating_add(size).min(LINEAR_MEMORY_SIZE);
        let first_page = (start / PAGE_SIZE) as usize;
        let last_page = ((end - 1) / PAGE_SIZE) as usize;
        if self.executable_pages[first_page..=last_page]
            .iter()
            .any(|executable| *executable)
        {
            let page_start = first_page as VAddr * PAGE_SIZE;
            let page_end = (last_page as VAddr + 1) * PAGE_SIZE;
            self.invalidate_cache_range_inner(page_start, (page_end - page_start) as usize);
        }
    }

    /// Get PC with the Thumb bit appropriately set.
    pub fn pc_with_thumb_bit(&self) -> GuestFunction {
        let pc = self.regs()[Self::PC];
        let thumb = (self.cpsr() & Self::CPSR_THUMB) == Self::CPSR_THUMB;
        GuestFunction::from_addr_and_thumb_flag(pc, thumb)
    }

    /// Set PC and the Thumb flag for executing a guest function. Note that this
    /// does not touch LR.
    pub fn branch(&mut self, new_pc: GuestFunction) {
        self.regs_mut()[Self::PC] = new_pc.addr_without_thumb_bit();
        let cpsr_without_thumb = self.cpsr() & (!Self::CPSR_THUMB);
        self.set_cpsr(cpsr_without_thumb | ((new_pc.is_thumb() as u32) * Self::CPSR_THUMB))
    }

    /// Set the PC and Thumb flag (like [Self::branch]), but also set the LR,
    /// and return the original PC and LR.
    pub fn branch_with_link(
        &mut self,
        new_pc: GuestFunction,
        new_lr: GuestFunction,
    ) -> (GuestFunction, GuestFunction) {
        let old_pc = self.pc_with_thumb_bit();
        let old_lr = GuestFunction::from_addr_with_thumb_bit(self.regs()[Self::LR]);
        self.branch(new_pc);
        self.regs_mut()[Self::LR] = new_lr.addr_with_thumb_bit();
        (old_pc, old_lr)
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
                -2 => {
                    panic!("Memory error during CPU execution!");
                }
                _ if res < -1 => panic!("Unexpected CPU execution result"),
                svc => CpuState::Svc(svc as u32),
            }
        }
    }
}
