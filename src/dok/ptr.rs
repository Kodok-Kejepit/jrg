// src/dok/ptr.rs — ptr:: — pointer/referensi sungguhan: sebuah "kotak" yang bisa dibaca,
// ditulis, dan DIBAGIKAN — beda dari semantik value biasa di bahasa ini

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::interpreter::{Interpreter, RuntimeError, Signal, Value};

static NEXT_ID: AtomicU64 = AtomicU64::new(1); // 0 dicadangkan buat null

type Cell = Arc<Mutex<Value>>;

static CELLS: OnceLock<Mutex<HashMap<u64, Cell>>> = OnceLock::new();
fn cells() -> &'static Mutex<HashMap<u64, Cell>> {
    CELLS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn make_ptr(id: u64) -> Value {
    let mut fields = HashMap::new();
    fields.insert("id".to_string(), Value::Int(id as i128));
    Value::Struct { name: "Ptr".to_string(), fields: fields.into() }
}

fn ptr_id(v: &Value, fname: &str) -> Result<u64, String> {
    match v {
        Value::Struct { name, fields } if name == "Ptr" => match fields.get("id") {
            Some(Value::Int(n)) => u64::try_from(*n)
                .map_err(|_| format!("{}: Ptr tidak valid (`id` {} bukan nomor alokasi)", fname, n)),
            _ => Err(format!("{}: Ptr tidak valid (field `id` hilang)", fname)),
        },
        other => Err(format!("{}: butuh Ptr, dapat `{}`", fname, other.type_name())),
    }
}

pub fn ptr_new(args: &[Value]) -> Result<Value, String> {
    let v = args.get(0).cloned().unwrap_or(Value::Nil);
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    cells().lock().unwrap_or_else(|p| p.into_inner()).insert(id, Arc::new(Mutex::new(v)));
    Ok(make_ptr(id))
}

pub fn ptr_null(_args: &[Value]) -> Result<Value, String> {
    Ok(make_ptr(0))
}

pub fn ptr_is_null(args: &[Value]) -> Result<Value, String> {
    let id = ptr_id(args.get(0).unwrap_or(&Value::Nil), "ptr::is_null")?;
    Ok(Value::Bool(id == 0))
}

pub fn ptr_addr(args: &[Value]) -> Result<Value, String> {
    let id = ptr_id(args.get(0).unwrap_or(&Value::Nil), "ptr::addr")?;
    Ok(Value::Int(id as i128))
}

