// src/dok/tls.rs — HTTPS untuk klien `http::`/`dok::http_*` lewat libssl sistem yang dimuat saat jalan
// (dlopen, pola yang sama dengan SQLite dan PulseAudio): tanpa dependensi crate, sertifikat diperiksa
// terhadap penyimpanan sistem dan nama host. Tanpa libssl (Android di luar Termux) jawabannya galat bernama.

use std::ffi::CString;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::raw::{c_char, c_int, c_long, c_ulong, c_void};
use std::sync::OnceLock;

type P = *mut c_void;

struct Ssl {
    ctx_new: unsafe extern "C" fn(P) -> P,
    client_method: unsafe extern "C" fn() -> P,
    ctx_default_verify_paths: unsafe extern "C" fn(P) -> c_int,
    ctx_set_verify: unsafe extern "C" fn(P, c_int, P),
    ctx_free: unsafe extern "C" fn(P),
    new: unsafe extern "C" fn(P) -> P,
    set_fd: unsafe extern "C" fn(P, c_int) -> c_int,
    ctrl: unsafe extern "C" fn(P, c_int, c_long, P) -> c_long,
    set1_host: unsafe extern "C" fn(P, *const c_char) -> c_int,
    connect: unsafe extern "C" fn(P) -> c_int,
    read: unsafe extern "C" fn(P, P, c_int) -> c_int,
    write: unsafe extern "C" fn(P, *const c_void, c_int) -> c_int,
    get_error: unsafe extern "C" fn(P, c_int) -> c_int,
    verify_result: unsafe extern "C" fn(P) -> c_long,
    shutdown: unsafe extern "C" fn(P) -> c_int,
    free: unsafe extern "C" fn(P),
    err_get: unsafe extern "C" fn() -> c_ulong,
    err_string: unsafe extern "C" fn(c_ulong, *mut c_char, usize),
    cert_string: unsafe extern "C" fn(c_long) -> *const c_char,
}

// SAFETY: penunjuk fungsi pustaka C yang dimuat sekali; libssl sendiri aman lintas utas sejak 1.1.
unsafe impl Send for Ssl {}
unsafe impl Sync for Ssl {}

#[cfg(unix)]
fn muat() -> Result<Ssl, String> {
    extern "C" {
        fn dlopen(f: *const c_char, flag: c_int) -> P;
        fn dlsym(h: P, s: *const c_char) -> P;
    }
    let calon = ["libssl.so.3", "libssl.so", "libssl.so.1.1", "libssl.3.dylib", "libssl.dylib"];
    let mut h: P = std::ptr::null_mut();
    for c in calon {
        let n = CString::new(c).unwrap();
        // SAFETY: dlopen dengan nama berakhir NUL; RTLD_NOW | RTLD_GLOBAL supaya libcrypto ikut terlihat.
        h = unsafe { dlopen(n.as_ptr(), 2 | 0x100) };
        if !h.is_null() { break; }
    }
    if h.is_null() {
        return Err("HTTPS butuh libssl (OpenSSL 1.1/3) dan pustakanya tidak ditemukan di sistem ini\n  bantuan: \
                    Debian/Ubuntu `sudo apt install libssl3`, Termux `pkg install openssl`; atau pakai http://".into());
    }
    macro_rules! sim {
        ($n:expr) => {{
            let n = CString::new($n).unwrap();
            // SAFETY: simbol dicari di pegangan yang sah; tipenya mengikuti tanda tangan OpenSSL.
            let p = unsafe { dlsym(h, n.as_ptr()) };
            if p.is_null() { return Err(format!("libssl tanpa simbol `{}` (versi terlalu lama?)", $n)); }
            unsafe { std::mem::transmute(p) }
        }};
    }
    Ok(Ssl {
        ctx_new: sim!("SSL_CTX_new"), client_method: sim!("TLS_client_method"),
        ctx_default_verify_paths: sim!("SSL_CTX_set_default_verify_paths"), ctx_set_verify: sim!("SSL_CTX_set_verify"),
        ctx_free: sim!("SSL_CTX_free"), new: sim!("SSL_new"), set_fd: sim!("SSL_set_fd"), ctrl: sim!("SSL_ctrl"),
        set1_host: sim!("SSL_set1_host"), connect: sim!("SSL_connect"), read: sim!("SSL_read"), write: sim!("SSL_write"),
        get_error: sim!("SSL_get_error"), verify_result: sim!("SSL_get_verify_result"), shutdown: sim!("SSL_shutdown"),
        free: sim!("SSL_free"), err_get: sim!("ERR_get_error"), err_string: sim!("ERR_error_string_n"),
        cert_string: sim!("X509_verify_cert_error_string"),
    })
}

