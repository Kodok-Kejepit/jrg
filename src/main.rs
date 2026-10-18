// src/main.rs
// Entry point: jrgc <source.jrg> [flags]

extern crate alloc;
mod antrean;
mod sinkron;
mod ruang_tipe;
mod hash_cepat;
mod json_inti;
mod lexer;
mod diag;
mod host;
mod pipa;
mod parser;
mod interpreter;
#[allow(dead_code, unused_imports)]
mod ffi;
mod canvas;
mod profiler;
mod module;
mod editor_src;
mod vm;
mod hotreload;
mod uji;
// `bantu` dipakai `src/lsp/` untuk MENURUNKAN daftar pelengkapan dan
// hover dari registry yang sedang berjalan — jadi ia harus ada di pohon
// modul biner ini juga, bukan cuma di lib.
mod argumen;
mod lebar;
mod term;
mod fmt;
mod putar;
#[path = "jrgm/main.rs"]
mod jrgm;
mod baris;
mod bantu;
mod lsp;
mod stdlib;
mod wokwok;
mod suara;
mod llvm;
mod dok;

use std::path::PathBuf;
use std::process;
use clap::Parser as ClapParser;

use crate::lexer::SourceFile;
use crate::parser::parse;
use crate::interpreter::interpret_with_paths;

// ─── CLI ───────────────────────────────────────────────────────────────────

/// JawaRustGo++ Compiler & Interpreter (jrgc)
#[derive(ClapParser, Debug)]
#[command(
    name    = "jrgc",
    version,
    // Pendek dengan sengaja. Bantuan yang panjang berhenti dibaca; penjelasan
    // tiap flag ada di situs dokumentasi, dan `-h` cuma perlu membuat orang
    // bisa mulai.
    about   = "JawaRustGo++ — jalankan, uji, dan kompilasi program .jrg",
    after_help = "\
Contoh:
  jrgc halo.jrg                 jalankan
  jrgc halo.jrg --vm            lewat VM (lebih cepat)
  jrgc halo.jrg --emit-llvm     kompilasi jadi biner native
  jrgc halo.jrg -- a b          kirim argumen `a b` ke programnya
  jrgc --sunting halo.jrg       buka penyunting kode
  jrgc                          REPL

  jrgc --help                   semua flag, termasuk yang jarang dipakai
  jrgc --cari <kata>            cari fungsi bawaan
  jrgc --doc <nama>             cara pakai satu fungsi bawaan

Penjelasan panjang tiap flag: https://jawarustgoplusplus.web.id/web_dokumentasi/jrgc.html",
    after_long_help = "\
Contoh:
  jrgc halo.jrg                 jalankan
  jrgc halo.jrg --vm            lewat VM (lebih cepat)
  jrgc halo.jrg --emit-llvm     kompilasi jadi biner native
  jrgc halo.jrg -- a b          kirim argumen `a b` ke programnya
  jrgc --sunting halo.jrg       buka penyunting kode
  jrgc                          REPL

Penjelasan panjang tiap flag: https://jawarustgoplusplus.web.id/web_dokumentasi/jrgc.html
Proyek banyak berkas:          jrgm --help"
)]
struct Cli {
    /// Berkas .jrg yang dijalankan (kosong = REPL)
    sources: Vec<PathBuf>,

    /// Argumen untuk PROGRAMNYA, bukan untuk jrgc
    #[arg(last = true)]
    argumen_program: Vec<String>,

    /// Jalankan lewat bytecode VM
    #[arg(long = "vm")]
    use_vm: bool,

    /// Kompilasi jadi biner native lewat clang
    #[arg(long = "emit-llvm")]
    emit_llvm: bool,

    /// Bungkus jadi aplikasi Android (lewat backend LLVM)
    #[arg(long = "apk", value_name = "BERKAS")]
    apk: Option<PathBuf>,