thread_local! {
    /// Pointer yang sedang di tengah `ptr::update` di thread ini (selnya terkunci).
    static SIBUK: std::cell::RefCell<Vec<u64>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Menandai sebuah pointer sibuk selama `ptr::update` memanggil fungsinya.
pub struct PenandaSibuk(u64);
impl PenandaSibuk {
    pub fn baru(id: u64) -> Self { SIBUK.with(|s| s.borrow_mut().push(id)); PenandaSibuk(id) }
}
impl Drop for PenandaSibuk {
    fn drop(&mut self) {
        SIBUK.with(|s| { let mut s = s.borrow_mut(); if let Some(i) = s.iter().rposition(|x| *x == self.0) { s.remove(i); } });
    }
}

/// Fungsi pembaruan yang menyentuh pointernya sendiri dulu MENGGANTUNG (selnya terkunci
/// thread yang sama). Sekarang ditolak, sambil menunjuk nilai yang sudah ada di tangannya.
pub fn periksa_sibuk(id: u64, fname: &str) -> Result<(), String> {
    if SIBUK.with(|s| s.borrow().contains(&id)) {
        return Err(format!("{}: pointer ini sedang diperbarui `ptr::update` — fungsi pembaruannya \
            sudah menerima nilai lamanya sebagai argumen, pakai itu", fname));
    }
    Ok(())
}

fn get_cell(id: u64) -> Option<Cell> {
    cells().lock().unwrap_or_else(|p| p.into_inner()).get(&id).cloned()
}

pub fn ptr_get(args: &[Value]) -> Result<Value, String> {
    let id = ptr_id(args.get(0).unwrap_or(&Value::Nil), "ptr::get")?;
    if id == 0 { return Err("ptr::get: null pointer dereference".to_string()); }
    periksa_sibuk(id, "ptr::get")?;
    match get_cell(id) {
        Some(cell) => Ok(cell.lock().unwrap_or_else(|p| p.into_inner()).clone()),
        None => Err("ptr::get: pointer tidak valid (sudah di-free?)".to_string()),
    }
}

pub fn ptr_set(args: &[Value]) -> Result<Value, String> {
    let id = ptr_id(args.get(0).unwrap_or(&Value::Nil), "ptr::set")?;
    if id == 0 { return Err("ptr::set: null pointer dereference".to_string()); }
    periksa_sibuk(id, "ptr::set")?;
    let v = args.get(1).cloned().unwrap_or(Value::Nil);
    match get_cell(id) {
        Some(cell) => { *cell.lock().unwrap_or_else(|p| p.into_inner()) = v; Ok(Value::Unit) }
        None => Err("ptr::set: pointer tidak valid (sudah di-free?)".to_string()),
    }
}

/// Sugar buat pola paling umum (`ptr::update(p, |n: int| n + 1)`) tanpa perlu
/// nulis closure — cukup `ptr::inc(p)`.
fn ptr_step(args: &[Value], fname: &str, delta: i128) -> Result<Value, String> {
    let id = ptr_id(args.get(0).unwrap_or(&Value::Nil), fname)?;
    if id == 0 { return Err(format!("{}: null pointer dereference", fname)); }
    periksa_sibuk(id, fname)?;
    match get_cell(id) {
        Some(cell) => {
            let mut guard = cell.lock().unwrap_or_else(|p| p.into_inner());
            let new_val = match &*guard {
                Value::Int(n) => Value::Int(n.checked_add(delta).ok_or_else(|| format!(
                    "{}: {}", fname, crate::interpreter::pesan_luber("+", *n, delta)))?),
                Value::Float(f) => Value::Float(f + delta as f64),
                other => return Err(format!("{}: isi Ptr bukan angka (dapat `{}`)", fname, other.type_name())),
            };
            *guard = new_val.clone();
            Ok(new_val)
        }
        None => Err(format!("{}: pointer tidak valid (sudah di-free?)", fname)),
    }
}
pub fn ptr_inc(args: &[Value]) -> Result<Value, String> { ptr_step(args, "ptr::inc", 1) }
pub fn ptr_dec(args: &[Value]) -> Result<Value, String> { ptr_step(args, "ptr::dec", -1) }

pub fn ptr_free(args: &[Value]) -> Result<Value, String> {
    let id = ptr_id(args.get(0).unwrap_or(&Value::Nil), "ptr::free")?;
    cells().lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
    Ok(Value::Unit)
}

/// Baca-ubah-simpan atomik: mengunci sel selama seluruh operasi (termasuk
/// selama memanggil `fn` milik user) supaya tidak ada goroutine lain yang …
pub fn ptr_update(interp: &mut Interpreter, args: &[Value]) -> Result<Value, Signal> {
    let id = ptr_id(args.get(0).unwrap_or(&Value::Nil), "ptr::update")
        .map_err(|e| Signal::RuntimeError(RuntimeError::new(e)))?;
    if id == 0 { return Err(Signal::RuntimeError(RuntimeError::new("ptr::update: null pointer dereference"))); }
    periksa_sibuk(id, "ptr::update").map_err(|e| Signal::RuntimeError(RuntimeError::new(e)))?;
    // Yang bukan fungsi ditolak pemanggilannya sendiri, dengan kalimat yang sama di `--vm`.
    let f = args.get(1).cloned().unwrap_or(Value::Nil);
    let cell = match get_cell(id) {
        Some(c) => c,
        None => return Err(Signal::RuntimeError(RuntimeError::new("ptr::update: pointer tidak valid (sudah di-free?)"))),
    };
    let mut guard = cell.lock().unwrap_or_else(|p| p.into_inner());
    let old = guard.clone();
    let _sibuk = PenandaSibuk::baru(id);
    let new_val = interp.call_fn(f, vec![old])?;
    *guard = new_val.clone();
    Ok(new_val)
}
