// src/ffi/wasm.rs
// Penerjemah WebAssembly (MVP core), ditulis sendiri.

use std::collections::HashMap;

// ─── Pembacaan biner ─────────────────────────────────────────────────────

pub struct Baca<'a> { pub b: &'a [u8], pub i: usize }

impl<'a> Baca<'a> {
    pub fn baru(b: &'a [u8]) -> Self { Baca { b, i: 0 } }
    pub fn habis(&self) -> bool { self.i >= self.b.len() }

    pub fn byte(&mut self) -> Result<u8, String> {
        let v = *self.b.get(self.i).ok_or("modul wasm terpotong")?;
        self.i += 1;
        Ok(v)
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.i + n > self.b.len() { return Err("modul wasm terpotong".to_string()); }
        let s = &self.b[self.i..self.i + n];
        self.i += n;
        Ok(s)
    }

    /// LEB128 tak bertanda. Batas 10 byte bukan kehati-hatian: tanpa itu
    /// sebuah berkas rusak bisa membuat loop ini berjalan selama byte-nya
    /// masih ada, dan yang terlihat adalah program yang menggantung.
    pub fn u32v(&mut self) -> Result<u32, String> {
        let mut hasil: u64 = 0;
        let mut geser = 0;
        for _ in 0..5 {
            let b = self.byte()?;
            hasil |= ((b & 0x7f) as u64) << geser;
            if b & 0x80 == 0 {
                if hasil > u32::MAX as u64 { return Err("LEB128 u32 kebesaran".to_string()); }
                return Ok(hasil as u32);
            }
            geser += 7;
        }
        Err("LEB128 u32 lebih dari 5 byte".to_string())
    }

    pub fn u64v(&mut self) -> Result<u64, String> {
        let mut hasil: u64 = 0;
        let mut geser = 0;
        for _ in 0..10 {
            let b = self.byte()?;
            hasil |= ((b & 0x7f) as u64) << geser;
            if b & 0x80 == 0 { return Ok(hasil); }
            geser += 7;
        }
        Err("LEB128 u64 lebih dari 10 byte".to_string())
    }

    pub fn i32v(&mut self) -> Result<i32, String> {
        Ok(self.i64v_bit(32)? as i32)
    }

    pub fn i64v(&mut self) -> Result<i64, String> {
        self.i64v_bit(64)
    }

    fn i64v_bit(&mut self, bit: u32) -> Result<i64, String> {
        let mut hasil: i64 = 0;
        let mut geser: u32 = 0;
        loop {
            if geser >= bit + 7 { return Err("LEB128 bertanda kepanjangan".to_string()); }
            let b = self.byte()?;
            hasil |= ((b & 0x7f) as i64) << geser;
            geser += 7;
            if b & 0x80 == 0 {
                if geser < 64 && (b & 0x40) != 0 { hasil |= -1i64 << geser; }
                return Ok(hasil);
            }
        }
    }

    pub fn f32b(&mut self) -> Result<f32, String> {
        let s = self.bytes(4)?;
        Ok(f32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    pub fn f64b(&mut self) -> Result<f64, String> {
        let s = self.bytes(8)?;
        Ok(f64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
    }

    pub fn nama(&mut self) -> Result<String, String> {
        let n = self.u32v()? as usize;
        let s = self.bytes(n)?;
        String::from_utf8(s.to_vec()).map_err(|_| "nama di modul wasm bukan UTF-8".to_string())
    }
}

// ─── Tipe ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tipe { I32, I64, F32, F64 }

impl Tipe {
    fn dari(b: u8) -> Result<Tipe, String> {
        Ok(match b {
            0x7f => Tipe::I32,
            0x7e => Tipe::I64,
            0x7d => Tipe::F32,
            0x7c => Tipe::F64,
            0x70 | 0x6f => return Err(
                "tipe referensi (funcref/externref) belum didukung".to_string()),
            0x7b => return Err("SIMD (v128) belum didukung".to_string()),
            lain => return Err(format!("tipe nilai 0x{:02x} tidak dikenal", lain)),
        })
    }
    pub fn nama(&self) -> &'static str {
        match self { Tipe::I32 => "i32", Tipe::I64 => "i64", Tipe::F32 => "f32", Tipe::F64 => "f64" }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Nilai { I32(i32), I64(i64), F32(f32), F64(f64) }

impl Nilai {
    pub fn tipe(&self) -> Tipe {
        match self {
            Nilai::I32(_) => Tipe::I32, Nilai::I64(_) => Tipe::I64,
            Nilai::F32(_) => Tipe::F32, Nilai::F64(_) => Tipe::F64,
        }
    }
    fn nol(t: Tipe) -> Nilai {
        match t {
            Tipe::I32 => Nilai::I32(0), Tipe::I64 => Nilai::I64(0),
            Tipe::F32 => Nilai::F32(0.0), Tipe::F64 => Nilai::F64(0.0),
        }
    }
    fn i32(&self) -> Result<i32, String> {
        match self { Nilai::I32(v) => Ok(*v), l => Err(salah_tipe("i32", l)) }
    }
    fn i64(&self) -> Result<i64, String> {
        match self { Nilai::I64(v) => Ok(*v), l => Err(salah_tipe("i64", l)) }
    }
    fn f32(&self) -> Result<f32, String> {
        match self { Nilai::F32(v) => Ok(*v), l => Err(salah_tipe("f32", l)) }
    }
    fn f64(&self) -> Result<f64, String> {
        match self { Nilai::F64(v) => Ok(*v), l => Err(salah_tipe("f64", l)) }
    }
}

fn salah_tipe(mau: &str, dapat: &Nilai) -> String {
    format!("stack wasm menyimpan `{}` di tempat `{}` diharapkan",
            dapat.tipe().nama(), mau)
}

#[derive(Clone, Debug)]
pub struct TipeFungsi { pub param: Vec<Tipe>, pub hasil: Vec<Tipe> }

pub struct Fungsi { pub tipe: usize, pub lokal: Vec<Tipe>, pub kode: Vec<u8> }

pub struct Modul {
    pub tipe: Vec<TipeFungsi>,
    pub fungsi: Vec<Fungsi>,
    pub ekspor: HashMap<String, usize>,
    pub global: Vec<(Tipe, bool, Nilai)>,
    pub memori_awal: u32,
    pub memori_maks: Option<u32>,
    pub data: Vec<(u32, Vec<u8>)>,
}

// ─── Parsing modul ───────────────────────────────────────────────────────

/// Baca sebuah modul `.wasm`.
pub fn baca_modul(b: &[u8]) -> Result<Modul, String> {
    let mut r = Baca::baru(b);
    let ajaib = r.bytes(4)?;
    if ajaib != b"\0asm" {
        return Err("bukan modul wasm (4 byte pertamanya bukan `\\0asm`)".to_string());
    }
    let versi = r.bytes(4)?;
    if versi != [1, 0, 0, 0] {
        return Err(format!("versi modul wasm {:?} belum didukung (yang ada: 1)", versi));
    }

    let mut m = Modul {
        tipe: Vec::new(), fungsi: Vec::new(), ekspor: HashMap::new(),
        global: Vec::new(), memori_awal: 0, memori_maks: None, data: Vec::new(),
    };
    let mut idx_tipe_fungsi: Vec<usize> = Vec::new();

    while !r.habis() {
        let id = r.byte()?;
        let ukuran = r.u32v()? as usize;
        let akhir = r.i + ukuran;
        if akhir > b.len() { return Err("panjang seksi melewati akhir berkas".to_string()); }
        match id {
            0 => { r.i = akhir; }  // seksi custom (nama, dll) — tidak dipakai
            1 => {
                let n = r.u32v()?;
                for _ in 0..n {
                    if r.byte()? != 0x60 {
                        return Err("entri seksi tipe bukan functype".to_string());
                    }
                    let np = r.u32v()?;
                    let mut param = Vec::with_capacity(np as usize);
                    for _ in 0..np { param.push(Tipe::dari(r.byte()?)?); }
                    let nh = r.u32v()?;
                    let mut hasil = Vec::with_capacity(nh as usize);
                    for _ in 0..nh { hasil.push(Tipe::dari(r.byte()?)?); }
                    m.tipe.push(TipeFungsi { param, hasil });
                }
            }
            2 => {
                let n = r.u32v()?;
                let mut nama = Vec::new();
                for _ in 0..n {
                    let modul = r.nama()?;
                    let bagian = r.nama()?;
                    nama.push(format!("{}.{}", modul, bagian));
                    let jenis = r.byte()?;
                    match jenis {
                        0x00 => { r.u32v()?; }
                        0x01 => { r.byte()?; let l = r.byte()?; r.u32v()?; if l == 1 { r.u32v()?; } }
                        0x02 => { let l = r.byte()?; r.u32v()?; if l == 1 { r.u32v()?; } }
                        0x03 => { r.byte()?; r.byte()?; }
                        _ => return Err("jenis impor tidak dikenal".to_string()),
                    }
                }
                return Err(format!(
                    "modul ini mengimpor {} hal ({}) — impor belum didukung. \
                     Modul yang butuh impor tidak bisa berjalan tanpa yang diimpornya, \
                     dan melewatinya akan menggeser indeks tiap fungsi",
                    nama.len(), nama.join(", ")));
            }
            3 => {
                let n = r.u32v()?;
                for _ in 0..n { idx_tipe_fungsi.push(r.u32v()? as usize); }
            }
            4 => { r.i = akhir; }  // tabel: cuma dipakai call_indirect, yang ditolak
            5 => {
                let n = r.u32v()?;
                if n > 1 { return Err("lebih dari satu memori belum didukung".to_string()); }
                for _ in 0..n {
                    let l = r.byte()?;
                    m.memori_awal = r.u32v()?;
                    if l == 1 { m.memori_maks = Some(r.u32v()?); }
                }
            }
            6 => {
                let n = r.u32v()?;
                for _ in 0..n {
                    let t = Tipe::dari(r.byte()?)?;
                    let bisa_ubah = r.byte()? == 1;
                    let awal = baca_ekspr_konstan(&mut r, t)?;
                    m.global.push((t, bisa_ubah, awal));
                }
            }
            7 => {
                let n = r.u32v()?;
                for _ in 0..n {
                    let nama = r.nama()?;
                    let jenis = r.byte()?;
                    let idx = r.u32v()? as usize;
                    // Cuma fungsi yang bisa dipanggil dari JRG; memori dan
                    // global yang diekspor dilewati tanpa berpura-pura ada.
                    if jenis == 0x00 { m.ekspor.insert(nama, idx); }
                }
            }
            8 => { r.i = akhir; }
            9 => { r.i = akhir; }
            10 => {
                let n = r.u32v()? as usize;
                if n != idx_tipe_fungsi.len() {
                    return Err(format!(
                        "seksi kode punya {} badan tapi seksi fungsi menyebut {}",
                        n, idx_tipe_fungsi.len()));
                }
                for k in 0..n {
                    let ukur = r.u32v()? as usize;
                    let sampai = r.i + ukur;
                    let nl = r.u32v()?;
                    let mut lokal = Vec::new();
                    for _ in 0..nl {
                        let ulang = r.u32v()?;
                        let t = Tipe::dari(r.byte()?)?;
                        if ulang > 100_000 {
                            return Err("jumlah lokal tidak masuk akal".to_string());
                        }
                        for _ in 0..ulang { lokal.push(t); }
                    }
                    if sampai > b.len() { return Err("badan fungsi terpotong".to_string()); }
                    let kode = b[r.i..sampai].to_vec();
                    r.i = sampai;
                    m.fungsi.push(Fungsi { tipe: idx_tipe_fungsi[k], lokal, kode });
                }
            }
            11 => {
                let n = r.u32v()?;
                for _ in 0..n {
                    let mode = r.u32v()?;
                    if mode != 0 {
                        return Err("segmen data pasif belum didukung".to_string());
                    }
                    let ofs = baca_ekspr_konstan(&mut r, Tipe::I32)?.i32()? as u32;
                    let panjang = r.u32v()? as usize;
                    m.data.push((ofs, r.bytes(panjang)?.to_vec()));
                }
            }
            12 => { r.i = akhir; }
            lain => return Err(format!("seksi wasm id {} tidak dikenal", lain)),
        }
        if r.i != akhir {
            return Err(format!("seksi {} tidak habis terbaca ({} byte tersisa)",
                               id, akhir.saturating_sub(r.i)));
        }
    }
    if m.fungsi.len() != idx_tipe_fungsi.len() {
        return Err("seksi fungsi ada tapi seksi kode tidak".to_string());
    }
    for f in &m.fungsi {
        if f.tipe >= m.tipe.len() {
            return Err("sebuah fungsi menunjuk indeks tipe yang tidak ada".to_string());
        }
    }
    Ok(m)
}

/// Ekspresi konstan (init global / offset data): cuma `*.const` lalu `end`.
fn baca_ekspr_konstan(r: &mut Baca, mau: Tipe) -> Result<Nilai, String> {
    let op = r.byte()?;
    let v = match op {
        0x41 => Nilai::I32(r.i32v()?),
        0x42 => Nilai::I64(r.i64v()?),
        0x43 => Nilai::F32(r.f32b()?),
        0x44 => Nilai::F64(r.f64b()?),
        0x23 => return Err("init global dari global lain belum didukung".to_string()),
        lain => return Err(format!("ekspresi konstan memakai opcode 0x{:02x}", lain)),
    };
    if r.byte()? != 0x0b {
        return Err("ekspresi konstan tidak ditutup `end`".to_string());
    }
    if v.tipe() != mau {
        return Err(format!("ekspresi konstan bertipe `{}`, harusnya `{}`",
                           v.tipe().nama(), mau.nama()));
    }
    Ok(v)
}

// ─── Penerjemah ──────────────────────────────────────────────────────────

const HALAMAN: usize = 65_536;
/// Batas memori: sebuah modul menyebut jumlah halamannya sendiri, jadi tanpa
/// batas sebuah berkas rusak (atau jahat) bisa meminta 4 TiB sebelum satu …
const MAKS_HALAMAN: u32 = 4096; // 256 MiB
const MAKS_KEDALAMAN: usize = 1024;

pub struct Mesin {
    pub modul: Modul,
    pub memori: Vec<u8>,
    pub global: Vec<Nilai>,
}

#[derive(Clone, Copy)]
enum Jenis { Blok, Loop, If }

struct Label { jenis: Jenis, arity: usize, tinggi: usize, mulai: usize, akhir: usize }

impl Mesin {
    pub fn baru(modul: Modul) -> Result<Mesin, String> {
        if modul.memori_awal > MAKS_HALAMAN {
            return Err(format!(
                "modul minta {} halaman memori ({} MiB) — batasnya {} halaman",
                modul.memori_awal, modul.memori_awal as usize * HALAMAN / 1_048_576,
                MAKS_HALAMAN));
        }
        let mut memori = vec![0u8; modul.memori_awal as usize * HALAMAN];
        for (ofs, isi) in &modul.data {
            let a = *ofs as usize;
            if a + isi.len() > memori.len() {
                return Err(format!(
                    "segmen data di offset {} panjang {} melewati memori {} byte",
                    a, isi.len(), memori.len()));
            }
            memori[a..a + isi.len()].copy_from_slice(isi);
        }
        let global = modul.global.iter().map(|(_, _, v)| *v).collect();
        Ok(Mesin { modul, memori, global })
    }

    pub fn tipe_ekspor(&self, nama: &str) -> Option<&TipeFungsi> {
        let idx = *self.modul.ekspor.get(nama)?;
        self.modul.fungsi.get(idx).map(|f| &self.modul.tipe[f.tipe])
    }

    pub fn panggil_ekspor(&mut self, nama: &str, args: &[Nilai]) -> Result<Vec<Nilai>, String> {
        let idx = *self.modul.ekspor.get(nama).ok_or_else(|| {
            let mut ada: Vec<&str> = self.modul.ekspor.keys().map(|s| s.as_str()).collect();
            ada.sort();
            format!("modul wasm tidak mengekspor fungsi `{}` — yang ada: {}",
                    nama, if ada.is_empty() { "(tidak ada)".to_string() } else { ada.join(", ") })
        })?;
        self.panggil(idx, args, 0)
    }

    fn panggil(&mut self, idx: usize, args: &[Nilai], dalam: usize) -> Result<Vec<Nilai>, String> {
        if dalam > MAKS_KEDALAMAN {
            return Err(format!("rekursi wasm melewati {} tingkat", MAKS_KEDALAMAN));
        }
        let f = self.modul.fungsi.get(idx)
            .ok_or_else(|| format!("indeks fungsi {} tidak ada", idx))?;
        let tf = self.modul.tipe[f.tipe].clone();
        if args.len() != tf.param.len() {
            return Err(format!("fungsi wasm minta {} argumen, diberi {}",
                               tf.param.len(), args.len()));
        }
        let mut lokal: Vec<Nilai> = Vec::with_capacity(tf.param.len() + f.lokal.len());
        for (i, (a, t)) in args.iter().zip(&tf.param).enumerate() {
            if a.tipe() != *t {
                return Err(format!("argumen ke-{} bertipe `{}`, fungsinya minta `{}`",
                                   i + 1, a.tipe().nama(), t.nama()));
            }
            lokal.push(*a);
        }
        for t in &f.lokal { lokal.push(Nilai::nol(*t)); }
        let kode = f.kode.clone();
        let hasil_arity = tf.hasil.len();

        let mut stack: Vec<Nilai> = Vec::new();
        self.jalankan(&kode, &mut lokal, &mut stack, dalam)?;
        if stack.len() < hasil_arity {
            return Err("fungsi wasm selesai tanpa cukup nilai hasil".to_string());
        }
        let mulai = stack.len() - hasil_arity;
        Ok(stack.split_off(mulai))
    }
}

/// Lewati immediate sebuah instruksi (opcode-nya sudah terbaca).
fn lewati_immediate(r: &mut Baca, op: u8) -> Result<(), String> {
    match op {
        0x02 | 0x03 | 0x04 => { baca_blocktype(r)?; }
        0x0c | 0x0d => { r.u32v()?; }
        0x0e => {
            let n = r.u32v()?;
            for _ in 0..n { r.u32v()?; }
            r.u32v()?;
        }
        0x10 => { r.u32v()?; }
        0x11 => { r.u32v()?; r.byte()?; }
        0x20..=0x24 => { r.u32v()?; }
        0x28..=0x3e => { r.u32v()?; r.u32v()?; }
        0x3f | 0x40 => { r.byte()?; }
        0x41 => { r.i32v()?; }
        0x42 => { r.i64v()?; }
        0x43 => { r.f32b()?; }
        0x44 => { r.f64b()?; }
        0xfc => { let sub = r.u32v()?; return Err(format!(
            "instruksi 0xfc {} (bulk memory / konversi jenuh) belum didukung", sub)); }
        0xfd => return Err("instruksi SIMD (0xfd) belum didukung".to_string()),
        0x00 | 0x01 | 0x05 | 0x0b | 0x0f | 0x1a | 0x1b => {}
        0x45..=0xc4 => {}
        lain => return Err(format!("opcode wasm 0x{:02x} tidak dikenal", lain)),
    }
    Ok(())
}

/// Tipe hasil sebuah blok. `0x40` berarti tanpa hasil; sebuah valtype berarti
/// satu hasil. Indeks tipe (multi-value) DITOLAK — menganggapnya tanpa hasil
/// akan membuat tinggi stack meleset dan nilai yang salah keluar.
fn baca_blocktype(r: &mut Baca) -> Result<usize, String> {
    let b = *r.b.get(r.i).ok_or("modul wasm terpotong")?;
    if b == 0x40 { r.i += 1; return Ok(0); }
    if matches!(b, 0x7f | 0x7e | 0x7d | 0x7c) { r.i += 1; return Ok(1); }
    Err("blok dengan tipe multi-value belum didukung".to_string())
}

/// Cari `else` dan `end` milik blok yang dimulai di `mulai` (sesudah
/// blocktype-nya).
fn pasangan(kode: &[u8], mulai: usize) -> Result<(Option<usize>, usize), String> {
    let mut r = Baca { b: kode, i: mulai };
    let mut dalam = 0usize;
    let mut lain: Option<usize> = None;
    loop {
        let pc = r.i;
        let op = r.byte()?;
        match op {
            0x02 | 0x03 | 0x04 => { dalam += 1; lewati_immediate(&mut r, op)?; }
            0x05 if dalam == 0 => { lain = Some(pc); }
            0x0b => {
                if dalam == 0 { return Ok((lain, pc)); }
                dalam -= 1;
            }
            _ => lewati_immediate(&mut r, op)?,
        }
    }
}

macro_rules! biner_i32 {
    ($s:expr, $f:expr) => {{
        let b = $s.pop().ok_or("stack wasm kosong")?.i32()?;
        let a = $s.pop().ok_or("stack wasm kosong")?.i32()?;
        $s.push(Nilai::I32($f(a, b)?));
    }};
}
macro_rules! biner_i64 {
    ($s:expr, $f:expr) => {{
        let b = $s.pop().ok_or("stack wasm kosong")?.i64()?;
        let a = $s.pop().ok_or("stack wasm kosong")?.i64()?;
        $s.push(Nilai::I64($f(a, b)?));
    }};
}
macro_rules! band_i32 {
    ($s:expr, $f:expr) => {{
        let b = $s.pop().ok_or("stack wasm kosong")?.i32()?;
        let a = $s.pop().ok_or("stack wasm kosong")?.i32()?;
        $s.push(Nilai::I32(if $f(a, b) { 1 } else { 0 }));
    }};
}
macro_rules! band_i64 {
    ($s:expr, $f:expr) => {{
        let b = $s.pop().ok_or("stack wasm kosong")?.i64()?;
        let a = $s.pop().ok_or("stack wasm kosong")?.i64()?;
        $s.push(Nilai::I32(if $f(a, b) { 1 } else { 0 }));
    }};
}
macro_rules! biner_f64 {
    ($s:expr, $f:expr) => {{
        let b = $s.pop().ok_or("stack wasm kosong")?.f64()?;
        let a = $s.pop().ok_or("stack wasm kosong")?.f64()?;
        $s.push(Nilai::F64($f(a, b)));
    }};
}
macro_rules! biner_f32 {
    ($s:expr, $f:expr) => {{
        let b = $s.pop().ok_or("stack wasm kosong")?.f32()?;
        let a = $s.pop().ok_or("stack wasm kosong")?.f32()?;
        $s.push(Nilai::F32($f(a, b)));
    }};
}
macro_rules! band_f64 {
    ($s:expr, $f:expr) => {{
        let b = $s.pop().ok_or("stack wasm kosong")?.f64()?;
        let a = $s.pop().ok_or("stack wasm kosong")?.f64()?;
        $s.push(Nilai::I32(if $f(a, b) { 1 } else { 0 }));
    }};
}
macro_rules! band_f32 {
    ($s:expr, $f:expr) => {{
        let b = $s.pop().ok_or("stack wasm kosong")?.f32()?;
        let a = $s.pop().ok_or("stack wasm kosong")?.f32()?;
        $s.push(Nilai::I32(if $f(a, b) { 1 } else { 0 }));
    }};
}

fn bagi_i32(a: i32, b: i32) -> Result<i32, String> {
    if b == 0 { return Err("wasm: pembagian integer dengan nol".to_string()); }
    a.checked_div(b).ok_or_else(|| "wasm: i32.div_s meluap (MIN / -1)".to_string())
}
fn sisa_i32(a: i32, b: i32) -> Result<i32, String> {
    if b == 0 { return Err("wasm: sisa bagi integer dengan nol".to_string()); }
    Ok(a.wrapping_rem(b))
}
fn bagi_i64(a: i64, b: i64) -> Result<i64, String> {
    if b == 0 { return Err("wasm: pembagian integer dengan nol".to_string()); }
    a.checked_div(b).ok_or_else(|| "wasm: i64.div_s meluap (MIN / -1)".to_string())
}
fn sisa_i64(a: i64, b: i64) -> Result<i64, String> {
    if b == 0 { return Err("wasm: sisa bagi integer dengan nol".to_string()); }
    Ok(a.wrapping_rem(b))
}

impl Mesin {
    fn alamat(&self, r: &mut Baca, stack: &mut Vec<Nilai>, lebar: usize) -> Result<usize, String> {
        let _align = r.u32v()?;
        let ofs = r.u32v()? as usize;
        let dasar = stack.pop().ok_or("stack wasm kosong")?.i32()? as u32 as usize;
        let a = dasar.checked_add(ofs).ok_or("alamat memori wasm meluap")?;
        if a + lebar > self.memori.len() {
            return Err(format!(
                "akses memori wasm di {}..{} melewati batas ({} byte)",
                a, a + lebar, self.memori.len()));
        }
        Ok(a)
    }

    fn jalankan(
        &mut self, kode: &[u8], lokal: &mut Vec<Nilai>,
        stack: &mut Vec<Nilai>, dalam: usize,
    ) -> Result<(), String> {
        let mut r = Baca::baru(kode);
        let mut label: Vec<Label> = Vec::new();

        loop {
            if r.habis() { return Ok(()); }
            let op = r.byte()?;
            match op {
                0x00 => return Err("wasm: `unreachable` tereksekusi".to_string()),
                0x01 => {}
                0x02 | 0x03 | 0x04 => {
                    let arity = baca_blocktype(&mut r)?;
                    let (lain, akhir) = pasangan(kode, r.i)?;
                    let jenis = match op {
                        0x02 => Jenis::Blok, 0x03 => Jenis::Loop, _ => Jenis::If,
                    };
                    if op == 0x04 {
                        let c = stack.pop().ok_or("stack wasm kosong")?.i32()?;
                        if c == 0 {
                            match lain {
                                Some(e) => {
                                    label.push(Label { jenis, arity, tinggi: stack.len(),
                                                       mulai: r.i, akhir });
                                    r.i = e + 1;
                                }
                                None => { r.i = akhir + 1; }
                            }
                            continue;
                        }
                    }
                    label.push(Label { jenis, arity, tinggi: stack.len(), mulai: r.i, akhir });
                }
                0x05 => {
                    // Akhir cabang `then`: lompat ke `end` milik blok ini.
                    let l = label.last().ok_or("`else` tanpa `if`")?;
                    r.i = l.akhir;
                }
                0x0b => {
                    if label.pop().is_none() { return Ok(()); }
                }
                0x0c => { let l = r.u32v()? as usize; cabang(stack, &mut label, l, &mut r)?; }
                0x0d => {
                    let l = r.u32v()? as usize;
                    let c = stack.pop().ok_or("stack wasm kosong")?.i32()?;
                    if c != 0 { cabang(stack, &mut label, l, &mut r)?; }
                }
                0x0e => {
                    let n = r.u32v()?;
                    let mut target = Vec::with_capacity(n as usize);
                    for _ in 0..n { target.push(r.u32v()? as usize); }
                    let bawaan = r.u32v()? as usize;
                    let i = stack.pop().ok_or("stack wasm kosong")?.i32()?;
                    let l = if i < 0 || i as usize >= target.len() { bawaan }
                            else { target[i as usize] };
                    cabang(stack, &mut label, l, &mut r)?;
                }
                0x0f => return Ok(()),
                0x10 => {
                    let idx = r.u32v()? as usize;
                    let tf = {
                        let f = self.modul.fungsi.get(idx)
                            .ok_or_else(|| format!("call ke indeks fungsi {} yang tidak ada", idx))?;
                        self.modul.tipe[f.tipe].clone()
                    };
                    if stack.len() < tf.param.len() {
                        return Err("stack wasm kurang untuk sebuah `call`".to_string());
                    }
                    let mulai = stack.len() - tf.param.len();
                    let args: Vec<Nilai> = stack.split_off(mulai);
                    let hasil = self.panggil(idx, &args, dalam + 1)?;
                    stack.extend(hasil);
                }
                0x11 => return Err(
                    "`call_indirect` belum didukung (butuh seksi tabel)".to_string()),
                0x1a => { stack.pop().ok_or("stack wasm kosong")?; }
                0x1b => {
                    let c = stack.pop().ok_or("stack wasm kosong")?.i32()?;
                    let b = stack.pop().ok_or("stack wasm kosong")?;
                    let a = stack.pop().ok_or("stack wasm kosong")?;
                    stack.push(if c != 0 { a } else { b });
                }
                0x20 => {
                    let i = r.u32v()? as usize;
                    stack.push(*lokal.get(i).ok_or_else(|| format!("local.get {} tidak ada", i))?);
                }
                0x21 => {
                    let i = r.u32v()? as usize;
                    let v = stack.pop().ok_or("stack wasm kosong")?;
                    *lokal.get_mut(i).ok_or_else(|| format!("local.set {} tidak ada", i))? = v;
                }
                0x22 => {
                    let i = r.u32v()? as usize;
                    let v = *stack.last().ok_or("stack wasm kosong")?;
                    *lokal.get_mut(i).ok_or_else(|| format!("local.tee {} tidak ada", i))? = v;
                }
                0x23 => {
                    let i = r.u32v()? as usize;
                    stack.push(*self.global.get(i).ok_or_else(|| format!("global {} tidak ada", i))?);
                }
                0x24 => {
                    let i = r.u32v()? as usize;
                    let v = stack.pop().ok_or("stack wasm kosong")?;
                    let (_, bisa_ubah, _) = *self.modul.global.get(i)
                        .ok_or_else(|| format!("global {} tidak ada", i))?;
                    if !bisa_ubah {
                        return Err(format!("global {} bukan `mut`, tidak bisa diubah", i));
                    }
                    self.global[i] = v;
                }
                0x28 => { let a = self.alamat(&mut r, stack, 4)?;
                    stack.push(Nilai::I32(i32::from_le_bytes(self.memori[a..a+4].try_into().unwrap()))); }
                0x29 => { let a = self.alamat(&mut r, stack, 8)?;
                    stack.push(Nilai::I64(i64::from_le_bytes(self.memori[a..a+8].try_into().unwrap()))); }
                0x2a => { let a = self.alamat(&mut r, stack, 4)?;
                    stack.push(Nilai::F32(f32::from_le_bytes(self.memori[a..a+4].try_into().unwrap()))); }
                0x2b => { let a = self.alamat(&mut r, stack, 8)?;
                    stack.push(Nilai::F64(f64::from_le_bytes(self.memori[a..a+8].try_into().unwrap()))); }
                0x2c => { let a = self.alamat(&mut r, stack, 1)?;
                    stack.push(Nilai::I32(self.memori[a] as i8 as i32)); }
                0x2d => { let a = self.alamat(&mut r, stack, 1)?;
                    stack.push(Nilai::I32(self.memori[a] as i32)); }
                0x2e => { let a = self.alamat(&mut r, stack, 2)?;
                    stack.push(Nilai::I32(i16::from_le_bytes(self.memori[a..a+2].try_into().unwrap()) as i32)); }
                0x2f => { let a = self.alamat(&mut r, stack, 2)?;
                    stack.push(Nilai::I32(u16::from_le_bytes(self.memori[a..a+2].try_into().unwrap()) as i32)); }
                0x30 => { let a = self.alamat(&mut r, stack, 1)?;
                    stack.push(Nilai::I64(self.memori[a] as i8 as i64)); }
                0x31 => { let a = self.alamat(&mut r, stack, 1)?;
                    stack.push(Nilai::I64(self.memori[a] as i64)); }
                0x32 => { let a = self.alamat(&mut r, stack, 2)?;
                    stack.push(Nilai::I64(i16::from_le_bytes(self.memori[a..a+2].try_into().unwrap()) as i64)); }
                0x33 => { let a = self.alamat(&mut r, stack, 2)?;
                    stack.push(Nilai::I64(u16::from_le_bytes(self.memori[a..a+2].try_into().unwrap()) as i64)); }
                0x34 => { let a = self.alamat(&mut r, stack, 4)?;
                    stack.push(Nilai::I64(i32::from_le_bytes(self.memori[a..a+4].try_into().unwrap()) as i64)); }
                0x35 => { let a = self.alamat(&mut r, stack, 4)?;
                    stack.push(Nilai::I64(u32::from_le_bytes(self.memori[a..a+4].try_into().unwrap()) as i64)); }
                0x36 => { let v = stack.pop().ok_or("stack wasm kosong")?.i32()?;
                    let a = self.alamat(&mut r, stack, 4)?;
                    self.memori[a..a+4].copy_from_slice(&v.to_le_bytes()); }
                0x37 => { let v = stack.pop().ok_or("stack wasm kosong")?.i64()?;
                    let a = self.alamat(&mut r, stack, 8)?;
                    self.memori[a..a+8].copy_from_slice(&v.to_le_bytes()); }
                0x38 => { let v = stack.pop().ok_or("stack wasm kosong")?.f32()?;
                    let a = self.alamat(&mut r, stack, 4)?;
                    self.memori[a..a+4].copy_from_slice(&v.to_le_bytes()); }
                0x39 => { let v = stack.pop().ok_or("stack wasm kosong")?.f64()?;
                    let a = self.alamat(&mut r, stack, 8)?;
                    self.memori[a..a+8].copy_from_slice(&v.to_le_bytes()); }
                0x3a => { let v = stack.pop().ok_or("stack wasm kosong")?.i32()?;
                    let a = self.alamat(&mut r, stack, 1)?; self.memori[a] = v as u8; }
                0x3b => { let v = stack.pop().ok_or("stack wasm kosong")?.i32()?;
                    let a = self.alamat(&mut r, stack, 2)?;
                    self.memori[a..a+2].copy_from_slice(&(v as u16).to_le_bytes()); }
                0x3c => { let v = stack.pop().ok_or("stack wasm kosong")?.i64()?;
                    let a = self.alamat(&mut r, stack, 1)?; self.memori[a] = v as u8; }
                0x3d => { let v = stack.pop().ok_or("stack wasm kosong")?.i64()?;
                    let a = self.alamat(&mut r, stack, 2)?;
                    self.memori[a..a+2].copy_from_slice(&(v as u16).to_le_bytes()); }
                0x3e => { let v = stack.pop().ok_or("stack wasm kosong")?.i64()?;
                    let a = self.alamat(&mut r, stack, 4)?;
                    self.memori[a..a+4].copy_from_slice(&(v as u32).to_le_bytes()); }
                0x3f => { r.byte()?; stack.push(Nilai::I32((self.memori.len() / HALAMAN) as i32)); }
                0x40 => {
                    r.byte()?;
                    let tambah = stack.pop().ok_or("stack wasm kosong")?.i32()? as u32;
                    let lama = (self.memori.len() / HALAMAN) as u32;
                    match lama.checked_add(tambah) {
                        Some(baru) if baru <= MAKS_HALAMAN
                            && self.modul.memori_maks.map_or(true, |m| baru <= m) => {
                            self.memori.resize(baru as usize * HALAMAN, 0);
                            stack.push(Nilai::I32(lama as i32));
                        }
                        // `memory.grow` memang MENJAWAB -1 kalau gagal; itu
                        // bagian dari kontraknya, bukan kegagalan yang
                        // disembunyikan.
                        _ => stack.push(Nilai::I32(-1)),
                    }
                }
                0x41 => { let v = r.i32v()?; stack.push(Nilai::I32(v)); }
                0x42 => { let v = r.i64v()?; stack.push(Nilai::I64(v)); }
                0x43 => { let v = r.f32b()?; stack.push(Nilai::F32(v)); }
                0x44 => { let v = r.f64b()?; stack.push(Nilai::F64(v)); }
                _ => self.numerik(op, stack)?,
            }
        }
    }
}

fn cabang(
    stack: &mut Vec<Nilai>, label: &mut Vec<Label>, l: usize, r: &mut Baca,
) -> Result<(), String> {
    if l >= label.len() {
        return Err(format!("br {} melewati {} label yang ada", l, label.len()));
    }
    let idx = label.len() - 1 - l;
    let lab_jenis = label[idx].jenis;
    let lab_tinggi = label[idx].tinggi;
    let lab_arity = label[idx].arity;
    let lab_mulai = label[idx].mulai;
    let lab_akhir = label[idx].akhir;
    // Sebuah `loop` yang di-`br` berarti MENGULANG: hasilnya bukan yang
    // dibawa, dan labelnya tetap ada karena kita masuk lagi ke dalamnya.
    let bawa = match lab_jenis { Jenis::Loop => 0, _ => lab_arity };
    if stack.len() < lab_tinggi + bawa {
        return Err("stack wasm kurang untuk sebuah `br`".to_string());
    }
    let mut simpan = Vec::with_capacity(bawa);
    for _ in 0..bawa { simpan.push(stack.pop().unwrap()); }
    stack.truncate(lab_tinggi);
    while let Some(v) = simpan.pop() { stack.push(v); }
    match lab_jenis {
        Jenis::Loop => { label.truncate(idx + 1); r.i = lab_mulai; }
        _ => { label.truncate(idx); r.i = lab_akhir + 1; }
    }
    Ok(())
}

macro_rules! unari {
    ($s:expr, $amb:ident, $bung:ident, $f:expr) => {{
        let a = $s.pop().ok_or("stack wasm kosong")?.$amb()?;
        $s.push(Nilai::$bung($f(a)));
    }};
}

/// Konversi float->int MENJEBAK pada NaN dan di luar jangkauan, sesuai
/// spesifikasi. Membiarkan `as` Rust menjenuhkannya akan menghasilkan angka
/// yang masuk akal dan salah — persis kelas yang dilarang berkas ini.
fn trunc_f64_i32(v: f64, bertanda: bool) -> Result<i32, String> {
    if v.is_nan() { return Err("wasm: konversi NaN ke integer".to_string()); }
    let v = v.trunc();
    if bertanda {
        if v < i32::MIN as f64 || v > i32::MAX as f64 {
            return Err("wasm: konversi float ke i32 di luar jangkauan".to_string());
        }
        Ok(v as i32)
    } else {
        if v < 0.0 || v > u32::MAX as f64 {
            return Err("wasm: konversi float ke u32 di luar jangkauan".to_string());
        }
        Ok(v as u32 as i32)
    }
}

fn trunc_f64_i64(v: f64, bertanda: bool) -> Result<i64, String> {
    if v.is_nan() { return Err("wasm: konversi NaN ke integer".to_string()); }
    let v = v.trunc();
    if bertanda {
        if v < i64::MIN as f64 || v >= 9.223372036854776e18 {
            return Err("wasm: konversi float ke i64 di luar jangkauan".to_string());
        }
        Ok(v as i64)
    } else {
        if v < 0.0 || v >= 1.8446744073709552e19 {
            return Err("wasm: konversi float ke u64 di luar jangkauan".to_string());
        }
        Ok(v as u64 as i64)
    }
}

impl Mesin {
    fn numerik(&mut self, op: u8, s: &mut Vec<Nilai>) -> Result<(), String> {
        match op {
            0x45 => unari!(s, i32, I32, |a: i32| if a == 0 { 1 } else { 0 }),
            0x46 => band_i32!(s, |a: i32, b: i32| a == b),
            0x47 => band_i32!(s, |a: i32, b: i32| a != b),
            0x48 => band_i32!(s, |a: i32, b: i32| a < b),
            0x49 => band_i32!(s, |a: i32, b: i32| (a as u32) < (b as u32)),
            0x4a => band_i32!(s, |a: i32, b: i32| a > b),
            0x4b => band_i32!(s, |a: i32, b: i32| (a as u32) > (b as u32)),
            0x4c => band_i32!(s, |a: i32, b: i32| a <= b),
            0x4d => band_i32!(s, |a: i32, b: i32| (a as u32) <= (b as u32)),
            0x4e => band_i32!(s, |a: i32, b: i32| a >= b),
            0x4f => band_i32!(s, |a: i32, b: i32| (a as u32) >= (b as u32)),

            0x50 => { let a = s.pop().ok_or("stack wasm kosong")?.i64()?;
                      s.push(Nilai::I32(if a == 0 { 1 } else { 0 })); }
            0x51 => band_i64!(s, |a: i64, b: i64| a == b),
            0x52 => band_i64!(s, |a: i64, b: i64| a != b),
            0x53 => band_i64!(s, |a: i64, b: i64| a < b),
            0x54 => band_i64!(s, |a: i64, b: i64| (a as u64) < (b as u64)),
            0x55 => band_i64!(s, |a: i64, b: i64| a > b),
            0x56 => band_i64!(s, |a: i64, b: i64| (a as u64) > (b as u64)),
            0x57 => band_i64!(s, |a: i64, b: i64| a <= b),
            0x58 => band_i64!(s, |a: i64, b: i64| (a as u64) <= (b as u64)),
            0x59 => band_i64!(s, |a: i64, b: i64| a >= b),
            0x5a => band_i64!(s, |a: i64, b: i64| (a as u64) >= (b as u64)),

            0x5b => band_f32!(s, |a: f32, b: f32| a == b),
            0x5c => band_f32!(s, |a: f32, b: f32| a != b),
            0x5d => band_f32!(s, |a: f32, b: f32| a < b),
            0x5e => band_f32!(s, |a: f32, b: f32| a > b),
            0x5f => band_f32!(s, |a: f32, b: f32| a <= b),
            0x60 => band_f32!(s, |a: f32, b: f32| a >= b),
            0x61 => band_f64!(s, |a: f64, b: f64| a == b),
            0x62 => band_f64!(s, |a: f64, b: f64| a != b),
            0x63 => band_f64!(s, |a: f64, b: f64| a < b),
            0x64 => band_f64!(s, |a: f64, b: f64| a > b),
            0x65 => band_f64!(s, |a: f64, b: f64| a <= b),
            0x66 => band_f64!(s, |a: f64, b: f64| a >= b),

            0x67 => unari!(s, i32, I32, |a: i32| a.leading_zeros() as i32),
            0x68 => unari!(s, i32, I32, |a: i32| a.trailing_zeros() as i32),
            0x69 => unari!(s, i32, I32, |a: i32| a.count_ones() as i32),
            0x6a => biner_i32!(s, |a: i32, b: i32| -> Result<i32, String> { Ok(a.wrapping_add(b)) }),
            0x6b => biner_i32!(s, |a: i32, b: i32| -> Result<i32, String> { Ok(a.wrapping_sub(b)) }),
            0x6c => biner_i32!(s, |a: i32, b: i32| -> Result<i32, String> { Ok(a.wrapping_mul(b)) }),
            0x6d => biner_i32!(s, bagi_i32),
            0x6e => biner_i32!(s, |a: i32, b: i32| if b == 0 {
                        Err("wasm: pembagian integer dengan nol".to_string())
                    } else { Ok(((a as u32) / (b as u32)) as i32) }),
            0x6f => biner_i32!(s, sisa_i32),
            0x70 => biner_i32!(s, |a: i32, b: i32| if b == 0 {
                        Err("wasm: sisa bagi integer dengan nol".to_string())
                    } else { Ok(((a as u32) % (b as u32)) as i32) }),
            0x71 => biner_i32!(s, |a: i32, b: i32| -> Result<i32, String> { Ok(a & b) }),
            0x72 => biner_i32!(s, |a: i32, b: i32| -> Result<i32, String> { Ok(a | b) }),
            0x73 => biner_i32!(s, |a: i32, b: i32| -> Result<i32, String> { Ok(a ^ b) }),
            0x74 => biner_i32!(s, |a: i32, b: i32| -> Result<i32, String> { Ok(a.wrapping_shl(b as u32 & 31)) }),
            0x75 => biner_i32!(s, |a: i32, b: i32| -> Result<i32, String> { Ok(a.wrapping_shr(b as u32 & 31)) }),
            0x76 => biner_i32!(s, |a: i32, b: i32| -> Result<i32, String> { Ok(((a as u32) >> (b as u32 & 31)) as i32) }),
            0x77 => biner_i32!(s, |a: i32, b: i32| -> Result<i32, String> { Ok(a.rotate_left(b as u32 & 31)) }),
            0x78 => biner_i32!(s, |a: i32, b: i32| -> Result<i32, String> { Ok(a.rotate_right(b as u32 & 31)) }),

            0x79 => unari!(s, i64, I64, |a: i64| a.leading_zeros() as i64),
            0x7a => unari!(s, i64, I64, |a: i64| a.trailing_zeros() as i64),
            0x7b => unari!(s, i64, I64, |a: i64| a.count_ones() as i64),
            0x7c => biner_i64!(s, |a: i64, b: i64| -> Result<i64, String> { Ok(a.wrapping_add(b)) }),
            0x7d => biner_i64!(s, |a: i64, b: i64| -> Result<i64, String> { Ok(a.wrapping_sub(b)) }),
            0x7e => biner_i64!(s, |a: i64, b: i64| -> Result<i64, String> { Ok(a.wrapping_mul(b)) }),
            0x7f => biner_i64!(s, bagi_i64),
            0x80 => biner_i64!(s, |a: i64, b: i64| if b == 0 {
                        Err("wasm: pembagian integer dengan nol".to_string())
                    } else { Ok(((a as u64) / (b as u64)) as i64) }),
            0x81 => biner_i64!(s, sisa_i64),
            0x82 => biner_i64!(s, |a: i64, b: i64| if b == 0 {
                        Err("wasm: sisa bagi integer dengan nol".to_string())
                    } else { Ok(((a as u64) % (b as u64)) as i64) }),
            0x83 => biner_i64!(s, |a: i64, b: i64| -> Result<i64, String> { Ok(a & b) }),
            0x84 => biner_i64!(s, |a: i64, b: i64| -> Result<i64, String> { Ok(a | b) }),
            0x85 => biner_i64!(s, |a: i64, b: i64| -> Result<i64, String> { Ok(a ^ b) }),
            0x86 => biner_i64!(s, |a: i64, b: i64| -> Result<i64, String> { Ok(a.wrapping_shl(b as u32 & 63)) }),
            0x87 => biner_i64!(s, |a: i64, b: i64| -> Result<i64, String> { Ok(a.wrapping_shr(b as u32 & 63)) }),
            0x88 => biner_i64!(s, |a: i64, b: i64| -> Result<i64, String> { Ok(((a as u64) >> (b as u32 & 63)) as i64) }),
            0x89 => biner_i64!(s, |a: i64, b: i64| -> Result<i64, String> { Ok(a.rotate_left(b as u32 & 63)) }),
            0x8a => biner_i64!(s, |a: i64, b: i64| -> Result<i64, String> { Ok(a.rotate_right(b as u32 & 63)) }),

            0x8b => unari!(s, f32, F32, |a: f32| a.abs()),
            0x8c => unari!(s, f32, F32, |a: f32| -a),
            0x8d => unari!(s, f32, F32, |a: f32| a.ceil()),
            0x8e => unari!(s, f32, F32, |a: f32| a.floor()),
            0x8f => unari!(s, f32, F32, |a: f32| a.trunc()),
            0x90 => unari!(s, f32, F32, |a: f32| bulat_genap32(a)),
            0x91 => unari!(s, f32, F32, |a: f32| a.sqrt()),
            0x92 => biner_f32!(s, |a: f32, b: f32| a + b),
            0x93 => biner_f32!(s, |a: f32, b: f32| a - b),
            0x94 => biner_f32!(s, |a: f32, b: f32| a * b),
            0x95 => biner_f32!(s, |a: f32, b: f32| a / b),
            0x96 => biner_f32!(s, |a: f32, b: f32| min_wasm32(a, b)),
            0x97 => biner_f32!(s, |a: f32, b: f32| maks_wasm32(a, b)),
            0x98 => biner_f32!(s, |a: f32, b: f32| a.copysign(b)),

            0x99 => unari!(s, f64, F64, |a: f64| a.abs()),
            0x9a => unari!(s, f64, F64, |a: f64| -a),
            0x9b => unari!(s, f64, F64, |a: f64| a.ceil()),
            0x9c => unari!(s, f64, F64, |a: f64| a.floor()),
            0x9d => unari!(s, f64, F64, |a: f64| a.trunc()),
            0x9e => unari!(s, f64, F64, |a: f64| bulat_genap64(a)),
            0x9f => unari!(s, f64, F64, |a: f64| a.sqrt()),
            0xa0 => biner_f64!(s, |a: f64, b: f64| a + b),
            0xa1 => biner_f64!(s, |a: f64, b: f64| a - b),
            0xa2 => biner_f64!(s, |a: f64, b: f64| a * b),
            0xa3 => biner_f64!(s, |a: f64, b: f64| a / b),
            0xa4 => biner_f64!(s, |a: f64, b: f64| min_wasm64(a, b)),
            0xa5 => biner_f64!(s, |a: f64, b: f64| maks_wasm64(a, b)),
            0xa6 => biner_f64!(s, |a: f64, b: f64| a.copysign(b)),

            0xa7 => { let a = s.pop().ok_or("stack wasm kosong")?.i64()?;
                      s.push(Nilai::I32(a as i32)); }
            0xa8 => { let a = s.pop().ok_or("stack wasm kosong")?.f32()?;
                      s.push(Nilai::I32(trunc_f64_i32(a as f64, true)?)); }
            0xa9 => { let a = s.pop().ok_or("stack wasm kosong")?.f32()?;
                      s.push(Nilai::I32(trunc_f64_i32(a as f64, false)?)); }
            0xaa => { let a = s.pop().ok_or("stack wasm kosong")?.f64()?;
                      s.push(Nilai::I32(trunc_f64_i32(a, true)?)); }
            0xab => { let a = s.pop().ok_or("stack wasm kosong")?.f64()?;
                      s.push(Nilai::I32(trunc_f64_i32(a, false)?)); }
            0xac => { let a = s.pop().ok_or("stack wasm kosong")?.i32()?;
                      s.push(Nilai::I64(a as i64)); }
            0xad => { let a = s.pop().ok_or("stack wasm kosong")?.i32()?;
                      s.push(Nilai::I64(a as u32 as i64)); }
            0xae => { let a = s.pop().ok_or("stack wasm kosong")?.f32()?;
                      s.push(Nilai::I64(trunc_f64_i64(a as f64, true)?)); }
            0xaf => { let a = s.pop().ok_or("stack wasm kosong")?.f32()?;
                      s.push(Nilai::I64(trunc_f64_i64(a as f64, false)?)); }
            0xb0 => { let a = s.pop().ok_or("stack wasm kosong")?.f64()?;
                      s.push(Nilai::I64(trunc_f64_i64(a, true)?)); }
            0xb1 => { let a = s.pop().ok_or("stack wasm kosong")?.f64()?;
                      s.push(Nilai::I64(trunc_f64_i64(a, false)?)); }
            0xb2 => { let a = s.pop().ok_or("stack wasm kosong")?.i32()?;
                      s.push(Nilai::F32(a as f32)); }
            0xb3 => { let a = s.pop().ok_or("stack wasm kosong")?.i32()?;
                      s.push(Nilai::F32(a as u32 as f32)); }
            0xb4 => { let a = s.pop().ok_or("stack wasm kosong")?.i64()?;
                      s.push(Nilai::F32(a as f32)); }
            0xb5 => { let a = s.pop().ok_or("stack wasm kosong")?.i64()?;
                      s.push(Nilai::F32(a as u64 as f32)); }
            0xb6 => { let a = s.pop().ok_or("stack wasm kosong")?.f64()?;
                      s.push(Nilai::F32(a as f32)); }
            0xb7 => { let a = s.pop().ok_or("stack wasm kosong")?.i32()?;
                      s.push(Nilai::F64(a as f64)); }
            0xb8 => { let a = s.pop().ok_or("stack wasm kosong")?.i32()?;
                      s.push(Nilai::F64(a as u32 as f64)); }
            0xb9 => { let a = s.pop().ok_or("stack wasm kosong")?.i64()?;
                      s.push(Nilai::F64(a as f64)); }
            0xba => { let a = s.pop().ok_or("stack wasm kosong")?.i64()?;
                      s.push(Nilai::F64(a as u64 as f64)); }
            0xbb => { let a = s.pop().ok_or("stack wasm kosong")?.f32()?;
                      s.push(Nilai::F64(a as f64)); }
            0xbc => { let a = s.pop().ok_or("stack wasm kosong")?.f32()?;
                      s.push(Nilai::I32(a.to_bits() as i32)); }
            0xbd => { let a = s.pop().ok_or("stack wasm kosong")?.f64()?;
                      s.push(Nilai::I64(a.to_bits() as i64)); }
            0xbe => { let a = s.pop().ok_or("stack wasm kosong")?.i32()?;
                      s.push(Nilai::F32(f32::from_bits(a as u32))); }
            0xbf => { let a = s.pop().ok_or("stack wasm kosong")?.i64()?;
                      s.push(Nilai::F64(f64::from_bits(a as u64))); }
            0xc0 => unari!(s, i32, I32, |a: i32| a as i8 as i32),
            0xc1 => unari!(s, i32, I32, |a: i32| a as i16 as i32),
            0xc2 => unari!(s, i64, I64, |a: i64| a as i8 as i64),
            0xc3 => unari!(s, i64, I64, |a: i64| a as i16 as i64),
            0xc4 => unari!(s, i64, I64, |a: i64| a as i32 as i64),
            lain => return Err(format!("opcode wasm 0x{:02x} belum didukung", lain)),
        }
        Ok(())
    }
}

/// `nearest` WASM membulatkan ke GENAP di titik tengah (round-half-to-even),
/// bukan menjauh dari nol seperti `f64::round` di Rust. Memakai `round` akan
/// membuat 2.5 jadi 3 sementara setiap runtime WASM lain menjawab 2.
fn bulat_genap64(a: f64) -> f64 {
    let b = a.round();
    if (a - a.trunc()).abs() == 0.5 && b % 2.0 != 0.0 { b - a.signum() } else { b }
}
fn bulat_genap32(a: f32) -> f32 { bulat_genap64(a as f64) as f32 }

/// `min`/`max` WASM menular NaN; `f64::min` di Rust justru MENGABAIKAN NaN.
fn min_wasm64(a: f64, b: f64) -> f64 { if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) } }
fn maks_wasm64(a: f64, b: f64) -> f64 { if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) } }
fn min_wasm32(a: f32, b: f32) -> f32 { if a.is_nan() || b.is_nan() { f32::NAN } else { a.min(b) } }
fn maks_wasm32(a: f32, b: f32) -> f32 { if a.is_nan() || b.is_nan() { f32::NAN } else { a.max(b) } }

// ─── Pintu builtin ───────────────────────────────────────────────────────

use std::sync::Mutex;

use crate::interpreter::{err_value, ok_value, Value};

/// Modul yang sedang dimuat, dipegang lewat NOMOR.
static REGISTRI: Mutex<Vec<Option<Mesin>>> = Mutex::new(Vec::new());

fn handle(id: usize) -> Value {
    let mut f = std::collections::HashMap::new();
    f.insert("id".to_string(), Value::Int(id as i128));
    Value::Struct { name: "WasmModul".to_string(), fields: f.into() }
}

fn id_dari(v: &Value) -> Result<usize, String> {
    match v {
        Value::Struct { name, fields } if name == "WasmModul" => match fields.get("id") {
            Some(Value::Int(i)) if usize::try_from(*i).is_ok() => Ok(*i as usize),
            _ => Err("handle wasm rusak".to_string()),
        },
        lain => Err(format!("butuh handle modul wasm, dapat `{}`", lain.type_name())),
    }
}

fn nilai_ke_wasm(v: &Value, t: Tipe, ke: usize) -> Result<Nilai, String> {
    let salah = || format!(
        "argumen ke-{} harus cocok `{}`, dapat `{}`", ke + 1, t.nama(), v.type_name());
    Ok(match (t, v) {
        (Tipe::I32, Value::Int(i)) => Nilai::I32(i32::try_from(*i).map_err(|_| format!(
            "argumen ke-{} ({}) tidak muat di `i32`", ke + 1, i))?),
        (Tipe::I64, Value::Int(i)) => Nilai::I64(i64::try_from(*i).map_err(|_| format!(
            "argumen ke-{} ({}) tidak muat di `i64`", ke + 1, i))?),
        (Tipe::F32, Value::Float(f)) => Nilai::F32(*f as f32),
        (Tipe::F32, Value::Int(i)) => Nilai::F32(*i as f32),
        (Tipe::F64, Value::Float(f)) => Nilai::F64(*f),
        (Tipe::F64, Value::Int(i)) => Nilai::F64(*i as f64),
        _ => return Err(salah()),
    })
}

fn wasm_ke_nilai(v: Nilai) -> Value {
    match v {
        Nilai::I32(i) => Value::Int(i as i128),
        Nilai::I64(i) => Value::Int(i as i128),
        Nilai::F32(f) => Value::Float(f as f64),
        Nilai::F64(f) => Value::Float(f),
    }
}

fn teks(v: &Value, nama: &str, ke: usize) -> Result<String, String> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        lain => Err(format!("{}: argumen ke-{} harus string, dapat `{}`",
                            nama, ke + 1, lain.type_name())),
    }
}

