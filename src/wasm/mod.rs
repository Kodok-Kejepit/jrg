//! Mesin kelima: program JRG → WebAssembly (`--emit-wasm`, `wasm32-wasip1`) lewat IR backend LLVM yang sama
//! (`CodegenOptions::ptr32`); runtime dan `primitif.c` dikompilasi ulang untuk WASI, korutin lewat `korutin_wasm.c`.
//! Alat WASI dicari di `JRG_WASI_DIR` atau `~/.cache/jrgm/wasi`.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Triple Rust dan clang untuk runtime dan objek program.
pub const SASARAN: &str = "wasm32-wasip1";

/// Alat yang dibutuhkan penyusun wasm.
pub struct Alat {
    clang: PathBuf,
    /// `wasm-ld`, atau `rust-lld` yang dipanggil `-flavor wasm`.
    penaut: (PathBuf, bool),
    sysroot: PathBuf,
    builtins: PathBuf,
    /// `wasm-opt` binaryen, untuk pass Asyncify (pindah stack korutin).
    wasm_opt: PathBuf,
}

fn dir_wasi() -> PathBuf {
    if let Some(d) = std::env::var_os("JRG_WASI_DIR").filter(|d| !d.is_empty()) { return PathBuf::from(d); }
    let rumah = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    rumah.join(".cache").join("jrgm").join("wasi")
}

/// Subfolder pertama `dir` yang namanya berawalan `awal` (urut nama, yang terbaru di belakang).
fn sub_terbaru(dir: &Path, awal: &str) -> Option<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(awal)) && p.is_dir()).collect();
    v.sort();
    v.pop()
}

/// `rust-lld` milik toolchain Rust (penaut wasm yang ikut rustup), kalau `wasm-ld` tidak ada.
fn rust_lld() -> Option<PathBuf> {
    let out = Command::new("rustc").args(["--print", "sysroot"]).output().ok()?;
    let akar = PathBuf::from(String::from_utf8(out.stdout).ok()?.trim());
    let rustlib = akar.join("lib").join("rustlib");
    std::fs::read_dir(&rustlib).ok()?.flatten()
        .map(|e| e.path().join("bin").join("rust-lld")).find(|p| p.exists())
}

fn di_path(nama: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|ps| std::env::split_paths(&ps).map(|d| d.join(nama)).find(|p| p.is_file()))
}

/// Mencari semua alat; galatnya menyebut apa yang kurang dan cara mendapatkannya.
pub fn cari_alat() -> Result<Alat, String> {
    let clang = crate::llvm::find_clang()?;
    let wasi = dir_wasi();
    let sysroot = sub_terbaru(&wasi, "wasi-sysroot-").filter(|d| d.join("lib").join(SASARAN).join("libc.a").exists())
        .ok_or_else(|| format!(
            "--emit-wasm butuh wasi-sysroot (wasi-sdk) di {}: unduh `wasi-sysroot-<versi>.tar.gz` dan \
             `libclang_rt-<versi>.tar.gz` dari rilis github.com/WebAssembly/wasi-sdk lalu ekstrak di sana \
             (atau set `JRG_WASI_DIR`)", wasi.display()))?;
    let builtins = sub_terbaru(&wasi.join("rt"), "libclang_rt-")
        .map(|d| d.join(format!("wasm32-unknown-{}", SASARAN.trim_start_matches("wasm32-"))).join("libclang_rt.builtins.a"))
        .filter(|p| p.exists())
        .ok_or_else(|| format!("--emit-wasm butuh compiler-rt wasm (`libclang_rt-<versi>.tar.gz` wasi-sdk) di {}",
            wasi.join("rt").display()))?;
    let wasm_opt = di_path("wasm-opt")
        .or_else(|| sub_terbaru(&wasi, "binaryen-").map(|d| d.join("bin").join("wasm-opt")).filter(|p| p.exists()))
        .ok_or_else(|| format!("--emit-wasm butuh `wasm-opt` (binaryen, untuk korutin lewat Asyncify): di PATH, atau \
            ekstrak `binaryen-version_<v>-x86_64-linux.tar.gz` dari rilis github.com/WebAssembly/binaryen di {}",
            wasi.display()))?;
    let penaut = di_path("wasm-ld").map(|p| (p, false))
        .or_else(|| rust_lld().map(|p| (p, true)))
        .ok_or("--emit-wasm butuh penaut wasm: `wasm-ld` (lld) di PATH, atau `rust-lld` dari rustup")?;
    Ok(Alat { clang, penaut, sysroot, builtins, wasm_opt })
}

