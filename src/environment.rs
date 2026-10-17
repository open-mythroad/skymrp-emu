/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use crate::window::Event;
use crate::{
    audio, cpu, dsm, editbox, fs, haptics, libc, mem, mrp, mythroad, options, stack, syscall,
    window,
};
use std::time::{Duration, Instant};

use std::path::PathBuf;

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
    pub startup_time: Instant,
}

impl Environment {
    /// Loads the binary and sets up the emulator.
    pub fn new(mrp_path: PathBuf, options: options::Options) -> Result<Environment, String> {
        let window = window::Window::new("SkyMRP", &options);
        audio::init(window.sdl_context()).map_err(|e| e.to_string())?;
        haptics::init(window.sdl_context()).map_err(|e| e.to_string())?;

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

        let mythroad = mythroad::Mythroad::new(&mut mem, options.screen_size);
        syscall.setup_stubs(&mut mem, &mythroad);
        let mut cpu = cpu::Cpu::new(options.cpu_backend);
        stack::prep_stack_for_start(&mut mem, &mut cpu);
        let libc_state = Default::default();

        log_dbg!("CPU emulation begins now.");

        cpu.set_cpsr(cpu::Cpu::CPSR_USER_MODE);

        Ok(Environment {
            options,
            window,
            mem,
            fs,
            initial_pack_filename,
            package_cache: Some(package_cache),
            syscall,
            cpu,
            libc_state,
            mythroad,
            startup_time: Instant::now(),
        })
    }

    fn stack_trace(&self) {
        eprintln!(
            "-1. {:#x} (PC)",
            self.cpu.pc_with_thumb_bit().addr_with_thumb_bit()
        );
        let mut lr = self.cpu.reg(cpu::Cpu::LR);
        eprintln!(" 0. {:#x} (LR)", lr);
        let mut i = 0;
        let mut frame_pointer: mem::ConstPtr<u8> = mem::Ptr::from_bits(self.cpu.reg(7));
        loop {
            let fp = frame_pointer.to_bits();
            if fp < mem::Memory::STACK_LOW_END || fp > mem::Memory::STACK_HIGH_END - 8 {
                eprintln!("Next FP ({frame_pointer:?}) is outside the stack.");
                break;
            }
            lr = self.mem.read((frame_pointer + 4).cast());
            frame_pointer = self.mem.read(frame_pointer.cast());
            eprintln!("{:2}. {:#x}", i, lr);
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
                eprintln!("Register state immediately after panic:");
                self.cpu.dump_regs();
                eprintln!("Attempting to produce stack trace:");
                self.stack_trace();
                std::panic::resume_unwind(e);
            }
        }
    }

    pub fn run_call(&mut self) {
        self.run_inner(false)
    }

    #[cfg(not(target_os = "emscripten"))]
    fn run_event_loop(&mut self) {
        loop {
            self.event_loop_tick();

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
        env.event_loop_tick();
    }

    fn event_loop_tick(&mut self) {
        self.window.poll_for_events(self.mythroad.editbox.is_some());

        while let Some(event) = self.window.pop_event() {
            match event {
                Event::Quit => panic!("User requested quit, exiting..."),
                Event::AppWillEnterBackground => {
                    dsm::mr_pause_app(self);
                    continue;
                }
                Event::AppDidEnterForeground => {
                    dsm::mr_resume_app(self);
                    continue;
                }
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
                Event::Quit | Event::AppWillEnterBackground | Event::AppDidEnterForeground => {
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

        dsm::mr_timer(self);
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
        remaining.min(MAX_SLEEP)
    }

    pub fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration)
    }

    fn run_inner(&mut self, root: bool) {
        loop {
            self.window.poll_for_events(self.mythroad.editbox.is_some());
            let mut ticks = 1_000;

            while ticks > 0 {
                match self.cpu.run(&mut self.mem, &mut ticks) {
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
                }
            }
        }
    }
}
