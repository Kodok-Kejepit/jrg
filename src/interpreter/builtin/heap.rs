// src/interpreter/builtin/heap.rs — interpreter/builtin/heap.rs
// jawa:: / jawir:: — manajemen memori manual ala C/C++, dikasih ke

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::interpreter::Value;
use super::NativeFnKind;
use crate::interpreter::{EvalResult, Signal, RuntimeError};

// ─── Registry: tiga jenis alokasi, satu ruang id bersama ────────────────

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
fn next_id() -> u64 { NEXT_ID.fetch_add(1, Ordering::SeqCst) }

static TOTAL_ALLOCATED: AtomicU64 = AtomicU64::new(0);
static TOTAL_FREED: AtomicU64 = AtomicU64::new(0);

type SingleCell = Arc<Mutex<Value>>;
type ArrayCell = Arc<Mutex<Vec<Value>>>;
type BufferCell = Arc<Mutex<Vec<u8>>>;

static SINGLES: OnceLock<Mutex<HashMap<u64, SingleCell>>> = OnceLock::new();
fn singles() -> &'static Mutex<HashMap<u64, SingleCell>> { SINGLES.get_or_init(|| Mutex::new(HashMap::new())) }

static ARRAYS: OnceLock<Mutex<HashMap<u64, ArrayCell>>> = OnceLock::new();
fn arrays() -> &'static Mutex<HashMap<u64, ArrayCell>> { ARRAYS.get_or_init(|| Mutex::new(HashMap::new())) }

static BUFFERS: OnceLock<Mutex<HashMap<u64, BufferCell>>> = OnceLock::new();
fn buffers() -> &'static Mutex<HashMap<u64, BufferCell>> { BUFFERS.get_or_init(|| Mutex::new(HashMap::new())) }

fn make_handle(kind: &str, id: u64) -> Value {
    let mut fields = HashMap::new();
    fields.insert("id".to_string(), Value::Int(id as i128));
    fields.insert("kind".to_string(), Value::Str(kind.to_string()));
    Value::Struct { name: "Ptr".to_string(), fields: fields.into() }
}

fn handle_parts(v: &Value, fname: &str) -> Result<(u64, String), String> {
    match v {
        Value::Struct { name, fields } if name == "Ptr" => {
            let id = match fields.get("id") { Some(Value::Int(n)) => u64::try_from(*n).map_err(|_| format!("{}: handle tidak valid (`id` {} bukan nomor alokasi)", fname, n))?, _ => return Err(format!("{}: handle tidak valid (field `id` hilang)", fname)) };
            let kind = match fields.get("kind") { Some(Value::Str(s)) => s.clone(), _ => return Err(format!("{}: handle tidak valid (field `kind` hilang)", fname)) };
            Ok((id, kind))
        }
        other => Err(pesan_bukan_ptr(fname, &other.type_name())),
    }
}

fn kind_alloc_fn_name(kind: &str) -> &'static str {
    match kind { "single" => "jawa::alloc", "array" => "jawa::array", "buffer" => "jawa::buffer", _ => "jawa::?" }
}

fn expect_kind(v: &Value, want: &str, fname: &str) -> Result<u64, String> {
    let (id, kind) = handle_parts(v, fname)?;
    if kind != want {
        return Err(pesan_jenis_salah(fname, &kind, want));
    }
    Ok(id)
}

// ─── Kalimat dan aturan bersama ─────────────────────────────────────────
// Registri tiap mesin memang terpisah (nilai VM tidak bisa disimpan di sini), tapi
// urutan pemeriksaan dan kalimatnya satu: `stdlib::jawa` memanggil fungsi-fungsi ini.

pub(crate) const MAKS_ALOKASI: i128 = 256 * 1024 * 1024;

pub(crate) fn pesan_bukan_ptr(f: &str, tipe: &str) -> String {
    format!("{}: butuh Ptr (dari jawa::alloc/array/buffer), dapat `{}`", f, tipe)
}

pub(crate) fn pesan_jenis_salah(f: &str, punya: &str, mau: &str) -> String {
    format!("{}: handle ini hasil `{}(...)`, bukan `{}(...)` — pakai fungsi yang sesuai buat jenis ini",
            f, kind_alloc_fn_name(punya), kind_alloc_fn_name(mau))
}

pub(crate) fn pesan_sudah_bebas(f: &str, jenis: &str) -> String {
    let apa = match jenis { "array" => "array", "buffer" => "buffer", _ => "pointer" };
    format!("{}: use-after-free — {} ini sudah di-`jawir::free`", f, apa)
}

