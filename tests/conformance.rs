// tests/conformance.rs

use std::path::{Path, PathBuf};

use jrgc::interpreter::interpret_with_paths;
use jrgc::lexer::{lex, SourceFile};
use jrgc::parser::parse;

fn conformance_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("conformance")
}

fn collect_programs() -> Vec<PathBuf> {
    let dir = conformance_dir();
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("tidak bisa baca {}: {}", dir.display(), e))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "jrg"))
        .collect();
    // Urutan file dari `read_dir` bergantung filesystem; diurutkan supaya
    // kegagalan selalu dilaporkan dalam urutan yang sama.
    out.sort();
    out
}

/// Stack untuk thread yang menjalankan program konformansi.
const STACK_SIZE: usize = 32 * 1024 * 1024;

/// Jalankan satu program; `Ok(())` berarti program selesai tanpa error.
fn run_program(path: &Path) -> Result<(), String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("tidak bisa baca file: {}", e))?;
    let sf = SourceFile { id: 0, name: path.display().to_string(), src };

    let tokens = lex(&sf).map_err(|e| format!("lex: {}", e.msg))?;
    let (ast, errors) = parse(tokens, false);
    if !errors.is_empty() {
        let msgs: Vec<&str> = errors.iter().map(|e| e.msg.as_str()).collect();
        return Err(format!("parse: {}", msgs.join("; ")));
    }
    interpret_with_paths(&ast, Vec::new()).map_err(|e| format!("runtime: {}", e.message))
}

/// Bungkus `run_program` di thread ber-stack besar (lihat `STACK_SIZE`).
fn run_program_with_stack(path: &Path) -> Result<(), String> {
    let owned = path.to_path_buf();
    std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(move || run_program(&owned))
        .map_err(|e| format!("tidak bisa spawn thread: {}", e))?
        .join()
        .map_err(|_| "program panik saat dijalankan".to_string())?
}

#[test]
fn conformance_suite_passes() {
    let programs = collect_programs();
    assert!(
        !programs.is_empty(),
        "tidak ada program konformansi ditemukan di {}",
        conformance_dir().display()
    );

    // Semua file dijalankan dulu, baru dilaporkan sekaligus — supaya satu
    // kegagalan tidak menyembunyikan kegagalan lain di file berikutnya.
    let mut failures = Vec::new();
    for path in &programs {
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        if let Err(err) = run_program_with_stack(path) {
            failures.push(format!("  {} -> {}", name, err));
        }
    }

    assert!(
        failures.is_empty(),
        "{} dari {} program konformansi gagal:\n{}",
        failures.len(),
        programs.len(),
        failures.join("\n")
    );
}

/// Penjaga agar suite tidak diam-diam menyusut: kalau sebuah file dihapus
/// atau berhenti terbaca, ini gagal alih-alih "lulus" karena tidak ada
/// yang dijalankan.
#[test]
fn conformance_suite_covers_expected_areas() {
    let names: Vec<String> = collect_programs()
        .iter()
        .map(|p| p.file_stem().unwrap_or_default().to_string_lossy().to_string())
        .collect();

    for area in [
        "control_flow",
        "collections",
        "pattern_matching",
        "functions",
        "values_and_operators",
        "strings",
        "indexing",
        "closures",
        "memori_manual",
        "semantik_nilai",
    ] {
        assert!(
            names.iter().any(|n| n == area),
            "area konformansi `{}` hilang dari suite (yang ada: {:?})",
            area,
            names
        );
    }
}
