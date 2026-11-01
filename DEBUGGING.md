# Debugging SkyMRP

See also `BUILD.md`.

## Logging

`src/log.rs` provides two logging macros, `log!()` and `log_dbg!()`. The former always prints a log message, whereas the latter only prints a message if the containing module is listed in `ENABLED_MODULES` in the same file.

## Debugging crashes in host code

The `RUST_BACKTRACE=1` environment variable is always helpful. You'll probably want a debug (not `--release`) build of SkyMRP to get the best output.

## Debugging crashes in guest code

SkyMRP will print the basic registers (r0-r12, SP, LR, PC) and a basic stack trace (using frame pointers) when a panic occurs. To make sense of the result, you will probably want to open the app binary in Ghidra or another reverse-engineering tool.

### GDB Remote Serial Protocol server

For more complex cases, you can use the `--gdb=` command-line argument to start SkyMRP in debugging mode, where it will provide a GDB Remote Serial Protocol server. You can then connect to SkyMRP with GDB. (In theory LLDB also should work, but it doesn't.)

A quick word of warning: this will not be the GDB experience you may be used to when writing C/C++ code and compiling it in debug mode. The GDB support was added to help with debugging apps for which we don't have symbols, let alone DWARF info or source code. GDB when connected to SkyMRP will not know about local variables or even stack frames! You'll need to know instruction addresses and register numbers. As such, having the binary open in a tool like Ghidra while debugging is practically mandatory.

Anyway, you'll need a version of GDB that supports ARMv5TE. On macOS, the Homebrew package for `gdb` is multi-architecture. If you're on Ubuntu, you might need the `gdb-multiarch` package (this hasn't been tested).

The basic set of steps is:

* Start SkyMRP in debugging mode: `skymrp --gdb=localhost:9001 path/to/app.mrp`.
* In a separate terminal window, start GDB. Then, inside GDB, run `target remote localhost:9001` to connect to SkyMRP.

If you prefer for GDB to connect immediately: `gdb -ex 'target remote localhost:9001'`.

When GDB first connects, CPU execution is paused and none of the guest app's code has been run yet. While execution is paused, SkyMRP allows GDB to:

* Read and write registers
* Read and write memory
* Resume execution, either indefinitely or for a single instruction
* Kill the emulated app (this just makes SkyMRP crash)

GDB provides various services on top of this, for example:

* `break *0x1000` sets a breakpoint
* `info registers` shows the content of registers
* `backtrace` shows a backtrace (though SkyMRP's own may be better)
* `print *(float*)0x2000` evaluates a simple C-like expression
* `layout asm` opens a disassembly view
* `kill` will make SkyMRP crash
* `stepi` resumes execution for a single instruction
* `continue` resumes execution indefinitely

Beware that native MRP apps may contain a mix of Thumb functions and normal Arm functions. GDB usually won't know which kind of function it's dealing with:

* When no symbols are available, GDB will assume an address is Arm code by default. You can use `set arm fallback-mode` to change this assumption.
* When full symbols are available, GDB seems to assume symbols are for Arm functions [even when they aren't](https://sourceware.org/bugzilla/show_bug.cgi?id=30386). You can use `set arm force-mode` to override this.

GDB seems to [mostly](https://sourceware.org/bugzilla/show_bug.cgi?id=30385) understand the convention of setting the lower bit of the address to 1 to indicate a Thumb function, and in any case setting an Arm breakpoint in Thumb code (not vice-versa) usually works, so you usually only need to worry about this when disassembling things.

SkyMRP only communicates with GDB while execution is paused. Beyond being paused when you initially connect, it is also paused when certain CPU errors occur, or after stepping (resuming execution for a single instruction). Breakpoints are a useful way to force execution to pause at convenient locations. Another option is to press the F12 key while you have the SkyMRP window in focus, which will make SkyMRP pause during a subsequent host event-loop iteration. If the app fails to return to the host event loop then this won't be useful.
