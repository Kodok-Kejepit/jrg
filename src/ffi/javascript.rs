//! `js::` — memanggil JavaScript (Node.js) dari JRG.

use crate::interpreter::{err_value, ok_value, Value};
use std::sync::{Mutex, OnceLock};

use super::pekerja::{self, Runtime, Worker};

// ---------------------------------------------------------------------------
// Runtime
// ---------------------------------------------------------------------------

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
    nama: "javascript",
    ns: "js",
    bin: "node",
    bin_args: &[],
    skrip_src: include_str!("js_runner.js"),
    skrip_nama: "jrg_js_runner.js",
    bantuan_pasang: "pastikan `node` ada di PATH — di Debian/Ubuntu: apt install nodejs; \
                     di Termux: pkg install nodejs",
    slot,
    generasi: &GENERASI,
    antrean: antrean_lepas,
    registri: registri_fungsi,
    fungsi_boleh_get: || FUNGSI_BOLEH.with(|b| b.get()),
    fungsi_boleh_set: |v| FUNGSI_BOLEH.with(|b| b.set(v)),
    dalam_cb_get: || DALAM_CALLBACK.with(|c| c.get()),
    dalam_cb_set: |v| DALAM_CALLBACK.with(|c| c.set(v)),
    buat_handle: |id, generasi, tipe| {
        ok_value(Value::JsObj(std::sync::Arc::new(JsObjInner { id, generasi, tipe })))
    },
    tulis_handle: |v, out| match v {
        Value::JsObj(h) => Some(tulis_handle_js(h, out)),
        // `JSON.parse` membaca tiap angka sebagai float 64 bit: yang di luar ±2^53
        // tiba sebagai angka LAIN, tanpa sepatah kata. Ditolak di tiap kedalaman.
        Value::Int(n) if n.unsigned_abs() > (1u128 << 53) - 1 => Some(Err(format!(
            "memuat bilangan bulat {} di luar jangkauan angka aman JavaScript (±{}) — \
             `Number` JS adalah float 64 bit, jadi angkanya akan diam-diam berubah.\n  \
             bantuan: kirim sebagai teks (`str(x)`) lalu `BigInt(...)` di sisi JS, atau \
             `x as float` kalau perkiraan cukup", n, (1u64 << 53) - 1))),
        _ => None,
    },
};

fn tulis_handle_js(h: &JsRef, out: &mut String) -> Result<(), String> {
    if h.generasi != RT.generasi_kini() {
        return Err(format!(
            "berupa handle JavaScript dari sesi worker sebelumnya (sesi #{}, sekarang \
             #{}) — objeknya sudah hilang bersama proses lamanya",
            h.generasi, RT.generasi_kini()));
    }
    out.push_str(&format!("{{\"__pyref__\":{}}}", h.id));
    Ok(())
}

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Isi sebuah handle JavaScript. Dipegang bersama lewat `Arc`.
#[derive(Debug)]
pub struct JsObjInner {
    pub id: u64,
    pub generasi: u64,
    /// Nama konstruktornya (`Buffer`, `Array`, `Object`, …), dipakai `typeof`
    /// dan pesan galat supaya handle tidak tampil sebagai angka tanpa arti.
    pub tipe: String,
}

impl Drop for JsObjInner {
    fn drop(&mut self) {
        // Objek dari sesi worker yang sudah mati tidak perlu dikabarkan —
        // prosesnya sudah membawa serta seluruh tabelnya.
        if self.generasi != RT.generasi_kini() { return; }
        RT.antre_lepas(self.id);
    }
}

pub type JsRef = std::sync::Arc<JsObjInner>;

fn periksa_generasi(h: &JsRef, fname: &str) -> Result<(), Value> {
    RT.periksa_generasi(h.generasi, fname)
}

// ---------------------------------------------------------------------------
// Perakitan permintaan
// ---------------------------------------------------------------------------

fn str_json(s: &str, out: &mut String) { pekerja::escape_json_string(s, out) }

fn kirim(req: &str) -> Value { pekerja::kirim_tanpa_fungsi(&RT, req) }