fn bilangan(v: &Value, nama: &str, ke: usize) -> Result<i64, String> {
    match v {
        Value::Int(i) => i64::try_from(*i).map_err(|_| format!("{}: angka kebesaran", nama)),
        lain => Err(format!("{}: argumen ke-{} harus int, dapat `{}`",
                            nama, ke + 1, lain.type_name())),
    }
}

pub fn b_muat(a: &[Value]) -> Result<Value, String> {
    if a.len() != 1 { return Err("wasm::muat(path)".to_string()); }
    let path = teks(a.first().ok_or("wasm::muat(path)")?, "wasm::muat", 0)?;
    let isi = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => return Ok(err_value(format!("tidak bisa baca `{}`: {}", path, crate::host::galat_io(&e)))),
    };
    let modul = match baca_modul(&isi) {
        Ok(m) => m,
        Err(e) => return Ok(err_value(format!("`{}`: {}", path, e))),
    };
    let mesin = match Mesin::baru(modul) {
        Ok(m) => m,
        Err(e) => return Ok(err_value(format!("`{}`: {}", path, e))),
    };
    let mut reg = REGISTRI.lock().map_err(|_| "registri wasm rusak".to_string())?;
    reg.push(Some(mesin));
    Ok(ok_value(handle(reg.len() - 1)))
}

