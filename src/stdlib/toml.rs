// src/stdlib/toml.rs — `toml::`: mengurai dan menulis TOML 1.0, tanpa dependensi.
// Bentuk yang dibaca lain oleh pengurai lain jadi `Err`: kunci/tabel kembar, tabel diperluas lewat kunci bertitik atau
// header, bilangan di luar 64 bit, escape asing, kendali, CR telanjang, tanggal mustahil; sarang paling dalam 256.

use crate::kutip_inti::kutip;
use std::collections::HashMap;

use crate::interpreter::{err_value, ok_value, Value};
use crate::vm::vm::Vm;

/// Sarang tabel/array paling dalam, sama dengan batas sarang pengurai JRG.
pub const MAKS_SARANG: usize = 256;

pub fn register(vm: &mut Vm) {
    vm.def("toml::parse", |a| crate::stdlib::sistem::lewat_hasil(a, t_parse));
    vm.def("toml::stringify", |a| crate::stdlib::sistem::lewat_hasil(a, t_stringify));
}

/// Asal sebuah tabel, yang menentukan siapa boleh menambah isinya (TOML 1.0 §Table).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Jenis { Implisit, Eksplisit, Bertitik, Sebaris }

#[derive(Debug)]
pub enum Simpul {
    Tabel(Tabel),
    /// `aot`: dibuat `[[..]]`, jadi boleh ditambah `[[..]]` lagi; array tertulis tidak.
    Larik { isi: Vec<Simpul>, aot: bool },
    Teks(String), Bulat(i64), Pecahan(f64), Benar(bool), Waktu(String),
}

#[derive(Debug)]
pub struct Tabel { pub isi: indexmap::IndexMap<String, Simpul>, jenis: Jenis, dalam: usize }

impl Tabel {
    fn baru(jenis: Jenis, dalam: usize) -> Result<Tabel, String> {
        if dalam > MAKS_SARANG { return Err(format!("tabel bersarang lebih dari {} tingkat", MAKS_SARANG)); }
        Ok(Tabel { isi: indexmap::IndexMap::new(), jenis, dalam })
    }
    /// Tabel sebaris tidak bisa diubah sesudah ditulis, termasuk tabel bertitik di dalamnya.
    fn bekukan(&mut self) {
        self.jenis = Jenis::Sebaris;
        for v in self.isi.values_mut() {
            if let Simpul::Tabel(t) = v { if t.jenis == Jenis::Bertitik { t.bekukan(); } }
        }
    }
}

fn nama_jalur(k: &[String]) -> String { kutip(&k.iter().map(|s| tulis_kunci(s)).collect::<Vec<_>>().join(".")) }

/// `[a.b]` (atau `[[a.b]]` kalau `aot`): induk dibuat implisit, ujungnya dibuat sekali saja.
fn tetapkan_tabel(akar: &mut Tabel, k: &[String], aot: bool) -> Result<(), String> {
    let mut t = akar;
    for (n, bag) in k[..k.len() - 1].iter().enumerate() {
        let dalam = t.dalam + 1;
        if !t.isi.contains_key(bag) { t.isi.insert(bag.clone(), Simpul::Tabel(Tabel::baru(Jenis::Implisit, dalam)?)); }
        t = match t.isi.get_mut(bag).unwrap() {
            Simpul::Tabel(x) if x.jenis == Jenis::Sebaris =>
                return Err(format!("tabel sebaris `{}` tidak bisa diperluas", nama_jalur(&k[..=n]))),
            Simpul::Tabel(x) => x,
            Simpul::Larik { isi, aot: true } => match isi.last_mut() { Some(Simpul::Tabel(x)) => x, _ => unreachable!() },
            Simpul::Larik { .. } => return Err(format!("`{}` sudah berisi array, bukan tabel", nama_jalur(&k[..=n]))),
            _ => return Err(format!("`{}` sudah berisi nilai, bukan tabel", nama_jalur(&k[..=n]))),
        };
    }
    let ujung = &k[k.len() - 1];
    let nama = nama_jalur(k);
    let dalam = t.dalam + 1;
    if aot {
        let baru = Simpul::Tabel(Tabel::baru(Jenis::Eksplisit, dalam + 1)?);
        match t.isi.get_mut(ujung) {
            None => { t.isi.insert(ujung.clone(), Simpul::Larik { isi: vec![baru], aot: true }); }
            Some(Simpul::Larik { isi, aot: true }) => isi.push(baru),
            Some(Simpul::Larik { .. }) => return Err(format!("array `{}` ditulis sebagai nilai; `[[{}]]` tidak bisa menambahnya", nama, nama)),
            Some(_) => return Err(format!("`{}` sudah didefinisikan, bukan array tabel", nama)),
        }
        return Ok(());
    }
    match t.isi.get_mut(ujung) {
        None => { t.isi.insert(ujung.clone(), Simpul::Tabel(Tabel::baru(Jenis::Eksplisit, dalam)?)); }
        Some(Simpul::Tabel(x)) if x.jenis == Jenis::Implisit => x.jenis = Jenis::Eksplisit,
        Some(Simpul::Tabel(x)) if x.jenis == Jenis::Eksplisit => return Err(format!("tabel `[{}]` didefinisikan dua kali", nama)),
        Some(Simpul::Tabel(x)) if x.jenis == Jenis::Bertitik =>
            return Err(format!("tabel `{}` sudah didefinisikan lewat kunci bertitik; `[{}]` tidak bisa mendefinisikannya lagi", nama, nama)),
        Some(Simpul::Tabel(_)) => return Err(format!("tabel sebaris `{}` tidak bisa diperluas", nama)),
        Some(Simpul::Larik { aot: true, .. }) => return Err(format!("`{}` sudah array tabel `[[{}]]`", nama, nama)),
        Some(_) => return Err(format!("`{}` sudah berisi nilai, bukan tabel", nama)),
    }
    Ok(())
}