/// Rakit `"args":[...]`. Sebuah fungsi JRG di dalamnya berubah jadi nomornya —
/// lihat `pekerja::value_to_json`.
fn rakit_args(args: &[Value], req: &mut String) -> Result<(), String> {
    req.push_str(",\"args\":[");
    for (i, a) in args.iter().enumerate() {
        if i > 0 { req.push(','); }
        pekerja::value_to_json(&RT, a, req)
            .map_err(|e| format!("{} {}", pekerja::argumen_diteruskan(i, "JavaScript"), e))?;
    }
    req.push(']');
    Ok(())
}

// ---------------------------------------------------------------------------
// API
// ---------------------------------------------------------------------------

/// `js::eval(code)` — nilai sebuah **ekspresi** JavaScript.
pub fn js_eval(code: &str) -> Value {
    let mut req = String::from("{\"op\":\"eval\",\"code\":");
    str_json(code, &mut req);
    req.push('}');
    kirim(&req)
}

/// `js::ref(code)` — seperti `js::eval`, tapi hasilnya SELALU jadi handle.
pub fn js_ref(code: &str) -> Value {
    let mut req = String::from("{\"op\":\"eval\",\"ref_saja\":true,\"code\":");
    str_json(code, &mut req);
    req.push('}');
    kirim(&req)
}

/// `js::exec(code)` — jalankan pernyataan; tidak mengembalikan nilai.
pub fn js_exec(code: &str) -> Value {
    let mut req = String::from("{\"op\":\"exec\",\"code\":");
    str_json(code, &mut req);
    req.push('}');
    kirim(&req)
}

/// `js::import(modul)` — `require`, dengan jatuh ke `import()` untuk paket
/// ESM.
pub fn js_import(modul: &str) -> Value {
    let mut req = String::from("{\"op\":\"import\",\"module\":");
    str_json(modul, &mut req);
    req.push('}');
    kirim(&req)
}

/// `js::call(modul, fungsi, ...args)`.
///
/// `fungsi` boleh berjalur: `js::call("fs", "promises.readFile", path, "utf8")`.
pub fn js_call(modul: &str, fungsi: &str, args: Vec<Value>, panggil: pekerja::Panggil<'_>) -> Value {
    js_call_opt(modul, fungsi, args, false, panggil)
}

pub fn js_call_opt(
    modul: &str, fungsi: &str, args: Vec<Value>, ref_saja: bool, panggil: pekerja::Panggil<'_>,
) -> Value {
    pekerja::jalankan_dgn_fungsi(&RT, |req| {
        req.push_str("{\"op\":\"call\",\"module\":");
        str_json(modul, req);
        req.push_str(",\"func\":");
        str_json(fungsi, req);
        if ref_saja { req.push_str(",\"ref_saja\":true"); }
        rakit_args(&args, req)?;
        req.push('}');
        Ok(())
    }, panggil)
}

fn op_handle(h: &JsRef, op: &str, siapa: &str) -> Value {
    if let Err(e) = periksa_generasi(h, siapa) { return e; }
    kirim(&format!("{{\"op\":\"{}\",\"id\":{}}}", op, h.id))
}

pub fn js_to_jrg(h: &JsRef) -> Value { op_handle(h, "to_jrg", "js::to_jrg") }
pub fn js_len(h: &JsRef) -> Value { op_handle(h, "len", "js::len") }
pub fn js_dir(h: &JsRef) -> Value { op_handle(h, "dir", "js::dir") }
pub fn js_repr(h: &JsRef) -> Value { op_handle(h, "repr", "js::repr") }

/// `js::refs()` — berapa handle yang masih dipegang worker.
pub fn js_refs() -> Value { kirim("{\"op\":\"ping\"}") }

pub fn js_get(h: &JsRef, nama: &str) -> Value { js_get_opt(h, nama, false) }
pub fn js_ref_get(h: &JsRef, nama: &str) -> Value { js_get_opt(h, nama, true) }

fn js_get_opt(h: &JsRef, nama: &str, ref_saja: bool) -> Value {
    if let Err(e) = periksa_generasi(h, "js::get") { return e; }
    let mut req = format!("{{\"op\":\"getattr\",\"id\":{}", h.id);
    if ref_saja { req.push_str(",\"ref_saja\":true"); }
    req.push_str(",\"name\":");
    str_json(nama, &mut req);
    req.push('}');
    kirim(&req)
}