pub fn b_ekspor(a: &[Value]) -> Result<Value, String> {
    if a.len() != 1 { return Err("wasm::ekspor(modul)".to_string()); }
    let id = id_dari(a.first().ok_or("wasm::ekspor(modul)")?)?;
    let reg = REGISTRI.lock().map_err(|_| "registri wasm rusak".to_string())?;
    match reg.get(id).and_then(|m| m.as_ref()) {
        None => Ok(err_value("modul wasm ini sudah ditutup")),
        Some(m) => {
            let mut nama: Vec<&str> = m.modul.ekspor.keys().map(|s| s.as_str()).collect();
            nama.sort();
            let v: Vec<Value> = nama.into_iter().map(|n| Value::Str(n.to_string())).collect();
            Ok(ok_value(Value::Array(v.into())))
        }
    }
}

pub fn b_panggil(a: &[Value]) -> Result<Value, String> {
    if a.len() < 2 || a.len() > 3 {
        return Err("wasm::panggil(modul, nama, args)".to_string());
    }
    let id = id_dari(&a[0])?;
    let nama = teks(&a[1], "wasm::panggil", 1)?;
    let args: Vec<Value> = match a.get(2) {
        None => Vec::new(),
        Some(Value::Array(arr)) => arr.iter().cloned().collect(),
        Some(lain) => return Err(format!(
            "wasm::panggil: argumen ke-3 harus array, dapat `{}`", lain.type_name())),
    };
    let mut reg = REGISTRI.lock().map_err(|_| "registri wasm rusak".to_string())?;
    let Some(mesin) = reg.get_mut(id).and_then(|m| m.as_mut()) else {
        return Ok(err_value("modul wasm ini sudah ditutup"));
    };
    let tf = match mesin.tipe_ekspor(&nama) {
        Some(t) => t.clone(),
        None => {
            let mut ada: Vec<&str> = mesin.modul.ekspor.keys().map(|s| s.as_str()).collect();
            ada.sort();
            return Ok(err_value(format!(
                "modul wasm tidak mengekspor fungsi `{}` — yang ada: {}",
                nama, if ada.is_empty() { "(tidak ada)".to_string() } else { ada.join(", ") })));
        }
    };
    if tf.param.len() != args.len() {
        return Ok(err_value(format!(
            "`{}` minta {} argumen, diberi {}", nama, tf.param.len(), args.len())));
    }
    let mut w = Vec::with_capacity(args.len());
    for (i, (v, t)) in args.iter().zip(&tf.param).enumerate() {
        match nilai_ke_wasm(v, *t, i) {
            Ok(x) => w.push(x),
            // Tipe argumen yang salah adalah bug di PROGRAMNYA, bukan keadaan
            // dunia — jadi galat keras, sama seperti aturan `dok::`.
            Err(e) => return Err(format!("wasm::panggil `{}`: {}", nama, e)),
        }
    }
    match mesin.panggil_ekspor(&nama, &w) {
        Err(e) => Ok(err_value(e)),
        Ok(h) => Ok(ok_value(match h.len() {
            0 => Value::Unit,
            1 => wasm_ke_nilai(h[0]),
            _ => {
                let v: Vec<Value> = h.into_iter().map(wasm_ke_nilai).collect();
                Value::Array(v.into())
            }
        })),
    }
}

