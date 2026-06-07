use crate::cpu::Cpu;
use crate::mem::{ConstVoidPtr, Ptr};
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
        println!("Begin call to guest function {:?}", self);

        let (old_pc, old_lr) = env
            .cpu
            .branch_with_link(self, env.syscall.return_to_host_routine());

        env.run_call();

        env.cpu.branch(old_pc);
        env.cpu.regs_mut()[Cpu::LR] = old_lr.addr_with_thumb_bit();

        println!("End call to guest function {:?}", self);
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
                let args: ($($P,)*) = {
                    let regs = env.cpu.regs();
                    let mut reg_offset = 0;
                    ($(read_next_arg::<$P>(&mut reg_offset, regs),)*)
                };
                let retval = self(env, $(args.$p),*);
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
                {
                    let regs = env.cpu.regs_mut();
                    let mut reg_offset = 0;
                    $(write_next_arg::<$P>(&mut reg_offset, regs, args.$p);)*
                };
                self.call_from_guest(env);
                <R as GuestRet>::from_regs(env.cpu.regs())
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
                {
                    let regs = env.cpu.regs_mut();
                    let mut reg_offset = 0;
                    $(write_next_arg::<$P>(&mut reg_offset, regs, args.$p);)*
                };
                self.call(env);
                <R as GuestRet>::from_regs(env.cpu.regs())
            }
        }

    }
}

impl_CallFromHost!();
impl_CallFromHost!(0 => P0);
impl_CallFromHost!(0 => P0, 1 => P1);
impl_CallFromHost!(0 => P0, 1 => P1, 2 => P2);
impl_CallFromHost!(0 => P0, 1 => P1, 2 => P2, 3 => P3);

/// Calling convention translation for a function argument type.
pub trait GuestArg: Sized {
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
fn read_next_arg<T: GuestArg>(reg_offset: &mut usize, regs: &[u32]) -> T {
    // After the fourth register is used, the arguments go on the stack.
    // (Support not implemented yet, Rust will panic if indexing out-of-bounds.)
    let regs = &regs[0..4];

    let val = T::from_regs(&regs[*reg_offset..][..T::REG_COUNT]);
    *reg_offset += T::REG_COUNT;
    val
}

/// Write a single argument to registers. Call this for each argument in order.
fn write_next_arg<T: GuestArg>(reg_offset: &mut usize, regs: &mut [u32], arg: T) {
    // After the fourth register is used, the arguments go on the stack.
    // (Support not implemented yet, Rust will panic if indexing out-of-bounds.)
    let regs = &mut regs[0..4];

    arg.to_regs(&mut regs[*reg_offset..][..T::REG_COUNT]);
    *reg_offset += T::REG_COUNT;
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

pub trait GuestRet: Sized {
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
