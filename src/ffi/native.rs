// src/ffi/native.rs
// The *real*, working runtime FFI: resolves `extern "C" { ... }` function

use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::sync::{Mutex, OnceLock};

use crate::interpreter::{err_value, ok_value, EvalResult, Value};
use crate::parser::{ExternParam, OwnershipQual, PrimTy, TypeExpr};

// ---------------------------------------------------------------------------
// Platform-specific dynamic-library loading
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod sys {
    use super::*;

    extern "C" {
        fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlerror() -> *mut c_char;
    }

    // `RTLD_NOW` (resolve all symbols immediately, surfacing a missing symbol as
    // a load-time failure rather than a later crash) — value `2` is identical …
    const RTLD_NOW: c_int = 2;

    /// Loads a shared library by path, or — when `path` is `None` — resolves
    /// against the current process's own global symbol table, which already …
    pub fn load(path: Option<&str>) -> Result<usize, String> {
        let c_path: Option<CString> = match path {
            Some(p) => Some(CString::new(p)
                .map_err(|_| format!("path library mengandung byte NUL: {:?}", p))?),
            None => None,
        };
        let ptr: *const c_char = c_path.as_ref()
            .map_or(std::ptr::null::<c_char>(), |c| c.as_ptr());
        let handle = unsafe { dlopen(ptr, RTLD_NOW) };
        if handle.is_null() {
            let msg = unsafe {
                let e = dlerror();
                if e.is_null() { "dlopen gagal (alasan tidak diketahui)".to_string() }
                else { std::ffi::CStr::from_ptr(e as *const c_char).to_string_lossy().into_owned() }
            };
            return Err(msg);
        }
        Ok(handle as usize)
    }

    /// Resolves one symbol's address within an already-loaded library
    /// handle (as returned by `load`).
    pub fn symbol(handle: usize, name: &str) -> Result<usize, String> {
        let c_name = CString::new(name)
            .map_err(|_| format!("nama simbol mengandung byte NUL: {:?}", name))?;
        // Clear any prior error before calling dlsym, since a genuinely valid symbol
        // can resolve to address 0 in rare cases and dlerror() is the documented way …
        unsafe { dlerror(); }
        let ptr = unsafe { dlsym(handle as *mut c_void, c_name.as_ptr()) };
        if ptr.is_null() {
            let msg = unsafe {
                let e = dlerror();
                if e.is_null() { format!("simbol `{}` tidak ditemukan", name) }
                else { std::ffi::CStr::from_ptr(e as *const c_char).to_string_lossy().into_owned() }
            };
            return Err(msg);
        }
        Ok(ptr as usize)
    }
}

#[cfg(windows)]
mod sys {
    use super::*;

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryA(lp_lib_file_name: *const c_char) -> *mut c_void;
        fn GetProcAddress(h_module: *mut c_void, lp_proc_name: *const c_char) -> *mut c_void;
        fn GetModuleHandleA(lp_module_name: *const c_char) -> *mut c_void;
    }

    /// Best-effort symbol search across a small, fixed list of system DLLs
    /// commonly already loaded into any process on modern Windows, used when no …
    const FALLBACK_MODULES: &[&str] = &["kernel32.dll", "ucrtbase.dll", "msvcrt.dll"];

    pub fn load(path: Option<&str>) -> Result<usize, String> {
        match path {
            Some(p) => {
                let c_path = CString::new(p)
                    .map_err(|_| format!("path library mengandung byte NUL: {:?}", p))?;
                let h = unsafe { LoadLibraryA(c_path.as_ptr()) };
                if h.is_null() {
                    return Err(format!("LoadLibraryA(\"{}\") gagal", p));
                }
                Ok(h as usize)
            }
            // No single handle represents "the whole process" on Windows the way it does
            // on Unix; `symbol` below tries `FALLBACK_MODULES` one at a time instead.
            None => Ok(usize::MAX),
        }
    }

    pub fn symbol(handle: usize, name: &str) -> Result<usize, String> {
        let c_name = CString::new(name)
            .map_err(|_| format!("nama simbol mengandung byte NUL: {:?}", name))?;
        if handle == usize::MAX {
            for module in FALLBACK_MODULES {
                let c_mod = CString::new(*module).unwrap();
                let h = unsafe { GetModuleHandleA(c_mod.as_ptr()) };
                if h.is_null() { continue; }
                let p = unsafe { GetProcAddress(h, c_name.as_ptr()) };
                if !p.is_null() { return Ok(p as usize); }
            }
            return Err(format!(
                "simbol `{}` tidak ditemukan pada module sistem manapun ({})",
                name, FALLBACK_MODULES.join(", ")
            ));
        }
        let p = unsafe { GetProcAddress(handle as *mut c_void, c_name.as_ptr()) };
        if p.is_null() {
            return Err(format!("simbol `{}` tidak ditemukan", name));
        }
        Ok(p as usize)
    }
}

#[cfg(not(any(unix, windows)))]
mod sys {
    pub fn load(_path: Option<&str>) -> Result<usize, String> {
        Err("FFI native belum didukung pada platform ini".to_string())
    }
    pub fn symbol(_handle: usize, _name: &str) -> Result<usize, String> {
        Err("FFI native belum didukung pada platform ini".to_string())
    }
}

/// Process-wide cache of already-loaded libraries, keyed by the path named in
/// the `extern` block (`None` for "no library named", i.e.
fn library_cache() -> &'static Mutex<HashMap<Option<String>, usize>> {
    static CACHE: OnceLock<Mutex<HashMap<Option<String>, usize>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Loads (or returns the already-cached handle for) the library named by
/// `path`, or the process/fallback-module symbol table when `path` is `None`.
pub fn resolve_symbol(handle: usize, name: &str) -> Result<usize, String> {
    sys::symbol(handle, name)
}

pub fn load_library(path: Option<&str>) -> Result<usize, String> {
    let key = path.map(|p| p.to_string());
    let mut cache = library_cache().lock().unwrap_or_else(|p| p.into_inner());
    if let Some(handle) = cache.get(&key) {
        return Ok(*handle);
    }
    let handle = sys::load(path)?;
    cache.insert(key, handle);
    Ok(handle)
}

// ---------------------------------------------------------------------------
// C-ABI type classification
// ---------------------------------------------------------------------------

/// The C-ABI shape of one parameter or return type, as declared in an
/// `extern "C" { ... }` block. See the module doc comment for exactly
/// which JRG types map to which variant, and which are rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfiType {
    /// Valid only as a return type — C `void`.
    Unit,
    /// C `_Bool`/`bool` (`<stdbool.h>`).
    Bool,
    /// Any integer-class C type, with its exact bit width and signedness.
    Int { bits: u8, signed: bool },
    /// C `float`.
    F32,
    /// C `double`.
    F64,
    /// Any pointer type (`*const T`, `*mut T`, `raw<T>`) — an opaque
    /// address, regardless of the pointee type `T`.
    Ptr,
    /// `str`/`string` — marshaled as a NUL-terminated C string
    /// (`*const char` on the C side), not a raw pointer/integer.
    CStr,
    /// `[]T` / `[N]T` untuk `T` numerik — array JRG disalin ke buffer C yang
    /// rapat, lalu POINTER-nya yang diserahkan.
    Arr { bits: u8, signed: bool, float: bool },
}

impl FfiType {
    /// Whether this type occupies a float-class argument/return slot (an SSE/XMM
    /// register on the SysV x86-64 ABI) as opposed to an integer-class one (a …
    fn is_float_class(self) -> bool {
        matches!(self, FfiType::F32 | FfiType::F64)
    }
}

