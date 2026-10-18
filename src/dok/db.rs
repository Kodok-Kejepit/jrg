//! `dok::db_*` — driver SQLite, lewat `libsqlite3` yang dimuat saat jalan.

use crate::interpreter::{err_value, ok_value, Value};
use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::sync::{Mutex, OnceLock};

// ─────────────────────────────────────────────────────────────────────────
// Simbol C
// ─────────────────────────────────────────────────────────────────────────

const SQLITE_OK: c_int = 0;
const SQLITE_ROW: c_int = 100;
const SQLITE_DONE: c_int = 101;
const SQLITE_OPEN_READWRITE: c_int = 0x0000_0002;
const SQLITE_OPEN_CREATE: c_int = 0x0000_0004;

/// Tanda bahwa teks yang diikat harus DISALIN sqlite, bukan dipinjam.
const SQLITE_TRANSIENT: isize = -1;

type FnOpenV2 = unsafe extern "C" fn(*const c_char, *mut *mut c_void, c_int, *const c_char) -> c_int;
type FnClose = unsafe extern "C" fn(*mut c_void) -> c_int;
type FnPrepare = unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut *mut c_void, *mut *const c_char) -> c_int;
type FnStep = unsafe extern "C" fn(*mut c_void) -> c_int;
type FnFinalize = unsafe extern "C" fn(*mut c_void) -> c_int;
type FnErrmsg = unsafe extern "C" fn(*mut c_void) -> *const c_char;
type FnColCount = unsafe extern "C" fn(*mut c_void) -> c_int;
type FnColName = unsafe extern "C" fn(*mut c_void, c_int) -> *const c_char;
type FnColType = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type FnColText = unsafe extern "C" fn(*mut c_void, c_int) -> *const u8;
type FnColBytes = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type FnColInt64 = unsafe extern "C" fn(*mut c_void, c_int) -> i64;
type FnColDouble = unsafe extern "C" fn(*mut c_void, c_int) -> f64;
type FnBindText = unsafe extern "C" fn(*mut c_void, c_int, *const c_char, c_int, isize) -> c_int;
type FnBindBlob = unsafe extern "C" fn(*mut c_void, c_int, *const c_void, c_int, isize) -> c_int;
type FnBindInt64 = unsafe extern "C" fn(*mut c_void, c_int, i64) -> c_int;
type FnBindDouble = unsafe extern "C" fn(*mut c_void, c_int, f64) -> c_int;
type FnBindNull = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type FnChanges = unsafe extern "C" fn(*mut c_void) -> c_int;
type FnLastId = unsafe extern "C" fn(*mut c_void) -> i64;

struct Api {
    open_v2: FnOpenV2, close: FnClose, prepare: FnPrepare, step: FnStep,
    finalize: FnFinalize, errmsg: FnErrmsg,
    col_count: FnColCount, col_name: FnColName, col_type: FnColType,
    col_text: FnColText, col_bytes: FnColBytes, col_int64: FnColInt64,
    col_double: FnColDouble,
    bind_text: FnBindText, bind_blob: FnBindBlob, bind_int64: FnBindInt64, bind_double: FnBindDouble,
    bind_null: FnBindNull, changes: FnChanges, last_id: FnLastId,
}

/// Nama pustaka yang dicoba, urut.
const KANDIDAT: &[&str] = &[
    "libsqlite3.so.0", "libsqlite3.so", "libsqlite3.dylib", "sqlite3.dll",
];

static API: OnceLock<Result<Api, String>> = OnceLock::new();

fn api() -> Result<&'static Api, String> {
    match API.get_or_init(muat_api) {
        Ok(a) => Ok(a),
        Err(e) => Err(e.clone()),
    }
}

