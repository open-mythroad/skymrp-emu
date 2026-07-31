/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use crate::cpu::Cpu;
use crate::mem::{ConstPtr, ConstVoidPtr, Memory, MutPtr, Ptr};
use crate::Environment;

#[derive(Copy, Clone, Debug)]
pub struct GuestFunction(ConstVoidPtr);

impl GuestFunction {
    pub const THUMB_BIT: u32 = 0x1;

    pub fn from_addr_with_thumb_bit(addr: u32) -> Self {
        GuestFunction(Ptr::from_bits(addr))
    }

    pub fn from_addr_and_thumb_flag(pc: u32, thumb: bool) -> Self {
        GuestFunction(Ptr::from_bits(pc | ((thumb as u32) * Self::THUMB_BIT)))
    }

    pub fn is_thumb(self) -> bool {
        self.0.to_bits() & Self::THUMB_BIT == Self::THUMB_BIT
    }

    pub fn addr_with_thumb_bit(self) -> u32 {
        self.0.to_bits()
    }

    pub fn addr_without_thumb_bit(self) -> u32 {
        self.0.to_bits() & !Self::THUMB_BIT
    }

    pub fn call(self, env: &mut Environment) {
        log_dbg!("Begin call to guest function {:?}", self);

        let (old_pc, old_lr) = env
            .cpu
            .branch_with_link(self, env.syscall.return_to_host_routine());

        env.run_call();

        env.cpu.branch(old_pc);
        env.cpu.regs_mut()[Cpu::LR] = old_lr.addr_with_thumb_bit();

        log_dbg!("End call to guest function {:?}", self);
    }
}

pub trait CallFromGuest {
    fn call_from_guest(&self, env: &mut Environment);
}

macro_rules! impl_CallFromGuest {
    ( $($p:tt => $P:ident),* ) => {
        impl<R, $($P),*> CallFromGuest for fn(&mut Environment, $($P),*) -> R
            where R: GuestRet, $($P: GuestArg,)* {
            // ignore warnings for the zero-argument case
            #[allow(unused_variables, unused_mut, clippy::unused_unit)]
            fn call_from_guest(&self, env: &mut Environment) {
                let regs = env.cpu.regs();
                let mut reg_offset = 0;
                let args: ($($P,)*) = {
                    ($(read_next_arg::<$P>(&mut reg_offset, regs, Ptr::from_bits(regs[Cpu::SP]), &env.mem),)*)
                };
                log_dbg!("CallFromGuest {:?}", args);
                let retval = self(env, $(args.$p),*);
                log_dbg!("CallFromGuest => {:?}", retval);
                retval.to_regs(env.cpu.regs_mut());
            }
        }

        impl<R, $($P),*> CallFromGuest for fn(&mut Environment, $($P,)* DotDotDot) -> R
            where R: GuestRet, $($P: GuestArg,)* {
            // ignore warnings for the zero-argument case
            #[allow(unused_variables, unused_mut, clippy::unused_unit)]
            fn call_from_guest(&self, env: &mut Environment) {
                let mut reg_offset = 0;
                let regs = env.cpu.regs();
                let args: ($($P,)*) = {
                    ($(read_next_arg::<$P>(&mut reg_offset, regs, Ptr::from_bits(regs[Cpu::SP]), &env.mem),)*)
                };
                let va_list = DotDotDot(VaList { reg_offset, stack_pointer: Ptr::from_bits(regs[Cpu::SP]) });
                log_dbg!("CallFromGuest {:?}, ...{:?}", args, va_list);
                let retval = self(env, $(args.$p,)* va_list);
                log_dbg!("CallFromGuest => {:?}", retval);
                retval.to_regs(env.cpu.regs_mut());
            }
        }
    }
}

