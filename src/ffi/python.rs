// src/ffi/python.rs
// Python FFI bridge: a persistent `python3` worker subprocess talked to

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Mutex, OnceLock};

use crate::interpreter::{err_value, ok_value, Value};

// ---------------------------------------------------------------------------
// Runtime Python …

use super::pekerja::{self, Runtime, Worker};

fn slot() -> &'static Mutex<Option<Worker>> {
    static SLOT: OnceLock<Mutex<Option<Worker>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

static GENERASI: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn antrean_lepas() -> &'static Mutex<Vec<u64>> {
    static Q: OnceLock<Mutex<Vec<u64>>> = OnceLock::new();
    Q.get_or_init(|| Mutex::new(Vec::new()))
}

pub static RT: Runtime = Runtime {
    nama: "python",
    ns: "py",
    bin: "python3",
    // `-u`: tanpa buffer, supaya responsnya sampai tanpa menunggu buffer penuh.
    bin_args: &["-u"],
    skrip_src: include_str!("py_runner.py"),
    skrip_nama: "jrg_py_runner.py",
    bantuan_pasang: "pastikan python3 ada di PATH — di Termux: pkg install python",
    slot,
    generasi: &GENERASI,
    antrean: antrean_lepas,
    registri: registri_fungsi,
    fungsi_boleh_get: || FUNGSI_BOLEH.with(|b| b.get()),
    fungsi_boleh_set: |v| FUNGSI_BOLEH.with(|b| b.set(v)),
    dalam_cb_get: || DALAM_CALLBACK.with(|c| c.get()),
    dalam_cb_set: |v| DALAM_CALLBACK.with(|c| c.set(v)),
    buat_handle: |id, generasi, tipe| {
        ok_value(Value::PyObj(std::sync::Arc::new(PyObjInner { id, generasi, tipe })))
    },
    tulis_handle: |v, out| match v {
        Value::PyObj(h) => Some(tulis_handle_py(h, out)),
        _ => None,
    },
};

fn generasi_kini() -> u64 { RT.generasi_kini() }

/// Tulis sebuah handle Python ke JSON sebagai nomornya.
fn tulis_handle_py(h: &PyRef, out: &mut String) -> Result<(), String> {
    if h.generasi != RT.generasi_kini() {
        return Err(format!(
            "berupa handle Python dari sesi worker sebelumnya (sesi #{}, sekarang #{}) — \
             objeknya sudah hilang bersama proses lamanya",
            h.generasi, RT.generasi_kini()));
    }
    out.push_str(&format!("{{\"__pyref__\":{}}}", h.id));
    Ok(())
}

fn send_request(req_json: &str) -> Value { pekerja::kirim_tanpa_fungsi(&RT, req_json) }

fn send_request_cb(req_json: &str, panggil: Option<pekerja::Panggil<'_>>) -> Value {
    pekerja::kirim(&RT, req_json, panggil)
}

fn jalankan_dgn_fungsi(
    bangun: impl FnOnce(&mut String) -> Result<(), String>,
    panggil: Panggil<'_>,
) -> Value {
    pekerja::jalankan_dgn_fungsi(&RT, bangun, panggil)
}

pub type Panggil<'a> = pekerja::Panggil<'a>;

fn value_to_json(v: &Value, out: &mut String) -> Result<(), String> {
    pekerja::value_to_json(&RT, v, out)
}

fn escape_json_string(s: &str, out: &mut String) { pekerja::escape_json_string(s, out) }

/// Isi sebuah handle. Dipegang bersama lewat `Arc` (lihat `PyRef`).
#[derive(Debug)]
pub struct PyObjInner {
    pub id: u64,
    pub generasi: u64,
    /// Nama tipe Python-nya, dipakai `typeof` dan pesan error supaya
    /// handle tidak tampil sebagai angka tanpa arti.
    pub tipe: String,
}

impl Drop for PyObjInner {
    fn drop(&mut self) {
        // Objek dari sesi worker yang sudah mati tidak perlu dikabarkan —
        // prosesnya sudah membawa serta seluruh tabelnya.
        if self.generasi != generasi_kini() {
            return;
        }
        RT.antre_lepas(self.id);
    }
}

/// Handle ke objek yang tinggal di worker Python.
pub type PyRef = std::sync::Arc<PyObjInner>;

const RUNNER_SRC: &str = include_str!("py_runner.py");

// ---------------------------------------------------------------------------
// Public entry points — dipanggil dari NativeFnKind::Py* di interpreter/mod.rs
// ---------------------------------------------------------------------------