fn muat_api() -> Result<Api, String> {
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
            "dok::db: tidak bisa memuat libsqlite3 — sqlite dipakai lewat pustaka sistem, \
             bukan ditautkan ke dalam biner ini (supaya satu biner tetap jalan di Termux \
             arm64 maupun x86_64).\n  bantuan: pasang sqlite — Debian/Ubuntu: \
             `apt install libsqlite3-0`; Termux: `pkg install sqlite`\n  yang dicoba: {}",
            gagal.join("; ")));
    };
    // Tiap simbol diselesaikan SEKARANG, bukan saat dipakai. Sebuah pustaka
    // yang ada tapi tidak punya salah satu simbol ini bukan sqlite — dan
    // mengetahuinya di `db_open` jauh lebih baik daripada di tengah query.
    macro_rules! sym {
        ($n:literal, $t:ty) => {{
            let addr = crate::ffi::native::resolve_symbol(h, $n)
                .map_err(|e| format!("dok::db: libsqlite3 tidak punya `{}`: {}", $n, e))?;
            unsafe { std::mem::transmute::<usize, $t>(addr) }
        }};
    }
    Ok(Api {
        open_v2:   sym!("sqlite3_open_v2", FnOpenV2),
        close:     sym!("sqlite3_close", FnClose),
        prepare:   sym!("sqlite3_prepare_v2", FnPrepare),
        step:      sym!("sqlite3_step", FnStep),
        finalize:  sym!("sqlite3_finalize", FnFinalize),
        errmsg:    sym!("sqlite3_errmsg", FnErrmsg),
        col_count: sym!("sqlite3_column_count", FnColCount),
        col_name:  sym!("sqlite3_column_name", FnColName),
        col_type:  sym!("sqlite3_column_type", FnColType),
        col_text:  sym!("sqlite3_column_text", FnColText),
        col_bytes: sym!("sqlite3_column_bytes", FnColBytes),
        col_int64: sym!("sqlite3_column_int64", FnColInt64),
        col_double: sym!("sqlite3_column_double", FnColDouble),
        bind_text: sym!("sqlite3_bind_text", FnBindText),
        bind_blob: sym!("sqlite3_bind_blob", FnBindBlob),
        bind_int64: sym!("sqlite3_bind_int64", FnBindInt64),
        bind_double: sym!("sqlite3_bind_double", FnBindDouble),
        bind_null: sym!("sqlite3_bind_null", FnBindNull),
        changes:   sym!("sqlite3_changes", FnChanges),
        last_id:   sym!("sqlite3_last_insert_rowid", FnLastId),
    })
}

// ─────────────────────────────────────────────────────────────────────────
// Registri koneksi
// ─────────────────────────────────────────────────────────────────────────

/// Pointer `sqlite3*`, dibungkus supaya bisa disimpan di registri global.
struct Db(*mut c_void);
unsafe impl Send for Db {}

static REG: OnceLock<Mutex<HashMap<u64, Db>>> = OnceLock::new();
fn reg() -> &'static Mutex<HashMap<u64, Db>> { REG.get_or_init(|| Mutex::new(HashMap::new())) }
static URUT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn handle(id: u64) -> Value {
    let mut f = HashMap::new();
    f.insert("id".to_string(), Value::Int(id as i128));
    Value::Struct { name: "DokDb".to_string(), fields: f.into() }
}

fn id_dari(v: Option<&Value>, fname: &str) -> Result<u64, String> {
    match v {
        Some(Value::Struct { name, fields }) if name == "DokDb" => match fields.get("id") {
            Some(Value::Int(n)) if u64::try_from(*n).is_ok() => Ok(*n as u64),
            _ => Err(format!("{}: handle basis data rusak", fname)),
        },
        Some(lain) => Err(format!(
            "{}: butuh koneksi basis data (hasil `dok::db_open`), dapat `{}`",
            fname, lain.type_name())),
        None => Err(format!("{}: butuh koneksinya", fname)),
    }
}

unsafe fn pesan_galat(a: &Api, db: *mut c_void) -> String {
    let p = (a.errmsg)(db);
    if p.is_null() { return "galat sqlite tanpa pesan".to_string(); }
    std::ffi::CStr::from_ptr(p).to_string_lossy().to_string()
}