    /// ABI APK, dipisah koma: arm64-v8a (HP), x86_64 (emulator)
    #[arg(long = "apk-abi", default_value = "arm64-v8a", requires = "apk",
          hide_short_help = true, help_heading = "Kompilasi native (--emit-llvm)")]
    apk_abi: String,

    /// Buka penyunting kode JRG
    #[arg(long = "sunting", visible_alias = "edit", value_name = "BERKAS")]
    sunting: Option<String>,

    /// Mulai REPL
    #[arg(long = "repl")]
    repl: bool,

    /// Jalankan tiap fn test_*/uji_*, bukan main
    #[arg(long = "uji")]
    uji: bool,

    /// Hanya tes yang namanya memuat teks ini
    #[arg(long = "uji-filter", requires = "uji", value_name = "TEKS")]
    uji_filter: Option<String>,

    /// Direktori tambahan untuk `import`
    #[arg(short = 'I', long = "include", value_name = "DIR")]
    include_dirs: Vec<PathBuf>,

    /// Periksa berkas dan semua import-nya tanpa menjalankan apa pun
    #[arg(long = "periksa", hide_short_help = true)]
    periksa: bool,

    /// Tulis berkas yang sudah dirapikan ke stdout (formatter `jrgm fmt`); berkasnya tidak diubah
    #[arg(long = "fmt", hide_short_help = true, help_heading = "Alat")]
    fmt: bool,

    /// Jalankan ulang tiap berkas berubah
    #[arg(long = "watch", short = 'w')]
    watch: bool,

    // ── Penemuan ────────────────────────────────────────────────────────
    /// Cari fungsi bawaan yang namanya memuat KATA
    #[arg(long = "cari", value_name = "KATA", help_heading = "Penemuan")]
    cari: Option<String>,

    /// Cara pakai satu fungsi bawaan
    #[arg(long = "doc", value_name = "NAMA", help_heading = "Penemuan")]
    doc: Option<String>,

    /// Cetak semua nama bawaan, satu per baris
    #[arg(long = "daftar-builtin", help_heading = "Penemuan")]
    daftar_builtin: bool,

    // ── Diagnostik ──────────────────────────────────────────────────────
    /// Nada pesan galat: sopan|galak|judes|hangat|cool|lawak
    #[arg(long = "nada", value_name = "NAMA", help_heading = "Diagnostik")]
    nada: Option<String>,

    /// Tanpa sapaan di atas pesan galat (sama dengan --nada=sopan)
    #[arg(long = "sopan", conflicts_with_all = ["galak", "nada"],
          help_heading = "Diagnostik")]
    sopan: bool,

    /// Sama dengan --nada=galak
    #[arg(long = "galak", conflicts_with_all = ["sopan", "nada"],
          help_heading = "Diagnostik")]
    galak: bool,

    /// Tulis profil per fungsi ke BERKAS sebagai JSON (nama utuh, angka dalam ns), bukan tabel
    #[arg(long = "profile-json", value_name = "BERKAS", help_heading = "Diagnostik")]
    profile_json: Option<PathBuf>,

    /// Laporkan waktu per fungsi sesudah program selesai
    #[arg(long = "profile", help_heading = "Diagnostik")]
    profile: bool,

    /// Cetak tiap tahap yang dikerjakan
    #[arg(short = 'v', long = "verbose", help_heading = "Diagnostik")]
    verbose: bool,

    // ── Kompilasi native ────────────────────────────────────────────────
    // Disembunyikan dari `-h`: 15 tombol yang cuma berarti bersama `--emit-llvm`.
    /// Nama berkas hasil --emit-llvm
    #[arg(short = 'o', long = "output", default_value = "a.out",
          hide_short_help = true, help_heading = "Kompilasi native (--emit-llvm)")]
    output: PathBuf,

    /// Level optimisasi 0-3
    #[arg(short = 'O', default_value_t = 2,
          hide_short_help = true, help_heading = "Kompilasi native (--emit-llvm)")]
    opt_level: u8,

    /// Target triple untuk cross-compile
    #[arg(long = "target", hide_short_help = true,
          help_heading = "Kompilasi native (--emit-llvm)")]
    target: Option<String>,

    /// Preset tercepat — biner jadi CPU-spesifik dan bukan IEEE ketat
    #[arg(long = "full-power", conflicts_with = "target",
          hide_short_help = true, help_heading = "Kompilasi native (--emit-llvm)")]
    full_power: bool,

    /// Optimalkan untuk CPU mesin ini
    #[arg(long = "native-cpu", conflicts_with = "target",
          hide_short_help = true, help_heading = "Kompilasi native (--emit-llvm)")]
    native_cpu: bool,

    /// Longgarkan aturan floating-point demi kecepatan
    #[arg(long = "fast-math", hide_short_help = true,
          help_heading = "Kompilasi native (--emit-llvm)")]
    fast_math: bool,

    /// Unroll/vectorize agresif, tanpa fast-math
    #[arg(long = "aggressive", hide_short_help = true,
          help_heading = "Kompilasi native (--emit-llvm)")]
    aggressive: bool,

    /// Optimisasi saat link: auto|off|thin|full
    #[arg(long = "lto", default_value = "auto",
          value_parser = ["auto", "off", "thin", "full"],
          hide_short_help = true, help_heading = "Kompilasi native (--emit-llvm)")]
    lto: String,

    /// Pakai linker lld
    #[arg(long = "lld", hide_short_help = true,
          help_heading = "Kompilasi native (--emit-llvm)")]
    lld: bool,

    /// Tahan stdout sampai penuh, bukan per baris
    #[arg(long = "full-buffered-stdout", hide_short_help = true,
          help_heading = "Kompilasi native (--emit-llvm)")]
    full_buffered_stdout: bool,

    /// Jangan buang seksi yang tidak terpakai
    #[arg(long = "no-gc-sections", hide_short_help = true,
          help_heading = "Kompilasi native (--emit-llvm)")]
    no_gc_sections: bool,

    /// Buang simbol dari biner
    #[arg(long = "strip", hide_short_help = true,
          help_heading = "Kompilasi native (--emit-llvm)")]
    strip: bool,

    /// Tulis profil pemakaian ke PATH
    #[arg(long = "pgo-generate", value_name = "PATH", conflicts_with = "pgo_use",
          hide_short_help = true, help_heading = "Kompilasi native (--emit-llvm)")]
    pgo_generate: Option<PathBuf>,

    /// Pakai profil dari PATH
    #[arg(long = "pgo-use", value_name = "PATH", conflicts_with = "pgo_generate",
          hide_short_help = true, help_heading = "Kompilasi native (--emit-llvm)")]
    pgo_use: Option<PathBuf>,

    /// Kompilasi saja, jangan dijalankan
    #[arg(long = "no-run", hide_short_help = true,
          help_heading = "Kompilasi native (--emit-llvm)")]
    no_run: bool,

    // ── Keluaran mentah & alat ──────────────────────────────────────────
    /// Cetak AST lalu berhenti
    #[arg(long = "emit-ast", hide_short_help = true, help_heading = "Alat")]
    emit_ast: bool,

    /// Cetak token lalu berhenti
    #[arg(long = "emit-tokens", hide_short_help = true, help_heading = "Alat")]
    emit_tokens: bool,

    /// Tulis LLVM bitcode (.bc) di samping binernya
    #[arg(long = "emit-bc", hide_short_help = true, help_heading = "Alat")]
    emit_bc: bool,

    /// Lanjutkan parsing sesudah galat, laporkan sebagai peringatan
    #[arg(long = "recover", hide_short_help = true, help_heading = "Alat")]
    recover: bool,

    /// Bersihkan layar tiap --watch mengulang
    #[arg(long = "clear", hide_short_help = true, help_heading = "Alat")]
    clear: bool,

    /// Jalankan language server (dipakai editor, bukan manusia)
    #[arg(long = "lsp", hide_short_help = true, help_heading = "Alat")]
    lsp: bool,
}

