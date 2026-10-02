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

/// The struct containing the entire emulator state.
pub struct Environment {
    pub options: options::Options,
    pub window: window::Window,
    pub mem: mem::Memory,
    pub fs: fs::Fs,
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
        let initial_mrp_base = mrp::load_from_file(guest_path, &fs, &mut mem)
            .map_err(|e| format!("Could not load MRP file: {}", e))?;

        let mut syscall = syscall::Syscall::new();

        let mythroad = mythroad::Mythroad::new(&mut mem);
        mythroad.register_app(&mut mem, 0, initial_mrp_base);
        syscall.setup_stubs(&mut mem, &mythroad);
        let mut cpu = cpu::Cpu::new();
        stack::prep_stack_for_start(&mut mem, &mut cpu);
        let libc_state = Default::default();

        log_dbg!("CPU emulation begins now.");

        cpu.set_cpsr(cpu::Cpu::CPSR_USER_MODE);

        Ok(Environment {
            options,
            window,
            mem,
            fs,
            package_cache: None,
            syscall,
            cpu,
            libc_state,
            mythroad,
            startup_time: Instant::now(),
        })
    }

    /// Run the emulator.
    pub fn run(&mut self) {
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            dsm::mr_start_dsm_c(self, mrp::MR_START_FILE_NAME, Some("*A"))
                == mythroad::MrResult::Success as i32
        }));

        match res {
            Ok(true) => {
                self.run_event_loop();
            }
            Ok(false) => {
                log!("Mythroad: mr_start_dsmC failed");
            }
            Err(e) => {
                log!(
                    "Panic at PC {:#x}, LR {:#x}",
                    self.cpu.regs()[cpu::Cpu::PC],
                    self.cpu.regs()[cpu::Cpu::LR]
                );
                std::panic::resume_unwind(e);
            }
        }
    }

    pub fn run_call(&mut self) {
        self.run_inner(false)
    }

    fn run_event_loop(&mut self) {
        loop {
            self.window.poll_for_events(self.mythroad.editbox.is_some());

            while let Some(event) = self.window.pop_event() {
                if matches!(event, Event::Quit) {
                    panic!("User requested quit, exiting...");
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
                    Event::Quit => unreachable!(),
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
                    Event::TextKeyDown { .. } | Event::TextInput(_) | Event::TextEditing { .. } => {
                    }
                }
            }

            if self.mythroad.editbox.is_some() {
                self.refresh_editbox();
            }

            dsm::mr_timer(self);

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

                        let f = self.syscall.get_svc_handler(&mut self.mem, svc_pc, svc);
                        f.call_from_guest(self);
                    }
                }
            }
        }
    }
}
