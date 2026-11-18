// runtime/src/bahasa.rs — bahasa galat biner `--emit-llvm`: aturan pilih yang sama dengan `jrgc` (`JRG_BAHASA`, lalu
// berkas setelan `jrgm language`, bawaan Inggris), tabel tersaring ditanam `jrgc` (`jrg_terjemah_pasang`).

use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_void;

use crate::terjemah_inti::{self, Pola};

extern "C" {
    fn getenv(nama: *const u8) -> *const u8;
    fn fopen(jalur: *const u8, mode: *const u8) -> *mut c_void;
    fn fread(buf: *mut u8, ukuran: usize, n: usize, f: *mut c_void) -> usize;
    fn fclose(f: *mut c_void) -> i32;
}

static mut TABEL: (*const u8, i64) = (core::ptr::null(), 0);
/// 0 = belum dibaca, 1 = Indonesia, 2 = Inggris.
static mut BAHASA: u8 = 0;
static mut POLA: Option<(Vec<Pola<'static>>, Vec<(&'static str, &'static str)>)> = None;

#[no_mangle]
pub unsafe extern "C" fn jrg_terjemah_pasang(p: *const u8, n: i64) {
    TABEL = (p, n);
}

unsafe fn cstr<'a>(p: *const u8) -> Option<&'a [u8]> {
    if p.is_null() { return None; }
    let mut n = 0;
    while *p.add(n) != 0 { n += 1; }
    Some(core::slice::from_raw_parts(p, n))
}

/// Padanan `Bahasa::dari_nama`: `Some(true)` Inggris, `Some(false)` Indonesia.
fn dari_nama(s: &[u8]) -> Option<bool> {
    let s = core::str::from_utf8(s).ok()?.trim();
    let sama = |x: &str| s.eq_ignore_ascii_case(x);
    if sama("en") || sama("english") || sama("inggris") { return Some(true); }
    if sama("id") || sama("indonesia") || sama("indonesian") || sama("in") { return Some(false); }
    None
}

/// Tanpa alokasi: dipanggil juga saat memori sudah habis.
unsafe fn dari_setelan() -> Option<bool> {
    let mut jalur = [0u8; 1100];
    let mut n = 0;
    let mut tulis = |b: &[u8], n: &mut usize| -> Option<()> {
        if *n + b.len() >= jalur.len() { return None; }
        jalur[*n..*n + b.len()].copy_from_slice(b);
        *n += b.len();
        Some(())
    };
    match cstr(getenv(b"XDG_CONFIG_HOME\0".as_ptr())).filter(|v| !v.is_empty()) {
        Some(v) => tulis(v, &mut n)?,
        None => match cstr(getenv(b"APPDATA\0".as_ptr())) {
            Some(v) => tulis(v, &mut n)?,
            None => { tulis(cstr(getenv(b"HOME\0".as_ptr()))?, &mut n)?; tulis(b"/.config", &mut n)? }
        },
    }
    tulis(b"/jrg/bahasa", &mut n)?;
    let f = fopen(jalur.as_ptr(), b"r\0".as_ptr());
    if f.is_null() { return None; }
    let mut isi = [0u8; 64];
    let m = fread(isi.as_mut_ptr(), 1, isi.len(), f);
    fclose(f);
    let isi = &isi[..m];
    dari_nama(isi.split(|b| *b == b'\n').next().unwrap_or(&[]))
}

pub fn inggris() -> bool {
    unsafe {
        if BAHASA == 0 {
            let en = cstr(getenv(b"JRG_BAHASA\0".as_ptr())).and_then(dari_nama)
                .or_else(|| dari_setelan()).unwrap_or(true);
            BAHASA = if en { 2 } else { 1 };
        }
        BAHASA == 2
    }
}

/// Teks galat dalam bahasa yang berlaku. Biner tanpa tabel (IR lama) menjawab teks aslinya.
pub fn t(teks: &str) -> String {
    if !inggris() { return teks.to_owned(); }
    unsafe {
        let p = &mut *core::ptr::addr_of_mut!(POLA);
        if p.is_none() {
            let (ptr, n) = TABEL;
            if ptr.is_null() || n <= 0 { return teks.to_owned(); }
            let blob: &'static str = core::str::from_utf8(core::slice::from_raw_parts(ptr, n as usize)).unwrap_or("");
            let (tabel, kata) = terjemah_inti::urai_blob(blob);
            *p = Some((terjemah_inti::siapkan(tabel.into_iter()), kata));
        }
        let (pola, kata) = p.as_ref().unwrap();
        terjemah_inti::terjemah(pola, kata, teks)
    }
}

/// Label pendek di pintu keluar (`bantuan`/`help`).
pub fn pilih(id: &'static str, en: &'static str) -> &'static str {
    if inggris() { en } else { id }
}
