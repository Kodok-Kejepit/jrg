// src/canvas/lanjut.rs — canvas/lanjut.rs — lapisan gambar tingkat lanjut.

use super::{Colour, Framebuffer};

// ─── Transformasi ────────────────────────────────────────────────────────

/// Matriks affine 2x3 baris-mayor: `[a c e; b d f]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transformasi {
    pub a: f32, pub b: f32,
    pub c: f32, pub d: f32,
    pub e: f32, pub f: f32,
}

impl Transformasi {
    pub const IDENTITAS: Transformasi =
        Transformasi { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    pub fn terapkan(&self, x: f32, y: f32) -> (f32, f32) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }

    /// `self` DULU, baru `lain` — urutan yang sama dengan tumpukan
    /// transformasi di pustaka grafis pada umumnya, jadi
    /// `translate(); rotate();` berarti "geser, lalu putar di tempat itu".
    pub fn lalu(&self, lain: &Transformasi) -> Transformasi {
        Transformasi {
            a: lain.a * self.a + lain.c * self.b,
            b: lain.b * self.a + lain.d * self.b,
            c: lain.a * self.c + lain.c * self.d,
            d: lain.b * self.c + lain.d * self.d,
            e: lain.a * self.e + lain.c * self.f + lain.e,
            f: lain.b * self.e + lain.d * self.f + lain.f,
        }
    }

    pub fn geser(dx: f32, dy: f32) -> Transformasi {
        Transformasi { e: dx, f: dy, ..Transformasi::IDENTITAS }
    }
    pub fn skala(sx: f32, sy: f32) -> Transformasi {
        Transformasi { a: sx, d: sy, ..Transformasi::IDENTITAS }
    }
    pub fn putar(radian: f32) -> Transformasi {
        let (s, k) = (radian.sin(), radian.cos());
        Transformasi { a: k, b: s, c: -s, d: k, e: 0.0, f: 0.0 }
    }
}

/// Keadaan gambar: transformasi aktif dan kotak pemotong.
#[derive(Clone, Debug)]
pub struct Keadaan {
    pub trafo: Transformasi,
    /// `(x0, y0, x1, y1)`, x1/y1 eksklusif. `None` = seluruh kanvas.
    pub potong: Option<(i32, i32, i32, i32)>,
    /// Tumpukan untuk `simpan`/`pulihkan`.
    tumpukan: Vec<(Transformasi, Option<(i32, i32, i32, i32)>)>,
}

impl Default for Keadaan {
    fn default() -> Self {
        Keadaan { trafo: Transformasi::IDENTITAS, potong: None, tumpukan: Vec::new() }
    }
}

impl Keadaan {
    /// Batas tumpukan: `push` di dalam loop tanpa `pop` jadi galat, bukan tumbuh terus.
    pub const MAKS_TUMPUKAN: usize = 256;

    /// `false` kalau tumpukannya penuh — dulu diam-diam TIDAK menyimpan, jadi
    /// `pop` sesudahnya memulihkan keadaan yang salah.
    pub fn simpan(&mut self) -> bool {
        if self.tumpukan.len() >= Self::MAKS_TUMPUKAN { return false; }
        self.tumpukan.push((self.trafo, self.potong));
        true
    }
    pub fn pulihkan(&mut self) -> bool {
        match self.tumpukan.pop() {
            Some((t, p)) => { self.trafo = t; self.potong = p; true }
            None => false,
        }
    }
    pub fn boleh(&self, x: i32, y: i32) -> bool {
        match self.potong {
            None => true,
            Some((x0, y0, x1, y1)) => x >= x0 && y >= y0 && x < x1 && y < y1,
        }
    }
}

/// Kuas: framebuffer + keadaan gambar. Semua fungsi di bawah lewat sini,
/// jadi transformasi dan pemotongan berlaku SERAGAM — tidak ada primitif
/// yang diam-diam mengabaikannya.
pub struct Kuas<'a> {
    pub fb: &'a mut Framebuffer,
    pub st: &'a Keadaan,
}

impl<'a> Kuas<'a> {
    #[inline]
    fn titik(&mut self, x: f32, y: f32, col: Colour, cakupan: f32) {
        if cakupan <= 0.0 { return; }
        let (tx, ty) = self.st.trafo.terapkan(x, y);
        let (ix, iy) = (tx.round() as i32, ty.round() as i32);
        if !self.st.boleh(ix, iy) { return; }
        let c = if cakupan >= 1.0 { col } else {
            Colour::rgba(col.r, col.g, col.b, (col.a as f32 * cakupan.min(1.0)) as u8)
        };
        self.fb.set_pixel(ix, iy, c);
    }

