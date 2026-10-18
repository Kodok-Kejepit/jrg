// src/ffi/jvm.rs
// Memanggil Java lewat JNI, tanpa crate baru.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::interpreter::Value;

/// Indeks fungsi di `struct JNINativeInterface_`, diturunkan dari `jni.h`
/// dengan `offsetof(...)/sizeof(void*)`. Dikunci `indeks_jni_cocok_dengan_header`
/// yang MEMBACA ULANG header JDK kalau ada di mesin ini.
mod idx {
    pub const GET_VERSION: usize = 4;
    pub const FIND_CLASS: usize = 6;
    pub const GET_OBJECT_CLASS: usize = 31;
    pub const GET_METHOD_ID: usize = 33;
    pub const CALL_OBJECT_METHOD_A: usize = 36;
    pub const EXCEPTION_OCCURRED: usize = 15;
    pub const EXCEPTION_CLEAR: usize = 17;
    pub const DELETE_LOCAL_REF: usize = 23;
    pub const GET_STATIC_METHOD_ID: usize = 113;
    pub const CALL_STATIC_OBJECT_A: usize = 116;
    pub const CALL_STATIC_BOOLEAN_A: usize = 119;
    pub const CALL_STATIC_BYTE_A: usize = 122;
    pub const CALL_STATIC_CHAR_A: usize = 125;
    pub const CALL_STATIC_SHORT_A: usize = 128;
    pub const CALL_STATIC_INT_A: usize = 131;
    pub const CALL_STATIC_LONG_A: usize = 134;
    pub const CALL_STATIC_FLOAT_A: usize = 137;
    pub const CALL_STATIC_DOUBLE_A: usize = 140;
    pub const CALL_STATIC_VOID_A: usize = 143;
    pub const NEW_STRING_UTF: usize = 167;
    pub const GET_STRING_UTF_CHARS: usize = 169;
    pub const RELEASE_STRING_UTF_CHARS: usize = 170;
    // Objek + metode instance + field. Diturunkan lewat `perkakas/jni_indeks.sh`,
    // nol perselisihan dengan 22 entri di atas.
    pub const NEW_GLOBAL_REF: usize = 21;
    pub const DELETE_GLOBAL_REF: usize = 22;
    pub const NEW_OBJECT_A: usize = 30;
    pub const IS_INSTANCE_OF: usize = 32;
    pub const CALL_BOOLEAN_A: usize = 39;
    pub const CALL_BYTE_A: usize = 42;
    pub const CALL_CHAR_A: usize = 45;
    pub const CALL_SHORT_A: usize = 48;
    pub const CALL_INT_A: usize = 51;
    pub const CALL_LONG_A: usize = 54;
    pub const CALL_FLOAT_A: usize = 57;
    pub const CALL_DOUBLE_A: usize = 60;
    pub const CALL_VOID_A: usize = 63;
    pub const GET_FIELD_ID: usize = 94;
    pub const GET_OBJECT_FIELD: usize = 95;
    pub const GET_BOOLEAN_FIELD: usize = 96;
    pub const GET_BYTE_FIELD: usize = 97;
    pub const GET_CHAR_FIELD: usize = 98;
    pub const GET_SHORT_FIELD: usize = 99;
    pub const GET_INT_FIELD: usize = 100;
    pub const GET_LONG_FIELD: usize = 101;
    pub const GET_FLOAT_FIELD: usize = 102;
    pub const GET_DOUBLE_FIELD: usize = 103;
    pub const GET_STATIC_FIELD_ID: usize = 144;
    pub const GET_STATIC_OBJECT_FIELD: usize = 145;
    pub const GET_STATIC_BOOLEAN_FIELD: usize = 146;
    pub const GET_STATIC_BYTE_FIELD: usize = 147;
    pub const GET_STATIC_CHAR_FIELD: usize = 148;
    pub const GET_STATIC_SHORT_FIELD: usize = 149;
    pub const GET_STATIC_INT_FIELD: usize = 150;
    pub const GET_STATIC_LONG_FIELD: usize = 151;
    pub const GET_STATIC_FLOAT_FIELD: usize = 152;
    pub const GET_STATIC_DOUBLE_FIELD: usize = 153;
    pub const GET_ARRAY_LENGTH: usize = 171;
    pub const NEW_OBJECT_ARRAY: usize = 172;
    pub const GET_OBJECT_ARRAY_ELEMENT: usize = 173;
    pub const SET_OBJECT_ARRAY_ELEMENT: usize = 174;
}

/// Indeks di `struct JNIInvokeInterface_` (tabel milik `JavaVM`).
mod idx_vm {
    pub const ATTACH_CURRENT_THREAD: usize = 4;
    pub const GET_ENV: usize = 6;
}

const JNI_VERSION_1_8: c_int = 0x0001_0008;
const JNI_OK: c_int = 0;
const JNI_EDETACHED: c_int = -2;

type Tabel = *const *const c_void;
type Env = *mut Tabel;
type Vm = *mut Tabel;

#[repr(C)]
#[derive(Clone, Copy)]
union JValue { z: u8, b: i8, c: u16, s: i16, i: i32, j: i64, f: f32, d: f64, l: *mut c_void }

#[repr(C)]
struct OpsiVm { option_string: *mut c_char, extra_info: *mut c_void }

#[repr(C)]
struct ArgsVm { version: c_int, n_options: c_int, options: *mut OpsiVm, ignore_unrecognized: u8 }

struct Mesin { vm: Vm, versi: c_int }
// JNI aman dipakai dari banyak thread ASALKAN tiap thread punya `JNIEnv`-nya
// sendiri — itulah gunanya `AttachCurrentThread`. Yang disimpan di sini
// cuma `JavaVM*`, yang memang dirancang untuk dibagi antar-thread.
unsafe impl Send for Mesin {}

static MESIN: OnceLock<Mutex<Option<Mesin>>> = OnceLock::new();

/// Sebab kegagalan yang TIDAK bisa diulang percobaannya.
static GAGAL_PERMANEN: OnceLock<String> = OnceLock::new();

fn simpanan() -> &'static Mutex<Option<Mesin>> {
    MESIN.get_or_init(|| Mutex::new(None))
}

/// Tempat `libjvm.so` biasanya berada, diperiksa berurutan.
fn cari_libjvm() -> Option<String> {
    let mut kandidat: Vec<String> = Vec::new();
    if let Ok(home) = std::env::var("JAVA_HOME") {
        for sub in ["lib/server/libjvm.so", "jre/lib/server/libjvm.so",
                    "lib/amd64/server/libjvm.so", "jre/lib/amd64/server/libjvm.so"] {
            kandidat.push(format!("{}/{}", home.trim_end_matches('/'), sub));
        }
    }
    for akar in ["/usr/lib/jvm", "/usr/lib64/jvm", "/opt/java", "/Library/Java/JavaVirtualMachines"] {
        let Ok(baca) = std::fs::read_dir(akar) else { continue };
        let mut anak: Vec<_> = baca.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        // Urut supaya pilihannya tidak bergantung urutan direktori sistem
        // berkas — dua mesin dengan JDK sama harus memilih yang sama.
        anak.sort();
        for p in anak {
            for sub in ["lib/server/libjvm.so", "jre/lib/server/libjvm.so",
                        "lib/amd64/server/libjvm.so", "Contents/Home/lib/server/libjvm.dylib"] {
                kandidat.push(format!("{}/{}", p.display(), sub));
            }
        }
    }
    kandidat.into_iter().find(|p| std::path::Path::new(p).exists())
}

pub fn alasan_tidak_ada() -> String {
    "libjvm tidak ketemu — pasang JDK-nya dulu: Debian/Ubuntu/Kali \
             `apt install default-jdk`, Fedora `dnf install java-devel`, Arch \
             `pacman -S jdk-openjdk`, Termux `pkg install openjdk-17`. Kalau JDK-nya \
             ada di tempat yang tidak biasa, set `JAVA_HOME`".to_string()
}

pub fn tersedia() -> bool { cari_libjvm().is_some() }

unsafe fn fungsi(env: Env, i: usize) -> *const c_void {
    *(*env).add(i)
}