fn jalankan_alat(cmd: &mut Command, apa: &str) -> Result<(), String> {
    let out = cmd.output().map_err(|e| format!("gagal menjalankan {}: {}", apa, crate::host::galat_io(&e)))?;
    if out.status.success() { return Ok(()); }
    Err(format!("{} gagal:\n{}", apa, String::from_utf8_lossy(&out.stderr)))
}

/// Nama fungsi yang DIDEFINISIKAN modul wasm (bagian `name`, subbagian 1; fungsi impor dilewati).
fn nama_fungsi(w: &[u8]) -> Vec<String> {
    fn leb(w: &[u8], i: &mut usize) -> Option<u64> {
        let (mut v, mut g) = (0u64, 0);
        loop {
            let b = *w.get(*i)?;
            *i += 1;
            v |= ((b & 0x7f) as u64) << g;
            if b & 0x80 == 0 { return Some(v); }
            g += 7;
            if g >= 64 { return None; }
        }
    }
    // Jumlah fungsi impor (bagian 2): indeks fungsi di bawahnya milik impor.
    fn impor(w: &[u8], mut j: usize) -> Option<u64> {
        let (mut f, cacah) = (0, leb(w, &mut j)?);
        for _ in 0..cacah {
            for _ in 0..2 { let l = leb(w, &mut j)? as usize; j += l; }
            let jenis = *w.get(j)?;
            j += 1;
            let batas = |w: &[u8], j: &mut usize| -> Option<()> {
                let b = *w.get(*j)?;
                *j += 1;
                leb(w, j)?;
                if b & 1 == 1 { leb(w, j)?; }
                Some(())
            };
            match jenis {
                0 => { leb(w, &mut j)?; f += 1; }
                1 => { j += 1; batas(w, &mut j)?; }
                2 => batas(w, &mut j)?,
                3 => j += 2,
                _ => { j += 1; leb(w, &mut j)?; }
            }
        }
        Some(f)
    }
    let mut hasil = Vec::new();
    let mut n_impor = 0;
    let mut i = 8;
    while i < w.len() {
        let Some(&id) = w.get(i) else { break };
        i += 1;
        let Some(n) = leb(w, &mut i) else { break };
        let ujung = i + n as usize;
        if id == 2 { n_impor = impor(w, i).unwrap_or(u64::MAX); }
        if id == 0 {
            let mut j = i;
            let nl = leb(w, &mut j).unwrap_or(0) as usize;
            if w.get(j..j + nl) == Some(b"name") {
                j += nl;
                while j < ujung {
                    let Some(&sub) = w.get(j) else { break };
                    j += 1;
                    let Some(sn) = leb(w, &mut j) else { break };
                    let sujung = j + sn as usize;
                    if sub == 1 {
                        let cacah = leb(w, &mut j).unwrap_or(0);
                        for _ in 0..cacah {
                            let Some(idx) = leb(w, &mut j) else { break };
                            let Some(pj) = leb(w, &mut j) else { break };
                            if idx >= n_impor {
                                if let Some(s) = w.get(j..j + pj as usize) { hasil.push(String::from_utf8_lossy(s).into_owned()); }
                            }
                            j += pj as usize;
                        }
                    }
                    j = sujung;
                }
            }
        }
        i = ujung;
    }
    hasil
}