// ─── Pipeline ──────────────────────────────────────────────────────────────

fn run_file(path: &PathBuf, cli: &Cli) -> Result<(), Vec<String>> {
    // 1. Baca source
    let src = std::fs::read_to_string(path)
        .map_err(|e| vec![format!("jrgc: tidak bisa baca `{}`: {}", path.display(), crate::host::galat_io(&e))])?;
    // Persis seperti sebelum `run_sumber` dipisah — termasuk `Some("")` untuk
    // berkas tanpa direktori, yang me-resolve sama dengan "." dan sengaja
    // tidak diubah supaya jalur import tidak bergeser diam-diam.
    let dir = path.parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    run_sumber(&path.display().to_string(), src, dir, cli)
}

/// Jalankan sumber yang sudah ada di memori.
fn run_sumber(nama: &str, src: String, dir: PathBuf, cli: &Cli) -> Result<(), Vec<String>> {
    let sf = SourceFile {
        id:   0,
        name: nama.to_string(),
        src,
    };

    // Peta sumber untuk merender diagnostik: tanpa ini `Span` yang sudah
    // dibawa lexer/parser tidak pernah jadi baris/kolom yang bisa dibaca
    // programmer. Dibuat satu kali di sini lalu dipinjamkan ke perender.
    let mut source_map = diag::SourceMap::with_file(sf.clone());

    // Penyunting di HP cuma membawa `jrgc`, jadi formatter `jrgm fmt` juga ada di sini.
    if cli.fmt {
        return match fmt::format_sumber(&sf) {
            Ok(rapi) => { print!("{}", rapi); Ok(()) }
            Err(e) => Err(vec![format!("{}: {}", nama, e)]),
        };
    }

    if cli.verbose {
        eprintln!("[jrgc] lexing: {}", nama);
    }

    // 2. Lexing
    let tokens = pipa::lex_dilaporkan(&sf, &source_map).ok_or_else(Vec::new)?;

    if cli.emit_tokens {
        for tok in &tokens { println!("{:?}", tok.kind); }
        return Ok(());
    }

    if cli.verbose {
        eprintln!("[jrgc] {} token(s) diproduksi", tokens.len());
        eprintln!("[jrgc] parsing: {}", nama);
    }

    // 3. Parsing
    let (ast, parse_errors) = parse(tokens, cli.recover);

    if !parse_errors.is_empty() {
        pipa::laporkan_galat_parse(&parse_errors, cli.recover, &source_map);
        if !cli.recover {
            // Diagnostik lengkapnya sudah dicetak di atas dengan lokasi
            // dan saran; mengembalikan pesan lagi ke `main` cuma akan
            // mencetak baris kedua yang mengulang hal yang sama.
            return Err(Vec::new());
        }
    }

    if cli.emit_ast  { println!("{:#?}", ast); return Ok(()); }

    // Diam kalau bersih, seperti `node --check`: penyunting dan skrip membaca kode keluarnya.
    if cli.periksa {
        let mut jalur = vec![dir.clone()];
        jalur.extend(cli.include_dirs.iter().cloned());
        let mut galat = vm::periksa_dengan_paths(&ast, jalur).err().unwrap_or_default();
        galat.extend(parser::aritas_fungsi_pemakai(&ast).into_iter()
            .map(|(pesan, span)| vm::VmError::at(pesan, Some(span))));
        if galat.is_empty() { return Ok(()); }
        for e in &galat { diag::laporkan_vm(&mut source_map, e); }
        return Err(Vec::new());
    }

    if cli.emit_llvm || cli.apk.is_some() {
        let effective_fast_math = cli.fast_math || cli.full_power;
        let effective_full_buffered_stdout = cli.full_buffered_stdout || cli.full_power;
        let effective_opt_level = if cli.full_power { 3 } else { cli.opt_level };
        if cli.verbose {
            eprintln!("[jrgc] backend LLVM aktif — lowering AST ke LLVM IR...");
        }
        let mut jalur_impor = vec![dir.clone()];
        jalur_impor.extend(cli.include_dirs.iter().cloned());
        let ir = llvm::generate_ir_dgn_jalur(
            &ast,
            llvm::codegen::CodegenOptions {
                fast_math: effective_fast_math,
                full_buffered_stdout: effective_full_buffered_stdout,
            },
            &jalur_impor,
        ).map_err(|errs| {
            // Dulu di sini errornya dijadikan satu baris teks (`"{file}:llvm: {pesan}"`)
            // — kelas bug yang sama dengan yang baru dibereskan di VM.
            let galat: Vec<_> = errs.iter()
                .map(|e| llvm::galat::GalatLlvm::dari_pesan(e))
                .collect();
            diag::laporkan_llvm(&mut source_map, &galat);
            Vec::new()
        })?;

        if let Some(apk) = &cli.apk {
            let opsi = llvm::apk::OpsiApk {
                abi: cli.apk_abi.split(',').map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect(),
                opt_level: effective_opt_level,
                rilis: false,
                aab: apk.extension().map_or(false, |e| e == "aab"),
                aplikasi: llvm::apk::Aplikasi::default(),
            };
            let hasil = llvm::apk::bangun(Some(&ir), std::path::Path::new(nama), apk, &opsi, cli.verbose)
                .map_err(|e| vec![format!("jrgc: --apk: {}", e)])?;
            eprintln!("[jrgc] APK: {}", hasil.display());
            return Ok(());
        }

        let is_ll_text = cli.output.extension().map(|e| e.eq_ignore_ascii_case("ll")).unwrap_or(false);
        if is_ll_text {
            std::fs::write(&cli.output, &ir)
                .map_err(|e| vec![format!("jrgc: gagal menulis {}: {}", cli.output.display(), crate::host::galat_io(&e))])?;
            eprintln!("[jrgc] LLVM IR ditulis ke {}", cli.output.display());
            return Ok(());
        }

        if cli.verbose {
            eprintln!("[jrgc] LLVM IR OK ({} baris) — mengompilasi ke native via clang -O{}...",
                ir.lines().count(), effective_opt_level);
        }
        let pgo = if let Some(path) = &cli.pgo_generate {
            llvm::PgoMode::Generate(path.clone())
        } else if let Some(path) = &cli.pgo_use {
            llvm::PgoMode::Use(path.clone())
        } else {
            llvm::PgoMode::Off
        };
        let lto = match cli.lto.as_str() {
            "auto" if cli.full_power => llvm::LtoMode::Thin,
            "auto" | "off" => llvm::LtoMode::Off,
            "thin" => llvm::LtoMode::Thin,
            "full" => llvm::LtoMode::Full,
            _ => unreachable!("clap membatasi nilai --lto"),
        };
        let native_options = llvm::NativeCompileOptions {
            opt_level: effective_opt_level,
            target: cli.target.clone(),
            native_cpu: cli.native_cpu || cli.full_power,
            aggressive: cli.aggressive || cli.full_power,
            fast_math: effective_fast_math,
            gc_sections: !cli.no_gc_sections,
            strip: cli.strip || cli.full_power,
            lto,
            linker: if cli.lld {
                llvm::LinkerMode::RequireLld
            } else if cli.full_power {
                llvm::LinkerMode::AutoLld
            } else {
                llvm::LinkerMode::System
            },
            pgo,
        };
        let binary = llvm::compile_ir_to_native(&ir, &cli.output, &native_options, cli.verbose)
            .map_err(|e| vec![format!("jrgc: {}", e)])?;

        if cli.no_run {
            eprintln!("[jrgc] native binary: {}", binary.display());
            return Ok(());
        }

        if cli.verbose {
            eprintln!("[jrgc] menjalankan {}...", binary.display());
        }
        let run_path = if binary.is_absolute() {
            binary.clone()
        } else {
            std::env::current_dir().map(|d| d.join(&binary)).unwrap_or(binary.clone())
        };
        let status = process::Command::new(&run_path).status();
        match status {
            Ok(s) => keluar(s.code().unwrap_or(1)),
            Err(e) => return Err(vec![format!("jrgc: gagal menjalankan {}: {}", run_path.display(), crate::host::galat_io(&e))]),
        }
    }

    if cli.no_run {
        if cli.verbose {
            eprintln!("[jrgc] syntax OK ({} top-level items) — --no-run: tidak dieksekusi",
                ast.items.len());
        } else {
            eprintln!("[jrgc] syntax OK");
        }
        return Ok(());
    }

    if cli.verbose {
        eprintln!("[jrgc] {} top-level items — memulai {}...",
            ast.items.len(),
            if cli.use_vm { "Bytecode VM" } else { "interpreter" });
    }

    // 4a. Bytecode VM path
    if cli.use_vm {
        let source_dir = dir.clone();
        // Dulu di sini errornya dibungkus jadi satu baris teks (`"{file}:vm:
        // {pesan}"`), yang berarti backend VM selamanya kehilangan cuplikan sumber …
        let mut jalur = vec![source_dir];
        jalur.extend(cli.include_dirs.iter().cloned());
        if cli.uji {
            return vm::jalankan_uji_with_paths(&ast, jalur, cli.uji_filter.as_deref(), Some(&sf.src))
                .map_err(|e| { diag::laporkan_vm(&mut source_map, &e); Vec::new() })
                .and_then(|gagal| if gagal == 0 { Ok(()) } else {
                    // Keluar tidak-nol TANPA pesan tambahan: tiap kegagalan sudah punya barisnya
                    // sendiri di atas, dan rangkumannya sudah menyebut berapa.
                    keluar(1)
                });
        }
        return vm::run_file_with_paths(&ast, jalur, cli.emit_bc, cli.profile)
            .map_err(|e| {
                diag::laporkan_vm(&mut source_map, &e);
                Vec::new()
            });
    }

    // 4b. Tree-Walking Interpreter (default)
    let source_dir = dir.clone();
    let mut search_paths = vec![source_dir];
    search_paths.extend(cli.include_dirs.iter().cloned());

    if cli.uji {
        let gagal = interpreter::jalankan_uji_with_paths(&ast, search_paths, cli.uji_filter.as_deref(), Some(&sf.src)).map_err(|e| {
            diag::laporkan_runtime(&mut source_map, &e);
            Vec::new()
        })?;
        if gagal > 0 { keluar(1); }
        return Ok(());
    }

    interpret_with_paths(&ast, search_paths).map_err(|e| {
        // Perakitnya tinggal di `diag` supaya `jrgm` memakai yang sama —
        // lihat catatan di `diag::laporkan_runtime`.
        diag::laporkan_runtime(&mut source_map, &e);
        Vec::new()
    })?;

    if cli.verbose {
        eprintln!("[jrgc] eksekusi selesai: {}", nama);
    }

    Ok(())
}

