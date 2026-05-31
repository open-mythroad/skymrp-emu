pub enum Event {
    Quit,
}

pub struct Window {
    _sdl_ctx: sdl2::Sdl,
    _video_ctx: sdl2::VideoSubsystem,
    _window: sdl2::video::Window,
    event_pump: sdl2::EventPump,
}

impl Window {
    pub fn new(title: &str) -> Window {
        let sdl_ctx = sdl2::init().unwrap();
        let video_ctx = sdl_ctx.video().unwrap();

        let window = video_ctx
            .window(title, 240, 320)
            .position_centered()
            .build()
            .unwrap();

        let event_pump = sdl_ctx.event_pump().unwrap();

        Window {
            _sdl_ctx: sdl_ctx,
            _video_ctx: video_ctx,
            _window: window,
            event_pump,
        }
    }

    pub fn poll_for_events(&mut self, events: &mut Vec<Event>) {
        for event in self.event_pump.poll_iter() {
            use sdl2::event::Event as E;
            match event {
                E::Quit { .. } => events.push(Event::Quit),
                _ => (),
            }
        }
    }
}
