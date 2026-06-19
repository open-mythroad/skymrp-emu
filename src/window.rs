use sdl2::pixels::PixelFormatEnum;
use sdl2::render::{Canvas, Texture};
use sdl2::video::Window as SdlWindow;
use std::collections::VecDeque;

pub enum Event {
    Quit,
}

pub struct Window {
    _sdl_ctx: sdl2::Sdl,
    _video_ctx: sdl2::VideoSubsystem,
    texture: Option<Texture>,
    texture_size: Option<(u32, u32)>,
    canvas: Canvas<SdlWindow>,
    event_pump: sdl2::EventPump,
    event_queue: VecDeque<Event>,
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

        let canvas = window.into_canvas().present_vsync().build().unwrap();

        let event_pump = sdl_ctx.event_pump().unwrap();

        Window {
            _sdl_ctx: sdl_ctx,
            _video_ctx: video_ctx,
            texture: None,
            texture_size: None,
            canvas,
            event_pump,
            event_queue: VecDeque::new(),
        }
    }

    pub fn refresh(&mut self, framebuffer: &[u8], screen_width: u32, screen_height: u32) {
        let pitch = screen_width as usize * 2;
        if framebuffer.len() != pitch * screen_height as usize {
            return;
        }

        if self.texture_size != Some((screen_width, screen_height)) {
            self.texture = Some(
                self.canvas
                    .create_texture_streaming(PixelFormatEnum::RGB565, screen_width, screen_height)
                    .unwrap(),
            );
            self.texture_size = Some((screen_width, screen_height));
        }

        let texture = self.texture.as_mut().unwrap();
        texture.update(None, framebuffer, pitch).unwrap();
        self.canvas.clear();
        self.canvas.copy(texture, None, None).unwrap();
        self.canvas.present();
    }

    pub fn poll_for_events(&mut self) {
        for event in self.event_pump.poll_iter() {
            use sdl2::event::Event as E;
            if let E::Quit { .. } = event {
                self.event_queue.push_back(Event::Quit);
            }
        }
    }

    pub fn pop_event(&mut self) -> Option<Event> {
        self.event_queue.pop_front()
    }
}