pub fn b_baca_memori(a: &[Value]) -> Result<Value, String> { baca_memori(a, "wasm::baca_memori", false) }

/// `wasm::baca_memori_bytes`: byte apa pun, sebagai array bilangan 0 sampai 255.
pub fn b_baca_memori_bytes(a: &[Value]) -> Result<Value, String> { baca_memori(a, "wasm::baca_memori_bytes", true) }

fn baca_memori(a: &[Value], nama: &str, byte: bool) -> Result<Value, String> {
    if a.len() != 3 { return Err(format!("{}(modul, offset, panjang)", nama)); }
    let id = id_dari(&a[0])?;
    let ofs = bilangan(&a[1], nama, 1)?;
    let n = bilangan(&a[2], nama, 2)?;
    if ofs < 0 || n < 0 { return Err("offset/panjang memori wasm negatif".to_string()); }
    let reg = REGISTRI.lock().map_err(|_| "registri wasm rusak".to_string())?;
    let Some(m) = reg.get(id).and_then(|m| m.as_ref()) else {
        return Ok(err_value("modul wasm ini sudah ditutup"));
    };
    let (a0, a1) = (ofs as usize, ofs as usize + n as usize);
    if a1 > m.memori.len() {
        return Ok(err_value(format!(
            "baca {}..{} melewati memori wasm ({} byte)", a0, a1, m.memori.len())));
    }
    if byte { return Ok(ok_value(crate::dok::crypto::byte_ke_array(m.memori[a0..a1].to_vec()))); }
    Ok(match std::str::from_utf8(&m.memori[a0..a1]) {
        Ok(t) => ok_value(Value::Str(t.to_string())),
        Err(_) => err_value(format!(
            "memori wasm {}..{} bukan teks UTF-8 yang sah; pakai `wasm::baca_memori_bytes` untuk mengambil byte-nya", a0, a1)),
    })
}