/// Classifies a JRG `TypeExpr` (as written in an `extern "C" { ...
fn klasifikasi_array(elem: &TypeExpr) -> Result<FfiType, String> {
    match classify_type(elem)? {
        FfiType::Int { bits, signed } => Ok(FfiType::Arr { bits, signed, float: false }),
        FfiType::F32 => Ok(FfiType::Arr { bits: 32, signed: true, float: true }),
        FfiType::F64 => Ok(FfiType::Arr { bits: 64, signed: true, float: true }),
        FfiType::Bool => Ok(FfiType::Arr { bits: 8, signed: false, float: false }),
        lain => Err(format!(
            "array FFI cuma boleh berisi angka atau bool, dapat `{:?}` — untuk yang \
             lain kirim sebagai `raw<T>` plus panjangnya", lain)),
    }
}

pub fn classify_type(ty: &TypeExpr) -> Result<FfiType, String> {
    match ty {
        TypeExpr::Primitive(p) => match p {
            PrimTy::I8  => Ok(FfiType::Int { bits: 8,  signed: true }),
            PrimTy::I16 => Ok(FfiType::Int { bits: 16, signed: true }),
            PrimTy::I32 => Ok(FfiType::Int { bits: 32, signed: true }),
            PrimTy::I64 => Ok(FfiType::Int { bits: 64, signed: true }),
            // `isize` and JRG's generic `int` are both treated as 64-bit for FFI
            // purposes (a sensible default matching `int64_t`/`long` on the LP64 Unix …
            PrimTy::Isize | PrimTy::Int => Ok(FfiType::Int { bits: 64, signed: true }),
            PrimTy::U8    => Ok(FfiType::Int { bits: 8,  signed: false }),
            PrimTy::U16   => Ok(FfiType::Int { bits: 16, signed: false }),
            PrimTy::U32   => Ok(FfiType::Int { bits: 32, signed: false }),
            PrimTy::U64   => Ok(FfiType::Int { bits: 64, signed: false }),
            PrimTy::Usize | PrimTy::Uint => Ok(FfiType::Int { bits: 64, signed: false }),
            PrimTy::Byte  => Ok(FfiType::Int { bits: 8, signed: false }),
            PrimTy::Bool  => Ok(FfiType::Bool),
            PrimTy::F64   => Ok(FfiType::F64),
            PrimTy::F32   => Ok(FfiType::F32),
            PrimTy::I128 | PrimTy::U128 => Err(
                "i128/u128 belum didukung di FFI (konvensi ABI 128-bit berbeda \
                 dari integer biasa — butuh dua register, bukan satu)".to_string()),
            // f128 is turned away here deliberately, not just "not implemented yet" —
            // two separate, real blockers: 1.
            PrimTy::F128 => Err(
                "f128 tidak didukung: tipe f128 Rust sendiri masih nightly-only, dan C \
                 `long double` bukan format 128-bit yang konsisten lintas x86-64 vs \
                 AArch64 — lihat komentar di classify_type untuk detail. Workaround: \
                 bungkus fungsi C-nya di shim kecil yang narrow ke f64/double.".to_string()),
            PrimTy::C64 | PrimTy::C128 => Err(
                "tipe complex number bukan tipe ABI-C standar, tidak didukung di FFI".to_string()),
            PrimTy::Rune => Err(
                "rune belum didukung sebagai tipe FFI langsung — gunakan i32/u32 \
                 untuk merepresentasikan code point Unicode-nya".to_string()),
            PrimTy::Str | PrimTy::String_ => Ok(FfiType::CStr),
            PrimTy::Unit => Ok(FfiType::Unit),
        },
        TypeExpr::Pointer(_) => Ok(FfiType::Ptr),
        TypeExpr::Ownership(o) => match o.qualifier {
            OwnershipQual::Raw => Ok(FfiType::Ptr),
            OwnershipQual::Owned => Err(
                "owned<T> tidak bisa langsung dipakai sebagai tipe FFI (FFI tidak tahu \
                 cara mengikuti aturan ownership JRG) — gunakan raw<T> atau pointer biasa \
                 (*const T / *mut T)".to_string()),
            OwnershipQual::Shared => Err(
                "shared<T> tidak bisa langsung dipakai sebagai tipe FFI — gunakan raw<T> \
                 atau pointer biasa".to_string()),
            OwnershipQual::Gc => Err(
                "gc<T> tidak bisa langsung dipakai sebagai tipe FFI — gunakan raw<T> \
                 atau pointer biasa".to_string()),
        },
        // Note: `str`/`string` are recognized as `PrimTy::Str`/ `PrimTy::String_`
        // directly above — `parse_type` always converts those two bare identifiers …
        TypeExpr::Never => Ok(FfiType::Unit),
        TypeExpr::Inferred => Err("tipe FFI tidak boleh `_` — harus dideklarasikan eksplisit".to_string()),
        TypeExpr::Array(a) => klasifikasi_array(&a.elem),
        TypeExpr::Slice(s) => klasifikasi_array(&s.elem),
        TypeExpr::Map(_)  => Err("map tidak didukung sebagai tipe FFI".to_string()),
        TypeExpr::Chan(_) => Err("channel tidak didukung sebagai tipe FFI".to_string()),
        TypeExpr::Fn(_) => Err(concat!(
            "pointer fungsi belum bisa jadi tipe FFI: menyerahkan fungsi JRG ke C ",
            "menuntut sebuah trampolin — alamat yang bisa dipanggil C dan yang di ",
            "dalamnya memanggil balik runtime JRG — dan membuatnya saat jalan berarti ",
            "membangkitkan kode. Yang SUDAH bisa memanggil balik: `py::` dan `js::`, ",
            "lewat protokol pekerjanya. Untuk pustaka C, bungkus di sisi C: simpan ",
            "fungsinya di sana, lalu panggil lewat `extern fn` biasa").to_string()),
        TypeExpr::Tuple(_) => Err("tuple tidak didukung sebagai tipe FFI".to_string()),
        TypeExpr::Named(n) => Err(format!(
            concat!(
                "struct `{}` belum bisa diserahkan ke C SECARA NILAI. Aturan ABI-nya ",
                "bergantung isi structnya — dua `int` muat di satu register, dua ",
                "`double` butuh dua register SSE, dan yang lebih besar dari 16 byte ",
                "lewat memori — jadi menebaknya bukan menghasilkan galat melainkan ",
                "angka yang kelihatan masuk akal. Dua jalan yang ada: kirim fieldnya ",
                "satu per satu (`fn f(x: i32, y: i32)`), atau kirim POINTER-nya ",
                "(`raw<{}>`) kalau fungsi C-nya memang menerima pointer"),
            n.name, n.name)),
        TypeExpr::Error(_) => Err("AST error node pada tipe FFI".to_string()),
    }
}

// ---------------------------------------------------------------------------
// ExternFnInfo — the Value payload of `Value::ExternFn`
// ---------------------------------------------------------------------------

/// What an `extern "C" { fn name(...) -> ...; }` declaration resolves to.
#[derive(Debug, Clone)]
pub struct ExternFnInfo {
    pub name: String,
    pub param_kinds: Vec<FfiType>,
    pub ret_kind: FfiType,
    /// `Ok(address)` once the library loaded and the symbol resolved
    /// successfully; `Err(reason)` if the library failed to load, the symbol …
    pub resolved: Result<usize, String>,
}

/// Builds the `ExternFnInfo` for one declared extern function — called once
/// per function by `Interpreter::register_extern_block` while hoisting an …
pub fn nama_simbol(
    abi: &str,
    name: &str,
    params: &[ExternParam],
) -> Result<String, String> {
    if !abi_cpp(abi) { return Ok(name.to_string()); }
    let mut tipe = Vec::with_capacity(params.len());
    for p in params {
        tipe.push(crate::ffi::cpp::tipe_cpp(&p.ty)?);
    }
    crate::ffi::cpp::mangle(name, &tipe)
}