// ─────────────────────────────────────────────────────────────────────────
// API
// ─────────────────────────────────────────────────────────────────────────

/// `dok::db_open(path)` → `Result<koneksi, string>`.
pub fn db_open(args: &[Value]) -> Result<Value, String> {
    let path = match args.get(0) {
        Some(Value::Str(s)) => s.clone(),
        Some(lain) => return Err(format!("dok::db_open: path harus string, dapat `{}`", lain.type_name())),
        None => return Err("dok::db_open: butuh path-nya (\":memory:\" untuk di memori)".to_string()),
    };
    let a = match api() { Ok(a) => a, Err(e) => return Ok(err_value(e)) };
    let c = match CString::new(path.clone()) {
        Ok(c) => c,
        Err(_) => return Ok(err_value("dok::db_open: path memuat byte NUL")),
    };
    let mut db: *mut c_void = std::ptr::null_mut();
    let rc = unsafe {
        (a.open_v2)(c.as_ptr(), &mut db, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
                    std::ptr::null())
    };
    if rc != SQLITE_OK {
        // Pesannya diambil DULU, lalu handle-nya ditutup: sqlite mengembalikan
        // sebuah handle bahkan saat gagal, justru supaya sebabnya bisa dibaca.
        // SQLITE_CANTOPEN (14): kalimat sqlite-nya berbahasa Inggris dan tidak menyebut sebabnya.
        let pesan = if rc == 14 { "tidak bisa dibuka — direktorinya tidak ada, atau tidak diizinkan menulis di sana".to_string() }
                    else if db.is_null() { format!("kode {}", rc) }
                    else { unsafe { pesan_galat(a, db) } };
        if !db.is_null() { unsafe { (a.close)(db); } }
        return Ok(err_value(format!("dok::db_open `{}`: {}", path, pesan)));
    }
    let id = URUT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    reg().lock().unwrap_or_else(|p| p.into_inner()).insert(id, Db(db));
    Ok(ok_value(handle(id)))
}

/// `dok::db_close(db)`.
pub fn db_close(args: &[Value]) -> Result<Value, String> {
    let id = id_dari(args.get(0), "dok::db_close")?;
    let a = match api() { Ok(a) => a, Err(e) => return Ok(err_value(e)) };
    if let Some(Db(p)) = reg().lock().unwrap_or_else(|q| q.into_inner()).remove(&id) {
        unsafe { (a.close)(p); }
    }
    Ok(Value::Unit)
}