pub fn b_tulis_memori(a: &[Value]) -> Result<Value, String> {
    if a.len() != 3 { return Err("wasm::tulis_memori(modul, offset, teks)".to_string()); }
    let id = id_dari(&a[0])?;
    let ofs = bilangan(&a[1], "wasm::tulis_memori", 1)?;
    let isi = match &a[2] {
        Value::Array(x) => crate::dok::crypto::array_ke_byte(x, "wasm::tulis_memori")?,
        v => teks(v, "wasm::tulis_memori", 2)?.into_bytes(),
    };
    if ofs < 0 { return Err("offset memori wasm negatif".to_string()); }
    let mut reg = REGISTRI.lock().map_err(|_| "registri wasm rusak".to_string())?;
    let Some(m) = reg.get_mut(id).and_then(|m| m.as_mut()) else {
        return Ok(err_value("modul wasm ini sudah ditutup"));
    };
    let b = &isi[..];
    let (a0, a1) = (ofs as usize, ofs as usize + b.len());
    if a1 > m.memori.len() {
        return Ok(err_value(format!(
            "tulis {}..{} melewati memori wasm ({} byte)", a0, a1, m.memori.len())));
    }
    m.memori[a0..a1].copy_from_slice(b);
    Ok(ok_value(Value::Int(b.len() as i128)))
}

pub fn b_tutup(a: &[Value]) -> Result<Value, String> {
    if a.len() != 1 { return Err("wasm::tutup(modul)".to_string()); }
    let id = id_dari(a.first().ok_or("wasm::tutup(modul)")?)?;
    let mut reg = REGISTRI.lock().map_err(|_| "registri wasm rusak".to_string())?;
    match reg.get_mut(id) {
        Some(slot) if slot.is_some() => { *slot = None; Ok(ok_value(Value::Unit)) }
        _ => Ok(err_value("modul wasm ini sudah ditutup")),
    }
}
