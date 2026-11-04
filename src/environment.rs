/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use crate::window::Event;
use crate::{
    audio, cpu, dsm, editbox, fs, gdb, haptics, libc, mem, mrp, mythroad, options, stack, syscall,
    window,
};
use crossbeam_utils::atomic::AtomicCell;
use std::net::TcpListener;
use std::time::{Duration, Instant};

use std::path::PathBuf;

static SPEED_PERCENT: AtomicCell<Option<u32>> = AtomicCell::new(None);
static SHOW_FPS: AtomicCell<Option<bool>> = AtomicCell::new(None);

enum Command {
    SetSpeedPercent(u32),
    SetShowFps(bool),
}

pub(crate) fn set_speed_percent(percent: u32) {
    SPEED_PERCENT.store(Some(percent));
}

pub(crate) fn set_show_fps(show: bool) {
    SHOW_FPS.store(Some(show));
}

fn take_commands() -> impl Iterator<Item = Command> {
    [
        SPEED_PERCENT.swap(None).map(Command::SetSpeedPercent),
        SHOW_FPS.swap(None).map(Command::SetShowFps),
    ]
    .into_iter()
    .flatten()
}

#[cfg(target_os = "emscripten")]
extern "C" {
    fn emscripten_set_main_loop_arg(
        func: extern "C" fn(*mut std::ffi::c_void),
        arg: *mut std::ffi::c_void,
        fps: std::ffi::c_int,
        simulate_infinite_loop: bool,
    );
}

/// The struct containing the entire emulator state.
pub struct Environment {
    pub options: options::Options,
    pub window: window::Window,
    pub mem: mem::Memory,
    pub fs: fs::Fs,
    pub initial_pack_filename: String,
    pub package_cache: Option<mrp::PackageCache>,
    pub syscall: syscall::Syscall,
    pub cpu: cpu::Cpu,
    pub libc_state: libc::State,
    pub mythroad: mythroad::Mythroad,
    clock: EmulationClock,
    gdb_server: Option<gdb::GdbServer>,
}

struct EmulationClock {
    host_anchor: Instant,
    guest_anchor: Duration,
    speed: f64,
}

impl EmulationClock {
    fn new(speed: f64) -> Self {
        Self {
            host_anchor: Instant::now(),
            guest_anchor: Duration::ZERO,
            speed,
        }
    }

    fn elapsed(&self) -> Duration {
        self.elapsed_at(Instant::now())
    }

    fn elapsed_at(&self, now: Instant) -> Duration {
        self.guest_anchor
            + Duration::from_secs_f64(
                now.duration_since(self.host_anchor).as_secs_f64() * self.speed,
            )
    }

    fn set_speed(&mut self, speed: f64) {
        self.set_speed_at(speed, Instant::now());
    }

    fn set_speed_at(&mut self, speed: f64, now: Instant) {
        self.guest_anchor = self.elapsed_at(now);
        self.host_anchor = now;
        self.speed = speed;
    }
}

impl Environment {
    /// Loads the binary and sets up the emulator.
    pub fn new(mrp_path: PathBuf, options: options::Options) -> Result<Environment, String> {
        let mut mem = mem::Memory::new();
        mem.reserve(0, mem::Memory::NULL_PAGE_SIZE);

        let (fs, guest_path) = fs::Fs::new(mrp_path.as_path());
        let initial_pack_filename = guest_path
            .as_str()
            .strip_prefix(fs::MYTHROAD.as_str())
            .and_then(|path| path.strip_prefix('/'))
            .ok_or_else(|| "MRP path is outside the Mythroad directory".to_owned())?
            .to_owned();
        let initial_mrp_data = fs
            .read(&guest_path)
            .map_err(|_| "Could not load MRP file".to_owned())?;
        let package_cache = mrp::PackageCache::new(guest_path, initial_mrp_data)
            .map_err(|e| format!("Could not load MRP file: {}", e))?;

        let mut syscall = syscall::Syscall::new();

        let mut mythroad = mythroad::Mythroad::new(&mut mem, options.screen_size);
        syscall.setup_stubs(&mut mem, &mythroad);
        let mut cpu = cpu::Cpu::new(options.cpu_backend);
        stack::prep_stack_for_start(&mut mem, &mut cpu);

        let hud = crate::hud::Hud::new(&mut mythroad.font, &fs, options.show_fps, options.speed);
        let window = window::Window::new("SkyMRP", &options, hud);
        audio::init(window.sdl_context()).map_err(|e| e.to_string())?;
        haptics::init(window.sdl_context()).map_err(|e| e.to_string())?;

        let clock = EmulationClock::new(options.speed);
        let mut env = Environment {
            options,
            window,
            mem,
            fs,
            initial_pack_filename,
            package_cache: Some(package_cache),
            syscall,
            cpu,
            libc_state: Default::default(),
            mythroad,
            clock,
            gdb_server: None,
        };
        env.cpu.set_cpsr(cpu::Cpu::CPSR_USER_MODE);

        if let Some(addrs) = env.options.gdb_listen_addrs.take() {
            let listener = TcpListener::bind(addrs.as_slice())
                .map_err(|e| format!("Could not bind to {:?}: {}", addrs, e))?;
            println!(
                "Waiting for debugger connection on {}...",
                addrs
                    .into_iter()
                    .map(|a| format!("{}", a))
                    .collect::<Vec<String>>()
                    .join(", ")
            );
            let (client, client_addr) = listener
                .accept()
                .map_err(|e| format!("Could not accept connection: {}", e))?;
            println!("Debugger client connected on {}.", client_addr);
            let mut gdb_server = gdb::GdbServer::new(client);
            let step = gdb_server.wait_for_debugger(None, &mut env.cpu, &mut env.mem);
            assert!(!step, "Can't step right now!"); // TODO?
            env.gdb_server = Some(gdb_server);
        }

        println!("CPU emulation begins now.");

        Ok(env)
    }

