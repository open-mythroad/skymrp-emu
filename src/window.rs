/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use crate::options::Options;
use sdl2::pixels::PixelFormatEnum;
use sdl2::rect::Rect;
use sdl2::render::{Canvas, Texture};
use std::collections::VecDeque;
use std::num::NonZeroU32;

const SDL_TOUCH_MOUSE_ID: u32 = u32::MAX;

pub type Coords = (f32, f32);

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum DeviceOrientation {
    Portrait,
    PortraitUpsideDown,
    LandscapeLeft,
    LandscapeRight,
}

fn size_for_orientation(orientation: DeviceOrientation, scale: NonZeroU32) -> (u32, u32) {
    let (width, height) = (240, 320);
    let scale = scale.get();
    match orientation {
        DeviceOrientation::Portrait => (width * scale, height * scale),
        DeviceOrientation::PortraitUpsideDown => (width * scale, height * scale),
        DeviceOrientation::LandscapeLeft => (height * scale, width * scale),
        DeviceOrientation::LandscapeRight => (height * scale, width * scale),
    }
}

pub enum Event {
    Quit,
    KeyDown(MrKey),
    KeyUp(MrKey),
    MouseDown(Coords),
    MouseUp(Coords),
    MouseMove(Coords),
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
    canvas: Canvas<sdl2::video::Window>,
    event_pump: sdl2::EventPump,
    event_queue: VecDeque<Event>,
    fullscreen: bool,
    scale: NonZeroU32,
    device_orientation: DeviceOrientation,
}