pub(crate) const PESAN_DOUBLE_FREE: &str =
    "jawir::free: double free — pointer ini sudah pernah di-`jawir::free` sebelumnya";

pub(crate) fn pesan_byte_luar(f: &str, n: i128) -> String {
    format!("{}: nilai {} di luar jangkauan byte (0..255)", f, n)
}

/// Panjang alokasi baru, dibatasi SEBELUM satu byte pun dialokasikan: tanpa batas,
/// `jawa::buffer(10^12)` di `--vm` dulu mematikan prosesnya.
pub(crate) fn cek_panjang(f: &str, n: i128) -> Result<usize, String> {
    if n < 0 { return Err(crate::interpreter::pesan_ukuran_tak_masuk_akal(f, "panjang", n)); }
    if n > MAKS_ALOKASI { return Err(format!("{}: n={} kelewat besar (maks {})", f, n, MAKS_ALOKASI)); }
    Ok(n as usize)
}

/// Index ke dalam alokasi: di luar `0..panjang` ditolak dengan kalimat `arr::`.
pub(crate) fn cek_index(f: &str, i: i128, panjang: usize) -> Result<usize, String> {
    if i < 0 || i >= panjang as i128 {
        return Err(crate::interpreter::pesan_index_luar_batas(f, i, panjang));
    }
    Ok(i as usize)
}

fn bulat(v: Option<&Value>, f: &str) -> Result<i128, String> {
    match v {
        Some(Value::Int(n)) => Ok(*n),
        Some(Value::Float(x)) => Err(crate::interpreter::pesan_bukan_bulat(
            f, "float", Some(&Value::Float(*x).to_string()))),
        Some(o) => Err(crate::interpreter::pesan_bukan_bulat(f, &o.type_name(), None)),
        None => Err(format!("{}: kurang argumen", f)),
    }
}

fn byte_dari(v: Option<&Value>, f: &str) -> Result<u8, String> {
    let n = bulat(v, f)?;
    if !(0..=255).contains(&n) { return Err(pesan_byte_luar(f, n)); }
    Ok(n as u8)
}

pub fn new_alloc(args: &[Value]) -> Result<Value, String> {
    let v = args.get(0).cloned().unwrap_or(Value::Nil);
    let id = next_id();
    singles().lock().unwrap_or_else(|p| p.into_inner()).insert(id, Arc::new(Mutex::new(v)));
    TOTAL_ALLOCATED.fetch_add(1, Ordering::Relaxed);
    Ok(make_handle("single", id))
}

pub fn new_get(args: &[Value]) -> Result<Value, String> {
    let id = expect_kind(args.get(0).unwrap_or(&Value::Nil), "single", "jawa::get")?;
    match singles().lock().unwrap_or_else(|p| p.into_inner()).get(&id) {
        Some(cell) => Ok(cell.lock().unwrap_or_else(|p| p.into_inner()).clone()),
        None => Err(pesan_sudah_bebas("jawa::get", "single")),
    }
}

pub fn new_set(args: &[Value]) -> Result<Value, String> {
    let id = expect_kind(args.get(0).unwrap_or(&Value::Nil), "single", "jawa::set")?;
    let v = args.get(1).cloned().unwrap_or(Value::Nil);
    match singles().lock().unwrap_or_else(|p| p.into_inner()).get(&id) {
        Some(cell) => { *cell.lock().unwrap_or_else(|p| p.into_inner()) = v; Ok(Value::Unit) }
        None => Err(pesan_sudah_bebas("jawa::set", "single")),
    }
}

pub fn new_array(args: &[Value]) -> Result<Value, String> {
    let n = cek_panjang("jawa::array", bulat(args.get(0), "jawa::array")?)?;
    let fill = args.get(1).cloned().unwrap_or(Value::Nil);
    let id = next_id();
    arrays().lock().unwrap_or_else(|p| p.into_inner()).insert(id, Arc::new(Mutex::new(vec![fill; n])));
    TOTAL_ALLOCATED.fetch_add(1, Ordering::Relaxed);
    Ok(make_handle("array", id))
}

fn get_array_cell(id: u64) -> Option<ArrayCell> {
    arrays().lock().unwrap_or_else(|p| p.into_inner()).get(&id).cloned()
}

pub fn new_array_get(args: &[Value]) -> Result<Value, String> {
    const F: &str = "jawa::array_get";
    let id = expect_kind(args.get(0).unwrap_or(&Value::Nil), "array", F)?;
    let i = bulat(args.get(1), F)?;
    let cell = get_array_cell(id).ok_or_else(|| pesan_sudah_bebas(F, "array"))?;
    let guard = cell.lock().unwrap_or_else(|p| p.into_inner());
    Ok(guard[cek_index(F, i, guard.len())?].clone())
}

