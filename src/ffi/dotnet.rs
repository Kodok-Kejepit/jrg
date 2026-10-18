// src/ffi/dotnet.rs
// Memanggil .NET lewat hosting API-nya, tanpa crate baru.

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::sync::{Mutex, OnceLock};

use crate::ffi::native::{classify_type, call_extern_fn, ExternFnInfo};
use crate::interpreter::{err_value, ok_value, Value};
use crate::parser::{prim_dari_nama, TypeExpr};

/// `hostfxr_delegate_type::hdt_load_assembly_and_get_function_pointer`,
/// dibaca dari urutan enum di `hostfxr.h` (com_activation=0 …
const HDT_LOAD_ASSEMBLY: c_int = 5;

/// `UNMANAGEDCALLERSONLY_METHOD` di `coreclr_delegates.h` adalah
/// `((const char_t*)-1)` — sebuah penanda, bukan alamat.
const UNMANAGED_CALLERS_ONLY: *const c_char = usize::MAX as *const c_char;

type Handle = *mut c_void;
type LoadFn = extern "C" fn(*const c_char, *const c_char, *const c_char,
                           *const c_char, *mut c_void, *mut *mut c_void) -> c_int;

struct Runtime { ctx: Handle, load: usize }
// `hostfxr` dirancang untuk dipakai dari banyak thread; yang disimpan di sini
// cuma handle konteks dan alamat delegate-nya.
unsafe impl Send for Runtime {}

static RUNTIME: OnceLock<Mutex<Option<Runtime>>> = OnceLock::new();

fn simpanan() -> &'static Mutex<Option<Runtime>> {
    RUNTIME.get_or_init(|| Mutex::new(None))
}

/// Cari `libhostfxr.so` yang terpasang.
fn cari_hostfxr() -> Option<String> {
    let mut akar: Vec<String> = Vec::new();
    if let Ok(r) = std::env::var("DOTNET_ROOT") { akar.push(r); }
    for a in ["/usr/share/dotnet", "/usr/lib/dotnet", "/opt/dotnet"] {
        akar.push(a.to_string());
    }
    for a in akar {
        let fxr = format!("{}/host/fxr", a.trim_end_matches('/'));
        let Ok(baca) = std::fs::read_dir(&fxr) else { continue };
        let mut versi: Vec<std::path::PathBuf> =
            baca.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        versi.sort();
        for v in versi.into_iter().rev() {
            let p = v.join("libhostfxr.so");
            if p.exists() { return Some(p.display().to_string()); }
            let d = v.join("libhostfxr.dylib");
            if d.exists() { return Some(d.display().to_string()); }
        }
    }
    None
}

pub fn alasan_tidak_ada() -> String {
    concat!(
        "libhostfxr tidak ketemu — pasang runtime .NET-nya dulu: Debian/Ubuntu/Kali ",
        "`apt install dotnet-sdk-8.0`, Fedora `dnf install dotnet-sdk-8.0`, Arch ",
        "`pacman -S dotnet-sdk`. Kalau .NET-nya ada di tempat yang tidak biasa, ",
        "set `DOTNET_ROOT`"
    ).to_string()
}

pub fn tersedia() -> bool { cari_hostfxr().is_some() }

/// Nyalakan runtime dari sebuah `.runtimeconfig.json`.
pub fn mulai(config: &str) -> Result<(), String> {
    let mut kunci = simpanan().lock().map_err(|_| "kunci runtime .NET rusak".to_string())?;
    if kunci.is_some() { return Ok(()); }

    if !std::path::Path::new(config).exists() {
        return Err(format!(
            "`{}` tidak ada — yang dibutuhkan berkas `.runtimeconfig.json` milik \
             assembly-nya, yang dibuat `dotnet build` di sebelah `.dll`-nya", config));
    }
    let path = cari_hostfxr().ok_or_else(alasan_tidak_ada)?;
    let lib = crate::ffi::native::load_library(Some(&path))
        .map_err(|e| format!("libhostfxr ada di `{}` tapi gagal dimuat: {}", path, e))?;

    let a_init = crate::ffi::native::resolve_symbol(lib, "hostfxr_initialize_for_runtime_config")
        .map_err(|e| format!("`hostfxr_initialize_for_runtime_config` tidak ada: {}", e))?;
    let a_deleg = crate::ffi::native::resolve_symbol(lib, "hostfxr_get_runtime_delegate")
        .map_err(|e| format!("`hostfxr_get_runtime_delegate` tidak ada: {}", e))?;

    let c_config = CString::new(config).map_err(|_| "path config memuat byte NUL".to_string())?;
    let mut ctx: Handle = std::ptr::null_mut();
    let rc = unsafe {
        let f: extern "C" fn(*const c_char, *mut c_void, *mut Handle) -> c_int =
            std::mem::transmute(a_init);
        f(c_config.as_ptr(), std::ptr::null_mut(), &mut ctx)
    };
    // hostfxr memakai HRESULT: 0 sukses, dan `Success_HostAlreadyInitialized`
    // (0x00000001) juga sukses.
    if (rc != 0 && rc != 1) || ctx.is_null() {
        return Err(format!(
            "menyalakan runtime .NET dari `{}` gagal (kode 0x{:x})", config, rc));
    }
    let mut deleg: *mut c_void = std::ptr::null_mut();
    let rc = unsafe {
        let f: extern "C" fn(Handle, c_int, *mut *mut c_void) -> c_int =
            std::mem::transmute(a_deleg);
        f(ctx, HDT_LOAD_ASSEMBLY, &mut deleg)
    };
    if rc != 0 || deleg.is_null() {
        return Err(format!("meminta delegate pemuat assembly gagal (kode 0x{:x})", rc));
    }
    *kunci = Some(Runtime { ctx, load: deleg as usize });
    Ok(())
}

