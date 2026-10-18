// src/jrgm/jalankan.rs — menjalankan dan memeriksa satu berkas

use super::dependensi::{graf_dependensi, path_dependensi};
use std::path::{Path, PathBuf};

use jrgc::lexer::SourceFile;
use jrgc::parser::parse;
use jrgc::interpreter::interpret_with_paths;

use super::dependensi::tambah_path_dependensi;

// ─── Pipeline helper ──────────────────────────────────────────────────────

/// Eksekusi satu file .jrg: Lex → Parse → VM/Interpret (+ optional profiler)
pub(crate) fn run_jrg_file(path: &Path, verbose: bool, prof: bool, use_vm: bool, emit_bc: bool) -> Result<(), String> {
    if prof { jrgc::profiler::init(true); eprintln!("[jrgm] profiler aktif\n"); }

    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("tidak bisa baca `{}`: {}", path.display(), jrgc::host::galat_io(&e)))?;
    let sf = SourceFile { id: 0, name: path.display().to_string(), src };
    // Peta sumber dipakai diagnostik untuk mengambil cuplikan baris dan menaruh
    // caret di kolom yang benar — galat lex dan parse juga, sama dengan `jrgc`.
    let mut source_map = jrgc::diag::SourceMap::with_file(sf.clone());

    if verbose { eprintln!("[jrgm] lexing: {}", path.display()); }
    let tokens = jrgc::pipa::lex_dilaporkan(&sf, &source_map).ok_or_else(String::new)?;
    if verbose { eprintln!("[jrgm] {} token(s) diproduksi", tokens.len()); }

    let (ast, parse_errors) = parse(tokens, false);
    if !parse_errors.is_empty() {
        jrgc::pipa::laporkan_galat_parse(&parse_errors, false, &source_map);
        return Err(String::new());
    }
    if verbose { eprintln!("[jrgm] {} items — memulai {}...",
        ast.items.len(), if use_vm { "Bytecode VM" } else { "interpreter" }); }

    let source_dir = path.parent().map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let project_root = std::env::current_dir().unwrap_or_else(|_| source_dir.clone());
    let search_paths: Vec<std::path::PathBuf> = {
        let mut v = vec![source_dir, project_root.clone()];
        tambah_path_dependensi(&project_root, &mut v);
        v.dedup();
        v
    };

    // ── Bytecode VM path ─────────────────────────────────────────────────
    if use_vm {
        let result = jrgc::vm::run_file_with_paths(&ast, search_paths.clone(), emit_bc, prof);
        if prof { jrgc::profiler::report(); }
        if verbose { eprintln!("[jrgm] selesai (VM)"); }
        return match result {
            Ok(()) => Ok(()),
            Err(e) => {
                // Sejak `VmError` membawa `Span`, jalurnya sama dengan
                // interpreter: cuplikan sumber dan caret ikut tampil.
                jrgc::diag::laporkan_vm(&mut source_map, &e);
                Err(String::new())
            }
        };
    }

    // ── Tree-walking interpreter path ─────────────────────────────────────
    let hasil = interpret_with_paths(&ast, search_paths);
    if prof { jrgc::profiler::report(); }
    let result = match hasil {
        Ok(()) => Ok(()),
        Err(e) => {
            jrgc::diag::laporkan_runtime(&mut source_map, &e);
            // Pesannya SUDAH dicetak lengkap di atas; string kosong
            // menandakan itu ke pemanggil supaya ia tidak mencetak versi
            // telanjangnya lagi di bawah.
            Err(String::new())
        }
    };
    if verbose { eprintln!("[jrgm] selesai"); }
    result
}


pub(crate) fn check_jrg_file(path: &Path, verbose: bool) -> Result<(), String> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("tidak bisa baca `{}`: {}", path.display(), jrgc::host::galat_io(&e)))?;

    let sf = SourceFile { id: 0, name: path.display().to_string(), src };

    if verbose { eprintln!("[jrgm] lexing: {}", path.display()); }

    let source_map = jrgc::diag::SourceMap::with_file(sf.clone());
    let tokens = jrgc::pipa::lex_dilaporkan(&sf, &source_map).ok_or_else(String::new)?;
    let (ast, parse_errors) = parse(tokens, true);

    if parse_errors.is_empty() {
        if verbose {
            eprintln!("[jrgm] syntax OK ({} items)", ast.items.len());
        } else {
            eprintln!("[jrgm] syntax OK: {}", path.display());
        }
        Ok(())
    } else {
        // Diurai dengan `recover` supaya SEMUA galatnya terkumpul, tapi tetap
        // dilaporkan sebagai galat: build yang punya satu saja harus gagal.
        jrgc::pipa::laporkan_galat_parse(&parse_errors, false, &source_map);
        Err(String::new())
    }
}