/// Pastikan JVM-nya ada, dan kembalikan `JNIEnv` untuk thread INI.
pub fn env_thread_ini(opsi: &[String]) -> Result<Env, String> {
    let mut kunci = simpanan().lock().map_err(|_| "kunci JVM rusak".to_string())?;
    if let Some(sebab) = GAGAL_PERMANEN.get() {
        return Err(sebab.clone());
    }
    if kunci.is_none() {
        match buat_vm(opsi) {
            Ok(m) => *kunci = Some(m),
            Err(e) => return Err(e),
        }
    }
    let mesin = kunci.as_ref().unwrap();
    let vm = mesin.vm;
    let versi = mesin.versi;
    unsafe {
        let mut env: Env = std::ptr::null_mut();
        let get_env: extern "C" fn(Vm, *mut Env, c_int) -> c_int =
            std::mem::transmute(*(*vm).add(idx_vm::GET_ENV));
        let r = get_env(vm, &mut env, versi);
        if r == JNI_OK && !env.is_null() { return Ok(env); }
        if r == JNI_EDETACHED {
            let attach: extern "C" fn(Vm, *mut Env, *mut c_void) -> c_int =
                std::mem::transmute(*(*vm).add(idx_vm::ATTACH_CURRENT_THREAD));
            if attach(vm, &mut env, std::ptr::null_mut()) == JNI_OK && !env.is_null() {
                return Ok(env);
            }
            return Err("gagal menyambungkan thread ini ke JVM".to_string());
        }
        Err(format!("GetEnv menjawab {} — JVM tidak bisa dipakai dari thread ini", r))
    }
}

fn buat_vm(opsi: &[String]) -> Result<Mesin, String> {
    let path = cari_libjvm().ok_or_else(alasan_tidak_ada)?;
    let handle = crate::ffi::native::load_library(Some(&path))
        .map_err(|e| format!("libjvm ada di `{}` tapi gagal dimuat: {}", path, e))?;
    let alamat = crate::ffi::native::resolve_symbol(handle, "JNI_CreateJavaVM")
        .map_err(|e| format!("`JNI_CreateJavaVM` tidak ada di `{}`: {}", path, e))?;

    let cstr: Vec<CString> = opsi.iter()
        .map(|o| CString::new(o.as_str()).unwrap_or_else(|_| CString::new("").unwrap()))
        .collect();
    let mut opts: Vec<OpsiVm> = cstr.iter()
        .map(|c| OpsiVm { option_string: c.as_ptr() as *mut c_char, extra_info: std::ptr::null_mut() })
        .collect();
    let mut args = ArgsVm {
        version: JNI_VERSION_1_8,
        n_options: opts.len() as c_int,
        options: if opts.is_empty() { std::ptr::null_mut() } else { opts.as_mut_ptr() },
        ignore_unrecognized: 0,
    };

    let mut vm: Vm = std::ptr::null_mut();
    let mut env: Env = std::ptr::null_mut();
    let r = unsafe {
        let f: extern "C" fn(*mut Vm, *mut Env, *mut c_void) -> c_int =
            std::mem::transmute(alamat);
        f(&mut vm, &mut env, &mut args as *mut ArgsVm as *mut c_void)
    };
    if r != JNI_OK || vm.is_null() || env.is_null() {
        return Err(format!(
            "JNI_CreateJavaVM gagal (kode {}) — opsi yang diberikan: {:?}", r, opsi));
    }

    // Verifikasi tabelnya SEBELUM indeks mana pun yang lain dipakai. Sebuah
    // tabel yang tata letaknya tidak seperti dugaan berarti tiap indeks
    // menunjuk fungsi yang salah, dan itu segfault, bukan galat.
    let versi = unsafe {
        let f: extern "C" fn(Env) -> c_int = std::mem::transmute(fungsi(env, idx::GET_VERSION));
        f(env)
    };
    // Versi JNI adalah `major << 16 | minor`: 1.8 adalah 0x00010008, dan JDK 24
    // adalah 0x00180000.
    let major = (versi >> 16) & 0xffff;
    if !(1..=99).contains(&major) {
        let _ = GAGAL_PERMANEN.set(format!(
            "tabel fungsi JNI tidak seperti yang diharapkan: GetVersion menjawab 0x{:x}. \
             Tidak ada fungsi JNI lain yang dipanggil, dan JVM-nya tidak bisa dibuat ulang \
             di proses ini", versi));
        return Err(format!(
            "tabel fungsi JNI tidak seperti yang diharapkan: GetVersion menjawab 0x{:x}. \
             Tidak ada fungsi JNI lain yang dipanggil — memanggilnya dengan tata letak \
             yang salah berarti proses mati tanpa pesan", versi));
    }
    Ok(Mesin { vm, versi })
}

pub fn versi(opsi: &[String]) -> Result<String, String> {
    let env = env_thread_ini(opsi)?;
    let v = unsafe {
        let f: extern "C" fn(Env) -> c_int = std::mem::transmute(fungsi(env, idx::GET_VERSION));
        f(env)
    };
    Ok(format!("{}.{}", (v >> 16) & 0xffff, v & 0xffff))
}

// ─── Tanda tangan JNI ────────────────────────────────────────────────────

/// Pecah deskriptor JNI (`"(ID)Ljava/lang/String;"`) jadi tipe parameter dan
/// tipe return.
pub fn urai_tanda_tangan(d: &str) -> Result<(Vec<String>, String), String> {
    let b = d.as_bytes();
    if b.first() != Some(&b'(') {
        return Err(format!("tanda tangan `{}` harus mulai dengan `(`", d));
    }
    let mut i = 1usize;
    let mut params = Vec::new();
    while i < b.len() && b[i] != b')' {
        let (t, j) = baca_tipe(b, i, d)?;
        params.push(t);
        i = j;
    }
    if i >= b.len() {
        return Err(format!("tanda tangan `{}` tidak punya `)`", d));
    }
    let (ret, j) = baca_tipe(b, i + 1, d)?;
    if j != b.len() {
        return Err(format!("ada sisa sesudah tipe return di tanda tangan `{}`", d));
    }
    Ok((params, ret))
}

fn baca_tipe(b: &[u8], mut i: usize, penuh: &str) -> Result<(String, usize), String> {
    let mulai = i;
    while i < b.len() && b[i] == b'[' { i += 1; }
    if i >= b.len() {
        return Err(format!("tanda tangan `{}` terpotong", penuh));
    }
    match b[i] {
        b'Z' | b'B' | b'C' | b'S' | b'I' | b'J' | b'F' | b'D' | b'V' => i += 1,
        b'L' => {
            while i < b.len() && b[i] != b';' { i += 1; }
            if i >= b.len() {
                return Err(format!("tipe objek di `{}` tidak ditutup `;`", penuh));
            }
            i += 1;
        }
        c => return Err(format!("huruf tipe `{}` di `{}` tidak dikenal", c as char, penuh)),
    }
    Ok((String::from_utf8_lossy(&b[mulai..i]).to_string(), i))
}

// ─── Objek Java yang dipegang JRG ────────────────────────────────────────
// Disimpan sebagai GLOBAL ref: local ref mati begitu panggilannya selesai.

struct Objek { global: *mut c_void, kelas: String }
// Global ref hanya disentuh lewat mutex di bawah, dan JNI membolehkan thread
// mana pun memakainya asalkan thread itu ter-attach.
unsafe impl Send for Objek {}

static OBJEK: OnceLock<Mutex<(u64, HashMap<u64, Objek>)>> = OnceLock::new();

fn tabel_objek() -> &'static Mutex<(u64, HashMap<u64, Objek>)> {
    OBJEK.get_or_init(|| Mutex::new((0, HashMap::new())))
}

/// Bungkus sebuah local ref jadi global ref + id. Local-nya dihapus di sini.
unsafe fn simpan_objek(env: Env, lokal: *mut c_void, kelas: &str) -> Result<u64, String> {
    if lokal.is_null() { return Err("objek Java-nya null".to_string()); }
    let newg: extern "C" fn(Env, *mut c_void) -> *mut c_void =
        std::mem::transmute(fungsi(env, idx::NEW_GLOBAL_REF));
    let g = newg(env, lokal);
    hapus_lokal(env, lokal);
    if g.is_null() { return Err("gagal membuat global ref".to_string()); }
    let mut t = tabel_objek().lock().map_err(|_| "tabel objek terkunci".to_string())?;
    t.0 += 1;
    let id = t.0;
    t.1.insert(id, Objek { global: g, kelas: kelas.to_string() });
    Ok(id)
}

fn handle_objek(id: u64, kelas: &str) -> Value {
    let mut f: HashMap<String, Value> = HashMap::new();
    f.insert("id".to_string(), Value::Int(id as i128));
    f.insert("kelas".to_string(), Value::Str(kelas.to_string()));
    Value::Struct { name: "JavaObj".to_string(), fields: f.into() }
}

/// Dipakai `ffi::kotlin` — lapisan Kotlin memakai handle yang SAMA, bukan
/// tabel kedua.
pub fn id_dari_handle_pub(v: &Value, nama: &str) -> Result<u64, String> {
    id_dari_handle(v, nama)
}