    fn stack_trace(&self) {
        echo!(
            "-1. {:#x} (PC)",
            self.cpu.pc_with_thumb_bit().addr_with_thumb_bit()
        );
        let mut lr = self.cpu.reg(cpu::Cpu::LR);
        echo!(" 0. {:#x} (LR)", lr);
        let mut i = 0;
        let mut frame_pointer: mem::ConstPtr<u8> = mem::Ptr::from_bits(self.cpu.reg(7));
        loop {
            let fp = frame_pointer.to_bits();
            if fp < mem::Memory::STACK_LOW_END || fp > mem::Memory::STACK_HIGH_END - 8 {
                echo!("Next FP ({frame_pointer:?}) is outside the stack.");
                break;
            }
            lr = self.mem.read((frame_pointer + 4).cast());
            frame_pointer = self.mem.read(frame_pointer.cast());
            echo!("{:2}. {:#x}", i, lr);
            i += 1;
        }
    }

    /// Run the emulator.
    pub fn run(mut self) {
        let entry = format!("%{}", self.initial_pack_filename);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let start_file = match self.options.runtime_mode {
                options::RuntimeMode::Full => mrp::MR_START_FILE_NAME,
                options::RuntimeMode::Mini => mrp::START_FILE_NAME,
            };

            let result = dsm::mr_start_dsm(&mut self, start_file, Some(&entry));
            result == mythroad::MrResult::Success as i32
        }));

        match res {
            Ok(true) => {
                self.run_event_loop();
            }
            Ok(false) => {
                log!("Mythroad: DSM startup failed");
            }
            Err(e) => {
                echo!("Register state immediately after panic:");
                self.cpu.dump_regs();
                echo!("Attempting to produce stack trace:");
                self.stack_trace();
                std::panic::resume_unwind(e);
            }
        }
    }

    #[cold]
    /// Let the debugger handle a CPU error, or panic if there's no debugger
    /// connected. Returns [true] if the CPU should step and then resume
    /// debugging, or [false] if it should resume normal execution.
    fn debug_cpu_error(&mut self, error: cpu::CpuError) -> bool {
        if matches!(error, cpu::CpuError::UndefinedInstruction)
            || matches!(error, cpu::CpuError::Breakpoint)
        {
            // Rewind the PC so that it's at the instruction where the error
            // occurred, rather than the next instruction. This is necessary for
            // GDB to detect its software breakpoints. For some reason this
            // isn't correct for memory errors however.
            let instruction_len = if (self.cpu.cpsr() & cpu::Cpu::CPSR_THUMB) != 0 {
                2
            } else {
                4
            };
            let pc = self.cpu.reg(cpu::Cpu::PC);
            self.cpu
                .set_reg(cpu::Cpu::PC, pc.wrapping_sub(instruction_len));
        }
        if self.gdb_server.is_none() {
            panic!("Error during CPU execution: {:?}", error);
        }

        echo!("Debuggable error during CPU execution: {:?}.", error);
        self.enter_debugger(Some(error))
    }

    /// Used to check whether a debugger is connected, and therefore whether
    /// [Environment::enter_debugger] will do something.
    pub fn is_debugging_enabled(&self) -> bool {
        self.gdb_server.is_some()
    }

    /// Suspend execution and hand control to the connected debugger.
    /// You should precede this call with a log message that explains why the
    /// debugger is being invoked. The return value is the same as
    /// [gdb::GdbServer::wait_for_debugger]'s.
    #[must_use]
    pub fn enter_debugger(&mut self, reason: Option<cpu::CpuError>) -> bool {
        // GDB doesn't seem to manage to produce a useful stack trace, so
        // let's print our own.
        self.stack_trace();
        self.gdb_server
            .as_mut()
            .unwrap()
            .wait_for_debugger(reason, &mut self.cpu, &mut self.mem)
    }

    pub fn run_call(&mut self) {
        self.run_inner(false)
    }

    #[cfg(not(target_os = "emscripten"))]
    fn run_event_loop(&mut self) {
        loop {
            if !self.event_loop_tick() {
                return;
            }

            let mr_state = self.mythroad.state.mr_state.get(&self.mem);
            if mr_state == mythroad::MrRunState::Stop as u32 {
                return;
            }

            let sleep_duration = self.event_loop_sleep_duration();
            if !sleep_duration.is_zero() {
                std::thread::sleep(sleep_duration);
            }
        }
    }

    #[cfg(target_os = "emscripten")]
    fn run_event_loop(self) {
        let env = Box::into_raw(Box::new(self));
        unsafe {
            emscripten_set_main_loop_arg(Self::web_loop, env.cast(), 0, true);
            drop(Box::from_raw(env));
        }
    }

    #[cfg(target_os = "emscripten")]
    extern "C" fn web_loop(arg: *mut std::ffi::c_void) {
        let env = unsafe { &mut *arg.cast::<Environment>() };
        let _ = env.event_loop_tick();
    }

    fn event_loop_tick(&mut self) -> bool {
        for command in take_commands() {
            match command {
                Command::SetSpeedPercent(percent) => {
                    self.set_speed(f64::from(percent) / 100.0);
                }
                Command::SetShowFps(show) => {
                    self.window.set_show_fps(show);
                }
            }
        }
        self.window.expire_hud_notice(Instant::now());
        self.window.poll_for_events(self.mythroad.editbox.is_some());

        while let Some(event) = self.window.pop_event() {
            match event {
                Event::Quit => return false,
                Event::AppWillEnterBackground => {
                    dsm::mr_pause_app(self);
                    continue;
                }
                Event::AppDidEnterForeground => {
                    dsm::mr_resume_app(self);
                    continue;
                }
                Event::Redraw => {
                    self.window.redraw();
                    continue;
                }
                Event::EnterDebugger => {
                    if self.is_debugging_enabled() {
                        log!("Handling EnterDebugger event: entering debugger.");
                        let step = self.enter_debugger(/* reason: */ None);
                        assert!(!step, "Can't step right now!"); // TODO?
                    } else {
                        log!("Ignoring EnterDebugger event: no debugger connected.");
                    }
                    continue;
                }
                Event::AdjustSpeed(delta) => {
                    self.adjust_speed(delta);
                    continue;
                }
                Event::ToggleFps => {
                    self.window.toggle_fps();
                    continue;
                }
                _ => {}
            }

            match event {
                Event::KeyDown(key) => self.mem.mmio_mut().set_keypad_key_state(key as u32, true),
                Event::KeyUp(key) => self.mem.mmio_mut().set_keypad_key_state(key as u32, false),
                _ => {}
            }

            if self.mythroad.editbox.is_some() {
                let result = self.handle_editbox_event(&event);
                match result {
                    editbox::EditResult::Ok => {
                        self.window.stop_text_input();
                        dsm::mr_event(self, dsm::MR_DIALOG_EVENT, dsm::MR_DIALOG_KEY_OK, 0);
                    }
                    editbox::EditResult::Cancel => {
                        self.window.stop_text_input();
                        dsm::mr_event(self, dsm::MR_DIALOG_EVENT, dsm::MR_DIALOG_KEY_CANCEL, 0);
                    }
                    editbox::EditResult::None => {}
                }
                continue;
            }

            match event {
                Event::Quit
                | Event::AppWillEnterBackground
                | Event::AppDidEnterForeground
                | Event::Redraw
                | Event::EnterDebugger
                | Event::AdjustSpeed(_)
                | Event::ToggleFps => {
                    unreachable!()
                }
                Event::KeyDown(key) => {
                    dsm::mr_event(self, dsm::MR_KEY_PRESS, key as i32, 0);
                }
                Event::KeyUp(key) => {
                    dsm::mr_event(self, dsm::MR_KEY_RELEASE, key as i32, 0);
                }
                Event::MouseDown((x, y)) => {
                    dsm::mr_event(self, dsm::MR_MOUSE_DOWN, x as i32, y as i32);
                }
                Event::MouseUp((x, y)) => {
                    dsm::mr_event(self, dsm::MR_MOUSE_UP, x as i32, y as i32);
                }
                Event::MouseMove((x, y)) => {
                    dsm::mr_event(self, dsm::MR_MOUSE_MOVE, x as i32, y as i32);
                }
                Event::TextKeyDown { .. } | Event::TextInput(_) | Event::TextEditing { .. } => {}
            }
        }

        if self.mythroad.editbox.is_some() {
            self.refresh_editbox();
        }

        dsm::mr_media_callback(self);
        dsm::mr_timer(self);
        self.window.update_hud(Instant::now());
        true
    }

    fn handle_editbox_event(&mut self, event: &Event) -> editbox::EditResult {
        let mut editbox = self.mythroad.editbox.take().unwrap();
        let result = editbox.handle_event(event, &self.window);
        mythroad::write_edit_text(self, editbox.text());
        self.render_editbox(&mut editbox);
        self.mythroad.editbox = Some(editbox);
        result
    }

    fn refresh_editbox(&mut self) {
        let mut editbox = self.mythroad.editbox.take().unwrap();
        self.render_editbox(&mut editbox);
        self.mythroad.editbox = Some(editbox);
    }

    fn render_editbox(&mut self, editbox: &mut editbox::EditBox) {
        editbox.render(&mut self.mythroad.font, &self.fs);
        let (width, height) = editbox.size();
        self.window.refresh(editbox.frame(), width, height);
        let (x, y, cursor_width, cursor_height) = editbox.cursor_rect();
        self.window
            .set_text_input_rect(x, y, cursor_width, cursor_height);
    }

    #[cfg(not(target_os = "emscripten"))]
    fn event_loop_sleep_duration(&self) -> Duration {
        const MAX_SLEEP: Duration = Duration::from_millis(1000 / 60);

        if self.mythroad.state.mr_timer_state.get(&self.mem)
            != mythroad::MrTimerState::Running as u32
        {
            return MAX_SLEEP;
        }

        let elapsed = dsm::mr_get_time(self).wrapping_sub(self.mythroad.state.mr_timer_start_time);
        if elapsed >= self.mythroad.state.mr_timer_interval {
            return Duration::ZERO;
        }

        let remaining =
            Duration::from_millis(u64::from(self.mythroad.state.mr_timer_interval - elapsed));
        Duration::from_secs_f64(remaining.as_secs_f64() / self.clock.speed).min(MAX_SLEEP)
    }

    pub(crate) fn guest_time_millis(&self) -> u32 {
        self.clock.elapsed().as_millis() as u32
    }

    fn adjust_speed(&mut self, delta: i32) {
        let current = self.clock.speed;
        let mut speed = (current + f64::from(delta) * 0.05).clamp(0.25, 4.0);
        if (current < 1.0 && speed > 1.0) || (current > 1.0 && speed < 1.0) {
            speed = 1.0;
        }
        self.set_speed((speed * 100.0).round() / 100.0);
    }

    fn set_speed(&mut self, speed: f64) {
        let speed = speed.clamp(0.25, 4.0);
        if (self.clock.speed - speed).abs() < f64::EPSILON {
            return;
        }
        self.clock.set_speed(speed);
        self.window.set_speed(speed);
    }

    pub fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(Duration::from_secs_f64(
            duration.as_secs_f64() / self.clock.speed,
        ))
    }

    fn run_inner(&mut self, root: bool) {
        loop {
            self.window.poll_for_events(self.mythroad.editbox.is_some());
            let mut ticks = 1_000;
            let mut step_and_debug = false;

            while ticks > 0 {
                let state = if step_and_debug {
                    self.cpu.run_or_step(&mut self.mem, None)
                } else {
                    self.cpu.run_or_step(&mut self.mem, Some(&mut ticks))
                };
                match state {
                    cpu::CpuState::Normal => (),
                    cpu::CpuState::Svc(svc) => {
                        // the program counter is pointing at the
                        // instruction after the SVC, but we want the
                        // address of the SVC itself
                        let instruction_size = if self.cpu.cpsr() & cpu::Cpu::CPSR_THUMB != 0 {
                            2
                        } else {
                            4
                        };
                        let svc_pc = self.cpu.reg(cpu::Cpu::PC) - instruction_size;
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

                        let f = self.syscall.get_svc_handler(&mut self.mem, svc_pc, svc);
                        f.call_from_guest(self);
                    }
                    cpu::CpuState::Error(error) => {
                        step_and_debug = self.debug_cpu_error(error);
                        continue;
                    }
                }
                if step_and_debug {
                    step_and_debug = self.gdb_server.as_mut().unwrap().wait_for_debugger(
                        None,
                        &mut self.cpu,
                        &mut self.mem,
                    );
                }
            }
        }
    }
}