/// Tabel bagian yang sedang ditulis: jalur header terakhir, elemen terakhir untuk array tabel.
fn bagian<'t>(akar: &'t mut Tabel, jalur: &[String]) -> &'t mut Tabel {
    let mut t = akar;
    for k in jalur {
        t = match t.isi.get_mut(k) {
            Some(Simpul::Tabel(x)) => x,
            Some(Simpul::Larik { isi, aot: true }) => match isi.last_mut() { Some(Simpul::Tabel(x)) => x, _ => unreachable!() },
            _ => unreachable!("jalur header sudah diperiksa tetapkan_tabel"),
        };
    }
    t
}

/// `a.b.c = v` di tabel `t`: induknya cuma boleh tabel yang juga lahir dari kunci bertitik.
fn sisip(t: &mut Tabel, k: &[String], v: Simpul) -> Result<(), String> {
    let mut t = t;
    for (n, bag) in k[..k.len() - 1].iter().enumerate() {
        let dalam = t.dalam + 1;
        if !t.isi.contains_key(bag) { t.isi.insert(bag.clone(), Simpul::Tabel(Tabel::baru(Jenis::Bertitik, dalam)?)); }
        let jalan = nama_jalur(&k[..=n]);
        match t.isi.get(bag).unwrap() {
            Simpul::Tabel(x) if x.jenis == Jenis::Bertitik => {}
            Simpul::Tabel(x) if x.jenis == Jenis::Sebaris => return Err(format!("tabel sebaris `{}` tidak bisa diperluas", jalan)),
            Simpul::Tabel(_) => return Err(format!(
                "tabel `{}` sudah didefinisikan lewat header; kunci bertitik tidak bisa menambah isinya", jalan)),
            _ => return Err(format!("`{}` sudah berisi nilai, bukan tabel", jalan)),
        }
        t = match t.isi.get_mut(bag) { Some(Simpul::Tabel(x)) => x, _ => unreachable!() };
    }
    let ujung = &k[k.len() - 1];
    if t.isi.contains_key(ujung) { return Err(format!("kunci `{}` didefinisikan dua kali", nama_jalur(k))); }
    t.isi.insert(ujung.clone(), v);
    Ok(())
}

struct P<'a> { s: &'a [u8], t: &'a str, i: usize }

fn kunci_polos(c: u8) -> bool { c.is_ascii_alphanumeric() || c == b'_' || c == b'-' }
fn kendali(c: u8) -> bool { (c < 0x20 && c != b'\t') || c == 0x7f }

impl<'a> P<'a> {
    fn lihat(&self) -> Option<u8> { self.s.get(self.i).copied() }
    fn lihat_di(&self, n: usize) -> Option<u8> { self.s.get(self.i + n).copied() }
    fn awali(&self, x: &str) -> bool { self.s[self.i..].starts_with(x.as_bytes()) }
    fn galat(&self, m: impl std::fmt::Display) -> String {
        let baris = 1 + self.s[..self.i.min(self.s.len())].iter().filter(|&&c| c == b'\n').count();
        format!("baris {}: {}", baris, m)
    }
    fn spasi(&mut self) { while matches!(self.lihat(), Some(b' ' | b'\t')) { self.i += 1; } }

    /// `#` sampai ujung baris; kendali selain tab tidak boleh ada di komentar.
    fn komentar(&mut self) -> Result<(), String> {
        self.i += 1;
        while let Some(c) = self.lihat() {
            if c == b'\n' || (c == b'\r' && self.lihat_di(1) == Some(b'\n')) { break; }
            if kendali(c) { return Err(self.galat(format!("karakter kendali U+{:04X} di komentar", c))); }
            self.i += 1;
        }
        Ok(())
    }

    /// Baris baru LF atau CRLF; CR telanjang ditolak.
    fn baris_baru(&mut self) -> Result<bool, String> {
        match self.lihat() {
            Some(b'\n') => { self.i += 1; Ok(true) }
            Some(b'\r') if self.lihat_di(1) == Some(b'\n') => { self.i += 2; Ok(true) }
            Some(b'\r') => Err(self.galat("CR tanpa LF")),
            _ => Ok(false),
        }
    }

    /// Sesudah nilai atau header: spasi, komentar, lalu baris baru atau akhir berkas.
    fn akhir_baris(&mut self) -> Result<(), String> {
        self.spasi();
        if self.lihat() == Some(b'#') { self.komentar()?; }
        if self.lihat().is_none() || self.baris_baru()? { return Ok(()); }
        Err(self.galat(format!("sesudah nilai harus baris baru, dapat `{}`", self.cuplik())))
    }

    fn cuplik(&self) -> String { kutip(&self.t[self.i..].chars().take(12).collect::<String>()) }

