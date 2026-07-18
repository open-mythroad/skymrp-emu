/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use crate::abi::GuestArg;
use crate::encoding;
use crate::font;
use crate::libc;
use crate::mem::{guest_size_of, ConstPtr, MutPtr, MutVoidPtr, Ptr, SafeRead};
use crate::Environment;

use super::{mr_free, MrResult};

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrBitmap {
    pub w: u16,
    pub h: u16,
    pub buflen: u32,
    pub type_: u32,
    pub p: MutPtr<u16>,
}

unsafe impl SafeRead for MrBitmap {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrBitmapDraw {
    pub p: MutPtr<u16>,
    pub w: u16,
    pub h: u16,
    pub x: u16,
    pub y: u16,
}

unsafe impl SafeRead for MrBitmapDraw {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrTransMatrix {
    pub a: i16,
    pub b: i16,
    pub c: i16,
    pub d: i16,
    pub rop: u16,
}

unsafe impl SafeRead for MrTransMatrix {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrTransBitmap {
    pub p_data: MutVoidPtr,
    pub width: i32,
    pub height: i32,
    pub max_width: i32,
    pub trans_color: u16,
    pub p_matrix: MutPtr<i16>,
    pub mark_count: i32,
    pub p_zion: MutPtr<u8>,
}

unsafe impl SafeRead for MrTransBitmap {}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct MrScreenRect {
    x: u16,
    y: u16,
    w: u16,
    h: u16,
}

impl GuestArg for MrScreenRect {
    const REG_COUNT: usize = 2;

    fn from_regs(regs: &[u32]) -> Self {
        Self {
            x: regs[0] as u16,
            y: (regs[0] >> 16) as u16,
            w: regs[1] as u16,
            h: (regs[1] >> 16) as u16,
        }
    }

