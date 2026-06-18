#[macro_use]
mod log;
mod abi;
mod cpu;
mod dsm;
mod encoding;
mod font;
mod fs;
mod gzip;
mod libc;
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
            log_dbg!("{}", USAGE);
            return Ok(());
        } else if mrp_path.is_none() {
            mrp_path = Some(PathBuf::from(arg));
        } else {
            log!("{}", USAGE);
            return Err(format!("Unexpected arguments: {:?}", arg));
        }
    }

    let Some(mrp_path) = mrp_path else {
        log!("{}", USAGE);
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
    fs: fs::Fs,
    font: font::Font,
    executable: mrp::Mrp,
    syscall: syscall::Syscall,
    cpu: cpu::Cpu,
    libc_state: libc::State,
    mythroad: mythroad::Mythroad,
}

impl Environment {
    /// Loads the binary and sets up the emulator.
    fn new(mrp_path: PathBuf) -> Result<Environment, String> {
        let window = window::Window::new("SKYMRP");

        let mut mem = mem::Memory::new();

        let (fs, guest_path) = fs::Fs::new(mrp_path.as_path());
        let executable = mrp::Mrp::load_from_file(guest_path, &fs)
            .map_err(|e| format!("Could not load MRP file: {}", e))?;
        let font = font::Font::new(&mut mem);

        let mut syscall = syscall::Syscall::new();

        let mythroad = mythroad::Mythroad::new(&mut mem);
        syscall.setup_stubs(&executable, &mut mem, &mythroad);
        let mut cpu = cpu::Cpu::new();
        stack::prep_stack_for_start(&mut mem, &mut cpu);
        let libc_state = Default::default();

        log_dbg!("CPU emulation begins now.");

        cpu.set_cpsr(cpu::Cpu::CPSR_USER_MODE);

        Ok(Environment {
            window,
            mem,
            fs,
            font,
            executable,
            syscall,
            cpu,
            libc_state,
            mythroad,
        })
    }

    /// Run the emulator.
    fn run(&mut self) {
        let entry = format!("%{}", self.executable.file_name);
        if dsm::mr_start_dsm_c(self, Some(&entry)) != mythroad::MrResult::Success.to_bits() {
            log!("Mythroad: mr_start_dsmC failed");
            return;
        }

        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.run_inner(true)));
        if let Err(e) = res {
            log!(
                "Panic at PC {:#x}, LR {:#x}",
                self.cpu.regs()[cpu::Cpu::PC],
                self.cpu.regs()[cpu::Cpu::LR]
            );
            std::panic::resume_unwind(e);
        }
    }

    pub fn run_call(&mut self) {
        self.run_inner(false)
    }

    fn run_inner(&mut self, root: bool) {
        let mut events = Vec::new();

        loop {
            self.window.poll_for_events(&mut events);
            for event in events.drain(..) {
                match event {
                    window::Event::Quit => {
                        log_dbg!("User requested quit, exiting...");
                        if root {
                            return;
                        } else {
                            panic!("Quit.");
                        }
                    }
                }
            }

            let mut ticks = 100;

            while ticks > 0 {
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
            }
        }
    }
}