fn id_dari_handle(v: &Value, nama: &str) -> Result<u64, String> {
    match v {
        Value::Struct { name, fields } if name == "JavaObj" => match fields.get("id") {
            Some(Value::Int(i)) if u64::try_from(*i).is_ok() => Ok(*i as u64),
            _ => Err(format!("{}: handle objek Java rusak", nama)),
        },
        lain => Err(format!(
            "{}: butuh objek Java (hasil `jvm::baru` atau `jvm::field_statis`), dapat `{}`",
            nama, lain.type_name())),
    }
}

fn ambil_global(id: u64, nama: &str) -> Result<(*mut c_void, String), String> {
    let t = tabel_objek().lock().map_err(|_| "tabel objek terkunci".to_string())?;
    match t.1.get(&id) {
        Some(o) => Ok((o.global, o.kelas.clone())),
        None => Err(format!(
            "{}: objek Java ini sudah dilepas lewat `jvm::lepas` — sebuah handle \
             yang sudah dilepas tidak boleh dipakai lagi", nama)),
    }
}

// ─── Panggilan ───────────────────────────────────────────────────────────

unsafe fn ambil_string(env: Env, obj: *mut c_void) -> Option<String> {
    if obj.is_null() { return None; }
    let get: extern "C" fn(Env, *mut c_void, *mut u8) -> *const c_char =
        std::mem::transmute(fungsi(env, idx::GET_STRING_UTF_CHARS));
    let rel: extern "C" fn(Env, *mut c_void, *const c_char) =
        std::mem::transmute(fungsi(env, idx::RELEASE_STRING_UTF_CHARS));
    let p = get(env, obj, std::ptr::null_mut());
    if p.is_null() { return None; }
    let s = CStr::from_ptr(p).to_string_lossy().into_owned();
    rel(env, obj, p);
    Some(s)
}

unsafe fn hapus_lokal(env: Env, obj: *mut c_void) {
    if obj.is_null() { return; }
    let f: extern "C" fn(Env, *mut c_void) = std::mem::transmute(fungsi(env, idx::DELETE_LOCAL_REF));
    f(env, obj);
}

/// Kalau ada exception Java yang menggantung, ubah jadi galat JRG.
unsafe fn periksa_exception(env: Env) -> Result<(), String> {
    let cek: extern "C" fn(Env) -> *mut c_void =
        std::mem::transmute(fungsi(env, idx::EXCEPTION_OCCURRED));
    let exc = cek(env);
    if exc.is_null() { return Ok(()); }
    let bersih: extern "C" fn(Env) = std::mem::transmute(fungsi(env, idx::EXCEPTION_CLEAR));
    bersih(env);

    let get_cls: extern "C" fn(Env, *mut c_void) -> *mut c_void =
        std::mem::transmute(fungsi(env, idx::GET_OBJECT_CLASS));
    let get_mid: extern "C" fn(Env, *mut c_void, *const c_char, *const c_char) -> *mut c_void =
        std::mem::transmute(fungsi(env, idx::GET_METHOD_ID));
    let call: extern "C" fn(Env, *mut c_void, *mut c_void, *const JValue) -> *mut c_void =
        std::mem::transmute(fungsi(env, idx::CALL_OBJECT_METHOD_A));

    let cls = get_cls(env, exc);
    let nama = CString::new("toString").unwrap();
    let sig = CString::new("()Ljava/lang/String;").unwrap();
    let mid = get_mid(env, cls, nama.as_ptr(), sig.as_ptr());
    let pesan = if mid.is_null() {
        None
    } else {
        let s = call(env, exc, mid, std::ptr::null());
        // `toString` sendiri bisa melempar; bersihkan supaya panggilan
        // berikutnya tidak mewarisi exception yang bukan miliknya.
        bersih(env);
        let t = ambil_string(env, s);
        hapus_lokal(env, s);
        t
    };
    hapus_lokal(env, cls);
    hapus_lokal(env, exc);
    Err(match pesan {
        Some(p) => format!("exception dari Java: {}{}", p, saran_classpath(&p)),
        None => "exception dari Java (pesannya sendiri tidak bisa dibaca)".to_string(),
    })
}

/// Sebagian kegagalan classpath punya sebab yang PASTI dan jalan keluar yang
/// pasti pula. Menyebutnya menghemat satu jam pencarian.
fn saran_classpath(pesan: &str) -> String {
    if pesan.contains("kotlin/jvm/internal") || pesan.contains("kotlin.jvm.internal") {
        return " — kelas ini milik `kotlin-stdlib.jar`. Tambahkan jar itu ke \
                 classpath waktu `jvm::mulai`, mis. \
                 `-Djava.class.path=kelas:/path/ke/kotlin-stdlib.jar`".to_string();
    }
    if pesan.contains("kotlin/coroutines") || pesan.contains("Continuation") {
        return " — ini fungsi `suspend`. Fungsi suspend butuh mesin coroutine \
                 Kotlin dan tidak bisa dipanggil langsung; bungkus dulu dengan \
                 fungsi biasa (`runBlocking { ... }`) di sisi Kotlin".to_string();
    }
    String::new()
}

/// Tipe yang boleh menerima sebuah `java.lang.String` apa adanya. Ditulis
/// eksplisit: JNI tidak memeriksa kecocokan tipe saat memanggil, jadi daftar
/// yang ditebak akan jadi perilaku tak terdefinisi, bukan sebuah galat.
const SUPERTIPE_STRING: &[&str] = &[
    "Ljava/lang/String;",
    "Ljava/lang/CharSequence;",
    "Ljava/lang/Object;",
    "Ljava/lang/Comparable;",
];

unsafe fn ke_jvalue(env: Env, tipe: &str, v: &Value, ke: usize, nama: &str)
    -> Result<(JValue, Option<*mut c_void>), String>
{
    let salah = |mau: &str| format!(
        "argumen ke-{} `{}` butuh {}, dapat `{}`", ke + 1, nama, mau, v.type_name());
    Ok(match tipe {
        "I" => (JValue { i: bil(v).ok_or_else(|| salah("int"))? as i32 }, None),
        "J" => (JValue { j: bil(v).ok_or_else(|| salah("long"))? }, None),
        "S" => (JValue { s: bil(v).ok_or_else(|| salah("short"))? as i16 }, None),
        "B" => (JValue { b: bil(v).ok_or_else(|| salah("byte"))? as i8 }, None),
        "C" => (JValue { c: bil(v).ok_or_else(|| salah("char"))? as u16 }, None),
        "D" => (JValue { d: pecahan(v).ok_or_else(|| salah("double"))? }, None),
        "F" => (JValue { f: pecahan(v).ok_or_else(|| salah("float"))? as f32 }, None),
        "Z" => match v {
            Value::Bool(b) => (JValue { z: if *b { 1 } else { 0 } }, None),
            _ => return Err(salah("bool")),
        },
        "Ljava/lang/String;" => match v {
            Value::Str(s) => {
                let c = CString::new(s.as_str())
                    .map_err(|_| format!("argumen ke-{} memuat byte NUL", ke + 1))?;
                let f: extern "C" fn(Env, *const c_char) -> *mut c_void =
                    std::mem::transmute(fungsi(env, idx::NEW_STRING_UTF));
                let o = f(env, c.as_ptr());
                if o.is_null() { return Err("gagal membuat String Java".to_string()); }
                (JValue { l: o }, Some(o))
            }
            _ => return Err(salah("string")),
        },
        // Objek: sebuah handle `JavaObj`, `nil` untuk `null`, atau sebuah string
        // JRG kalau parameternya SUPERTIPE String — daftarnya pasti dan kecil,
        // jadi tidak ada tebakan tentang apa yang cocok dengan apa.
        lain if lain.starts_with('L') && lain.ends_with(';') => match v {
            Value::Nil => (JValue { l: std::ptr::null_mut() }, None),
            Value::Str(_) if SUPERTIPE_STRING.contains(&lain) => {
                return ke_jvalue(env, "Ljava/lang/String;", v, ke, nama);
            }
            Value::Struct { name, .. } if name == "JavaObj" => {
                let id = id_dari_handle(v, nama)?;
                let (g, _) = ambil_global(id, nama)?;
                (JValue { l: g }, None)
            }
            _ => return Err(salah("objek Java (hasil `jvm::baru`) atau `nil`")),
        },
        // Array: dibangun dari array JRG, isinya disalin satu per satu.
        lain if lain.starts_with('[') => match v {
            Value::Nil => (JValue { l: std::ptr::null_mut() }, None),
            Value::Array(a) => {
                let o = buat_array_java(env, &lain[1..], a, ke, nama)?;
                (JValue { l: o }, Some(o))
            }
            _ => return Err(salah("array")),
        },
        lain => return Err(format!(
            "tipe argumen `{}` belum didukung — yang bisa: Z B C S I J F D, \
             objek `Lpaket/Kelas;`, dan array `[T`", lain)),
    })
}

