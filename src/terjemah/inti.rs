// src/terjemah/inti.rs — pencocok pola terjemahan tanpa `std`: dipakai `jrgc` dan, sebagai SALINAN PERSIS di
// `runtime/src/terjemah_inti.rs` (dijaga tes), oleh biner `--emit-llvm`. Aturannya di dokumentasi/src/terjemah/mod.md.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub struct Pola<'a> {
    /// Potongan teks Indonesia di antara `{}`; selalu diawali dan diakhiri teks (dijaga tes).
    pub seg: Vec<&'a str>,
    /// Potongan padanan Inggris, dan nomor tangkapan yang diletakkan sesudah tiap potongan.
    pub en_seg: Vec<&'a str>,
    pub en_lubang: Vec<usize>,
    pub bobot: usize,
}

/// `{}` atau `{N}` adalah isian; kurung kurawal lain (contoh kode JRG di kalimat) tetap teks.
pub fn urai_en(en: &str, n_lubang: usize) -> (Vec<&str>, Vec<usize>) {
    let mut seg = Vec::new();
    let mut lubang = Vec::new();
    let (mut mulai, mut i, mut urut) = (0, 0, 0);
    while let Some(rel) = en[i..].find('{') {
        let a = i + rel;
        if let Some(rb) = en[a..].find('}') {
            let isi = &en[a + 1..a + rb];
            let nomor = if isi.is_empty() { Some(urut) } else { isi.parse::<usize>().ok() };
            if let Some(n) = nomor.filter(|n| *n < n_lubang) {
                if isi.is_empty() { urut += 1; }
                seg.push(&en[mulai..a]);
                lubang.push(n);
                mulai = a + rb + 1;
                i = mulai;
                continue;
            }
        }
        i = a + 1;
    }
    seg.push(&en[mulai..]);
    (seg, lubang)
}

/// Pola dari pasangan (Indonesia, Inggris), yang paling panjang lebih dulu.
pub fn siapkan<'a>(tabel: impl Iterator<Item = (&'a str, &'a str)>) -> Vec<Pola<'a>> {
    let mut v: Vec<Pola<'a>> = tabel.map(|(id, en)| {
        let seg: Vec<&'a str> = id.split("{}").collect();
        let (en_seg, en_lubang) = urai_en(en, seg.len() - 1);
        let bobot = seg.iter().map(|s| s.len()).sum();
        Pola { seg, en_seg, en_lubang, bobot }
    }).collect();
    v.sort_by(|a, b| b.bobot.cmp(&a.bobot));
    v
}

/// Satu potongan keluaran: sudah diterjemahkan (tidak disentuh lagi), atau masih teks asli. `lubang` = isi
/// tangkapan sebuah pola; cuma itu yang boleh diganti utuh lewat `kata` (`kunci` di kolom perintah tetap).
enum Potong { Jadi(String), Asli(String, bool) }

fn huruf(c: Option<char>) -> bool { c.is_some_and(|c| c.is_alphanumeric() || c == '_') }

/// Cari `p` di `teks`; kembalikan (awal, akhir, tangkapan).
fn cocok<'t>(p: &Pola, teks: &'t str) -> Option<(usize, usize, Vec<&'t str>)> {
    let mut mulai_cari = 0;
    while let Some(rel) = teks[mulai_cari..].find(p.seg[0]) {
        let awal = mulai_cari + rel;
        let mut pos = awal + p.seg[0].len();
        let mut tangkap = Vec::new();
        let mut gagal = false;
        for s in &p.seg[1..] {
            // Tangkapan tidak melewati baris baru dan tidak lebih dari 400 byte.
            let mut batas = teks[pos..].find('\n').map(|i| pos + i).unwrap_or(teks.len()).min(pos + 400 + s.len());
            while !teks.is_char_boundary(batas) { batas -= 1; }
            match teks[pos..batas].find(s) {
                Some(i) => {
                    tangkap.push(&teks[pos..pos + i]);
                    pos += i + s.len();
                }
                _ => { gagal = true; break }
            }
        }
        // Pola yang diawali/diakhiri huruf cuma cocok dengan kata utuh: `argumen` tidak di dalam `argument`.
        let tepi_kiri = !huruf(p.seg[0].chars().next()) || !huruf(teks[..awal].chars().next_back());
        let tepi_kanan = !huruf(p.seg.last().and_then(|s| s.chars().next_back())) || !huruf(teks[pos..].chars().next());
        if !gagal && tepi_kiri && tepi_kanan { return Some((awal, pos, tangkap)); }
        mulai_cari = awal + p.seg[0].len().max(1);
        while mulai_cari < teks.len() && !teks.is_char_boundary(mulai_cari) { mulai_cari += 1; }
        if mulai_cari >= teks.len() { break; }
    }
    None
}

