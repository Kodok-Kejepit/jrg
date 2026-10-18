// src/llvm/mod.rs — jrgc LLVM backend — src/llvm/mod.rs

pub mod types;
pub mod mangle;
pub mod sema;
pub mod codegen;
mod codegen_fungsi;
mod codegen_stmt;
pub mod codegen_expr;
pub mod codegen_access;
// Badan `impl Codegen` untuk akses/panggilan tersebar di empat berkas ini.
mod codegen_agregat;
mod codegen_panggil;
mod codegen_opsi;
mod codegen_forrange;
pub mod codegen_builtins;
pub mod codegen_pattern;

/// Kesetaraan struktural untuk nilai agregat (array/slice/tuple/struct/unit).
pub mod codegen_eq;

/// Namespace `arr::` — koleksi yang mengembalikan nilai baru.
pub mod codegen_arr;

/// Namespace `dict::` dan literal `{ "k": v }`.
pub mod codegen_any;
pub mod codegen_dict;

/// `defer` — lingkup fungsi, LIFO, argumen dibekukan saat pendaftaran.
pub mod codegen_tunda;

/// Fungsi sebagai nilai: `%JrgFn = { ptr kode, ptr lingkungan }`.
pub mod codegen_fnval;

/// Closure: tangkapan sebagai kotak bersama di heap.
pub mod codegen_closure;

/// Method tingkat-tinggi: `map` `filter` `reduce` `find` `each` `any` `all`.
pub mod codegen_hof;

/// `jawa::`/`jawir::` — alokasi manual di atas registri berid.
pub mod codegen_jawa;
pub mod codegen_konversi;

/// Menyimpulkan tipe return fungsi yang tidak beranotasi.
pub mod infer;
mod infer_tumbuh;
pub mod infer_bentrok;

/// Error backend ini, dengan lokasinya — lihat berkasnya untuk alasan
/// kenapa penandanya diurai alih-alih tipenya diganti.
pub mod galat;
pub mod modul;
pub mod generik;
mod codegen_tumbuh;
mod codegen_korutin;
mod codegen_sinkron;
mod codegen_jembatan;
mod tulis_modul;
pub mod apk;
pub mod apk_bahasa;

/// Program yang sedang dikompilasi memanggil builtin lewat jembatan: pustakanya ikut ditautkan.
pub(crate) static PAKAI_JEMBATAN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use crate::parser::ParsedFile;

use codegen::{Codegen, CodegenOptions};
use sema::{GlobalTables, TypeSubst};

/// Lex/parse → sema → monomorphizing codegen → rendered `.ll` text.
pub fn generate_ir(file: &ParsedFile) -> Result<String, Vec<String>> {
    generate_ir_with_options(file, CodegenOptions::default())
}

/// Variant of [`generate_ir`] used by the native driver for opt-in
/// semantics-changing codegen modes such as fast floating-point math.
pub fn generate_ir_with_options(
    file: &ParsedFile,
    options: CodegenOptions,
) -> Result<String, Vec<String>> {
    generate_ir_dgn_jalur(file, options, &[])
}

/// `generate_ir_with_options`, dengan direktori tempat `import` dicari (Fase MODUL-LLVM).
pub fn generate_ir_dgn_jalur(
    file: &ParsedFile,
    options: CodegenOptions,
    jalur: &[std::path::PathBuf],
) -> Result<String, Vec<String>> {
    // Modul `std/` yang tidak ada ditolak dengan kalimat acuan — dulu import-nya diabaikan
    // diam-diam dan programnya jalan seolah tidak menulis apa-apa.
    for imp in &file.imports {
        if let Some(n) = crate::module::ModuleResolver::std_module_name(&imp.path) {
            // Namespace bawaan (`import "std/fmt"`) dilewati, seperti dua mesin lain dan `modul::muat`.
            if !crate::interpreter::stdlib_src::nama_modul().contains(&n)
                && !crate::module::ModuleResolver::is_stdlib(&imp.path) {
                return Err(vec![format!("[{}:{}] {}", imp.span.lo.0, imp.span.hi.0,
                    crate::interpreter::stdlib_src::pesan_modul_std_tak_ada(&imp.path))]);
            }
        }
    }
    let unit = modul::muat(file, jalur).map_err(|e| vec![e])?;
    let mut tables = GlobalTables::new();
    // Unit 0 sudah ditulis ulang `ruang_tipe` (label tipe dan trait), bukan `file` aslinya.
    tables.collect(&unit[0].file);
    for (n, u) in unit.iter().enumerate().skip(1) { tables.collect_unit(&u.file, n); }
    tables.alias_unit = unit.iter().map(|u| u.alias.clone()).collect();

    // Visibilitas field: aturannya punya satu definisi di
    // `crate::vm::visibilitas` (lahir di sana saat asimetri yang sama ditutup …
    crate::vm::visibilitas::pasang_dari_deklarasi(
        unit.iter().flat_map(|u| u.file.items.iter()).filter_map(|i| match i {
            crate::parser::Item::Struct(d) => Some(d),
            _ => None,
        }),
    );

    let mut cg = Codegen::new_with_options(tables, options);
    cg.unit_berkas = unit.iter().map(|u| u.file_id).collect();
    cg.unit_lapis = unit.iter().position(|u| u.nama == modul::NAMA_LAPIS);
    // Dibaca dari bentuk AST unit milik program (bukan lapis): murah, dan cuma menentukan
    // apakah `main` memanggil penunggu goroutine di ujungnya.
    cg.berkonkurensi = unit.iter().filter(|u| u.nama != modul::NAMA_LAPIS).any(|u| {
        let teks = format!("{:?}", u.file.items);
        ["Spawn(", "\"Future\"", "\"Task\""].iter().any(|k| teks.contains(k))
    });
    cg.register_all_struct_enum_layouts();
    if !cg.errors.is_empty() {
        return Err(cg.errors.clone());
    }

    let main_decl = match cg.tables.functions.get("main").cloned() {
        Some(d) => d,
        None => return Err(vec!["fungsi `main` tidak ditemukan — backend LLVM butuh titik masuk `pub fn main()`".to_string()]),
    };
    if !main_decl.generics.params.is_empty() {
        return Err(vec!["`main` tidak boleh generik".to_string()]);
    }
    if !main_decl.params.is_empty() {
        return Err(vec!["`main` tidak boleh punya parameter di backend LLVM v1".to_string()]);
    }
    let main_ret_ty = match &main_decl.ret_type {
        Some(ret) => cg
            .resolve_type_with_subst(ret, &TypeSubst::new())
            .map_err(|error| vec![format!("tipe return `main` tidak valid: {}", error)])?,
        None => types::JrgType::Unit,
    };
    if main_ret_ty != types::JrgType::Unit && !main_ret_ty.is_int() {
        return Err(vec![format!(
            "`main` hanya boleh mengembalikan integer exit code atau unit, bukan `{}`",
            main_ret_ty.display_name()
        )]);
    }

    cg.enqueue_fn("main", &main_decl, TypeSubst::new(), "main".to_string(), None);
    cg.drain_worklist();

    if !cg.errors.is_empty() {
        let mut errs = cg.errors.clone();
        // Sebabnya ditambahkan di DEPAN, bukan di belakang.
        if !cg.var_kosong_tak_tersimpulkan.is_empty() {
            let mut nama: Vec<String> = cg.var_kosong_tak_tersimpulkan.iter()
                .map(|n| modul::tampil(n).to_string()).collect();
            nama.sort();
            nama.dedup();
            let daftar: Vec<String> = nama.iter().map(|n| format!("`{}`", n)).collect();
            errs.insert(0, format!(
                "{} dideklarasikan sebagai array kosong (`[]`) dan tipe elemennya tidak bisa disimpulkan dari program ini, jadi backend `--emit-llvm` memakai `int` sebagai bawaan — pemakaian dengan tipe elemen lain lalu ditolak, dan itu yang dilaporkan di bawah\n  bantuan: beri anotasi eksplisit, misalnya `var {}: []string = []`\n  catatan: kalau variabel itu memang dipakai dengan tipe elemen yang BERBEDA di tempat berbeda, itu pengetikan dinamis — interpreter dan VM membolehkannya karena keduanya menyimpan tipe bersama nilainya saat jalan, sedangkan backend statis butuh nilai bertag untuk itu",
                daftar.join(" dan "),
                nama[0]));
        }
        errs.truncate(50);
        return Err(errs);
    }
    Ok(cg.render())
}