    /// Spasi, baris baru dan komentar di dalam array.
    fn sela(&mut self) -> Result<(), String> {
        loop {
            self.spasi();
            match self.lihat() {
                Some(b'#') => self.komentar()?,
                Some(b'\n' | b'\r') => { self.baris_baru()?; }
                _ => return Ok(()),
            }
        }
    }

    fn kunci(&mut self) -> Result<Vec<String>, String> {
        let mut k = Vec::new();
        loop {
            self.spasi();
            let bag = match self.lihat() {
                Some(b'"') => { if self.awali("\"\"\"") { return Err(self.galat("kunci tidak boleh string multi-baris")); } self.teks_biasa()? }
                Some(b'\'') => { if self.awali("'''") { return Err(self.galat("kunci tidak boleh string multi-baris")); } self.teks_harfiah()? }
                Some(c) if kunci_polos(c) => {
                    let a = self.i;
                    while self.lihat().map_or(false, kunci_polos) { self.i += 1; }
                    self.t[a..self.i].to_string()
                }
                _ => return Err(self.galat(format!("kunci hilang atau tidak sah di `{}`", self.cuplik()))),
            };
            k.push(bag);
            self.spasi();
            if self.lihat() == Some(b'.') { self.i += 1; } else { return Ok(k); }
        }
    }