/// Fungsi milik crate yang tidak pernah berganti korutin: `jrgc` (jembatan; panggilan balik ke program yang berganti
/// korutin dijaga `jrg_wasm_di_jembatan`) dan pustaka Rust. Runtime (`jrgwasmrt`) tetap diinstrumentasi.
fn tanpa_korutin(n: &str) -> bool {
    const CRATE: &[&str] = &["jrgc", "core", "alloc", "std", "hashbrown", "indexmap", "smallvec", "unicode_xid",
        "bitflags", "clap", "rustc_demangle", "miniz_oxide", "adler2", "memchr", "addr2line", "gimli", "object"];
    // Mangling v0 (`_R…`): crate pertama sesudah `Cs<penanda>_`; selain itu nama ter-demangle `crate::…`.
    let krat = if let Some(p) = n.strip_prefix("_R") {
        let Some(k) = p.find("Cs") else { return false };
        let s = p[k + 2..].split_once('_').map_or("", |x| x.1);
        let d: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(len) = d.parse::<usize>() else { return false };
        s.get(d.len()..d.len() + len).unwrap_or("").to_string()
    } else {
        n.trim_start_matches('<').split("::").next().unwrap_or("").to_string()
    };
    CRATE.contains(&krat.as_str())
}

/// Pustaka gabungan runtime + jembatan (`wasm-rt/`, `libjrgwasmrt.a`), dibangun kalau belum ada atau lebih tua dari
/// sumber Rust yang dipakainya (runtime, jembatan, `jrgc`).
fn ensure_wasm_rt(verbose: bool) -> Result<PathBuf, String> {
    let akar = match std::env::var("JRGC_WASM_RT_DIR") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => Path::new(env!("CARGO_MANIFEST_DIR")).join("wasm-rt"),
    };
    if !akar.join("Cargo.toml").exists() {
        return Err(format!("pustaka wasm runtime + jembatan tidak ditemukan di {}\n  bantuan: set `JRGC_WASM_RT_DIR` ke \
            folder `wasm-rt/` sumber JRG", akar.display()));
    }
    let lib = akar.join("target").join(SASARAN).join("release").join("libjrgwasmrt.a");
    fn terbaru(d: &Path, kini: &mut std::time::SystemTime) {
        let Ok(isi) = std::fs::read_dir(d) else { return };
        for e in isi.flatten() {
            let p = e.path();
            if p.is_dir() { terbaru(&p, kini); continue; }
            if p.extension().is_some_and(|x| x == "rs") {
                if let Ok(t) = e.metadata().and_then(|m| m.modified()) { if t > *kini { *kini = t; } }
            }
        }
    }
    let induk = akar.parent().unwrap_or(&akar).to_path_buf();
    let perlu = match std::fs::metadata(&lib).and_then(|m| m.modified()) {
        Err(_) => true,
        Ok(t) => {
            let mut kini = std::time::SystemTime::UNIX_EPOCH;
            for d in [akar.join("src"), induk.join("runtime").join("src"), induk.join("jembatan").join("src"), induk.join("src")] {
                terbaru(&d, &mut kini);
            }
            kini > t
        }
    };
    if perlu {
        if verbose { eprintln!("[jrgc] membangun pustaka wasm runtime + jembatan — beberapa menit, di-cache setelahnya..."); }
        let mut cmd = Command::new("cargo");
        cmd.args(["build", "--release", "-j", "1", "--target", SASARAN]).current_dir(&akar);
        let out = crate::putar::jalankan(&mut cmd, "membangun pustaka wasm runtime + jembatan (sekali, di-cache)")
            .map_err(|e| format!("gagal menjalankan `cargo build` untuk pustaka wasm: {}", e))?;
        if !out.status.success() {
            return Err(format!("gagal membangun pustaka wasm runtime + jembatan:\n{}", String::from_utf8_lossy(&out.stderr)));
        }
        crate::llvm::segarkan_cap(&lib);
    }
    if !lib.exists() { return Err(format!("build pustaka wasm selesai tapi {} tidak ditemukan", lib.display())); }
    Ok(lib)
}

/// `main` JRG di IR berbentuk `i32 main()`. wasi-libc memanggil `__main_void`, yang di sini milik `korutin_wasm.c`:
/// dasar Asyncify yang menjalankan `main` program sebagai korutin.
fn ganti_main(ir: &str) -> String {
    ir.replacen("@\"main\"()", "@\"jrg_main_program\"()", 1)
}