impl_CallFromGuest!();
impl_CallFromGuest!(0 => P0);
impl_CallFromGuest!(0 => P0, 1 => P1);
impl_CallFromGuest!(0 => P0, 1 => P1, 2 => P2);
impl_CallFromGuest!(0 => P0, 1 => P1, 2 => P2, 3 => P3);
impl_CallFromGuest!(0 => P0, 1 => P1, 2 => P2, 3 => P3, 4 => P4);
impl_CallFromGuest!(0 => P0, 1 => P1, 2 => P2, 3 => P3, 4 => P4, 5 => P5);
impl_CallFromGuest!(0 => P0, 1 => P1, 2 => P2, 3 => P3, 4 => P4, 5 => P5, 6 => P6);
impl_CallFromGuest!(0 => P0, 1 => P1, 2 => P2, 3 => P3, 4 => P4, 5 => P5, 6 => P6, 7 => P7);
impl_CallFromGuest!(0 => P0, 1 => P1, 2 => P2, 3 => P3, 4 => P4, 5 => P5, 6 => P6, 7 => P7, 8 => P8);
impl_CallFromGuest!(0 => P0, 1 => P1, 2 => P2, 3 => P3, 4 => P4, 5 => P5, 6 => P6, 7 => P7, 8 => P8, 9 => P9);

pub trait CallFromHost<R, P> {
    fn call_from_host(&self, env: &mut Environment, args: P) -> R;
}

macro_rules! impl_CallFromHost {
    ( $($p:tt => $P:ident),* ) => {
        impl <T, R, $($P),*> CallFromHost<R, ($($P,)*)> for T
            where T: CallFromGuest, R: GuestRet, $($P: GuestArg,)* {
            // ignore warnings for the zero-argument case
            #[allow(unused_variables, unused_mut, clippy::unused_unit)]
            fn call_from_host(
                &self,
                env: &mut Environment,
                args: ($($P,)*),
            ) -> R {
                let mut reg_offset = 0;
                let regs = env.cpu.regs_mut();
                let old_sp = extend_stack_for_args(
                    0 $(+ <$P as GuestArg>::REG_COUNT)*,
                    regs,
                );
                $(write_next_arg::<$P>(&mut reg_offset, regs, &mut env.mem, args.$p);)*
                self.call_from_guest(env);
                let regs = env.cpu.regs_mut();
                regs[Cpu::SP] = old_sp;
                <R as GuestRet>::from_regs(regs)
            }
        }

        impl <R, $($P),*> CallFromHost<R, ($($P,)*)> for GuestFunction
            where R: GuestRet, $($P: GuestArg,)* {
            // ignore warnings for the zero-argument case
            #[allow(unused_variables, unused_mut, clippy::unused_unit)]
            fn call_from_host(
                &self,
                env: &mut Environment,
                args: ($($P,)*),
            ) -> R {
                log_dbg!("Begin call to guest function {:?}", self);
                let mut reg_offset = 0;
                let regs = env.cpu.regs_mut();
                let old_sp = extend_stack_for_args(
                    0 $(+ <$P as GuestArg>::REG_COUNT)*,
                    regs,
                );
                $(write_next_arg::<$P>(&mut reg_offset, regs, &mut env.mem, args.$p);)*
                self.call(env);
                let regs = env.cpu.regs_mut();
                log_dbg!("End call to guest function {:?}", self);
                regs[Cpu::SP] = old_sp;
                <R as GuestRet>::from_regs(regs)
            }
        }

    }
}

impl_CallFromHost!();
impl_CallFromHost!(0 => P0);
impl_CallFromHost!(0 => P0, 1 => P1);
impl_CallFromHost!(0 => P0, 1 => P1, 2 => P2);
impl_CallFromHost!(0 => P0, 1 => P1, 2 => P2, 3 => P3);
impl_CallFromHost!(0 => P0, 1 => P1, 2 => P2, 3 => P3, 4 => P4);
impl_CallFromHost!(0 => P0, 1 => P1, 2 => P2, 3 => P3, 4 => P4, 5 => P5);

/// Calling convention translation for a function argument type.
pub trait GuestArg: std::fmt::Debug + Sized {
    /// How many registers does this argument type consume?
    const REG_COUNT: usize;

    /// Read the argument from registers. Only `&regs[0..Self::REG_COUNT]` may
    /// be accessed.
    fn from_regs(regs: &[u32]) -> Self;

    /// Write the argument to registers. Only '&mut regs[0..Self::REG_COUNT]`
    /// may be accessed.
    fn to_regs(self, regs: &mut [u32]);
}