/// Bangun array Java dari array JRG. Panjangnya datang dari sisi JRG, jadi
/// tidak ada nilai dari luar yang menentukan besar alokasi di sini.
unsafe fn buat_array_java(
    env: Env, elem: &str, isi: &[Value], ke: usize, nama: &str,
) -> Result<*mut c_void, String> {
    let n = isi.len() as i32;
    // Tipe elemen menentukan JNI mana yang dipakai; yang primitif punya
    // pembuatnya sendiri, sisanya lewat NewObjectArray + set per elemen.
    let kelas_elem = if elem.starts_with('L') && elem.ends_with(';') {
        &elem[1..elem.len() - 1]
    } else {
        return Err(format!(
            "argumen ke-{} `{}`: array bertipe elemen `{}` belum didukung — \
             yang bisa baru array objek, mis. `[Ljava/lang/String;`", ke + 1, nama, elem));
    };
    let cls = cari_kelas(env, kelas_elem)?;
    let newarr: extern "C" fn(Env, i32, *mut c_void, *mut c_void) -> *mut c_void =
        std::mem::transmute(fungsi(env, idx::NEW_OBJECT_ARRAY));
    let arr = newarr(env, n, cls, std::ptr::null_mut());
    hapus_lokal(env, cls);
    if arr.is_null() {
        periksa_exception(env)?;
        return Err(format!("gagal membuat array `{}` berisi {} elemen", elem, n));
    }
    let set: extern "C" fn(Env, *mut c_void, i32, *mut c_void) =
        std::mem::transmute(fungsi(env, idx::SET_OBJECT_ARRAY_ELEMENT));
    for (i, v) in isi.iter().enumerate() {
        let (jv, tmp) = ke_jvalue(env, elem, v, ke, nama)?;
        set(env, arr, i as i32, jv.l);
        if let Some(o) = tmp { hapus_lokal(env, o); }
        periksa_exception(env)?;
    }
    Ok(arr)
}

fn bil(v: &Value) -> Option<i64> {
    match v { Value::Int(i) => i64::try_from(*i).ok(), _ => None }
}

