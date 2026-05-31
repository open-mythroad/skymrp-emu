mod cpu;
mod gzip;
mod mem;
mod mrp;
mod stack;
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

    let cfuntion_ext = mrp::read_file(&mrp.data, "cfunction.ext")?;
    println!("cfuntion.ext size: {}", cfuntion_ext.len());

    let mut cpu = cpu::Cpu::new();
    stack::prep_stack_for_start(&mut mem, &mut cpu);

    mem.write(mem::Ptr::from_bits(0), 0xE0800001u32); // A32: add r0, r0, r1
    mem.write(mem::Ptr::from_bits(4), 0xEF000001u32); // A32: svc 0
    let a = 1;
    let b = 2;
    cpu.regs_mut()[0] = a;
    cpu.regs_mut()[1] = b;
    cpu.regs_mut()[cpu::Cpu::PC] = 0;
    cpu.run(&mut mem);
    let res = cpu.regs()[0];
    println!("According to dynarmic, {} + {} = {}!", a, b, res);

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
    }
}
