mod abi;
mod cpu;
mod gzip;
mod mem;
mod mrp;
mod mythroad;
mod stack;
mod syscall;
mod window;

use std::path::PathBuf;

const USAGE: &str = "\
Usage:
    skymrp path/to/example.mrp

Options:
    --help
        Print this help text.
";

fn main() -> Result<(), String> {
    let mut args = std::env::args();
    let _ = args.next().unwrap(); // skip argv[0]

    let mut mrp_path: Option<PathBuf> = None;
    for arg in args {
        if arg == "--help" {
            println!("{}", USAGE);
            return Ok(());
        } else if mrp_path.is_none() {
            mrp_path = Some(PathBuf::from(arg));
        } else {
            eprintln!("{}", USAGE);
            return Err(format!("Unexpected arguments: {:?}", arg));
        }
    }

    let Some(mrp_path) = mrp_path else {
        eprintln!("{}", USAGE);
        return Err("Path to mrp must be specified".to_string());
    };

    let mut env = Environment::new(mrp_path)?;
    env.run();
    Ok(())
}

/// The struct containing the entire emulator state.
pub struct Environment {
    window: window::Window,
    mem: mem::Memory,
    executable: mrp::Mrp,
    syscall: syscall::Syscall,
    cpu: cpu::Cpu,
}

impl Environment {
    /// Loads the binary and sets up the emulator.
    fn new(mrp_path: PathBuf) -> Result<Environment, String> {
        let window = window::Window::new("skymrp");

        let mut mem = mem::Memory::new();

        let executable = mrp::Mrp::load_from_file(mrp_path, &mut mem)
            .map_err(|e| format!("Could not load MRP file: {}", e))?;

        let entry_point_pc = executable
            .entry_point_pc
            .ok_or_else(|| "MRP file has no cfunction.ext".to_string())?;

        let entry_point_pc = abi::GuestFunction::from_addr_with_thumb_bit(entry_point_pc);
        let lr = abi::GuestFunction::from_addr_with_thumb_bit(0);

        println!("Address of start function: {:?}", entry_point_pc);

        let mut syscall = syscall::Syscall::new();
        syscall.setup_stubs(&executable, &mut mem);
        let mut cpu = cpu::Cpu::new();
        stack::prep_stack_for_start(&mut mem, &mut cpu);

        println!("CPU emulation begins now.");

        cpu.regs_mut()[cpu::Cpu::LR] = 0;
        cpu.set_cpsr(cpu::Cpu::CPSR_USER_MODE);
        cpu.branch_with_link(entry_point_pc, lr);

        Ok(Environment {
            window,
            mem,
            executable,
            syscall,
            cpu,
        })
    }

    /// Run the emulator.
    fn run(&mut self) {
        self.run_inner(true)
    }

    pub fn run_call(&mut self) {
        self.run_inner(false)
    }

    fn run_inner(&mut self, root: bool) {
        let mut events = Vec::new();

        let mut early_exit = false;
        while !early_exit {
            self.window.poll_for_events(&mut events);
            for event in events.drain(..) {
                match event {
                    window::Event::Quit => {
                        println!("User requested quit, exiting...");
                        if root {
                            return;
                        } else {
                            panic!("Quit.");
                        }
                    }
                }
            }

            let mut ticks = 100;

            while ticks > 0 && !early_exit {
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    match self.cpu.run(&mut self.mem, &mut ticks) {
                        cpu::CpuState::Normal => (),
                        cpu::CpuState::Svc(svc) => {
                            // the program counter is pointing at the
                            // instruction after the SVC, but we want the
                            // address of the SVC itself
                            let svc_pc = self.cpu.regs()[cpu::Cpu::PC] - 4;
                            if svc == syscall::Syscall::SVC_RETURN_TO_HOST {
                                assert!(!root);
                                assert!(
                                    svc_pc
                                        == self
                                            .syscall
                                            .return_to_host_routine()
                                            .addr_without_thumb_bit()
                                );
                                early_exit = true;
                                return;
                            }

                            let f = self.syscall.get_svc_handler(
                                &self.executable,
                                &mut self.mem,
                                svc_pc,
                                svc,
                            );
                            f.call_from_guest(self);
                        }
                    }
                }));
                if let Err(e) = res {
                    eprintln!(
                        "Panic at PC {:#x}, LR {:#x}",
                        self.cpu.regs()[cpu::Cpu::PC],
                        self.cpu.regs()[cpu::Cpu::LR]
                    );
                    std::panic::resume_unwind(e);
                }
            }
            if !early_exit {
                println!("{} ticks elapsed", ticks);
            }
        }
    }
}