/// Terminal dikembalikan dulu, baru keluar. Dua salinan: biner ini mendeklarasikan ulang
/// modul pustakanya, sementara REPL berjalan di atas salinan milik pustaka.
/// `jrgc --jrgm run ...`, atau biner ini dipanggil lewat nama `jrgm` (tautan, `libjrgm.so`): perannya `jrgm`.
fn sebagai_jrgm(a: Vec<String>) -> Option<Vec<String>> {
    let nama = a.first().and_then(|p| std::path::Path::new(p).file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if nama == "jrgm" || nama.starts_with("jrgm.") || nama == "libjrgm.so" { return Some(a); }
    if a.get(1).map(String::as_str) == Some("--jrgm") {
        return Some(std::iter::once("jrgm".to_string()).chain(a.into_iter().skip(2)).collect());
    }
    None
}

fn keluar(kode: i32) -> ! {
    crate::term::pulihkan();
    jrgc::term::pulihkan();
    process::exit(kode)
}

fn main() {
    // Profil dev: bingkainya beberapa kali lebih besar, jadi penjaga kedalaman (rekursi,
    // hook tampilan) baru kena kalau seluruh jrgc jalan di thread ber-stack lega.
    if cfg!(debug_assertions) {
        let utas = std::thread::Builder::new().name("main".into()).stack_size(1 << 30).spawn(utama);
        match utas.map(|u| u.join()) {
            Ok(Ok(())) => return,
            _ => process::exit(101),
        }
    }
    utama()
}

fn utama() {
    crate::term::pasang_pengait_panik_dengan(jrgc::term::pulihkan);
    if let Some(argumen) = sebagai_jrgm(std::env::args().collect()) {
        // `jrgm test`/`run --watch` menjalankan jrgc sebagai proses anak: itu biner ini sendiri.
        if std::env::var_os("JRGC_BIN").is_none() {
            if let Ok(saya) = std::env::current_exe() { std::env::set_var("JRGC_BIN", saya); }
        }
        jrgm::utama(argumen);
        keluar(0);
    }
    let mut cli = Cli::parse();
    if let Some(p) = &cli.profile_json {
        cli.profile = true;
        profiler::tulis_json_ke(p.clone());
    }

    // Nada diagnostik disetel sekali, sebelum apa pun sempat melapor.
    // Tanpa flag, yang berlaku bawaan (galak) atau variabel lingkungan.
    if let Some(n) = &cli.nada {
        match diag::Nada::dari_nama(n) {
            Some(nd) => diag::set_nada(nd),
            // Nama yang salah DITOLAK, bukan diam-diam jatuh ke bawaan.
            // Diam-diam jatuh berarti seseorang menulis `--nada=hanget`
            // lalu heran kenapa tidak ada bedanya.
            None => {
                eprintln!(
                    "error: nada `{}` tidak ada. yang tersedia: {}",
                    n, diag::Nada::SEMUA.join(", ")
                );
                keluar(2);
            }
        }
    } else if cli.sopan {
        diag::set_nada(diag::Nada::Sopan);
    } else if cli.galak {
        diag::set_nada(diag::Nada::Galak);
    }

    // ── Daftar builtin ──────────────────────────────────────────────────────
    if cli.daftar_builtin {
        for n in vm::daftar_builtin() { println!("{}", n); }
        return;
    }

    // ── Penemuan builtin ────────────────────────────────────────────────────
    if let Some(kata) = &cli.cari {
        let hasil = jrgc::bantu::cari(kata);
        if hasil.is_empty() {
            eprintln!("tidak ada builtin yang namanya memuat `{}`", kata);
            keluar(1);
        }
        for h in &hasil { println!("{}", h); }
        return;
    }
    if let Some(nama) = &cli.doc {
        match jrgc::bantu::jelaskan(nama) {
            Some(t) => println!("{}", t),
            None => {
                let semua = vm::daftar_builtin();
                let rujuk: Vec<&str> = semua.iter().map(|s| s.as_str()).collect();
                match diag::saran_nama_terdekat(nama, rujuk.iter().copied()) {
                    Some(m) => eprintln!("`{}` tidak terdaftar — maksudnya `{}`?", nama, m),
                    None => eprintln!("`{}` tidak terdaftar. Coba `jrgc --cari {}`.", nama, nama),
                }
                keluar(1);
            }
        }
        return;
    }

    // ── Editor ──────────────────────────────────────────────────────────────
    // HARUS di atas cabang REPL: `--sunting f.jrg` menaruh namanya di flag, jadi …
    if let Some(berkas) = cli.sunting.clone() {
        // Nama berkasnya lewat `env::args()`, jalur yang sama dengan `jrgc editor.jrg -- f.jrg`;
        // `jrgc --sunting a.jrg b.jrg` membuka keduanya — berkas kedua dst. tiba sebagai sumber.
        let mut semua = vec![berkas];
        semua.extend(cli.sources.iter().map(|p| p.display().to_string()));
        crate::argumen::pasang("jrgedit", &semua);
        // Sumbernya tertanam (`editor_src`), jadi ia jalan dari direktori mana pun; modulnya
        // dicari di akar maya; dan VM yang menjalankannya — satu ketukan ±7 ms, bukan ±23.
        cli.use_vm = true;
        let utama = format!("{}/jrgedit.jrg", module::AKAR_EDITOR);
        match run_sumber(&utama, editor_src::utama().to_string(), PathBuf::from(module::AKAR_EDITOR), &cli) {
            Ok(()) => return,
            Err(errs) => {
                for e in &errs { eprintln!("error: {}", e); }
                keluar(1);
            }
        }
    }

    // ── REPL ────────────────────────────────────────────────────────────────
    if cli.repl || cli.sources.is_empty() {
        keluar(jrgc::repl::jalankan());
    }

    // ── LSP mode ─────────────────────────────────────────────────────────────
    if cli.lsp {
        lsp::run_server();
        return;
    }

    // ── Inisialisasi profiler ───────────────────────────────────────────────
    if cli.profile {
        profiler::init(true);
        eprintln!("[jrgc] profiler aktif\n");
    }

    // ── Hot reload mode ─────────────────────────────────────────────────────
    if cli.watch {
        if cli.sources.len() != 1 {
            eprintln!("error: --watch hanya mendukung satu source file");
            keluar(1);
        }
        let entry = cli.sources[0].clone();
        let watch_dir = entry.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let search_paths = vec![watch_dir.clone()];

        let mut config = hotreload::ReloadConfig::new(entry);
        config.use_vm       = cli.use_vm;
        config.profiling    = cli.profile;
        config.emit_bc      = cli.emit_bc;
        config.clear_screen = cli.clear;
        config.search_paths = search_paths;
        config.watch_dir    = watch_dir;

        hotreload::watch(config);
        return; // watch blocks sampai Ctrl+C
    }

    let mut had_error = false;
    for source in &cli.sources {
        crate::argumen::pasang(&source.display().to_string(), &cli.argumen_program);
        match run_file(source, &cli) {
            Ok(()) => {}
            Err(errors) => {
                for e in &errors { eprintln!("error: {}", e); }
                had_error = true;
            }
        }
    }

    // ── Tampilkan laporan profiler ──────────────────────────────────────────
    if cli.profile {
        profiler::report();
    }

    if had_error { keluar(1); }
}
