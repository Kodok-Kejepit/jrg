// src/canvas/pena.rs — satu pintu piksel untuk primitif dasar kanvas.

use super::lanjut::{Keadaan, Kuas, Transformasi};
use super::{Colour, Framebuffer, FONT_5X7, FONT_CHAR_W};

pub struct Pena<'a> {
    pub fb: &'a mut Framebuffer,
    pub st: &'a Keadaan,
}

/// Bentuk transformasi aktif: geser murni, atau yang lain.
enum Ragam { Geser(f32, f32), Umum }

impl Transformasi {
    /// Kebalikannya; `None` kalau determinannya nol (gambar diruntuhkan ke garis).
    pub fn balik(&self) -> Option<Transformasi> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < 1e-12 { return None; }
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        Some(Transformasi { a, b, c, d, e: -(a * self.e + c * self.f), f: -(b * self.e + d * self.f) })
    }
}

impl<'a> Pena<'a> {
    fn ragam(&self) -> Ragam {
        let t = &self.st.trafo;
        if t.a == 1.0 && t.b == 0.0 && t.c == 0.0 && t.d == 1.0 { Ragam::Geser(t.e, t.f) } else { Ragam::Umum }
    }

    fn kuas(&mut self) -> Kuas<'_> { Kuas { fb: &mut *self.fb, st: self.st } }

    /// Satu piksel di koordinat LAYAR, lewat clip.
    #[inline]
    fn plot(&mut self, x: i32, y: i32, col: Colour) {
        if self.st.boleh(x, y) { self.fb.set_pixel(x, y, col); }
    }

    fn bulat(v: f32, geser: f32) -> i32 { (v + geser).floor() as i32 }

    pub fn piksel(&mut self, x: f32, y: f32, col: Colour) {
        match self.ragam() {
            Ragam::Geser(e, f) => self.plot(Self::bulat(x, e), Self::bulat(y, f), col),
            Ragam::Umum => self.kuas().poligon(&[(x, y), (x + 1.0, y), (x + 1.0, y + 1.0), (x, y + 1.0)], col),
        }
    }

