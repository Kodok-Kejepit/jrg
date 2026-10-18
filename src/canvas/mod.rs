// src/canvas/mod.rs
// Built-in Native Canvas Library

#![allow(dead_code, unused_imports)]

use std::sync::{Arc, Mutex};
use std::time::Instant;
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Colour — ARGB u32 (minifb format)
// ---------------------------------------------------------------------------

/// Warna dalam format ARGB u32 (0xAARRGGBB), compatible dengan minifb.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Colour {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Colour {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self { Colour { r, g, b, a: 255 } }
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self { Colour { r, g, b, a } }
    pub const BLACK:   Colour = Colour::rgb(0, 0, 0);
    pub const WHITE:   Colour = Colour::rgb(255, 255, 255);
    pub const RED:     Colour = Colour::rgb(255, 0, 0);
    pub const GREEN:   Colour = Colour::rgb(0, 255, 0);
    pub const BLUE:    Colour = Colour::rgb(0, 0, 255);
    pub const YELLOW:  Colour = Colour::rgb(255, 255, 0);
    pub const CYAN:    Colour = Colour::rgb(0, 255, 255);
    pub const MAGENTA: Colour = Colour::rgb(255, 0, 255);
    pub const ORANGE:  Colour = Colour::rgb(255, 165, 0);

    pub fn to_argb_u32(self) -> u32 {
        ((self.a as u32) << 24) | ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }

    pub fn from_argb_u32(v: u32) -> Self {
        Colour {
            a: ((v >> 24) & 0xFF) as u8,
            r: ((v >> 16) & 0xFF) as u8,
            g: ((v >>  8) & 0xFF) as u8,
            b: (v         & 0xFF) as u8,
        }
    }

    pub fn blend_over(self, bg: Colour) -> Colour {
        if self.a == 255 { return self; }
        if self.a == 0   { return bg; }
        let alpha = self.a as u32;
        let inv   = 255 - alpha;
        Colour {
            r: ((self.r as u32 * alpha + bg.r as u32 * inv) / 255) as u8,
            g: ((self.g as u32 * alpha + bg.g as u32 * inv) / 255) as u8,
            b: ((self.b as u32 * alpha + bg.b as u32 * inv) / 255) as u8,
            a: 255,
        }
    }
}

// ---------------------------------------------------------------------------
// Pixel-font 5×7
// ---------------------------------------------------------------------------

static FONT_5X7: &[u8] = &[
    0x00,0x00,0x00,0x00,0x00, // ' '
    0x00,0x00,0x5F,0x00,0x00, // '!'
    0x00,0x07,0x00,0x07,0x00, // '"'
    0x14,0x7F,0x14,0x7F,0x14, // '#'
    0x24,0x2A,0x7F,0x2A,0x12, // '$'
    0x23,0x13,0x08,0x64,0x62, // '%'
    0x36,0x49,0x55,0x22,0x50, // '&'
    0x00,0x05,0x03,0x00,0x00, // '''
    0x00,0x1C,0x22,0x41,0x00, // '('
    0x00,0x41,0x22,0x1C,0x00, // ')'
    0x14,0x08,0x3E,0x08,0x14, // '*'
    0x08,0x08,0x3E,0x08,0x08, // '+'
    0x00,0x50,0x30,0x00,0x00, // ','
    0x08,0x08,0x08,0x08,0x08, // '-'
    0x00,0x60,0x60,0x00,0x00, // '.'
    0x20,0x10,0x08,0x04,0x02, // '/'
    0x3E,0x51,0x49,0x45,0x3E, // '0'
    0x00,0x42,0x7F,0x40,0x00, // '1'
    0x42,0x61,0x51,0x49,0x46, // '2'
    0x21,0x41,0x45,0x4B,0x31, // '3'
    0x18,0x14,0x12,0x7F,0x10, // '4'
    0x27,0x45,0x45,0x45,0x39, // '5'
    0x3C,0x4A,0x49,0x49,0x30, // '6'
    0x01,0x71,0x09,0x05,0x03, // '7'
    0x36,0x49,0x49,0x49,0x36, // '8'
    0x06,0x49,0x49,0x29,0x1E, // '9'
    0x00,0x36,0x36,0x00,0x00, // ':'
    0x00,0x56,0x36,0x00,0x00, // ';'
    0x08,0x14,0x22,0x41,0x00, // '<'
    0x14,0x14,0x14,0x14,0x14, // '='
    0x00,0x41,0x22,0x14,0x08, // '>'
    0x02,0x01,0x51,0x09,0x06, // '?'
    0x32,0x49,0x79,0x41,0x3E, // '@'
    0x7E,0x11,0x11,0x11,0x7E, // 'A'
    0x7F,0x49,0x49,0x49,0x36, // 'B'
    0x3E,0x41,0x41,0x41,0x22, // 'C'
    0x7F,0x41,0x41,0x22,0x1C, // 'D'
    0x7F,0x49,0x49,0x49,0x41, // 'E'
    0x7F,0x09,0x09,0x09,0x01, // 'F'
    0x3E,0x41,0x49,0x49,0x7A, // 'G'
    0x7F,0x08,0x08,0x08,0x7F, // 'H'
    0x00,0x41,0x7F,0x41,0x00, // 'I'
    0x20,0x40,0x41,0x3F,0x01, // 'J'
    0x7F,0x08,0x14,0x22,0x41, // 'K'
    0x7F,0x40,0x40,0x40,0x40, // 'L'
    0x7F,0x02,0x0C,0x02,0x7F, // 'M'
    0x7F,0x04,0x08,0x10,0x7F, // 'N'
    0x3E,0x41,0x41,0x41,0x3E, // 'O'
    0x7F,0x09,0x09,0x09,0x06, // 'P'
    0x3E,0x41,0x51,0x21,0x5E, // 'Q'
    0x7F,0x09,0x19,0x29,0x46, // 'R'
    0x46,0x49,0x49,0x49,0x31, // 'S'
    0x01,0x01,0x7F,0x01,0x01, // 'T'
    0x3F,0x40,0x40,0x40,0x3F, // 'U'
    0x1F,0x20,0x40,0x20,0x1F, // 'V'
    0x3F,0x40,0x38,0x40,0x3F, // 'W'
    0x63,0x14,0x08,0x14,0x63, // 'X'
    0x07,0x08,0x70,0x08,0x07, // 'Y'
    0x61,0x51,0x49,0x45,0x43, // 'Z'
    0x00,0x7F,0x41,0x41,0x00, // '['
    0x02,0x04,0x08,0x10,0x20, // '\'
    0x00,0x41,0x41,0x7F,0x00, // ']'
    0x04,0x02,0x01,0x02,0x04, // '^'
    0x40,0x40,0x40,0x40,0x40, // '_'
    0x00,0x01,0x02,0x04,0x00, // '`'
    0x20,0x54,0x54,0x54,0x78, // 'a'
    0x7F,0x48,0x44,0x44,0x38, // 'b'
    0x38,0x44,0x44,0x44,0x20, // 'c'
    0x38,0x44,0x44,0x48,0x7F, // 'd'
    0x38,0x54,0x54,0x54,0x18, // 'e'
    0x08,0x7E,0x09,0x01,0x02, // 'f'
    0x0C,0x52,0x52,0x52,0x3E, // 'g'
    0x7F,0x08,0x04,0x04,0x78, // 'h'
    0x00,0x44,0x7D,0x40,0x00, // 'i'
    0x20,0x40,0x44,0x3D,0x00, // 'j'
    0x7F,0x10,0x28,0x44,0x00, // 'k'
    0x00,0x41,0x7F,0x40,0x00, // 'l'
    0x7C,0x04,0x18,0x04,0x78, // 'm'
    0x7C,0x08,0x04,0x04,0x78, // 'n'
    0x38,0x44,0x44,0x44,0x38, // 'o'
    0x7C,0x14,0x14,0x14,0x08, // 'p'
    0x08,0x14,0x14,0x18,0x7C, // 'q'
    0x7C,0x08,0x04,0x04,0x08, // 'r'
    0x48,0x54,0x54,0x54,0x20, // 's'
    0x04,0x3F,0x44,0x40,0x20, // 't'
    0x3C,0x40,0x40,0x20,0x7C, // 'u'
    0x1C,0x20,0x40,0x20,0x1C, // 'v'
    0x3C,0x40,0x30,0x40,0x3C, // 'w'
    0x44,0x28,0x10,0x28,0x44, // 'x'
    0x0C,0x50,0x50,0x50,0x3C, // 'y'
    0x44,0x64,0x54,0x4C,0x44, // 'z'
    0x00,0x08,0x36,0x41,0x00, // '{'
    0x00,0x00,0x7F,0x00,0x00, // '|'
    0x00,0x41,0x36,0x08,0x00, // '}'
    0x08,0x08,0x2A,0x1C,0x08, // '~'
];

const FONT_CHAR_W: i32 = 6;
const FONT_CHAR_H: i32 = 8;