/// Ejaan ABI C++ yang diterima.
pub fn penjelasan_abi(abi: &str) -> String {
    let dasar = format!(
        "ABI \"{}\" belum didukung oleh FFI runtime (yang ada: \"C\" dan \"C++\")",
        abi
    );
    match abi.to_ascii_lowercase().as_str() {
        "rust" => format!(
            "{}\n  bantuan: fungsi Rust terjangkau lewat `extern \"C\"` kalau di sisi Rust \
             ditulis `#[no_mangle] pub extern \"C\" fn ...`. Tanpa `#[no_mangle]`, nama \
             simbolnya memuat hash yang dihitung compiler dari metadata crate — itu tidak \
             bisa diturunkan dari tanda tangan, jadi bukan sesuatu yang menunggu diimplementasi",
            dasar
        ),
        "go" => format!(
            "{}\n  bantuan: fungsi Go terjangkau lewat `extern \"C\"` kalau paketnya dibangun \
             `go build -buildmode=c-shared` dan fungsinya ditandai `//export <nama>`",
            dasar
        ),
        "stdcall" | "system" | "fastcall" | "win64" => format!(
            "{}\n  catatan: konvensi pemanggilan Windows tidak berlaku di target ini",
            dasar
        ),
        _ => dasar,
    }
}

/// Ejaan ABI C++ yang diterima. Tiga ejaan karena ketiganya wajar ditulis,
/// dan menolak dua di antaranya cuma menghasilkan "ABI belum didukung" untuk
/// sesuatu yang jelas didukung.


pub fn abi_cpp(abi: &str) -> bool {
    matches!(abi, "C++" | "c++" | "cpp")
}

pub fn resolve_extern_fn(
    name: &str,
    params: &[ExternParam],
    ret: &Option<TypeExpr>,
    lib_outcome: &Result<usize, String>,
) -> ExternFnInfo {
    resolve_extern_fn_abi("C", name, params, ret, lib_outcome)
}

pub fn resolve_extern_fn_abi(
    abi: &str,
    name: &str,
    params: &[ExternParam],
    ret: &Option<TypeExpr>,
    lib_outcome: &Result<usize, String>,
) -> ExternFnInfo {
    selesai_extern(pra_extern(abi, name, params, ret), lib_outcome)
}

/// Bagian resolusi sebuah `extern fn` yang TIDAK butuh pustakanya: jenis tiap parameter, jenis
/// kembalian, dan nama simbol yang akan dicari. Backend LLVM menghitungnya saat kompilasi.
pub struct PraExtern {
    pub name: String,
    pub param_kinds: Vec<FfiType>,
    pub ret_kind: FfiType,
    pub galat: Option<String>,
    pub simbol: Result<String, String>,
    pub cpp: bool,
}

pub fn pra_extern(abi: &str, name: &str, params: &[ExternParam], ret: &Option<TypeExpr>) -> PraExtern {
    let mut param_kinds = Vec::with_capacity(params.len());
    let mut first_err: Option<String> = None;
    for p in params {
        match classify_type(&p.ty) {
            Ok(k) => param_kinds.push(k),
            Err(e) => { first_err = Some(format!("parameter tidak didukung: {}", e)); break; }
        }
    }
    let ret_kind = match ret {
        None => Ok(FfiType::Unit),
        // Sebuah array tidak bisa jadi tipe KEMBALIAN: yang dikembalikan C cuma
        // pointer, dan pointer tidak membawa panjangnya.
        Some(TypeExpr::Array(_)) | Some(TypeExpr::Slice(_)) => Err(concat!(
            "array tidak bisa jadi tipe kembalian: yang dikembalikan C cuma sebuah ",
            "pointer, dan pointer tidak membawa panjangnya — jadi tidak ada yang bisa ",
            "menentukan berapa elemen yang boleh dibaca. Jalannya: terima `raw<T>` lalu ",
            "panjangnya lewat parameter keluaran atau fungsi kedua, dan baca dengan `ptr::`"
        ).to_string()),
        Some(t) => classify_type(t),
    };
    let galat = if let Some(e) = first_err {
        Some(e)
    } else if let Err(e) = &ret_kind {
        Some(format!("tipe return tidak didukung: {}", e))
    } else if param_kinds.len() > 3 {
        Some(format!(
            "FFI v1 hanya mendukung maksimal 3 parameter per fungsi (`{}` punya {}) — \
             bungkus argumen tambahan dalam sebuah struct dan kirim sebagai satu pointer, \
             via shim C kecil, jika perlu",
            name, param_kinds.len()
        ))
    } else { None };
    let simbol = if galat.is_some() { Ok(name.to_string()) } else { nama_simbol(abi, name, params) };
    PraExtern {
        name: name.to_string(),
        param_kinds,
        ret_kind: ret_kind.unwrap_or(FfiType::Unit),
        galat,
        simbol,
        cpp: abi_cpp(abi),
    }
}

/// Tahap kedua: pustakanya (atau kegagalan memuatnya) sudah diketahui.
pub fn selesai_extern(pra: PraExtern, lib_outcome: &Result<usize, String>) -> ExternFnInfo {
    let resolved: Result<usize, String> = if let Some(e) = pra.galat {
        Err(e)
    } else {
        match lib_outcome {
            Err(e) => Err(format!("library gagal di-load: {}", e)),
            Ok(handle) => match pra.simbol {
                Err(e) => Err(e),
                Ok(simbol) => sys::symbol(*handle, &simbol).map_err(|e| {
                    if pra.cpp {
                        // Sebutkan simbol yang DICARI: kalau tanda tangannya meleset, yang salah
                        // bukan pustakanya, dan tanpa nama ini pembacanya tidak punya cara …
                        format!("{} (dicari sebagai simbol C++ `{}`; bandingkan dengan \
                                 keluaran `nm -D` pustakanya — `i64` di sini berarti \
                                 `long`, bukan `long long`)", e, simbol)
                    } else { e }
                }),
            },
        }
    };
    ExternFnInfo { name: pra.name, param_kinds: pra.param_kinds, ret_kind: pra.ret_kind, resolved }
}

/// Pustaka sebuah blok `extern`, dengan peringatan yang sama di tiap mesin kalau gagal dimuat.
pub fn muat_pustaka_blok(abi: &str, lib: Option<&str>) -> Result<usize, String> {
    let hasil = if abi != "C" && !abi_cpp(abi) { Err(penjelasan_abi(abi)) } else { load_library(lib) };
    if let Err(ref e) = hasil {
        crate::host::galat_baris(&format!(
            "peringatan: extern \"{}\"{} gagal di-load: {} (fungsi pada blok ini akan mengembalikan Err saat dipanggil, bukan menghentikan program)",
            abi, lib.map(|p| format!(" \"{}\"", p)).unwrap_or_default(), e,
        ));
    }
    hasil
}

const SEKAT: char = '\u{1f}';

impl FfiType {
    fn kode(self) -> String {
        match self {
            FfiType::Unit => "u".into(),
            FfiType::Bool => "b".into(),
            FfiType::Int { bits, signed } => format!("i{}{}", bits, if signed { 's' } else { 'n' }),
            FfiType::F32 => "f32".into(),
            FfiType::F64 => "f64".into(),
            FfiType::Ptr => "p".into(),
            FfiType::CStr => "c".into(),
            FfiType::Arr { bits, signed, float } =>
                format!("a{}{}{}", bits, if signed { 's' } else { 'n' }, if float { 'f' } else { 'b' }),
        }
    }

    fn dari_kode(k: &str) -> Option<FfiType> {
        Some(match k {
            "u" => FfiType::Unit,
            "b" => FfiType::Bool,
            "f32" => FfiType::F32,
            "f64" => FfiType::F64,
            "p" => FfiType::Ptr,
            "c" => FfiType::CStr,
            _ if k.starts_with('i') => {
                let (angka, tanda) = k[1..].split_at(k.len().checked_sub(2)?);
                FfiType::Int { bits: angka.parse().ok()?, signed: tanda == "s" }
            }
            _ if k.starts_with('a') => {
                let (angka, ekor) = k[1..].split_at(k.len().checked_sub(3)?);
                FfiType::Arr { bits: angka.parse().ok()?, signed: ekor.starts_with('s'), float: ekor.ends_with('f') }
            }
            _ => return None,
        })
    }
}

