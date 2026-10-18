// src/suara/pemutar.rs — pemutar lewat `libpulse-simple`, dimuat saat jalan
// (pola driver SQLite): tanpa crate, jadi satu biner tetap jalan di mana pun.
// Konstanta diturunkan dari header pulse: S16LE = 3, PLAYBACK = 1, spec 12 byte.

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::sync::OnceLock;

use super::LAJU;

const PA_SAMPLE_S16LE: c_int = 3;
const PA_STREAM_PLAYBACK: c_int = 1;

#[repr(C)]
struct SpecSampel { format: c_int, laju: u32, kanal: u8 }

type FnBaru = unsafe extern "C" fn(*const c_char, *const c_char, c_int, *const c_char,
    *const c_char, *const SpecSampel, *const c_void, *const c_void, *mut c_int) -> *mut c_void;
type FnTulis = unsafe extern "C" fn(*mut c_void, *const c_void, usize, *mut c_int) -> c_int;
type FnKuras = unsafe extern "C" fn(*mut c_void, *mut c_int) -> c_int;
type FnBebas = unsafe extern "C" fn(*mut c_void);
type FnGalat = unsafe extern "C" fn(c_int) -> *const c_char;

struct Api { baru: FnBaru, tulis: FnTulis, kuras: FnKuras, bebas: FnBebas, galat: FnGalat }

const KANDIDAT: &[&str] = &["libpulse-simple.so.0", "libpulse-simple.so"];

static API: OnceLock<Result<Api, String>> = OnceLock::new();

fn api() -> Result<&'static Api, String> {
    match API.get_or_init(muat) { Ok(a) => Ok(a), Err(e) => Err(e.clone()) }
}

fn muat() -> Result<Api, String> {
    let mut gagal = Vec::new();
    let mut lib = None;
    for nama in KANDIDAT {
        match crate::ffi::native::load_library(Some(nama)) {
            Ok(h) => { lib = Some(h); break }
            Err(e) => gagal.push(format!("{}: {}", nama, e)),
        }
    }
    let Some(h) = lib else {
        return Err(format!(
            "suara: tidak bisa memuat libpulse-simple — suara diputar lewat PulseAudio/PipeWire \
             milik sistem, bukan ditautkan ke biner ini.\n  bantuan: Debian/Ubuntu: `apt install \
             libpulse0`; tanpa pemutar, `suara::simpan_wav` tetap bisa dipakai\n  yang dicoba: {}",
            gagal.join("; ")));
    };
    macro_rules! sym {
        ($n:literal, $t:ty) => {{
            let addr = crate::ffi::native::resolve_symbol(h, $n)
                .map_err(|e| format!("suara: libpulse-simple tidak punya `{}`: {}", $n, e))?;
            unsafe { std::mem::transmute::<usize, $t>(addr) }
        }};
    }
    Ok(Api {
        baru: sym!("pa_simple_new", FnBaru),
        tulis: sym!("pa_simple_write", FnTulis),
        kuras: sym!("pa_simple_drain", FnKuras),
        bebas: sym!("pa_simple_free", FnBebas),
        galat: sym!("pa_strerror", FnGalat),
    })
}

fn teks_galat(a: &Api, kode: c_int) -> String {
    let p = unsafe { (a.galat)(kode) };
    if p.is_null() { return format!("kode {}", kode) }
    unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// Stream pulse yang sudah terhubung, dibungkus supaya bisa dipindah ke thread.
struct Stream(*mut c_void);
unsafe impl Send for Stream {}

fn buka(a: &Api, nama_fn: &str) -> Result<Stream, String> {
    let spec = SpecSampel { format: PA_SAMPLE_S16LE, laju: LAJU, kanal: 1 };
    let nama = CString::new("jrgc").unwrap();
    let alir = CString::new("suara").unwrap();
    let mut kode: c_int = 0;
    let s = unsafe {
        (a.baru)(std::ptr::null(), nama.as_ptr(), PA_STREAM_PLAYBACK, std::ptr::null(),
                 alir.as_ptr(), &spec, std::ptr::null(), std::ptr::null(), &mut kode)
    };
    if s.is_null() {
        return Err(format!("{}: tidak bisa terhubung ke server suara: {}", nama_fn, teks_galat(a, kode)))
    }
    Ok(Stream(s))
}

/// Apakah bunyi bisa diputar di sini: pustakanya ada DAN servernya menjawab.
pub fn tersedia() -> bool {
    let Ok(a) = api() else { return false };
    match buka(a, "suara::tersedia") {
        Ok(s) => { unsafe { (a.bebas)(s.0) }; true }
        Err(_) => false,
    }
}

fn tulis_habis(a: &Api, s: Stream, sampel: &[i16], nama_fn: &str) -> Result<(), String> {
    let mut kode: c_int = 0;
    let hasil = unsafe {
        if (a.tulis)(s.0, sampel.as_ptr() as *const c_void, sampel.len() * 2, &mut kode) != 0 {
            Err(format!("{}: gagal menulis ke server suara: {}", nama_fn, teks_galat(a, kode)))
        } else if (a.kuras)(s.0, &mut kode) != 0 {
            Err(format!("{}: gagal menunggu bunyinya habis: {}", nama_fn, teks_galat(a, kode)))
        } else {
            Ok(())
        }
    };
    unsafe { (a.bebas)(s.0) };
    hasil
}

/// Putar di thread sendiri, supaya putaran game tidak tertahan. Galat KONEKSI dijawab
/// sekarang; sesudah terhubung, penulisan cuma gagal kalau servernya mati di tengah jalan.
pub fn putar(sampel: Vec<i16>) -> Result<(), String> {
    let a = api()?;
    let s = buka(a, "suara::putar")?;
    std::thread::spawn(move || { let _ = tulis_habis(a, s, &sampel, "suara::putar"); });
    Ok(())
}

/// Putar dan TUNGGU sampai habis — untuk skrip yang selesai sesudahnya (thread
/// `putar` mati bersama prosesnya, jadi bunyinya terpotong).
pub fn putar_tunggu(sampel: Vec<i16>) -> Result<(), String> {
    let a = api()?;
    let s = buka(a, "suara::putar_tunggu")?;
    tulis_habis(a, s, &sampel, "suara::putar_tunggu")
}