fn draw_glyph(fb: &mut [u32], fb_w: i32, fb_h: i32, x: i32, y: i32, ch: char, col: Colour) {
    let code = ch as u32;
    if code < 0x20 || code > 0x7E { return; }
    let idx = ((code - 0x20) as usize) * 5;
    let bytes = &FONT_5X7[idx..idx + 5];
    for col_x in 0..5i32 {
        let byte = bytes[col_x as usize];
        for row_y in 0..7i32 {
            if byte & (1 << row_y) != 0 {
                let px = x + col_x;
                let py = y + row_y;
                if px >= 0 && px < fb_w && py >= 0 && py < fb_h {
                    fb[(py * fb_w + px) as usize] = col.to_argb_u32();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Framebuffer
// ---------------------------------------------------------------------------

pub struct Framebuffer {
    pub width:  usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

impl Framebuffer {
    pub fn new(width: usize, height: usize) -> Self {
        Framebuffer {
            width,
            height,
            pixels: vec![0xFF000000u32; width * height],
        }
    }

    #[inline]
    fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height
    }

    #[inline]
    pub fn set_pixel(&mut self, x: i32, y: i32, col: Colour) {
        if self.in_bounds(x, y) {
            let idx = y as usize * self.width + x as usize;
            let bg = Colour::from_argb_u32(self.pixels[idx]);
            self.pixels[idx] = col.blend_over(bg).to_argb_u32();
        }
    }

    #[inline]
    pub fn get_pixel(&self, x: i32, y: i32) -> Colour {
        if self.in_bounds(x, y) {
            Colour::from_argb_u32(self.pixels[y as usize * self.width + x as usize])
        } else {
            Colour::BLACK
        }
    }

    pub fn clear(&mut self, col: Colour) {
        self.pixels.fill(col.to_argb_u32());
    }

    pub fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, col: Colour) {
        let (mut x, mut y) = (x0, y0);
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx: i32 = if x0 < x1 { 1 } else { -1 };
        let sy: i32 = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            self.set_pixel(x, y, col);
            if x == x1 && y == y1 { break; }
            let e2 = 2 * err;
            if e2 >= dy { if x == x1 { break; } err += dy; x += sx; }
            if e2 <= dx { if y == y1 { break; } err += dx; y += sy; }
        }
    }

    pub fn draw_rect(&mut self, x: i32, y: i32, w: i32, h: i32, col: Colour) {
        self.draw_line(x,     y,     x+w-1, y,     col);
        self.draw_line(x,     y+h-1, x+w-1, y+h-1, col);
        self.draw_line(x,     y,     x,     y+h-1, col);
        self.draw_line(x+w-1, y,     x+w-1, y+h-1, col);
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, col: Colour) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.width as i32);
        let y1 = (y + h).min(self.height as i32);
        let fill = col.to_argb_u32();
        for py in y0..y1 {
            let row_start = py as usize * self.width;
            if col.a == 255 {
                for px in x0..x1 {
                    self.pixels[row_start + px as usize] = fill;
                }
            } else {
                for px in x0..x1 {
                    let idx = row_start + px as usize;
                    let existing = Colour::from_argb_u32(self.pixels[idx]);
                    self.pixels[idx] = col.blend_over(existing).to_argb_u32();
                }
            }
        }
    }

    pub fn draw_circle(&mut self, cx: i32, cy: i32, r: i32, col: Colour) {
        if r <= 0 { self.set_pixel(cx, cy, col); return; }
        let (mut x, mut y) = (0i32, r);
        let mut d = 3 - 2 * r;
        self.plot_circle_8(cx, cy, x, y, col);
        while y >= x {
            x += 1;
            if d > 0 { y -= 1; d += 4 * (x - y) + 10; }
            else      { d += 4 * x + 6; }
            self.plot_circle_8(cx, cy, x, y, col);
        }
    }

    fn plot_circle_8(&mut self, cx: i32, cy: i32, x: i32, y: i32, col: Colour) {
        let pts = [
            (cx+x, cy+y), (cx-x, cy+y), (cx+x, cy-y), (cx-x, cy-y),
            (cx+y, cy+x), (cx-y, cy+x), (cx+y, cy-x), (cx-y, cy-x),
        ];
        for (px, py) in pts { self.set_pixel(px, py, col); }
    }

    pub fn fill_circle(&mut self, cx: i32, cy: i32, r: i32, col: Colour) {
        if r <= 0 { self.set_pixel(cx, cy, col); return; }
        let (mut x, mut y) = (0i32, r);
        let mut d = 3 - 2 * r;
        self.fill_circle_hline(cx, cy, x, y, col);
        while y >= x {
            x += 1;
            if d > 0 { y -= 1; d += 4 * (x - y) + 10; }
            else      { d += 4 * x + 6; }
            self.fill_circle_hline(cx, cy, x, y, col);
        }
    }

    fn fill_circle_hline(&mut self, cx: i32, cy: i32, x: i32, y: i32, col: Colour) {
        self.fill_rect(cx - x, cy + y, 2 * x, 1, col);
        self.fill_rect(cx - x, cy - y, 2 * x, 1, col);
        self.fill_rect(cx - y, cy + x, 2 * y, 1, col);
        self.fill_rect(cx - y, cy - x, 2 * y, 1, col);
    }

    pub fn draw_triangle(
        &mut self,
        x0: i32, y0: i32,
        x1: i32, y1: i32,
        x2: i32, y2: i32,
        col: Colour,
    ) {
        self.draw_line(x0, y0, x1, y1, col);
        self.draw_line(x1, y1, x2, y2, col);
        self.draw_line(x2, y2, x0, y0, col);
    }

    pub fn fill_triangle(
        &mut self,
        x0: i32, y0: i32,
        x1: i32, y1: i32,
        x2: i32, y2: i32,
        col: Colour,
    ) {
        let mut pts = [(x0,y0),(x1,y1),(x2,y2)];
        pts.sort_by_key(|p| p.1);
        let (ax,ay) = pts[0];
        let (bx,by) = pts[1];
        let (cx,cy) = pts[2];
        let total_h = cy - ay;
        if total_h == 0 { return; }
        for y in ay..=cy {
            let second_half = y >= by;
            let seg_h = if second_half { cy - by } else { by - ay };
            let seg_h = if seg_h == 0 { 1 } else { seg_h };
            let alpha = (y - ay) as f32 / total_h as f32;
            let beta  = if second_half {
                (y - by) as f32 / seg_h as f32
            } else {
                (y - ay) as f32 / seg_h as f32
            };
            let mut xa = ax as f32 + (cx - ax) as f32 * alpha;
            let mut xb = if second_half {
                bx as f32 + (cx - bx) as f32 * beta
            } else {
                ax as f32 + (bx - ax) as f32 * beta
            };
            if xa > xb { std::mem::swap(&mut xa, &mut xb); }
            for x in (xa as i32)..=(xb as i32) {
                self.set_pixel(x, y, col);
            }
        }
    }

    pub fn draw_text(&mut self, x: i32, y: i32, text: &str, col: Colour) {
        let w = self.width as i32;
        let h = self.height as i32;
        let mut cur_x = x;
        for ch in text.chars() {
            if ch == '\n' { continue; }
            draw_glyph(&mut self.pixels, w, h, cur_x, y, ch, col);
            cur_x += FONT_CHAR_W;
        }
    }

    /// Teks yang diperbesar, tiap piksel font jadi kotak `skala x skala`.
    pub fn draw_text_scaled(&mut self, x: i32, y: i32, text: &str, skala: i32, col: Colour) {
        let skala = skala.max(1);
        let mut cur_x = x;
        for ch in text.chars() {
            if ch == '\n' { continue; }
            let code = ch as u32;
            if (0x20..=0x7E).contains(&code) {
                let idx = ((code - 0x20) as usize) * 5;
                let bytes: [u8; 5] = [
                    FONT_5X7[idx], FONT_5X7[idx + 1], FONT_5X7[idx + 2],
                    FONT_5X7[idx + 3], FONT_5X7[idx + 4],
                ];
                for cx in 0..5i32 {
                    for ry in 0..7i32 {
                        if bytes[cx as usize] & (1 << ry) != 0 {
                            self.fill_rect(cur_x + cx * skala, y + ry * skala, skala, skala, col);
                        }
                    }
                }
            }
            cur_x += FONT_CHAR_W * skala;
        }
    }

    pub fn text_width(text: &str) -> i32 {
        text.len() as i32 * FONT_CHAR_W
    }

    pub fn save_ppm(&self, path: &str) -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::File::create(path)?;
        write!(f, "P6\n{} {}\n255\n", self.width, self.height)?;
        let mut data = Vec::with_capacity(self.width * self.height * 3);
        for &px in &self.pixels {
            data.push(((px >> 16) & 0xFF) as u8);
            data.push(((px >>  8) & 0xFF) as u8);
            data.push((px         & 0xFF) as u8);
        }
        f.write_all(&data)
    }

    pub fn save_bmp(&self, path: &str) -> std::io::Result<()> {
        use std::io::Write;
        let row_size = ((self.width * 3 + 3) / 4) * 4;
        let pixel_data_size = row_size * self.height;
        let file_size = 54 + pixel_data_size;
        let mut f = std::fs::File::create(path)?;
        f.write_all(b"BM")?;
        f.write_all(&(file_size as u32).to_le_bytes())?;
        f.write_all(&0u32.to_le_bytes())?;
        f.write_all(&54u32.to_le_bytes())?;
        f.write_all(&40u32.to_le_bytes())?;
        f.write_all(&(self.width as i32).to_le_bytes())?;
        f.write_all(&(-(self.height as i32)).to_le_bytes())?;
        f.write_all(&1u16.to_le_bytes())?;
        f.write_all(&24u16.to_le_bytes())?;
        f.write_all(&0u32.to_le_bytes())?;
        f.write_all(&(pixel_data_size as u32).to_le_bytes())?;
        f.write_all(&2835i32.to_le_bytes())?;
        f.write_all(&2835i32.to_le_bytes())?;
        f.write_all(&0u32.to_le_bytes())?;
        f.write_all(&0u32.to_le_bytes())?;
        let mut row_buf = vec![0u8; row_size];
        for y in 0..self.height {
            for x in 0..self.width {
                let px = self.pixels[y * self.width + x];
                let r = ((px >> 16) & 0xFF) as u8;
                let g = ((px >>  8) & 0xFF) as u8;
                let b = (px         & 0xFF) as u8;
                let i = x * 3;
                row_buf[i]   = b;
                row_buf[i+1] = g;
                row_buf[i+2] = r;
            }
            for i in (self.width * 3)..row_size { row_buf[i] = 0; }
            f.write_all(&row_buf)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Key state tracking
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct KeyState {
    pub pressed:  HashSet<String>,
    pub held:     HashSet<String>,
}

impl KeyState {
    pub fn is_pressed(&self, key: &str) -> bool { self.pressed.contains(key) }
    pub fn is_held(&self,    key: &str) -> bool { self.held.contains(key) }
}

// ---------------------------------------------------------------------------
// CanvasInner
// ---------------------------------------------------------------------------

pub mod lanjut;
pub mod pena;
pub mod png;
pub mod png_baca;
#[cfg(feature = "canvas")]
#[cfg_attr(target_os = "android", path = "jendela_android.rs")]
pub mod jendela;
#[cfg(not(target_arch = "wasm32"))]
pub mod web;

/// Mode tampilan yang sah untuk `Canvas::new(.., mode)`; `"default"` ejaan lain `"polos"`.
pub const MODE_KANVAS: &[&str] = &["polos", "tab", "web", "png"];

pub struct CanvasInner {
    pub width:      usize,
    pub height:     usize,
    pub title:      String,
    pub fb:         Framebuffer,
    pub is_open:    bool,
    pub keys:       KeyState,
    pub mouse_x:    f32,
    pub mouse_y:    f32,
    pub mouse_btns: [bool; 3],
    /// Jari yang menempel bingkai ini (koordinat kanvas); di desktop: tetikus kiri.
    pub sentuhan:   Vec<(f32, f32)>,
    pub frame_idx:  u64,
    pub last_tick:  Instant,
    pub delta_sec:  f64,
    pub fps:        f64,
    pub headless_export_path: Option<String>,
    /// Transformasi aktif dan kotak pemotong. Dipisah dari framebuffer
    /// supaya `push`/`pop` tidak berarti menyalin seluruh gambar.
    pub st: lanjut::Keadaan,
    #[cfg(feature = "canvas")]
    pub window: Option<jendela::Jendela>,
    /// Mode `"png"`: tiap `update` menimpa berkas ini dengan bingkainya.
    pub png_berkas: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    pub web: Option<web::Web>,
}

impl std::fmt::Debug for CanvasInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CanvasInner({}x{}, open={}, frame={})",
            self.width, self.height, self.is_open, self.frame_idx)
    }
}