    fn escape(&mut self, keluar: &mut String) -> Result<(), String> {
        let c = self.lihat().ok_or_else(|| self.galat("escape terpotong"))?;
        self.i += 1;
        let ganti = match c {
            b'b' => '\u{8}', b't' => '\t', b'n' => '\n', b'f' => '\u{c}', b'r' => '\r', b'"' => '"', b'\\' => '\\',
            b'u' | b'U' => {
                let n = if c == b'u' { 4 } else { 8 };
                let h = self.t.get(self.i..self.i + n).filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))
                    .ok_or_else(|| self.galat(format!("`\\{}` butuh tepat {} digit heksa", c as char, n)))?;
                let kode = u32::from_str_radix(h, 16).unwrap();
                self.i += n;
                char::from_u32(kode).ok_or_else(|| self.galat(format!("`\\{}{}` bukan karakter Unicode (surrogate atau di luar U+10FFFF)", c as char, h)))?
            }
            _ => { self.i -= 1; return Err(self.galat(format!("escape `\\{}` tidak dikenal TOML 1.0", self.cuplik().chars().next().unwrap_or(' ')))); }
        };
        keluar.push(ganti);
        Ok(())
    }

    /// Salin satu karakter (UTF-8 utuh) ke `keluar`.
    fn salin_karakter(&mut self, keluar: &mut String) {
        let c = self.t[self.i..].chars().next().unwrap();
        keluar.push(c);
        self.i += c.len_utf8();
    }

    fn teks_biasa(&mut self) -> Result<String, String> {
        self.i += 1;
        let mut o = String::new();
        loop {
            match self.lihat() {
                None | Some(b'\n') | Some(b'\r') => return Err(self.galat("string tidak ditutup sebelum ujung baris")),
                Some(b'"') => { self.i += 1; return Ok(o); }
                Some(b'\\') => { self.i += 1; self.escape(&mut o)?; }
                Some(c) if kendali(c) => return Err(self.galat(format!("karakter kendali U+{:04X} di string; tulis `\\u{:04X}`", c, c))),
                Some(_) => self.salin_karakter(&mut o),
            }
        }
    }

    fn teks_harfiah(&mut self) -> Result<String, String> {
        self.i += 1;
        let a = self.i;
        loop {
            match self.lihat() {
                None | Some(b'\n') | Some(b'\r') => return Err(self.galat("string tidak ditutup sebelum ujung baris")),
                Some(b'\'') => { let o = self.t[a..self.i].to_string(); self.i += 1; return Ok(o); }
                Some(c) if kendali(c) => return Err(self.galat(format!("karakter kendali U+{:04X} di string harfiah", c))),
                Some(_) => self.i += 1,
            }
        }
    }

    /// `"""` atau `'''`. Baris baru langsung sesudah pembuka dibuang; CRLF dibaca sebagai LF.
    fn teks_panjang(&mut self, kutip: u8) -> Result<String, String> {
        self.i += 3;
        self.baris_baru()?;
        let mut o = String::new();
        loop {
            match self.lihat() {
                None => return Err(self.galat("string multi-baris tidak ditutup")),
                Some(c) if c == kutip => {
                    let mut n = 0;
                    while self.lihat_di(n) == Some(kutip) { n += 1; }
                    if n < 3 { for _ in 0..n { o.push(kutip as char); } self.i += n; continue; }
                    if n > 5 { return Err(self.galat("lebih dari dua tanda kutip sebelum penutup string multi-baris")); }
                    for _ in 0..n - 3 { o.push(kutip as char); }
                    self.i += n;
                    return Ok(o);
                }
                Some(b'\n' | b'\r') => { self.baris_baru()?; o.push('\n'); }
                Some(b'\\') if kutip == b'"' => {
                    // `\` di ujung baris: buang baris baru dan spasi sesudahnya.
                    let mut j = self.i + 1;
                    while matches!(self.s.get(j), Some(b' ' | b'\t')) { j += 1; }
                    if matches!(self.s.get(j), Some(b'\n')) || (self.s.get(j) == Some(&b'\r') && self.s.get(j + 1) == Some(&b'\n')) {
                        self.i = j;
                        loop {
                            self.spasi();
                            if !self.baris_baru()? { break; }
                        }
                    } else {
                        self.i += 1;
                        self.escape(&mut o)?;
                    }
                }
                Some(c) if kendali(c) => return Err(self.galat(format!("karakter kendali U+{:04X} di string", c))),
                Some(_) => self.salin_karakter(&mut o),
            }
        }
    }

    fn kata(&mut self, k: &str) -> bool {
        if !self.awali(k) { return false; }
        let akhir = self.s.get(self.i + k.len()).copied();
        if akhir.map_or(false, |c| kunci_polos(c) || c == b'.' || c == b':') { return false; }
        self.i += k.len();
        true
    }

    fn nilai(&mut self, dalam: usize) -> Result<Simpul, String> {
        if dalam > MAKS_SARANG { return Err(self.galat(format!("nilai bersarang lebih dari {} tingkat", MAKS_SARANG))); }
        match self.lihat() {
            None | Some(b'\n' | b'\r' | b'#') => Err(self.galat("nilai hilang sesudah `=`")),
            Some(b'"') if self.awali("\"\"\"") => Ok(Simpul::Teks(self.teks_panjang(b'"')?)),
            Some(b'"') => Ok(Simpul::Teks(self.teks_biasa()?)),
            Some(b'\'') if self.awali("'''") => Ok(Simpul::Teks(self.teks_panjang(b'\'')?)),
            Some(b'\'') => Ok(Simpul::Teks(self.teks_harfiah()?)),
            Some(b't') if self.kata("true") => Ok(Simpul::Benar(true)),
            Some(b'f') if self.kata("false") => Ok(Simpul::Benar(false)),
            Some(b'[') => {
                self.i += 1;
                let mut isi = Vec::new();
                loop {
                    self.sela()?;
                    if self.lihat() == Some(b']') { self.i += 1; break; }
                    isi.push(self.nilai(dalam + 1)?);
                    self.sela()?;
                    match self.lihat() {
                        Some(b',') => self.i += 1,
                        Some(b']') => { self.i += 1; break; }
                        _ => return Err(self.galat(format!("array butuh `,` atau `]`, dapat `{}`", self.cuplik()))),
                    }
                }
                Ok(Simpul::Larik { isi, aot: false })
            }
            Some(b'{') => {
                self.i += 1;
                let mut t = Tabel::baru(Jenis::Bertitik, dalam).map_err(|e| self.galat(e))?;
                self.spasi();
                if self.lihat() == Some(b'}') { self.i += 1; t.bekukan(); return Ok(Simpul::Tabel(t)); }
                loop {
                    let k = self.kunci()?;
                    if self.lihat() != Some(b'=') { return Err(self.galat("kunci di tabel sebaris butuh `=`")); }
                    self.i += 1;
                    self.spasi();
                    let v = self.nilai(dalam + k.len())?;
                    sisip(&mut t, &k, v).map_err(|e| self.galat(e))?;
                    self.spasi();
                    match self.lihat() {
                        Some(b',') => {
                            self.i += 1;
                            self.spasi();
                            match self.lihat() {
                                Some(b'}') => return Err(self.galat("koma penutup tidak boleh di tabel sebaris")),
                                Some(b'\n' | b'\r') => return Err(self.galat("tabel sebaris tidak boleh dipecah antar-baris")),
                                _ => {}
                            }
                        }
                        Some(b'}') => { self.i += 1; break; }
                        Some(b'\n' | b'\r') => return Err(self.galat("tabel sebaris tidak boleh dipecah antar-baris")),
                        _ => return Err(self.galat(format!("tabel sebaris butuh `,` atau `}}`, dapat `{}`", self.cuplik()))),
                    }
                }
                t.bekukan();
                Ok(Simpul::Tabel(t))
            }
            Some(_) => self.angka_atau_waktu(),
        }
    }

    fn angka_atau_waktu(&mut self) -> Result<Simpul, String> {
        let a = self.i;
        let pemisah = |c: u8| matches!(c, b' ' | b'\t' | b',' | b']' | b'}' | b'#' | b'\n' | b'\r');
        while self.lihat().map_or(false, |c| !pemisah(c)) { self.i += 1; }
        // `1979-05-27 07:32:00`: spasi sebagai pemisah tanggal dan jam.
        let tanggal_saja = self.i - a == 10 && self.s[a + 4] == b'-';
        if tanggal_saja && self.lihat() == Some(b' ') && self.s.get(self.i + 1..self.i + 4).map_or(false, |x| x[0].is_ascii_digit() && x[1].is_ascii_digit() && x[2] == b':') {
            self.i += 1;
            while self.lihat().map_or(false, |c| !pemisah(c)) { self.i += 1; }
        }
        let tok = &self.t[a..self.i];
        let b = tok.as_bytes();
        let hasil = if b.len() >= 5 && b[..4].iter().all(u8::is_ascii_digit) && b[4] == b'-' {
            periksa_waktu(tok).map(|_| Simpul::Waktu(tok.to_string()))
        } else if b.len() >= 3 && b[..2].iter().all(u8::is_ascii_digit) && b[2] == b':' {
            periksa_jam(tok).map(|_| Simpul::Waktu(tok.to_string()))
        } else {
            angka(tok)
        };
        hasil.map_err(|e| { self.i = a; self.galat(e) })
    }
}

