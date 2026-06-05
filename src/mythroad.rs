use crate::cpu::Cpu;
use crate::syscall::{Export, FunctionExports};
use crate::Environment;

fn mr_malloc(env: &mut Environment, len: u32) {
    unimplemented!(
        "mr_malloc({:#x}...) called from {:#x}",
        len,
        env.cpu.regs()[Cpu::PC]
    );
}

pub const MR_C_FUNCTION_TABLE: FunctionExports =
    &[Export::Func(&(mr_malloc as fn(&mut Environment, u32)))];
