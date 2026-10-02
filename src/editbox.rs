/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
// Adapted for SDL2 and the RGB565 Mythroad framebuffer from:
// https://github.com/libsdl-org/SDL_ttf/blob/main/examples/editbox.c
use crate::font::{Font, GlyphBitmap};
use crate::fs::Fs;
use crate::window::{Event, Window};
use sdl2::keyboard::Keycode;
use std::ops::Range;
use std::time::Instant;

const MARGIN: i32 = 8;
const HEADER_HEIGHT: i32 = 25;
const FOOTER_HEIGHT: i32 = 25;
const LINE_HEIGHT: i32 = 19;
const GLYPH_HEIGHT: i32 = 16;

const COLOR_PANEL: u16 = 0xffff;
const COLOR_HEADER: u16 = 0x2104;
const COLOR_TEXT: u16 = 0x1082;
const COLOR_SELECTION: u16 = 0x9e7f;
const COLOR_COMPOSITION: u16 = 0x045f;

pub(crate) const EDIT_HANDLE: i32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EditResult {
    None,
    Ok,
    Cancel,
}

struct Line {
    range: Range<usize>,
    width: i32,
}

pub(crate) struct EditBox {
    title: Vec<u16>,

    // Committed UCS-2 text and selection state.
    text: Vec<u16>,
    cursor: usize,
    anchor: usize,

    // Selects the visual side of an ambiguous cursor at a soft wrap.
    cursor_downstream: bool,

    // IME preedit text is kept separate from committed text.
    composition: Vec<u16>,
    composition_cursor: usize,

    max_units: usize,
    width: u32,
    height: u32,

    // The editor is composited over a snapshot of the guest framebuffer.
    background: Vec<u8>,
    frame: Vec<u8>,

    // View and pointer interaction state.
    scroll_line: usize,
    dragging: bool,
    footer_pressed: Option<EditResult>,
    finished: bool,
    last_action: Instant,
}

impl EditBox {
    pub(crate) fn new(
        title: Vec<u16>,
        mut text: Vec<u16>,
        max_size: i32,
        width: u32,
        height: u32,
        background: Vec<u8>,
    ) -> Self {
        let max_units = usize::try_from(max_size.clamp(1, 65_535)).unwrap_or(1);
        text.truncate(max_units);
        let cursor = text.len();
        let mut edit = Self {
            title,
            text,
            cursor,
            anchor: cursor,
            cursor_downstream: false,
            composition: Vec::new(),
            composition_cursor: 0,
            max_units,
            width,
            height,
            frame: background.clone(),
            background,
            scroll_line: 0,
            dragging: false,
            footer_pressed: None,
            finished: false,
            last_action: Instant::now(),
        };
        edit.normalize_cursor();
        edit
    }

    pub(crate) fn text(&self) -> &[u16] {
        &self.text
    }

    pub(crate) fn frame(&self) -> &[u8] {
        &self.frame
    }