    fn to_regs(self, regs: &mut [u32]) {
        regs[0] = u32::from(self.x) | (u32::from(self.y) << 16);
        regs[1] = u32::from(self.w) | (u32::from(self.h) << 16);
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct MrColour {
    r: u8,
    g: u8,
    b: u8,
}

impl GuestArg for MrColour {
    const REG_COUNT: usize = 1;

    fn from_regs(regs: &[u32]) -> Self {
        Self {
            r: regs[0] as u8,
            g: (regs[0] >> 8) as u8,
            b: (regs[0] >> 16) as u8,
        }
    }

    fn to_regs(self, regs: &mut [u32]) {
        regs[0] = u32::from(self.r) | (u32::from(self.g) << 8) | (u32::from(self.b) << 16);
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrJGraphicsMutableValues {
    pub clip_x: i32,
    pub clip_y: i32,
    pub clip_width: i32,
    pub clip_height: i32,
    pub clip_x_right: i32,
    pub clip_y_bottom: i32,
    pub translate_x: i32,
    pub translate_y: i32,
    pub font: i32,
    pub color_r: u8,
    pub color_g: u8,
    pub color_b: u8,
    pub color_565: u16,
}

unsafe impl SafeRead for MrJGraphicsMutableValues {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrJGraphicsContext {
    pub mutable_values: MrJGraphicsMutableValues,
    pub screen_buffer: MutPtr<u16>,
    pub screen_width: i32,
    pub screen_height: i32,
    pub flag: i32,
    pub real_screen_buffer: MutPtr<u16>,
    pub real_screen_width: i32,
    pub real_screen_height: i32,
}

unsafe impl SafeRead for MrJGraphicsContext {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrJImage {
    pub data: MutPtr<u16>,
    pub width: u16,
    pub height: u16,
    pub trans: u8,
    pub transcolor: u16,
}

unsafe impl SafeRead for MrJImage {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrTile {
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
    pub x1: i16,
    pub y1: i16,
    pub x2: i16,
    pub y2: i16,
    pub tilew: u16,
    pub tileh: u16,
}

unsafe impl SafeRead for MrTile {}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MrSprite {
    pub h: u16,
}

unsafe impl SafeRead for MrSprite {}

#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BitmapRasterOp {
    Or = 0,
    Xor = 1,
    Copy = 2,
    Not = 3,
    MergeNot = 4,
    AndNot = 5,
    Transparent = 6,
    And = 7,
    Gray = 8,
    Reverse = 9,
}

impl TryFrom<u16> for BitmapRasterOp {
    type Error = ();

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Or),
            1 => Ok(Self::Xor),
            2 => Ok(Self::Copy),
            3 => Ok(Self::Not),
            4 => Ok(Self::MergeNot),
            5 => Ok(Self::AndNot),
            6 => Ok(Self::Transparent),
            7 => Ok(Self::And),
            8 => Ok(Self::Gray),
            9 => Ok(Self::Reverse),
            _ => Err(()),
        }
    }
}

pub(super) fn mr_draw_bitmap(
    env: &mut Environment,
    bmp: MutPtr<u16>,
    x: i16,
    y: i16,
    w: u16,
    h: u16,
) {
    log_dbg!(
        "Mythroad: mr_drawBitmap(bmp={:#x}, x={x}, y={y}, w={w}, h={h}) called from {:#x}",
        bmp.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    if bmp.is_null() || w == 0 || h == 0 {
        return;
    }

    let screen_w_i32 = env.mythroad.state.mr_screen_w.get(&env.mem);
    let screen_h_i32 = env.mythroad.state.mr_screen_h.get(&env.mem);
    if screen_w_i32 <= 0 || screen_h_i32 <= 0 {
        return;
    }

    let screen_w = screen_w_i32 as u32;
    let screen_h = screen_h_i32 as u32;
    let len = screen_w * screen_h * guest_size_of::<u16>();

    let x = i32::from(x);
    let y = i32::from(y);
    let w = i32::from(w);
    let h = i32::from(h);
    if x >= screen_w_i32 || y >= screen_h_i32 || w <= 0 || h <= 0 {
        return;
    }

    let x1 = x + w - 1;
    let y1 = y + h - 1;
    if x1 < 0 || y1 < 0 {
        return;
    }

    let framebuffer = env.mem.bytes_at(bmp.cast::<u8>().cast_const(), len);
    env.window.refresh(framebuffer, screen_w, screen_h);
}

pub(super) fn mr_get_char_bitmap(
    env: &mut Environment,
    ch: u16,
    font_size: u16,
    width: MutPtr<i32>,
    height: MutPtr<i32>,
) -> ConstPtr<u8> {
    log_dbg!(
        "Mythroad: mr_getCharBitmap(ch={ch}, fontSize={font_size}, width={:#x}, height={:#x}) called from {:#x}",
        width.to_bits(),
        height.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    font::get_char_bitmap(env, ch, font_size, width, height)
}

pub(super) fn mr_get_screen_info(env: &mut Environment, screen_info: MutPtr<u32>) -> i32 {
    log_dbg!(
        "Mythroad: mr_getScreenInfo(s={:#x}) called from {:#x}",
        screen_info.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    env.mem
        .write(screen_info, env.mythroad.state.sysinfo.screen_width as u32);
    env.mem.write(
        screen_info + 1,
        env.mythroad.state.sysinfo.screen_height as u32,
    );
    env.mem.write(
        screen_info + 2,
        env.mythroad.state.sysinfo.screen_bits as u32,
    );

    MrResult::Success as i32
}

pub(super) fn disp_up_ex(env: &mut Environment, x: i16, y: i16, w: u16, h: u16) -> i32 {
    log_dbg!(
        "Mythroad: _DispUpEx(x={x}, y={y}, w={w}, h={h}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    if env.mythroad.state.vm_state.get(&env.mem) == 1 {
        let screen_buf = env.mythroad.state.mr_screen_buf.get(&env.mem);
        mr_draw_bitmap(env, screen_buf, x, y, w, h);
    }

    MrResult::Success as i32
}

pub(super) fn draw_point(env: &mut Environment, x: i16, y: i16, native_color: u16) {
    log_dbg!(
        "Mythroad: _DrawPoint(x={x}, y={y}, native_color={native_color}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let state = &env.mythroad.state;

    let screen_buf = state.mr_screen_buf.get(&env.mem);
    let screen_w = state.mr_screen_w.get(&env.mem) as i16;
    let screen_h = state.mr_screen_h.get(&env.mem) as i16;

    if x < 0 || y < 0 || x >= screen_w || y >= screen_h {
        return;
    }

    let offset = screen_w.saturating_mul(y).saturating_add(x) as u32;

    env.mem.write(screen_buf + offset, native_color);
}

#[inline]
fn make_rgb565(r: u32, g: u32, b: u32) -> u16 {
    (((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)) as u16
}

pub(super) fn draw_bitmap(
    env: &mut Environment,
    p: MutPtr<u16>,
    x: i16,
    y: i16,
    w: u16,
    h: u16,
    rop: u16,
    transcolor: u16,
    sx: i16,
    sy: i16,
    mw: i16,
) {
    log_dbg!(
        "Mythroad: _DrawBitmap(p={:#x}, x={x}, y={y}, w={w}, h={h}, rop={rop}, transcolor={transcolor:#x}, sx={sx}, sy={sy}, mw={mw}) called from {:#x}",
        p.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let screen_buf: MutPtr<u16> = env.mythroad.state.mr_screen_buf.get(&env.mem);
    let screen_w = env.mythroad.state.mr_screen_w.get(&env.mem);
    let screen_h = env.mythroad.state.mr_screen_h.get(&env.mem);

    let mut x = i32::from(x);
    let mut y = i32::from(y);
    let mut w = i32::from(w);
    let mut h = i32::from(h);
    let mut sx = i32::from(sx);
    let mut sy = i32::from(sy);
    let mw = i32::from(mw);

    if p.is_null()
        || screen_buf.is_null()
        || screen_w <= 0
        || screen_h <= 0
        || w == 0
        || h == 0
        || mw <= 0
        || sx > mw - 1
        || x + w <= 0
        || x > screen_w - 1
        || y > screen_h - 1
        || y + h <= 0
    {
        return;
    }

    if sx < 0 {
        sx = 0;
    }
    if sy < 0 {
        sy = 0;
    }

    if sx + w > mw - 1 {
        w = mw - sx;
    }
    if w == 0 {
        return;
    }

    if x < 0 {
        w += x;
        sx -= x;
        x = 0;
    }
    if y < 0 {
        h += y;
        sy -= y;
        y = 0;
    }
    if x + w > screen_w - 1 {
        w = screen_w - x;
    }
    if y + h > screen_h - 1 {
        h = screen_h - y;
    }
    if w <= 0 || h <= 0 {
        return;
    }

    let mut dest = screen_buf + (y as u32 * screen_w as u32 + x as u32);
    let mut src = p + (sy as u32 * mw as u32 + sx as u32);
    let rop = BitmapRasterOp::try_from(rop).unwrap_or(BitmapRasterOp::Copy);

    for _ in 0..h as u32 {
        for _ in 0..w as u32 {
            let src_pixel: u16 = env.mem.read(src);
            match rop {
                BitmapRasterOp::Transparent => {
                    if transcolor != src_pixel {
                        env.mem.write(dest, src_pixel);
                    }
                }
                BitmapRasterOp::Or => {
                    let dest_pixel: u16 = env.mem.read(dest);
                    env.mem.write(dest, dest_pixel | src_pixel);
                }
                BitmapRasterOp::Xor => {
                    let dest_pixel: u16 = env.mem.read(dest);
                    env.mem.write(dest, dest_pixel ^ src_pixel);
                }
                BitmapRasterOp::Not => {
                    env.mem.write(dest, !src_pixel);
                }
                BitmapRasterOp::MergeNot => {
                    let dest_pixel: u16 = env.mem.read(dest);
                    env.mem.write(dest, dest_pixel | !src_pixel);
                }
                BitmapRasterOp::AndNot => {
                    let dest_pixel: u16 = env.mem.read(dest);
                    env.mem.write(dest, dest_pixel & !src_pixel);
                }
                BitmapRasterOp::And => {
                    let dest_pixel: u16 = env.mem.read(dest);
                    env.mem.write(dest, dest_pixel & src_pixel);
                }
                BitmapRasterOp::Gray => {
                    if transcolor != src_pixel {
                        let r = u32::from((src_pixel >> 11) & 0x1f);
                        let g = u32::from((src_pixel >> 5) & 0x3f);
                        let b = u32::from(src_pixel & 0x1f);
                        let gray = (0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64) as u32;
                        env.mem.write(dest, make_rgb565(gray, gray, gray));
                    }
                }
                BitmapRasterOp::Reverse => {
                    if transcolor != src_pixel {
                        env.mem.write(dest, !src_pixel);
                    }
                }
                BitmapRasterOp::Copy => {
                    env.mem.write(dest, src_pixel);
                }
            }
            dest += 1;
            src += 1;
        }
        dest += (screen_w - w) as u32;
        src += (mw - w) as u32;
    }
}

pub(super) fn draw_bitmap_ex(
    env: &mut Environment,
    srcbmp: ConstPtr<MrBitmapDraw>,
    dstbmp: ConstPtr<MrBitmapDraw>,
    w: u16,
    h: u16,
    p_trans: ConstPtr<MrTransMatrix>,
    transcolor: u16,
) {
    log_dbg!(
        "Mythroad: _DrawBitmapEx(srcbmp={:#x}, dstbmp={:#x}, w={w}, h={h}, pTrans={:#x}, transcolor={transcolor:#x}) called from {:#x}",
        srcbmp.to_bits(),
        dstbmp.to_bits(),
        p_trans.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    if srcbmp.is_null() || dstbmp.is_null() || p_trans.is_null() || w == 0 || h == 0 {
        return;
    }

    let srcbmp: MrBitmapDraw = env.mem.read(srcbmp);
    let dstbmp: MrBitmapDraw = env.mem.read(dstbmp);
    let trans: MrTransMatrix = env.mem.read(p_trans);
    if srcbmp.p.is_null() || dstbmp.p.is_null() || srcbmp.w == 0 || dstbmp.w == 0 {
        return;
    }

    let a = i32::from(trans.a);
    let b = i32::from(trans.b);
    let c = i32::from(trans.c);
    let d = i32::from(trans.d);
    let determinant = a * d - b * c;
    if determinant == 0 {
        return;
    }

    let w = i32::from(w);
    let h = i32::from(h);
    let center_x = i32::from(dstbmp.x) + w / 2;
    let center_y = i32::from(dstbmp.y) + h / 2;

    let mut max_y = (c.abs() * w + d.abs() * h) >> 9;
    let mut min_y = -max_y;
    max_y = max_y.min(i32::from(dstbmp.h) - center_y);
    min_y = min_y.max(-center_y);

    for dy in min_y..max_y {
        let half_w_det = (w * determinant) >> 9;
        let half_h_det = (h * determinant) >> 9;
        let max_x_by_d = if d == 0 {
            999
        } else {
            ((half_w_det + b * dy) / d).max((b * dy - half_w_det) / d)
        };
        let max_x_by_c = if c == 0 {
            999
        } else {
            ((a * dy + half_h_det) / c).max((a * dy - half_h_det) / c)
        };
        let min_x_by_d = if d == 0 {
            -999
        } else {
            ((b * dy - half_w_det) / d).min((half_w_det + b * dy) / d)
        };
        let min_x_by_c = if c == 0 {
            -999
        } else {
            ((a * dy - half_h_det) / c).min((a * dy + half_h_det) / c)
        };

        let max_x = max_x_by_d
            .min(max_x_by_c)
            .min(i32::from(dstbmp.w) - center_x);
        let min_x = min_x_by_d.min(min_x_by_c).max(-center_x);
        if min_x >= max_x {
            continue;
        }

        let mut dstp = dstbmp.p + ((dy + center_y) * i32::from(dstbmp.w) + min_x + center_x) as u32;
        match BitmapRasterOp::try_from(trans.rop).ok() {
            Some(BitmapRasterOp::Transparent) => {
                for dx in min_x..max_x {
                    let offset_y = ((((a * dy - c * dx) as i64) << 8) / i64::from(determinant)
                        + i64::from(h / 2)) as i32;
                    let offset_x = ((((d * dx - b * dy) as i64) << 8) / i64::from(determinant)
                        + i64::from(w / 2)) as i32;
                    if offset_y >= 0 && offset_y < h && offset_x >= 0 && offset_x < w {
                        let srcp = srcbmp.p
                            + ((offset_y + i32::from(srcbmp.y)) * i32::from(srcbmp.w)
                                + offset_x
                                + i32::from(srcbmp.x)) as u32;
                        let pixel: u16 = env.mem.read(srcp);
                        if pixel != transcolor {
                            env.mem.write(dstp, pixel);
                        }
                    }
                    dstp += 1;
                }
            }
            Some(BitmapRasterOp::Copy) => {
                for dx in min_x..max_x {
                    let offset_y = ((((a * dy - c * dx) as i64) << 8) / i64::from(determinant)
                        + i64::from(h / 2)) as i32;
                    let offset_x = ((((d * dx - b * dy) as i64) << 8) / i64::from(determinant)
                        + i64::from(w / 2)) as i32;
                    if offset_y >= 0 && offset_y < h && offset_x >= 0 && offset_x < w {
                        let srcp = srcbmp.p
                            + ((offset_y + i32::from(srcbmp.y)) * i32::from(srcbmp.w)
                                + offset_x
                                + i32::from(srcbmp.x)) as u32;
                        let pixel: u16 = env.mem.read(srcp);
                        env.mem.write(dstp, pixel);
                    }
                    dstp += 1;
                }
            }
            Some(_) | None => {}
        }
    }
}

pub(super) fn draw_rect(
    env: &mut Environment,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    r: u8,
    g: u8,
    b: u8,
) {
    log_dbg!(
        "Mythroad: DrawRect(x={x}, y={y}, w={w}, h={h}, r={r}, g={g}, b={b}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let screen_buf: MutPtr<u16> = env.mythroad.state.mr_screen_buf.get(&env.mem);
    if screen_buf.is_null() {
        log_dbg!("Mythroad: DrawRect ignored because mr_screen_buf is null");
        return;
    }

    let screen_w = env.mythroad.state.mr_screen_w.get(&env.mem);
    let screen_h = env.mythroad.state.mr_screen_h.get(&env.mem);
    if screen_w <= 0 || screen_h <= 0 || w <= 0 || h <= 0 {
        return;
    }

    let screen_w = screen_w as i32;
    let screen_h = screen_h as i32;

    let x_min = x.max(0);
    let y_min = y.max(0);
    let x_max = (x + w).min(screen_w);
    let y_max = (y + h).min(screen_h);

    if x_max <= x_min || y_max <= y_min {
        return;
    }

    let color = make_rgb565(r as u32, g as u32, b as u32);
    let x_min = x_min as u32;
    let y_min = y_min as u32;
    let x_max = x_max as u32;
    let y_max = y_max as u32;
    let screen_w = screen_w as u32;
    let rect_w = x_max - x_min;

    let first_row = screen_buf + (y_min * screen_w + x_min);
    for x_d in 0..rect_w {
        env.mem.write(first_row + x_d, color);
    }

    if first_row.to_bits() & 0x3 != 0 {
        for y_d in (y_min + 1)..y_max {
            let row = screen_buf + (y_d * screen_w + x_min);
            env.mem.write(row, color);
            if rect_w > 1 {
                libc::string::memcpy(
                    env,
                    (row + 1).cast_void(),
                    (first_row + 1).cast_const().cast_void(),
                    (rect_w - 1) * guest_size_of::<u16>(),
                );
            }
        }
    } else {
        for y_d in (y_min + 1)..y_max {
            let row = screen_buf + (y_d * screen_w + x_min);
            libc::string::memcpy(
                env,
                row.cast_void(),
                first_row.cast_const().cast_void(),
                rect_w * guest_size_of::<u16>(),
            );
        }
    }
}

pub(super) fn draw_text(
    env: &mut Environment,
    pc_text: ConstPtr<u8>,
    x: i32,
    y: i32,
    r: u8,
    g: u8,
    b: u8,
    is_unicode: u32,
    font: u16,
) -> i32 {
    let color = make_rgb565(r as u32, g as u32, b as u32);

    log_dbg!(
        "Mythroad: _DrawText(pcText={:#x}, x={x}, y={y}, r={r}, g={g}, b={b}, is_unicode={is_unicode}, font={font}) called from {:#x}",
        pc_text.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let screen_w = env.mythroad.state.mr_screen_w.get(&env.mem);
    if screen_w <= 0 {
        return MrResult::Success as i32;
    }

    let mut converted_len = 0u32;
    let pc_text = if is_unicode == 0 {
        let converted = encoding::c2u(env, pc_text, false);
        converted_len = converted.size;
        converted.ptr.cast::<u8>().cast_const()
    } else {
        pc_text
    };

    let mut sx = x;
    let mut p = pc_text;
    loop {
        let high: u8 = env.mem.read(p);
        let low: u8 = env.mem.read(p + 1);
        if high == 0 && low == 0 {
            break;
        }

        let ch = (u16::from(high) << 8) | u16::from(low);
        let (fw, _fh) = font::measure_char(env, ch, font);
        mr_plat_draw_char(env, ch, sx, y, u32::from(color));
        sx += fw;
        p += 2;

        if sx > screen_w {
            break;
        }
    }

    if is_unicode == 0 {
        mr_free(env, pc_text.cast_mut().cast_void(), converted_len);
    }
    MrResult::Success as i32
}

pub(super) fn bitmap_check(
    env: &mut Environment,
    p: ConstPtr<u16>,
    x: i16,
    y: i16,
    w: u16,
    h: u16,
    transcolor: u16,
    color_check: u16,
) -> i32 {
    log_dbg!(
        "Mythroad: _BitmapCheck(p={:#x}, x={x}, y={y}, w={w}, h={h}, transcolor={transcolor:#x}, color_check={color_check:#x}) called from {:#x}",
        p.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let screen_buf: MutPtr<u16> = env.mythroad.state.mr_screen_buf.get(&env.mem);
    let screen_w = env.mythroad.state.mr_screen_w.get(&env.mem);
    let screen_h = env.mythroad.state.mr_screen_h.get(&env.mem);
    if p.is_null() || screen_buf.is_null() || screen_w <= 0 || screen_h <= 0 || w == 0 || h == 0 {
        return 0;
    }

    let x = i32::from(x);
    let y = i32::from(y);
    let w = i32::from(w);
    let h = i32::from(h);
    let max_y = screen_h.min(y + h);
    let max_x = screen_w.min(x + w);
    let min_y = 0.max(y);
    let min_x = 0.max(x);

    if min_y >= max_y || min_x >= max_x {
        return 0;
    }

    let mut result = 0i32;
    for dy in min_y..max_y {
        let mut dstp = screen_buf + (dy * screen_w + min_x) as u32;
        let mut srcp = p + ((dy - y) * w + (min_x - x)) as u32;
        for _ in min_x..max_x {
            let src_pixel: u16 = env.mem.read(srcp);
            if src_pixel != transcolor {
                let dst_pixel: u16 = env.mem.read(dstp);
                if dst_pixel != color_check {
                    result += 1;
                }
            }
            dstp += 1;
            srcp += 1;
        }
    }

    result
}

pub(super) fn mr_eff_set_con(
    env: &mut Environment,
    x: i16,
    y: i16,
    w: i16,
    h: i16,
    perr: i16,
    perg: i16,
    perb: i16,
) -> i32 {
    log_dbg!(
        "Mythroad: _mr_EffSetCon(x={x}, y={y}, w={w}, h={h}, perr={perr}, perg={perg}, perb={perb}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    let screen_buf: MutPtr<u16> = env.mythroad.state.mr_screen_buf.get(&env.mem);
    let screen_w = env.mythroad.state.mr_screen_w.get(&env.mem);
    let screen_h = env.mythroad.state.mr_screen_h.get(&env.mem);
    if screen_buf.is_null() || screen_w <= 0 || screen_h <= 0 || w <= 0 || h <= 0 {
        return 0;
    }

    let x = i32::from(x);
    let y = i32::from(y);
    let w = i32::from(w);
    let h = i32::from(h);
    let perr = u32::from(perr as u16);
    let perg = u32::from(perg as u16);
    let perb = u32::from(perb as u16);

    let max_y = screen_h.min(y + h);
    let max_x = screen_w.min(x + w);
    let min_y = 0.max(y);
    let min_x = 0.max(x);
    if min_y >= max_y || min_x >= max_x {
        return 0;
    }

    for dy in min_y..max_y {
        let mut p = screen_buf + (screen_w * dy + min_x) as u32;
        for _ in min_x..max_x {
            let pixel = u32::from(env.mem.read::<u16, _>(p));
            let mut pixel_new = (((pixel & 0xf800) * perr) >> 8) & 0xf800;
            pixel_new |= (((pixel & 0x07e0) * perg) >> 8) & 0x07e0;
            pixel_new |= (((pixel & 0x001f) * perb) >> 8) & 0x001f;
            env.mem.write(p, pixel_new as u16);
            p += 1;
        }
    }

    0
}

const DRAW_TEXT_EX_IS_UNICODE: i32 = 1;
const DRAW_TEXT_EX_IS_AUTO_NEWLINE: i32 = 2;

pub(super) fn draw_text_ex(
    env: &mut Environment,
    pc_text: ConstPtr<u8>,
    x: i16,
    y: i16,
    rect: MrScreenRect,
    color: MrColour,
    flag: i32,
    font: u16,
) -> i32 {
    let native_color = make_rgb565(u32::from(color.r), u32::from(color.g), u32::from(color.b));

    log_dbg!(
        "Mythroad: _DrawTextEx(pcText={:#x}, x={x}, y={y}, rect=({}, {}, {}, {}), color=({}, {}, {}), flag={flag:#x}, font={font}) called from {:#x}",
        pc_text.to_bits(),
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        color.r,
        color.g,
        color.b,
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    if pc_text.is_null() {
        log_dbg!("Mythroad: _DrawTextEx ignored because pcText is null");
        return 0;
    }

    let screen_w = env.mythroad.state.mr_screen_w.get(&env.mem);
    let screen_h = env.mythroad.state.mr_screen_h.get(&env.mem);
    if screen_w <= 0 || screen_h <= 0 {
        return MrResult::Success as i32;
    }

    let is_unicode = flag & DRAW_TEXT_EX_IS_UNICODE != 0;
    let auto_newline = flag & DRAW_TEXT_EX_IS_AUTO_NEWLINE != 0;

    let mut converted_len = 0u32;
    let pc_text = if is_unicode {
        pc_text
    } else {
        let converted = encoding::c2u(env, pc_text, false);
        converted_len = converted.size;
        if converted.ptr.is_null() {
            log_dbg!("Mythroad: _DrawTextEx failed to convert text to unicode");
            return 0;
        }
        converted.ptr.cast::<u8>().cast_const()
    };

    let left = 0.max(i32::from(rect.x));
    let top = 0.max(i32::from(rect.y));
    let right = (screen_w - 1).min(i32::from(x) + i32::from(rect.w) - 1);
    let bottom = (screen_h - 1).min(i32::from(y) + i32::from(rect.h) - 1);

    let mut sx = i32::from(x);
    let mut sy = i32::from(y);
    let mut p = pc_text;

    loop {
        let high: u8 = env.mem.read(p);
        let low: u8 = env.mem.read(p + 1);
        if high == 0 && low == 0 {
            break;
        }

        let ch = (u16::from(high) << 8) | u16::from(low);
        let (fw, fh) = font::measure_char(env, ch, font);

        if sx >= left && sx + fw <= right && sy >= top && sy <= bottom {
            mr_plat_draw_char(env, ch, sx, sy, u32::from(native_color));
        }

        p += 2;
        if auto_newline && sx + fw > right {
            sx = left;
            sy += fh;
            if sy > bottom {
                break;
            }
        } else {
            sx += fw;
            if sx > right {
                break;
            }
        }
    }

    if !is_unicode {
        mr_free(env, pc_text.cast_mut().cast_void(), converted_len);
    }

    MrResult::Success as i32
}

pub(super) fn mr_plat_draw_char(env: &mut Environment, ch: u16, x: i32, y: i32, color: u32) {
    log_dbg!(
        "Mythroad: mr_platDrawChar(ch={ch}, x={x}, y={y}, color={color:#x}) called from {:#x}",
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );
    font::draw_char(env, x, y, ch, color as u16);
}

pub(super) fn mr_transbitmap_draw(
    env: &mut Environment,
    h_trans_bmp: ConstPtr<MrTransBitmap>,
    dst_buf: MutPtr<u16>,
    dest_max_w: i32,
    dest_max_h: i32,
    mut sx: i32,
    mut sy: i32,
    mut width: i32,
    mut height: i32,
    mut dx: i32,
    mut dy: i32,
) -> i32 {
    log_dbg!(
        "Mythroad: mr_transbitmapDraw(hTransBmp={:#x}, dstBuf={:#x}, dest_max_w={dest_max_w}, dest_max_h={dest_max_h}, sx={sx}, sy={sy}, width={width}, height={height}, dx={dx}, dy={dy}) called from {:#x}",
        h_trans_bmp.to_bits(),
        dst_buf.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    if h_trans_bmp.is_null() {
        return MrResult::Failed as i32;
    }

    let trans_bmp: MrTransBitmap = env.mem.read(h_trans_bmp);
    if dst_buf.is_null()
        || trans_bmp.p_data.is_null()
        || trans_bmp.p_matrix.is_null()
        || dest_max_w <= 0
        || dest_max_h <= 0
        || trans_bmp.max_width <= 0
        || trans_bmp.mark_count <= 0
    {
        return MrResult::Failed as i32;
    }

    if dx < 0 {
        sx += -dx;
        width += dx;
        dx = 0;
    }

    if dy < 0 {
        sy += -dy;
        height += dy;
        dy = 0;
    }

    if sx < 0 {
        width += sx;
        dx += -sx;
        sx = 0;
    }

    if sy < 0 {
        height += sy;
        dy += -sy;
        sy = 0;
    }

    width = width.min(dest_max_w - dx);
    if height > dest_max_h - dy {
        height = dest_max_h - dy;
    }

    if width <= 0 || height <= 0 || dx >= dest_max_w || dy >= dest_max_h {
        return MrResult::Success as i32;
    }

    let mut matrix = trans_bmp.p_matrix + (sy * 2 * trans_bmp.mark_count) as u32;
    let mut src_line = trans_bmp.p_data.cast::<u16>() + (trans_bmp.max_width * sy) as u32;
    let mut dest_line = dst_buf + (dest_max_w * dy + dx) as u32;
    let pos1_e = sx + width;

    for _ in sy..(sy + height) {
        let mut has_next = false;
        let mut next_pixel = 0;

        for j in 0..trans_bmp.mark_count {
            let mark = matrix + (j * 2) as u32;
            let mut start_pixel: i32 = i32::from(env.mem.read(mark));
            if start_pixel < 0 {
                break;
            }

            let mut len: i32 = i32::from(env.mem.read(mark + 1));
            if j == trans_bmp.mark_count - 1 {
                has_next = (len & 0x4000) != 0;
                len &= 0x3fff;
                if has_next {
                    next_pixel = start_pixel + len;
                }
            }

            let mut pos2_e = start_pixel + len;
            if pos2_e < start_pixel || pos1_e < sx {
                continue;
            }

            start_pixel = start_pixel.max(sx);
            pos2_e = pos2_e.min(pos1_e);
            let actual_len = pos2_e - start_pixel;
            if actual_len > 0 {
                let src_pixel = src_line + start_pixel as u32;
                let dest_pixel = dest_line + (start_pixel - sx) as u32;
                libc::string::memcpy(
                    env,
                    dest_pixel.cast_void(),
                    src_pixel.cast_const().cast_void(),
                    actual_len as u32 * guest_size_of::<u16>(),
                );
            }
        }

        if has_next {
            let start_pixel = next_pixel.max(sx);
            let mut src_pixel = src_line + start_pixel as u32;
            let mut dest_pixel = dest_line + (start_pixel - sx) as u32;

            for _ in start_pixel..(sx + width) {
                let src: u16 = env.mem.read(src_pixel);
                if src != trans_bmp.trans_color {
                    env.mem.write(dest_pixel, src);
                }
                dest_pixel += 1;
                src_pixel += 1;
            }
        }

        src_line += trans_bmp.max_width as u32;
        dest_line += dest_max_w as u32;
        matrix += (trans_bmp.mark_count * 2) as u32;
    }

    MrResult::Success as i32
}

const TOP_GRAPHICS: i32 = 16;
const BASELINE_GRAPHICS: i32 = 64;
const BOTTOM_GRAPHICS: i32 = 32;
const VCENTER_GRAPHICS: i32 = 0x2;
const LEFT_GRAPHICS: i32 = 0x4;
const HCENTER_GRAPHICS: i32 = 0x1;
const RIGHT_GRAPHICS: i32 = 0x8;
const AP_V_MASK_GRAPHICS: i32 =
    TOP_GRAPHICS | BASELINE_GRAPHICS | BOTTOM_GRAPHICS | VCENTER_GRAPHICS;
const AP_H_MASK_GRAPHICS: i32 = LEFT_GRAPHICS | HCENTER_GRAPHICS | RIGHT_GRAPHICS;

const TRANS_NONE_SPRITE: i32 = 0;
const TRANS_MIRROR_ROT180_SPRITE: i32 = 1;
const TRANS_MIRROR_SPRITE: i32 = 2;
const TRANS_ROT180_SPRITE: i32 = 3;
const TRANS_MIRROR_ROT270_SPRITE: i32 = 4;
const TRANS_ROT90_SPRITE: i32 = 5;
const TRANS_ROT270_SPRITE: i32 = 6;
const TRANS_MIRROR_ROT90_SPRITE: i32 = 7;

fn ptr_offset<T, const MUT: bool>(ptr: Ptr<T, MUT>, offset: i32) -> Ptr<T, MUT> {
    let byte_offset = i64::from(offset) * i64::from(guest_size_of::<T>());
    Ptr::from_bits((i64::from(ptr.to_bits()) + byte_offset) as u32)
}

fn calc_anchor(
    g_context: MrJGraphicsContext,
    mut x: i32,
    mut y: i32,
    width: i32,
    height: i32,
    anchor: i32,
) -> Option<(i32, i32)> {
    x += g_context.mutable_values.translate_x;
    y += g_context.mutable_values.translate_y;

    let (anchor_h, anchor_v) = if anchor == 0 {
        (LEFT_GRAPHICS, TOP_GRAPHICS)
    } else {
        (AP_H_MASK_GRAPHICS & anchor, AP_V_MASK_GRAPHICS & anchor)
    };

    if anchor_h == LEFT_GRAPHICS && anchor_v == TOP_GRAPHICS {
        return Some((x, y));
    }

    let out_x = match anchor_h {
        RIGHT_GRAPHICS => x - width,
        LEFT_GRAPHICS => x,
        HCENTER_GRAPHICS => x - width / 2,
        _ => return None,
    };

    let out_y = match anchor_v {
        BOTTOM_GRAPHICS => y - height,
        TOP_GRAPHICS => y,
        BASELINE_GRAPHICS => y - height / 2 - 3,
        VCENTER_GRAPHICS => y - height / 2,
        _ => return None,
    };

    Some((out_x, out_y))
}

pub(super) fn mr_draw_region(
    env: &mut Environment,
    g_context: ConstPtr<MrJGraphicsContext>,
    src: ConstPtr<MrJImage>,
    sx: i32,
    sy: i32,
    w: i32,
    h: i32,
    transform: i32,
    mut x: i32,
    mut y: i32,
    anchor: i32,
) {
    log_dbg!(
        "Mythroad: mr_drawRegion(gContext={:#x}, src={:#x}, sx={sx}, sy={sy}, w={w}, h={h}, transform={transform}, x={x}, y={y}, anchor={anchor}) called from {:#x}",
        g_context.to_bits(),
        src.to_bits(),
        env.cpu.regs()[crate::cpu::Cpu::PC]
    );

    if g_context.is_null() || src.is_null() {
        return;
    }

    let g_context: MrJGraphicsContext = env.mem.read(g_context);
    let src: MrJImage = env.mem.read(src);
    if src.data.is_null() || g_context.screen_buffer.is_null() {
        return;
    }

    let transcolor = if src.trans != 0 {
        if src.trans == 1 {
            env.mem.read(src.data)
        } else {
            src.transcolor
        }
    } else {
        0
    };

    if anchor == 20 || anchor == 0 {
        x += g_context.mutable_values.translate_x;
        y += g_context.mutable_values.translate_y;
    } else {
        let dims = if (transform >> 2) == 0 {
            (w, h)
        } else {
            (h, w)
        };
        let Some((anchor_x, anchor_y)) = calc_anchor(g_context, x, y, dims.0, dims.1, anchor)
        else {
            return;
        };
        x = anchor_x;
        y = anchor_y;
    }

    let min_x = x.max(g_context.mutable_values.clip_x);
    let min_y = y.max(g_context.mutable_values.clip_y);
    let (max_x, max_y) = if (transform >> 2) == 0 {
        (
            (x + w).min(g_context.mutable_values.clip_x_right),
            (y + h).min(g_context.mutable_values.clip_y_bottom),
        )
    } else {
        (
            (x + h).min(g_context.mutable_values.clip_x_right),
            (y + w).min(g_context.mutable_values.clip_y_bottom),
        )
    };

    let mut dy = max_y - min_y;
    let dx_o = max_x - min_x;
    if dy <= 0 || dx_o <= 0 {
        return;
    }

    let src_width = i32::from(src.width);
    let screen_width = g_context.real_screen_width;
    if src_width <= 0 || g_context.screen_width <= 0 || screen_width <= 0 {
        return;
    }

    let mut value: MutPtr<u16>;
    let k: i32;
    let n: i32;

    match transform {
        TRANS_NONE_SPRITE => {
            if src.trans == 0 {
                let mut srcp = src.data + ((sx + min_y - y) * src_width + (min_x - x + sx)) as u32;
                let mut dstp =
                    g_context.screen_buffer + (min_y * g_context.screen_width + min_x) as u32;
                while dy > 0 {
                    libc::string::memcpy(
                        env,
                        dstp.cast_void(),
                        srcp.cast_const().cast_void(),
                        dx_o as u32 * guest_size_of::<u16>(),
                    );
                    dstp += screen_width as u32;
                    srcp += src_width as u32;
                    dy -= 1;
                }
                return;
            }
            value = src.data + ((sx + min_y - y) * src_width + (min_x - x + sx)) as u32;
            k = 1;
            n = src_width;
        }
        TRANS_MIRROR_ROT180_SPRITE => {
            value = src.data + ((h - 1 - (min_y - y - sy)) * src_width + (min_x - x + sx)) as u32;
            k = 1;
            n = -src_width;

            if src.trans == 0 {
                let mut srcp = value;
                let mut dstp =
                    g_context.screen_buffer + (min_y * g_context.screen_width + min_x) as u32;
                while dy > 0 {
                    libc::string::memcpy(
                        env,
                        dstp.cast_void(),
                        srcp.cast_const().cast_void(),
                        dx_o as u32 * guest_size_of::<u16>(),
                    );
                    dstp += screen_width as u32;
                    srcp = ptr_offset(srcp, n);
                    dy -= 1;
                }
                return;
            }
        }
        TRANS_ROT180_SPRITE => {
            value = src.data
                + ((h - 1 + sy) * src_width + w - (min_y - y) * src_width - 1 + sx - min_x + x)
                    as u32;
            k = -1;
            n = -src_width;
        }
        TRANS_MIRROR_SPRITE => {
            value = src.data + ((min_y - y + sy) * src_width + sx + w - 1 + x - min_x) as u32;
            k = -1;
            n = src_width;
        }
        TRANS_ROT90_SPRITE => {
            value = src.data
                + ((h + sy - 1) * src_width + (min_y - y) + sx - (min_x - x) * src_width) as u32;
            k = -src_width;
            n = 1;
        }
        TRANS_MIRROR_ROT90_SPRITE => {
            value = src.data
                + (sx + (h + sy - 1) * src_width + w - min_y + y - 1 - min_x * src_width
                    + x * src_width) as u32;
            k = -src_width;
            n = -1;
        }
        TRANS_ROT270_SPRITE => {
            value = src.data
                + (sx + sy * src_width + w - (min_y - y) - 1 + min_x * src_width - x * src_width)
                    as u32;
            k = src_width;
            n = -1;
        }
        TRANS_MIRROR_ROT270_SPRITE => {
            value = src.data
                + (sy * src_width - y + sx + min_y + min_x * src_width - x * src_width) as u32;
            k = src_width;
            n = 1;
        }
        _ => return,
    }

    let mut dstp_o = g_context.screen_buffer + (min_y * g_context.screen_width + min_x) as u32;

    while dy > 0 {
        let mut srcp = value;
        let mut dstp = dstp_o;
        let mut dx = dx_o;

        while dx > 0 {
            let pixel: u16 = env.mem.read(srcp);
            if src.trans == 0 || pixel != transcolor {
                env.mem.write(dstp, pixel);
            }

            srcp = ptr_offset(srcp, k);
            dstp += 1;
            dx -= 1;
        }

        dstp_o += screen_width as u32;
        value = ptr_offset(value, n);
        dy -= 1;
    }
}