fn pecahan(v: &Value) -> Option<f64> {
    match v {
        Value::Float(f) => Some(*f),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

/// Panggil sebuah metode statis Java.
pub fn panggil_statis(
    opsi: &[String],
    kelas: &str,
    metode: &str,
    tanda_tangan: &str,
    args: &[Value],
) -> Result<Value, String> {
    let (params, ret) = urai_tanda_tangan(tanda_tangan)?;
    if params.len() != args.len() {
        return Err(format!(
            "`{}.{}{}` minta {} argumen, diberi {}",
            kelas, metode, tanda_tangan, params.len(), args.len()));
    }
    let env = env_thread_ini(opsi)?;
    // Nama kelas Java memakai `/`, bukan `.` — dan menerima keduanya lebih
    // baik daripada menolak bentuk yang justru paling wajar ditulis orang.
    let kelas_jni = kelas.replace('.', "/");
    unsafe {
        let find: extern "C" fn(Env, *const c_char) -> *mut c_void =
            std::mem::transmute(fungsi(env, idx::FIND_CLASS));
        let c_kelas = CString::new(kelas_jni.as_str())
            .map_err(|_| "nama kelas memuat byte NUL".to_string())?;
        let cls = find(env, c_kelas.as_ptr());
        if cls.is_null() {
            periksa_exception(env)?;
            return Err(format!(
                "kelas `{}` tidak ketemu — cek classpath-nya (opsi \
             `-Djava.class.path=...` waktu `jvm::mulai`)", kelas));
        }

        let get_mid: extern "C" fn(Env, *mut c_void, *const c_char, *const c_char) -> *mut c_void =
            std::mem::transmute(fungsi(env, idx::GET_STATIC_METHOD_ID));
        let c_nama = CString::new(metode).map_err(|_| "nama metode memuat NUL".to_string())?;
        let c_sig = CString::new(tanda_tangan).map_err(|_| "tanda tangan memuat NUL".to_string())?;
        let mid = get_mid(env, cls, c_nama.as_ptr(), c_sig.as_ptr());
        if mid.is_null() {
            let _ = periksa_exception(env);
            hapus_lokal(env, cls);
            return Err(format!(
                "`{}.{}` dengan tanda tangan `{}` tidak ketemu — bandingkan dengan \
             keluaran `javap -s {}`", kelas, metode, tanda_tangan, kelas));
        }

        let mut nilai: Vec<JValue> = Vec::with_capacity(args.len());
        let mut sementara: Vec<*mut c_void> = Vec::new();
        for (i, (t, a)) in params.iter().zip(args).enumerate() {
            match ke_jvalue(env, t, a, i, metode) {
                Ok((jv, tmp)) => { nilai.push(jv); if let Some(o) = tmp { sementara.push(o); } }
                Err(e) => {
                    for o in sementara { hapus_lokal(env, o); }
                    hapus_lokal(env, cls);
                    return Err(e);
                }
            }
        }
        let ptr_args = if nilai.is_empty() { std::ptr::null() } else { nilai.as_ptr() };

        let hasil = panggil_menurut_return(env, cls, mid, ptr_args, &ret);
        for o in sementara { hapus_lokal(env, o); }
        let cek = periksa_exception(env);
        hapus_lokal(env, cls);
        cek?;
        hasil
    }
}

unsafe fn panggil_menurut_return(
    env: Env, cls: *mut c_void, mid: *mut c_void, args: *const JValue, ret: &str,
) -> Result<Value, String> {
    macro_rules! panggil {
        ($i:expr, $t:ty) => {{
            let f: extern "C" fn(Env, *mut c_void, *mut c_void, *const JValue) -> $t =
                std::mem::transmute(fungsi(env, $i));
            f(env, cls, mid, args)
        }};
    }
    Ok(match ret {
        "V" => {
            let f: extern "C" fn(Env, *mut c_void, *mut c_void, *const JValue) =
                std::mem::transmute(fungsi(env, idx::CALL_STATIC_VOID_A));
            f(env, cls, mid, args);
            Value::Unit
        }
        "I" => Value::Int(panggil!(idx::CALL_STATIC_INT_A, i32) as i128),
        "J" => Value::Int(panggil!(idx::CALL_STATIC_LONG_A, i64) as i128),
        "S" => Value::Int(panggil!(idx::CALL_STATIC_SHORT_A, i16) as i128),
        "B" => Value::Int(panggil!(idx::CALL_STATIC_BYTE_A, i8) as i128),
        "C" => Value::Int(panggil!(idx::CALL_STATIC_CHAR_A, u16) as i128),
        "Z" => Value::Bool(panggil!(idx::CALL_STATIC_BOOLEAN_A, u8) != 0),
        "D" => Value::Float(panggil!(idx::CALL_STATIC_DOUBLE_A, f64)),
        "F" => Value::Float(panggil!(idx::CALL_STATIC_FLOAT_A, f32) as f64),
        "Ljava/lang/String;" => {
            let o = panggil!(idx::CALL_STATIC_OBJECT_A, *mut c_void);
            // `null` Java bukan string kosong, jadi ia jadi `nil` — bukan
            // `""`, yang akan menyamarkan "tidak ada" jadi "kosong".
            let s = ambil_string(env, o);
            hapus_lokal(env, o);
            match s { Some(t) => Value::Str(t), None => Value::Nil }
        }
        lain => return Err(format!(
            "tipe return `{}` belum didukung — yang bisa: Z B C S I J F D V dan \
             Ljava/lang/String;. Sebuah objek Java belum bisa dipegang nilai JRG", lain)),
    })
}

/// Cari kelas, dengan pesan yang menyebut classpath kalau tidak ketemu.
unsafe fn cari_kelas(env: Env, kelas: &str) -> Result<*mut c_void, String> {
    let find: extern "C" fn(Env, *const c_char) -> *mut c_void =
        std::mem::transmute(fungsi(env, idx::FIND_CLASS));
    let c = CString::new(kelas.replace('.', "/"))
        .map_err(|_| "nama kelas memuat byte NUL".to_string())?;
    let cls = find(env, c.as_ptr());
    if cls.is_null() {
        periksa_exception(env)?;
        return Err(format!(
            "kelas `{}` tidak ketemu — cek classpath-nya (opsi \
             `-Djava.class.path=...` waktu `jvm::mulai`)", kelas));
    }
    Ok(cls)
}

/// Siapkan argumen JNI dari nilai JRG. Objek sementara dikembalikan supaya
/// pemanggil bisa menghapusnya SESUDAH panggilan, bukan sebelum.
unsafe fn siapkan_args(
    env: Env, params: &[String], args: &[Value], nama: &str,
) -> Result<(Vec<JValue>, Vec<*mut c_void>), String> {
    let mut nilai = Vec::with_capacity(args.len());
    let mut sementara = Vec::new();
    for (i, (t, a)) in params.iter().zip(args).enumerate() {
        match ke_jvalue(env, t, a, i, nama) {
            Ok((jv, tmp)) => { nilai.push(jv); if let Some(o) = tmp { sementara.push(o); } }
            Err(e) => { for o in sementara { hapus_lokal(env, o); } return Err(e); }
        }
    }
    Ok((nilai, sementara))
}

/// `new Kelas(...)`, hasilnya sebuah handle objek.
pub fn buat_objek(
    opsi: &[String], kelas: &str, tanda_tangan: &str, args: &[Value],
) -> Result<Value, String> {
    let (params, ret) = urai_tanda_tangan(tanda_tangan)?;
    if ret != "V" {
        return Err(format!(
            "tanda tangan konstruktor harus berakhir `)V`, dapat `{}` — mis. `(II)V`",
            tanda_tangan));
    }
    if params.len() != args.len() {
        return Err(format!("konstruktor `{}{}` minta {} argumen, diberi {}",
            kelas, tanda_tangan, params.len(), args.len()));
    }
    let env = env_thread_ini(opsi)?;
    unsafe {
        let cls = cari_kelas(env, kelas)?;
        let get_mid: extern "C" fn(Env, *mut c_void, *const c_char, *const c_char) -> *mut c_void =
            std::mem::transmute(fungsi(env, idx::GET_METHOD_ID));
        let c_init = CString::new("<init>").unwrap();
        let c_sig = CString::new(tanda_tangan).map_err(|_| "tanda tangan memuat NUL".to_string())?;
        let mid = get_mid(env, cls, c_init.as_ptr(), c_sig.as_ptr());
        if mid.is_null() {
            let _ = periksa_exception(env);
            hapus_lokal(env, cls);
            return Err(format!(
                "`{}` tidak punya konstruktor dengan tanda tangan `{}` — bandingkan \
                 dengan keluaran `javap -s {}`", kelas, tanda_tangan, kelas));
        }
        let (nilai, sementara) = match siapkan_args(env, &params, args, "konstruktor") {
            Ok(v) => v,
            Err(e) => { hapus_lokal(env, cls); return Err(e); }
        };
        let ptr = if nilai.is_empty() { std::ptr::null() } else { nilai.as_ptr() };
        let new: extern "C" fn(Env, *mut c_void, *mut c_void, *const JValue) -> *mut c_void =
            std::mem::transmute(fungsi(env, idx::NEW_OBJECT_A));
        let obj = new(env, cls, mid, ptr);
        for o in sementara { hapus_lokal(env, o); }
        let cek = periksa_exception(env);
        hapus_lokal(env, cls);
        cek?;
        let id = simpan_objek(env, obj, kelas)?;
        Ok(handle_objek(id, kelas))
    }
}

/// Panggil metode INSTANCE pada sebuah handle objek.
pub fn panggil_objek(
    opsi: &[String], id: u64, metode: &str, tanda_tangan: &str, args: &[Value],
) -> Result<Value, String> {
    let (params, ret) = urai_tanda_tangan(tanda_tangan)?;
    if params.len() != args.len() {
        return Err(format!("`{}{}` minta {} argumen, diberi {}",
            metode, tanda_tangan, params.len(), args.len()));
    }
    let (global, kelas) = ambil_global(id, "jvm::panggil")?;
    let env = env_thread_ini(opsi)?;
    unsafe {
        let get_cls: extern "C" fn(Env, *mut c_void) -> *mut c_void =
            std::mem::transmute(fungsi(env, idx::GET_OBJECT_CLASS));
        let cls = get_cls(env, global);
        let get_mid: extern "C" fn(Env, *mut c_void, *const c_char, *const c_char) -> *mut c_void =
            std::mem::transmute(fungsi(env, idx::GET_METHOD_ID));
        let c_nama = CString::new(metode).map_err(|_| "nama metode memuat NUL".to_string())?;
        let c_sig = CString::new(tanda_tangan).map_err(|_| "tanda tangan memuat NUL".to_string())?;
        let mid = get_mid(env, cls, c_nama.as_ptr(), c_sig.as_ptr());
        if mid.is_null() {
            let _ = periksa_exception(env);
            hapus_lokal(env, cls);
            return Err(format!(
                "`{}.{}` dengan tanda tangan `{}` tidak ketemu — bandingkan dengan \
                 keluaran `javap -s {}`", kelas, metode, tanda_tangan, kelas));
        }
        let (nilai, sementara) = match siapkan_args(env, &params, args, metode) {
            Ok(v) => v,
            Err(e) => { hapus_lokal(env, cls); return Err(e); }
        };
        let ptr = if nilai.is_empty() { std::ptr::null() } else { nilai.as_ptr() };
        let hasil = panggil_instance(env, global, mid, ptr, &ret);
        for o in sementara { hapus_lokal(env, o); }
        let cek = periksa_exception(env);
        hapus_lokal(env, cls);
        cek?;
        hasil
    }
}

unsafe fn panggil_instance(
    env: Env, obj: *mut c_void, mid: *mut c_void, args: *const JValue, ret: &str,
) -> Result<Value, String> {
    macro_rules! panggil {
        ($i:expr, $t:ty) => {{
            let f: extern "C" fn(Env, *mut c_void, *mut c_void, *const JValue) -> $t =
                std::mem::transmute(fungsi(env, $i));
            f(env, obj, mid, args)
        }};
    }
    Ok(match ret {
        "V" => {
            let f: extern "C" fn(Env, *mut c_void, *mut c_void, *const JValue) =
                std::mem::transmute(fungsi(env, idx::CALL_VOID_A));
            f(env, obj, mid, args);
            Value::Unit
        }
        "I" => Value::Int(panggil!(idx::CALL_INT_A, i32) as i128),
        "J" => Value::Int(panggil!(idx::CALL_LONG_A, i64) as i128),
        "S" => Value::Int(panggil!(idx::CALL_SHORT_A, i16) as i128),
        "B" => Value::Int(panggil!(idx::CALL_BYTE_A, i8) as i128),
        "C" => Value::Int(panggil!(idx::CALL_CHAR_A, u16) as i128),
        "Z" => Value::Bool(panggil!(idx::CALL_BOOLEAN_A, u8) != 0),
        "D" => Value::Float(panggil!(idx::CALL_DOUBLE_A, f64)),
        "F" => Value::Float(panggil!(idx::CALL_FLOAT_A, f32) as f64),
        "Ljava/lang/String;" => {
            let o = panggil!(idx::CALL_OBJECT_METHOD_A, *mut c_void);
            let s = ambil_string(env, o);
            hapus_lokal(env, o);
            match s { Some(t) => Value::Str(t), None => Value::Nil }
        }
        lain if lain.starts_with('L') && lain.ends_with(';') => {
            // Objek lain: dipegang sebagai handle, bukan di-stringify —
            // `toString()` sebuah objek bukan datanya.
            let o = panggil!(idx::CALL_OBJECT_METHOD_A, *mut c_void);
            if o.is_null() { return Ok(Value::Nil); }
            let nama = lain[1..lain.len() - 1].replace('/', ".");
            let id = simpan_objek(env, o, &nama)?;
            handle_objek(id, &nama)
        }
        lain => return Err(format!(
            "tipe return `{}` belum didukung — yang bisa: Z B C S I J F D V, \
             Ljava/lang/String;, dan objek `Lpaket/Kelas;`", lain)),
    })
}

/// Baca field STATIS (`Kelas.NAMA`). Ini yang membuat `object`/`companion`
/// Kotlin terjangkau, karena keduanya diekspos sebagai field statis.
pub fn field_statis(
    opsi: &[String], kelas: &str, nama: &str, tipe: &str,
) -> Result<Value, String> {
    let env = env_thread_ini(opsi)?;
    unsafe {
        let cls = cari_kelas(env, kelas)?;
        let get_fid: extern "C" fn(Env, *mut c_void, *const c_char, *const c_char) -> *mut c_void =
            std::mem::transmute(fungsi(env, idx::GET_STATIC_FIELD_ID));
        let c_nama = CString::new(nama).map_err(|_| "nama field memuat NUL".to_string())?;
        let c_tipe = CString::new(tipe).map_err(|_| "tipe field memuat NUL".to_string())?;
        let fid = get_fid(env, cls, c_nama.as_ptr(), c_tipe.as_ptr());
        if fid.is_null() {
            let _ = periksa_exception(env);
            hapus_lokal(env, cls);
            return Err(format!(
                "field statis `{}.{}` bertipe `{}` tidak ketemu — bandingkan dengan \
                 keluaran `javap -s {}`", kelas, nama, tipe, kelas));
        }
        let hasil = baca_field(env, cls, fid, tipe, true);
        let cek = periksa_exception(env);
        hapus_lokal(env, cls);
        cek?;
        hasil
    }
}

/// Baca field INSTANCE dari sebuah handle objek.
pub fn field_objek(opsi: &[String], id: u64, nama: &str, tipe: &str) -> Result<Value, String> {
    let (global, kelas) = ambil_global(id, "jvm::field")?;
    let env = env_thread_ini(opsi)?;
    unsafe {
        let get_cls: extern "C" fn(Env, *mut c_void) -> *mut c_void =
            std::mem::transmute(fungsi(env, idx::GET_OBJECT_CLASS));
        let cls = get_cls(env, global);
        let get_fid: extern "C" fn(Env, *mut c_void, *const c_char, *const c_char) -> *mut c_void =
            std::mem::transmute(fungsi(env, idx::GET_FIELD_ID));
        let c_nama = CString::new(nama).map_err(|_| "nama field memuat NUL".to_string())?;
        let c_tipe = CString::new(tipe).map_err(|_| "tipe field memuat NUL".to_string())?;
        let fid = get_fid(env, cls, c_nama.as_ptr(), c_tipe.as_ptr());
        hapus_lokal(env, cls);
        if fid.is_null() {
            let _ = periksa_exception(env);
            return Err(format!(
                "field `{}.{}` bertipe `{}` tidak ketemu — bandingkan dengan \
                 keluaran `javap -s {}`", kelas, nama, tipe, kelas));
        }
        let hasil = baca_field(env, global, fid, tipe, false);
        periksa_exception(env)?;
        hasil
    }
}

unsafe fn baca_field(
    env: Env, wadah: *mut c_void, fid: *mut c_void, tipe: &str, statis: bool,
) -> Result<Value, String> {
    macro_rules! baca {
        ($s:expr, $i:expr, $t:ty) => {{
            let f: extern "C" fn(Env, *mut c_void, *mut c_void) -> $t =
                std::mem::transmute(fungsi(env, if statis { $s } else { $i }));
            f(env, wadah, fid)
        }};
    }
    Ok(match tipe {
        "I" => Value::Int(baca!(idx::GET_STATIC_INT_FIELD, idx::GET_INT_FIELD, i32) as i128),
        "J" => Value::Int(baca!(idx::GET_STATIC_LONG_FIELD, idx::GET_LONG_FIELD, i64) as i128),
        "S" => Value::Int(baca!(idx::GET_STATIC_SHORT_FIELD, idx::GET_SHORT_FIELD, i16) as i128),
        "B" => Value::Int(baca!(idx::GET_STATIC_BYTE_FIELD, idx::GET_BYTE_FIELD, i8) as i128),
        "C" => Value::Int(baca!(idx::GET_STATIC_CHAR_FIELD, idx::GET_CHAR_FIELD, u16) as i128),
        "Z" => Value::Bool(baca!(idx::GET_STATIC_BOOLEAN_FIELD, idx::GET_BOOLEAN_FIELD, u8) != 0),
        "D" => Value::Float(baca!(idx::GET_STATIC_DOUBLE_FIELD, idx::GET_DOUBLE_FIELD, f64)),
        "F" => Value::Float(baca!(idx::GET_STATIC_FLOAT_FIELD, idx::GET_FLOAT_FIELD, f32) as f64),
        "Ljava/lang/String;" => {
            let o = baca!(idx::GET_STATIC_OBJECT_FIELD, idx::GET_OBJECT_FIELD, *mut c_void);
            let s = ambil_string(env, o);
            hapus_lokal(env, o);
            match s { Some(t) => Value::Str(t), None => Value::Nil }
        }
        lain if lain.starts_with('L') && lain.ends_with(';') => {
            let o = baca!(idx::GET_STATIC_OBJECT_FIELD, idx::GET_OBJECT_FIELD, *mut c_void);
            if o.is_null() { return Ok(Value::Nil); }
            let nama = lain[1..lain.len() - 1].replace('/', ".");
            let id = simpan_objek(env, o, &nama)?;
            handle_objek(id, &nama)
        }
        lain => return Err(format!(
            "tipe field `{}` belum didukung — yang bisa: Z B C S I J F D, \
             Ljava/lang/String;, dan objek `Lpaket/Kelas;`", lain)),
    })
}

/// Lepas sebuah handle. Global ref TIDAK dikumpulkan sendiri oleh JVM, jadi
/// tanpa ini objeknya hidup selama proses berjalan.
pub fn lepas_objek(opsi: &[String], id: u64) -> Result<Value, String> {
    let global = {
        let mut t = tabel_objek().lock().map_err(|_| "tabel objek terkunci".to_string())?;
        match t.1.remove(&id) { Some(o) => o.global, None => return Ok(Value::Bool(false)) }
    };
    let env = env_thread_ini(opsi)?;
    unsafe {
        let del: extern "C" fn(Env, *mut c_void) =
            std::mem::transmute(fungsi(env, idx::DELETE_GLOBAL_REF));
        del(env, global);
    }
    Ok(Value::Bool(true))
}

// ─── Pintu builtin ───────────────────────────────────────────────────────

use crate::interpreter::{err_value, ok_value};

fn teks(v: &Value, nama: &str, ke: usize) -> Result<String, String> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        lain => Err(format!(
            "{}: argumen ke-{} harus string, dapat `{}`", nama, ke + 1, lain.type_name())),
    }
}