/// Terjemahkan `teks` sejauh `pola` mengenalnya. Pola dengan rentang PALING PANJANG menang (seri: teks polanya
/// lebih panjang), supaya kalimat luar menang atas potongan pendek di dalamnya.
pub fn terjemah(pola: &[Pola], kata: &[(&str, &str)], teks: &str) -> String {
    if !teks.bytes().any(|b| b.is_ascii_lowercase()) { return teks.to_string(); }
    let mut potong = alloc::vec![Potong::Asli(teks.to_string(), false)];
    loop {
        let mut berubah = false;
        let mut baru = Vec::with_capacity(potong.len() + 4);
        for pt in potong {
            let Potong::Asli(s, lubang) = pt else { baru.push(pt); continue };
            let terbaik = pola.iter().filter_map(|p| cocok(p, &s).map(|(a, b, t)| (p, a, b, t)))
                .max_by_key(|(p, a, b, _)| (b - a, p.bobot, core::cmp::Reverse(*a)));
            let Some((p, a, b, tangkap)) = terbaik else { baru.push(Potong::Asli(s, lubang)); continue };
            berubah = true;
            if a > 0 { baru.push(Potong::Asli(s[..a].to_string(), false)); }
            for (i, seg) in p.en_seg.iter().enumerate() {
                if !seg.is_empty() { baru.push(Potong::Jadi(seg.to_string())); }
                if let Some(&n) = p.en_lubang.get(i) {
                    if !tangkap[n].is_empty() { baru.push(Potong::Asli(tangkap[n].to_string(), true)); }
                }
            }
            if b < s.len() { baru.push(Potong::Asli(s[b..].to_string(), false)); }
        }
        potong = baru;
        if !berubah { break; }
    }
    let hasil: String = potong.into_iter().map(|p| match p {
        Potong::Jadi(s) => s,
        Potong::Asli(s, true) => kata_utuh(kata, &s).unwrap_or(s),
        Potong::Asli(s, false) => s,
    }).collect();
    if hasil == teks { hasil } else { tunggal(&hasil) }
}

/// Bahasa Indonesia tidak menandai jamak, jadi "{} berkas" jadi "{} files"; "1 files" dibetulkan jadi "1 file".
fn tunggal(s: &str) -> String {
    const JAMAK: [(&str, &str); 14] = [("tests", "test"), ("files", "file"), ("errors", "error"),
        ("warnings", "warning"), ("elements", "element"), ("arguments", "argument"), ("calls", "call"),
        ("names", "name"), ("functions", "function"), ("characters", "character"), ("items", "item"),
        ("parameters", "parameter"), ("values", "value"), ("bytes", "byte")];
    let mut keluar = String::with_capacity(s.len());
    let mut sisa = s;
    while let Some(i) = sisa.find("1 ") {
        let sebelum = sisa[..i].chars().next_back();
        let lepas = !sebelum.is_some_and(|c| c.is_ascii_digit() || c == '.' || c == '-');
        let ganti = lepas.then(|| JAMAK.iter().find(|(j, _)| {
            sisa[i + 2..].starts_with(j) && !huruf(sisa[i + 2 + j.len()..].chars().next())
        })).flatten();
        match ganti {
            Some((j, t)) => { keluar.push_str(&sisa[..i + 2]); keluar.push_str(t); sisa = &sisa[i + 2 + j.len()..]; }
            None => { keluar.push_str(&sisa[..i + 2]); sisa = &sisa[i + 2..]; }
        }
    }
    keluar.push_str(sisa);
    keluar
}

/// Sisipan yang SELURUHNYA satu kata dari `kata` (spasi di tepi dipertahankan).
fn kata_utuh(kata: &[(&str, &str)], s: &str) -> Option<String> {
    let inti = s.trim();
    let (_, en) = kata.iter().find(|(id, _)| *id == inti)?;
    let awal = &s[..s.len() - s.trim_start().len()];
    let akhir = &s[s.trim_end().len()..];
    Some(format!("{awal}{en}{akhir}"))
}

/// Tabel sebagai satu blob untuk biner `--emit-llvm`: pasangan `id \x1F en`, dipisah `\x1E`; sesudah `\x1D`,
/// daftar kata dengan bentuk yang sama.
pub fn urai_blob(blob: &str) -> (Vec<(&str, &str)>, Vec<(&str, &str)>) {
    let (tabel, kata) = blob.split_once('\u{1D}').unwrap_or((blob, ""));
    (pasang(tabel), pasang(kata))
}

fn pasang(s: &str) -> Vec<(&str, &str)> {
    s.split('\u{1E}').filter_map(|e| e.split_once('\u{1F}')).collect()
}
