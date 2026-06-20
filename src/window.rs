use sdl2::pixels::PixelFormatEnum;
use sdl2::render::{Canvas, Texture};
use sdl2::video::Window as SdlWindow;
use std::collections::VecDeque;

pub enum Event {
    Quit,
    KeyDown { key: MrKey },
    KeyUp { key: MrKey },
    MouseDown { x: i32, y: i32 },
    MouseUp { x: i32, y: i32 },
    MouseMove { x: i32, y: i32 },
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MrKey {
    Num0 = 0,
    Num1 = 1,
    Num2 = 2,
    Num3 = 3,
    Num4 = 4,
    Num5 = 5,
    Num6 = 6,
    Num7 = 7,
    Num8 = 8,
    Num9 = 9,
    Star = 10,
    Pound = 11,
    Up = 12,
    Down = 13,
    Left = 14,
    Right = 15,
    Power = 16,
    SoftLeft = 17,
    SoftRight = 18,
    Send = 19,
    Select = 20,
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

    pub fn sdl_context(&self) -> &sdl2::Sdl {
        &self._sdl_ctx
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
        let events: Vec<_> = self.event_pump.poll_iter().collect();
        for event in events {
            use sdl2::event::Event as E;
            match event {
                E::Quit { .. } => self.event_queue.push_back(Event::Quit),
                E::KeyDown {
                    keycode: Some(keycode),
                    repeat,
                    ..
                } => {
                    if !repeat {
                        if let Some(key) = keycode_to_mr_key(keycode) {
                            self.event_queue.push_back(Event::KeyDown { key });
                        }
                    }
                }
                E::KeyUp {
                    keycode: Some(keycode),
                    ..
                } => {
                    if let Some(key) = keycode_to_mr_key(keycode) {
                        self.event_queue.push_back(Event::KeyUp { key });
                    }
                }
                E::MouseButtonDown { x, y, .. } => {
                    let (x, y) = self.window_point_to_screen(x, y);
                    self.event_queue.push_back(Event::MouseDown { x, y });
                }
                E::MouseButtonUp { x, y, .. } => {
                    let (x, y) = self.window_point_to_screen(x, y);
                    self.event_queue.push_back(Event::MouseUp { x, y });
                }
                E::MouseMotion {
                    x, y, mousestate, ..
                } if mousestate.left() || mousestate.right() || mousestate.middle() => {
                    let (x, y) = self.window_point_to_screen(x, y);
                    self.event_queue.push_back(Event::MouseMove { x, y });
                }
                E::FingerDown { x, y, .. } => {
                    let (x, y) = self.normalized_point_to_screen(x, y);
                    self.event_queue.push_back(Event::MouseDown { x, y });
                }
                E::FingerUp { x, y, .. } => {
                    let (x, y) = self.normalized_point_to_screen(x, y);
                    self.event_queue.push_back(Event::MouseUp { x, y });
                }
                E::FingerMotion { x, y, .. } => {
                    let (x, y) = self.normalized_point_to_screen(x, y);
                    self.event_queue.push_back(Event::MouseMove { x, y });
                }
                _ => {}
            }
        }
    }

    pub fn pop_event(&mut self) -> Option<Event> {
        self.event_queue.pop_front()
    }

    fn screen_size(&self) -> (u32, u32) {
        self.texture_size
            .or_else(|| self.canvas.output_size().ok())
            .unwrap_or((240, 320))
    }

    fn window_point_to_screen(&self, x: i32, y: i32) -> (i32, i32) {
        let (screen_w, screen_h) = self.screen_size();
        let (window_w, window_h) = self.canvas.output_size().unwrap_or((screen_w, screen_h));

        let x = x * screen_w as i32 / window_w.max(1) as i32;
        let y = y * screen_h as i32 / window_h.max(1) as i32;
        clamp_screen_point(x, y, screen_w, screen_h)
    }

    fn normalized_point_to_screen(&self, x: f32, y: f32) -> (i32, i32) {
        let (screen_w, screen_h) = self.screen_size();
        let x = (x.clamp(0.0, 1.0) * screen_w as f32) as i32;
        let y = (y.clamp(0.0, 1.0) * screen_h as f32) as i32;
        clamp_screen_point(x, y, screen_w, screen_h)
    }
}

fn clamp_screen_point(x: i32, y: i32, screen_w: u32, screen_h: u32) -> (i32, i32) {
    let max_x = screen_w.saturating_sub(1) as i32;
    let max_y = screen_h.saturating_sub(1) as i32;
    (x.clamp(0, max_x), y.clamp(0, max_y))
}

fn keycode_to_mr_key(keycode: sdl2::keyboard::Keycode) -> Option<MrKey> {
    use sdl2::keyboard::Keycode;

    match keycode {
        Keycode::Num0 | Keycode::Kp0 => Some(MrKey::Num0),
        Keycode::Num1 | Keycode::Kp1 => Some(MrKey::Num1),
        Keycode::Num2 | Keycode::Kp2 => Some(MrKey::Num2),
        Keycode::Num3 | Keycode::Kp3 => Some(MrKey::Num3),
        Keycode::Num4 | Keycode::Kp4 => Some(MrKey::Num4),
        Keycode::Num5 | Keycode::Kp5 => Some(MrKey::Num5),
        Keycode::Num6 | Keycode::Kp6 => Some(MrKey::Num6),
        Keycode::Num7 | Keycode::Kp7 => Some(MrKey::Num7),
        Keycode::Num8 | Keycode::Kp8 => Some(MrKey::Num8),
        Keycode::Num9 | Keycode::Kp9 => Some(MrKey::Num9),
        Keycode::Return | Keycode::KpEnter => Some(MrKey::Select),
        Keycode::Equals => Some(MrKey::Pound),
        Keycode::Minus => Some(MrKey::Star),
        Keycode::W | Keycode::Up => Some(MrKey::Up),
        Keycode::S | Keycode::Down => Some(MrKey::Down),
        Keycode::A | Keycode::Left => Some(MrKey::Left),
        Keycode::D | Keycode::Right => Some(MrKey::Right),
        Keycode::Q | Keycode::LeftBracket => Some(MrKey::SoftLeft),
        Keycode::E | Keycode::RightBracket => Some(MrKey::SoftRight),
        Keycode::Tab => Some(MrKey::Send),
        Keycode::Escape => Some(MrKey::Power),
        _ => None,
    }
}
