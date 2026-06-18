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
    current_size: u32,
    bitmap_buf: MutPtr<u8>,
}

impl Font {
    pub fn new(mem: &mut Memory) -> Self {
        Self {
            font12: None,
            font16: None,
            current_size: 16,
            bitmap_buf: mem.calloc(FONT_BITMAP_BUF_LEN).cast(),
        }
    }

    fn select_size(font_size: u16) -> u32 {
        match FontSize::from_bits(font_size) {
            Some(FontSize::Small) => 12,
            Some(FontSize::Medium | FontSize::Big) => 16,
            None => 16,
        }
    }

    fn ch_len(size: u32) -> u32 {
        (size * size).div_ceil(8)
    }

    fn init_size(&mut self, fs: &Fs, size: u32) -> bool {
        let (slot, path) = match size {
            12 => (&mut self.font12, FONT_12_PATH),
            _ => (&mut self.font16, FONT_16_PATH),
        };

        if slot.is_some() {
            return true;
        }

        match fs.read(GuestPath::new(path)) {
            Ok(font_data) => {
                let ch_len = Self::ch_len(size);
                if font_data.len() < ch_len as usize {
                    log_dbg!(
                        "Font: ignored {:?}, file is too small ({:#x} bytes)",
                        path,
                        font_data.len()
                    );
                    return false;
                }
                log_dbg!("Font: loaded {:?} ({:#x} bytes)", path, font_data.len());
                *slot = Some(font_data);
                true
            }
            Err(()) => {
                log_dbg!("Font: {:?} not found", path);
                false
            }
        }
    }

    fn set_current_size(&mut self, font_size: u16) {
        self.current_size = Self::select_size(font_size);
    }

    fn current_size(&self) -> u32 {
        self.current_size
    }

    fn ch_width(size: u32, ch: u16) -> i32 {
        if ch < 0x80 {
            (size / 2) as i32
        } else {
            size as i32
        }
    }

    fn data_for_size(&self, size: u32) -> Option<&[u8]> {
        match size {
            12 => self.font12.as_deref(),
            _ => self.font16.as_deref(),
        }
    }

    fn get_bitmap(&mut self, mem: &mut Memory, size: u32, ch: u16) -> ConstPtr<u8> {
        mem.bytes_at_mut(self.bitmap_buf, FONT_BITMAP_BUF_LEN)
            .fill(0);

        let Some(data) = self.data_for_size(size) else {
            return self.bitmap_buf.cast_const();
        };

        let ch_len = Self::ch_len(size);
        let offset = u32::from(ch).saturating_mul(ch_len);
        let end = offset.saturating_add(ch_len);
        if end as usize > data.len() {
            return self.bitmap_buf.cast_const();
        }

        mem.bytes_at_mut(self.bitmap_buf, ch_len)
            .copy_from_slice(&data[offset as usize..end as usize]);
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
    env.font.set_current_size(font_size);
    let size = env.font.current_size();
    env.font.init_size(&env.fs, size);

    if !width.is_null() {
        env.mem.write(width, Font::ch_width(size, ch));
    }
    if !height.is_null() {
        env.mem.write(height, size as i32);
    }

    env.font.get_bitmap(&mut env.mem, size, ch)
}

pub(crate) fn draw_char(env: &mut Environment, x: i32, y: i32, ch: u16, color: u16) {
    let size = env.font.current_size();
    if !env.font.init_size(&env.fs, size) {
        return;
    }

    let bitmap = env.font.get_bitmap(&mut env.mem, size, ch);
    let fw = Font::ch_width(size, ch) as u32;
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
    for dy in 0..size {
        for dx in 0..fw {
            let bit_index = dy * size + dx;
            let byte: u8 = env.mem.read(bitmap + bit_index / 8);
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