/// Siapkan sebuah statement dan ikat parameternya.
unsafe fn siapkan(
    a: &Api, db: *mut c_void, sql: &str, params: &[Value],
) -> Result<*mut c_void, String> {
    let c = CString::new(sql).map_err(|_| "SQL memuat byte NUL".to_string())?;
    let mut stmt: *mut c_void = std::ptr::null_mut();
    let mut ekor: *const c_char = std::ptr::null();
    let rc = (a.prepare)(db, c.as_ptr(), -1, &mut stmt, &mut ekor);
    if rc != SQLITE_OK || stmt.is_null() {
        return Err(pesan_galat(a, db));
    }
    for (i, p) in params.iter().enumerate() {
        let idx = (i + 1) as c_int;   // parameter sqlite dihitung dari 1
        let rc = match p {
            Value::Nil | Value::Unit => (a.bind_null)(stmt, idx),
            Value::Bool(b) => (a.bind_int64)(stmt, idx, if *b { 1 } else { 0 }),
            Value::Int(n) => {
                // `i128` tidak muat di `INTEGER` sqlite (64-bit). Ditolak,
                // bukan dipotong: sebuah id yang berubah diam-diam adalah baris
                // yang salah, bukan galat.
                match i64::try_from(*n) {
                    Ok(v) => (a.bind_int64)(stmt, idx, v),
                    Err(_) => {
                        (a.finalize)(stmt);
                        return Err(format!(
                            "parameter ke-{} ({}) tidak muat di INTEGER sqlite (64-bit)",
                            i + 1, n));
                    }
                }
            }
            Value::Float(f) => (a.bind_double)(stmt, idx, *f),
            Value::Str(s) => {
                let cs = match CString::new(s.as_str()) {
                    Ok(c) => c,
                    Err(_) => {
                        (a.finalize)(stmt);
                        return Err(format!("parameter ke-{} memuat byte NUL", i + 1));
                    }
                };
                (a.bind_text)(stmt, idx, cs.as_ptr(), s.len() as c_int, SQLITE_TRANSIENT)
            }
            // Array byte jadi BLOB, kebalikan `nilai_kolom`.
            Value::Array(isi) => match super::crypto::array_ke_byte(isi, "parameter BLOB") {
                Ok(b) => (a.bind_blob)(stmt, idx, b.as_ptr() as *const c_void, b.len() as c_int, SQLITE_TRANSIENT),
                Err(e) => { (a.finalize)(stmt); return Err(format!("parameter ke-{}: {}", i + 1, e)); }
            },
            lain => {
                (a.finalize)(stmt);
                return Err(format!(
                    "parameter ke-{} bertipe `{}` tidak punya padanan di sqlite — \
                     yang bisa diikat: nil, bool, int, float, string, array byte",
                    i + 1, lain.type_name()));
            }
        };
        if rc != SQLITE_OK {
            let e = pesan_galat(a, db);
            (a.finalize)(stmt);
            return Err(e);
        }
    }
    Ok(stmt)
}

fn params_dari(v: Option<&Value>, fname: &str) -> Result<Vec<Value>, String> {
    match v {
        None | Some(Value::Nil) => Ok(Vec::new()),
        Some(Value::Array(a)) => Ok(a.to_vec()),
        Some(lain) => Err(format!(
            "{}: parameter harus array, dapat `{}`", fname, lain.type_name())),
    }
}

/// `dok::db_exec(db, sql, params?)` → `Result<int, string>` (jumlah baris berubah).
pub fn db_exec(args: &[Value]) -> Result<Value, String> {
    let id = id_dari(args.get(0), "dok::db_exec")?;
    let sql = match args.get(1) {
        Some(Value::Str(s)) => s.clone(),
        _ => return Err("dok::db_exec: butuh SQL-nya sebagai string".to_string()),
    };
    let params = params_dari(args.get(2), "dok::db_exec")?;
    let a = match api() { Ok(a) => a, Err(e) => return Ok(err_value(e)) };
    let reg = reg().lock().unwrap_or_else(|p| p.into_inner());
    let Some(Db(db)) = reg.get(&id) else { return Ok(err_value("koneksi sudah ditutup")) };
    let db = *db;
    unsafe {
        let stmt = match siapkan(a, db, &sql, &params) {
            Ok(s) => s, Err(e) => return Ok(err_value(format!("dok::db_exec: {}", e))),
        };
        loop {
            match (a.step)(stmt) {
                SQLITE_DONE => break,
                SQLITE_ROW => continue,   // `exec` atas SELECT: barisnya dibuang
                _ => {
                    let e = pesan_galat(a, db);
                    (a.finalize)(stmt);
                    return Ok(err_value(format!("dok::db_exec: {}", e)));
                }
            }
        }
        (a.finalize)(stmt);
        Ok(ok_value(Value::Int((a.changes)(db) as i128)))
    }
}

