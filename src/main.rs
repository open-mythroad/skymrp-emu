mod cpu;
mod gzip;
mod mrp;
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

    let mrp = mrp::Mrp::load_from_file(mrp_path)
        .map_err(|e| format!("Could not load MRP file: {}", e))?;

    let header = mrp.header();
    let entries = mrp.entries();

    println!("Header of MRP file: {:#?}", header);
    println!("Entries of MRP file: {:#?}", entries);

    let start_mr: Vec<u8> = mrp
        .read_file("start.mr")
        .map_err(|e| format!("Could not read start.mr in MRP file: {}", e))?;

    let cfuntion_ext: Vec<u8> = mrp
        .read_file("cfunction.ext")
        .map_err(|e| format!("Could not read cfunction.ext in MRP file: {}", e))?;

    println!("start.mr size: {}", start_mr.len());
    println!("cfunction.ext size: {}", cfuntion_ext.len());

    let mut window = window::Window::new("skymrp");
    let mut events = Vec::new();
    let _cpu = cpu::Cpu::new();

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
