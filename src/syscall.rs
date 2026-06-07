use crate::abi::{CallFromGuest, GuestFunction};
use crate::mem::{GuestUSize, Memory, MutPtr, Ptr};
use crate::mrp::Mrp;

type HostFunction = &'static dyn CallFromGuest;

#[derive(Clone, Copy)]
pub enum Export {
    Func(HostFunction),
    Data(GuestUSize),
    Reserved,
}

pub type FunctionExports = &'static [Export];

const FUNCTION_TABLE: FunctionExports = crate::mythroad::MR_C_FUNCTION_TABLE;

pub struct Syscall {
    host_functions: Vec<HostFunction>,
    return_to_host_routine: Option<GuestFunction>,
}

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
    pub const SVC_RETURN_TO_HOST: u32 = 0;
    const SVC_HOST_FUNCTIONS_BASE: u32 = Self::SVC_RETURN_TO_HOST + 1;

    pub fn new() -> Syscall {
        Syscall {
            host_functions: Vec::new(),
            return_to_host_routine: None,
        }
    }

    pub fn return_to_host_routine(&self) -> GuestFunction {
        self.return_to_host_routine.unwrap()
    }

    pub fn setup_stubs(&mut self, bin: &Mrp, mem: &mut Memory) {
        assert!(self.return_to_host_routine.is_none());
        self.return_to_host_routine = {
            let routine = [encode_a32_svc(Self::SVC_RETURN_TO_HOST), encode_a32_trap()];
            let ptr: MutPtr<u32> = mem.alloc(4 * 2).cast();
            mem.write(ptr + 0, routine[0]);
            mem.write(ptr + 1, routine[1]);
            let ptr = GuestFunction::from_addr_with_thumb_bit(ptr.to_bits());
            assert!(!ptr.is_thumb());
            Some(ptr)
        };

        let entry_point_pc = bin.entry_point_pc.unwrap();

        let export_count = FUNCTION_TABLE.len() as u32;
        let func_count = FUNCTION_TABLE
            .iter()
            .filter(|export| matches!(export, Export::Func(_)))
            .count() as u32;
        let table_size = export_count * 4;
        let stub_size = func_count * 8;
        mem.reserve(bin.mr_c_function_table_addr, table_size + stub_size);

        // Store the mr_c_function_table address in the entry header.
        mem.write(
            Ptr::from_bits(entry_point_pc - 8),
            bin.mr_c_function_table_addr,
        );

        let table_base_ptr: MutPtr<u32> = Ptr::from_bits(bin.mr_c_function_table_addr);
        let stub_base_ptr: MutPtr<u32> = table_base_ptr + export_count;

        let mut stub_index = 0u32;

        for (export_index, export) in FUNCTION_TABLE.iter().copied().enumerate() {
            let export_index = export_index as u32;
            let table_entry_ptr = table_base_ptr + export_index;

            match export {
                Export::Func(function) => {
                    let svc = self.host_functions.len() as u32 + Self::SVC_HOST_FUNCTIONS_BASE;
                    self.host_functions.push(function);

                    let stub_ptr = stub_base_ptr + stub_index * 2;
                    mem.write(table_entry_ptr, stub_ptr.to_bits());

                    mem.write(stub_ptr, encode_a32_svc(svc));
                    mem.write(stub_ptr + 1, encode_a32_ret());

                    stub_index += 1;
                }

                Export::Data(value) => {
                    mem.write(table_entry_ptr, value);
                }

                Export::Reserved => {
                    mem.write(table_entry_ptr, 0);
                }
            }
        }
    }

    /// Return a host function that can be called to handle an SVC instruction
    /// encountered during CPU emulation.
    pub fn get_svc_handler(
        &mut self,
        bin: &Mrp,
        mem: &mut Memory,
        svc_pc: u32,
        svc: u32,
    ) -> HostFunction {
        match svc {
            Self::SVC_RETURN_TO_HOST => unreachable!(),
            Self::SVC_HOST_FUNCTIONS_BASE.. => {
                let f = self
                    .host_functions
                    .get((svc - Self::SVC_HOST_FUNCTIONS_BASE) as usize);
                let Some(&f) = f else {
                    panic!("Unexpected SVC #{} at {:#x}", svc, svc_pc);
                };
                f
            }
        }
    }
}