/// `py::eval(code)` — evaluates a Python *expression* string and returns its
/// value.
pub fn py_eval(code: &str, panggil: Panggil<'_>) -> Value {
    let mut req = String::from("{\"op\":\"eval\",\"code\":");
    escape_json_string(code, &mut req);
    req.push('}');
    send_request_cb(&req, Some(panggil))
}

/// Seperti `py_eval`, tapi hasilnya SELALU tinggal di Python sebagai handle —
/// walau sebenarnya bisa dikonversi.
pub fn py_ref(code: &str, panggil: Panggil<'_>) -> Value {
    let mut req = String::from("{\"op\":\"eval\",\"ref_saja\":true,\"code\":");
    escape_json_string(code, &mut req);
    req.push('}');
    send_request_cb(&req, Some(panggil))
}

/// Baca atribut sebuah handle: `h.nama`.
pub fn py_getattr(h: &PyRef, nama: &str, panggil: Panggil<'_>) -> Value { py_getattr_opt(h, nama, false, panggil) }

/// Sama, tapi `ref_saja` memaksa hasilnya tetap jadi handle.
pub fn py_getattr_opt(h: &PyRef, nama: &str, ref_saja: bool, panggil: Panggil<'_>) -> Value {
    if let Err(e) = periksa_generasi(h, "py::get") { return e; }
    let mut req = format!("{{\"op\":\"getattr\",\"ref_saja\":{},\"ref\":{},\"name\":", ref_saja, h.id);
    escape_json_string(nama, &mut req);
    req.push('}');
    send_request_cb(&req, Some(panggil))
}

/// Panggil method sebuah handle: `h.nama(args...)`.
pub fn py_method(h: &PyRef, nama: &str, args: Vec<Value>, panggil: Panggil<'_>) -> Value { py_method_opt(h, nama, args, false, panggil) }

/// Sama, tapi `ref_saja` memaksa hasilnya tetap jadi handle.
pub fn py_method_opt(h: &PyRef, nama: &str, args: Vec<Value>, ref_saja: bool, panggil: Panggil<'_>) -> Value {
    if let Err(e) = periksa_generasi(h, "py::method") { return e; }
    let mut req = format!("{{\"op\":\"method\",\"ref_saja\":{},\"ref\":{},\"name\":", ref_saja, h.id);
    escape_json_string(nama, &mut req);
    req.push_str(",\"args\":[");
    for (i, a) in args.iter().enumerate() {
        if i > 0 { req.push(','); }
        if let Err(e) = value_to_json(a, &mut req) {
            return err_value(format!("py::method `{}`: {} {}", nama, pekerja::argumen_diteruskan(i, "Python"), e));
        }
    }
    req.push_str("]}");
    send_request_cb(&req, Some(panggil))
}

/// Indexing: `h[kunci]` — `df["kolom"]`, `arr[3]`, `d["k"]`.
pub fn py_item(h: &PyRef, kunci: &Value, panggil: Panggil<'_>) -> Value { py_item_opt(h, kunci, false, panggil) }

/// Sama, tapi `ref_saja` memaksa hasilnya tetap jadi handle.
pub fn py_item_opt(h: &PyRef, kunci: &Value, ref_saja: bool, panggil: Panggil<'_>) -> Value {
    if let Err(e) = periksa_generasi(h, "py::item") { return e; }
    let mut req = format!("{{\"op\":\"item\",\"ref_saja\":{},\"ref\":{},\"key\":", ref_saja, h.id);
    if let Err(e) = value_to_json(kunci, &mut req) {
        return err_value(format!("py::item: kuncinya {}", e));
    }
    req.push('}');
    send_request_cb(&req, Some(panggil))
}

/// Paksa objeknya menyeberang jadi nilai JRG biasa.
pub fn py_to_jrg(h: &PyRef, panggil: Panggil<'_>) -> Value {
    if let Err(e) = periksa_generasi(h, "py::to_jrg") { return e; }
    send_request_cb2(&format!("{{\"op\":\"to_jrg\",\"ref\":{}}}", h.id), panggil)
}

/// Berapa objek yang masih ditahan worker.
pub fn py_refs(panggil: Panggil<'_>) -> Value {
    send_request("{\"op\":\"ping\"}")
}

/// `repr()` Python-nya sebagai string — untuk dilihat manusia, bukan
/// untuk dihitung.
pub fn py_repr(h: &PyRef, panggil: Panggil<'_>) -> Value {
    if let Err(e) = periksa_generasi(h, "py::repr") { return e; }
    send_request_cb2(&format!("{{\"op\":\"repr\",\"ref\":{}}}", h.id), panggil)
}

/// `py::exec(code)` — runs a Python *statement(s)* string (assignments,
/// imports, `def`, loops, ...) for its side effects. Returns
/// `Ok(nil)` on success.
pub fn py_exec(code: &str, panggil: Panggil<'_>) -> Value {
    let mut req = String::from("{\"op\":\"exec\",\"code\":");
    escape_json_string(code, &mut req);
    req.push('}');
    send_request_cb(&req, Some(panggil))
}

/// `py::import(module)` — pre-imports a module so a missing package fails
/// right where the user can see the intent, instead of failing later inside …
pub fn py_import(module: &str, panggil: Panggil<'_>) -> Value {
    let mut req = String::from("{\"op\":\"import\",\"module\":");
    escape_json_string(module, &mut req);
    req.push('}');
    send_request_cb(&req, Some(panggil))
}

/// `py::call(module, func_path, arg1, arg2, ...)` — imports `module` (cached
/// after the first call), resolves `func_path` as a dot-separated attribute …
pub fn py_call(module: &str, func: &str, args: Vec<Value>, panggil: Panggil<'_>) -> Value {
    py_call_opt(module, func, args, false, panggil)
}

/// `py::call` yang boleh membawa fungsi JRG di argumennya.
pub fn py_call_cb(
    module: &str, func: &str, args: Vec<Value>, ref_saja: bool,
    panggil: &mut dyn FnMut(&Value, Vec<Value>) -> Result<Value, String>,
) -> Value {
    jalankan_dgn_fungsi(|req| rakit_call(module, func, &args, ref_saja, req), panggil)
}

/// `py::call_kw` yang boleh membawa fungsi JRG di argumen maupun kwargs.
pub fn py_call_kw_cb(
    module: &str, func: &str, args: Vec<Value>, kw: Vec<Value>, ref_saja: bool,
    panggil: &mut dyn FnMut(&Value, Vec<Value>) -> Result<Value, String>,
) -> Value {
    jalankan_dgn_fungsi(|req| {
        rakit_call(module, func, &args, ref_saja, req)?;
        // Tutup kurung `}` yang dipasang `rakit_call` dilepas dulu, karena
        // kwargs masih harus masuk ke objek yang sama.
        req.pop();
        rakit_kwargs("py::call_kw", &kw, req)?;
        req.push('}');
        Ok(())
    }, panggil)
}

/// `py::method` / `py::method_kw` yang boleh membawa fungsi JRG.
pub fn py_method_cb(
    h: &PyRef, nama: &str, args: Vec<Value>, kw: Vec<Value>, ref_saja: bool,
    panggil: &mut dyn FnMut(&Value, Vec<Value>) -> Result<Value, String>,
) -> Value {
    if let Err(e) = periksa_generasi(h, "py::method") { return e; }
    jalankan_dgn_fungsi(|req| {
        rakit_method(h, nama, &args, ref_saja, req)?;
        req.pop();
        rakit_kwargs("py::method_kw", &kw, req)?;
        req.push('}');
        Ok(())
    }, panggil)
}

/// Perakit bersama untuk `call`. Dipisah dari pengirimannya supaya jalur
/// biasa dan jalur callback memakai bentuk permintaan yang SAMA.
fn rakit_call(module: &str, func: &str, args: &[Value], ref_saja: bool, req: &mut String) -> Result<(), String> {
    req.push_str(&format!("{{\"op\":\"call\",\"ref_saja\":{},\"module\":", ref_saja));
    escape_json_string(module, req);
    req.push_str(",\"func\":");
    escape_json_string(func, req);
    req.push_str(",\"args\":[");
    for (i, a) in args.iter().enumerate() {
        if i > 0 { req.push(','); }
        value_to_json(a, req).map_err(|e| format!("py::call: {} {}", pekerja::argumen_diteruskan(i, "Python"), e))?;
    }
    req.push_str("]}");
    Ok(())
}

fn rakit_method(h: &PyRef, nama: &str, args: &[Value], ref_saja: bool, req: &mut String) -> Result<(), String> {
    req.push_str(&format!("{{\"op\":\"method\",\"ref_saja\":{},\"ref\":{},\"name\":", ref_saja, h.id));
    escape_json_string(nama, req);
    req.push_str(",\"args\":[");
    for (i, a) in args.iter().enumerate() {
        if i > 0 { req.push(','); }
        value_to_json(a, req).map_err(|e| format!("py::method `{}`: {} {}", nama, pekerja::argumen_diteruskan(i, "Python"), e))?;
    }
    req.push_str("]}");
    Ok(())
}

/// Sama, tapi `ref_saja` memaksa hasilnya tetap jadi handle.
pub fn py_call_opt(module: &str, func: &str, args: Vec<Value>, ref_saja: bool, panggil: Panggil<'_>) -> Value {
    let mut req = format!("{{\"op\":\"call\",\"ref_saja\":{},\"module\":", ref_saja);
    escape_json_string(module, &mut req);
    req.push_str(",\"func\":");
    escape_json_string(func, &mut req);
    req.push_str(",\"args\":[");
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            req.push(',');
        }
        if let Err(e) = value_to_json(a, &mut req) {
            return err_value(format!("py::call: {} {}", pekerja::argumen_diteruskan(i, "Python"), e));
        }
    }
    req.push_str("]}");
    send_request_cb(&req, Some(panggil))
}

/// `py::call_kw(module, func_path, args_array, kwargs_pairs)` — like
/// `py_call`, but for Python calls that need keyword arguments too …
pub fn py_call_kw(module: &str, func: &str, args: Vec<Value>, kwarg_pairs: Vec<Value>, panggil: Panggil<'_>) -> Value {
    let mut req = String::from("{\"op\":\"call\",\"module\":");
    escape_json_string(module, &mut req);
    req.push_str(",\"func\":");
    escape_json_string(func, &mut req);
    req.push_str(",\"args\":[");
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            req.push(',');
        }
        if let Err(e) = value_to_json(a, &mut req) {
            return err_value(format!("py::call_kw: args ke-{} {}", i + 1, e));
        }
    }
    req.push(']');
    if let Err(e) = rakit_kwargs("py::call_kw", &kwarg_pairs, &mut req) {
        return err_value(e);
    }
    req.push('}');
    send_request_cb(&req, Some(panggil))
}