/// Digit dengan `_` cuma di antara dua digit.
fn buang_garis(s: &str, sah: fn(u8) -> bool) -> Result<String, String> {
    let b = s.as_bytes();
    if b.is_empty() { return Err("digit hilang".into()); }
    let mut o = String::with_capacity(b.len());
    for (n, &c) in b.iter().enumerate() {
        if c == b'_' {
            if n == 0 || n + 1 == b.len() || !sah(b[n - 1]) || !sah(b[n + 1]) {
                return Err("`_` di bilangan harus di antara dua digit".into());
            }
        } else if sah(c) { o.push(c as char); } else { return Err(format!("`{}` bukan digit", c as char)); }
    }
    Ok(o)
}

fn angka(tok: &str) -> Result<Simpul, String> {
    let (tanda, badan) = match tok.as_bytes().first() {
        Some(b'+') => ("", &tok[1..]),
        Some(b'-') => ("-", &tok[1..]),
        _ => ("", tok),
    };
    match badan {
        "inf" => return Ok(Simpul::Pecahan(if tanda == "-" { f64::NEG_INFINITY } else { f64::INFINITY })),
        "nan" => return Ok(Simpul::Pecahan(f64::NAN)),
        _ => {}
    }
    if !badan.as_bytes().first().map_or(false, u8::is_ascii_digit) {
        return Err(format!("`{}` bukan nilai TOML (teks harus dikutip)", kutip(tok)));
    }
    for (awal, basis) in [("0x", 16u32), ("0o", 8), ("0b", 2)] {
        if let Some(d) = badan.strip_prefix(awal) {
            if tok.len() != badan.len() { return Err(format!("`{}`: bilangan berawalan `{}` tidak boleh bertanda", kutip(tok), awal)); }
            let sah: fn(u8) -> bool = match basis {
                16 => |c| c.is_ascii_hexdigit(),
                8 => |c| (b'0'..=b'7').contains(&c),
                _ => |c| c == b'0' || c == b'1',
            };
            let bersih = buang_garis(d, sah).map_err(|e| format!("`{}`: {}", kutip(tok), e))?;
            return i64::from_str_radix(&bersih, basis).map(Simpul::Bulat)
                .map_err(|_| format!("`{}` di luar bilangan bulat 64 bit", kutip(tok)));
        }
    }
    if badan.starts_with("0X") || badan.starts_with("0O") || badan.starts_with("0B") {
        return Err(format!("`{}`: awalan basis ditulis huruf kecil (`0x`, `0o`, `0b`)", kutip(tok)));
    }
    let digit = |c: u8| c.is_ascii_digit();
    let (kepala, ekor) = match badan.find(['.', 'e', 'E']) { Some(n) => (&badan[..n], &badan[n..]), None => (badan, "") };
    let bulat = buang_garis(kepala, digit).map_err(|e| format!("`{}`: {}", kutip(tok), e))?;
    if bulat.len() > 1 && bulat.starts_with('0') { return Err(format!("`{}`: bilangan tidak boleh berawalan nol", kutip(tok))); }
    if ekor.is_empty() {
        return format!("{}{}", tanda, bulat).parse::<i64>().map(Simpul::Bulat)
            .map_err(|_| format!("`{}` di luar bilangan bulat 64 bit", kutip(tok)));
    }
    let mut teks = format!("{}{}", tanda, bulat);
    let mut sisa = ekor;
    if let Some(r) = sisa.strip_prefix('.') {
        let n = r.find(['e', 'E']).unwrap_or(r.len());
        teks.push('.');
        teks.push_str(&buang_garis(&r[..n], digit).map_err(|_| format!("`{}`: pecahan butuh digit sesudah `.`", kutip(tok)))?);
        sisa = &r[n..];
    }
    if !sisa.is_empty() {
        let r = &sisa[1..];
        let (t, d) = match r.as_bytes().first() { Some(b'+') => ("", &r[1..]), Some(b'-') => ("-", &r[1..]), _ => ("", r) };
        teks.push('e');
        teks.push_str(t);
        teks.push_str(&buang_garis(d, digit).map_err(|_| format!("`{}`: eksponen butuh digit", kutip(tok)))?);
    }
    let f: f64 = teks.parse().map_err(|_| format!("`{}` bukan bilangan pecahan", kutip(tok)))?;
    if f.is_infinite() { return Err(format!("`{}` di luar pecahan 64 bit", kutip(tok))); }
    Ok(Simpul::Pecahan(f))
}

fn dua(b: &[u8], n: usize) -> Option<u32> {
    let x = b.get(n..n + 2)?;
    if x.iter().all(u8::is_ascii_digit) { Some(((x[0] - b'0') * 10 + (x[1] - b'0')) as u32) } else { None }
}

