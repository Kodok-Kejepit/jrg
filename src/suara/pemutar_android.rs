// src/suara/pemutar_android.rs — pemutar lewat AAudio (`libaaudio.so`, Android 8+), dimuat saat
// jalan seperti pemutar PulseAudio di desktop. Konstanta dari <aaudio/AAudio.h>:
// FORMAT_PCM_I16 = 1, STATE_STOPPING = 9, STATE_STOPPED = 10.

use std::os::raw::{c_char, c_int, c_void};
use std::sync::OnceLock;

use super::LAJU;

const FORMAT_PCM_I16: i32 = 1;
const STATE_STOPPING: i32 = 9;
const STATE_STOPPED: i32 = 10;
const DETIK_NANO: i64 = 1_000_000_000;

type FnBuatPembangun = unsafe extern "C" fn(*mut *mut c_void) -> c_int;
type FnAtur = unsafe extern "C" fn(*mut c_void, i32);
type FnBuka = unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> c_int;
type FnSatu = unsafe extern "C" fn(*mut c_void) -> c_int;
type FnTulis = unsafe extern "C" fn(*mut c_void, *const c_void, i32, i64) -> c_int;
type FnTungguKeadaan = unsafe extern "C" fn(*mut c_void, i32, *mut i32, i64) -> c_int;
type FnTeks = unsafe extern "C" fn(c_int) -> *const c_char;

struct Api {
    buat: FnBuatPembangun, laju: FnAtur, kanal: FnAtur, format: FnAtur, buka: FnBuka, hapus_pembangun: FnSatu,
    mulai: FnSatu, tulis: FnTulis, berhenti: FnSatu, tunggu: FnTungguKeadaan, tutup: FnSatu, teks: FnTeks,
}

static API: OnceLock<Result<Api, String>> = OnceLock::new();

fn api() -> Result<&'static Api, String> {
    match API.get_or_init(muat) { Ok(a) => Ok(a), Err(e) => Err(e.clone()) }
}

fn muat() -> Result<Api, String> {
    let h = crate::ffi::native::load_library(Some("libaaudio.so"))
        .map_err(|e| format!("suara: tidak bisa memuat libaaudio.so (butuh Android 8 ke atas): {}", e))?;
    macro_rules! sym {
        ($n:literal, $t:ty) => {{
            let addr = crate::ffi::native::resolve_symbol(h, $n)
                .map_err(|e| format!("suara: libaaudio tidak punya `{}`: {}", $n, e))?;
            unsafe { std::mem::transmute::<usize, $t>(addr) }
        }};
    }
    Ok(Api {
        buat: sym!("AAudio_createStreamBuilder", FnBuatPembangun),
        laju: sym!("AAudioStreamBuilder_setSampleRate", FnAtur),
        kanal: sym!("AAudioStreamBuilder_setChannelCount", FnAtur),
        format: sym!("AAudioStreamBuilder_setFormat", FnAtur),
        buka: sym!("AAudioStreamBuilder_openStream", FnBuka),
        hapus_pembangun: sym!("AAudioStreamBuilder_delete", FnSatu),
        mulai: sym!("AAudioStream_requestStart", FnSatu),
        tulis: sym!("AAudioStream_write", FnTulis),
        berhenti: sym!("AAudioStream_requestStop", FnSatu),
        tunggu: sym!("AAudioStream_waitForStateChange", FnTungguKeadaan),
        tutup: sym!("AAudioStream_close", FnSatu),
        teks: sym!("AAudio_convertResultToText", FnTeks),
    })
}

fn teks_galat(a: &Api, kode: c_int) -> String {
    let p = unsafe { (a.teks)(kode) };
    if p.is_null() { return format!("kode {}", kode) }
    unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// Stream AAudio yang sudah terbuka, dibungkus supaya bisa dipindah ke thread.
struct Stream(*mut c_void);
unsafe impl Send for Stream {}

fn buka(a: &Api, nama_fn: &str) -> Result<Stream, String> {
    let mut pembangun: *mut c_void = std::ptr::null_mut();
    let k = unsafe { (a.buat)(&mut pembangun) };
    if k != 0 || pembangun.is_null() {
        return Err(format!("{}: AAudio tidak bisa disiapkan: {}", nama_fn, teks_galat(a, k)))
    }
    let mut s: *mut c_void = std::ptr::null_mut();
    let k = unsafe {
        (a.laju)(pembangun, LAJU as i32);
        (a.kanal)(pembangun, 1);
        (a.format)(pembangun, FORMAT_PCM_I16);
        let k = (a.buka)(pembangun, &mut s);
        (a.hapus_pembangun)(pembangun);
        k
    };
    if k != 0 || s.is_null() {
        return Err(format!("{}: perangkat suara tidak bisa dibuka: {}", nama_fn, teks_galat(a, k)))
    }
    Ok(Stream(s))
}

/// Apakah bunyi bisa diputar di sini: pustakanya ada DAN perangkatnya bisa dibuka.
pub fn tersedia() -> bool {
    let Ok(a) = api() else { return false };
    match buka(a, "suara::tersedia") {
        Ok(s) => { unsafe { (a.tutup)(s.0) }; true }
        Err(_) => false,
    }
}

fn tulis_habis(a: &Api, s: Stream, sampel: &[i16], nama_fn: &str) -> Result<(), String> {
    let hasil = (|| {
        let k = unsafe { (a.mulai)(s.0) };
        if k != 0 { return Err(format!("{}: pemutaran tidak bisa dimulai: {}", nama_fn, teks_galat(a, k))) }
        let mut sisa = sampel;
        while !sisa.is_empty() {
            let n = sisa.len().min(i32::MAX as usize) as i32;
            let k = unsafe { (a.tulis)(s.0, sisa.as_ptr() as *const c_void, n, DETIK_NANO) };
            if k < 0 { return Err(format!("{}: gagal menulis ke perangkat suara: {}", nama_fn, teks_galat(a, k))) }
            sisa = &sisa[k as usize..];
        }
        // `requestStop` memainkan dulu yang masih di penampung; tunggu sampai benar-benar berhenti.
        let k = unsafe { (a.berhenti)(s.0) };
        if k != 0 { return Err(format!("{}: gagal menunggu bunyinya habis: {}", nama_fn, teks_galat(a, k))) }
        let mut keadaan = STATE_STOPPING;
        for _ in 0..30 {
            let mut berikut = keadaan;
            if unsafe { (a.tunggu)(s.0, keadaan, &mut berikut, DETIK_NANO) } != 0 || berikut == STATE_STOPPED { break; }
            keadaan = berikut;
        }
        Ok(())
    })();
    unsafe { (a.tutup)(s.0) };
    hasil
}

/// Putar di thread sendiri, supaya putaran game tidak tertahan.
pub fn putar(sampel: Vec<i16>) -> Result<(), String> {
    let a = api()?;
    let s = buka(a, "suara::putar")?;
    std::thread::spawn(move || { let _ = tulis_habis(a, s, &sampel, "suara::putar"); });
    Ok(())
}

/// Putar dan TUNGGU sampai habis.
pub fn putar_tunggu(sampel: Vec<i16>) -> Result<(), String> {
    let a = api()?;
    let s = buka(a, "suara::putar_tunggu")?;
    tulis_habis(a, s, &sampel, "suara::putar_tunggu")
}