/// Rakit blok `"kwargs":{...}` dari daftar pasangan `[nama, nilai]`.
fn rakit_kwargs(siapa: &str, pairs: &[Value], req: &mut String) -> Result<(), String> {
    req.push_str(",\"kwargs\":{");
    for (i, pair) in pairs.iter().enumerate() {
        let elems: &[Value] = match pair {
            Value::Array(p) | Value::Tuple(p) => p.as_slice(),
            other => return Err(format!(
                "{}: kwargs ke-{} harus pasangan [nama, nilai], dapat `{}`", siapa, i + 1, other
            )),
        };
        let (key, val) = match elems {
            [k, v] => (k, v),
            _ => return Err(format!(
                "{}: kwargs ke-{} harus persis 2 elemen [nama, nilai], dapat {} elemen",
                siapa, i + 1, elems.len()
            )),
        };
        let key_str = match key {
            Value::Str(s) => s.clone(),
            other => return Err(format!(
                "{}: nama kwarg ke-{} harus string, dapat `{}`", siapa, i + 1, other
            )),
        };
        if i > 0 { req.push(','); }
        escape_json_string(&key_str, req);
        req.push(':');
        if let Err(e) = value_to_json(val, req) {
            return Err(format!("{}: kwarg \"{}\" {}", siapa, key_str, e));
        }
    }
    req.push('}');
    Ok(())
}

