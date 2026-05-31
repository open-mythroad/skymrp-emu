mod cpu;
mod gzip;
mod mem;
mod mrp;
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

    let mut window = window::Window::new("skymrp");
    let mut events = Vec::new();

    let mut mem = mem::Memory::new();

    let mrp = mrp::Mrp::load_from_file(mrp_path, &mut mem)
        .map_err(|e| format!("Could not load MRP file: {}", e))?;

    let entry_point_pc = mrp
        .entry_point_pc
        .ok_or_else(|| "MRP file has no cfunction.ext".to_string())?;

    println!("Address of start function: {:#x}", entry_point_pc);

    let stub_addr: u32 = 0x00000011;

    let mut syscall = syscall::Syscall::new();
    syscall.setup_stubs(stub_addr, &mut mem);
    let mut cpu = cpu::Cpu::new();
    stack::prep_stack_for_start(&mut mem, &mut cpu);

    println!("CPU emulation begins now.");

    cpu.regs_mut()[cpu::Cpu::LR] = 0;
    cpu.regs_mut()[cpu::Cpu::PC] = stub_addr;

    loop {
        window.poll_for_events(&mut events);
        for event in events.drain(..) {
            match event {
                window::Event::Quit => {
                    println!("User requested quit, exiting...");
                    return Ok(());
                }
            }
        }

        let mut ticks = 100;
        while ticks > 0 {
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                match cpu.run(&mut mem, &mut ticks) {
                    cpu::CpuState::Normal => (),
                    cpu::CpuState::Svc(svc) => {
                        // the program counter is one instruction ahead
                        let current_instruction = cpu.regs()[cpu::Cpu::PC] - 4;
                        syscall.handle_svc(&mrp, current_instruction, svc)
                    }
                }
            }));
            if let Err(e) = res {
                eprintln!(
                    "Panic at PC {:#x}, LR {:#x}",
                    cpu.regs()[cpu::Cpu::PC],
                    cpu.regs()[cpu::Cpu::LR]
                );
                std::panic::resume_unwind(e);
            }
        }
        println!("{} ticks elapsed", ticks);
    }
}