    fn garis_bulat(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, col: Colour) {
        let (mut x, mut y) = (x0, y0);
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx: i32 = if x0 < x1 { 1 } else { -1 };
        let sy: i32 = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            self.plot(x, y, col);
            if x == x1 && y == y1 { break; }
            let e2 = 2 * err;
            if e2 >= dy { if x == x1 { break; } err += dy; x += sx; }
            if e2 <= dx { if y == y1 { break; } err += dx; y += sy; }
        }
    }

    pub fn garis(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, col: Colour) {
        match self.ragam() {
            Ragam::Geser(e, f) => self.garis_bulat(Self::bulat(x0, e), Self::bulat(y0, f),
                                                   Self::bulat(x1, e), Self::bulat(y1, f), col),
            Ragam::Umum => self.kuas().garis_aa(x0 + 0.5, y0 + 0.5, x1 + 0.5, y1 + 0.5, col),
        }
    }

    pub fn kotak(&mut self, x: f32, y: f32, w: f32, h: f32, col: Colour) {
        let (x1, y1) = (x + w - 1.0, y + h - 1.0);
        self.garis(x, y, x1, y, col);
        self.garis(x, y1, x1, y1, col);
        self.garis(x, y, x, y1, col);
        self.garis(x1, y, x1, y1, col);
    }

    pub fn isi_kotak(&mut self, x: f32, y: f32, w: f32, h: f32, col: Colour) {
        match self.ragam() {
            Ragam::Geser(e, f) => {
                let (mut x0, mut y0) = (Self::bulat(x, e), Self::bulat(y, f));
                let (mut x1, mut y1) = (Self::bulat(x + w, e), Self::bulat(y + h, f));
                x0 = x0.max(0); y0 = y0.max(0);
                x1 = x1.min(self.fb.width as i32); y1 = y1.min(self.fb.height as i32);
                if let Some((cx0, cy0, cx1, cy1)) = self.st.potong {
                    x0 = x0.max(cx0); y0 = y0.max(cy0); x1 = x1.min(cx1); y1 = y1.min(cy1);
                }
                let isi = col.to_argb_u32();
                let lebar = self.fb.width;
                for py in y0..y1 {
                    let awal = py as usize * lebar;
                    for px in x0..x1 {
                        let i = awal + px as usize;
                        self.fb.pixels[i] = if col.a == 255 { isi } else {
                            col.blend_over(Colour::from_argb_u32(self.fb.pixels[i])).to_argb_u32()
                        };
                    }
                }
            }
            Ragam::Umum => self.kuas().poligon(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h)], col),
        }
    }

    pub fn lingkaran(&mut self, cx: f32, cy: f32, r: i32, col: Colour) {
        match self.ragam() {
            Ragam::Geser(e, f) => {
                let (cx, cy) = (Self::bulat(cx, e), Self::bulat(cy, f));
                if r <= 0 { self.plot(cx, cy, col); return; }
                let (mut x, mut y) = (0i32, r);
                let mut d = 3 - 2 * r;
                self.delapan(cx, cy, x, y, col);
                while y >= x {
                    x += 1;
                    if d > 0 { y -= 1; d += 4 * (x - y) + 10; } else { d += 4 * x + 6; }
                    self.delapan(cx, cy, x, y, col);
                }
            }
            Ragam::Umum => {
                let rr = r.max(0) as f32;
                let t = Kuas::elips_titik(cx + 0.5, cy + 0.5, rr, rr, 0.0, std::f32::consts::TAU);
                self.kuas().garis_poligon(&t, col, true);
            }
        }
    }

    fn delapan(&mut self, cx: i32, cy: i32, x: i32, y: i32, col: Colour) {
        for (px, py) in [(cx+x, cy+y), (cx-x, cy+y), (cx+x, cy-y), (cx-x, cy-y),
                         (cx+y, cy+x), (cx-y, cy+x), (cx+y, cy-x), (cx-y, cy-x)] {
            self.plot(px, py, col);
        }
    }

    pub fn isi_lingkaran(&mut self, cx: f32, cy: f32, r: i32, col: Colour) {
        match self.ragam() {
            Ragam::Geser(e, f) => {
                let (cx, cy) = (Self::bulat(cx, e), Self::bulat(cy, f));
                if r <= 0 { self.plot(cx, cy, col); return; }
                let (mut x, mut y) = (0i32, r);
                let mut d = 3 - 2 * r;
                self.garis_datar(cx, cy, x, y, col);
                while y >= x {
                    x += 1;
                    if d > 0 { y -= 1; d += 4 * (x - y) + 10; } else { d += 4 * x + 6; }
                    self.garis_datar(cx, cy, x, y, col);
                }
            }
            Ragam::Umum => {
                let rr = r.max(0) as f32 + 0.5;
                let t = Kuas::elips_titik(cx + 0.5, cy + 0.5, rr, rr, 0.0, std::f32::consts::TAU);
                self.kuas().poligon(&t, col);
            }
        }
    }

    /// Empat potong baris lingkaran terisi, di koordinat layar (sudah digeser).
    fn garis_datar(&mut self, cx: i32, cy: i32, x: i32, y: i32, col: Colour) {
        for (ax, ay, lebar) in [(cx - x, cy + y, 2 * x), (cx - x, cy - y, 2 * x),
                                (cx - y, cy + x, 2 * y), (cx - y, cy - x, 2 * y)] {
            for px in ax..ax + lebar { self.plot(px, ay, col); }
        }
    }

    pub fn segitiga(&mut self, p: [(f32, f32); 3], col: Colour) {
        self.garis(p[0].0, p[0].1, p[1].0, p[1].1, col);
        self.garis(p[1].0, p[1].1, p[2].0, p[2].1, col);
        self.garis(p[2].0, p[2].1, p[0].0, p[0].1, col);
    }

    pub fn isi_segitiga(&mut self, p: [(f32, f32); 3], col: Colour) {
        match self.ragam() {
            Ragam::Geser(e, f) => {
                let mut q = [(0i32, 0i32); 3];
                for (i, &(x, y)) in p.iter().enumerate() { q[i] = (Self::bulat(x, e), Self::bulat(y, f)); }
                q.sort_by_key(|t| t.1);
                let ((ax, ay), (bx, by), (cx, cy)) = (q[0], q[1], q[2]);
                let total = cy - ay;
                if total == 0 { return; }
                for y in ay..=cy {
                    let kedua = y >= by;
                    let seg = if kedua { cy - by } else { by - ay };
                    let seg = if seg == 0 { 1 } else { seg };
                    let alfa = (y - ay) as f32 / total as f32;
                    let beta = if kedua { (y - by) as f32 / seg as f32 } else { (y - ay) as f32 / seg as f32 };
                    let mut xa = ax as f32 + (cx - ax) as f32 * alfa;
                    let mut xb = if kedua { bx as f32 + (cx - bx) as f32 * beta }
                                 else { ax as f32 + (bx - ax) as f32 * beta };
                    if xa > xb { std::mem::swap(&mut xa, &mut xb); }
                    for x in (xa as i32)..=(xb as i32) { self.plot(x, y, col); }
                }
            }
            Ragam::Umum => {
                let t: Vec<(f32, f32)> = p.iter().map(|&(x, y)| (x + 0.5, y + 0.5)).collect();
                self.kuas().poligon(&t, col);
            }
        }
    }

    /// Teks dengan font 5x7; tiap piksel font jadi kotak `skala x skala`.
    pub fn teks(&mut self, x: f32, y: f32, teks: &str, skala: i32, col: Colour) {
        let s = skala.max(1) as f32;
        let mut kursor = x;
        for ch in teks.chars() {
            if ch == '\n' { continue; }
            let kode = ch as u32;
            if (0x20..=0x7E).contains(&kode) {
                let i = ((kode - 0x20) as usize) * 5;
                for kx in 0..5usize {
                    for ry in 0..7i32 {
                        if FONT_5X7[i + kx] & (1 << ry) != 0 {
                            self.isi_kotak(kursor + kx as f32 * s, y + ry as f32 * s, s, s, col);
                        }
                    }
                }
            }
            kursor += FONT_CHAR_W as f32 * s;
        }
    }

    /// Sebagian sumber `(sx, sy, w, h)` ke `(dx, dy)` di ruang gambar; alfa menyatu.
    pub fn blit(&mut self, spix: &[u32], sw: usize, sh: usize,
                (sx, sy, w, h): (i32, i32, i32, i32), dx: f32, dy: f32) {
        let ambil = |u: i32, v: i32| -> Option<Colour> {
            let (px, py) = (sx + u, sy + v);
            if px < 0 || py < 0 || px >= sw as i32 || py >= sh as i32 { return None; }
            Some(Colour::from_argb_u32(spix[py as usize * sw + px as usize]))
        };
        match self.ragam() {
            Ragam::Geser(e, f) => {
                let (ox, oy) = (Self::bulat(dx, e), Self::bulat(dy, f));
                for v in 0..h {
                    for u in 0..w {
                        if let Some(c) = ambil(u, v) { self.plot(ox + u, oy + v, c); }
                    }
                }
            }
            Ragam::Umum => {
                // Tiap piksel tujuan di dalam kotak batas dipetakan BALIK ke sumber.
                let Some(balik) = self.st.trafo.balik() else { return };
                let t = self.st.trafo;
                let sudut = [t.terapkan(dx, dy), t.terapkan(dx + w as f32, dy),
                             t.terapkan(dx, dy + h as f32), t.terapkan(dx + w as f32, dy + h as f32)];
                let x0 = sudut.iter().map(|p| p.0).fold(f32::INFINITY, f32::min).floor().max(0.0) as i32;
                let y0 = sudut.iter().map(|p| p.1).fold(f32::INFINITY, f32::min).floor().max(0.0) as i32;
                let x1 = sudut.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max).ceil()
                    .min(self.fb.width as f32) as i32;
                let y1 = sudut.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max).ceil()
                    .min(self.fb.height as f32) as i32;
                for py in y0..y1 {
                    for px in x0..x1 {
                        let (gx, gy) = balik.terapkan(px as f32 + 0.5, py as f32 + 0.5);
                        let (u, v) = ((gx - dx).floor(), (gy - dy).floor());
                        if u < 0.0 || v < 0.0 || u >= w as f32 || v >= h as f32 { continue; }
                        if let Some(c) = ambil(u as i32, v as i32) { self.plot(px, py, c); }
                    }
                }
            }
        }
    }
}