/// Read a single argument from registers. Call this for each argument in order.
fn read_next_arg<T: GuestArg>(
    reg_offset: &mut usize,
    regs: &[u32],
    stack_ptr: ConstPtr<u32>,
    mem: &Memory,
) -> T {
    // After the fourth register is used, the arguments go on the stack.
    // In some cases the argument is split over both registers and the stack.

    let mut fake_regs = [0u32; 4]; // Rust doesn't allow [0u32; Trait::T] alas.
    let fake_regs = &mut fake_regs[0..T::REG_COUNT];

    for fake_reg in fake_regs.iter_mut() {
        if *reg_offset < 4 {
            *fake_reg = regs[*reg_offset];
        } else {
            *fake_reg = mem.read(stack_ptr + (*reg_offset - 4).try_into().unwrap());
        }
        *reg_offset += 1;
    }

    T::from_regs(fake_regs)
}

/// Decrements the stack pointer to prepare for calling [write_next_arg]. Pass
/// the sum of the [GuestArg::REG_COUNT]s for all the arguments to be written,
/// and this will update the stack pointer if necessary, as well as returning
/// a copy of the original stack pointer so it can be restored later.
pub fn extend_stack_for_args(reg_count_sum: usize, regs: &mut [u32]) -> u32 {
    // After the fourth register is used, the arguments go on the stack.
    // In some cases the argument is split over both registers and the stack.

    let old = regs[Cpu::SP];
    if reg_count_sum > 4 {
        let old: ConstPtr<u32> = Ptr::from_bits(old);
        regs[Cpu::SP] = (old - (reg_count_sum - 4).try_into().unwrap()).to_bits()
    }
    old
}

/// Write a single argument to registers or the stack. Call this for each
/// argument in order.
///
/// If `reg_offset` is or will be >= 4, the stack pointer **must** be
/// appropriately decremented in advance! See [extend_stack_for_args].
pub fn write_next_arg<T: GuestArg>(
    reg_offset: &mut usize,
    regs: &mut [u32],
    mem: &mut Memory,
    arg: T,
) {
    // After the fourth register is used, the arguments go on the stack.
    // In some cases the argument is split over both registers and the stack.
    let mut fake_regs = [0u32; 4]; // Rust doesn't allow [0u32; Trait::T] alas.
    let fake_regs = &mut fake_regs[0..T::REG_COUNT];
    arg.to_regs(fake_regs);

    for &mut fake_reg in fake_regs {
        if *reg_offset < 4 {
            regs[*reg_offset] = fake_reg;
        } else {
            let stack_ptr: MutPtr<u32> = Ptr::from_bits(regs[Cpu::SP]);
            mem.write(stack_ptr + (*reg_offset - 4).try_into().unwrap(), fake_reg);
        }
        *reg_offset += 1;
    }
}

#[derive(Debug)]
pub struct DotDotDot(VaList);
impl DotDotDot {
    pub fn start(&self) -> VaList {
        self.0
    }
}

/// Calling convention translation for a variable arguments list (like C
/// `va_list`).
#[derive(Copy, Clone, Debug)]
pub struct VaList {
    reg_offset: usize,
    stack_pointer: ConstVoidPtr,
}
impl VaList {
    /// Get the next argument, like C's `va_arg()`.
    pub fn next<T: GuestArg>(&mut self, env: &mut Environment) -> T {
        let sp_reg = self.stack_pointer.cast();
        read_next_arg(&mut self.reg_offset, env.cpu.regs_mut(), sp_reg, &env.mem)
    }
}

macro_rules! impl_GuestArg_with {
    ($for:ty, $with:ty) => {
        impl GuestArg for $for {
            const REG_COUNT: usize = <$with as GuestArg>::REG_COUNT;
            fn from_regs(regs: &[u32]) -> Self {
                <$with as GuestArg>::from_regs(regs) as $for
            }

            fn to_regs(self, regs: &mut [u32]) {
                <u32 as GuestArg>::to_regs(self as $with, regs)
            }
        }
    };
}

// GuestArg implementations for u32-like types

impl GuestArg for u32 {
    const REG_COUNT: usize = 1;
    fn from_regs(regs: &[u32]) -> Self {
        regs[0]
    }