// ─── clang / runtime driver ─────────────────────────────────────────────

const CLANG_CANDIDATES: &[&str] = &[
    "clang-21",
    "clang-20",
    "clang-19",
    "clang-18",
    "clang-17",
    "clang-16",
    "clang-15",
    "clang-14",
    "clang",
];
const LLVM_PROFDATA_CANDIDATES: &[&str] = &[
    "llvm-profdata-21",
    "llvm-profdata-20",
    "llvm-profdata-19",
    "llvm-profdata-18",
    "llvm-profdata-17",
    "llvm-profdata-16",
    "llvm-profdata-15",
    "llvm-profdata-14",
    "llvm-profdata",
];
const LLVM_OPT_CANDIDATES: &[&str] = &[
    "opt-21",
    "opt-20",
    "opt-19",
    "opt-18",
    "opt-17",
    "opt-16",
    "opt-15",
    "opt-14",
    "opt",
];
const LLD_CANDIDATES: &[&str] = &[
    "ld.lld-21",
    "ld.lld-20",
    "ld.lld-19",
    "ld.lld-18",
    "ld.lld-17",
    "ld.lld-16",
    "ld.lld-15",
    "ld.lld-14",
    "ld.lld",
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PgoMode {
    #[default]
    Off,
    Generate(PathBuf),
    Use(PathBuf),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LtoMode {
    #[default]
    Off,
    Thin,
    Full,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LinkerMode {
    #[default]
    System,
    AutoLld,
    RequireLld,
}

#[derive(Clone, Debug)]
pub struct NativeCompileOptions {
    pub opt_level: u8,
    pub target: Option<String>,
    pub native_cpu: bool,
    pub aggressive: bool,
    pub fast_math: bool,
    pub gc_sections: bool,
    pub strip: bool,
    pub lto: LtoMode,
    pub linker: LinkerMode,
    pub pgo: PgoMode,
}

impl Default for NativeCompileOptions {
    fn default() -> Self {
        Self {
            opt_level: 2,
            target: None,
            native_cpu: false,
            aggressive: false,
            fast_math: false,
            gc_sections: true,
            strip: false,
            lto: LtoMode::Off,
            linker: LinkerMode::System,
            pgo: PgoMode::Off,
        }
    }
}

impl NativeCompileOptions {
    fn validate(&self) -> Result<(), String> {
        if self.opt_level > 3 {
            return Err(format!(
                "optimization level `-O{}` tidak valid; gunakan 0, 1, 2, atau 3",
                self.opt_level
            ));
        }
        if self.native_cpu && self.target.is_some() {
            return Err("`--native-cpu` tidak bisa digabung dengan `--target` karena CPU host belum tentu cocok dengan target".to_string());
        }
        if let PgoMode::Use(path) = &self.pgo {
            if !path.exists() {
                return Err(format!("profil PGO tidak ditemukan: {}", path.display()));
            }
        }
        Ok(())
    }
}

pub fn find_clang() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("JRGC_CLANG") {
        if !p.is_empty() { return Ok(PathBuf::from(p)); }
    }
    for cand in CLANG_CANDIDATES {
        if let Ok(out) = Command::new("which").arg(cand).output() {
            if out.status.success() {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !path.is_empty() { return Ok(PathBuf::from(path)); }
            }
        }
    }
    Err("tidak menemukan `clang` di PATH (dicoba: clang-21 turun ke clang-14, lalu clang). \
         Set env JRGC_CLANG ke path clang, atau install clang (mis. `apt install clang`).".to_string())
}

fn find_llvm_profdata(clang: &Path) -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("JRGC_LLVM_PROFDATA") {
        if !p.is_empty() { return Ok(PathBuf::from(p)); }
    }
    if let Some(parent) = clang.parent() {
        let sibling = parent.join("llvm-profdata");
        if sibling.is_file() {
            return Ok(sibling);
        }
    }
    if let Some(version) = clang.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("clang-"))
    {
        let versioned = format!("llvm-profdata-{}", version);
        if let Ok(out) = Command::new("which").arg(&versioned).output() {
            if out.status.success() {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !path.is_empty() { return Ok(PathBuf::from(path)); }
            }
        }
        let path = PathBuf::from(format!("/usr/lib/llvm-{}/bin/llvm-profdata", version));
        if path.is_file() {
            return Ok(path);
        }
    }
    for cand in LLVM_PROFDATA_CANDIDATES {
        if let Ok(out) = Command::new("which").arg(cand).output() {
            if out.status.success() {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !path.is_empty() { return Ok(PathBuf::from(path)); }
            }
        }
    }
    for version in (14..=21).rev() {
        let path = PathBuf::from(format!("/usr/lib/llvm-{}/bin/llvm-profdata", version));
        if path.is_file() {
            return Ok(path);
        }
    }
    Err("tidak menemukan `llvm-profdata`; set JRGC_LLVM_PROFDATA, install paket LLVM, atau berikan file `.profdata` yang sudah digabung".to_string())
}