    pub(crate) fn background(&self) -> &[u8] {
        &self.background
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    // Handle one SDL-derived event and report dialog completion.
    pub(crate) fn handle_event(&mut self, event: &Event, window: &Window) -> EditResult {
        if self.finished {
            return EditResult::None;
        }
        let result = match event {
            Event::TextInput(text) => {
                self.composition.clear();
                self.insert_utf8(text);
                EditResult::None
            }
            Event::TextEditing {
                text,
                start,
                length,
            } => {
                self.update_composition(text, *start, *length);
                EditResult::None
            }
            Event::TextKeyDown {
                keycode,
                shift,
                ctrl,
            } => self.handle_key(*keycode, *shift, *ctrl, window),
            Event::MouseDown((x, y)) => {
                if *y >= (self.height as i32 - FOOTER_HEIGHT) as f32 {
                    self.dragging = false;
                    self.footer_pressed = Some(if *x < self.width as f32 / 2.0 {
                        EditResult::Ok
                    } else {
                        EditResult::Cancel
                    });
                    EditResult::None
                } else {
                    (self.cursor, self.cursor_downstream) = self.hit_test(*x as i32, *y as i32);
                    self.anchor = self.cursor;
                    self.dragging = true;
                    self.footer_pressed = None;
                    self.composition.clear();
                    window.start_text_input();
                    EditResult::None
                }
            }
            Event::MouseMove((x, y)) if self.dragging => {
                (self.cursor, self.cursor_downstream) = self.hit_test(*x as i32, *y as i32);
                EditResult::None
            }
            Event::MouseUp((x, y)) => {
                if let Some(pressed) = self.footer_pressed.take() {
                    let released = if *y >= (self.height as i32 - FOOTER_HEIGHT) as f32 {
                        Some(if *x < self.width as f32 / 2.0 {
                            EditResult::Ok
                        } else {
                            EditResult::Cancel
                        })
                    } else {
                        None
                    };
                    if released == Some(pressed) {
                        self.finish(pressed)
                    } else {
                        EditResult::None
                    }
                } else if self.dragging {
                    (self.cursor, self.cursor_downstream) = self.hit_test(*x as i32, *y as i32);
                    self.dragging = false;
                    EditResult::None
                } else {
                    EditResult::None
                }
            }
            _ => EditResult::None,
        };
        self.normalize_cursor();
        self.last_action = Instant::now();
        self.finished = result != EditResult::None;
        result
    }

    fn handle_key(&mut self, key: Keycode, shift: bool, ctrl: bool, window: &Window) -> EditResult {
        if ctrl {
            match key {
                Keycode::A => {
                    self.anchor = 0;
                    self.cursor = self.text.len();
                }
                Keycode::C => {
                    if let Some(range) = self.selection() {
                        window.set_clipboard_text(&String::from_utf16_lossy(&self.text[range]));
                    }
                }
                Keycode::X => {
                    if let Some(range) = self.selection() {
                        window.set_clipboard_text(&String::from_utf16_lossy(&self.text[range]));
                        self.delete_selection();
                    }
                }
                Keycode::V => {
                    if let Some(text) = window.clipboard_text() {
                        self.insert_utf8(&text);
                    }
                }
                Keycode::Return | Keycode::KpEnter => return self.finish(EditResult::Ok),
                _ => {}
            }
            return EditResult::None;
        }

        self.composition.clear();
        match key {
            Keycode::Escape => return EditResult::Cancel,
            Keycode::Backspace => self.backspace(),
            Keycode::Delete => self.delete_forward(),
            Keycode::Left => self.move_horizontal(false, shift),
            Keycode::Right => self.move_horizontal(true, shift),
            Keycode::Up => self.move_vertical(-1, shift),
            Keycode::Down => self.move_vertical(1, shift),
            Keycode::Home => self.move_line_edge(false, shift),
            Keycode::End => self.move_line_edge(true, shift),
            Keycode::Return | Keycode::KpEnter => self.insert_units(&[b'\n' as u16]),
            _ => {}
        }
        EditResult::None
    }

    fn insert_utf8(&mut self, text: &str) {
        let units = normalized_utf16(text);
        self.insert_units(&units);
    }

    // Replace the selection before anchoring a new IME composition.
    fn update_composition(&mut self, text: &str, start: i32, length: i32) {
        self.delete_selection();
        self.composition = normalized_utf16(text);
        self.composition_cursor = if start > 0 || length > 0 {
            usize::try_from(start.max(0))
                .unwrap_or(0)
                .min(self.composition.len())
        } else {
            self.composition.len()
        };
    }

    fn insert_units(&mut self, units: &[u16]) {
        self.delete_selection();
        let available = self.max_units.saturating_sub(self.text.len());
        let mut insert_len = available.min(units.len());
        if insert_len > 0
            && insert_len < units.len()
            && is_high_surrogate(units[insert_len - 1])
            && is_low_surrogate(units[insert_len])
        {
            insert_len -= 1;
        }
        self.text.splice(
            self.cursor..self.cursor,
            units[..insert_len].iter().copied(),
        );
        self.cursor += insert_len;
        self.anchor = self.cursor;
        self.cursor_downstream = false;
    }

    // Preserve visible preedit text when the user accepts the dialog.
    fn finish(&mut self, result: EditResult) -> EditResult {
        if result == EditResult::Ok && !self.composition.is_empty() {
            let composition = std::mem::take(&mut self.composition);
            self.composition_cursor = 0;
            self.insert_units(&composition);
        }
        result
    }

    fn backspace(&mut self) {
        if self.delete_selection() || self.cursor == 0 {
            return;
        }
        let start = previous_boundary(&self.text, self.cursor);
        self.text.drain(start..self.cursor);
        self.cursor = start;
        self.anchor = start;
        self.cursor_downstream = false;
    }

    fn delete_forward(&mut self) {
        if self.delete_selection() || self.cursor == self.text.len() {
            return;
        }
        let end = next_boundary(&self.text, self.cursor);
        self.text.drain(self.cursor..end);
        self.anchor = self.cursor;
        self.cursor_downstream = false;
    }

    fn delete_selection(&mut self) -> bool {
        let Some(range) = self.selection() else {
            return false;
        };
        self.cursor = range.start;
        self.anchor = range.start;
        self.cursor_downstream = false;
        self.text.drain(range);
        true
    }

    fn selection(&self) -> Option<Range<usize>> {
        if self.cursor == self.anchor {
            None
        } else {
            Some(self.cursor.min(self.anchor)..self.cursor.max(self.anchor))
        }
    }

    fn move_horizontal(&mut self, forward: bool, extend: bool) {
        if !extend {
            if let Some(range) = self.selection() {
                self.cursor = if forward { range.end } else { range.start };
                self.anchor = self.cursor;
                self.cursor_downstream = !forward;
                return;
            }
        }
        self.cursor = if forward {
            next_boundary(&self.text, self.cursor)
        } else {
            previous_boundary(&self.text, self.cursor)
        };
        if !extend {
            self.anchor = self.cursor;
        }
        self.cursor_downstream = !forward;
    }

    fn move_vertical(&mut self, delta: isize, extend: bool) {
        let lines = layout(&self.text, self.body_width());
        let (line_index, x) =
            cursor_position(&self.text, &lines, self.cursor, self.cursor_downstream);
        let target = line_index
            .saturating_add_signed(delta)
            .min(lines.len().saturating_sub(1));
        self.cursor = index_at_x(&self.text, &lines[target], x);
        self.cursor_downstream = is_soft_wrap_start(&lines, target, self.cursor);
        if !extend {
            self.anchor = self.cursor;
        }
    }

    fn move_line_edge(&mut self, end: bool, extend: bool) {
        let lines = layout(&self.text, self.body_width());
        let (line_index, _) =
            cursor_position(&self.text, &lines, self.cursor, self.cursor_downstream);
        self.cursor = if end {
            lines[line_index].range.end
        } else {
            lines[line_index].range.start
        };
        self.cursor_downstream = !end && is_soft_wrap_start(&lines, line_index, self.cursor);
        if !extend {
            self.anchor = self.cursor;
        }
    }

    fn hit_test(&self, x: i32, y: i32) -> (usize, bool) {
        let lines = layout(&self.text, self.body_width());
        let relative_y = (y - HEADER_HEIGHT - MARGIN).max(0);
        let line = self
            .scroll_line
            .saturating_add((relative_y / LINE_HEIGHT) as usize)
            .min(lines.len().saturating_sub(1));
        let cursor = index_at_x(&self.text, &lines[line], (x - MARGIN).max(0));
        (cursor, is_soft_wrap_start(&lines, line, cursor))
    }

    fn normalize_cursor(&mut self) {
        self.cursor = valid_boundary(&self.text, self.cursor.min(self.text.len()));
        self.anchor = valid_boundary(&self.text, self.anchor.min(self.text.len()));
    }

    fn body_width(&self) -> i32 {
        self.width as i32 - MARGIN * 2
    }

    fn visible_line_count(&self) -> usize {
        ((self.height as i32 - HEADER_HEIGHT - FOOTER_HEIGHT - MARGIN * 2) / LINE_HEIGHT).max(1)
            as usize
    }

    // Compose the complete editor UI into the RGB565 host frame.
    pub(crate) fn render(&mut self, font: &mut Font, fs: &Fs) {
        self.frame.clone_from(&self.background);
        fill_rect(
            &mut self.frame,
            self.width,
            self.height,
            0,
            0,
            self.width as i32,
            self.height as i32,
            COLOR_PANEL,
        );
        fill_rect(
            &mut self.frame,
            self.width,
            self.height,
            0,
            0,
            self.width as i32,
            HEADER_HEIGHT,
            COLOR_HEADER,
        );

        let title = if self.title.is_empty() {
            "Edit".encode_utf16().collect::<Vec<_>>()
        } else {
            self.title.clone()
        };
        draw_units(
            &mut self.frame,
            self.width,
            self.height,
            font,
            fs,
            &title,
            MARGIN,
            4,
            0xffff,
            None,
        );

        let mut display = self.text.clone();
        display.splice(self.cursor..self.cursor, self.composition.iter().copied());
        let composition_range = self.cursor..self.cursor + self.composition.len();
        let display_cursor = self.cursor + self.composition_cursor.min(self.composition.len());
        let lines = layout(&display, self.body_width());
        let cursor_downstream = self.cursor_downstream && self.composition.is_empty();
        let (cursor_line, _) = cursor_position(&display, &lines, display_cursor, cursor_downstream);
        let visible = self.visible_line_count();
        if cursor_line < self.scroll_line {
            self.scroll_line = cursor_line;
        } else if cursor_line >= self.scroll_line + visible {
            self.scroll_line = cursor_line + 1 - visible;
        }
        self.scroll_line = self
            .scroll_line
            .min(lines.len().saturating_sub(visible.min(lines.len())));

        let selection = self.selection().map(|range| {
            let start = if range.start > self.cursor {
                range.start + self.composition.len()
            } else {
                range.start
            };
            let end = if range.end > self.cursor {
                range.end + self.composition.len()
            } else {
                range.end
            };
            start..end
        });

        for (visible_index, line) in lines
            .iter()
            .enumerate()
            .skip(self.scroll_line)
            .take(visible)
        {
            let y = HEADER_HEIGHT
                + MARGIN
                + (visible_index.saturating_sub(self.scroll_line) as i32 * LINE_HEIGHT);
            draw_line(
                &mut self.frame,
                self.width,
                self.height,
                font,
                fs,
                &display,
                line,
                selection.as_ref(),
                &composition_range,
                MARGIN,
                y,
            );
        }

        if self.last_action.elapsed().as_millis() % 1000 < 550 {
            let (line_index, cursor_x) =
                cursor_position(&display, &lines, display_cursor, cursor_downstream);
            if line_index >= self.scroll_line && line_index < self.scroll_line + visible {
                let y =
                    HEADER_HEIGHT + MARGIN + ((line_index - self.scroll_line) as i32 * LINE_HEIGHT);
                fill_rect(
                    &mut self.frame,
                    self.width,
                    self.height,
                    MARGIN + cursor_x,
                    y,
                    1,
                    GLYPH_HEIGHT,
                    COLOR_TEXT,
                );
            }
        }

        let footer_y = self.height as i32 - FOOTER_HEIGHT;
        fill_rect(
            &mut self.frame,
            self.width,
            self.height,
            0,
            footer_y,
            self.width as i32,
            FOOTER_HEIGHT,
            COLOR_HEADER,
        );
        let ok = "确定".encode_utf16().collect::<Vec<_>>();
        let cancel = "取消".encode_utf16().collect::<Vec<_>>();
        draw_units(
            &mut self.frame,
            self.width,
            self.height,
            font,
            fs,
            &ok,
            MARGIN,
            footer_y + 5,
            COLOR_PANEL,
            None,
        );
        draw_units(
            &mut self.frame,
            self.width,
            self.height,
            font,
            fs,
            &cancel,
            self.width as i32 - MARGIN - text_width(&cancel),
            footer_y + 5,
            COLOR_PANEL,
            None,
        );
    }

    pub(crate) fn cursor_rect(&self) -> (i32, i32, u32, u32) {
        let mut display = self.text.clone();
        display.splice(self.cursor..self.cursor, self.composition.iter().copied());
        let lines = layout(&display, self.body_width());
        let cursor = self.cursor + self.composition_cursor.min(self.composition.len());
        let cursor_downstream = self.cursor_downstream && self.composition.is_empty();
        let (line, x) = cursor_position(&display, &lines, cursor, cursor_downstream);
        let visible_line = line.saturating_sub(self.scroll_line);
        (
            MARGIN + x,
            HEADER_HEIGHT + MARGIN + visible_line as i32 * LINE_HEIGHT,
            1,
            GLYPH_HEIGHT as u32,
        )
    }
}

// Convert host UTF-8 into the UCS-2 subset supported by Mythroad.
fn normalized_utf16(text: &str) -> Vec<u16> {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter_map(|ch| {
            let value = ch as u32;
            if value == 0 {
                None
            } else if value <= u16::MAX as u32 {
                Some(value as u16)
            } else {
                Some(0xfffd)
            }
        })
        .collect()
}

fn is_high_surrogate(unit: u16) -> bool {
    (0xd800..=0xdbff).contains(&unit)
}

fn is_low_surrogate(unit: u16) -> bool {
    (0xdc00..=0xdfff).contains(&unit)
}

fn valid_boundary(text: &[u16], mut index: usize) -> usize {
    if index > 0
        && index < text.len()
        && is_high_surrogate(text[index - 1])
        && is_low_surrogate(text[index])
    {
        index -= 1;
    }
    index
}

fn previous_boundary(text: &[u16], index: usize) -> usize {
    if index == 0 {
        return 0;
    }
    let mut previous = index - 1;
    if previous > 0 && is_low_surrogate(text[previous]) && is_high_surrogate(text[previous - 1]) {
        previous -= 1;
    }
    previous
}

fn next_boundary(text: &[u16], index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    if index + 1 < text.len() && is_high_surrogate(text[index]) && is_low_surrogate(text[index + 1])
    {
        index + 2
    } else {
        index + 1
    }
}

fn unit_width(unit: u16) -> i32 {
    Font::edit_glyph_width(unit) as i32
}

fn text_width(text: &[u16]) -> i32 {
    text.iter().map(|unit| unit_width(*unit)).sum()
}

// Build visual lines while preserving explicit empty lines.
fn layout(text: &[u16], max_width: i32) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut index = 0;
    let mut width = 0;
    while index < text.len() {
        if text[index] == b'\n' as u16 {
            lines.push(Line {
                range: start..index,
                width,
            });
            index += 1;
            start = index;
            width = 0;
            continue;
        }
        let next = next_boundary(text, index);
        let glyph_width = unit_width(text[index]);
        if width + glyph_width > max_width && index > start {
            lines.push(Line {
                range: start..index,
                width,
            });
            start = index;
            width = 0;
            continue;
        }
        width += glyph_width;
        index = next;
    }
    lines.push(Line {
        range: start..text.len(),
        width,
    });
    lines
}