/// Sebuah `extern fn` sebagai SATU baris teks: yang backend LLVM tanam di binernya, supaya
/// pemanggilannya saat jalan lewat aturan yang sama persis dengan dua mesin lain.
pub fn deskripsi_extern(abi: &str, lib: Option<&str>, name: &str, params: &[ExternParam], ret: &Option<TypeExpr>) -> String {
    let pra = pra_extern(abi, name, params, ret);
    let jenis: Vec<String> = pra.param_kinds.iter().map(|k| k.kode()).collect();
    let (ada_simbol, simbol) = match pra.simbol { Ok(s) => ("1", s), Err(e) => ("0", e) };
    [
        abi, if lib.is_some() { "1" } else { "0" }, lib.unwrap_or(""), name, &jenis.join(","), &pra.ret_kind.kode(),
        if pra.galat.is_some() { "1" } else { "0" }, pra.galat.as_deref().unwrap_or(""), ada_simbol, &simbol,
    ].join(&SEKAT.to_string())
}

/// Kebalikan `deskripsi_extern`, ditambah tahap kedua (memuat pustaka dan mencari simbolnya).
pub fn dari_deskripsi(teks: &str) -> Result<ExternFnInfo, String> {
    let b: Vec<&str> = teks.split(SEKAT).collect();
    if b.len() != 10 { return Err("deskripsi `extern` rusak — ini bug di jrgc, bukan di programmu".to_string()); }
    let mut param_kinds = Vec::new();
    for k in b[4].split(',').filter(|k| !k.is_empty()) {
        param_kinds.push(FfiType::dari_kode(k).ok_or("deskripsi `extern` rusak: jenis parameter tak dikenal")?);
    }
    let pra = PraExtern {
        name: b[3].to_string(),
        param_kinds,
        ret_kind: FfiType::dari_kode(b[5]).ok_or("deskripsi `extern` rusak: jenis kembalian tak dikenal")?,
        galat: if b[6] == "1" { Some(b[7].to_string()) } else { None },
        simbol: if b[8] == "1" { Ok(b[9].to_string()) } else { Err(b[9].to_string()) },
        cpp: abi_cpp(b[0]),
    };
    let pustaka = muat_pustaka_blok(b[0], if b[1] == "1" { Some(b[2]) } else { None });
    Ok(selesai_extern(pra, &pustaka))
}

// ---------------------------------------------------------------------------
// Argument marshaling
// ---------------------------------------------------------------------------

/// One marshaled argument, already converted to its raw register-class
/// representation — either an integer-class 8-byte value (covering …
#[derive(Clone, Copy)]
enum CArg {
    I(i64),
    F(f64),
}

impl CArg {
    /// Reads this argument as `i64`.
    fn as_i(self) -> i64 {
        match self { CArg::I(v) => v, CArg::F(v) => v.to_bits() as i64 }
    }
    /// Reads this argument as `f64`. See `as_i`'s note on the
    /// mismatched-variant case.
    fn as_f(self) -> f64 {
        match self { CArg::F(v) => v, CArg::I(v) => f64::from_bits(v as u64) }
    }
}

/// Converts one JRG `Value` into its raw C-ABI representation according to
/// its declared `FfiType`.
fn marshal_arg(
    kind: FfiType,
    val: &Value,
    keep_alive: &mut Vec<CString>,
    buffers: &mut Vec<Vec<u64>>,
) -> Result<CArg, String> {
    match kind {
        FfiType::Bool => match val {
            Value::Bool(b) => Ok(CArg::I(*b as i64)),
            Value::Int(n)  => Ok(CArg::I((*n != 0) as i64)),
            other => Err(format!("butuh bool, dapat `{}`", other.type_name())),
        },
        FfiType::Int { .. } => match val {
            Value::Int(n)  => Ok(CArg::I(*n as i64)),
            Value::Bool(b) => Ok(CArg::I(*b as i64)),
            Value::Rune(c) => Ok(CArg::I(*c as i64)),
            other => Err(format!("butuh bilangan bulat, dapat `{}`", other.type_name())),
        },
        // f32 dan f64 sama-sama disimpan sebagai CArg::F(f64) di sini — nilai belum
        // di-narrow ke presisi f32 pada tahap ini.
        FfiType::F32 | FfiType::F64 => match val {
            Value::Float(f) => Ok(CArg::F(*f)),
            Value::Int(n)   => Ok(CArg::F(*n as f64)),
            other => Err(format!("butuh float, dapat `{}`", other.type_name())),
        },
        FfiType::Ptr => match val {
            Value::Ptr(addr) => Ok(CArg::I(*addr as i64)),
            Value::Nil       => Ok(CArg::I(0)),
            Value::Int(n)    => Ok(CArg::I(*n as i64)),
            other => Err(format!("butuh pointer (`ptr::` atau nil), dapat `{}`", other.type_name())),
        },
        FfiType::CStr => match val {
            Value::Str(s) => {
                let cs = CString::new(s.as_str())
                    .map_err(|_| format!(
                        "string mengandung byte NUL di tengah, tidak bisa dikirim sebagai \
                         C string (C string berakhir pada byte NUL pertama): {:?}", s))?;
                let ptr = cs.as_ptr() as i64;
                keep_alive.push(cs);
                Ok(CArg::I(ptr))
            }
            Value::Nil => Ok(CArg::I(0)),
            other => Err(format!("butuh string, dapat `{}`", other.type_name())),
        },
        FfiType::Arr { bits, signed, float } => match val {
            Value::Array(isi) => {
                let lebar = (bits / 8) as usize;
                let mut byte: Vec<u8> = Vec::with_capacity(isi.len() * lebar);
                for (i, v) in isi.iter().enumerate() {
                    let potong = elemen_ke_byte(v, bits, signed, float).map_err(|e| format!(
                        "elemen ke-{} tidak bisa dikirim: {}", i, e))?;
                    byte.extend_from_slice(&potong[..lebar]);
                }
                // Buffernya dialokasikan sebagai `Vec<u64>`, bukan `Vec<u8>`: yang
                // membacanya di sisi C bisa `double*` atau `int64_t*`, sementara `Vec<u8>` …
                let mut buf: Vec<u64> = Vec::with_capacity(byte.len() / 8 + 1);
                for potongan in byte.chunks(8) {
                    let mut w = [0u8; 8];
                    w[..potongan.len()].copy_from_slice(potongan);
                    buf.push(u64::from_ne_bytes(w));
                }
                // Array kosong tetap harus punya alamat yang sah: sebuah `Vec`
                // kosong menunjuk alamat dangling, dan pustaka C yang
                // memeriksa `p != NULL` lebih dulu akan salah membacanya.
                if buf.is_empty() { buf.push(0); }
                let ptr = buf.as_ptr() as i64;
                buffers.push(buf);
                Ok(CArg::I(ptr))
            }
            Value::Nil => Ok(CArg::I(0)),
            other => Err(format!("butuh array, dapat `{}`", other.type_name())),
        },
        FfiType::Unit => Err("unit/void tidak valid sebagai tipe parameter".to_string()),
    }
}