    /// Garis dengan anti-alias (Xiaolin Wu).
    pub fn garis_aa(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, col: Colour) {
        let curam = (y1 - y0).abs() > (x1 - x0).abs();
        let (mut x0, mut y0, mut x1, mut y1) = if curam { (y0, x0, y1, x1) } else { (x0, y0, x1, y1) };
        if x0 > x1 { std::mem::swap(&mut x0, &mut x1); std::mem::swap(&mut y0, &mut y1); }
        let dx = x1 - x0;
        let dy = y1 - y0;
        let gradien = if dx.abs() < 1e-6 { 1.0 } else { dy / dx };

        let taruh = |k: &mut Kuas, x: f32, y: f32, c: f32| {
            if curam { k.titik(y, x, col, c) } else { k.titik(x, y, col, c) }
        };

        // Ujung awal
        let xend = x0.round();
        let yend = y0 + gradien * (xend - x0);
        let xgap = 1.0 - (x0 + 0.5).fract();
        let xpxl1 = xend;
        let ypxl1 = yend.floor();
        taruh(self, xpxl1, ypxl1, (1.0 - yend.fract()) * xgap);
        taruh(self, xpxl1, ypxl1 + 1.0, yend.fract() * xgap);
        let mut antar = yend + gradien;

        // Ujung akhir
        let xend2 = x1.round();
        let yend2 = y1 + gradien * (xend2 - x1);
        let xgap2 = (x1 + 0.5).fract();
        let xpxl2 = xend2;
        let ypxl2 = yend2.floor();

        let mut x = xpxl1 + 1.0;
        while x < xpxl2 {
            taruh(self, x, antar.floor(), 1.0 - antar.fract());
            taruh(self, x, antar.floor() + 1.0, antar.fract());
            antar += gradien;
            x += 1.0;
        }
        taruh(self, xpxl2, ypxl2, (1.0 - yend2.fract()) * xgap2);
        taruh(self, xpxl2, ypxl2 + 1.0, yend2.fract() * xgap2);
    }

    /// Isi poligon sembarang dengan anti-alias.
    pub fn poligon(&mut self, titik: &[(f32, f32)], col: Colour) {
        if titik.len() < 3 { return; }
        // Titik ditransformasikan DULU, lalu dirasterkan di ruang layar —
        // kalau tidak, transformasi non-seragam (skala x != y) akan
        // membuat tepinya salah cakupan.
        let p: Vec<(f32, f32)> = titik.iter().map(|&(x, y)| self.st.trafo.terapkan(x, y)).collect();
        let ymin = p.iter().map(|q| q.1).fold(f32::INFINITY, f32::min).floor().max(0.0) as i32;
        let ymax = p.iter().map(|q| q.1).fold(f32::NEG_INFINITY, f32::max).ceil()
            .min(self.fb.height as f32) as i32;
        const SUB: i32 = 4;
        let lebar = self.fb.width as i32;
        let mut cakupan = vec![0.0f32; lebar.max(0) as usize];
        let mut potong: Vec<f32> = Vec::with_capacity(16);

        for py in ymin..ymax {
            for c in cakupan.iter_mut() { *c = 0.0; }
            for s in 0..SUB {
                let y = py as f32 + (s as f32 + 0.5) / SUB as f32;
                potong.clear();
                for i in 0..p.len() {
                    let (x1, y1) = p[i];
                    let (x2, y2) = p[(i + 1) % p.len()];
                    if (y1 <= y && y2 > y) || (y2 <= y && y1 > y) {
                        potong.push(x1 + (y - y1) / (y2 - y1) * (x2 - x1));
                    }
                }
                // `total_cmp`, bukan `partial_cmp(..).unwrap_or(Equal)`: yang kedua
                // melanggar total order begitu ada NaN (titik ber-koordinat tak
                // hingga menghasilkannya), dan sejak Rust 1.81 itu PANIK.
                potong.sort_by(|a, b| a.total_cmp(b));
                let mut k = 0;
                while k + 1 < potong.len() {
                    let (mut xa, mut xb) = (potong[k], potong[k + 1]);
                    if xb < xa { std::mem::swap(&mut xa, &mut xb); }
                    let ia = xa.floor().max(0.0) as i32;
                    let ib = xb.ceil().min(lebar as f32) as i32;
                    for px in ia..ib {
                        // Cakupan sub-baris ini pada piksel `px`: seberapa
                        // besar potongan [xa,xb] menutupi [px, px+1].
                        let kiri = xa.max(px as f32);
                        let kanan = xb.min(px as f32 + 1.0);
                        if kanan > kiri {
                            cakupan[px as usize] += (kanan - kiri) / SUB as f32;
                        }
                    }
                    k += 2;
                }
            }
            for px in 0..lebar {
                let c = cakupan[px as usize];
                if c > 0.002 && self.st.boleh(px, py) {
                    let a = (col.a as f32 * c.min(1.0)) as u8;
                    self.fb.set_pixel(px, py, Colour::rgba(col.r, col.g, col.b, a));
                }
            }
        }
    }

    pub fn garis_poligon(&mut self, titik: &[(f32, f32)], col: Colour, tutup: bool) {
        if titik.len() < 2 { return; }
        for i in 0..titik.len() - 1 {
            self.garis_aa(titik[i].0, titik[i].1, titik[i + 1].0, titik[i + 1].1, col);
        }
        if tutup {
            let (a, b) = (titik[titik.len() - 1], titik[0]);
            self.garis_aa(a.0, a.1, b.0, b.1, col);
        }
    }