// A soft wrap has two visual cursor positions for the same text offset.
fn is_soft_wrap_start(lines: &[Line], line: usize, cursor: usize) -> bool {
    line > 0 && lines[line].range.start == cursor && lines[line - 1].range.end == cursor
}

fn cursor_position(text: &[u16], lines: &[Line], cursor: usize, downstream: bool) -> (usize, i32) {
    for (index, line) in lines.iter().enumerate() {
        if cursor <= line.range.end {
            if downstream
                && cursor == line.range.end
                && lines
                    .get(index + 1)
                    .is_some_and(|next| next.range.start == cursor)
            {
                continue;
            }
            return (
                index,
                text_width(&text[line.range.start..cursor.min(line.range.end)]),
            );
        }
    }
    let index = lines.len().saturating_sub(1);
    (index, lines[index].width)
}

fn index_at_x(text: &[u16], line: &Line, x: i32) -> usize {
    let mut index = line.range.start;
    let mut current_x = 0;
    while index < line.range.end {
        let width = unit_width(text[index]);
        if x < current_x + width / 2 {
            return index;
        }
        current_x += width;
        index = next_boundary(text, index);
    }
    line.range.end
}

#[allow(clippy::too_many_arguments)]
fn draw_line(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    font: &mut Font,
    fs: &Fs,
    text: &[u16],
    line: &Line,
    selection: Option<&Range<usize>>,
    composition: &Range<usize>,
    x: i32,
    y: i32,
) {
    let mut index = line.range.start;
    let mut draw_x = x;
    while index < line.range.end {
        let next = next_boundary(text, index);
        let width = unit_width(text[index]);
        if selection.is_some_and(|range| index < range.end && next > range.start) {
            fill_rect(
                frame,
                frame_width,
                frame_height,
                draw_x,
                y,
                width,
                GLYPH_HEIGHT,
                COLOR_SELECTION,
            );
        }
        if let Some(glyph) = font.edit_glyph(fs, text[index]) {
            draw_glyph(
                frame,
                frame_width,
                frame_height,
                &glyph,
                draw_x,
                y,
                COLOR_TEXT,
            );
        }
        if index < composition.end && next > composition.start {
            fill_rect(
                frame,
                frame_width,
                frame_height,
                draw_x,
                y + GLYPH_HEIGHT,
                width,
                1,
                COLOR_COMPOSITION,
            );
        }
        draw_x += width;
        index = next;
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_units(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    font: &mut Font,
    fs: &Fs,
    text: &[u16],
    mut x: i32,
    y: i32,
    color: u16,
    clip_right: Option<i32>,
) {
    let mut index = 0;
    while index < text.len() {
        let width = unit_width(text[index]);
        if clip_right.is_some_and(|right| x + width > right) {
            break;
        }
        if let Some(glyph) = font.edit_glyph(fs, text[index]) {
            draw_glyph(frame, frame_width, frame_height, &glyph, x, y, color);
        }
        x += width;
        index = next_boundary(text, index);
    }
}

fn draw_glyph(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    glyph: &GlyphBitmap,
    x: i32,
    y: i32,
    color: u16,
) {
    for dy in 0..glyph.height {
        for dx in 0..glyph.width {
            if Font::edit_glyph_bit(glyph, dx, dy) {
                set_pixel(
                    frame,
                    frame_width,
                    frame_height,
                    x + dx as i32,
                    y + dy as i32,
                    color,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_rect(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    color: u16,
) {
    for py in y.max(0)..(y + height).min(frame_height as i32) {
        for px in x.max(0)..(x + width).min(frame_width as i32) {
            set_pixel(frame, frame_width, frame_height, px, py, color);
        }
    }
}

fn set_pixel(frame: &mut [u8], frame_width: u32, frame_height: u32, x: i32, y: i32, color: u16) {
    if x < 0 || y < 0 || x >= frame_width as i32 || y >= frame_height as i32 {
        return;
    }
    let offset = (y as usize * frame_width as usize + x as usize) * 2;
    frame[offset..offset + 2].copy_from_slice(&color.to_ne_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editbox(text: &str, max_size: i32) -> EditBox {
        EditBox::new(
            Vec::new(),
            normalized_utf16(text),
            max_size,
            240,
            320,
            vec![0; 240 * 320 * 2],
        )
    }

    #[test]
    fn normalizes_line_endings_and_non_bmp_characters() {
        assert_eq!(
            normalized_utf16("a\r\nb\rc\u{1f600}"),
            vec![
                b'a' as u16,
                b'\n' as u16,
                b'b' as u16,
                b'\n' as u16,
                b'c' as u16,
                0xfffd
            ]
        );
    }

    #[test]
    fn insertion_replaces_selection_and_honors_limit() {
        let mut edit = editbox("abcd", 5);
        edit.anchor = 1;
        edit.cursor = 3;
        edit.insert_utf8("XYZ");
        assert_eq!(String::from_utf16_lossy(edit.text()), "aXYZd");
        assert_eq!(edit.cursor, 4);
    }

    #[test]
    fn layout_preserves_explicit_empty_lines() {
        let text = normalized_utf16("a\n\nb");
        let lines = layout(&text, 100);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].range, 0..1);
        assert_eq!(lines[1].range, 2..2);
        assert_eq!(lines[2].range, 3..4);
    }

    #[test]
    fn layout_wraps_at_glyph_boundaries() {
        let text = normalized_utf16("abcd");
        let lines = layout(&text, 16);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].range, 0..2);
        assert_eq!(lines[1].range, 2..4);
    }

    #[test]
    fn cursor_affinity_distinguishes_both_sides_of_soft_wrap() {
        let text = normalized_utf16("abcd");
        let lines = layout(&text, 16);

        assert_eq!(cursor_position(&text, &lines, 2, false), (0, 16));
        assert_eq!(cursor_position(&text, &lines, 2, true), (1, 0));
    }

    #[test]
    fn accepting_commits_active_composition() {
        let mut edit = editbox("ab", 8);
        edit.composition = normalized_utf16("中文");
        edit.composition_cursor = edit.composition.len();

        assert_eq!(edit.finish(EditResult::Ok), EditResult::Ok);
        assert_eq!(String::from_utf16_lossy(edit.text()), "ab中文");
        assert!(edit.composition.is_empty());
    }

    #[test]
    fn composition_replaces_selection_from_its_start() {
        let mut edit = editbox("abcdef", 16);
        edit.anchor = 2;
        edit.cursor = 4;

        edit.update_composition("zhong", 0, 0);

        assert_eq!(String::from_utf16_lossy(edit.text()), "abef");
        assert_eq!(edit.cursor, 2);
        assert_eq!(edit.anchor, 2);
        assert_eq!(String::from_utf16_lossy(&edit.composition), "zhong");
    }
}