impl CanvasInner {
    pub fn new(width: usize, height: usize, title: &str) -> Self {
        CanvasInner {
            width,
            height,
            title: title.to_string(),
            fb:    Framebuffer::new(width, height),
            is_open: true,
            keys:  KeyState::default(),
            mouse_x: 0.0,
            mouse_y: 0.0,
            mouse_btns: [false; 3],
            sentuhan: Vec::new(),
            frame_idx: 0,
            last_tick: Instant::now(),
            delta_sec: 0.0,
            fps: 0.0,
            headless_export_path: None,
            st: lanjut::Keadaan::default(),
            #[cfg(feature = "canvas")]
            window: None,
            png_berkas: None,
            #[cfg(not(target_arch = "wasm32"))]
            web: None,
        }
    }
}

pub type CanvasHandle = Arc<Mutex<CanvasInner>>;

pub fn canvas_new(width: usize, height: usize, title: &str) -> CanvasHandle {
    Arc::new(Mutex::new(CanvasInner::new(width, height, title)))
}

/// `Canvas::new(lebar, tinggi, judul?)` — SATU definisi untuk dua mesin. Dulu
/// dua salinan diam-diam mengubah ukuran 0 atau negatif jadi 1, dan ukuran
/// raksasa mematikan prosesnya lewat alokasi yang gagal.
pub fn canvas_baru(args: &[crate::interpreter::Value]) -> Result<CanvasHandle, String> {
    use crate::interpreter::Value;
    if args.len() < 2 {
        return Err("Canvas::new butuh minimal 2 arg: Canvas::new(lebar, tinggi) \
                    atau Canvas::new(lebar, tinggi, \"judul\")".to_string());
    }
    let mut asli = [0i128; 2];
    for (i, nama) in [(0usize, "lebar"), (1, "tinggi")] {
        let n = match &args[i] {
            Value::Int(n) => *n,
            Value::Float(f) if f.is_finite() => f.trunc() as i128,
            Value::Float(f) => return Err(format!("Canvas::new: {} harus angka yang terhingga, dapat `{}`",
                                                   nama, Value::Float(*f))),
            lain => return Err(pesan_ukuran_bukan_angka(nama, &lain.type_name())),
        };
        if n < 1 {
            return Err(format!("Canvas::new: {} harus paling sedikit 1, dapat {}", nama, n));
        }
        asli[i] = n;
    }
    // Ukuran yang DIMINTA yang disebut: dulu dijepit dulu, jadi 2^40 x 2 dilaporkan 16777216x2.
    let piksel = asli[0].checked_mul(asli[1]);
    if piksel.map_or(true, |p| p > crate::interpreter::MAKS_ELEMEN) {
        return Err(format!(
            "Canvas::new: kanvas {}x{} butuh {} piksel, batasnya {} — sebesar itu \
             mematikan prosesnya sebelum sempat menjawab. Cek lagi ukurannya",
            asli[0], asli[1], piksel.map_or("lebih dari 10^38".to_string(), |p| p.to_string()),
            crate::interpreter::MAKS_ELEMEN));
    }
    let ukuran = [asli[0] as usize, asli[1] as usize];
    // Judul adalah teks yang DITAMPILKAN, jadi nilai apa pun dirender.
    let judul = match args.get(2) {
        Some(Value::Str(s)) => s.clone(),
        Some(v) => format!("{}", v),
        None => "JRG Canvas".to_string(),
    };
    let mode = mode_kanvas(args.get(3))?;
    let handle = canvas_new(ukuran[0], ukuran[1], &judul);
    {
        let mut g = handle.lock().map_err(|e| format!("canvas lock: {}", e))?;
        for m in &mode {
            match *m {
                // Gagal membuka jendela DILAPORKAN — dulu `.ok()` menelannya dan `update` pertama
                // menutup kanvas tanpa pesan.
                "tab" => {
                    #[cfg(feature = "canvas")]
                    { g.window = Some(jendela::Jendela::buka(&judul, ukuran[0], ukuran[1])
                        .map_err(|e| format!("Canvas::new: {}", e))?); }
                    #[cfg(not(feature = "canvas"))]
                    return Err("Canvas::new: mode `\"tab\"` butuh jendela, dan biner ini dibangun tanpa \
                                fitur `canvas` — bangun dengan `cargo build --release` (fitur bawaan) \
                                atau pakai mode `\"web\"`/`\"png\"`".to_string());
                }
                "web" => {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let w = web::Web::buka(&judul, ukuran[0], ukuran[1]).map_err(|e| format!("Canvas::new: {}", e))?;
                        crate::host::galat_baris(&format!("kanvas `{}` tampil di {}", judul, w.alamat));
                        g.web = Some(w);
                    }
                    #[cfg(target_arch = "wasm32")]
                    return Err("Canvas::new: mode `\"web\"` tidak tersedia di peramban".to_string());
                }
                "png" => g.png_berkas = Some(nama_berkas_png(&judul)),
                _ => {}
            }
        }
    }
    Ok(handle)
}