    /// Elips sebagai poligon bersegmen. Jumlah segmennya ikut ukuran, jadi
    /// lingkaran kecil tidak membayar 200 sisi dan lingkaran besar tidak
    /// terlihat bersegi.
    pub fn elips_titik(cx: f32, cy: f32, rx: f32, ry: f32, mulai: f32, akhir: f32) -> Vec<(f32, f32)> {
        let r = rx.abs().max(ry.abs());
        let n = ((r * 0.8) as usize).clamp(12, 512);
        let rentang = akhir - mulai;
        let n = ((n as f32 * (rentang.abs() / std::f32::consts::TAU)).ceil() as usize).max(3);
        (0..=n).map(|i| {
            let t = mulai + rentang * i as f32 / n as f32;
            (cx + rx * t.cos(), cy + ry * t.sin())
        }).collect()
    }

    /// Kurva Bezier kuadratik/kubik, dicacah jadi garis.
    pub fn bezier_titik(p: &[(f32, f32)]) -> Vec<(f32, f32)> {
        let mut panjang = 0.0f32;
        for i in 0..p.len() - 1 {
            panjang += ((p[i + 1].0 - p[i].0).powi(2) + (p[i + 1].1 - p[i].1).powi(2)).sqrt();
        }
        let n = ((panjang * 0.5) as usize).clamp(8, 512);
        (0..=n).map(|i| {
            let t = i as f32 / n as f32;
            de_casteljau(p, t)
        }).collect()
    }

    /// Gradien linear sepanjang sumbu `(x0,y0) -> (x1,y1)`.
    pub fn gradien_linear(&mut self, x0: f32, y0: f32, x1: f32, y1: f32,
                          c0: Colour, c1: Colour) {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let panjang2 = dx * dx + dy * dy;
        if panjang2 <= 0.0 { return; }
        let (w, h) = (self.fb.width as i32, self.fb.height as i32);
        for py in 0..h {
            for px in 0..w {
                if !self.st.boleh(px, py) { continue; }
                let t = (((px as f32 - x0) * dx + (py as f32 - y0) * dy) / panjang2).clamp(0.0, 1.0);
                self.fb.set_pixel(px, py, campur(c0, c1, t));
            }
        }
    }

    /// Gradien radial dari `(cx,cy)` sampai jari-jari `r`.
    pub fn gradien_radial(&mut self, cx: f32, cy: f32, r: f32, c0: Colour, c1: Colour) {
        if r <= 0.0 { return; }
        let (w, h) = (self.fb.width as i32, self.fb.height as i32);
        for py in 0..h {
            for px in 0..w {
                if !self.st.boleh(px, py) { continue; }
                let d = ((px as f32 - cx).powi(2) + (py as f32 - cy).powi(2)).sqrt();
                self.fb.set_pixel(px, py, campur(c0, c1, (d / r).clamp(0.0, 1.0)));
            }
        }
    }
}

fn de_casteljau(p: &[(f32, f32)], t: f32) -> (f32, f32) {
    let mut q: Vec<(f32, f32)> = p.to_vec();
    while q.len() > 1 {
        for i in 0..q.len() - 1 {
            q[i] = (q[i].0 + (q[i + 1].0 - q[i].0) * t,
                    q[i].1 + (q[i + 1].1 - q[i].1) * t);
        }
        q.pop();
    }
    q[0]
}

pub fn campur(a: Colour, b: Colour, t: f32) -> Colour {
    let l = |p: u8, q: u8| (p as f32 + (q as f32 - p as f32) * t) as u8;
    Colour::rgba(l(a.r, b.r), l(a.g, b.g), l(a.b, b.b), l(a.a, b.a))
}

/// Sudut membulat, sebagai poligon.
pub fn kotak_bulat_titik(x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<(f32, f32)> {
    // Jari-jarinya dijepit ke setengah sisi terpendek: tanpa ini, `r` yang
    // kelewat besar membuat sudut-sudutnya saling menyeberang dan
    // poligonnya jadi bentuk yang bukan kotak sama sekali.
    let r = r.min(w.abs() / 2.0).min(h.abs() / 2.0).max(0.0);
    if r <= 0.0 {
        return vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
    }
    let mut t = Vec::new();
    let seg = ((r * 0.8) as usize).clamp(4, 64);
    let sudut = [
        (x + w - r, y + h - r, 0.0f32),
        (x + r,     y + h - r, std::f32::consts::FRAC_PI_2),
        (x + r,     y + r,     std::f32::consts::PI),
        (x + w - r, y + r,     std::f32::consts::PI * 1.5),
    ];
    for (cx, cy, mulai) in sudut {
        for i in 0..=seg {
            let a = mulai + std::f32::consts::FRAC_PI_2 * i as f32 / seg as f32;
            t.push((cx + r * a.cos(), cy + r * a.sin()));
        }
    }
    t
}