/// Satu elemen array jadi delapan byte little-endian; pemanggilnya mengambil
/// sebanyak lebar tipenya.
fn elemen_ke_byte(v: &Value, bits: u8, signed: bool, float: bool) -> Result<[u8; 8], String> {
    if float {
        let f = match v {
            Value::Float(f) => *f,
            Value::Int(n) => *n as f64,
            other => return Err(format!("butuh angka, dapat `{}`", other.type_name())),
        };
        return Ok(if bits == 32 {
            let mut b = [0u8; 8];
            b[..4].copy_from_slice(&(f as f32).to_ne_bytes());
            b
        } else {
            f.to_ne_bytes()
        });
    }
    let n = match v {
        Value::Int(n) => *n,
        Value::Bool(b) => *b as i128,
        Value::Rune(c) => *c as i128,
        other => return Err(format!("butuh bilangan bulat, dapat `{}`", other.type_name())),
    };
    let (min, maks): (i128, i128) = if signed {
        (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
    } else {
        (0, (1i128 << bits) - 1)
    };
    if n < min || n > maks {
        return Err(format!(
            "{} tidak muat di {}{} (jangkauannya {}..{})",
            n, if signed { "i" } else { "u" }, bits, min, maks));
    }
    // Dipotong ke lebarnya DULU, lalu ditulis dalam urutan byte NATIF —
    // jangkauannya sudah diperiksa di atas, jadi `as` di sini tidak membuang apa …
    Ok(match bits {
        8  => { let mut b = [0u8; 8]; b[..1].copy_from_slice(&(n as i8 ).to_ne_bytes()); b }
        16 => { let mut b = [0u8; 8]; b[..2].copy_from_slice(&(n as i16).to_ne_bytes()); b }
        32 => { let mut b = [0u8; 8]; b[..4].copy_from_slice(&(n as i32).to_ne_bytes()); b }
        _  => (n as i64).to_ne_bytes(),
    })
}

/// Truncates a raw 64-bit integer return value down to its true C width and
/// sign, then sign/zero-extends that back up to `i128` for `Value::Int`.
fn truncate_int_return(raw: i64, bits: u8, signed: bool) -> i128 {
    match (bits, signed) {
        (8,  true)  => (raw as i8)  as i128,
        (8,  false) => (raw as u8)  as i128,
        (16, true)  => (raw as i16) as i128,
        (16, false) => (raw as u16) as i128,
        (32, true)  => (raw as i32) as i128,
        (32, false) => (raw as u32) as i128,
        (_,  true)  => raw as i128,
        (_,  false) => (raw as u64) as i128,
    }
}

/// The raw result of a native call, before being converted into a JRG
/// `Value` according to the declared `FfiType` — see `raw_to_value`.
enum CRawRet {
    Void,
    I(i64),
    F(f64),
}

/// Converts a `CRawRet` into the `Value` a JRG program actually sees,
/// according to the function's declared return `FfiType`.
fn raw_to_value(raw: CRawRet, kind: FfiType) -> Value {
    match kind {
        FfiType::Unit => Value::Unit,
        FfiType::Bool => match raw {
            CRawRet::I(r) => Value::Bool((r as u64 & 0xFF) != 0),
            _ => Value::Unit,
        },
        FfiType::Int { bits, signed } => match raw {
            CRawRet::I(r) => Value::Int(truncate_int_return(r, bits, signed)),
            _ => Value::Unit,
        },
        FfiType::Ptr => match raw {
            CRawRet::I(0) => Value::Nil,
            CRawRet::I(r) => Value::Ptr(r as usize),
            _ => Value::Unit,
        },
        // Ditolak sebagai tipe kembalian di `resolve_extern_fn_abi`, jadi lengan
        // ini tidak terjangkau — tapi ditulis supaya penambahan varian
        // berikutnya tetap dipaksa kompilator memikirkannya.
        FfiType::Arr { .. } => Value::Unit,
        FfiType::CStr => match raw {
            CRawRet::I(0) => Value::Nil,
            CRawRet::I(r) => {
                // Safety relies on the callee having returned a genuine NUL-terminated C
                // string, which is the documented contract of every standard libc function …
                let cstr = unsafe { std::ffi::CStr::from_ptr(r as *const c_char) };
                Value::Str(cstr.to_string_lossy().into_owned())
            }
            _ => Value::Unit,
        },
        // f32 dan f64 kembali sebagai Value::Float yang sama (Value hanya punya satu
        // varian float, f64) — widening f32 -> f64 di dispatch_call sebelum sampai …
        FfiType::F32 | FfiType::F64 => match raw {
            CRawRet::F(f) => Value::Float(f),
            _ => Value::Unit,
        },
    }
}

// ---------------------------------------------------------------------------
// The native call dispatcher
// ---------------------------------------------------------------------------

/// Calls the real, native function at `addr`, which must already have been
/// resolved via `sys::symbol`.
fn dispatch_call(
    addr: usize,
    param_kinds: &[FfiType],
    cargs: &[CArg],
    ret_kind: FfiType,
) -> Result<CRawRet, String> {
    #[derive(Clone, Copy, PartialEq)]
    enum Slot { I, F32, F64 }
    #[derive(Clone, Copy, PartialEq)]
    enum RetSlot { Void, I, F32, F64 }

    let slots: Vec<Slot> = param_kinds.iter()
        .map(|k| match k {
            FfiType::F32 => Slot::F32,
            FfiType::F64 => Slot::F64,
            _ => Slot::I,
        })
        .collect();
    let ret_slot = match ret_kind {
        FfiType::Unit => RetSlot::Void,
        FfiType::F32  => RetSlot::F32,
        FfiType::F64  => RetSlot::F64,
        _ => RetSlot::I,
    };

    // The macro itself is the only place an `unsafe { transmute(...) }` happens;
    // every match arm below just supplies the concrete parameter/return Rust …
    macro_rules! ffi_arm {
        ( ($($pty:ty),*) -> $rty:ty, $wrap:expr, ($($aexpr:expr),*) ) => {{
            type F = extern "C" fn($($pty),*) -> $rty;
            let f: F = unsafe { std::mem::transmute::<usize, F>(addr) };
            let __ret = f($($aexpr),*);
            Ok($wrap(__ret))
        }};
        ( ($($pty:ty),*), ($($aexpr:expr),*) ) => {{
            type F = extern "C" fn($($pty),*);
            let f: F = unsafe { std::mem::transmute::<usize, F>(addr) };
            f($($aexpr),*);
            Ok(CRawRet::Void)
        }};
    }

    match (slots.as_slice(), ret_slot) {
        // ── Arity 0 — 1 shape × 4 returns = 4 arms ───────────────────
        ([], RetSlot::Void) => ffi_arm!((), ()),
        ([], RetSlot::I) => ffi_arm!(() -> i64, CRawRet::I, ()),
        ([], RetSlot::F32) => ffi_arm!(() -> f32, |r: f32| CRawRet::F(r as f64), ()),
        ([], RetSlot::F64) => ffi_arm!(() -> f64, CRawRet::F, ()),

        // ── Arity 1 — 3 shapes × 4 returns = 12 arms ──────────────────
        ([Slot::I], RetSlot::Void) => { let a0 = cargs[0].as_i();
            ffi_arm!((i64), (a0)) }
        ([Slot::I], RetSlot::I) => { let a0 = cargs[0].as_i();
            ffi_arm!((i64) -> i64, CRawRet::I, (a0)) }
        ([Slot::I], RetSlot::F32) => { let a0 = cargs[0].as_i();
            ffi_arm!((i64) -> f32, |r: f32| CRawRet::F(r as f64), (a0)) }
        ([Slot::I], RetSlot::F64) => { let a0 = cargs[0].as_i();
            ffi_arm!((i64) -> f64, CRawRet::F, (a0)) }
        ([Slot::F32], RetSlot::Void) => { let a0 = cargs[0].as_f() as f32;
            ffi_arm!((f32), (a0)) }
        ([Slot::F32], RetSlot::I) => { let a0 = cargs[0].as_f() as f32;
            ffi_arm!((f32) -> i64, CRawRet::I, (a0)) }
        ([Slot::F32], RetSlot::F32) => { let a0 = cargs[0].as_f() as f32;
            ffi_arm!((f32) -> f32, |r: f32| CRawRet::F(r as f64), (a0)) }
        ([Slot::F32], RetSlot::F64) => { let a0 = cargs[0].as_f() as f32;
            ffi_arm!((f32) -> f64, CRawRet::F, (a0)) }
        ([Slot::F64], RetSlot::Void) => { let a0 = cargs[0].as_f();
            ffi_arm!((f64), (a0)) }
        ([Slot::F64], RetSlot::I) => { let a0 = cargs[0].as_f();
            ffi_arm!((f64) -> i64, CRawRet::I, (a0)) }
        ([Slot::F64], RetSlot::F32) => { let a0 = cargs[0].as_f();
            ffi_arm!((f64) -> f32, |r: f32| CRawRet::F(r as f64), (a0)) }
        ([Slot::F64], RetSlot::F64) => { let a0 = cargs[0].as_f();
            ffi_arm!((f64) -> f64, CRawRet::F, (a0)) }

        // ── Arity 2 — 9 shapes × 4 returns = 36 arms ──────────────────
        ([Slot::I, Slot::I], RetSlot::Void) => { let a0 = cargs[0].as_i(); let a1 = cargs[1].as_i();
            ffi_arm!((i64, i64), (a0, a1)) }
        ([Slot::I, Slot::I], RetSlot::I) => { let a0 = cargs[0].as_i(); let a1 = cargs[1].as_i();
            ffi_arm!((i64, i64) -> i64, CRawRet::I, (a0, a1)) }
        ([Slot::I, Slot::I], RetSlot::F32) => { let a0 = cargs[0].as_i(); let a1 = cargs[1].as_i();
            ffi_arm!((i64, i64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1)) }
        ([Slot::I, Slot::I], RetSlot::F64) => { let a0 = cargs[0].as_i(); let a1 = cargs[1].as_i();
            ffi_arm!((i64, i64) -> f64, CRawRet::F, (a0, a1)) }
        ([Slot::I, Slot::F32], RetSlot::Void) => { let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f() as f32;
            ffi_arm!((i64, f32), (a0, a1)) }
        ([Slot::I, Slot::F32], RetSlot::I) => { let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f() as f32;
            ffi_arm!((i64, f32) -> i64, CRawRet::I, (a0, a1)) }
        ([Slot::I, Slot::F32], RetSlot::F32) => { let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f() as f32;
            ffi_arm!((i64, f32) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1)) }
        ([Slot::I, Slot::F32], RetSlot::F64) => { let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f() as f32;
            ffi_arm!((i64, f32) -> f64, CRawRet::F, (a0, a1)) }
        ([Slot::I, Slot::F64], RetSlot::Void) => { let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f();
            ffi_arm!((i64, f64), (a0, a1)) }
        ([Slot::I, Slot::F64], RetSlot::I) => { let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f();
            ffi_arm!((i64, f64) -> i64, CRawRet::I, (a0, a1)) }
        ([Slot::I, Slot::F64], RetSlot::F32) => { let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f();
            ffi_arm!((i64, f64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1)) }
        ([Slot::I, Slot::F64], RetSlot::F64) => { let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f();
            ffi_arm!((i64, f64) -> f64, CRawRet::F, (a0, a1)) }
        ([Slot::F32, Slot::I], RetSlot::Void) => { let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_i();
            ffi_arm!((f32, i64), (a0, a1)) }
        ([Slot::F32, Slot::I], RetSlot::I) => { let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_i();
            ffi_arm!((f32, i64) -> i64, CRawRet::I, (a0, a1)) }
        ([Slot::F32, Slot::I], RetSlot::F32) => { let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_i();
            ffi_arm!((f32, i64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1)) }
        ([Slot::F32, Slot::I], RetSlot::F64) => { let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_i();
            ffi_arm!((f32, i64) -> f64, CRawRet::F, (a0, a1)) }
        ([Slot::F32, Slot::F32], RetSlot::Void) => { let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f() as f32;
            ffi_arm!((f32, f32), (a0, a1)) }
        ([Slot::F32, Slot::F32], RetSlot::I) => { let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f() as f32;
            ffi_arm!((f32, f32) -> i64, CRawRet::I, (a0, a1)) }
        ([Slot::F32, Slot::F32], RetSlot::F32) => { let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f() as f32;
            ffi_arm!((f32, f32) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1)) }
        ([Slot::F32, Slot::F32], RetSlot::F64) => { let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f() as f32;
            ffi_arm!((f32, f32) -> f64, CRawRet::F, (a0, a1)) }
        ([Slot::F32, Slot::F64], RetSlot::Void) => { let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f();
            ffi_arm!((f32, f64), (a0, a1)) }
        ([Slot::F32, Slot::F64], RetSlot::I) => { let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f();
            ffi_arm!((f32, f64) -> i64, CRawRet::I, (a0, a1)) }
        ([Slot::F32, Slot::F64], RetSlot::F32) => { let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f();
            ffi_arm!((f32, f64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1)) }
        ([Slot::F32, Slot::F64], RetSlot::F64) => { let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f();
            ffi_arm!((f32, f64) -> f64, CRawRet::F, (a0, a1)) }
        ([Slot::F64, Slot::I], RetSlot::Void) => { let a0 = cargs[0].as_f(); let a1 = cargs[1].as_i();
            ffi_arm!((f64, i64), (a0, a1)) }
        ([Slot::F64, Slot::I], RetSlot::I) => { let a0 = cargs[0].as_f(); let a1 = cargs[1].as_i();
            ffi_arm!((f64, i64) -> i64, CRawRet::I, (a0, a1)) }
        ([Slot::F64, Slot::I], RetSlot::F32) => { let a0 = cargs[0].as_f(); let a1 = cargs[1].as_i();
            ffi_arm!((f64, i64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1)) }
        ([Slot::F64, Slot::I], RetSlot::F64) => { let a0 = cargs[0].as_f(); let a1 = cargs[1].as_i();
            ffi_arm!((f64, i64) -> f64, CRawRet::F, (a0, a1)) }
        ([Slot::F64, Slot::F32], RetSlot::Void) => { let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f() as f32;
            ffi_arm!((f64, f32), (a0, a1)) }
        ([Slot::F64, Slot::F32], RetSlot::I) => { let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f() as f32;
            ffi_arm!((f64, f32) -> i64, CRawRet::I, (a0, a1)) }
        ([Slot::F64, Slot::F32], RetSlot::F32) => { let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f() as f32;
            ffi_arm!((f64, f32) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1)) }
        ([Slot::F64, Slot::F32], RetSlot::F64) => { let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f() as f32;
            ffi_arm!((f64, f32) -> f64, CRawRet::F, (a0, a1)) }
        ([Slot::F64, Slot::F64], RetSlot::Void) => { let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f();
            ffi_arm!((f64, f64), (a0, a1)) }
        ([Slot::F64, Slot::F64], RetSlot::I) => { let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f();
            ffi_arm!((f64, f64) -> i64, CRawRet::I, (a0, a1)) }
        ([Slot::F64, Slot::F64], RetSlot::F32) => { let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f();
            ffi_arm!((f64, f64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1)) }
        ([Slot::F64, Slot::F64], RetSlot::F64) => { let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f();
            ffi_arm!((f64, f64) -> f64, CRawRet::F, (a0, a1)) }

        // ── Arity 3 — 27 shapes × 4 returns = 108 arms ─────────────────
        ([Slot::I, Slot::I, Slot::I], RetSlot::Void) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_i();
            ffi_arm!((i64, i64, i64), (a0, a1, a2)) }
        ([Slot::I, Slot::I, Slot::I], RetSlot::I) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_i();
            ffi_arm!((i64, i64, i64) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::I, Slot::I, Slot::I], RetSlot::F32) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_i();
            ffi_arm!((i64, i64, i64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::I, Slot::I, Slot::I], RetSlot::F64) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_i();
            ffi_arm!((i64, i64, i64) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::I, Slot::I, Slot::F32], RetSlot::Void) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((i64, i64, f32), (a0, a1, a2)) }
        ([Slot::I, Slot::I, Slot::F32], RetSlot::I) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((i64, i64, f32) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::I, Slot::I, Slot::F32], RetSlot::F32) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((i64, i64, f32) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::I, Slot::I, Slot::F32], RetSlot::F64) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((i64, i64, f32) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::I, Slot::I, Slot::F64], RetSlot::Void) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f();
            ffi_arm!((i64, i64, f64), (a0, a1, a2)) }
        ([Slot::I, Slot::I, Slot::F64], RetSlot::I) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f();
            ffi_arm!((i64, i64, f64) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::I, Slot::I, Slot::F64], RetSlot::F32) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f();
            ffi_arm!((i64, i64, f64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::I, Slot::I, Slot::F64], RetSlot::F64) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f();
            ffi_arm!((i64, i64, f64) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::I, Slot::F32, Slot::I], RetSlot::Void) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_i();
            ffi_arm!((i64, f32, i64), (a0, a1, a2)) }
        ([Slot::I, Slot::F32, Slot::I], RetSlot::I) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_i();
            ffi_arm!((i64, f32, i64) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::I, Slot::F32, Slot::I], RetSlot::F32) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_i();
            ffi_arm!((i64, f32, i64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::I, Slot::F32, Slot::I], RetSlot::F64) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_i();
            ffi_arm!((i64, f32, i64) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::I, Slot::F32, Slot::F32], RetSlot::Void) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f() as f32;
            ffi_arm!((i64, f32, f32), (a0, a1, a2)) }
        ([Slot::I, Slot::F32, Slot::F32], RetSlot::I) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f() as f32;
            ffi_arm!((i64, f32, f32) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::I, Slot::F32, Slot::F32], RetSlot::F32) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f() as f32;
            ffi_arm!((i64, f32, f32) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::I, Slot::F32, Slot::F32], RetSlot::F64) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f() as f32;
            ffi_arm!((i64, f32, f32) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::I, Slot::F32, Slot::F64], RetSlot::Void) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f();
            ffi_arm!((i64, f32, f64), (a0, a1, a2)) }
        ([Slot::I, Slot::F32, Slot::F64], RetSlot::I) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f();
            ffi_arm!((i64, f32, f64) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::I, Slot::F32, Slot::F64], RetSlot::F32) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f();
            ffi_arm!((i64, f32, f64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::I, Slot::F32, Slot::F64], RetSlot::F64) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f();
            ffi_arm!((i64, f32, f64) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::I, Slot::F64, Slot::I], RetSlot::Void) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_i();
            ffi_arm!((i64, f64, i64), (a0, a1, a2)) }
        ([Slot::I, Slot::F64, Slot::I], RetSlot::I) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_i();
            ffi_arm!((i64, f64, i64) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::I, Slot::F64, Slot::I], RetSlot::F32) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_i();
            ffi_arm!((i64, f64, i64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::I, Slot::F64, Slot::I], RetSlot::F64) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_i();
            ffi_arm!((i64, f64, i64) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::I, Slot::F64, Slot::F32], RetSlot::Void) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((i64, f64, f32), (a0, a1, a2)) }
        ([Slot::I, Slot::F64, Slot::F32], RetSlot::I) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((i64, f64, f32) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::I, Slot::F64, Slot::F32], RetSlot::F32) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((i64, f64, f32) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::I, Slot::F64, Slot::F32], RetSlot::F64) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((i64, f64, f32) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::I, Slot::F64, Slot::F64], RetSlot::Void) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f();
            ffi_arm!((i64, f64, f64), (a0, a1, a2)) }
        ([Slot::I, Slot::F64, Slot::F64], RetSlot::I) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f();
            ffi_arm!((i64, f64, f64) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::I, Slot::F64, Slot::F64], RetSlot::F32) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f();
            ffi_arm!((i64, f64, f64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::I, Slot::F64, Slot::F64], RetSlot::F64) => {
            let a0 = cargs[0].as_i(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f();
            ffi_arm!((i64, f64, f64) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::F32, Slot::I, Slot::I], RetSlot::Void) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_i(); let a2 = cargs[2].as_i();
            ffi_arm!((f32, i64, i64), (a0, a1, a2)) }
        ([Slot::F32, Slot::I, Slot::I], RetSlot::I) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_i(); let a2 = cargs[2].as_i();
            ffi_arm!((f32, i64, i64) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::F32, Slot::I, Slot::I], RetSlot::F32) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_i(); let a2 = cargs[2].as_i();
            ffi_arm!((f32, i64, i64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::F32, Slot::I, Slot::I], RetSlot::F64) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_i(); let a2 = cargs[2].as_i();
            ffi_arm!((f32, i64, i64) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::F32, Slot::I, Slot::F32], RetSlot::Void) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f32, i64, f32), (a0, a1, a2)) }
        ([Slot::F32, Slot::I, Slot::F32], RetSlot::I) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f32, i64, f32) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::F32, Slot::I, Slot::F32], RetSlot::F32) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f32, i64, f32) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::F32, Slot::I, Slot::F32], RetSlot::F64) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f32, i64, f32) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::F32, Slot::I, Slot::F64], RetSlot::Void) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f();
            ffi_arm!((f32, i64, f64), (a0, a1, a2)) }
        ([Slot::F32, Slot::I, Slot::F64], RetSlot::I) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f();
            ffi_arm!((f32, i64, f64) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::F32, Slot::I, Slot::F64], RetSlot::F32) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f();
            ffi_arm!((f32, i64, f64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::F32, Slot::I, Slot::F64], RetSlot::F64) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f();
            ffi_arm!((f32, i64, f64) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::F32, Slot::F32, Slot::I], RetSlot::Void) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_i();
            ffi_arm!((f32, f32, i64), (a0, a1, a2)) }
        ([Slot::F32, Slot::F32, Slot::I], RetSlot::I) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_i();
            ffi_arm!((f32, f32, i64) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::F32, Slot::F32, Slot::I], RetSlot::F32) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_i();
            ffi_arm!((f32, f32, i64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::F32, Slot::F32, Slot::I], RetSlot::F64) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_i();
            ffi_arm!((f32, f32, i64) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::F32, Slot::F32, Slot::F32], RetSlot::Void) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f32, f32, f32), (a0, a1, a2)) }
        ([Slot::F32, Slot::F32, Slot::F32], RetSlot::I) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f32, f32, f32) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::F32, Slot::F32, Slot::F32], RetSlot::F32) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f32, f32, f32) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::F32, Slot::F32, Slot::F32], RetSlot::F64) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f32, f32, f32) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::F32, Slot::F32, Slot::F64], RetSlot::Void) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f();
            ffi_arm!((f32, f32, f64), (a0, a1, a2)) }
        ([Slot::F32, Slot::F32, Slot::F64], RetSlot::I) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f();
            ffi_arm!((f32, f32, f64) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::F32, Slot::F32, Slot::F64], RetSlot::F32) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f();
            ffi_arm!((f32, f32, f64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::F32, Slot::F32, Slot::F64], RetSlot::F64) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f();
            ffi_arm!((f32, f32, f64) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::F32, Slot::F64, Slot::I], RetSlot::Void) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f(); let a2 = cargs[2].as_i();
            ffi_arm!((f32, f64, i64), (a0, a1, a2)) }
        ([Slot::F32, Slot::F64, Slot::I], RetSlot::I) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f(); let a2 = cargs[2].as_i();
            ffi_arm!((f32, f64, i64) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::F32, Slot::F64, Slot::I], RetSlot::F32) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f(); let a2 = cargs[2].as_i();
            ffi_arm!((f32, f64, i64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::F32, Slot::F64, Slot::I], RetSlot::F64) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f(); let a2 = cargs[2].as_i();
            ffi_arm!((f32, f64, i64) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::F32, Slot::F64, Slot::F32], RetSlot::Void) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f32, f64, f32), (a0, a1, a2)) }
        ([Slot::F32, Slot::F64, Slot::F32], RetSlot::I) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f32, f64, f32) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::F32, Slot::F64, Slot::F32], RetSlot::F32) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f32, f64, f32) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::F32, Slot::F64, Slot::F32], RetSlot::F64) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f32, f64, f32) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::F32, Slot::F64, Slot::F64], RetSlot::Void) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f();
            ffi_arm!((f32, f64, f64), (a0, a1, a2)) }
        ([Slot::F32, Slot::F64, Slot::F64], RetSlot::I) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f();
            ffi_arm!((f32, f64, f64) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::F32, Slot::F64, Slot::F64], RetSlot::F32) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f();
            ffi_arm!((f32, f64, f64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::F32, Slot::F64, Slot::F64], RetSlot::F64) => {
            let a0 = cargs[0].as_f() as f32; let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f();
            ffi_arm!((f32, f64, f64) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::F64, Slot::I, Slot::I], RetSlot::Void) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_i();
            ffi_arm!((f64, i64, i64), (a0, a1, a2)) }
        ([Slot::F64, Slot::I, Slot::I], RetSlot::I) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_i();
            ffi_arm!((f64, i64, i64) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::F64, Slot::I, Slot::I], RetSlot::F32) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_i();
            ffi_arm!((f64, i64, i64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::F64, Slot::I, Slot::I], RetSlot::F64) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_i();
            ffi_arm!((f64, i64, i64) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::F64, Slot::I, Slot::F32], RetSlot::Void) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f64, i64, f32), (a0, a1, a2)) }
        ([Slot::F64, Slot::I, Slot::F32], RetSlot::I) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f64, i64, f32) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::F64, Slot::I, Slot::F32], RetSlot::F32) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f64, i64, f32) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::F64, Slot::I, Slot::F32], RetSlot::F64) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f64, i64, f32) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::F64, Slot::I, Slot::F64], RetSlot::Void) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f();
            ffi_arm!((f64, i64, f64), (a0, a1, a2)) }
        ([Slot::F64, Slot::I, Slot::F64], RetSlot::I) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f();
            ffi_arm!((f64, i64, f64) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::F64, Slot::I, Slot::F64], RetSlot::F32) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f();
            ffi_arm!((f64, i64, f64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::F64, Slot::I, Slot::F64], RetSlot::F64) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_i(); let a2 = cargs[2].as_f();
            ffi_arm!((f64, i64, f64) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::F64, Slot::F32, Slot::I], RetSlot::Void) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_i();
            ffi_arm!((f64, f32, i64), (a0, a1, a2)) }
        ([Slot::F64, Slot::F32, Slot::I], RetSlot::I) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_i();
            ffi_arm!((f64, f32, i64) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::F64, Slot::F32, Slot::I], RetSlot::F32) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_i();
            ffi_arm!((f64, f32, i64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::F64, Slot::F32, Slot::I], RetSlot::F64) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_i();
            ffi_arm!((f64, f32, i64) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::F64, Slot::F32, Slot::F32], RetSlot::Void) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f64, f32, f32), (a0, a1, a2)) }
        ([Slot::F64, Slot::F32, Slot::F32], RetSlot::I) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f64, f32, f32) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::F64, Slot::F32, Slot::F32], RetSlot::F32) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f64, f32, f32) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::F64, Slot::F32, Slot::F32], RetSlot::F64) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f64, f32, f32) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::F64, Slot::F32, Slot::F64], RetSlot::Void) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f();
            ffi_arm!((f64, f32, f64), (a0, a1, a2)) }
        ([Slot::F64, Slot::F32, Slot::F64], RetSlot::I) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f();
            ffi_arm!((f64, f32, f64) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::F64, Slot::F32, Slot::F64], RetSlot::F32) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f();
            ffi_arm!((f64, f32, f64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::F64, Slot::F32, Slot::F64], RetSlot::F64) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f() as f32; let a2 = cargs[2].as_f();
            ffi_arm!((f64, f32, f64) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::F64, Slot::F64, Slot::I], RetSlot::Void) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_i();
            ffi_arm!((f64, f64, i64), (a0, a1, a2)) }
        ([Slot::F64, Slot::F64, Slot::I], RetSlot::I) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_i();
            ffi_arm!((f64, f64, i64) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::F64, Slot::F64, Slot::I], RetSlot::F32) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_i();
            ffi_arm!((f64, f64, i64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::F64, Slot::F64, Slot::I], RetSlot::F64) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_i();
            ffi_arm!((f64, f64, i64) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::F64, Slot::F64, Slot::F32], RetSlot::Void) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f64, f64, f32), (a0, a1, a2)) }
        ([Slot::F64, Slot::F64, Slot::F32], RetSlot::I) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f64, f64, f32) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::F64, Slot::F64, Slot::F32], RetSlot::F32) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f64, f64, f32) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::F64, Slot::F64, Slot::F32], RetSlot::F64) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f() as f32;
            ffi_arm!((f64, f64, f32) -> f64, CRawRet::F, (a0, a1, a2)) }
        ([Slot::F64, Slot::F64, Slot::F64], RetSlot::Void) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f();
            ffi_arm!((f64, f64, f64), (a0, a1, a2)) }
        ([Slot::F64, Slot::F64, Slot::F64], RetSlot::I) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f();
            ffi_arm!((f64, f64, f64) -> i64, CRawRet::I, (a0, a1, a2)) }
        ([Slot::F64, Slot::F64, Slot::F64], RetSlot::F32) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f();
            ffi_arm!((f64, f64, f64) -> f32, |r: f32| CRawRet::F(r as f64), (a0, a1, a2)) }
        ([Slot::F64, Slot::F64, Slot::F64], RetSlot::F64) => {
            let a0 = cargs[0].as_f(); let a1 = cargs[1].as_f(); let a2 = cargs[2].as_f();
            ffi_arm!((f64, f64, f64) -> f64, CRawRet::F, (a0, a1, a2)) }

        (other, _) => Err(format!(
            "FFI: {} parameter tidak didukung (maksimal 3 parameter per fungsi)",
            other.len()
        )),
    }
}