/// Argumen berlebih ditolak DI SINI, bukan diabaikan.
fn tanpa_argumen(a: &[Value], nama: &str) -> Result<(), String> {
    if a.is_empty() { return Ok(()); }
    Err(format!("{}() tidak menerima argumen, diberi {}", nama, a.len()))
}

pub fn b_tersedia(a: &[Value]) -> Result<Value, String> {
    tanpa_argumen(a, "jvm::tersedia")?;
    Ok(Value::Bool(tersedia()))
}

pub fn b_mulai(a: &[Value]) -> Result<Value, String> {
    if a.len() > 1 {
        return Err(format!("jvm::mulai(opsi) menerima paling banyak 1 argumen, diberi {}", a.len()));
    }
    let opsi = match a.first() {
        None => Vec::new(),
        Some(Value::Array(arr)) => {
            let mut v = Vec::new();
            for (i, x) in arr.iter().enumerate() { v.push(teks(x, "jvm::mulai", i)?); }
            v
        }
        Some(lain) => return Err(format!(
            "jvm::mulai(opsi): opsi harus array of string, dapat `{}`", lain.type_name())),
    };
    {
        let kunci = simpanan().lock().map_err(|_| "kunci JVM rusak".to_string())?;
        if kunci.is_some() {
            // Bukan diam-diam: `JNI_CreateJavaVM` cuma boleh sekali per proses, jadi
            // opsi di panggilan kedua benar-benar tidak berlaku — dan sebuah classpath …
            return Ok(err_value(
                concat!(
                    "JVM sudah jalan di proses ini — `JNI_CreateJavaVM` cuma boleh ",
                    "dipanggil sekali, jadi opsi kali ini TIDAK berlaku. Panggil ",
                    "`jvm::mulai` satu kali saja, di awal")));
        }
    }
    match env_thread_ini(&opsi) {
        Ok(_) => Ok(ok_value(Value::Unit)),
        Err(e) => Ok(err_value(e)),
    }
}

pub fn b_versi(a: &[Value]) -> Result<Value, String> {
    tanpa_argumen(a, "jvm::versi")?;
    match versi(&[]) {
        Ok(v) => Ok(ok_value(Value::Str(v))),
        Err(e) => Ok(err_value(e)),
    }
}