/// Menyusun IR (`CodegenOptions::ptr32`) jadi `keluar` (.wasm) dan pemuatnya `keluar.mjs`.
pub fn kompilasi(ir: &str, keluar: &Path, verbose: bool) -> Result<PathBuf, String> {
    let alat = cari_alat()?;
    // Program yang memanggil builtin lewat jembatan memakai pustaka gabungan runtime + jembatan (`wasm-rt/`).
    let jembatan = ir.contains("@jrg_jembatan(") || ir.contains("@jrg_jembatan_lepas(");
    let runtime = if jembatan { ensure_wasm_rt(verbose) } else { crate::llvm::ensure_runtime(verbose, Some(SASARAN)) }
        .map_err(|e| format!("{e}\n  bantuan: runtime wasm butuh target Rust `{SASARAN}` (`rustup target add {SASARAN}`)"))?;
    let dir = crate::host::dir_sementara("jrgc-wasm")
        .map_err(|e| format!("gagal membuat folder sementara: {}", crate::host::galat_io(&e)))?;
    let (ll, prog_o) = (dir.join("program.ll"), dir.join("program.o"));
    std::fs::write(&ll, ganti_main(ir)).map_err(|e| format!("gagal menulis {}: {}", ll.display(), crate::host::galat_io(&e)))?;
    let sysroot = format!("--sysroot={}", alat.sysroot.display());
    let susun = |sumber: &Path, objek: &Path| -> Result<(), String> {
        jalankan_alat(Command::new(&alat.clang).arg(format!("--target={SASARAN}")).arg(&sysroot).arg("-O2")
            .args(jembatan.then_some("-DJRG_WASM_JEMBATAN"))
            .arg("-Wno-override-module").arg("-c").arg(sumber).arg("-o").arg(objek), "clang (wasm)")
    };
    susun(&ll, &prog_o)?;
    // Primitif runtime yang sama dengan `--emit-llvm` (satu definisi); korutin diganti versi WASM.
    let c = dir.join("pengantar.c");
    std::fs::write(&c, concat!(include_str!("../llvm/primitif.c"), "\n", include_str!("../llvm/jaga_stack.c"), "\n",
        include_str!("../llvm/korutin.c"), "\n", include_str!("../llvm/korutin_wasm.c")))
        .map_err(|e| format!("gagal menulis {}: {}", c.display(), crate::host::galat_io(&e)))?;
    let c_o = dir.join("pengantar.o");
    susun(&c, &c_o)?;
    let taut_wasm = dir.join("tertaut.wasm");
    let lib = alat.sysroot.join("lib").join(SASARAN);
    let mut taut = Command::new(&alat.penaut.0);
    if alat.penaut.1 { taut.args(["-flavor", "wasm"]); }
    // Stack bayangan 8 MiB seperti stack utama native (bawaan wasm-ld 64 KiB: larik `[2000]int` lokal sudah meluap).
    taut.args(["-m", "wasm32", "--gc-sections", "-z", "stack-size=8388608"]).arg(format!("-L{}", lib.display())).arg(lib.join("crt1-command.o"))
        .arg(&prog_o).arg(&c_o).arg(&runtime).arg("-lc").arg(&alat.builtins).arg("-o").arg(&taut_wasm);
    let hasil = jalankan_alat(&mut taut, "penaut wasm").and_then(|_| {
        // Asyncify: korutin dibuka dan digulung ulang sampai `__main_void`, yang karena itu tidak diinstrumentasi.
        // `JRG_WASM_NAMA=1`: nama fungsi ikut di .wasm (jejak trap node terbaca) dan IR-nya disimpan, untuk menyelidiki.
        let nama = std::env::var_os("JRG_WASM_NAMA").is_some_and(|v| v == "1");
        // Dengan jembatan: fungsi crate yang tidak pernah berganti korutin (`jrgc`, pustaka Rust) tidak diinstrumentasi.
        let mut buang = vec!["__main_void".to_string()];
        if jembatan {
            let isi = std::fs::read(&taut_wasm).map_err(|e| format!("gagal membaca {}: {}", taut_wasm.display(), crate::host::galat_io(&e)))?;
            buang.extend(nama_fungsi(&isi).into_iter().filter(|n| tanpa_korutin(n)));
        }
        let daftar = dir.join("tanpa-asyncify.txt");
        std::fs::write(&daftar, buang.join("\n")).map_err(|e| format!("gagal menulis {}: {}", daftar.display(), crate::host::galat_io(&e)))?;
        jalankan_alat(Command::new(&alat.wasm_opt).arg(&taut_wasm).arg("--asyncify")
            .arg(format!("--pass-arg=asyncify-removelist@@{}", daftar.display())).arg("-O2")
            .args(nama.then_some("-g")).arg("-o").arg(keluar), "wasm-opt (Asyncify)")
    });
    if std::env::var_os("JRG_WASM_NAMA").is_some_and(|v| v == "1") { let _ = std::fs::copy(&ll, keluar.with_extension("ll")); }
    let _ = std::fs::remove_dir_all(&dir);
    hasil?;
    let pemuat = pemuat_untuk(keluar);
    std::fs::write(&pemuat, PEMUAT_NODE).map_err(|e| format!("gagal menulis {}: {}", pemuat.display(), crate::host::galat_io(&e)))?;
    if verbose { eprintln!("[jrgc] wasm: {} (pemuat {})", keluar.display(), pemuat.display()); }
    Ok(keluar.to_path_buf())
}