/// Mode dari argumen ke-4: teks atau array teks; tanpa argumen, `"polos"`.
fn mode_kanvas(v: Option<&crate::interpreter::Value>) -> Result<Vec<&'static str>, String> {
    use crate::interpreter::Value;
    let satu = |s: &str| -> Result<&'static str, String> {
        let s = if s == "default" { "polos" } else { s };
        MODE_KANVAS.iter().copied().find(|m| *m == s).ok_or_else(|| {
            let saran = crate::diag::saran_nama_terdekat(s, MODE_KANVAS.iter().copied())
                .map(|x| format!(" — maksudnya `\"{}\"`?", x)).unwrap_or_default();
            format!("Canvas::new: mode `\"{}\"` tidak dikenal{}\nbantuan: mode yang ada `\"polos\"` \
                     (bawaan, tanpa layar), `\"tab\"` (jendela desktop), `\"web\"` (peramban lokal), \
                     `\"png\"` (berkas tiap bingkai); beberapa sekaligus ditulis sebagai array", s, saran)
        })
    };
    match v {
        None => Ok(vec!["polos"]),
        Some(Value::Str(s)) => Ok(vec![satu(s)?]),
        Some(Value::Array(a)) if !a.is_empty() => {
            let mut out = Vec::new();
            for x in a.iter() {
                let Value::Str(s) = x else {
                    return Err(format!("Canvas::new: tiap mode harus teks, dapat `{}`", x.type_name()));
                };
                let m = satu(s)?;
                if !out.contains(&m) { out.push(m); }
            }
            Ok(out)
        }
        Some(lain) => Err(format!("Canvas::new: mode harus teks atau array teks yang tidak kosong, dapat `{}`",
                                  lain.type_name())),
    }
}

/// `"Tangkap Bintang"` -> `tangkap_bintang.png`, di direktori kerja.
fn nama_berkas_png(judul: &str) -> String {
    let dasar: String = judul.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    let dasar = dasar.trim_matches('_');
    format!("{}.png", if dasar.is_empty() { "kanvas" } else { dasar })
}

// ---------------------------------------------------------------------------
// canvas_update
// ---------------------------------------------------------------------------

pub type CanvasResult<T> = Result<T, String>;

pub fn canvas_update(handle: &CanvasHandle) -> CanvasResult<bool> {
    let mut inner = handle.lock().map_err(|e| format!("canvas lock: {}", e))?;
    if !inner.is_open { return Ok(false); }

    let now = Instant::now();
    let elapsed = now.duration_since(inner.last_tick).as_secs_f64();
    inner.delta_sec = elapsed;
    inner.fps = if elapsed > 0.0 { 1.0 / elapsed } else { 0.0 };
    inner.last_tick = now;
    inner.frame_idx += 1;
    inner.keys.pressed.clear();

    // Tiap mode lapisan tampilan atas framebuffer yang SAMA; masukannya digabung.
    let mut ditahan: HashSet<String> = HashSet::new();
    let mut ada_masukan = false;
    let mut jari: Option<Vec<(f32, f32)>> = None;
    #[cfg(feature = "canvas")]
    if inner.window.is_some() {
        let laporan = match inner.window { Some(ref j) => j.tampilkan(&inner.fb.pixels)?, None => unreachable!() };
        if let Some(e) = laporan.galat { return Err(e); }
        if !laporan.terbuka {
            inner.is_open = false;
            inner.window = None;
            return Ok(false);
        }
        inner.keys.pressed.extend(laporan.ditekan);
        ditahan.extend(laporan.ditahan);
        ada_masukan = true;
        if let Some((mx, my)) = laporan.tetikus {
            inner.mouse_x = mx;
            inner.mouse_y = my;
        }
        inner.mouse_btns = laporan.tombol_tetikus;
        if laporan.sentuhan.is_some() { jari = laporan.sentuhan; }
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(w) = &inner.web {
        w.kirim(&inner.fb.pixels);
        let (tekan, tahan, tetikus, tombol) = {
            let mut b = w.bersama.lock().unwrap_or_else(|p| p.into_inner());
            (std::mem::take(&mut b.ditekan), b.ditahan.clone(), b.tetikus, b.tombol_tetikus)
        };
        // `Escape` menutup, sama dengan jendela `"tab"`.
        if tahan.contains("Escape") { inner.is_open = false; return Ok(false); }
        inner.keys.pressed.extend(tekan);
        ditahan.extend(tahan);
        if !ada_masukan { inner.mouse_x = tetikus.0; inner.mouse_y = tetikus.1; inner.mouse_btns = tombol; }
        ada_masukan = true;
    }
    if ada_masukan {
        inner.keys.held = ditahan;
        // Tanpa layar sentuh, tombol kiri tetikus dihitung satu jari: satu kode untuk HP dan desktop.
        inner.sentuhan = jari.unwrap_or_else(|| {
            if inner.mouse_btns[0] { vec![(inner.mouse_x, inner.mouse_y)] } else { Vec::new() }
        });
    }
    if let Some(berkas) = inner.png_berkas.clone() {
        let isi = png::susun(inner.width, inner.height, &inner.fb.pixels);
        std::fs::write(&berkas, isi).map_err(|e| format!("mode `png`: gagal menulis '{}': {}", berkas, crate::host::galat_io(&e)))?;
    }

    {
        if let Some(ref base_path) = inner.headless_export_path.clone() {
            let path = format!("{}_{:06}.ppm", base_path, inner.frame_idx);
            // Gagal tulis dilaporkan: dulu `.ok()` menelannya, jadi bingkainya hilang diam-diam.
            inner.fb.save_ppm(&path)
                .map_err(|e| format!("set_headless_export: gagal menulis '{}': {}", path, crate::host::galat_io(&e)))?;
        }
    }

    Ok(inner.is_open)
}

// ---------------------------------------------------------------------------
// Key queries
// ---------------------------------------------------------------------------

/// Nama tombol yang bisa ditanyakan — persis nama varian `minifb::Key`, karena
/// itu yang diisi `canvas_update` ke `KeyState`. Satu kosakata untuk dua jalur.
pub const TOMBOL: &[&str] = &[
    "Key0", "Key1", "Key2", "Key3", "Key4", "Key5", "Key6", "Key7", "Key8", "Key9",
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P",
    "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
    "F13", "F14", "F15",
    "Down", "Left", "Right", "Up", "Apostrophe", "Backquote", "Backslash", "Comma",
    "Equal", "LeftBracket", "Minus", "Period", "RightBracket", "Semicolon", "Slash",
    "Backspace", "Delete", "End", "Enter", "Escape", "Home", "Insert", "Menu",
    "PageDown", "PageUp", "Pause", "Space", "Tab", "NumLock", "CapsLock",
    "ScrollLock", "LeftShift", "RightShift", "LeftCtrl", "RightCtrl",
    "NumPad0", "NumPad1", "NumPad2", "NumPad3", "NumPad4", "NumPad5", "NumPad6",
    "NumPad7", "NumPad8", "NumPad9", "NumPadDot", "NumPadSlash", "NumPadAsterisk",
    "NumPadMinus", "NumPadPlus", "NumPadEnter", "LeftAlt", "RightAlt",
    "LeftSuper", "RightSuper",
];

/// Ejaan lain yang biasa ditulis orang, dan nama kanoniknya.
const TOMBOL_ALIAS: &[(&str, &str)] = &[
    (" ", "Space"), ("ArrowLeft", "Left"), ("ArrowRight", "Right"),
    ("ArrowUp", "Up"), ("ArrowDown", "Down"), ("Return", "Enter"), ("Esc", "Escape"),
    ("Shift", "LeftShift"), ("Ctrl", "LeftCtrl"), ("Control", "LeftCtrl"),
    ("Alt", "LeftAlt"), ("0", "Key0"), ("1", "Key1"), ("2", "Key2"), ("3", "Key3"),
    ("4", "Key4"), ("5", "Key5"), ("6", "Key6"), ("7", "Key7"), ("8", "Key8"),
    ("9", "Key9"),
];

/// Apakah biner ini bisa membuka jendela sungguhan (`--features canvas`). Tanpanya,
/// `canvas::update` selalu `true` dan putaran game berputar tanpa layar.
/// Apakah kanvas di sini disentuh jari (APK Android), bukan diklik tetikus.
pub fn layar_sentuh() -> bool {
    #[cfg(all(feature = "canvas", target_os = "android"))]
    { jendela::ada_layar() }
    #[cfg(not(all(feature = "canvas", target_os = "android")))]
    { false }
}

pub fn punya_jendela() -> bool {
    // Fitur jendelanya ada DAN ada layar: tanpa layar, contoh game memakai `simulasi`.
    let ada = |v: &str| std::env::var_os(v).map_or(false, |x| !x.is_empty());
    #[cfg(all(feature = "canvas", target_os = "android"))]
    { let _ = ada; jendela::ada_layar() }
    #[cfg(not(all(feature = "canvas", target_os = "android")))]
    { cfg!(feature = "canvas") && (ada("DISPLAY") || ada("WAYLAND_DISPLAY")) }
}

/// Nama kanonik sebuah tombol, tanpa peduli huruf besar-kecil. Nama yang tidak
/// dikenal DITOLAK: dulu ia dijawab `false` selamanya, dan tak ada yang bilang.
pub fn kenali_tombol(nama: &str) -> Result<&'static str, String> {
    if let Some(t) = TOMBOL.iter().find(|t| t.eq_ignore_ascii_case(nama)) {
        return Ok(t);
    }
    if let Some((_, t)) = TOMBOL_ALIAS.iter().find(|(a, _)| a.eq_ignore_ascii_case(nama)) {
        return Ok(t);
    }
    let calon = TOMBOL.iter().copied().chain(TOMBOL_ALIAS.iter().map(|(a, _)| *a));
    let saran = crate::diag::saran_nama_terdekat(nama, calon)
        .map(|s| format!(" — maksudnya `{}`?", s)).unwrap_or_default();
    Err(format!(
        "`{}` bukan nama tombol yang dikenal{}\n\
         bantuan: nama tombol misalnya `Space`, `Enter`, `Escape`, `Left`/`Right`/`Up`/`Down`, \
         huruf `A`-`Z`, angka `0`-`9`, `F1`-`F15`, `LeftShift`, `LeftCtrl` \
         (huruf besar-kecil tidak berpengaruh)", nama, saran))
}

