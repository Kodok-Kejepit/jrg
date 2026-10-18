// src/stdlib/mod.rs
// Native Rust Standard Library Registration

pub mod dict;
pub mod dok;
pub mod dotnet;
pub mod sistem;
pub mod jawa;
pub mod koleksi;
pub mod fs;
pub mod json;
pub mod jvm;
pub mod kotlin;
pub mod terminal;
pub mod proses;
pub mod wasm;
pub mod http;
pub mod regex;
pub mod future;
mod kanvas;

use crate::vm::vm::Vm;

/// Register semua native stdlib ke dalam VM
pub fn register_all(vm: &mut Vm) {
    dict::register(vm);
    // 94 dari 96 fungsi `dok::`; lihat kepala berkasnya untuk dua sisanya.
    dok::register(vm);
    sistem::register(vm);
    jawa::register(vm);
    koleksi::register(vm);
    fs::register(vm);
    json::register(vm);
    jvm::register(vm);
    kotlin::daftar(vm);
    terminal::daftar(vm);
    proses::daftar(vm);
    wasm::register(vm);
    dotnet::register(vm);
    http::register(vm);
    regex::register(vm);
    future::register(vm);
    // `wokwok::` — library numerik. Tinggal di `src/wokwok/` (bukan di
    // sini) karena ia punya tipe datanya sendiri dan bukan cuma sekumpulan
    // fungsi; yang di sini cuma pendaftarannya.
    kanvas::register(vm);
    crate::wokwok::daftar::register(vm);
    // `suara::` — bunyi sebagai larik `wokwok::`; tinggal di `src/suara/`.
    crate::suara::register(vm);
}

/// Return embedded JRG stdlib source code (tanpa file system)
pub fn get_embedded(import_path: &str) -> Option<&'static str> {
    match import_path {
        "std/iterator" | "iterator" => Some(include_str!("../../stdlib/iterator.jrg")),
        "std/result"   | "result"   => Some(include_str!("../../stdlib/result.jrg")),
        "std/option"   | "option"   => Some(include_str!("../../stdlib/option.jrg")),
        "std/testing"  | "testing"  => Some(include_str!("../../stdlib/testing.jrg")),
        _ => None,
    }
}
