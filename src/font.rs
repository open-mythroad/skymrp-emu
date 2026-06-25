use crate::fs::{Fs, GuestPath};
use crate::mem::{ConstPtr, Memory, MutPtr};
use crate::Environment;

const FONT_12_PATH: &str = "/mythroad/system/gb12.uc2";
const FONT_16_PATH: &str = "/mythroad/system/gb16.uc2";
const FONT_BITMAP_BUF_LEN: u32 = 64;

struct BitmapFont {
    data: Vec<u8>,
    cell_size: u32,
    variable_width_row_stride: u32,
}

struct GlyphBitmap {
    width: u32,
    height: u32,
    bits: Vec<u8>,
}

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
    font12: Option<BitmapFont>,
    font16: Option<BitmapFont>,
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
    ) -> (&mut Option<BitmapFont>, &'static str) {
        match pixel_size {
            12 => (&mut self.font12, FONT_12_PATH),
            _ => (&mut self.font16, FONT_16_PATH),
        }
    }

    fn load_font_data(
        font_data_cache: &mut Option<BitmapFont>,
        fs: &Fs,
        path: &str,
        pixel_size: u32,
    ) -> bool {
        if font_data_cache.is_some() {
            return true;
        }

        match fs.read(GuestPath::new(path)) {
            Ok(font_data) => {
                let min_glyph_len = Self::glyph_bitmap_len(pixel_size);
                if font_data.len() < min_glyph_len as usize {
                    log_dbg!(
                        "Font: ignored {:?}, file is too small ({:#x} bytes)",
                        path,
                        font_data.len()
                    );
                    return false;
                }
                let variable_width_row_stride =
                    Self::detect_variable_width_row_stride(&font_data, pixel_size);
                log_dbg!(
                    "Font: loaded {:?} ({:#x} bytes, variable-width row stride: {})",
                    path,
                    font_data.len(),
                    variable_width_row_stride
                );
                *font_data_cache = Some(BitmapFont {
                    data: font_data,
                    cell_size: pixel_size,
                    variable_width_row_stride,
                });
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
        Self::load_font_data(font_data_cache, fs, path, pixel_size)
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

    fn detect_variable_width_row_stride(data: &[u8], pixel_size: u32) -> u32 {
        let width = pixel_size / 2;
        let candidates = [
            width,
            width.div_ceil(8) * 8,
            pixel_size,
            pixel_size.div_ceil(8) * 8,
        ];

        let mut best_stride = width;
        let mut best_score = 0;
        for stride in candidates {
            if stride < width {
                continue;
            }

            let score = Self::score_font_layout(data, pixel_size, stride, width);
            if score > best_score {
                best_score = score;
                best_stride = stride;
            }
        }
        best_stride
    }

    fn score_font_layout(data: &[u8], pixel_size: u32, bit_stride: u32, width: u32) -> u32 {
        let glyph_len = Self::glyph_bitmap_len(pixel_size);
        let mut score = 0;

        for ch in 0x21..0x7f {
            let offset = ch * glyph_len;
            let end = offset + glyph_len;
            if end as usize > data.len() {
                break;
            }
            score += Self::score_glyph_layout(
                &data[offset as usize..end as usize],
                pixel_size,
                bit_stride,
                width,
            );
        }

        score
    }

    fn score_glyph_layout(bitmap: &[u8], height: u32, bit_stride: u32, width: u32) -> u32 {
        let mut score = 0u32;

        for y in 0..height {
            let mut row_has_pixel = false;
            let mut prev = false;
            for x in 0..width {
                let set = Self::glyph_bit(bitmap, bit_stride, x, y);
                if set {
                    row_has_pixel = true;
                    score += 1;
                }
                if x != 0 && set != prev {
                    score += 1;
                }
                prev = set;
            }
            if row_has_pixel {
                score += 10;
            }
        }

        score
    }

    fn glyph_bit(bitmap: &[u8], bit_stride: u32, x: u32, y: u32) -> bool {
        let bit_index = y * bit_stride + x;
        let Some(byte) = bitmap.get((bit_index / 8) as usize) else {
            return false;
        };
        byte & (0x80 >> (bit_index % 8)) != 0
    }

    fn set_glyph_bit(bitmap: &mut [u8], bit_stride: u32, x: u32, y: u32) {
        let bit_index = y * bit_stride + x;
        bitmap[(bit_index / 8) as usize] |= 0x80 >> (bit_index % 8);
    }

    fn font_for_pixel_size(&self, pixel_size: u32) -> Option<&BitmapFont> {
        match pixel_size {
            12 => self.font12.as_ref(),
            _ => self.font16.as_ref(),
        }
    }

    fn glyph_bitmap(&self, pixel_size: u32, ch: u16) -> Option<&[u8]> {
        let data = &self.font_for_pixel_size(pixel_size)?.data;
        let glyph_len = Self::glyph_bitmap_len(pixel_size);
        let offset = u32::from(ch).saturating_mul(glyph_len);
        let end = offset.saturating_add(glyph_len);
        if end as usize > data.len() {
            return None;
        }

        Some(&data[offset as usize..end as usize])
    }

    fn glyph_row_stride(font: &BitmapFont, width: u32) -> u32 {
        if width == font.cell_size {
            font.cell_size
        } else {
            font.variable_width_row_stride
        }
    }

    fn rasterize_glyph(&self, pixel_size: u32, ch: u16) -> Option<GlyphBitmap> {
        let font = self.font_for_pixel_size(pixel_size)?;
        let source = self.glyph_bitmap(pixel_size, ch)?;
        let width = Self::glyph_width(pixel_size, ch) as u32;
        let height = pixel_size;
        let source_stride = Self::glyph_row_stride(font, width);
        let mut bits = vec![0; (width * height).div_ceil(8) as usize];

        for y in 0..height {
            for x in 0..width {
                if Self::glyph_bit(source, source_stride, x, y) {
                    Self::set_glyph_bit(&mut bits, width, x, y);
                }
            }
        }

        Some(GlyphBitmap {
            width,
            height,
            bits,
        })
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

        let Some(glyph) = self.rasterize_glyph(pixel_size, ch) else {
            return self.bitmap_buf.cast_const();
        };

        let packed_len: u32 = glyph.bits.len().try_into().unwrap();
        mem.bytes_at_mut(self.bitmap_buf, packed_len)
            .copy_from_slice(&glyph.bits);
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

    let Some(glyph) = env.mythroad.font.rasterize_glyph(pixel_size, ch) else {
        return;
    };
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
    for dy in 0..glyph.height {
        for dx in 0..glyph.width {
            if !Font::glyph_bit(&glyph.bits, glyph.width, dx, dy) {
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