pub fn canvas_key_pressed(handle: &CanvasHandle, key: &str) -> CanvasResult<bool> {
    let inner = handle.lock().map_err(|e| format!("canvas lock: {}", e))?;
    Ok(inner.keys.pressed.contains(kenali_tombol(key)?))
}

pub fn canvas_key_held(handle: &CanvasHandle, key: &str) -> CanvasResult<bool> {
    let inner = handle.lock().map_err(|e| format!("canvas lock: {}", e))?;
    Ok(inner.keys.held.contains(kenali_tombol(key)?))
}

// ---------------------------------------------------------------------------
// Value ↔ Colour helpers — dipakai oleh interpreter dan dispatch
// ---------------------------------------------------------------------------

/// Pesan untuk ukuran kanvas yang bukan angka.
pub fn pesan_ukuran_bukan_angka(nama_arg: &str, tipe: &str) -> String {
    format!("Canvas::new: {} harus angka, dapat `{}`", nama_arg, tipe)
}

/// Warna dari nilai JRG: `0xRRGGBB` (opak), `0xAARRGGBB`, atau struct/dict
/// ber-field `r`, `g`, `b` (dan `a`) masing-masing 0 sampai 255. Yang lain DITOLAK
/// — dulu jadi putih atau hitam diam-diam, dan `r: 300` terbungkus jadi 44.
pub fn value_to_colour(v: &crate::interpreter::Value) -> Result<Colour, String> {
    use crate::interpreter::Value;
    match v {
        Value::Struct { fields, .. } => {
            let komponen = |k: &str, bawaan: Option<u8>| -> Result<u8, String> {
                match (fields.get(k), bawaan) {
                    (Some(Value::Int(n)), _) if (0..=255).contains(n) => Ok(*n as u8),
                    (Some(lain), _) => Err(format!(
                        "berisi komponen `{}` {} — tiap komponen warna harus bilangan bulat \
                         0 sampai 255", k,
                        match lain { Value::Int(n) => n.to_string(), l => format!("bertipe `{}`", l.type_name()) })),
                    (None, Some(b)) => Ok(b),
                    (None, None) => Err(format!(
                        "tidak punya komponen `{}` — warna butuh `r`, `g`, `b` (dan `a` opsional)", k)),
                }
            };
            Ok(Colour { r: komponen("r", None)?, g: komponen("g", None)?,
                        b: komponen("b", None)?, a: komponen("a", Some(255))? })
        }
        // `0xRRGGBB` berarti OPAK; `0xAARRGGBB` memakai alphanya sendiri.
        Value::Int(n) if (0..=0xFFFF_FFFF).contains(n) => {
            let n = *n as u32;
            Ok(if n <= 0xFF_FFFF { Colour::rgb((n >> 16) as u8, (n >> 8) as u8, n as u8) }
               else { Colour::from_argb_u32(n) })
        }
        Value::Int(n) => Err(format!("harus warna 0 sampai 0xFFFFFFFF, dapat {}", n)),
        lain => Err(format!(
            "harus warna — angka `0xRRGGBB`/`0xAARRGGBB` atau `Warna {{ r, g, b, a }}` — dapat `{}`",
            lain.type_name())),
    }
}

pub fn colour_to_value(c: Colour) -> crate::interpreter::Value {
    let mut fields = std::collections::HashMap::new();
    fields.insert("r".to_string(), crate::interpreter::Value::Int(c.r as i128));
    fields.insert("g".to_string(), crate::interpreter::Value::Int(c.g as i128));
    fields.insert("b".to_string(), crate::interpreter::Value::Int(c.b as i128));
    fields.insert("a".to_string(), crate::interpreter::Value::Int(c.a as i128));
    crate::interpreter::Value::Struct { name: "Warna".to_string(), fields: fields.into() }
}

/// Koordinat/ukuran bulat: bilangan bulat DIJENUHKAN ke jangkauan `i32` (dulu
/// terbungkus, jadi `2^32` jatuh di 0), pecahan terhingga dipotong, sisanya ditolak.
pub fn angka_i32(v: &crate::interpreter::Value) -> Result<i32, String> {
    match v {
        crate::interpreter::Value::Int(n) => Ok((*n).clamp(i32::MIN as i128, i32::MAX as i128) as i32),
        _ => angka_f64(v).map(|f| f as i32),
    }
}

/// Angka untuk menggambar. NaN dan tak hingga ditolak: dulu jadi 0 tanpa suara.
pub fn angka_f64(v: &crate::interpreter::Value) -> Result<f64, String> {
    use crate::interpreter::Value;
    match v {
        Value::Float(f) if f.is_finite() => Ok(*f),
        Value::Float(f) => Err(format!("harus angka yang terhingga, dapat `{}`", Value::Float(*f))),
        Value::Int(n) => Ok(*n as f64),
        lain => Err(format!("harus angka, dapat `{}`", lain.type_name())),
    }
}

// ---------------------------------------------------------------------------
// dispatch_canvas_method — entry point dari interpreter eval_method
// ---------------------------------------------------------------------------