/// `HH:MM:SS[.frac]`, panjangnya dikembalikan.
fn jam(b: &[u8]) -> Result<usize, String> {
    let (h, m, s) = match (dua(b, 0), b.get(2), dua(b, 3), b.get(5), dua(b, 6)) {
        (Some(h), Some(b':'), Some(m), Some(b':'), Some(s)) => (h, m, s),
        _ => return Err("jam harus `HH:MM:SS` (detik wajib di TOML 1.0)".into()),
    };
    if h > 23 || m > 59 || s > 60 { return Err(format!("jam {:02}:{:02}:{:02} tidak ada", h, m, s)); }
    let mut n = 8;
    if b.get(8) == Some(&b'.') {
        n = 9;
        while b.get(n).map_or(false, u8::is_ascii_digit) { n += 1; }
        if n == 9 { return Err("pecahan detik butuh digit sesudah `.`".into()); }
    }
    Ok(n)
}

fn periksa_jam(tok: &str) -> Result<(), String> {
    let n = jam(tok.as_bytes()).map_err(|e| format!("`{}`: {}", kutip(tok), e))?;
    if n != tok.len() { return Err(format!("`{}` bukan jam yang sah", kutip(tok))); }
    Ok(())
}

fn periksa_waktu(tok: &str) -> Result<(), String> {
    let b = tok.as_bytes();
    let salah = |m: &str| format!("`{}`: {}", kutip(tok), m);
    let th: u32 = tok[..4].parse().unwrap();
    let (bl, hr) = match (dua(b, 5), b.get(7), dua(b, 8)) {
        (Some(bl), Some(b'-'), Some(hr)) if b.get(4) == Some(&b'-') => (bl, hr),
        _ => return Err(salah("tanggal harus `YYYY-MM-DD`")),
    };
    let kabisat = (th % 4 == 0 && th % 100 != 0) || th % 400 == 0;
    let hari = match bl { 1 | 3 | 5 | 7 | 8 | 10 | 12 => 31, 4 | 6 | 9 | 11 => 30, 2 if kabisat => 29, 2 => 28, _ => 0 };
    if hr == 0 || hr > hari { return Err(salah(&format!("tanggal {:04}-{:02}-{:02} tidak ada", th, bl, hr))); }
    if b.len() == 10 { return Ok(()); }
    if !matches!(b[10], b'T' | b't' | b' ') { return Err(salah("pemisah tanggal dan jam harus `T` atau spasi")); }
    let sisa = &b[11..];
    let n = jam(sisa).map_err(|e| salah(&e))?;
    let zona = &sisa[n..];
    match zona {
        [] | [b'Z'] | [b'z'] => Ok(()),
        [b'+' | b'-', ..] => match (dua(zona, 1), zona.get(3), dua(zona, 4)) {
            (Some(h), Some(b':'), Some(m)) if zona.len() == 6 && h < 24 && m < 60 => Ok(()),
            _ => Err(salah("zona waktu harus `Z` atau `+HH:MM`")),
        },
        _ => Err(salah("zona waktu harus `Z` atau `+HH:MM`")),
    }
}

/// Urai satu dokumen TOML jadi tabel akarnya.
pub fn urai(t: &str) -> Result<Simpul, String> {
    let mut p = P { s: t.as_bytes(), t, i: 0 };
    if t.starts_with('\u{FEFF}') { p.i = 3; }
    let mut akar = Tabel::baru(Jenis::Eksplisit, 0)?;
    let mut jalur: Vec<String> = Vec::new();
    loop {
        p.spasi();
        match p.lihat() {
            None => break,
            Some(b'#') => p.komentar()?,
            Some(b'\n' | b'\r') => { p.baris_baru()?; }
            Some(b'[') => {
                let aot = p.lihat_di(1) == Some(b'[');
                p.i += if aot { 2 } else { 1 };
                let k = p.kunci()?;
                let tutup = if aot { "]]" } else { "]" };
                if !p.awali(tutup) { return Err(p.galat(format!("header tabel butuh `{}`", tutup))); }
                p.i += tutup.len();
                tetapkan_tabel(&mut akar, &k, aot).map_err(|e| p.galat(e))?;
                jalur = k;
                p.akhir_baris()?;
            }
            Some(_) => {
                let k = p.kunci()?;
                if p.lihat() != Some(b'=') { return Err(p.galat(format!("kunci `{}` butuh `=`", nama_jalur(&k)))); }
                p.i += 1;
                p.spasi();
                let t = bagian(&mut akar, &jalur);
                let v = p.nilai(t.dalam + k.len())?;
                let t = bagian(&mut akar, &jalur);
                sisip(t, &k, v).map_err(|e| p.galat(e))?;
                p.akhir_baris()?;
            }
        }
    }
    Ok(Simpul::Tabel(akar))
}

fn ke_nilai(s: Simpul) -> Value {
    match s {
        Simpul::Tabel(t) => {
            let m: HashMap<String, Value> = t.isi.into_iter().map(|(k, v)| (k, ke_nilai(v))).collect();
            Value::Struct { name: "dict".to_string(), fields: m.into() }
        }
        Simpul::Larik { isi, .. } => Value::Array(isi.into_iter().map(ke_nilai).collect::<Vec<_>>().into()),
        Simpul::Teks(t) | Simpul::Waktu(t) => Value::Str(std::sync::Arc::new(t)),
        Simpul::Bulat(n) => Value::Int(n as i128),
        Simpul::Pecahan(f) => Value::Float(f),
        Simpul::Benar(b) => Value::Bool(b),
    }
}

// ─── Penulis ──────────────────────────────────────────────────────────────

pub fn tulis_kunci(k: &str) -> String {
    if !k.is_empty() && k.bytes().all(kunci_polos) { k.to_string() } else { tulis_teks(k) }
}

