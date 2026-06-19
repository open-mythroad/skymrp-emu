use crate::fs::{Fs, GuestPath};
use crate::mem::{ConstPtr, Memory, MutPtr};
use crate::Environment;

const FONT_12_PATH: &str = "/mythroad/system/gb12.uc2";
const FONT_16_PATH: &str = "/mythroad/system/gb16.uc2";
const FONT_BITMAP_BUF_LEN: u32 = 64;

#[repr(u16)]
enum FontSize {
    Small = 0,
    Medium = 1,
    Big = 2,
}

impl FontSize {
    fn from_bits(bits: u16) -> Option<Self> {
        match bits {
            0 => Some(Self::Small),
            1 => Some(Self::Medium),
            2 => Some(Self::Big),
            _ => None,
        }
    }
}

pub struct Font {
    font12: Option<Vec<u8>>,
    font16: Option<Vec<u8>>,
    current_pixel_size: u32,
    bitmap_buf: MutPtr<u8>,
}

impl Font {
    pub fn new(mem: &mut Memory) -> Self {
        Self {
            font12: None,
            font16: None,
            current_pixel_size: 16,
            bitmap_buf: mem.calloc(FONT_BITMAP_BUF_LEN).cast(),
        }
    }

    fn glyph_bitmap_len(pixel_size: u32) -> u32 {
        (pixel_size * pixel_size).div_ceil(8)
    }

    fn font_cache_for_pixel_size(
        &mut self,
        pixel_size: u32,
    ) -> (&mut Option<Vec<u8>>, &'static str) {
        match pixel_size {
            12 => (&mut self.font12, FONT_12_PATH),
            _ => (&mut self.font16, FONT_16_PATH),
        }
    }

    fn load_font_data(
        font_data_cache: &mut Option<Vec<u8>>,
        fs: &Fs,
        path: &str,
        min_glyph_len: u32,
    ) -> bool {
        if font_data_cache.is_some() {
            return true;
        }

        match fs.read(GuestPath::new(path)) {
            Ok(font_data) => {
                if font_data.len() < min_glyph_len as usize {
                    log_dbg!(
                        "Font: ignored {:?}, file is too small ({:#x} bytes)",
                        path,
                        font_data.len()
                    );
                    return false;
                }
                log_dbg!("Font: loaded {:?} ({:#x} bytes)", path, font_data.len());
                *font_data_cache = Some(font_data);
                true
            }
            Err(()) => {
                log_dbg!("Font: {:?} not found", path);
                false
            }
        }
    }

    fn ensure_font_loaded(&mut self, fs: &Fs, pixel_size: u32) -> bool {
        let (font_data_cache, path) = self.font_cache_for_pixel_size(pixel_size);
        Self::load_font_data(
            font_data_cache,
            fs,
            path,
            Self::glyph_bitmap_len(pixel_size),
        )
    }

    fn select_mythroad_font(&mut self, font_size: u16) {
        self.current_pixel_size = match FontSize::from_bits(font_size) {
            Some(FontSize::Small) => 12,
            Some(FontSize::Medium | FontSize::Big) => 16,
            None => 16,
        }
    }

    fn current_pixel_size(&self) -> u32 {
        self.current_pixel_size
    }

    fn glyph_width(pixel_size: u32, ch: u16) -> i32 {
        if ch < 0x80 {
            (pixel_size / 2) as i32
        } else {
            pixel_size as i32
        }
    }

    fn font_data_for_pixel_size(&self, pixel_size: u32) -> Option<&[u8]> {
        match pixel_size {
            12 => self.font12.as_deref(),
            _ => self.font16.as_deref(),
        }
    }

    fn glyph_bitmap(&self, pixel_size: u32, ch: u16) -> Option<&[u8]> {
        let data = self.font_data_for_pixel_size(pixel_size)?;
        let glyph_len = Self::glyph_bitmap_len(pixel_size);
        let offset = u32::from(ch).saturating_mul(glyph_len);
        let end = offset.saturating_add(glyph_len);
        if end as usize > data.len() {
            return None;
        }

        Some(&data[offset as usize..end as usize])
    }

    fn copy_glyph_bitmap_to_guest(
        &mut self,
        mem: &mut Memory,
        pixel_size: u32,
        ch: u16,
    ) -> ConstPtr<u8> {
        debug_assert!(Self::glyph_bitmap_len(pixel_size) <= FONT_BITMAP_BUF_LEN);
        mem.bytes_at_mut(self.bitmap_buf, FONT_BITMAP_BUF_LEN)
            .fill(0);

        let Some(bitmap) = self.glyph_bitmap(pixel_size, ch) else {
            return self.bitmap_buf.cast_const();
        };

        let glyph_len = Self::glyph_bitmap_len(pixel_size);
        mem.bytes_at_mut(self.bitmap_buf, glyph_len)
            .copy_from_slice(bitmap);
        self.bitmap_buf.cast_const()
    }
}

pub(crate) fn get_char_bitmap(
    env: &mut Environment,
    ch: u16,
    font_size: u16,
    width: MutPtr<i32>,
    height: MutPtr<i32>,
) -> ConstPtr<u8> {
    env.mythroad.font.select_mythroad_font(font_size);
    let pixel_size = env.mythroad.font.current_pixel_size();
    env.mythroad.font.ensure_font_loaded(&env.fs, pixel_size);

    if !width.is_null() {
        env.mem.write(width, Font::glyph_width(pixel_size, ch));
    }
    if !height.is_null() {
        env.mem.write(height, pixel_size as i32);
    }

    env.mythroad
        .font
        .copy_glyph_bitmap_to_guest(&mut env.mem, pixel_size, ch)
}

pub(crate) fn measure_char(env: &mut Environment, ch: u16, font_size: u16) -> (i32, i32) {
    env.mythroad.font.select_mythroad_font(font_size);
    let pixel_size = env.mythroad.font.current_pixel_size();
    env.mythroad.font.ensure_font_loaded(&env.fs, pixel_size);

    (Font::glyph_width(pixel_size, ch), pixel_size as i32)
}

pub(crate) fn draw_char(env: &mut Environment, x: i32, y: i32, ch: u16, color: u16) {
    let pixel_size = env.mythroad.font.current_pixel_size();
    if !env.mythroad.font.ensure_font_loaded(&env.fs, pixel_size) {
        return;
    }

    let Some(bitmap) = env.mythroad.font.glyph_bitmap(pixel_size, ch) else {
        return;
    };
    let fw = Font::glyph_width(pixel_size, ch) as u32;
    let screen_buf: MutPtr<u16> = env.mythroad.state.mr_screen_buf.get(&env.mem);
    if screen_buf.is_null() {
        return;
    }

    let screen_w = env.mythroad.state.mr_screen_w.get(&env.mem);
    let screen_h = env.mythroad.state.mr_screen_h.get(&env.mem);
    if screen_w <= 0 || screen_h <= 0 {
        return;
    }

    let screen_w = screen_w as i32;
    let screen_h = screen_h as i32;
    for dy in 0..pixel_size {
        for dx in 0..fw {
            let bit_index = dy * pixel_size + dx;
            let byte = bitmap[(bit_index / 8) as usize];
            if byte & (0x80 >> (bit_index % 8)) == 0 {
                continue;
            }

            let px = x + dx as i32;
            let py = y + dy as i32;
            if px >= 0 && py >= 0 && px < screen_w && py < screen_h {
                env.mem.write(
                    screen_buf + (py as u32 * screen_w as u32 + px as u32),
                    color,
                );
            }
        }
    }
}