/// `h.nama(args..., **kwargs)` — metode DENGAN keyword argument.
pub fn py_method_kw(
    h: &PyRef, nama: &str, args: Vec<Value>, kwargs: Vec<Value>, ref_saja: bool,
    panggil: Panggil<'_>,
) -> Value {
    if let Err(e) = periksa_generasi(h, "py::method_kw") { return e; }
    let mut req = format!("{{\"op\":\"method\",\"ref_saja\":{},\"ref\":{},\"name\":", ref_saja, h.id);
    escape_json_string(nama, &mut req);
    req.push_str(",\"args\":[");
    for (i, a) in args.iter().enumerate() {
        if i > 0 { req.push(','); }
        if let Err(e) = value_to_json(a, &mut req) {
            return err_value(format!("py::method_kw `{}`: {} {}", nama, pekerja::argumen_diteruskan(i, "Python"), e));
        }
    }
    req.push(']');
    if let Err(e) = rakit_kwargs("py::method_kw", &kwargs, &mut req) {
        return err_value(e);
    }
    req.push('}');
    send_request_cb(&req, Some(panggil))
}

/// Operasi satu-handle yang tidak membawa nilai: `len`, `dir`.
fn op_handle(h: &PyRef, op: &str, siapa: &str, panggil: Panggil<'_>) -> Value {
    if let Err(e) = periksa_generasi(h, siapa) { return e; }
    send_request_cb2(&format!("{{\"op\":\"{}\",\"ref\":{}}}", op, h.id), panggil)
}