pub fn b_panggil_statis(a: &[Value]) -> Result<Value, String> {
    if a.len() < 3 || a.len() > 4 {
        return Err("jvm::panggil_statis(kelas, metode, tanda_tangan, args)".to_string());
    }
    let kelas = teks(&a[0], "jvm::panggil_statis", 0)?;
    let metode = teks(&a[1], "jvm::panggil_statis", 1)?;
    let sig = teks(&a[2], "jvm::panggil_statis", 2)?;
    let args: Vec<Value> = match a.get(3) {
        None => Vec::new(),
        Some(Value::Array(arr)) => arr.iter().cloned().collect(),
        Some(lain) => return Err(format!(
            "jvm::panggil_statis: argumen ke-4 harus array, dapat `{}`", lain.type_name())),
    };
    match panggil_statis(&[], &kelas, &metode, &sig, &args) {
        Ok(v) => Ok(ok_value(v)),
        Err(e) => Ok(err_value(e)),
    }
}

fn args_array(a: Option<&Value>, nama: &str, ke: usize) -> Result<Vec<Value>, String> {
    match a {
        None => Ok(Vec::new()),
        Some(Value::Array(arr)) => Ok(arr.iter().cloned().collect()),
        Some(lain) => Err(format!(
            "{}: argumen ke-{} harus array, dapat `{}`", nama, ke, lain.type_name())),
    }
}

pub fn b_baru(a: &[Value]) -> Result<Value, String> {
    if a.len() < 2 || a.len() > 3 {
        return Err("jvm::baru(kelas, tanda_tangan, args) — mis. \
                    jvm::baru(\"java.lang.StringBuilder\", \"()V\")".to_string());
    }
    let kelas = teks(&a[0], "jvm::baru", 0)?;
    let sig = teks(&a[1], "jvm::baru", 1)?;
    let args = args_array(a.get(2), "jvm::baru", 3)?;
    match buat_objek(&[], &kelas, &sig, &args) {
        Ok(v) => Ok(ok_value(v)),
        Err(e) => Ok(err_value(e)),
    }
}

pub fn b_panggil(a: &[Value]) -> Result<Value, String> {
    if a.len() < 3 || a.len() > 4 {
        return Err("jvm::panggil(objek, metode, tanda_tangan, args)".to_string());
    }
    let id = match id_dari_handle(&a[0], "jvm::panggil") {
        Ok(i) => i, Err(e) => return Ok(err_value(e)),
    };
    let metode = teks(&a[1], "jvm::panggil", 1)?;
    let sig = teks(&a[2], "jvm::panggil", 2)?;
    let args = args_array(a.get(3), "jvm::panggil", 4)?;
    match panggil_objek(&[], id, &metode, &sig, &args) {
        Ok(v) => Ok(ok_value(v)),
        Err(e) => Ok(err_value(e)),
    }
}

pub fn b_field_statis(a: &[Value]) -> Result<Value, String> {
    if a.len() != 3 {
        return Err("jvm::field_statis(kelas, nama, tipe) — mis. \
                    jvm::field_statis(\"java.lang.Integer\", \"MAX_VALUE\", \"I\")".to_string());
    }
    let kelas = teks(&a[0], "jvm::field_statis", 0)?;
    let nama = teks(&a[1], "jvm::field_statis", 1)?;
    let tipe = teks(&a[2], "jvm::field_statis", 2)?;
    match field_statis(&[], &kelas, &nama, &tipe) {
        Ok(v) => Ok(ok_value(v)),
        Err(e) => Ok(err_value(e)),
    }
}

pub fn b_field(a: &[Value]) -> Result<Value, String> {
    if a.len() != 3 { return Err("jvm::field(objek, nama, tipe)".to_string()); }
    let id = match id_dari_handle(&a[0], "jvm::field") {
        Ok(i) => i, Err(e) => return Ok(err_value(e)),
    };
    let nama = teks(&a[1], "jvm::field", 1)?;
    let tipe = teks(&a[2], "jvm::field", 2)?;
    match field_objek(&[], id, &nama, &tipe) {
        Ok(v) => Ok(ok_value(v)),
        Err(e) => Ok(err_value(e)),
    }
}

pub fn b_ke_teks(a: &[Value]) -> Result<Value, String> {
    if a.len() != 1 { return Err("jvm::ke_teks(objek)".to_string()); }
    let id = match id_dari_handle(&a[0], "jvm::ke_teks") {
        Ok(i) => i, Err(e) => return Ok(err_value(e)),
    };
    match panggil_objek(&[], id, "toString", "()Ljava/lang/String;", &[]) {
        Ok(v) => Ok(ok_value(v)),
        Err(e) => Ok(err_value(e)),
    }
}

pub fn b_lepas(a: &[Value]) -> Result<Value, String> {
    if a.len() != 1 { return Err("jvm::lepas(objek)".to_string()); }
    let id = match id_dari_handle(&a[0], "jvm::lepas") {
        Ok(i) => i, Err(e) => return Ok(err_value(e)),
    };
    match lepas_objek(&[], id) {
        Ok(v) => Ok(ok_value(v)),
        Err(e) => Ok(err_value(e)),
    }
}