pub fn tulis_teks(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""), '\\' => o.push_str("\\\\"), '\n' => o.push_str("\\n"), '\t' => o.push_str("\\t"),
            '\r' => o.push_str("\\r"), '\u{8}' => o.push_str("\\b"), '\u{c}' => o.push_str("\\f"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => o.push_str(&format!("\\u{:04X}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn dict_dari(v: &Value) -> Option<&crate::interpreter::Field> {
    match v { Value::Struct { name, fields } if name == "dict" => Some(fields), _ => None }
}

/// Array berisi dict saja (tidak kosong) ditulis sebagai `[[..]]`.
fn array_tabel(v: &Value) -> bool {
    matches!(v, Value::Array(a) if !a.is_empty() && a.iter().all(|x| dict_dari(x).is_some()))
}

fn kunci_urut(f: &crate::interpreter::Field) -> Vec<&String> {
    let mut k: Vec<&String> = f.keys().collect();
    k.sort();
    k
}

fn tulis_sebaris(v: &Value, dalam: usize) -> Result<String, String> {
    if dalam > MAKS_SARANG { return Err(format!("nilai bersarang lebih dari {} tingkat", MAKS_SARANG)); }
    Ok(match v {
        Value::Str(s) => tulis_teks(s),
        Value::Int(n) => match i64::try_from(*n) {
            Ok(n) => n.to_string(),
            Err(_) => return Err(format!("{} di luar bilangan bulat 64 bit TOML", n)),
        },
        Value::Float(f) if f.is_nan() => "nan".into(),
        Value::Float(f) if f.is_infinite() => if *f > 0.0 { "inf".into() } else { "-inf".into() },
        Value::Float(f) => format!("{:?}", f),
        Value::Bool(b) => b.to_string(),
        Value::Array(a) => {
            let isi: Result<Vec<String>, String> = a.iter().map(|x| tulis_sebaris(x, dalam + 1)).collect();
            format!("[{}]", isi?.join(", "))
        }
        v if dict_dari(v).is_some() => {
            let f = dict_dari(v).unwrap();
            if f.is_empty() { return Ok("{}".into()); }
            let mut isi = Vec::new();
            for k in kunci_urut(f) { isi.push(format!("{} = {}", tulis_kunci(k), tulis_sebaris(&f[k.as_str()], dalam + 1)?)); }
            format!("{{ {} }}", isi.join(", "))
        }
        Value::Nil => return Err("TOML tidak punya `nil`; buang kuncinya".into()),
        lain => return Err(format!("`{}` tidak punya padanan TOML", lain.type_name())),
    })
}

fn tulis_tabel(f: &crate::interpreter::Field, jalur: &mut Vec<String>, o: &mut String, dalam: usize) -> Result<(), String> {
    if dalam > MAKS_SARANG { return Err(format!("tabel bersarang lebih dari {} tingkat", MAKS_SARANG)); }
    let kunci = kunci_urut(f);
    for k in &kunci {
        let v = &f[k.as_str()];
        if dict_dari(v).is_none() && !array_tabel(v) {
            o.push_str(&format!("{} = {}\n", tulis_kunci(k), tulis_sebaris(v, dalam + 1)?));
        }
    }
    for k in &kunci {
        let v = &f[k.as_str()];
        jalur.push(k.to_string());
        if let Some(anak) = dict_dari(v) {
            o.push_str(&format!("\n[{}]\n", nama_jalur(jalur)));
            tulis_tabel(anak, jalur, o, dalam + 1)?;
        } else if array_tabel(v) {
            if let Value::Array(a) = v {
                for x in a.iter() {
                    o.push_str(&format!("\n[[{}]]\n", nama_jalur(jalur)));
                    tulis_tabel(dict_dari(x).unwrap(), jalur, o, dalam + 2)?;
                }
            }
        }
        jalur.pop();
    }
    Ok(())
}

fn teks_arg(a: &[Value], nama: &str) -> Result<String, String> {
    match a.first() {
        Some(Value::Str(s)) => Ok(s.to_string()),
        Some(lain) => Err(format!("{}: argumen ke-1 harus string, dapat `{}`", nama, lain.type_name())),
        None => Err(format!("{}: argumen ke-1 wajib diisi", nama)),
    }
}

/// `toml::parse(teks)` → `Result<dict>`; tanggal/jam jadi teks seperti yang ditulis.
fn t_parse(a: &[Value]) -> Result<Value, String> {
    let t = teks_arg(a, "toml::parse")?;
    Ok(match urai(&t) {
        Ok(s) => ok_value(ke_nilai(s)),
        Err(e) => err_value(format!("toml::parse: {}", e)),
    })
}

/// `toml::stringify(dict)` → `Result<string>`: kunci urut nama, tabel sebagai `[..]`, array dict sebagai `[[..]]`.
fn t_stringify(a: &[Value]) -> Result<Value, String> {
    let v = a.first().ok_or("toml::stringify: argumen ke-1 wajib diisi")?;
    let f = match dict_dari(v) {
        Some(f) => f,
        None => return Ok(err_value(format!("toml::stringify: akar dokumen harus dict, dapat `{}`", v.type_name()))),
    };
    let mut o = String::new();
    Ok(match tulis_tabel(f, &mut Vec::new(), &mut o, 0) {
        Ok(()) => ok_value(Value::Str(std::sync::Arc::new(o.trim_start_matches('\n').to_string()))),
        Err(e) => err_value(format!("toml::stringify: {}", e)),
    })
}

#[cfg(test)]
mod tes {
    use super::*;

    fn ditolak(s: &str, kata: &str) {
        match urai(s) {
            Ok(x) => panic!("{:?} diterima: {:?}", s, x),
            Err(e) => assert!(e.contains(kata), "{:?} ditolak dengan kalimat lain: {}", s, e),
        }
    }

    fn nilai(s: &str) -> String { format!("{}", ke_nilai(urai(s).unwrap())) }

    #[test]
    fn dokumen_sah_terurai() {
        let d = "\u{FEFF}# c\r\njudul = \"a\\tb\\u00E9\"\nx.y = 1_000\n[t]\nf = -1.5e3\nh = 0xff\nw = 1979-05-27 07:32:00Z\n\
                 s = '''\nx''y'''''\nm = \"\"\"\\\n   a \\\n  b\"\"\"\n[[l]]\nn = 1\n[[l]]\n[a.b]\n[a]\nc = [1, \"x\", [true], {k = 2},]\n";
        let v = nilai(d);
        for bag in ["judul: a\tbé", "y: 1000", "f: -1500.0", "h: 255", "w: 1979-05-27 07:32:00Z", "s: x''y''",
                    "m: a b", "l: [dict {n: 1}, dict {}]", "c: [1, x, [true], dict {k: 2}]"] {
            assert!(v.contains(bag), "{} tidak ada di {}", bag, v);
        }
    }

    #[test]
    fn bentuk_ambigu_ditolak() {
        ditolak("a = 1\na = 2", "dua kali");
        ditolak("[t]\n[t]", "dua kali");
        ditolak("a.b = 1\n[a]", "lewat kunci bertitik");
        ditolak("[a.b]\n[a]\nb.c = 1", "lewat header");
        ditolak("a = {b = 1}\na.c = 2", "sebaris");
        ditolak("a = {b = 1}\n[a.c]", "sebaris");
        ditolak("a = [1]\n[[a]]", "ditulis sebagai nilai");
        ditolak("[[a]]\n[a]", "array tabel");
        ditolak("a = 9223372036854775808", "64 bit");
        ditolak("a = 0x8000000000000000", "64 bit");
        ditolak("a = 1e400", "64 bit");
        ditolak("a = 012", "berawalan nol");
        ditolak("a = 1__0", "di antara dua digit");
        ditolak("a = _1", "bukan nilai TOML");
        ditolak("a = 1.", "pecahan butuh digit");
        ditolak("a = +0x1", "tidak boleh bertanda");
        ditolak("a = \"\\x41\"", "tidak dikenal");
        ditolak("a = \"\\uD800\"", "surrogate");
        ditolak("a = \"x\u{1}\"", "kendali");
        ditolak("a = 1 # \u{7f}", "kendali");
        ditolak("a = 1\rb = 2", "CR tanpa LF");
        ditolak("a = 2023-02-29", "tidak ada");
        ditolak("a = 2024-01-01T25:00:00", "tidak ada");
        ditolak("a = 07:32", "detik wajib");
        ditolak("a = {b = 1,}", "koma penutup");
        ditolak("a = {b = 1,\nc = 2}", "antar-baris");
        ditolak("a = 1 b = 2", "baris baru");
        ditolak("a =", "nilai hilang");
        ditolak("\"\"\"k\"\"\" = 1", "multi-baris");
        ditolak("a = \"\"\"x\"\"\"\"\"\"", "dua tanda kutip");
        ditolak(&format!("a = {}{}", "[".repeat(300), "]".repeat(300)), "bersarang lebih dari");
        ditolak(&format!("[{}]", vec!["a"; 300].join(".")), "bersarang lebih dari");
        // Kunci bertitik di dalam tabel sebaris: sah, dan ikut beku.
        assert!(nilai("a = {b.c = 1, b.d = 2}").contains("dict {b: dict {c: 1, d: 2}}"));
        ditolak("a = {b.c = 1}\n[a.b]", "sebaris");
        // Sub-tabel di bawah tabel bertitik lewat header: sah (contoh di spesifikasi).
        assert!(nilai("[f]\napel.warna = 1\n[f.apel.rasa]\nmanis = true").contains("rasa: dict {manis: true}"));
    }

    #[test]
    fn penulis_bolak_balik() {
        let asal = "a = \"x\\\"\\n\\u0001\"\nb = [1, 2.5, nan, -inf]\n\n[\"c d\"]\ne = true\n\n[[l]]\nn = 1\n\n[[l]]\nn = 2\n\n[t]\nk = -0.0\n";
        let v = ke_nilai(urai(asal).unwrap());
        let teks = match t_stringify(&[v]).unwrap() {
            Value::Enum { payload: Some(p), .. } => match *p { Value::Str(s) => s, x => panic!("{}", x) },
            x => panic!("{}", x),
        };
        assert_eq!(teks.as_str(), asal);
        assert_eq!(tulis_kunci("a.b"), "\"a.b\"");
        assert!(tulis_sebaris(&Value::Int(1i128 << 70), 0).is_err() && tulis_sebaris(&Value::Nil, 0).is_err());
    }
}
