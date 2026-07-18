/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
/// Opaque type from C
#[allow(non_camel_case_types)]
pub type skymrp_DynarmicWrapper = std::ffi::c_void;
/// Opaque type from Rust
#[allow(non_camel_case_types)]
pub type skymrp_Memory = std::ffi::c_void;

unsafe extern "C" {
    pub unsafe fn skymrp_DynarmicWrapper_new() -> *mut skymrp_DynarmicWrapper;
    pub unsafe fn skymrp_DynarmicWrapper_delete(cpu: *mut skymrp_DynarmicWrapper);
    pub unsafe fn skymrp_DynarmicWrapper_regs_const(
        cpu: *const skymrp_DynarmicWrapper,
    ) -> *const u32;
    pub unsafe fn skymrp_DynarmicWrapper_regs_mut(cpu: *mut skymrp_DynarmicWrapper) -> *mut u32;
    pub unsafe fn skymrp_DynarmicWrapper_cpsr(cpu: *const skymrp_DynarmicWrapper) -> u32;
    pub unsafe fn skymrp_DynarmicWrapper_set_cpsr(cpu: *mut skymrp_DynarmicWrapper, cpsr: u32);
    pub unsafe fn skymrp_DynarmicWrapper_run(
        cpu: *mut skymrp_DynarmicWrapper,
        mem: *mut skymrp_Memory,
        ticks: *mut u64,
    ) -> i32;
}