pub fn js_method(h: &JsRef, nama: &str, args: Vec<Value>, panggil: pekerja::Panggil<'_>) -> Value {
    js_method_opt(h, nama, args, false, panggil)
}
pub fn js_ref_method(h: &JsRef, nama: &str, args: Vec<Value>, panggil: pekerja::Panggil<'_>) -> Value {
    js_method_opt(h, nama, args, true, panggil)
}

pub fn js_method_opt(
    h: &JsRef, nama: &str, args: Vec<Value>, ref_saja: bool, panggil: pekerja::Panggil<'_>,
) -> Value {
    if let Err(e) = periksa_generasi(h, "js::method") { return e; }
    let id = h.id;
    pekerja::jalankan_dgn_fungsi(&RT, |req| {
        req.push_str(&format!("{{\"op\":\"method\",\"id\":{}", id));
        if ref_saja { req.push_str(",\"ref_saja\":true"); }
        req.push_str(",\"name\":");
        str_json(nama, req);
        rakit_args(&args, req)?;
        req.push('}');
        Ok(())
    }, panggil)
}

pub fn js_item(h: &JsRef, kunci: &Value) -> Value { js_item_opt(h, kunci, false) }
pub fn js_ref_item(h: &JsRef, kunci: &Value) -> Value { js_item_opt(h, kunci, true) }

fn js_item_opt(h: &JsRef, kunci: &Value, ref_saja: bool) -> Value {
    if let Err(e) = periksa_generasi(h, "js::item") { return e; }
    let mut req = format!("{{\"op\":\"item\",\"id\":{}", h.id);
    if ref_saja { req.push_str(",\"ref_saja\":true"); }
    req.push_str(",\"key\":");
    if let Err(e) = pekerja::value_to_json(&RT, kunci, &mut req) {
        return err_value(format!("js::item: kuncinya {}", e));
    }
    req.push('}');
    kirim(&req)
}

pub fn js_set(h: &JsRef, nama: &str, nilai: &Value) -> Value {
    if let Err(e) = periksa_generasi(h, "js::set") { return e; }
    let mut req = format!("{{\"op\":\"setattr\",\"id\":{},\"name\":", h.id);
    str_json(nama, &mut req);
    req.push_str(",\"value\":");
    if let Err(e) = pekerja::value_to_json(&RT, nilai, &mut req) {
        return err_value(format!("js::set: nilainya {}", e));
    }
    req.push('}');
    kirim(&req)
}

pub fn js_set_item(h: &JsRef, kunci: &Value, nilai: &Value) -> Value {
    if let Err(e) = periksa_generasi(h, "js::set_item") { return e; }
    let mut req = format!("{{\"op\":\"setitem\",\"id\":{},\"key\":", h.id);
    if let Err(e) = pekerja::value_to_json(&RT, kunci, &mut req) {
        return err_value(format!("js::set_item: kuncinya {}", e));
    }
    req.push_str(",\"value\":");
    if let Err(e) = pekerja::value_to_json(&RT, nilai, &mut req) {
        return err_value(format!("js::set_item: nilainya {}", e));
    }
    req.push('}');
    kirim(&req)
}

pub fn js_del_item(h: &JsRef, kunci: &Value) -> Value {
    if let Err(e) = periksa_generasi(h, "js::del_item") { return e; }
    let mut req = format!("{{\"op\":\"delitem\",\"id\":{},\"key\":", h.id);
    if let Err(e) = pekerja::value_to_json(&RT, kunci, &mut req) {
        return err_value(format!("js::del_item: kuncinya {}", e));
    }
    req.push('}');
    kirim(&req)
}

pub fn js_has(h: &JsRef, kunci: &Value) -> Value {
    if let Err(e) = periksa_generasi(h, "js::has") { return e; }
    let mut req = format!("{{\"op\":\"contains\",\"id\":{},\"key\":", h.id);
    if let Err(e) = pekerja::value_to_json(&RT, kunci, &mut req) {
        return err_value(format!("js::has: kuncinya {}", e));
    }
    req.push('}');
    kirim(&req)
}

/// Dipakai `js::method` bentuk langsung di atas handle (`h.foo(1)`), lewat
/// jalur yang sama dengan `py::`.
pub fn js_method_cb(
    h: &JsRef, nama: &str, args: Vec<Value>, panggil: pekerja::Panggil<'_>,
) -> Value {
    js_method_opt(h, nama, args, false, panggil)
}