pub fn new_array_set(args: &[Value]) -> Result<Value, String> {
    const F: &str = "jawa::array_set";
    let id = expect_kind(args.get(0).unwrap_or(&Value::Nil), "array", F)?;
    let i = bulat(args.get(1), F)?;
    let v = args.get(2).cloned().unwrap_or(Value::Nil);
    let cell = get_array_cell(id).ok_or_else(|| pesan_sudah_bebas(F, "array"))?;
    let mut guard = cell.lock().unwrap_or_else(|p| p.into_inner());
    let k = cek_index(F, i, guard.len())?;
    guard[k] = v;
    Ok(Value::Unit)
}

pub fn new_array_push(args: &[Value]) -> Result<Value, String> {
    let id = expect_kind(args.get(0).unwrap_or(&Value::Nil), "array", "jawa::array_push")?;
    let v = args.get(1).cloned().unwrap_or(Value::Nil);
    match get_array_cell(id) {
        Some(cell) => { cell.lock().unwrap_or_else(|p| p.into_inner()).push(v); Ok(Value::Unit) }
        None => Err(pesan_sudah_bebas("jawa::array_push", "array")),
    }
}

pub fn new_array_pop(args: &[Value]) -> Result<Value, String> {
    let id = expect_kind(args.get(0).unwrap_or(&Value::Nil), "array", "jawa::array_pop")?;
    match get_array_cell(id) {
        // Array kosong DITOLAK: dulu `nil` di sini dan galat di `--vm`.
        Some(cell) => cell.lock().unwrap_or_else(|p| p.into_inner()).pop()
            .ok_or_else(|| "jawa::array_pop: array kosong".to_string()),
        None => Err(pesan_sudah_bebas("jawa::array_pop", "array")),
    }
}

pub fn new_array_len(args: &[Value]) -> Result<Value, String> {
    let id = expect_kind(args.get(0).unwrap_or(&Value::Nil), "array", "jawa::array_len")?;
    match get_array_cell(id) {
        Some(cell) => Ok(Value::Int(cell.lock().unwrap_or_else(|p| p.into_inner()).len() as i128)),
        None => Err(pesan_sudah_bebas("jawa::array_len", "array")),
    }
}

pub fn new_array_to_list(args: &[Value]) -> Result<Value, String> {
    let id = expect_kind(args.get(0).unwrap_or(&Value::Nil), "array", "jawa::array_to_list")?;
    match get_array_cell(id) {
        Some(cell) => Ok(Value::Array(cell.lock().unwrap_or_else(|p| p.into_inner()).clone().into())),
        None => Err(pesan_sudah_bebas("jawa::array_to_list", "array")),
    }
}

pub fn new_buffer(args: &[Value]) -> Result<Value, String> {
    let n = cek_panjang("jawa::buffer", bulat(args.get(0), "jawa::buffer")?)?;
    let id = next_id();
    buffers().lock().unwrap_or_else(|p| p.into_inner()).insert(id, Arc::new(Mutex::new(vec![0u8; n])));
    TOTAL_ALLOCATED.fetch_add(1, Ordering::Relaxed);
    Ok(make_handle("buffer", id))
}

pub fn new_buffer_from_hex(args: &[Value]) -> Result<Value, String> {
    let bytes = buffer_dari_hex(args.get(0).map(|v| match v { Value::Str(s) => Ok(s.as_str()), o => Err(o.type_name()) }))?;
    let id = next_id();
    buffers().lock().unwrap_or_else(|p| p.into_inner()).insert(id, Arc::new(Mutex::new(bytes)));
    TOTAL_ALLOCATED.fetch_add(1, Ordering::Relaxed);
    Ok(make_handle("buffer", id))
}

/// Isi buffer dari teks hex — satu definisi untuk dua mesin (`hex_decode_str`). Salinan
/// VM dulu memotong teksnya per BYTE dan panik di tengah karakter multi-byte.
pub(crate) fn buffer_dari_hex(teks: Option<Result<&str, String>>) -> Result<Vec<u8>, String> {
    const F: &str = "jawa::buffer_from_hex";
    let hex = match teks {
        Some(Ok(s)) => s,
        Some(Err(tipe)) => return Err(crate::interpreter::pesan_arg_harus_teks(F, "teks hex-nya", &tipe)),
        None => return Err(format!("{}: kurang argumen", F)),
    };
    crate::dok::crypto::hex_decode_str(hex).map_err(|e| format!("{}: {}", F, e))
}

