//! `php::` — memanggil PHP dari JRG, lewat mesin worker yang sama dengan
//! `py::` dan `js::`.

use crate::interpreter::{ok_value, Value};
use std::sync::{Mutex, OnceLock};

use super::pekerja::{self, Panggil, Runtime, Worker};

fn slot() -> &'static Mutex<Option<Worker>> {
    static SLOT: OnceLock<Mutex<Option<Worker>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

static GENERASI: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn antrean_lepas() -> &'static Mutex<Vec<u64>> {
    static Q: OnceLock<Mutex<Vec<u64>>> = OnceLock::new();
    Q.get_or_init(|| Mutex::new(Vec::new()))
}

fn registri_fungsi() -> &'static Mutex<pekerja::RegistriFungsi> {
    static R: OnceLock<Mutex<pekerja::RegistriFungsi>> = OnceLock::new();
    R.get_or_init(Default::default)
}

thread_local! {
    static FUNGSI_BOLEH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static DALAM_CALLBACK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub static RT: Runtime = Runtime {
    nama: "php",
    ns: "php",
    bin: "php",
    // Galat yang lolos dari penangan (fatal saat kompilasi, galat startup) ke
    // stderr, bukan ke stdout yang jadi kawat protokol; prepend tidak ikut.
    bin_args: &["-d", "display_errors=stderr", "-d", "log_errors=0", "-d", "html_errors=0",
                "-d", "auto_prepend_file=", "-d", "auto_append_file="],
    skrip_src: include_str!("php_runner.php"),
    skrip_nama: "jrg_php_runner.php",
    bantuan_pasang: "pastikan `php` (8.0 ke atas) ada di PATH — di Debian/Ubuntu: apt install \
                     php-cli; di Termux: pkg install php",
    slot,
    generasi: &GENERASI,
    antrean: antrean_lepas,
    registri: registri_fungsi,
    fungsi_boleh_get: || FUNGSI_BOLEH.with(|b| b.get()),
    fungsi_boleh_set: |v| FUNGSI_BOLEH.with(|b| b.set(v)),
    dalam_cb_get: || DALAM_CALLBACK.with(|c| c.get()),
    dalam_cb_set: |v| DALAM_CALLBACK.with(|c| c.set(v)),
    buat_handle: |id, generasi, tipe| {
        ok_value(Value::PhpObj(std::sync::Arc::new(PhpObjInner { id, generasi, tipe })))
    },
    tulis_handle: |v, out| match v {
        Value::PhpObj(h) => Some(tulis_handle_php(h, out)),
        // `int` PHP 64 bit, dan `json_decode` mengubah yang lebih besar jadi float
        // tanpa sepatah kata — jadi ditolak di sini, di tiap kedalaman.
        Value::Int(n) if *n > i64::MAX as i128 || *n < i64::MIN as i128 => Some(Err(format!(
            "memuat bilangan bulat {} yang tidak muat di `int` PHP (64 bit, {} sampai {}) — \
             PHP akan diam-diam mengubahnya jadi float yang tidak tepat.\n  bantuan: kirim \
             sebagai teks (`str(x)`) kalau angkanya harus utuh, atau `x as float` kalau \
             perkiraan cukup", n, i64::MIN, i64::MAX))),
        _ => None,
    },
};

fn tulis_handle_php(h: &PhpRef, out: &mut String) -> Result<(), String> {
    if h.generasi != RT.generasi_kini() {
        return Err(format!(
            "berupa handle PHP dari sesi worker sebelumnya (sesi #{}, sekarang #{}) — \
             objeknya sudah hilang bersama proses lamanya",
            h.generasi, RT.generasi_kini()));
    }
    out.push_str(&format!("{{\"__pyref__\":{}}}", h.id));
    Ok(())
}

/// Isi sebuah handle PHP. Dipegang bersama lewat `Arc`.
#[derive(Debug)]
pub struct PhpObjInner {
    pub id: u64,
    pub generasi: u64,
    /// Nama kelasnya (`DateTime`, `PDO`, `Closure`) atau jenis nilainya
    /// (`array`, `string`), untuk `typeof` dan pesan galat.
    pub tipe: String,
}

impl Drop for PhpObjInner {
    fn drop(&mut self) {
        // Handle dari sesi yang sudah mati tidak perlu dikabarkan.
        if self.generasi != RT.generasi_kini() { return; }
        RT.antre_lepas(self.id);
    }
}

pub type PhpRef = std::sync::Arc<PhpObjInner>;

// ---------------------------------------------------------------------------
// Perakitan permintaan
// ---------------------------------------------------------------------------

fn str_json(s: &str, out: &mut String) { pekerja::escape_json_string(s, out) }

/// Semua permintaan `php::` membawa kemampuan memanggil balik: kode PHP apa pun
/// — termasuk `__get`, `__destruct`, templat — bisa memanggil fungsi JRG yang
/// pernah disimpannya.
fn kirim(panggil: Panggil<'_>, bangun: impl FnOnce(&mut String) -> Result<(), String>) -> Value {
    pekerja::jalankan_dgn_fungsi(&RT, bangun, panggil)
}

fn nilai(v: &Value, out: &mut String, peran: &str) -> Result<(), String> {
    pekerja::value_to_json(&RT, v, out).map_err(|e| format!("{} {}", peran, e))
}

fn rakit_args(args: &[Value], kw: Option<&Value>, req: &mut String) -> Result<(), String> {
    req.push_str(",\"args\":[");
    for (i, a) in args.iter().enumerate() {
        if i > 0 { req.push(','); }
        nilai(a, req, &pekerja::argumen_diteruskan(i, "PHP"))?;
    }
    req.push(']');
    if let Some(kw) = kw {
        req.push_str(",\"kwargs\":");
        nilai(kw, req, "argumen bernamanya")?;
    }
    Ok(())
}

fn periksa(h: &PhpRef, siapa: &str) -> Result<(), Value> {
    RT.periksa_generasi(h.generasi, siapa)
}

// ---------------------------------------------------------------------------
// API — tiap fungsi memanggil satu operasi `php_runner.php`
// ---------------------------------------------------------------------------

/// `php::eval(kode)` / `php::ref(kode)` — nilai sebuah ekspresi PHP.
pub fn php_eval(kode: &str, ref_saja: bool, panggil: Panggil<'_>) -> Value {
    kirim(panggil, |req| {
        req.push_str("{\"op\":\"eval\",\"code\":");
        str_json(kode, req);
        if ref_saja { req.push_str(",\"ref_saja\":true"); }
        req.push('}');
        Ok(())
    })
}

/// `php::exec(kode)` — jalankan pernyataan; variabelnya bertahan.
pub fn php_exec(kode: &str, panggil: Panggil<'_>) -> Value {
    kirim(panggil, |req| {
        req.push_str("{\"op\":\"exec\",\"code\":");
        str_json(kode, req);
        req.push('}');
        Ok(())
    })
}

/// `php::call(fungsi, ...args)` — `fungsi` berupa nama (`"strlen"`,
/// `"Kelas::statik"`, `"Ns\\fungsi"`) atau handle ke sesuatu yang bisa dipanggil.
pub fn php_call(fungsi: &Value, args: &[Value], kw: Option<&Value>, ref_saja: bool,
                panggil: Panggil<'_>) -> Value {
    if let Value::PhpObj(h) = fungsi {
        if let Err(e) = periksa(h, "php::call") { return e; }
    }
    kirim(panggil, |req| {
        req.push_str("{\"op\":\"call\",\"func\":");
        nilai(fungsi, req, "fungsinya")?;
        if ref_saja { req.push_str(",\"ref_saja\":true"); }
        rakit_args(args, kw, req)?;
        req.push('}');
        Ok(())
    })
}

/// `php::new(kelas, ...args)` — selalu menjawab handle.
pub fn php_new(kelas: &str, args: &[Value], kw: Option<&Value>, panggil: Panggil<'_>) -> Value {
    kirim(panggil, |req| {
        req.push_str("{\"op\":\"new\",\"class\":");
        str_json(kelas, req);
        rakit_args(args, kw, req)?;
        req.push('}');
        Ok(())
    })
}

pub fn php_method(h: &PhpRef, nama: &str, args: &[Value], kw: Option<&Value>, ref_saja: bool,
                  panggil: Panggil<'_>) -> Value {
    if let Err(e) = periksa(h, "php::method") { return e; }
    let id = h.id;
    kirim(panggil, |req| {
        req.push_str(&format!("{{\"op\":\"method\",\"id\":{}", id));
        if ref_saja { req.push_str(",\"ref_saja\":true"); }
        req.push_str(",\"name\":");
        str_json(nama, req);
        rakit_args(args, kw, req)?;
        req.push('}');
        Ok(())
    })
}

pub fn php_get(h: &PhpRef, nama: &str, ref_saja: bool, panggil: Panggil<'_>) -> Value {
    if let Err(e) = periksa(h, "php::get") { return e; }
    let id = h.id;
    kirim(panggil, |req| {
        req.push_str(&format!("{{\"op\":\"getattr\",\"id\":{}", id));
        if ref_saja { req.push_str(",\"ref_saja\":true"); }
        req.push_str(",\"name\":");
        str_json(nama, req);
        req.push('}');
        Ok(())
    })
}

pub fn php_set(h: &PhpRef, nama: &str, v: &Value, panggil: Panggil<'_>) -> Value {
    if let Err(e) = periksa(h, "php::set") { return e; }
    let id = h.id;
    kirim(panggil, |req| {
        req.push_str(&format!("{{\"op\":\"setattr\",\"id\":{},\"name\":", id));
        str_json(nama, req);
        req.push_str(",\"value\":");
        nilai(v, req, "nilainya")?;
        req.push('}');
        Ok(())
    })
}

/// `op` salah satu `item`/`delitem`/`contains`; `nilai_baru` hanya untuk `setitem`.
pub fn php_kunci(h: &PhpRef, op: &str, siapa: &str, kunci: &Value, nilai_baru: Option<&Value>,
                 ref_saja: bool, panggil: Panggil<'_>) -> Value {
    if let Err(e) = periksa(h, siapa) { return e; }
    let id = h.id;
    kirim(panggil, |req| {
        req.push_str(&format!("{{\"op\":\"{}\",\"id\":{}", op, id));
        if ref_saja { req.push_str(",\"ref_saja\":true"); }
        req.push_str(",\"key\":");
        nilai(kunci, req, "kuncinya")?;
        if let Some(v) = nilai_baru {
            req.push_str(",\"value\":");
            nilai(v, req, "nilainya")?;
        }
        req.push('}');
        Ok(())
    })
}

/// `to_jrg`, `len`, `dir`, `repr` — satu handle, tanpa argumen lain.
pub fn php_op_handle(h: &PhpRef, op: &str, siapa: &str, panggil: Panggil<'_>) -> Value {
    if let Err(e) = periksa(h, siapa) { return e; }
    let id = h.id;
    kirim(panggil, |req| {
        req.push_str(&format!("{{\"op\":\"{}\",\"id\":{}}}", op, id));
        Ok(())
    })
}

/// `php::refs()` — berapa handle yang masih dipegang worker.
pub fn php_refs(panggil: Panggil<'_>) -> Value {
    kirim(panggil, |req| { req.push_str("{\"op\":\"ping\"}"); Ok(()) })
}

/// `php::require(berkas)` — `require_once`, dibaca relatif terhadap direktori kerja.
pub fn php_require(jalur: &str, panggil: Panggil<'_>) -> Value {
    kirim(panggil, |req| {
        req.push_str("{\"op\":\"require\",\"path\":");
        str_json(jalur, req);
        req.push('}');
        Ok(())
    })
}

/// `php::render(berkas, data)` dan `php::render_str(templat, data)` — keluaran
/// templat PHP sebagai teks. PHP TIDAK meloloskan HTML otomatis.
pub fn php_render(sumber: &str, berkas: bool, data: Option<&Value>, panggil: Panggil<'_>) -> Value {
    kirim(panggil, |req| {
        if berkas {
            req.push_str("{\"op\":\"render\",\"path\":");
        } else {
            req.push_str("{\"op\":\"render\",\"berkas\":false,\"template\":");
        }
        str_json(sumber, req);
        if let Some(d) = data {
            req.push_str(",\"vars\":");
            nilai(d, req, "datanya")?;
        }
        req.push('}');
        Ok(())
    })
}

/// `php::tersedia()` — apakah worker PHP bisa dijalankan di mesin ini.
pub fn php_tersedia(panggil: Panggil<'_>) -> Value {
    match php_refs(panggil) {
        Value::Enum { variant, .. } if variant == "Ok" => Value::Bool(true),
        _ => Value::Bool(false),
    }
}

/// Pesan untuk argumen yang harus handle PHP.
pub fn pesan_bukan_handle(fname: &str, n: usize, dapat: &Value) -> String {
    format!(
        "{}: argumen {} harus handle PHP (hasil `php::ref(...)`/`php::new(...)`, atau nilai \
         balik `php::` yang memang tidak bisa dikonversi), dapat `{}`",
        fname, n, dapat.type_name())
}