/// Alamat sebuah metode `[UnmanagedCallersOnly]`.
fn alamat_metode(asm: &str, tipe: &str, metode: &str) -> Result<usize, String> {
    let kunci = simpanan().lock().map_err(|_| "kunci runtime .NET rusak".to_string())?;
    let rt = kunci.as_ref().ok_or_else(|| concat!(
        "runtime .NET belum dinyalakan — panggil `dotnet::mulai(\"...runtimeconfig.json\")` ",
        "dulu").to_string())?;
    if !std::path::Path::new(asm).exists() {
        return Err(format!("assembly `{}` tidak ada", asm));
    }
    let c_asm = CString::new(asm).map_err(|_| "path assembly memuat NUL".to_string())?;
    let c_tipe = CString::new(tipe).map_err(|_| "nama tipe memuat NUL".to_string())?;
    let c_metode = CString::new(metode).map_err(|_| "nama metode memuat NUL".to_string())?;
    let mut fp: *mut c_void = std::ptr::null_mut();
    let rc = unsafe {
        let f: LoadFn = std::mem::transmute(rt.load);
        f(c_asm.as_ptr(), c_tipe.as_ptr(), c_metode.as_ptr(),
          UNMANAGED_CALLERS_ONLY, std::ptr::null_mut(), &mut fp)
    };
    if rc != 0 || fp.is_null() {
        return Err(format!(
            "`{}::{}` di `{}` tidak ketemu (kode 0x{:x}) — nama tipenya harus \
             BERKUALIFIKASI assembly (`Namespace.Tipe, NamaAssembly`), dan metodenya \
             harus `public static` bertanda `[UnmanagedCallersOnly]`",
            tipe, metode, asm, rc));
    }
    Ok(fp as usize)
}

// ─── Pintu builtin ───────────────────────────────────────────────────────

/// Registri fungsi yang sudah diambil alamatnya. Dipegang lewat NOMOR, jadi
/// handle-nya menyeberang jembatan nilai apa adanya.
static FUNGSI: Mutex<Vec<Option<ExternFnInfo>>> = Mutex::new(Vec::new());

fn handle(id: usize) -> Value {
    let mut f = std::collections::HashMap::new();
    f.insert("id".to_string(), Value::Int(id as i128));
    Value::Struct { name: "DotnetFn".to_string(), fields: f.into() }
}

fn id_dari(v: &Value) -> Result<usize, String> {
    match v {
        Value::Struct { name, fields } if name == "DotnetFn" => match fields.get("id") {
            Some(Value::Int(i)) if usize::try_from(*i).is_ok() => Ok(*i as usize),
            _ => Err("handle fungsi .NET rusak".to_string()),
        },
        lain => Err(format!("butuh handle fungsi .NET, dapat `{}`", lain.type_name())),
    }
}

fn teks(v: &Value, nama: &str, ke: usize) -> Result<String, String> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        lain => Err(format!("{}: argumen ke-{} harus string, dapat `{}`",
                            nama, ke + 1, lain.type_name())),
    }
}

