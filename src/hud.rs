use crate::font::{Font, GlyphBitmap};
use crate::fs::Fs;
use sdl2::pixels::PixelFormatEnum;
use sdl2::rect::Rect;
use sdl2::render::{BlendMode, Canvas, Texture};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const NOTICE_DURATION: Duration = Duration::from_millis(1500);
const TEXT_PADDING: usize = 2;
const FONT_HEIGHT: usize = 16;

pub struct FpsCounter {
    time: Instant,
    frames: u32,
}

impl FpsCounter {
    pub fn start() -> Self {
        Self {
            time: Instant::now(),
            frames: 0,
        }
    }

    pub fn count_frame(&mut self) {
        self.frames += 1;
    }

    pub fn sample(&mut self, now: Instant) -> Option<f32> {
        let duration = now.duration_since(self.time);
        if duration < Duration::from_secs(1) {
            return None;
        }

        self.time = now;
        Some(std::mem::take(&mut self.frames) as f32 / duration.as_secs_f32())
    }
}

struct TextTexture {
    label: String,
    texture: Texture,
    width: u32,
    height: u32,
}

pub struct Hud {
    pub show_fps: bool,
    pub fps: u32,
    pub speed: f64,
    pub notice_expires_at: Option<Instant>,
    fps_counter: FpsCounter,
    fps_texture: Option<TextTexture>,
    speed_texture: Option<TextTexture>,
    glyphs: HashMap<u16, GlyphBitmap>,
    fps_dirty: bool,
    speed_dirty: bool,
}

impl Hud {
    pub fn new(font: &mut Font, fs: &Fs, show_fps: bool, speed: f64) -> Self {
        let glyphs = (0x20..=0x7e)
            .filter_map(|ch| font.edit_glyph(fs, ch).map(|glyph| (ch, glyph)))
            .collect();
        Self {
            show_fps,
            fps: 0,
            speed,
            notice_expires_at: (speed != 1.0).then(|| Instant::now() + NOTICE_DURATION),
            fps_counter: FpsCounter::start(),
            fps_texture: None,
            speed_texture: None,
            glyphs,
            fps_dirty: true,
            speed_dirty: true,
        }
    }

    pub fn count_frame(&mut self) {
        if !self.show_fps {
            return;
        }
        self.fps_counter.count_frame();
    }

    pub fn update_fps(&mut self, now: Instant) {
        if !self.show_fps {
            return;
        }
        if let Some(fps) = self.fps_counter.sample(now) {
            let fps = fps.round() as u32;
            if self.fps == fps {
                return;
            }
            self.fps = fps;
            self.fps_dirty = true;
        }
    }

    pub fn set_show_fps(&mut self, show: bool) {
        if self.show_fps != show {
            self.show_fps = show;
            if show {
                self.fps = 0;
                self.fps_counter = FpsCounter::start();
                self.fps_dirty = true;
            }
        }
    }

    pub fn toggle_fps(&mut self) {
        self.set_show_fps(!self.show_fps);
    }

    pub fn set_speed(&mut self, speed: f64) {
        self.speed = speed;
        self.notice_expires_at = Some(Instant::now() + NOTICE_DURATION);
        self.speed_dirty = true;
    }

    pub fn expire_notice(&mut self, now: Instant) -> bool {
        if self.speed == 1.0
            && self
                .notice_expires_at
                .is_some_and(|expires_at| now >= expires_at)
        {
            self.notice_expires_at = None;
            return true;
        }
        false
    }

    pub fn update_textures(&mut self, canvas: &Canvas<sdl2::video::Window>) -> bool {
        let mut updated = false;
        if self.show_fps && self.fps_dirty {
            let label = format!("FPS {}", self.fps);
            update_text_texture(canvas, &mut self.fps_texture, &label, &self.glyphs);
            self.fps_dirty = false;
            updated = true;
        }

        if (self.speed != 1.0 || self.notice_expires_at.is_some()) && self.speed_dirty {
            let label = format!("{:.2}x", self.speed);
            update_text_texture(canvas, &mut self.speed_texture, &label, &self.glyphs);
            self.speed_dirty = false;
            updated = true;
        }
        updated
    }

    pub fn draw(
        &mut self,
        canvas: &mut Canvas<sdl2::video::Window>,
        viewport: (u32, u32, u32, u32),
        guest_size: (u32, u32),
    ) {
        let show_speed = self.speed != 1.0 || self.notice_expires_at.is_some();
        if !self.show_fps && !show_speed {
            return;
        }

        let scale =
            (viewport.2 as f32 / guest_size.0 as f32).min(viewport.3 as f32 / guest_size.1 as f32);
        let margin = (4.0 * scale).round() as i32;

        if let Some(text) = self.fps_texture.as_ref().filter(|_| self.show_fps) {
            let width = (text.width as f32 * scale).round().max(1.0) as u32;
            let height = (text.height as f32 * scale).round().max(1.0) as u32;
            let dst = Rect::new(
                viewport.0 as i32 + margin,
                viewport.1 as i32 + margin,
                width,
                height,
            );
            canvas.copy(&text.texture, None, dst).unwrap();
        }

        if let Some(text) = self.speed_texture.as_ref().filter(|_| show_speed) {
            let width = (text.width as f32 * scale).round().max(1.0) as u32;
            let height = (text.height as f32 * scale).round().max(1.0) as u32;
            let dst = Rect::new(
                viewport.0 as i32 + viewport.2 as i32 - margin - width as i32,
                viewport.1 as i32 + margin,
                width,
                height,
            );
            canvas.copy(&text.texture, None, dst).unwrap();
        }
    }
}

fn update_text_texture(
    canvas: &Canvas<sdl2::video::Window>,
    slot: &mut Option<TextTexture>,
    label: &str,
    glyphs: &HashMap<u16, GlyphBitmap>,
) {
    if slot.as_ref().is_some_and(|texture| texture.label == label) {
        return;
    }

    let units: Vec<_> = label.encode_utf16().collect();
    let text_width: usize = units
        .iter()
        .map(|unit| Font::edit_glyph_width(*unit) as usize)
        .sum();
    let width = TEXT_PADDING * 2 + text_width;
    let height = TEXT_PADDING * 2 + FONT_HEIGHT;
    let mut pixels = vec![0_u8; width * height * 4];

    for pixel in pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[0, 0, 0, 176]);
    }

    let mut origin_x = TEXT_PADDING;
    for unit in units {
        let glyph_width = Font::edit_glyph_width(unit) as usize;
        if let Some(glyph) = glyphs.get(&unit) {
            for y in 0..glyph.height {
                for x in 0..glyph.width {
                    if !Font::edit_glyph_bit(&glyph, x, y) {
                        continue;
                    }
                    let pixel_x = origin_x + x as usize;
                    let pixel_y = TEXT_PADDING + y as usize;
                    let offset = (pixel_y * width + pixel_x) * 4;
                    pixels[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
        origin_x += glyph_width;
    }

    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator
        .create_texture_static(PixelFormatEnum::RGBA32, width as u32, height as u32)
        .unwrap();
    texture.update(None, &pixels, width * 4).unwrap();
    texture.set_blend_mode(BlendMode::Blend);
    *slot = Some(TextTexture {
        label: label.to_owned(),
        texture,
        width: width as u32,
        height: height as u32,
    });
}
