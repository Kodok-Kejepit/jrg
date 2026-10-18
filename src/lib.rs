// src/lib.rs
// Library crate root — mengekspos semua modul publik untuk dipakai

extern crate alloc;
pub mod antrean;
pub mod sinkron;
pub mod ruang_tipe;
pub mod hash_cepat;
pub mod json_inti;
pub mod diag;
pub mod host;
pub mod pipa;
pub mod lexer;
pub mod parser;
pub mod interpreter;

#[allow(dead_code, unused_imports)]
pub mod ffi;

pub mod canvas;
pub mod profiler;
pub mod module;
pub mod editor_src;
pub mod vm;

/// Backend ketiga: lowering AST ke LLVM IR, lalu native lewat `clang`.
pub mod llvm;

pub mod fmt;
pub mod uji;
pub mod bantu;
pub mod argumen;
pub mod lebar;
pub mod term;
pub mod putar;
pub mod baris;
pub mod repl;
pub mod hotreload;
pub mod lsp;
pub mod stdlib;
pub mod wokwok;
pub mod suara;

/// Library "full power" all-in-one: networking, HTTP server/client, JSON,
/// numerik tajam, dan logging — lihat `dok::mod` untuk ringkasan API.
pub mod dok;