/// `program.wasm` → `program.wasm.mjs`.
pub fn pemuat_untuk(wasm: &Path) -> PathBuf {
    let mut n = wasm.as_os_str().to_os_string();
    n.push(".mjs");
    PathBuf::from(n)
}

/// Pemuat node: WASI bawaan node, argumen sesudah nama berkas wasm, kode keluar dari `proc_exit`. Stack V8 bawaan
/// (±1 MiB) menolak rekursi yang jalan di native, jadi `jalankan` memberi `--stack-size` (jauh di bawah stack OS).
const PEMUAT_NODE: &str = r#"// Dibuat jrgc --emit-wasm. Pakai: node --stack-size=3000 <program>.wasm.mjs [argumen...]
import fs from 'node:fs';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import { WASI } from 'node:wasi';
const wasm = fileURLToPath(import.meta.url).replace(/\.mjs$/, '');
const wasi = new WASI({ version: 'preview1', args: [wasm, ...process.argv.slice(2)], env: process.env,
  preopens: { '/': '/' }, returnOnExit: true });
const mod = await WebAssembly.compile(fs.readFileSync(wasm));
const inst = await WebAssembly.instantiate(mod, { wasi_snapshot_preview1: wasi.wasiImport });
process.exitCode = wasi.start(inst);
"#;

/// Menjalankan `wasm` lewat node; kode keluarnya kode keluar program.
pub fn jalankan(wasm: &Path, args: &[String]) -> Result<i32, String> {
    let node = di_path("node").ok_or("--emit-wasm menjalankan programnya lewat `node` (WASI bawaan node 20+), \
        dan `node` tidak ada di PATH; pakai `--no-run` untuk menyusun saja")?;
    let status = Command::new(node).args(["--no-warnings", "--stack-size=3000"]).arg(pemuat_untuk(wasm)).args(args).status()
        .map_err(|e| format!("gagal menjalankan node: {}", crate::host::galat_io(&e)))?;
    if let Some(k) = status.code() { return Ok(k); }
    // Mati oleh sinyal (stack native habis: SIGSEGV): disebut, dengan 128+N seperti shell; dulu jadi kode 1 tanpa pesan.
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        let n = status.signal().unwrap_or(0);
        eprintln!("jrgc: program wasm berhenti oleh sinyal {n}{}", if n == 11 { " (SIGSEGV, kemungkinan stack habis)" } else { "" });
        Ok(128 + n)
    }
    #[cfg(not(unix))]
    Err("program wasm berhenti tanpa kode keluar".into())
}