    fn to_regs(self, regs: &mut [u32]) {
        regs[0] = self;
    }
}

impl_GuestArg_with!(i32, u32);
impl_GuestArg_with!(u16, u32);
impl_GuestArg_with!(i16, u32);
impl_GuestArg_with!(u8, u32);
impl_GuestArg_with!(i8, u32);

impl GuestArg for f32 {
    const REG_COUNT: usize = <u32 as GuestArg>::REG_COUNT;
    fn from_regs(regs: &[u32]) -> Self {
        Self::from_bits(<u32 as GuestArg>::from_regs(regs))
    }

    fn to_regs(self, regs: &mut [u32]) {
        <u32 as GuestArg>::to_regs(self.to_bits(), regs)
    }
}

impl<T, const MUT: bool> GuestArg for Ptr<T, MUT> {
    const REG_COUNT: usize = <u32 as GuestArg>::REG_COUNT;
    fn from_regs(regs: &[u32]) -> Self {
        Self::from_bits(<u32 as GuestArg>::from_regs(regs))
    }

    fn to_regs(self, regs: &mut [u32]) {
        <u32 as GuestArg>::to_regs(self.to_bits(), regs)
    }
}

impl GuestArg for GuestFunction {
    const REG_COUNT: usize = <ConstVoidPtr as GuestArg>::REG_COUNT;
    fn from_regs(regs: &[u32]) -> Self {
        GuestFunction(<ConstVoidPtr as GuestArg>::from_regs(regs))
    }
    fn to_regs(self, regs: &mut [u32]) {
        <ConstVoidPtr as GuestArg>::to_regs(self.0, regs)
    }
}

impl GuestArg for VaList {
    const REG_COUNT: usize = <ConstVoidPtr as GuestArg>::REG_COUNT;
    fn from_regs(regs: &[u32]) -> Self {
        // `reg_offset` initialized to 4 as we want to use `stack_pointer` when calling [read_next_arg]
        VaList {
            reg_offset: 4,
            stack_pointer: <ConstVoidPtr as GuestArg>::from_regs(regs),
        }
    }
    fn to_regs(self, _regs: &mut [u32]) {
        todo!()
    }
}

pub trait GuestRet: std::fmt::Debug + Sized {
    /// Read the return value from registers.
    fn from_regs(regs: &[u32]) -> Self;

    /// Write the return value to registers.
    fn to_regs(self, regs: &mut [u32]);
}

macro_rules! impl_GuestRet_with {
    ($for:ty, $with:ty) => {
        impl GuestRet for $for {
            fn from_regs(regs: &[u32]) -> Self {
                <$with as GuestRet>::from_regs(regs) as $for
            }
            fn to_regs(self, regs: &mut [u32]) {
                <$with as GuestRet>::to_regs(self as $with, regs)
            }
        }
    };
}

impl GuestRet for () {
    fn to_regs(self, _regs: &mut [u32]) {}
    fn from_regs(_regs: &[u32]) -> Self {}
}

// GuestRet implementations for u32-like types
impl GuestRet for u32 {
    fn from_regs(regs: &[u32]) -> Self {
        regs[0]
    }
    fn to_regs(self, regs: &mut [u32]) {
        regs[0] = self;
    }
}

impl_GuestRet_with!(i32, u32);
impl_GuestRet_with!(u16, u32);
impl_GuestRet_with!(i16, u32);
impl_GuestRet_with!(u8, u32);
impl_GuestRet_with!(i8, u32);

impl GuestRet for f32 {
    fn from_regs(regs: &[u32]) -> Self {
        Self::from_bits(<u32 as GuestRet>::from_regs(regs))
    }

    fn to_regs(self, regs: &mut [u32]) {
        <u32 as GuestRet>::to_regs(self.to_bits(), regs)
    }
}

impl<T, const MUT: bool> GuestRet for Ptr<T, MUT> {
    fn from_regs(regs: &[u32]) -> Self {
        Self::from_bits(<u32 as GuestRet>::from_regs(regs))
    }
    fn to_regs(self, regs: &mut [u32]) {
        <u32 as GuestRet>::to_regs(self.to_bits(), regs)
    }
}