pub fn dispatch_canvas_method(
    handle: CanvasHandle,
    method: &str,
    args: &[crate::interpreter::Value],
) -> crate::interpreter::EvalResult {
    use crate::interpreter::{Value, Signal, RuntimeError};

    macro_rules! cerr {
        ($msg:expr) => {
            return Err(Signal::RuntimeError(RuntimeError::new($msg)))
        };
        ($fmt:expr, $($arg:tt)*) => {
            return Err(Signal::RuntimeError(RuntimeError::new(format!($fmt, $($arg)*))))
        };
    }
    macro_rules! lock {
        ($inner:ident, mut) => {
            let mut $inner = handle.lock()
                .map_err(|e| Signal::RuntimeError(RuntimeError::new(format!("canvas lock: {}", e))))?;
        };
        ($inner:ident) => {
            let $inner = handle.lock()
                .map_err(|e| Signal::RuntimeError(RuntimeError::new(format!("canvas lock: {}", e))))?;
        };
    }

    // Satu pintu piksel (`pena`) yang mematuhi transformasi dan clip.
    macro_rules! pena {
        ($g:ident) => { pena::Pena { fb: &mut $g.fb, st: &$g.st } };
    }

    // Argumen yang salah TIPE ditolak dengan satu kalimat (dulu jadi 0 atau putih
    // tanpa suara). Yang HILANG diberi bawaan, dan itu aman hanya karena
    // jumlahnya sudah diperiksa pemanggilnya.
    let tolak = |idx: usize, alasan: String| -> Signal {
        Signal::RuntimeError(RuntimeError::new(format!(
            "canvas::{}: argumen ke-{} {}", method, idx + 2, alasan)))
    };
    let get_col = |idx: usize| -> Result<Colour, Signal> {
        match args.get(idx) {
            Some(v) => value_to_colour(v).map_err(|e| tolak(idx, e)),
            None => Ok(Colour::WHITE),
        }
    };
    let get_i = |idx: usize| -> Result<i32, Signal> {
        match args.get(idx) {
            Some(v) => angka_i32(v).map_err(|e| tolak(idx, e)),
            None => Ok(0),
        }
    };
    let get_f = |idx: usize| -> Result<f32, Signal> {
        match args.get(idx) {
            Some(v) => angka_f64(v).map(|f| f as f32).map_err(|e| tolak(idx, e)),
            None => Ok(0.0),
        }
    };
    let get_str = |idx: usize| -> String {
        match args.get(idx) {
            Some(Value::Str(s)) => s.clone(),
            Some(v)             => format!("{}", v),
            None                => String::new(),
        }
    };
    // Sebuah PATH menuntut string, dan tidak boleh jatuh ke `Display`.
    let get_path = |idx: usize, metode: &str| -> Result<String, Signal> {
        match args.get(idx) {
            Some(Value::Str(s)) => Ok(s.clone()),
            Some(v) => Err(Signal::RuntimeError(RuntimeError::new(format!(
                "{}: path harus string, dapat `{}` — sebuah path tidak bisa \
                 diturunkan dari rendering sebuah nilai", metode, v.type_name())))),
            None => Err(Signal::RuntimeError(RuntimeError::new(format!(
                "{}: path tidak diberikan", metode)))),
        }
    };

    match method {
        // ── Window state ─────────────────────────────────────────────────
        "is_open"    => { lock!(inner); Ok(Value::Bool(inner.is_open)) }
        "width"      => { lock!(inner); Ok(Value::Int(inner.width as i128)) }
        "height"     => { lock!(inner); Ok(Value::Int(inner.height as i128)) }
        "fps"        => { lock!(inner); Ok(Value::Float(inner.fps)) }
        "delta_time" => { lock!(inner); Ok(Value::Float(inner.delta_sec)) }
        "frame"      => { lock!(inner); Ok(Value::Int(inner.frame_idx as i128)) }

        "set_title" => {
            if args.is_empty() { cerr!("set_title butuh 1 arg: (judul)"); }
            let new_title = get_str(0);
            lock!(inner, mut);
            inner.title = new_title.clone();
            #[cfg(feature = "canvas")]
            if let Some(ref j) = inner.window {
                j.judul(&new_title);
            }
            Ok(Value::Unit)
        }

        "close" => {
            lock!(inner, mut);
            inner.is_open = false;
            #[cfg(feature = "canvas")]
            { inner.window = None; }
            Ok(Value::Unit)
        }

        // ── Frame update ─────────────────────────────────────────────────
        "update" => {
            match canvas_update(&handle) {
                Ok(open) => Ok(Value::Bool(open)),
                Err(e)   => Err(Signal::RuntimeError(RuntimeError::new(e))),
            }
        }

        // ── Drawing ──────────────────────────────────────────────────────
        "clear" => {
            if args.is_empty() { cerr!("clear butuh 1 arg: (warna) — warnanya adalah seluruh isi operasi ini"); }
            let col = get_col(0)?;
            lock!(inner, mut);
            inner.fb.clear(col);
            Ok(Value::Unit)
        }

        "draw_pixel" => {
            if args.len() < 3 { cerr!("draw_pixel butuh 3 arg: (x, y, warna)"); }
            let (x, y) = (get_f(0)?, get_f(1)?);
            let col = get_col(2)?;
            lock!(inner, mut);
            let g = &mut *inner;
            pena!(g).piksel(x, y, col);
            Ok(Value::Unit)
        }

        "get_pixel" => {
            if args.len() < 2 { cerr!("get_pixel butuh 2 arg: (x, y)"); }
            let (x, y) = (get_i(0)?, get_i(1)?);
            lock!(inner);
            Ok(colour_to_value(inner.fb.get_pixel(x, y)))
        }

        "draw_line" => {
            if args.len() < 5 { cerr!("draw_line butuh 5 arg: (x0,y0,x1,y1,warna)"); }
            let (x0,y0,x1,y1) = (get_f(0)?,get_f(1)?,get_f(2)?,get_f(3)?);
            let col = get_col(4)?;
            lock!(inner, mut);
            let g = &mut *inner;
            pena!(g).garis(x0, y0, x1, y1, col);
            Ok(Value::Unit)
        }

        "draw_rect" => {
            if args.len() < 5 { cerr!("draw_rect butuh 5 arg: (x,y,w,h,warna)"); }
            let (x,y,w,h) = (get_f(0)?,get_f(1)?,get_f(2)?,get_f(3)?);
            let col = get_col(4)?;
            lock!(inner, mut);
            let g = &mut *inner;
            pena!(g).kotak(x, y, w, h, col);
            Ok(Value::Unit)
        }

        "fill_rect" => {
            if args.len() < 5 { cerr!("fill_rect butuh 5 arg: (x,y,w,h,warna)"); }
            let (x,y,w,h) = (get_f(0)?,get_f(1)?,get_f(2)?,get_f(3)?);
            let col = get_col(4)?;
            lock!(inner, mut);
            let g = &mut *inner;
            pena!(g).isi_kotak(x, y, w, h, col);
            Ok(Value::Unit)
        }

        "draw_circle" => {
            if args.len() < 4 { cerr!("draw_circle butuh 4 arg: (cx,cy,r,warna)"); }
            let (cx,cy,r) = (get_f(0)?,get_f(1)?,get_i(2)?);
            let col = get_col(3)?;
            lock!(inner, mut);
            let g = &mut *inner;
            pena!(g).lingkaran(cx, cy, r, col);
            Ok(Value::Unit)
        }

        "fill_circle" => {
            if args.len() < 4 { cerr!("fill_circle butuh 4 arg: (cx,cy,r,warna)"); }
            let (cx,cy,r) = (get_f(0)?,get_f(1)?,get_i(2)?);
            let col = get_col(3)?;
            lock!(inner, mut);
            let g = &mut *inner;
            pena!(g).isi_lingkaran(cx, cy, r, col);
            Ok(Value::Unit)
        }

        "draw_triangle" => {
            if args.len() < 7 { cerr!("draw_triangle butuh 7 arg: (x0,y0,x1,y1,x2,y2,warna)"); }
            let p = [(get_f(0)?, get_f(1)?), (get_f(2)?, get_f(3)?), (get_f(4)?, get_f(5)?)];
            let col = get_col(6)?;
            lock!(inner, mut);
            let g = &mut *inner;
            pena!(g).segitiga(p, col);
            Ok(Value::Unit)
        }

        "fill_triangle" => {
            if args.len() < 7 { cerr!("fill_triangle butuh 7 arg: (x0,y0,x1,y1,x2,y2,warna)"); }
            let p = [(get_f(0)?, get_f(1)?), (get_f(2)?, get_f(3)?), (get_f(4)?, get_f(5)?)];
            let col = get_col(6)?;
            lock!(inner, mut);
            let g = &mut *inner;
            pena!(g).isi_segitiga(p, col);
            Ok(Value::Unit)
        }

        "draw_text" => {
            if args.len() < 4 { cerr!("draw_text butuh 4 arg: (x,y,text,warna)"); }
            let (x,y) = (get_f(0)?, get_f(1)?);
            let text  = get_str(2);
            let col   = get_col(3)?;
            lock!(inner, mut);
            let g = &mut *inner;
            pena!(g).teks(x, y, &text, 1, col);
            Ok(Value::Unit)
        }

        // ── Input ─────────────────────────────────────────────────────────
        "key_pressed" => {
            if args.is_empty() { cerr!("key_pressed butuh 1 arg: (tombol) — tombol yang mana?"); }
            // Nama tombol adalah ISI, bukan teks yang dirender: `key_held(c, f)` dulu
            // menanyakan tombol bernama "<closure>" dan menjawab `false`.
            let Value::Str(key) = &args[0] else {
                return Err(tolak(0, format!("harus nama tombol (string), dapat `{}`", args[0].type_name())));
            };
            kenali_tombol(key).map_err(|e| tolak(0, e))?;
            match canvas_key_pressed(&handle, key) {
                Ok(v)  => Ok(Value::Bool(v)),
                Err(e) => Err(Signal::RuntimeError(RuntimeError::new(e))),
            }
        }

        "key_held" => {
            if args.is_empty() { cerr!("key_held butuh 1 arg: (tombol) — tombol yang mana?"); }
            // Nama tombol adalah ISI, bukan teks yang dirender: `key_held(c, f)` dulu
            // menanyakan tombol bernama "<closure>" dan menjawab `false`.
            let Value::Str(key) = &args[0] else {
                return Err(tolak(0, format!("harus nama tombol (string), dapat `{}`", args[0].type_name())));
            };
            kenali_tombol(key).map_err(|e| tolak(0, e))?;
            match canvas_key_held(&handle, key) {
                Ok(v)  => Ok(Value::Bool(v)),
                Err(e) => Err(Signal::RuntimeError(RuntimeError::new(e))),
            }
        }

        "get_mouse_pos" => {
            lock!(inner);
            let mut fields = std::collections::HashMap::new();
            fields.insert("x".to_string(), Value::Float(inner.mouse_x as f64));
            fields.insert("y".to_string(), Value::Float(inner.mouse_y as f64));
            Ok(Value::Struct { name: "MousePos".to_string(), fields: fields.into() })
        }

        "jumlah_sentuhan" => { lock!(inner); Ok(Value::Int(inner.sentuhan.len() as i128)) }

        "sentuhan" => {
            if args.is_empty() { cerr!("sentuhan butuh 1 arg: (nomor jari)"); }
            let i = match &args[0] {
                Value::Int(n) => *n,
                lain => return Err(tolak(0, format!("harus nomor jari (int), dapat `{}`", lain.type_name()))),
            };
            lock!(inner);
            let n = inner.sentuhan.len();
            let Some(&(x, y)) = usize::try_from(i).ok().and_then(|i| inner.sentuhan.get(i)) else {
                return Err(tolak(0, format!("tidak ada jari ke-{} — yang menempel cuma {} (lihat \
                                             `canvas::jumlah_sentuhan`)", i, n)));
            };
            let mut fields = std::collections::HashMap::new();
            fields.insert("x".to_string(), Value::Float(x as f64));
            fields.insert("y".to_string(), Value::Float(y as f64));
            Ok(Value::Struct { name: "MousePos".to_string(), fields: fields.into() })
        }

        "get_mouse_button" => {
            if args.is_empty() { cerr!("get_mouse_button butuh 1 arg: (0 kiri, 1 tengah, 2 kanan)"); }
            // Nomornya mengikuti kalimat di atas (dan peramban): dulu `"1"` berarti
            // tombol KANAN sementara kalimatnya sendiri menyebut tengah.
            let tombol = match &args[0] {
                Value::Int(n) if (0..=2).contains(n) => *n as usize,
                Value::Str(t) => match t.to_lowercase().as_str() {
                    "left" | "0" => 0, "middle" | "1" => 1, "right" | "2" => 2,
                    _ => return Err(tolak(0, format!(
                        "harus tombol tetikus — 0/\"left\", 1/\"middle\", atau 2/\"right\" — dapat \"{}\"", t))),
                },
                lain => return Err(tolak(0, format!(
                    "harus tombol tetikus — 0/\"left\", 1/\"middle\", atau 2/\"right\" — dapat `{}`",
                    match lain { Value::Int(n) => n.to_string(), l => l.type_name() }))),
            };
            lock!(inner);
            // `mouse_btns` diisi urut kiri, KANAN, tengah (lihat `canvas_update`).
            Ok(Value::Bool(inner.mouse_btns[[0, 2, 1][tombol]]))
        }

        // ── Export ────────────────────────────────────────────────────────
        "save_ppm" => {
            let path = get_path(0, "save_ppm")?;
            if path.is_empty() { cerr!("save_ppm: path tidak boleh kosong"); }
            lock!(inner);
            match inner.fb.save_ppm(&path) {
                Ok(())  => Ok(Value::Bool(true)),
                Err(e)  => { crate::host::galat_baris(&format!("canvas save_ppm: {}", crate::host::galat_io(&e))); Ok(Value::Bool(false)) }
            }
        }

        "save_bmp" => {
            let path = get_path(0, "save_bmp")?;
            if path.is_empty() { cerr!("save_bmp: path tidak boleh kosong"); }
            lock!(inner);
            match inner.fb.save_bmp(&path) {
                Ok(())  => Ok(Value::Bool(true)),
                Err(e)  => { crate::host::galat_baris(&format!("canvas save_bmp: {}", crate::host::galat_io(&e))); Ok(Value::Bool(false)) }
            }
        }

        "set_headless_export" => {
            let path = get_path(0, "set_headless_export")?;
            lock!(inner, mut);
            inner.headless_export_path = if path.is_empty() { None } else { Some(path) };
            Ok(Value::Unit)
        }

        // ── Batch helpers ─────────────────────────────────────────────────
        "draw_grid" => {
            if args.len() < 5 { cerr!("draw_grid butuh 5 arg: (cols,rows,cell_w,cell_h,warna)"); }
            let (cols,rows) = (get_i(0)?,get_i(1)?);
            let (cw,ch)     = (get_i(2)?,get_i(3)?);
            let col          = get_col(4)?;
            lock!(inner, mut);
            let (fw, fh) = (inner.width as f32, inner.height as f32);
            let g = &mut *inner;
            let mut p = pena!(g);
            for c in 0..=cols { let x = (c * cw) as f32; p.garis(x, 0.0, x, fh, col); }
            for r in 0..=rows { let y = (r * ch) as f32; p.garis(0.0, y, fw, y, col); }
            Ok(Value::Unit)
        }

        "draw_crosshair" => {
            if args.len() < 4 { cerr!("draw_crosshair butuh 4 arg: (x,y,size,warna)"); }
            let (x,y,size) = (get_f(0)?,get_f(1)?,get_f(2)?);
            let col = get_col(3)?;
            lock!(inner, mut);
            let g = &mut *inner;
            let mut p = pena!(g);
            p.garis(x-size, y, x+size, y, col);
            p.garis(x, y-size, x, y+size, col);
            Ok(Value::Unit)
        }

        // ══ Blit: menggambar satu canvas ke atas canvas lain ═════════════
        "blit" | "blit_region" => {
            let Some(Value::Canvas(sumber)) = args.first() else {
                cerr!("{}: argumen pertamanya harus Canvas sumber, dapat `{}`",
                      method, args.first().map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".to_string()));
            };
            // Sumber dan tujuan yang SAMA harus disalin lebih dulu.
            let sama = std::sync::Arc::ptr_eq(&handle, sumber);
            let (sw, sh, spix) = if sama {
                lock!(inner);
                (inner.width, inner.height, inner.fb.pixels.clone())
            } else {
                let src = sumber.lock().map_err(|e| Signal::RuntimeError(
                    RuntimeError::new(format!("canvas sumber lock: {}", e))))?;
                (src.width, src.height, src.fb.pixels.clone())
            };

            // `blit(src, dx, dy)` atau
            // `blit_region(src, sx, sy, w, h, dx, dy)`.
            let (sx, sy, w, h, dx, dy) = if method == "blit" {
                // Posisinya WAJIB.
                if args.len() < 3 { cerr!("blit butuh (sumber, x, y)"); }
                (0i32, 0i32, sw as i32, sh as i32, get_f(1)?, get_f(2)?)
            } else {
                if args.len() < 7 {
                    cerr!("blit_region butuh (sumber, sx, sy, lebar, tinggi, dx, dy)");
                }
                (get_i(1)?, get_i(2)?, get_i(3)?, get_i(4)?, get_f(5)?, get_f(6)?)
            };
            if w <= 0 || h <= 0 {
                cerr!("{}: lebar dan tinggi harus lebih dari 0, dapat {}x{}", method, w, h);
            }

            lock!(inner, mut);
            let g = &mut *inner;
            pena!(g).blit(&spix, sw, sh, (sx, sy, w, h), dx, dy);
            Ok(Value::Bool(true))
        }

        // ══ Ekspor PNG ═══════════════════════════════════════════════════
        "save_png" => {
            let path = get_path(0, "save_png")?;
            // Sejajar dengan `save_ppm`/`save_bmp`: tanpa penjaga ini yang
            // keluar `No such file or directory (os error 2)` — sebuah errno
            // mentah untuk kesalahan yang sudah diketahui namanya.
            if path.is_empty() { cerr!("save_png: path tidak boleh kosong"); }
            lock!(inner);
            let isi = png::susun(inner.width, inner.height, &inner.fb.pixels);
            match std::fs::write(&path, isi) {
                Ok(()) => Ok(Value::Bool(true)),
                // Gagal tulis dilaporkan dengan sebabnya, bukan `false`
                // polos — "gagal" tanpa alasan mengirim orang menebak
                // antara path salah, izin, dan disk penuh.
                Err(e) => cerr!("save_png '{}': {}", path, crate::host::galat_io(&e)),
            }
        }

        // ══ Transformasi ═════════════════════════════════════════════════
        "push"  => {
            lock!(inner, mut);
            if !inner.st.simpan() {
                cerr!("push: tumpukan keadaan penuh ({} tingkat) — tiap `push` butuh `pop` pasangannya",
                      lanjut::Keadaan::MAKS_TUMPUKAN);
            }
            Ok(Value::Unit)
        }
        "pop"   => {
            lock!(inner, mut);
            if !inner.st.pulihkan() {
                cerr!("pop: tidak ada keadaan tersimpan — tiap `pop` butuh `push` sebelumnya");
            }
            Ok(Value::Unit)
        }
        "translate" => {
            if args.len() < 2 { cerr!("translate butuh 2 arg: (dx, dy)"); }
            let (dx, dy) = (get_f(0)?, get_f(1)?);
            lock!(inner, mut);
            inner.st.trafo = lanjut::Transformasi::geser(dx, dy).lalu(&inner.st.trafo);
            Ok(Value::Unit)
        }
        "scale" => {
            if args.is_empty() { cerr!("scale butuh 1 atau 2 arg: (s) untuk seragam, atau (sx, sy)"); }
            let (sx, sy) = (get_f(0)?, if args.len() > 1 { get_f(1)? } else { get_f(0)? });
            lock!(inner, mut);
            inner.st.trafo = lanjut::Transformasi::skala(sx, sy).lalu(&inner.st.trafo);
            Ok(Value::Unit)
        }
        "rotate" => {
            if args.is_empty() { cerr!("rotate butuh 1 arg: (sudut dalam DERAJAT)"); }
            // Derajat, bukan radian: yang menggambar berpikir dalam
            // derajat, dan `rotate(90)` yang ternyata berarti 90 radian
            // adalah kejutan yang mahal.
            let d = get_f(0)?.to_radians();
            lock!(inner, mut);
            inner.st.trafo = lanjut::Transformasi::putar(d).lalu(&inner.st.trafo);
            Ok(Value::Unit)
        }
        "reset_transform" => {
            lock!(inner, mut);
            inner.st.trafo = lanjut::Transformasi::IDENTITAS;
            Ok(Value::Unit)
        }
        "clip" => {
            if args.len() < 4 { cerr!("clip butuh 4 arg: (x,y,w,h)"); }
            let (x, y, w, h) = (get_i(0)?, get_i(1)?, get_i(2)?, get_i(3)?);
            lock!(inner, mut);
            inner.st.potong = Some((x, y, x + w, y + h));
            Ok(Value::Unit)
        }
        "clip_reset" => { lock!(inner, mut); inner.st.potong = None; Ok(Value::Unit) }

        // ══ Bentuk halus (anti-alias) ════════════════════════════════════
        "line_aa" => {
            if args.len() < 5 { cerr!("line_aa butuh 5 arg: (x0,y0,x1,y1,warna)"); }
            let (x0, y0, x1, y1) = (get_f(0)?, get_f(1)?, get_f(2)?, get_f(3)?);
            let col = get_col(4)?;
            lock!(inner, mut);
            let st = inner.st.clone();
            lanjut::Kuas { fb: &mut inner.fb, st: &st }.garis_aa(x0, y0, x1, y1, col);
            Ok(Value::Unit)
        }
        "fill_poly" | "draw_poly" => {
            if args.len() < 2 { cerr!("{} butuh 2 arg: (titik, warna)", method); }
            let titik = ambil_titik(&args[0])
                .map_err(|e| Signal::RuntimeError(RuntimeError::new(format!("{}: {}", method, e))))?;
            let col = get_col(1)?;
            lock!(inner, mut);
            let st = inner.st.clone();
            let mut k = lanjut::Kuas { fb: &mut inner.fb, st: &st };
            if method == "fill_poly" { k.poligon(&titik, col) } else { k.garis_poligon(&titik, col, true) }
            Ok(Value::Unit)
        }
        "fill_ellipse" | "draw_ellipse" => {
            if args.len() < 5 { cerr!("{} butuh 5 arg: (cx,cy,rx,ry,warna)", method); }
            let (cx, cy, rx, ry) = (get_f(0)?, get_f(1)?, get_f(2)?, get_f(3)?);
            let col = get_col(4)?;
            let t = lanjut::Kuas::elips_titik(cx, cy, rx, ry, 0.0, std::f32::consts::TAU);
            lock!(inner, mut);
            let st = inner.st.clone();
            let mut k = lanjut::Kuas { fb: &mut inner.fb, st: &st };
            if method == "fill_ellipse" { k.poligon(&t, col) } else { k.garis_poligon(&t, col, true) }
            Ok(Value::Unit)
        }
        "draw_arc" => {
            if args.len() < 6 { cerr!("draw_arc butuh 6 arg: (cx,cy,r,derajat_mulai,derajat_akhir,warna)"); }
            let (cx, cy, r) = (get_f(0)?, get_f(1)?, get_f(2)?);
            let (m, a) = (get_f(3)?.to_radians(), get_f(4)?.to_radians());
            let col = get_col(5)?;
            let t = lanjut::Kuas::elips_titik(cx, cy, r, r, m, a);
            lock!(inner, mut);
            let st = inner.st.clone();
            lanjut::Kuas { fb: &mut inner.fb, st: &st }.garis_poligon(&t, col, false);
            Ok(Value::Unit)
        }
        "fill_round_rect" | "draw_round_rect" => {
            if args.len() < 6 { cerr!("{} butuh 6 arg: (x,y,w,h,radius,warna)", method); }
            let (x, y, w, h, r) = (get_f(0)?, get_f(1)?, get_f(2)?, get_f(3)?, get_f(4)?);
            let col = get_col(5)?;
            let t = lanjut::kotak_bulat_titik(x, y, w, h, r);
            lock!(inner, mut);
            let st = inner.st.clone();
            let mut k = lanjut::Kuas { fb: &mut inner.fb, st: &st };
            if method == "fill_round_rect" { k.poligon(&t, col) } else { k.garis_poligon(&t, col, true) }
            Ok(Value::Unit)
        }
        "draw_bezier" => {
            if args.len() < 2 { cerr!("draw_bezier butuh 2 arg: (titik_kendali, warna)"); }
            let p = ambil_titik(&args[0])
                .map_err(|e| Signal::RuntimeError(RuntimeError::new(format!("draw_bezier: {}", e))))?;
            if p.len() < 3 { cerr!("draw_bezier butuh minimal 3 titik kendali"); }
            let col = get_col(1)?;
            let t = lanjut::Kuas::bezier_titik(&p);
            lock!(inner, mut);
            let st = inner.st.clone();
            lanjut::Kuas { fb: &mut inner.fb, st: &st }.garis_poligon(&t, col, false);
            Ok(Value::Unit)
        }

        // ══ Gradien ══════════════════════════════════════════════════════
        "fill_linear_gradient" => {
            if args.len() < 6 { cerr!("fill_linear_gradient butuh 6 arg: (x0,y0,x1,y1,warna0,warna1)"); }
            let (x0, y0, x1, y1) = (get_f(0)?, get_f(1)?, get_f(2)?, get_f(3)?);
            let (c0, c1) = (get_col(4)?, get_col(5)?);
            lock!(inner, mut);
            let st = inner.st.clone();
            lanjut::Kuas { fb: &mut inner.fb, st: &st }.gradien_linear(x0, y0, x1, y1, c0, c1);
            Ok(Value::Unit)
        }
        "fill_radial_gradient" => {
            if args.len() < 5 { cerr!("fill_radial_gradient butuh 5 arg: (cx,cy,r,warna_dalam,warna_luar)"); }
            let (cx, cy, r) = (get_f(0)?, get_f(1)?, get_f(2)?);
            let (c0, c1) = (get_col(3)?, get_col(4)?);
            lock!(inner, mut);
            let st = inner.st.clone();
            lanjut::Kuas { fb: &mut inner.fb, st: &st }.gradien_radial(cx, cy, r, c0, c1);
            Ok(Value::Unit)
        }

        // ══ Teks berskala ════════════════════════════════════════════════
        "draw_text_scaled" => {
            if args.len() < 5 { cerr!("draw_text_scaled butuh 5 arg: (x,y,teks,skala,warna)"); }
            let (x, y) = (get_f(0)?, get_f(1)?);
            let teks = get_str(2);
            let skala = get_i(3)?;
            if skala < 1 {
                return Err(tolak(3, format!("adalah skala, dan harus 1 atau lebih, dapat {}", skala)));
            }
            let col = get_col(4)?;
            lock!(inner, mut);
            let g = &mut *inner;
            pena!(g).teks(x, y, &teks, skala, col);
            Ok(Value::Unit)
        }

        // Kalimat yang sama dengan setiap penerima lain, di kedua backend — dan
        // sarannya diturunkan dari registry `canvas::`, bukan dari daftar tangan di …
        other => {
            let calon = crate::interpreter::calon_method("canvas");
            let saran = crate::diag::saran_nama_terdekat(
                other, calon.iter().map(|s| s.as_str()));
            Err(Signal::RuntimeError(RuntimeError::new(
                crate::interpreter::pesan_method_tak_ada(
                    "canvas", other, "", saran.as_deref())
            )))
        }
    }
}

/// Baca daftar titik dari nilai JRG.
fn ambil_titik(v: &crate::interpreter::Value) -> Result<Vec<(f32, f32)>, String> {
    use crate::interpreter::Value;
    let Value::Array(a) = v else {
        return Err(format!("butuh array titik, dapat `{}`", v.type_name()));
    };
    if a.is_empty() { return Ok(Vec::new()); }
    let angka = |x: &Value, i: usize| -> Result<f32, String> {
        angka_f64(x).map(|f| f as f32).map_err(|e| format!("titik ke-{}: koordinatnya {}", i, e))
    };
    if let Some(Value::Array(_)) = a.first() {
        let mut out = Vec::with_capacity(a.len());
        for (i, p) in a.iter().enumerate() {
            let Value::Array(pair) = p else {
                return Err(format!("titik ke-{} bukan pasangan [x,y]", i));
            };
            if pair.len() != 2 {
                return Err(format!("titik ke-{} punya {} angka, butuh tepat 2", i, pair.len()));
            }
            out.push((angka(&pair[0], i)?, angka(&pair[1], i)?));
        }
        return Ok(out);
    }
    if a.len() % 2 != 0 {
        return Err(format!(
            "daftar rata harus genap (pasangan x,y), dapat {} angka", a.len()
        ));
    }
    a.chunks(2).enumerate().map(|(i, c)| Ok((angka(&c[0], i)?, angka(&c[1], i)?))).collect()
}
