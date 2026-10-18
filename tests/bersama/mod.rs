//! Pembantu yang dipakai bersama berkas-berkas tes integrasi.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Jalur direktori sementara yang unik: PID + nomor urut se-proses + nano-detik.
/// Nano-detik saja TIDAK unik lintas thread — dua thread tes bisa membaca nilai
/// yang sama persis; nomor urutlah yang menjaminnya.
pub fn dir_unik(awalan: &str) -> PathBuf {
    static URUT: AtomicU64 = AtomicU64::new(0);
    let nano = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "{}-{}-{}-{}", awalan, std::process::id(), URUT.fetch_add(1, Ordering::Relaxed), nano))
}

/// Keluaran satu program FFI di dua mesin harus sama. Dengan `JRG_FFI_CATAT=<berkas>`
/// bedanya dicatat ke sana alih-alih menggagalkan tes — untuk menyapu sekali jalan.
#[allow(dead_code)]
pub fn dua_mesin_sama(sumber: &str, interp: &str, vm: &str) {
    // Puncak memori proses memang beda per mesin, dan direktori sementaranya bernomor.
    let saring = |t: &str| t.lines().filter(|l| !l.starts_with("VmHWM"))
        .map(|l| if l.contains("/jrg-") { l.chars().filter(|c| !c.is_ascii_digit()).collect() } else { l.to_string() })
        .collect::<Vec<_>>().join("\n");
    if saring(interp) == saring(vm) { return; }
    let laporan = format!("=== SUMBER\n{}\n--- interpreter\n{}\n--- vm\n{}\n", sumber, interp, vm);
    if let Ok(p) = std::env::var("JRG_FFI_CATAT") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
            let _ = f.write_all(laporan.as_bytes());
        }
        return;
    }
    panic!("dua mesin menjawab beda untuk program FFI yang sama:\n{}", laporan);
}