// ---------------------------------------------------------------------------
// Public entry point — called from `Interpreter::call_fn`
// ---------------------------------------------------------------------------

/// Calls an already-registered extern function.
pub fn call_extern_fn(info: &ExternFnInfo, args: Vec<Value>) -> EvalResult {
    let addr = match &info.resolved {
        Ok(addr) => *addr,
        Err(reason) => return Ok(err_value(format!(
            "extern fn `{}` tidak bisa dipanggil: {}", info.name, reason
        ))),
    };
    if args.len() != info.param_kinds.len() {
        return Ok(err_value(format!(
            "extern fn `{}`: butuh {} argumen, diberi {}",
            info.name, info.param_kinds.len(), args.len()
        )));
    }
    let mut keep_alive: Vec<CString> = Vec::new();
    // Buffer array hidup sampai panggilannya selesai: yang diserahkan cuma
    // alamatnya, jadi membebaskannya lebih awal berarti C membaca memori yang …
    let mut buffers: Vec<Vec<u64>> = Vec::new();
    let mut cargs: Vec<CArg> = Vec::with_capacity(args.len());
    for (kind, val) in info.param_kinds.iter().zip(args.iter()) {
        match marshal_arg(*kind, val, &mut keep_alive, &mut buffers) {
            Ok(c) => cargs.push(c),
            Err(e) => return Ok(err_value(format!(
                "extern fn `{}`: argumen tidak valid: {}", info.name, e
            ))),
        }
    }
    let raw = dispatch_call(addr, &info.param_kinds, &cargs, info.ret_kind);
    // `keep_alive`'s `CString`s must outlive the actual native call — which has
    // just happened, synchronously, inside `dispatch_call` — so they are dropped …
    drop(keep_alive);
    match raw {
        Ok(r)  => Ok(ok_value(raw_to_value(r, info.ret_kind))),
        Err(e) => Ok(err_value(format!("extern fn `{}` gagal dipanggil: {}", info.name, e))),
    }
}