/// Nama tipe -> `FfiType`, lewat kosakata tipe BAHASA ini.
fn tipe_ffi(nama: &str) -> Result<crate::ffi::native::FfiType, String> {
    let p = prim_dari_nama(nama).ok_or_else(|| format!(
        "tipe `{}` tidak dikenal — pakai nama tipe JRG (`i32`, `i64`, `f64`, \
         `bool`, `string`, `unit`)", nama))?;
    classify_type(&TypeExpr::Primitive(p))
}

pub fn b_tersedia(a: &[Value]) -> Result<Value, String> {
    if !a.is_empty() {
        return Err(format!("dotnet::tersedia() tidak menerima argumen, diberi {}", a.len()));
    }
    Ok(Value::Bool(tersedia()))
}

pub fn b_mulai(a: &[Value]) -> Result<Value, String> {
    if a.len() != 1 { return Err("dotnet::mulai(runtimeconfig)".to_string()); }
    let cfg = teks(&a[0], "dotnet::mulai", 0)?;
    match mulai(&cfg) {
        Ok(()) => Ok(ok_value(Value::Unit)),
        Err(e) => Ok(err_value(e)),
    }
}

pub fn b_fungsi(a: &[Value]) -> Result<Value, String> {
    if a.len() != 5 {
        return Err("dotnet::fungsi(assembly, tipe, metode, tipe_param, tipe_hasil)".to_string());
    }
    let asm = teks(&a[0], "dotnet::fungsi", 0)?;
    let tipe = teks(&a[1], "dotnet::fungsi", 1)?;
    let metode = teks(&a[2], "dotnet::fungsi", 2)?;
    let param_nama: Vec<String> = match &a[3] {
        Value::Array(arr) => {
            let mut v = Vec::new();
            for (i, x) in arr.iter().enumerate() { v.push(teks(x, "dotnet::fungsi", i)?); }
            v
        }
        lain => return Err(format!(
            "dotnet::fungsi: tipe_param harus array of string, dapat `{}`", lain.type_name())),
    };
    let hasil_nama = teks(&a[4], "dotnet::fungsi", 4)?;

    let mut param_kinds = Vec::with_capacity(param_nama.len());
    for n in &param_nama {
        match tipe_ffi(n) { Ok(k) => param_kinds.push(k), Err(e) => return Ok(err_value(e)) }
    }
    let ret_kind = match tipe_ffi(&hasil_nama) { Ok(k) => k, Err(e) => return Ok(err_value(e)) };

    let alamat = match alamat_metode(&asm, &tipe, &metode) {
        Ok(x) => x,
        Err(e) => return Ok(err_value(e)),
    };
    let info = ExternFnInfo {
        name: format!("{}::{}", tipe, metode),
        param_kinds,
        ret_kind,
        resolved: Ok(alamat),
    };
    let mut reg = FUNGSI.lock().map_err(|_| "registri fungsi .NET rusak".to_string())?;
    reg.push(Some(info));
    Ok(ok_value(handle(reg.len() - 1)))
}

pub fn b_panggil(a: &[Value]) -> Result<Value, String> {
    if a.is_empty() || a.len() > 2 { return Err("dotnet::panggil(fungsi, args)".to_string()); }
    let id = id_dari(&a[0])?;
    let args: Vec<Value> = match a.get(1) {
        None => Vec::new(),
        Some(Value::Array(arr)) => arr.iter().cloned().collect(),
        Some(lain) => return Err(format!(
            "dotnet::panggil: argumen ke-2 harus array, dapat `{}`", lain.type_name())),
    };
    let info = {
        let reg = FUNGSI.lock().map_err(|_| "registri fungsi .NET rusak".to_string())?;
        match reg.get(id).and_then(|f| f.clone()) {
            Some(i) => i,
            None => return Ok(err_value("handle fungsi .NET ini sudah dilepas")),
        }
    };
    // Marshalling dan pemanggilannya memakai mesin `extern "C"` yang sudah
    // ada: sebuah metode `[UnmanagedCallersOnly]` MEMANG pointer fungsi C.
    match call_extern_fn(&info, args) {
        Ok(v) => Ok(v),
        Err(e) => Ok(err_value(format!("{}", e))),
    }
}

pub fn b_lepas(a: &[Value]) -> Result<Value, String> {
    if a.len() != 1 { return Err("dotnet::lepas(fungsi)".to_string()); }
    let id = id_dari(&a[0])?;
    let mut reg = FUNGSI.lock().map_err(|_| "registri fungsi .NET rusak".to_string())?;
    match reg.get_mut(id) {
        Some(slot) if slot.is_some() => { *slot = None; Ok(ok_value(Value::Unit)) }
        _ => Ok(err_value("handle fungsi .NET ini sudah dilepas")),
    }
}