fn get_buffer_cell(id: u64) -> Option<BufferCell> {
    buffers().lock().unwrap_or_else(|p| p.into_inner()).get(&id).cloned()
}

pub fn new_buffer_get(args: &[Value]) -> Result<Value, String> {
    const F: &str = "jawa::buffer_get";
    let id = expect_kind(args.get(0).unwrap_or(&Value::Nil), "buffer", F)?;
    let i = bulat(args.get(1), F)?;
    let cell = get_buffer_cell(id).ok_or_else(|| pesan_sudah_bebas(F, "buffer"))?;
    let guard = cell.lock().unwrap_or_else(|p| p.into_inner());
    Ok(Value::Int(guard[cek_index(F, i, guard.len())?] as i128))
}

pub fn new_buffer_set(args: &[Value]) -> Result<Value, String> {
    const F: &str = "jawa::buffer_set";
    let id = expect_kind(args.get(0).unwrap_or(&Value::Nil), "buffer", F)?;
    let i = bulat(args.get(1), F)?;
    let byte = byte_dari(args.get(2), F)?;
    let cell = get_buffer_cell(id).ok_or_else(|| pesan_sudah_bebas(F, "buffer"))?;
    let mut guard = cell.lock().unwrap_or_else(|p| p.into_inner());
    let k = cek_index(F, i, guard.len())?;
    guard[k] = byte;
    Ok(Value::Unit)
}

pub fn new_buffer_len(args: &[Value]) -> Result<Value, String> {
    let id = expect_kind(args.get(0).unwrap_or(&Value::Nil), "buffer", "jawa::buffer_len")?;
    match get_buffer_cell(id) {
        Some(cell) => Ok(Value::Int(cell.lock().unwrap_or_else(|p| p.into_inner()).len() as i128)),
        None => Err(pesan_sudah_bebas("jawa::buffer_len", "buffer")),
    }
}

pub fn new_buffer_fill(args: &[Value]) -> Result<Value, String> {
    let id = expect_kind(args.get(0).unwrap_or(&Value::Nil), "buffer", "jawa::buffer_fill")?;
    let byte = byte_dari(args.get(1), "jawa::buffer_fill")?;
    match get_buffer_cell(id) {
        Some(cell) => { let mut g = cell.lock().unwrap_or_else(|p| p.into_inner()); let len = g.len(); *g = vec![byte; len]; Ok(Value::Unit) }
        None => Err(pesan_sudah_bebas("jawa::buffer_fill", "buffer")),
    }
}

pub fn new_buffer_to_hex(args: &[Value]) -> Result<Value, String> {
    let id = expect_kind(args.get(0).unwrap_or(&Value::Nil), "buffer", "jawa::buffer_to_hex")?;
    match get_buffer_cell(id) {
        Some(cell) => Ok(Value::Str(crate::dok::crypto::hex_encode(&cell.lock().unwrap_or_else(|p| p.into_inner())))),
        None => Err(pesan_sudah_bebas("jawa::buffer_to_hex", "buffer")),
    }
}

pub fn new_is_valid(args: &[Value]) -> Result<Value, String> {
    let (id, kind) = match handle_parts(args.get(0).unwrap_or(&Value::Nil), "jawa::is_valid") {
        Ok(p) => p,
        Err(_) => return Ok(Value::Bool(false)),
    };
    let alive = match kind.as_str() {
        "single" => singles().lock().unwrap_or_else(|p| p.into_inner()).contains_key(&id),
        "array"  => arrays().lock().unwrap_or_else(|p| p.into_inner()).contains_key(&id),
        "buffer" => buffers().lock().unwrap_or_else(|p| p.into_inner()).contains_key(&id),
        _ => false,
    };
    Ok(Value::Bool(alive))
}

pub(crate) fn value_size_estimate(v: &Value) -> usize {
    match v {
        Value::Nil | Value::Unit => 0,
        Value::Bool(_) => 1,
        Value::Int(_) => 16,
        Value::Float(_) => 8,
        Value::Rune(_) => 4,
        Value::Str(s) => s.len(),
        Value::Array(a) | Value::Tuple(a) => a.iter().map(value_size_estimate).sum::<usize>() + a.len() * 8,
        Value::Struct { fields, .. } => fields.values().map(value_size_estimate).sum::<usize>() + fields.len() * 24,
        _ => 8,
    }
}