/// Berapa objek Java yang masih dipegang. Ada supaya janji "handle bisa
/// dilepas" bisa DIPERIKSA, bukan sekadar dipercaya.
pub fn b_objek_hidup(a: &[Value]) -> Result<Value, String> {
    tanpa_argumen(a, "jvm::objek_hidup")?;
    let t = tabel_objek().lock().map_err(|_| "tabel objek terkunci".to_string())?;
    Ok(Value::Int(t.1.len() as i128))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tanda_tangan_diurai_apa_adanya() {
        let (p, r) = urai_tanda_tangan("(II)I").unwrap();
        assert_eq!(p, vec!["I", "I"]);
        assert_eq!(r, "I");
        let (p, r) = urai_tanda_tangan("()V").unwrap();
        assert!(p.is_empty());
        assert_eq!(r, "V");
        let (p, r) = urai_tanda_tangan("(Ljava/lang/String;D)Ljava/lang/String;").unwrap();
        assert_eq!(p, vec!["Ljava/lang/String;", "D"]);
        assert_eq!(r, "Ljava/lang/String;");
        let (p, r) = urai_tanda_tangan("([I[[Ljava/lang/String;)J").unwrap();
        assert_eq!(p, vec!["[I", "[[Ljava/lang/String;"]);
        assert_eq!(r, "J");
    }

    #[test]
    fn tanda_tangan_rusak_ditolak_bukan_ditebak() {
        for d in ["II)I", "(II", "(IQ)I", "(Ljava/lang/String)I", "(I)IX", "(I)"] {
            assert!(urai_tanda_tangan(d).is_err(), "`{}` harusnya ditolak", d);
        }
    }

    /// Indeks tabel JNI DITURUNKAN ULANG dari `jni.h` milik JDK yang ada di mesin
    /// ini, lalu dibandingkan dengan yang tertulis di modul `idx`.
    #[test]
    fn indeks_jni_cocok_dengan_header_jdk() {
        use std::process::Command;
        let Some(inc) = cari_include_jni() else {
            eprintln!("LEWAT: `jni.h` tidak ada di mesin ini, indeks JNI tidak bisa \
             diturunkan ulang");
            return;
        };
        if Command::new("cc").arg("--version").output().map(|o| !o.status.success()).unwrap_or(true) {
            eprintln!("LEWAT: tidak ada `cc` untuk membaca `jni.h`");
            return;
        }
        let dir = std::env::temp_dir().join(format!("jrg-jni-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let c = dir.join("i.c");
        std::fs::write(&c, r#"
#include <stdio.h>
#include <stddef.h>
#include <jni.h>
#define P(f) printf("%s %zu\n", #f, offsetof(struct JNINativeInterface_, f)/sizeof(void*))
#define Q(f) printf("VM_%s %zu\n", #f, offsetof(struct JNIInvokeInterface_, f)/sizeof(void*))
int main(void){
  P(GetVersion);P(FindClass);P(GetObjectClass);P(GetMethodID);P(CallObjectMethodA);
  P(ExceptionOccurred);P(ExceptionClear);P(DeleteLocalRef);P(GetStaticMethodID);
  P(CallStaticObjectMethodA);P(CallStaticBooleanMethodA);P(CallStaticByteMethodA);
  P(CallStaticCharMethodA);P(CallStaticShortMethodA);P(CallStaticIntMethodA);
  P(CallStaticLongMethodA);P(CallStaticFloatMethodA);P(CallStaticDoubleMethodA);
  P(CallStaticVoidMethodA);P(NewStringUTF);P(GetStringUTFChars);P(ReleaseStringUTFChars);
  P(NewGlobalRef);P(DeleteGlobalRef);P(NewObjectA);P(IsInstanceOf);
  P(CallBooleanMethodA);P(CallByteMethodA);P(CallCharMethodA);P(CallShortMethodA);
  P(CallIntMethodA);P(CallLongMethodA);P(CallFloatMethodA);P(CallDoubleMethodA);
  P(CallVoidMethodA);P(GetFieldID);P(GetObjectField);P(GetBooleanField);
  P(GetByteField);P(GetCharField);P(GetShortField);P(GetIntField);P(GetLongField);
  P(GetFloatField);P(GetDoubleField);P(GetStaticFieldID);P(GetStaticObjectField);
  P(GetStaticBooleanField);P(GetStaticByteField);P(GetStaticCharField);
  P(GetStaticShortField);P(GetStaticIntField);P(GetStaticLongField);
  P(GetStaticFloatField);P(GetStaticDoubleField);P(GetArrayLength);
  P(NewObjectArray);P(GetObjectArrayElement);P(SetObjectArrayElement);
  Q(AttachCurrentThread);Q(GetEnv);
  return 0;
}
"#).expect("tulis c");
        let bin = dir.join("i");
        let out = Command::new("cc")
            .arg("-I").arg(&inc).arg("-I").arg(inc.join("linux"))
            .arg("-o").arg(&bin).arg(&c).output().expect("cc");
        if !out.status.success() {
            eprintln!("LEWAT: `jni.h` ada tapi tidak bisa dikompilasi: {}",
                      String::from_utf8_lossy(&out.stderr));
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let hasil = Command::new(&bin).output().expect("jalankan");
        let teks = String::from_utf8_lossy(&hasil.stdout).to_string();
        let ambil = |n: &str| -> usize {
            teks.lines().find(|l| l.starts_with(&format!("{} ", n)))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| panic!("`{}` tidak ada di keluaran header", n))
        };
        assert_eq!(ambil("GetVersion"), idx::GET_VERSION);
        assert_eq!(ambil("FindClass"), idx::FIND_CLASS);
        assert_eq!(ambil("GetObjectClass"), idx::GET_OBJECT_CLASS);
        assert_eq!(ambil("GetMethodID"), idx::GET_METHOD_ID);
        assert_eq!(ambil("CallObjectMethodA"), idx::CALL_OBJECT_METHOD_A);
        assert_eq!(ambil("ExceptionOccurred"), idx::EXCEPTION_OCCURRED);
        assert_eq!(ambil("ExceptionClear"), idx::EXCEPTION_CLEAR);
        assert_eq!(ambil("DeleteLocalRef"), idx::DELETE_LOCAL_REF);
        assert_eq!(ambil("GetStaticMethodID"), idx::GET_STATIC_METHOD_ID);
        assert_eq!(ambil("CallStaticObjectMethodA"), idx::CALL_STATIC_OBJECT_A);
        assert_eq!(ambil("CallStaticBooleanMethodA"), idx::CALL_STATIC_BOOLEAN_A);
        assert_eq!(ambil("CallStaticByteMethodA"), idx::CALL_STATIC_BYTE_A);
        assert_eq!(ambil("CallStaticCharMethodA"), idx::CALL_STATIC_CHAR_A);
        assert_eq!(ambil("CallStaticShortMethodA"), idx::CALL_STATIC_SHORT_A);
        assert_eq!(ambil("CallStaticIntMethodA"), idx::CALL_STATIC_INT_A);
        assert_eq!(ambil("CallStaticLongMethodA"), idx::CALL_STATIC_LONG_A);
        assert_eq!(ambil("CallStaticFloatMethodA"), idx::CALL_STATIC_FLOAT_A);
        assert_eq!(ambil("CallStaticDoubleMethodA"), idx::CALL_STATIC_DOUBLE_A);
        assert_eq!(ambil("CallStaticVoidMethodA"), idx::CALL_STATIC_VOID_A);
        assert_eq!(ambil("NewStringUTF"), idx::NEW_STRING_UTF);
        assert_eq!(ambil("GetStringUTFChars"), idx::GET_STRING_UTF_CHARS);
        assert_eq!(ambil("ReleaseStringUTFChars"), idx::RELEASE_STRING_UTF_CHARS);
        // Indeks untuk objek/metode instance/field — ditambahkan Fase FFI-KOTLIN.
        assert_eq!(ambil("NewGlobalRef"), idx::NEW_GLOBAL_REF);
        assert_eq!(ambil("DeleteGlobalRef"), idx::DELETE_GLOBAL_REF);
        assert_eq!(ambil("NewObjectA"), idx::NEW_OBJECT_A);
        assert_eq!(ambil("IsInstanceOf"), idx::IS_INSTANCE_OF);
        assert_eq!(ambil("CallBooleanMethodA"), idx::CALL_BOOLEAN_A);
        assert_eq!(ambil("CallByteMethodA"), idx::CALL_BYTE_A);
        assert_eq!(ambil("CallCharMethodA"), idx::CALL_CHAR_A);
        assert_eq!(ambil("CallShortMethodA"), idx::CALL_SHORT_A);
        assert_eq!(ambil("CallIntMethodA"), idx::CALL_INT_A);
        assert_eq!(ambil("CallLongMethodA"), idx::CALL_LONG_A);
        assert_eq!(ambil("CallFloatMethodA"), idx::CALL_FLOAT_A);
        assert_eq!(ambil("CallDoubleMethodA"), idx::CALL_DOUBLE_A);
        assert_eq!(ambil("CallVoidMethodA"), idx::CALL_VOID_A);
        assert_eq!(ambil("GetFieldID"), idx::GET_FIELD_ID);
        assert_eq!(ambil("GetObjectField"), idx::GET_OBJECT_FIELD);
        assert_eq!(ambil("GetBooleanField"), idx::GET_BOOLEAN_FIELD);
        assert_eq!(ambil("GetByteField"), idx::GET_BYTE_FIELD);
        assert_eq!(ambil("GetCharField"), idx::GET_CHAR_FIELD);
        assert_eq!(ambil("GetShortField"), idx::GET_SHORT_FIELD);
        assert_eq!(ambil("GetIntField"), idx::GET_INT_FIELD);
        assert_eq!(ambil("GetLongField"), idx::GET_LONG_FIELD);
        assert_eq!(ambil("GetFloatField"), idx::GET_FLOAT_FIELD);
        assert_eq!(ambil("GetDoubleField"), idx::GET_DOUBLE_FIELD);
        assert_eq!(ambil("GetStaticFieldID"), idx::GET_STATIC_FIELD_ID);
        assert_eq!(ambil("GetStaticObjectField"), idx::GET_STATIC_OBJECT_FIELD);
        assert_eq!(ambil("GetStaticBooleanField"), idx::GET_STATIC_BOOLEAN_FIELD);
        assert_eq!(ambil("GetStaticByteField"), idx::GET_STATIC_BYTE_FIELD);
        assert_eq!(ambil("GetStaticCharField"), idx::GET_STATIC_CHAR_FIELD);
        assert_eq!(ambil("GetStaticShortField"), idx::GET_STATIC_SHORT_FIELD);
        assert_eq!(ambil("GetStaticIntField"), idx::GET_STATIC_INT_FIELD);
        assert_eq!(ambil("GetStaticLongField"), idx::GET_STATIC_LONG_FIELD);
        assert_eq!(ambil("GetStaticFloatField"), idx::GET_STATIC_FLOAT_FIELD);
        assert_eq!(ambil("GetStaticDoubleField"), idx::GET_STATIC_DOUBLE_FIELD);
        assert_eq!(ambil("GetArrayLength"), idx::GET_ARRAY_LENGTH);
        assert_eq!(ambil("NewObjectArray"), idx::NEW_OBJECT_ARRAY);
        assert_eq!(ambil("GetObjectArrayElement"), idx::GET_OBJECT_ARRAY_ELEMENT);
        assert_eq!(ambil("SetObjectArrayElement"), idx::SET_OBJECT_ARRAY_ELEMENT);
        assert_eq!(ambil("VM_AttachCurrentThread"), idx_vm::ATTACH_CURRENT_THREAD);
        assert_eq!(ambil("VM_GetEnv"), idx_vm::GET_ENV);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn cari_include_jni() -> Option<std::path::PathBuf> {
        if let Ok(h) = std::env::var("JAVA_HOME") {
            let p = std::path::PathBuf::from(h).join("include");
            if p.join("jni.h").exists() { return Some(p); }
        }
        let baca = std::fs::read_dir("/usr/lib/jvm").ok()?;
        let mut anak: Vec<_> = baca.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        anak.sort();
        anak.into_iter().map(|p| p.join("include")).find(|p| p.join("jni.h").exists())
    }
}