fn find_llvm_opt(clang: &Path) -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("JRGC_LLVM_OPT") {
        if !p.is_empty() { return Ok(PathBuf::from(p)); }
    }
    if let Some(parent) = clang.parent() {
        let sibling = parent.join("opt");
        if sibling.is_file() {
            return Ok(sibling);
        }
    }
    if let Some(version) = clang.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("clang-"))
    {
        let versioned = format!("opt-{}", version);
        if let Ok(out) = Command::new("which").arg(&versioned).output() {
            if out.status.success() {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !path.is_empty() { return Ok(PathBuf::from(path)); }
            }
        }
        let path = PathBuf::from(format!("/usr/lib/llvm-{}/bin/opt", version));
        if path.is_file() {
            return Ok(path);
        }
    }
    for cand in LLVM_OPT_CANDIDATES {
        if let Ok(out) = Command::new("which").arg(cand).output() {
            if out.status.success() {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !path.is_empty() { return Ok(PathBuf::from(path)); }
            }
        }
    }
    Err("tidak menemukan LLVM `opt`; set JRGC_LLVM_OPT atau install paket tools LLVM yang versinya cocok dengan clang".to_string())
}

fn find_lld(clang: &Path) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("JRGC_LLD") {
        if !p.is_empty() {
            let path = PathBuf::from(p);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    if let Some(parent) = clang.parent() {
        let sibling = parent.join("ld.lld");
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    if let Some(version) = clang.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("clang-"))
    {
        for path in [
            PathBuf::from(format!("/usr/lib/llvm-{}/bin/ld.lld", version)),
            PathBuf::from(format!("/usr/local/opt/llvm@{}/bin/ld.lld", version)),
        ] {
            if path.is_file() {
                return Some(path);
            }
        }
    }
    for cand in LLD_CANDIDATES {
        if let Ok(out) = Command::new("which").arg(cand).output() {
            if out.status.success() {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !path.is_empty() {
                    return Some(PathBuf::from(path));
                }
            }
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TargetLinkerFlavor {
    Elf,
    Darwin,
    Msvc,
}

fn target_linker_flavor(target: Option<&str>) -> TargetLinkerFlavor {
    match target {
        Some(triple) if triple.contains("apple-darwin") => TargetLinkerFlavor::Darwin,
        Some(triple) if triple.contains("windows-msvc") => TargetLinkerFlavor::Msvc,
        Some(_) => TargetLinkerFlavor::Elf,
        None if cfg!(target_os = "macos") => TargetLinkerFlavor::Darwin,
        None if cfg!(target_env = "msvc") => TargetLinkerFlavor::Msvc,
        None => TargetLinkerFlavor::Elf,
    }
}

/// Pustaka JADI yang ikut arsip rilis (`libjrgrt.a`, jembatan): `JRGC_PUSTAKA_DIR`, di samping
/// binernya, atau `../lib/jrg/`. Tanpa ini biner rilis mencari sumber di jalur mesin PEMBANGUNNYA.
fn pustaka_jadi(nama: &str) -> Option<PathBuf> {
    let mut calon: Vec<PathBuf> = Vec::new();
    if let Ok(d) = std::env::var("JRGC_PUSTAKA_DIR") { if !d.is_empty() { calon.push(PathBuf::from(d).join(nama)); } }
    if let Some(d) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        calon.push(d.join(nama));
        calon.push(d.join("..").join("lib").join("jrg").join(nama));
    }
    calon.into_iter().find(|p| p.is_file())
}

fn runtime_dir() -> PathBuf {
    if let Ok(p) = std::env::var("JRGC_RUNTIME_DIR") {
        if !p.is_empty() { return PathBuf::from(p); }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("runtime")
}

/// Lazily builds (`cargo build --release`) the `jrgrt` native runtime
/// staticlib the first time it's needed, or whenever its source is newer than …
pub fn ensure_runtime(verbose: bool, target: Option<&str>) -> Result<PathBuf, String> {
    if target.is_none() {
        let nama = if cfg!(target_env = "msvc") { "jrgrt.lib" } else { "libjrgrt.a" };
        if let Some(p) = pustaka_jadi(nama) { return Ok(p); }
    }
    let dir = runtime_dir();
    let manifest = dir.join("Cargo.toml");
    if !manifest.exists() {
        return Err(format!(
            "runtime jrgrt tidak ditemukan: tidak ada `libjrgrt.a` jadi di samping jrgc (atau di `JRGC_PUSTAKA_DIR`), \
             dan sumbernya tidak ada di {}\n  bantuan: pakai arsip rilis yang membawa pustakanya, atau set \
             `JRGC_RUNTIME_DIR` ke folder `runtime/` sumber JRG",
            dir.display()
        ));
    }
    let lib_name = if target.is_some_and(|triple| triple.contains("windows-msvc")) {
        "jrgrt.lib"
    } else {
        "libjrgrt.a"
    };
    let lib = match target {
        Some(triple) => dir.join("target").join(triple).join("release").join(lib_name),
        None => dir.join("target").join("release").join(lib_name),
    };
    let needs_build = if !lib.exists() {
        true
    } else {
        let lib_mtime = std::fs::metadata(&lib).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
        [
            manifest.clone(),
            dir.join("Cargo.lock"),
            dir.join("src").join("lib.rs"),
        ]
        .iter()
        .any(|input| {
            std::fs::metadata(input)
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::now())
                > lib_mtime
        })
    };
    if needs_build {
        if verbose { eprintln!("[jrgc] membangun runtime native (jrgrt) — sekali saja, di-cache setelahnya..."); }
        let mut cmd = Command::new("cargo");
        cmd.args(["build", "--release"]);
        if let Some(triple) = target {
            cmd.arg("--target").arg(triple);
        }
        let out = crate::putar::jalankan(cmd.current_dir(&dir), "membangun runtime jrgrt (sekali, di-cache)")
            .map_err(|e| format!("gagal menjalankan `cargo build` untuk runtime: {}", e))?;
        if !out.status.success() {
            return Err(format!(
                "gagal membangun runtime jrgrt{}:\n{}",
                target.map(|t| format!(" untuk target `{}`", t)).unwrap_or_default(),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        segarkan_cap(&lib);
        if verbose { eprintln!("[jrgc] runtime jrgrt siap: {}", lib.display()); }
    }
    if !lib.exists() {
        return Err(format!("build runtime selesai tapi {} tidak ditemukan", lib.display()));
    }
    Ok(lib)
}

/// Cargo yang menjawab "sudah segar" tidak menyentuh pustakanya; tanpa ini sumber yang lebih baru (tapi
/// tidak dipakai pustaka itu) memanggil cargo lagi di SETIAP kompilasi.
fn segarkan_cap(lib: &Path) {
    if let Ok(f) = std::fs::File::options().append(true).open(lib) { let _ = f.set_modified(SystemTime::now()); }
}

/// Pustaka dinamis jembatan (`jembatan/`), dibangun kalau belum ada atau lebih tua dari sumbernya.
pub fn ensure_jembatan(verbose: bool) -> Result<PathBuf, String> {
    ensure_jembatan_untuk(verbose, None)
}

/// Jembatan untuk target lain: `(triple Rust, penaut, ar)`, mis. Android lewat clang NDK.
pub fn ensure_jembatan_untuk(verbose: bool, sasaran: Option<(&str, &Path, &Path)>) -> Result<PathBuf, String> {
    if sasaran.is_none() {
        let nama = if cfg!(target_os = "macos") { "libjrgjembatan.dylib" } else { "libjrgjembatan.so" };
        if let Some(p) = pustaka_jadi(nama) { return Ok(p); }
    }
    let dir = match std::env::var("JRGC_JEMBATAN_DIR") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => Path::new(env!("CARGO_MANIFEST_DIR")).join("jembatan"),
    };
    if !dir.join("Cargo.toml").exists() {
        return Err(format!("jembatan builtin tidak ditemukan: tidak ada `libjrgjembatan` jadi di samping jrgc (atau di \
            `JRGC_PUSTAKA_DIR`), dan sumbernya tidak ada di {}\n  bantuan: pakai arsip rilis yang membawa pustakanya, \
            atau set `JRGC_JEMBATAN_DIR` ke folder `jembatan/` sumber JRG", dir.display()));
    }
    let nama = if cfg!(target_os = "macos") { "libjrgjembatan.dylib" } else { "libjrgjembatan.so" };
    let nama = if sasaran.is_some() { "libjrgjembatan.so" } else { nama };
    let lib = match sasaran {
        Some((t, _, _)) => dir.join("target").join(t).join("release").join(nama),
        None => dir.join("target").join("release").join(nama),
    };
    fn terbaru(d: &Path, kini: &mut SystemTime) {
        let Ok(isi) = std::fs::read_dir(d) else { return };
        for e in isi.flatten() {
            let p = e.path();
            if p.is_dir() { terbaru(&p, kini); continue; }
            if p.extension().map_or(false, |x| x == "rs") {
                if let Ok(t) = e.metadata().and_then(|m| m.modified()) { if t > *kini { *kini = t; } }
            }
        }
    }
    let perlu = match std::fs::metadata(&lib).and_then(|m| m.modified()) {
        Err(_) => true,
        Ok(t) => {
            let mut kini = SystemTime::UNIX_EPOCH;
            terbaru(&dir.join("src"), &mut kini);
            terbaru(&dir.parent().unwrap_or(&dir).join("src"), &mut kini);
            kini > t
        }
    };
    if perlu {
        if verbose { eprintln!("[jrgc] membangun jembatan builtin (libjrgjembatan) — beberapa menit, di-cache setelahnya..."); }
        let mut cmd = Command::new("cargo");
        cmd.args(["build", "--release", "-j", "1"]).current_dir(&dir);
        if let Some((t, penaut, ar)) = sasaran {
            let besar = t.to_uppercase().replace('-', "_");
            cmd.arg("--target").arg(t)
                .env(format!("CARGO_TARGET_{}_LINKER", besar), penaut)
                .env(format!("CARGO_TARGET_{}_RUSTFLAGS", besar), "-C link-arg=-Wl,-z,max-page-size=16384")
                .env(format!("CC_{}", t.replace('-', "_")), penaut)
                .env(format!("AR_{}", t.replace('-', "_")), ar);
        }
        let out = crate::putar::jalankan(&mut cmd, "membangun jembatan builtin (sekali, di-cache)")
            .map_err(|e| format!("gagal menjalankan `cargo build` untuk jembatan: {}", e))?;
        if !out.status.success() {
            return Err(format!("gagal membangun jembatan builtin:\n{}", String::from_utf8_lossy(&out.stderr)));
        }
        segarkan_cap(&lib);
    }
    if !lib.exists() { return Err(format!("build jembatan selesai tapi {} tidak ditemukan", lib.display())); }
    Ok(lib)
}

/// Full compile: write `<output>.ll` next to the requested binary (always
/// kept as a debugging artifact, like `-save-temps`), then invoke clang to …
pub fn compile_ir_to_native(
    ir: &str,
    output: &Path,
    options: &NativeCompileOptions,
    verbose: bool,
) -> Result<PathBuf, String> {
    options.validate()?;
    let clang = find_clang()?;
    let runtime_lib = ensure_runtime(verbose, options.target.as_deref())?;

    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("gagal membuat direktori output {}: {}", parent.display(), e))?;
        }
    }
    if let PgoMode::Generate(path) = &options.pgo {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("gagal membuat direktori profil PGO {}: {}", parent.display(), e))?;
            }
        }
    }

    let ll_path = with_extension_ll(output);
    let instrumented_ir;
    let ir_to_write = if let PgoMode::Generate(path) = &options.pgo {
        instrumented_ir = inject_profile_filename(ir, path);
        instrumented_ir.as_str()
    } else {
        ir
    };
    std::fs::write(&ll_path, ir_to_write)
        .map_err(|e| format!("gagal menulis {}: {}", ll_path.display(), e))?;
    if verbose { eprintln!("[jrgc] LLVM IR ditulis ke {}", ll_path.display()); }

    let pgo_ll_path = with_extension_pgo_ll(output);
    let clang_input = match &options.pgo {
        PgoMode::Off => ll_path.clone(),
        PgoMode::Generate(_) => {
            run_opt_pgo_generate(&clang, &ll_path, &pgo_ll_path, verbose)?;
            pgo_ll_path.clone()
        }
        PgoMode::Use(path) => {
            let profile = resolve_pgo_profile(path, &clang, verbose)?;
            run_opt_pgo_use(&clang, &ll_path, &pgo_ll_path, &profile, verbose)?;
            pgo_ll_path.clone()
        }
    };

    let opt_flag = format!("-O{}", options.opt_level.min(3));
    let linker_flavor = target_linker_flavor(options.target.as_deref());
    let lld = match options.linker {
        LinkerMode::System => None,
        LinkerMode::AutoLld => find_lld(&clang),
        LinkerMode::RequireLld => Some(find_lld(&clang).ok_or_else(|| {
            "linker lld diminta tetapi tidak ditemukan; install lld atau set JRGC_LLD ke path `ld.lld`".to_string()
        })?),
    };
    let mut cmd = Command::new(&clang);
    // Berkas C (primitif + penjaga stack) DI DEPAN arsip runtime: ia memanggil `jrg_panic`, dan
    // penaut cuma menarik anggota arsip untuk simbol yang sudah diminta di sebelah kirinya.
    let jaga = tulis_jaga_stack()?;
    cmd.arg(&clang_input).arg(&jaga).arg(&runtime_lib).arg(&opt_flag);
    if PAKAI_JEMBATAN.load(std::sync::atomic::Ordering::Relaxed) {
        let lib = ensure_jembatan(verbose)?;
        let dir = lib.parent().map(|d| d.display().to_string()).unwrap_or_default();
        cmd.arg(&lib).arg(format!("-Wl,-rpath,{}", dir));
    }
    if let Some(t) = &options.target {
        cmd.arg("-target").arg(t);
    }
    if options.native_cpu {
        cmd.arg("-march=native").arg("-mtune=native");
    }
    if options.aggressive {
        cmd.arg("-funroll-loops")
            .arg("-fvectorize")
            .arg("-fslp-vectorize")
            .arg("-fomit-frame-pointer");
    }
    if options.fast_math {
        cmd.arg("-fno-math-errno")
            .arg("-fno-trapping-math")
            .arg("-ffp-contract=fast");
    }
    match options.lto {
        LtoMode::Off => {}
        LtoMode::Thin => {
            cmd.arg("-flto=thin");
        }
        LtoMode::Full => {
            cmd.arg("-flto");
        }
    }
    if let Some(path) = &lld {
        cmd.arg(format!("-fuse-ld={}", path.display()));
    }
    if options.gc_sections {
        cmd.arg("-ffunction-sections").arg("-fdata-sections");
        match linker_flavor {
            TargetLinkerFlavor::Elf => {
                cmd.arg("-Wl,--gc-sections").arg("-Wl,-O2");
            }
            TargetLinkerFlavor::Darwin => {
                cmd.arg("-Wl,-dead_strip");
            }
            TargetLinkerFlavor::Msvc => {
                cmd.arg("-Wl,/OPT:REF");
            }
        }
    }
    if linker_flavor == TargetLinkerFlavor::Elf {
        cmd.arg("-Wl,--as-needed");
        if lld.is_some() {
            cmd.arg("-Wl,--icf=all");
        }
    }
    if let PgoMode::Generate(path) = &options.pgo {
        cmd.arg(format!("-fprofile-instr-generate={}", path.display()));
    }
    if options.strip {
        match linker_flavor {
            TargetLinkerFlavor::Msvc => {}
            _ => {
                cmd.arg("-s");
            }
        }
    }
    match linker_flavor {
        TargetLinkerFlavor::Elf => {
            cmd.arg("-lm").arg("-pthread").arg("-ldl");
        }
        TargetLinkerFlavor::Darwin => {
            cmd.arg("-lm").arg("-pthread");
        }
        TargetLinkerFlavor::Msvc => {}
    }
    cmd.arg("-o").arg(output);

    if verbose {
        eprintln!("[jrgc] {} {}", clang.display(), format_cmd_args(&cmd));
    }
    let out = cmd.output().map_err(|e| format!("gagal menjalankan clang ({}): {}", clang.display(), e));
    let _ = std::fs::remove_file(&jaga);
    let out = out?;
    if !out.status.success() {
        return Err(format!(
            "clang gagal mengompilasi hasil LLVM IR:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let warnings = String::from_utf8_lossy(&out.stderr);
    if verbose && !warnings.trim().is_empty() {
        eprintln!("[jrgc] clang: {}", warnings.trim());
    }
    Ok(output.to_path_buf())
}

/// Sumber penjaga stack, ditulis ke berkas sementara milik pemanggilan ini saja (tes
/// mengompilasi banyak program sekaligus, jadi namanya memuat pid dan nomor urut).
fn tulis_jaga_stack() -> Result<PathBuf, String> {
    static URUT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = URUT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("jrgc-jaga-{}-{}.c", std::process::id(), n));
    // Primitif memori (semua sasaran) lalu penjaga stack (cuma Unix; berkasnya menjaga dirinya).
    std::fs::write(&p, concat!(include_str!("primitif.c"), "\n", include_str!("korutin.c"), "\n", include_str!("jaga_stack.c")))
        .map_err(|e| format!("gagal menulis {}: {}", p.display(), e))?;
    Ok(p)
}

fn run_opt_pgo_generate(
    clang: &Path,
    input: &Path,
    output: &Path,
    verbose: bool,
) -> Result<(), String> {
    let opt = find_llvm_opt(clang)?;
    let mut cmd = Command::new(&opt);
    cmd.arg("-passes=pgo-instr-gen,instrprof")
        .arg(input)
        .arg("-S")
        .arg("-o")
        .arg(output);
    run_opt_command(&opt, &mut cmd, "menginstrumentasi IR untuk PGO", verbose)
}

fn run_opt_pgo_use(
    clang: &Path,
    input: &Path,
    output: &Path,
    profile: &Path,
    verbose: bool,
) -> Result<(), String> {
    let opt = find_llvm_opt(clang)?;
    let mut cmd = Command::new(&opt);
    cmd.arg("-passes=pgo-instr-use")
        .arg(format!("-pgo-test-profile-file={}", profile.display()))
        .arg(input)
        .arg("-S")
        .arg("-o")
        .arg(output);
    run_opt_command(&opt, &mut cmd, "menerapkan profil PGO ke IR", verbose)
}

fn run_opt_command(
    opt: &Path,
    cmd: &mut Command,
    action: &str,
    verbose: bool,
) -> Result<(), String> {
    if verbose {
        eprintln!("[jrgc] {} {}", opt.display(), format_cmd_args(cmd));
    }
    let out = cmd.output()
        .map_err(|e| format!("gagal menjalankan LLVM opt ({}): {}", opt.display(), e))?;
    if !out.status.success() {
        return Err(format!(
            "LLVM opt gagal {}:\n{}",
            action,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

fn resolve_pgo_profile(path: &Path, clang: &Path, verbose: bool) -> Result<PathBuf, String> {
    if path.is_dir() {
        let raw_profiles = collect_raw_profiles(path)?;
        return merge_raw_profiles(&raw_profiles, &path.join("merged.profdata"), clang, verbose);
    }
    if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("profraw")) {
        let output = path.with_extension("profdata");
        return merge_raw_profiles(&[path.to_path_buf()], &output, clang, verbose);
    }
    Ok(path.to_path_buf())
}

fn collect_raw_profiles(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut profiles = Vec::new();
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("gagal membaca direktori profil PGO {}: {}", dir.display(), e))?;
    for entry in entries {
        let path = entry
            .map_err(|e| format!("gagal membaca entry profil PGO di {}: {}", dir.display(), e))?
            .path();
        if path.is_file()
            && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("profraw"))
        {
            profiles.push(path);
        }
    }
    profiles.sort();
    if profiles.is_empty() {
        return Err(format!(
            "direktori profil PGO {} tidak berisi file `.profraw`",
            dir.display()
        ));
    }
    Ok(profiles)
}

fn merge_raw_profiles(
    raw_profiles: &[PathBuf],
    profdata: &Path,
    clang: &Path,
    verbose: bool,
) -> Result<PathBuf, String> {
    let llvm_profdata = find_llvm_profdata(clang)?;
    let mut cmd = Command::new(&llvm_profdata);
    cmd.arg("merge").arg("-output").arg(profdata);
    for raw in raw_profiles {
        cmd.arg(raw);
    }
    if verbose {
        eprintln!("[jrgc] {} {}", llvm_profdata.display(), format_cmd_args(&cmd));
    }
    let out = cmd.output()
        .map_err(|e| format!("gagal menjalankan llvm-profdata ({}): {}", llvm_profdata.display(), e))?;
    if !out.status.success() {
        return Err(format!(
            "llvm-profdata gagal menggabungkan {} profil mentah:\n{}",
            raw_profiles.len(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(profdata.to_path_buf())
}

fn inject_profile_filename(ir: &str, path: &Path) -> String {
    let filename = path.to_string_lossy();
    let escaped = mangle::llvm_escape(filename.as_bytes());
    let mut out = String::with_capacity(ir.len() + escaped.len() + 128);
    out.push_str("$__llvm_profile_filename = comdat any\n");
    out.push_str(&format!(
        "@__llvm_profile_filename = hidden constant [{} x i8] c\"{}\\00\", comdat\n\n",
        filename.len() + 1,
        escaped
    ));
    out.push_str(ir);
    out
}

fn with_extension_ll(output: &Path) -> PathBuf {
    let mut p = output.to_path_buf();
    let stem = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "out".to_string());
    p.set_file_name(format!("{}.ll", stem));
    p
}

fn with_extension_pgo_ll(output: &Path) -> PathBuf {
    let mut p = output.to_path_buf();
    let stem = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "out".to_string());
    p.set_file_name(format!("{}.pgo.ll", stem));
    p
}

fn format_cmd_args(cmd: &Command) -> String {
    cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::{lex, SourceFile};
    use crate::parser::parse;

    fn parse_source(source: &str) -> ParsedFile {
        let source = SourceFile {
            id: 0,
            name: "<llvm-test>".to_string(),
            src: source.to_string(),
        };
        let tokens = lex(&source).expect("source test harus valid");
        let (file, errors) = parse(tokens, false);
        assert!(errors.is_empty(), "parse error: {:?}", errors);
        file
    }

    #[test]
    fn native_cpu_rejects_cross_target() {
        let options = NativeCompileOptions {
            target: Some("aarch64-unknown-linux-gnu".to_string()),
            native_cpu: true,
            ..NativeCompileOptions::default()
        };
        assert!(options.validate().is_err());
    }

    #[test]
    fn invalid_optimization_level_is_rejected() {
        let options = NativeCompileOptions {
            opt_level: 4,
            ..NativeCompileOptions::default()
        };
        assert!(options.validate().is_err());
    }

    #[test]
    fn integer_main_return_is_lowered_to_i32_exit_code() {
        let file = parse_source(
            "package main\n\
             pub fn main() -> int { 7 }\n",
        );
        let ir = generate_ir(&file).unwrap();
        assert!(ir.contains("define dso_local i32 @\"main\"()"), "IR aktual:\n{}", ir);
        assert!(ir.contains("trunc i64 7 to i32"), "IR aktual:\n{}", ir);
        assert!(ir.contains("ret i32 %cvt."), "IR aktual:\n{}", ir);
        assert!(!ir.contains("ret i64 7"), "IR aktual:\n{}", ir);
    }

    #[test]
    fn non_integer_main_return_is_rejected() {
        let file = parse_source(
            "package main\n\
             pub fn main() -> string { \"invalid\" }\n",
        );
        let errors = generate_ir(&file).unwrap_err();
        assert!(errors.iter().any(|error| error.contains("integer exit code")));
    }

    #[test]
    fn production_defaults_are_optimized_and_dead_strip_sections() {
        let options = NativeCompileOptions::default();
        assert_eq!(options.opt_level, 2);
        assert!(options.gc_sections);
        assert!(!options.native_cpu);
        assert!(!options.aggressive);
        assert!(!options.fast_math);
        assert!(!options.strip);
        assert_eq!(options.lto, LtoMode::Off);
        assert_eq!(options.linker, LinkerMode::System);
        assert_eq!(options.pgo, PgoMode::Off);
    }

    #[test]
    fn pgo_filename_is_embedded_in_ir() {
        let ir = inject_profile_filename("define i32 @main() { ret i32 0 }\n", Path::new("/tmp/run.profraw"));
        assert!(ir.contains("$__llvm_profile_filename = comdat any"));
        assert!(ir.contains("[17 x i8] c\"/tmp/run.profraw\\00\""));
    }

    #[test]
    fn pgo_directory_collects_profiles_deterministically() {
        let unique = format!(
            "jrg-pgo-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let dir = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.profraw"), b"b").unwrap();
        std::fs::write(dir.join("a.profraw"), b"a").unwrap();
        std::fs::write(dir.join("ignore.txt"), b"x").unwrap();

        let profiles = collect_raw_profiles(&dir).unwrap();
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].file_name().unwrap(), "a.profraw");
        assert_eq!(profiles[1].file_name().unwrap(), "b.profraw");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn generated_functions_expose_optimizer_friendly_attributes() {
        let file = parse_source(
            "package main\n\
             fn helper(x: float) -> float = sqrt(x)\n\
             pub fn main() { wok(helper(9.0)) }\n",
        );
        let ir = generate_ir_with_options(
            &file,
            CodegenOptions {
                fast_math: true,
                full_buffered_stdout: true,
            },
        )
        .unwrap();

        assert!(ir.contains("define dso_local i32 @\"main\"() local_unnamed_addr nounwind"));
        assert!(
            ir.contains("define internal double @\"helper\"(double %p0) local_unnamed_addr nounwind"),
            "IR aktual:\n{}",
            ir
        );
        assert!(ir.contains("declare double @llvm.sqrt.f64(double)"));
        assert!(ir.contains("call fast double @llvm.sqrt.f64"));
        assert!(ir.contains("declare dso_local void @jrg_set_stdout_buffering(i32) nounwind"));
        assert!(ir.contains("call void @jrg_set_stdout_buffering(i32 1)"));
        assert!(ir.contains("declare dso_local void @jrg_print_f64(double, i32) nounwind"));
    }

    /// `pub` pada field di dua tes berikut bukan gaya penulisan: keduanya dibaca
    /// dari `main`, dan field tanpa `pub` bersifat private.
    #[test]
    fn nested_option_layout_is_emitted() {
        let file = parse_source(
            "package main\n\
             struct Envelope { pub value: Option<int> }\n\
             pub fn main() { let e: Envelope = Envelope { value: Some(7) }; wok(e.value.unwrap()) }\n",
        );
        let ir = generate_ir(&file).unwrap();
        assert!(ir.contains("%\"Option$i64\" = type { i32, [1 x i64] }"), "IR aktual:\n{}", ir);
        assert!(ir.contains("%\"Envelope\" = type { %\"Option$i64\" }"), "IR aktual:\n{}", ir);
    }

    #[test]
    fn forward_declared_payload_layout_is_recomputed() {
        let file = parse_source(
            "package main\n\
             struct A { pub value: Option<B> }\n\
             struct B { pub number: int }\n\
             pub fn main() {\n\
                 let b: B = B { number: 9 }\n\
                 let a: A = A { value: Some(b) }\n\
                 wok(a.value.unwrap().number)\n\
             }\n",
        );
        let ir = generate_ir(&file).unwrap();
        assert!(ir.contains("%\"Option$B\" = type { i32, [1 x i64] }"), "IR aktual:\n{}", ir);
    }

    #[test]
    fn slice_bounds_check_covers_start_end_and_order() {
        let file = parse_source(
            "package main\n\
             pub fn main() { let xs: [3]int = [1, 2, 3]; let ys = xs[1:3]; wok(ys) }\n",
        );
        let ir = generate_ir(&file).unwrap();
        assert!(ir.contains("@jrg_panic_slice_oob(i64"), "IR aktual:\n{}", ir);
        assert!(ir.contains("icmp sgt i64"), "IR aktual:\n{}", ir);
    }

    #[test]
    fn linker_flags_follow_target_family() {
        assert_eq!(
            target_linker_flavor(Some("x86_64-unknown-linux-gnu")),
            TargetLinkerFlavor::Elf
        );
        assert_eq!(
            target_linker_flavor(Some("aarch64-apple-darwin")),
            TargetLinkerFlavor::Darwin
        );
        assert_eq!(
            target_linker_flavor(Some("x86_64-pc-windows-msvc")),
            TargetLinkerFlavor::Msvc
        );
    }

    // ── expression-valued control flow ──────────────────────────────────

    /// The original reproducer. Before the fix this compiled and returned
    /// 0; the branch values must now actually reach the caller.
    #[test]
    fn if_expression_in_match_arm_yields_branch_value() {
        let file = parse_source(
            "package main\n\
             enum E { A, B }\n\
             fn f(e: E, v: int) -> int {\n\
                 match e {\n\
                     E::A => { if v > 5 { 100 } else { 200 } }\n\
                     E::B => 7,\n\
                 }\n\
             }\n\
             pub fn main() { wok(f(E::A, 10)) }\n",
        );
        let ir = generate_ir(&file).expect("harus kompilasi");
        // The constants must be materialized into the result slot rather
        // than the whole arm collapsing to a zeroinitializer.
        assert!(ir.contains("store i64 100"), "IR aktual:\n{}", ir);
        assert!(ir.contains("store i64 200"), "IR aktual:\n{}", ir);
        assert!(ir.contains("ifx.slot."), "IR aktual:\n{}", ir);
    }

    /// The same hole reached through a plain block, with no `match`
    /// involved — this is the shape that shows the bug was never
    /// match-specific.
    #[test]
    fn if_expression_as_block_tail_yields_branch_value() {
        let file = parse_source(
            "package main\n\
             pub fn main() { let x: int = { if 10 > 5 { 100 } else { 200 } }; wok(x) }\n",
        );
        let ir = generate_ir(&file).expect("harus kompilasi");
        assert!(ir.contains("store i64 100"), "IR aktual:\n{}", ir);
        assert!(ir.contains("store i64 200"), "IR aktual:\n{}", ir);
    }

    /// `else if` chains must thread every branch into the one result slot.
    #[test]
    fn if_else_if_chain_is_expression_valued() {
        let file = parse_source(
            "package main\n\
             fn c(v: int) -> int { if v < 10 { 1 } else if v < 20 { 2 } else { 3 } }\n\
             pub fn main() { wok(c(5)) }\n",
        );
        let ir = generate_ir(&file).expect("harus kompilasi");
        for konstanta in ["store i64 1", "store i64 2", "store i64 3"] {
            assert!(ir.contains(konstanta), "kurang `{}` di IR:\n{}", konstanta, ir);
        }
    }

    /// An unannotated `let` takes the if-expression's type from its
    /// branches — here `str`, which must not be silently turned into an
    /// integer slot.
    #[test]
    fn if_expression_infers_non_integer_type_without_annotation() {
        let file = parse_source(
            "package main\n\
             pub fn main() { let s = if true { \"yes\" } else { \"no\" }; wok(s) }\n",
        );
        let ir = generate_ir(&file).expect("harus kompilasi");
        assert!(ir.contains("store %JrgStr"), "IR aktual:\n{}", ir);
    }

    /// THE class-level guard: a valueless expression must never be
    /// silently materialized as a typed value. This is what stops any
    /// *future* construct from reintroducing the same bug.
    #[test]
    fn unit_value_in_typed_slot_is_a_hard_error_not_zero() {
        // A block whose last statement is a `while` produces no value.
        let file = parse_source(
            "package main\n\
             pub fn main() { let x: int = { var i: int = 0; while i < 3 { i = i + 1 } }; wok(x) }\n",
        );
        let errors = generate_ir(&file).expect_err("harus ditolak, bukan diam-diam jadi 0");
        assert!(
            errors.iter().any(|e| e.contains("tidak menghasilkan nilai")),
            "pesan error aktual: {:?}",
            errors
        );
    }

    /// An `if` with no `else` has a path that produces nothing, so it is
    /// not value-producing — and must say so instead of yielding 0.
    #[test]
    fn if_without_else_cannot_be_used_as_a_value() {
        let file = parse_source(
            "package main\n\
             pub fn main() { let x: int = if true { 100 }; wok(x) }\n",
        );
        let errors = generate_ir(&file).expect_err("harus ditolak");
        assert!(
            errors.iter().any(|e| e.contains("tidak menghasilkan nilai")),
            "pesan error aktual: {:?}",
            errors
        );
    }

    /// Statement-position `if` must keep its original value-less lowering:
    /// no result slot, and branches that need not agree on a type.
    #[test]
    fn statement_position_if_is_unchanged() {
        let file = parse_source(
            "package main\n\
             pub fn main() {\n\
                 var x: int = 0\n\
                 if true { x = 5 } else { x = 6 }\n\
                 if x > 0 { }\n\
                 wok(x)\n\
             }\n",
        );
        let ir = generate_ir(&file).expect("harus kompilasi");
        assert!(!ir.contains("ifx.slot."), "statement `if` tidak boleh bikin result slot:\n{}", ir);
    }

    /// A branch that diverges contributes no value but must not be
    /// mistaken for a valueless branch — the other branch still decides
    /// the type, and the merge block stays well-formed.
    #[test]
    fn diverging_branch_does_not_poison_if_expression() {
        let file = parse_source(
            "package main\n\
             fn f(v: int) -> int { let x: int = if v > 0 { 10 } else { return -1 }; x + 1 }\n\
             pub fn main() { wok(f(5)) }\n",
        );
        let ir = generate_ir(&file).expect("harus kompilasi");
        assert!(ir.contains("store i64 10"), "IR aktual:\n{}", ir);
    }

    /// A trailing bare block is a value too. Before the fix it stayed a
    /// `Stmt::Block`, could never become a block tail, and the enclosing
    /// block silently evaluated to unit — the same class of hole.
    #[test]
    fn trailing_bare_block_is_the_block_value() {
        let file = parse_source(
            "package main\n\
             pub fn main() { let x: int = { { 77 } }; wok(x) }\n",
        );
        let ir = generate_ir(&file).expect("harus kompilasi");
        assert!(ir.contains("77"), "IR aktual:\n{}", ir);
    }
}