/// `dok::db_query(db, sql, params?)` → `Result<array<dict>, string>`.
pub fn db_query(args: &[Value]) -> Result<Value, String> {
    let id = id_dari(args.get(0), "dok::db_query")?;
    let sql = match args.get(1) {
        Some(Value::Str(s)) => s.clone(),
        _ => return Err("dok::db_query: butuh SQL-nya sebagai string".to_string()),
    };
    let params = params_dari(args.get(2), "dok::db_query")?;
    let a = match api() { Ok(a) => a, Err(e) => return Ok(err_value(e)) };
    let reg = reg().lock().unwrap_or_else(|p| p.into_inner());
    let Some(Db(db)) = reg.get(&id) else { return Ok(err_value("koneksi sudah ditutup")) };
    let db = *db;
    unsafe {
        let stmt = match siapkan(a, db, &sql, &params) {
            Ok(s) => s, Err(e) => return Ok(err_value(format!("dok::db_query: {}", e))),
        };
        let n = (a.col_count)(stmt);
        let mut nama = Vec::with_capacity(n as usize);
        for i in 0..n {
            let p = (a.col_name)(stmt, i);
            nama.push(if p.is_null() { format!("kolom{}", i) }
                      else { std::ffi::CStr::from_ptr(p).to_string_lossy().to_string() });
        }
        let mut baris = Vec::new();
        loop {
            match (a.step)(stmt) {
                SQLITE_ROW => {
                    let mut f: HashMap<String, Value> = HashMap::new();
                    for i in 0..n {
                        match nilai_kolom(a, stmt, i) {
                            Ok(v) => { f.insert(nama[i as usize].clone(), v); }
                            Err(()) => {
                                (a.finalize)(stmt);
                                return Ok(err_value(format!(
                                    "dok::db_query: kolom `{0}` berisi teks yang bukan UTF-8 sah; ambil byte-nya dengan `CAST({0} AS BLOB)`",
                                    nama[i as usize])));
                            }
                        }
                    }
                    baris.push(Value::Struct {
                        name: super::json::OBJ_NAME.to_string(), fields: f.into() });
                }
                SQLITE_DONE => break,
                _ => {
                    let e = pesan_galat(a, db);
                    (a.finalize)(stmt);
                    return Ok(err_value(format!("dok::db_query: {}", e)));
                }
            }
        }
        (a.finalize)(stmt);
        Ok(ok_value(Value::Array(baris.into())))
    }
}

/// BLOB jadi array byte; TEXT harus UTF-8 sah (`Err` kalau bukan, jangan diganti U+FFFD).
/// `column_bytes` dipakai, bukan `strlen`: isinya boleh memuat NUL di tengah.
unsafe fn nilai_kolom(a: &Api, stmt: *mut c_void, i: c_int) -> Result<Value, ()> {
    Ok(match (a.col_type)(stmt, i) {
        1 => Value::Int((a.col_int64)(stmt, i) as i128),   // INTEGER
        2 => Value::Float((a.col_double)(stmt, i)),        // FLOAT
        5 => Value::Nil,                                   // NULL
        jenis => {
            let p = (a.col_text)(stmt, i);
            let n = (a.col_bytes)(stmt, i).max(0) as usize;
            let b = if p.is_null() { Vec::new() } else { std::slice::from_raw_parts(p, n).to_vec() };
            if jenis == 4 { super::crypto::byte_ke_array(b) }
            else { Value::Str(String::from_utf8(b).map_err(|_| ())?) }
        }
    })
}

/// `dok::db_last_id(db)` → int — rowid dari INSERT terakhir di koneksi ini.
pub fn db_last_id(args: &[Value]) -> Result<Value, String> {
    let id = id_dari(args.get(0), "dok::db_last_id")?;
    let a = match api() { Ok(a) => a, Err(e) => return Ok(err_value(e)) };
    let reg = reg().lock().unwrap_or_else(|p| p.into_inner());
    let Some(Db(db)) = reg.get(&id) else { return Ok(err_value("koneksi sudah ditutup")) };
    Ok(ok_value(Value::Int(unsafe { (a.last_id)(*db) } as i128)))
}

/// `dok::db_tersedia()` → bool — apakah libsqlite3 bisa dimuat di mesin ini.
pub fn db_tersedia(_args: &[Value]) -> Result<Value, String> {
    Ok(Value::Bool(api().is_ok()))
}