impl Window {
    pub fn new(title: &str, options: &Options) -> Window {
        let sdl_ctx = sdl2::init().unwrap();
        let video_ctx = sdl_ctx.video().unwrap();

        let device_orientation = DeviceOrientation::Portrait;
        let fullscreen = options.fullscreen;
        let scale = options.scale;

        let window = if fullscreen {
            let (width, height) = video_ctx.display_bounds(0).unwrap().size();
            let window = video_ctx
                .window(title, width, height)
                .fullscreen_desktop()
                .build()
                .unwrap();
            window
        } else {
            let (width, height) = size_for_orientation(device_orientation, scale);
            let window = video_ctx
                .window(title, width, height)
                .position_centered()
                .build()
                .unwrap();
            window
        };

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
            fullscreen,
            scale,
            device_orientation: device_orientation,
        }
    }

    pub fn sdl_context(&self) -> &sdl2::Sdl {
        &self._sdl_ctx
    }

    pub fn refresh(
        &mut self,
        framebuffer: &[u8],
        guest_screen_width: u32,
        guest_screen_height: u32,
    ) {
        let pitch = guest_screen_width as usize * 2;
        if framebuffer.len() != pitch * guest_screen_height as usize {
            return;
        }

        if self.texture_size != Some((guest_screen_width, guest_screen_height)) {
            self.texture = Some(
                self.canvas
                    .create_texture_streaming(
                        PixelFormatEnum::RGB565,
                        guest_screen_width,
                        guest_screen_height,
                    )
                    .unwrap(),
            );
            self.texture_size = Some((guest_screen_width, guest_screen_height));
        }

        let viewport = self.viewport();
        let dst = Rect::new(viewport.0 as i32, viewport.1 as i32, viewport.2, viewport.3);

        let texture = self.texture.as_mut().unwrap();
        texture.update(None, framebuffer, pitch).unwrap();
        self.canvas.clear();
        self.canvas.copy(texture, None, Some(dst)).unwrap();
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
                            self.event_queue.push_back(Event::KeyDown(key));
                        }
                    }
                }
                E::KeyUp {
                    keycode: Some(keycode),
                    ..
                } => {
                    if let Some(key) = keycode_to_mr_key(keycode) {
                        self.event_queue.push_back(Event::KeyUp(key));
                    }
                }
                E::MouseButtonDown { which, x, y, .. } if which != SDL_TOUCH_MOUSE_ID => {
                    let coords = transform_input_coords(self, (x as f32, y as f32), false);
                    self.event_queue.push_back(Event::MouseDown(coords));
                }
                E::MouseButtonUp { which, x, y, .. } if which != SDL_TOUCH_MOUSE_ID => {
                    let coords = transform_input_coords(self, (x as f32, y as f32), false);
                    self.event_queue.push_back(Event::MouseUp(coords));
                }
                E::MouseMotion {
                    which,
                    x,
                    y,
                    mousestate,
                    ..
                } if which != SDL_TOUCH_MOUSE_ID
                    && (mousestate.left() || mousestate.right() || mousestate.middle()) =>
                {
                    let coords = transform_input_coords(self, (x as f32, y as f32), false);
                    self.event_queue.push_back(Event::MouseMove(coords));
                }
                E::FingerDown { x, y, .. } => {
                    let abs_coords = finger_absolute_coords(self, (x, y));
                    let coords = transform_input_coords(self, abs_coords, false);
                    self.event_queue.push_back(Event::MouseDown(coords));
                }
                E::FingerUp { x, y, .. } => {
                    let abs_coords = finger_absolute_coords(self, (x, y));
                    let coords = transform_input_coords(self, abs_coords, false);
                    self.event_queue.push_back(Event::MouseUp(coords));
                }
                E::FingerMotion { x, y, .. } => {
                    let abs_coords = finger_absolute_coords(self, (x, y));
                    let coords = transform_input_coords(self, abs_coords, false);
                    self.event_queue.push_back(Event::MouseMove(coords));
                }
                _ => {}
            }
        }
    }

    pub fn pop_event(&mut self) -> Option<Event> {
        self.event_queue.pop_front()
    }

    /// Returns the current device orientation
    pub fn current_rotation(&self) -> DeviceOrientation {
        self.device_orientation
    }

    /// Get the size in pixels of the window without rotation or scaling.
    ///
    /// The aspect ratio, scale and orientation reflect the guest app's view of
    /// the world.
    pub fn size_unrotated_unscaled(&self) -> (u32, u32) {
        size_for_orientation(DeviceOrientation::Portrait, NonZeroU32::new(1).unwrap())
    }

    pub fn viewport(&self) -> (u32, u32, u32, u32) {
        let (app_width, app_height) = size_for_orientation(self.device_orientation, self.scale);
        if !cfg!(target_os = "android") && !self.fullscreen {
            return (0, 0, app_width, app_height);
        }

        let (screen_width, screen_height) = self.canvas.window().drawable_size();

        let app_aspect = app_width as f32 / app_height as f32;
        let screen_aspect = screen_width as f32 / screen_height as f32;
        let (scaled_width, scaled_height) = if app_aspect < screen_aspect {
            (
                (screen_height as f32 * app_aspect).round() as u32,
                screen_height,
            )
        } else {
            (
                screen_width,
                (screen_width as f32 / app_aspect).round() as u32,
            )
        };
        let x = (screen_width - scaled_width) / 2;
        let y = (screen_height - scaled_height) / 2;
        (x, y, scaled_width, scaled_height)
    }
}

fn finger_absolute_coords(window: &Window, (x, y): (f32, f32)) -> (f32, f32) {
    let (screen_width, screen_height) = window.canvas.window().drawable_size();
    (screen_width as f32 * x, screen_height as f32 * y)
}
fn transform_input_coords(
    window: &Window,
    (in_x, in_y): (f32, f32),
    independent_of_viewport: bool,
) -> (f32, f32) {
    let (vx, vy, vw, vh) = if independent_of_viewport {
        let (width, height) =
            size_for_orientation(window.device_orientation, NonZeroU32::new(1).unwrap());
        (0, 0, width, height)
    } else {
        window.viewport()
    };
    // normalize to unit square centred on origin
    let x = (in_x - vx as f32) / vw as f32 - 0.5;
    let y = (in_y - vy as f32) / vh as f32 - 0.5;
    // rotate
    // back to pixels
    let (out_w, out_h) = window.size_unrotated_unscaled();
    let out_x = (x + 0.5) * out_w as f32;
    let out_y = (y + 0.5) * out_h as f32;
    // Round to match touch precision of official devices.
    (out_x.round(), out_y.round())
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