#[cfg(not(unix))]
fn muat() -> Result<Ssl, String> { Err("HTTPS belum ada di sistem ini; pakai http://".into()) }

fn ssl() -> Result<&'static Ssl, String> {
    static S: OnceLock<Result<Ssl, String>> = OnceLock::new();
    S.get_or_init(muat).as_ref().map_err(|e| e.clone())
}

fn galat_terakhir(s: &Ssl) -> String {
    // SAFETY: ERR_* membaca antrean galat utas ini; penyangga 256 byte cukup untuk ERR_error_string_n.
    unsafe {
        let k = (s.err_get)();
        if k == 0 { return "sambungan TLS putus".into(); }
        let mut b = [0 as c_char; 256];
        (s.err_string)(k, b.as_mut_ptr(), b.len());
        std::ffi::CStr::from_ptr(b.as_ptr()).to_string_lossy().into_owned()
    }
}

/// Sambungan TLS di atas sebuah `TcpStream` (yang tetap dipegang supaya fd-nya hidup).
pub struct Tls {
    ctx: P,
    ssl: P,
    _tcp: TcpStream,
}

// SAFETY: satu `Tls` dipakai satu pemilik pada satu waktu (Read/Write butuh &mut).
unsafe impl Send for Tls {}

impl Tls {
    pub fn sambung(tcp: TcpStream, host: &str) -> Result<Tls, String> {
        let s = ssl()?;
        #[cfg(unix)]
        let fd = { use std::os::unix::io::AsRawFd; tcp.as_raw_fd() };
        #[cfg(not(unix))]
        let fd: c_int = -1;
        let nama = CString::new(host).map_err(|_| "nama host memuat NUL".to_string())?;
        // SAFETY: urutan panggilan OpenSSL yang lazim; tiap penunjuk null diperiksa sebelum dipakai.
        unsafe {
            let ctx = (s.ctx_new)((s.client_method)());
            if ctx.is_null() { return Err(format!("TLS: {}", galat_terakhir(s))); }
            (s.ctx_default_verify_paths)(ctx);
            (s.ctx_set_verify)(ctx, 1, std::ptr::null_mut());
            let ssl = (s.new)(ctx);
            if ssl.is_null() { (s.ctx_free)(ctx); return Err(format!("TLS: {}", galat_terakhir(s))); }
            let t = Tls { ctx, ssl, _tcp: tcp };
            (s.set_fd)(ssl, fd);
            (s.ctrl)(ssl, 55, 0, nama.as_ptr() as P);
            (s.set1_host)(ssl, nama.as_ptr());
            if (s.connect)(ssl) != 1 {
                let v = (s.verify_result)(ssl);
                let alasan = if v != 0 {
                    format!("sertifikat {} ditolak: {}", host, std::ffi::CStr::from_ptr((s.cert_string)(v)).to_string_lossy())
                } else { galat_terakhir(s) };
                return Err(format!("TLS ke {}: {}", host, alasan));
            }
            Ok(t)
        }
    }
}

impl Read for Tls {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let s = ssl().map_err(std::io::Error::other)?;
        let n = buf.len().min(c_int::MAX as usize) as c_int;
        // SAFETY: penyangga milik pemanggil, panjangnya dijepit ke c_int.
        let r = unsafe { (s.read)(self.ssl, buf.as_mut_ptr() as P, n) };
        if r > 0 { return Ok(r as usize); }
        // SSL_ERROR_ZERO_RETURN (6) = ditutup rapi; server yang memutus tanpa close_notify juga dianggap selesai.
        match unsafe { (s.get_error)(self.ssl, r) } {
            6 | 5 => Ok(0),
            _ => Err(std::io::Error::other(galat_terakhir(s))),
        }
    }
}

impl Write for Tls {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let s = ssl().map_err(std::io::Error::other)?;
        let n = buf.len().min(c_int::MAX as usize) as c_int;
        // SAFETY: penyangga milik pemanggil, panjangnya dijepit ke c_int.
        let r = unsafe { (s.write)(self.ssl, buf.as_ptr() as *const c_void, n) };
        if r > 0 { Ok(r as usize) } else { Err(std::io::Error::other(galat_terakhir(s))) }
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

impl Drop for Tls {
    fn drop(&mut self) {
        if let Ok(s) = ssl() {
            // SAFETY: pegangan milik struct ini, dibebaskan sekali.
            unsafe { (s.shutdown)(self.ssl); (s.free)(self.ssl); (s.ctx_free)(self.ctx); }
        }
    }
}
