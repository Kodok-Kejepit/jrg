// src/argumen.rs
// Argumen untuk PROGRAM yang dijalankan, bukan untuk jrgc. Dipisah supaya
// `env::args()` punya satu sumber, dan supaya REPL/test bisa mengisinya.

use std::sync::{Mutex, OnceLock};

static ARG: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn simpanan() -> &'static Mutex<Vec<String>> {
    ARG.get_or_init(|| Mutex::new(Vec::new()))
}

/// Dipanggil sekali per berkas yang dijalankan.
pub fn pasang(skrip: &str, sisa: &[String]) {
    let mut v = vec![skrip.to_string()];
    v.extend(sisa.iter().cloned());
    *simpanan().lock().unwrap_or_else(|p| p.into_inner()) = v;
}

pub fn ambil() -> Vec<String> {
    simpanan().lock().unwrap_or_else(|p| p.into_inner()).clone()
}