/// `len(h)` di sisi Python — TANPA menyeberangkan isinya.
pub fn py_len(h: &PyRef, panggil: Panggil<'_>) -> Value { op_handle(h, "len", "py::len", panggil) }

/// Nama atribut dan metode yang dimiliki objek ini.
pub fn py_dir(h: &PyRef, panggil: Panggil<'_>) -> Value { op_handle(h, "dir", "py::dir", panggil) }

/// `h.nama = nilai`.
pub fn py_setattr(h: &PyRef, nama: &str, nilai: &Value, panggil: Panggil<'_>) -> Value {
    if let Err(e) = periksa_generasi(h, "py::set") { return e; }
    let mut req = format!("{{\"op\":\"setattr\",\"ref\":{},\"name\":", h.id);
    escape_json_string(nama, &mut req);
    req.push_str(",\"value\":");
    if let Err(e) = value_to_json(nilai, &mut req) {
        return err_value(format!("py::set: nilainya {}", e));
    }
    req.push('}');
    send_request_cb(&req, Some(panggil))
}

/// `h[kunci] = nilai`.
pub fn py_setitem(h: &PyRef, kunci: &Value, nilai: &Value, panggil: Panggil<'_>) -> Value {
    if let Err(e) = periksa_generasi(h, "py::set_item") { return e; }
    let mut req = format!("{{\"op\":\"setitem\",\"ref\":{},\"key\":", h.id);
    if let Err(e) = value_to_json(kunci, &mut req) {
        return err_value(format!("py::set_item: kuncinya {}", e));
    }
    req.push_str(",\"value\":");
    if let Err(e) = value_to_json(nilai, &mut req) {
        return err_value(format!("py::set_item: nilainya {}", e));
    }
    req.push('}');
    send_request_cb(&req, Some(panggil))
}

/// `del h[kunci]`.
pub fn py_delitem(h: &PyRef, kunci: &Value, panggil: Panggil<'_>) -> Value {
    if let Err(e) = periksa_generasi(h, "py::del_item") { return e; }
    let mut req = format!("{{\"op\":\"delitem\",\"ref\":{},\"key\":", h.id);
    if let Err(e) = value_to_json(kunci, &mut req) {
        return err_value(format!("py::del_item: kuncinya {}", e));
    }
    req.push('}');
    send_request_cb(&req, Some(panggil))
}

/// `kunci in h`.
pub fn py_contains(h: &PyRef, kunci: &Value, panggil: Panggil<'_>) -> Value {
    if let Err(e) = periksa_generasi(h, "py::has") { return e; }
    let mut req = format!("{{\"op\":\"contains\",\"ref\":{},\"key\":", h.id);
    if let Err(e) = value_to_json(kunci, &mut req) {
        return err_value(format!("py::has: kuncinya {}", e));
    }
    req.push('}');
    send_request_cb(&req, Some(panggil))
}

/// Bentuk pendek untuk permintaan yang dirakit langsung lewat `format!`.
fn send_request_cb2(req: &str, panggil: Panggil<'_>) -> Value {
    send_request_cb(req, Some(panggil))
}


/// Serializes a `Value` as JSON text, appended into `out`.

fn other_fn(v: &Value) -> Value { v.clone() }

/// Fungsi JRG yang pernah diserahkan ke Python, hidup SELAMA SESI.
fn registri_fungsi() -> &'static std::sync::Mutex<pekerja::RegistriFungsi> {
    static R: std::sync::OnceLock<std::sync::Mutex<pekerja::RegistriFungsi>> = std::sync::OnceLock::new();
    R.get_or_init(Default::default)
}

thread_local! {
    static FUNGSI_BOLEH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };

    /// Sedang berada DI DALAM sebuah callback dari Python.
    static DALAM_CALLBACK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn daftar_fungsi_kini(f: Value) -> Option<usize> { RT.daftar_fungsi(f) }

// ---------------------------------------------------------------------------
// JSON -> Value (untuk membaca respons dari Python)
// ---------------------------------------------------------------------------


/// Pastikan sebuah handle masih milik sesi worker yang sekarang.
fn periksa_generasi(h: &PyRef, fname: &str) -> Result<(), Value> {
    RT.periksa_generasi(h.generasi, fname)
}