pub fn new_sizeof(args: &[Value]) -> Result<Value, String> {
    let v = args.get(0).unwrap_or(&Value::Nil);
    if let Ok((id, kind)) = handle_parts(v, "jawa::sizeof") {
        let size = match kind.as_str() {
            "single" => singles().lock().unwrap_or_else(|p| p.into_inner()).get(&id).map(|c| value_size_estimate(&c.lock().unwrap_or_else(|p| p.into_inner()))),
            "array"  => arrays().lock().unwrap_or_else(|p| p.into_inner()).get(&id).map(|c| c.lock().unwrap_or_else(|p| p.into_inner()).iter().map(value_size_estimate).sum::<usize>()),
            "buffer" => buffers().lock().unwrap_or_else(|p| p.into_inner()).get(&id).map(|c| c.lock().unwrap_or_else(|p| p.into_inner()).len()),
            _ => None,
        };
        // Alokasi yang sudah dibebaskan tidak punya ukuran: dulu dijawab `0`.
        return size.map(|n| Value::Int(n as i128)).ok_or_else(|| pesan_sudah_bebas("jawa::sizeof", &kind));
    }
    Ok(Value::Int(value_size_estimate(v) as i128))
}

pub fn new_stats(_args: &[Value]) -> Result<Value, String> {
    let alive_single = singles().lock().unwrap_or_else(|p| p.into_inner()).len();
    let alive_array = arrays().lock().unwrap_or_else(|p| p.into_inner()).len();
    let alive_buffer = buffers().lock().unwrap_or_else(|p| p.into_inner()).len();
    let mut fields = HashMap::new();
    fields.insert("alive_total".to_string(), Value::Int((alive_single + alive_array + alive_buffer) as i128));
    fields.insert("alive_single".to_string(), Value::Int(alive_single as i128));
    fields.insert("alive_array".to_string(), Value::Int(alive_array as i128));
    fields.insert("alive_buffer".to_string(), Value::Int(alive_buffer as i128));
    fields.insert("total_allocated".to_string(), Value::Int(TOTAL_ALLOCATED.load(Ordering::Relaxed) as i128));
    fields.insert("total_freed".to_string(), Value::Int(TOTAL_FREED.load(Ordering::Relaxed) as i128));
    Ok(Value::Struct { name: "HeapStats".to_string(), fields: fields.into() })
}

pub fn delete_free(args: &[Value]) -> Result<Value, String> {
    let (id, kind) = handle_parts(args.get(0).unwrap_or(&Value::Nil), "jawir::free")?;
    let removed = match kind.as_str() {
        "single" => singles().lock().unwrap_or_else(|p| p.into_inner()).remove(&id).is_some(),
        "array"  => arrays().lock().unwrap_or_else(|p| p.into_inner()).remove(&id).is_some(),
        "buffer" => buffers().lock().unwrap_or_else(|p| p.into_inner()).remove(&id).is_some(),
        other => return Err(format!("jawir::free: jenis handle tidak dikenal `{}`", other)),
    };
    if removed {
        TOTAL_FREED.fetch_add(1, Ordering::Relaxed);
        Ok(Value::Unit)
    } else {
        Err(PESAN_DOUBLE_FREE.to_string())
    }
}

// ─── Dispatch dari NativeFnKind — dipanggil dari builtin::call_native ────

pub(super) fn dispatch(kind: &NativeFnKind, args: &[Value]) -> Option<EvalResult> {
    use NativeFnKind::*;
    let r: Result<Value, String> = match kind {
        NewAlloc         => new_alloc(args),
        NewGet           => new_get(args),
        NewSet           => new_set(args),
        NewArray         => new_array(args),
        NewArrayGet      => new_array_get(args),
        NewArraySet      => new_array_set(args),
        NewArrayPush     => new_array_push(args),
        NewArrayPop      => new_array_pop(args),
        NewArrayLen      => new_array_len(args),
        NewArrayToList   => new_array_to_list(args),
        NewBuffer        => new_buffer(args),
        NewBufferFromHex => new_buffer_from_hex(args),
        NewBufferGet     => new_buffer_get(args),
        NewBufferSet     => new_buffer_set(args),
        NewBufferLen     => new_buffer_len(args),
        NewBufferFill    => new_buffer_fill(args),
        NewBufferToHex   => new_buffer_to_hex(args),
        NewIsValid       => new_is_valid(args),
        NewSizeof        => new_sizeof(args),
        NewStats         => new_stats(args),
        DeleteFree       => delete_free(args),
        _ => return None,
    };
    Some(r.map_err(|e| Signal::RuntimeError(RuntimeError::new(e))))
}
