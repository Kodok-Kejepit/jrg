// src/jrgm/konsol.rs — `jrgm apk` untuk proyek C/C++: programnya dikompilasi NDK jadi executable
// (`program_c`), dan aplikasinya halaman konsol (tampilan web, `konsol/main.jrg`) yang menjalankannya,
// menampilkan keluarannya, dan meneruskan masukan. Berkas proyek ikut sebagai folder kerjanya.

use std::path::{Path, PathBuf};

use super::bahasa::Resep;
use super::manifes::Manifest;
use jrgc::llvm::apk::{self, OpsiApk};

const MAIN: &str = include_str!("konsol/main.jrg");
const HALAMAN: &str = include_str!("konsol/index.html");
const TANPA_BUFFER: &str = include_str!("konsol/tanpa_buffer.c");

/// Bahasa yang bisa dibungkus konsol APK.
pub(crate) fn didukung(r: &Resep) -> bool { r.nama == "c" || r.nama == "cpp" }

/// Tulis berkas konsol ke `target/apk-konsol/`; jawabannya (main.jrg, index.html, tanpa_buffer.c).
pub(crate) fn tulis_templat(akar: &Path) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let d = akar.join("target").join("apk-konsol");
    std::fs::create_dir_all(&d).map_err(|e| format!("{}: {}", d.display(), jrgc::host::galat_io(&e)))?;
    let tulis = |n: &str, isi: &str| -> Result<PathBuf, String> {
        let p = d.join(n);
        std::fs::write(&p, isi).map_err(|e| format!("{}: {}", p.display(), jrgc::host::galat_io(&e)))?;
        Ok(p)
    };
    Ok((tulis("main.jrg", MAIN)?, tulis("index.html", HALAMAN)?, tulis("tanpa_buffer.c", TANPA_BUFFER)?))
}

/// Sumber program: semua berkas bahasa itu di `src/` (atau akar), seperti `jrgm build`.
pub(crate) fn sumber(akar: &Path, r: &Resep) -> Vec<PathBuf> {
    let mut v = Vec::new();
    let dasar = if akar.join("src").is_dir() { akar.join("src") } else { akar.to_path_buf() };
    kumpul(&dasar, r.ekstensi, &mut v);
    v.sort();
    v
}

fn kumpul(dir: &Path, ekstensi: &[&str], keluar: &mut Vec<PathBuf>) {
    let Ok(baca) = std::fs::read_dir(dir) else { return };
    for e in baca.flatten() {
        let p = e.path();
        let nama = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if nama.starts_with('.') || nama == "target" { continue; }
            kumpul(&p, ekstensi, keluar);
        } else if p.extension().and_then(|x| x.to_str()).map_or(false, |x| ekstensi.contains(&x)) {
            keluar.push(p);
        }
    }
}

pub(crate) fn bangun(akar: &Path, m: &Manifest, r: &Resep, opsi: &OpsiApk, tujuan: &Path, verbose: bool) -> Result<PathBuf, String> {
    let src = sumber(akar, r);
    if src.is_empty() { return Err(format!("tidak ada berkas .{} di src/ untuk dibungkus", r.ekstensi.join("/."))); }
    let (main, halaman, tanpa_buffer) = tulis_templat(akar)?;
    let mut app = opsi.aplikasi.clone();
    let mut semua = src.clone();
    semua.push(tanpa_buffer);
    app.program_c.push(("libprogram".to_string(), semua));
    app.entri = Some("_konsol/main.jrg".to_string());
    super::android::kumpul_proyek_pub(akar, &m.android.abaikan, &mut app.berkas_proyek);
    app.berkas_proyek.push(("_konsol/main.jrg".to_string(), main));
    app.berkas_proyek.push(("_konsol/index.html".to_string(), halaman));
    if let Some(d) = tujuan.parent() { let _ = std::fs::create_dir_all(d); }
    let opsi = OpsiApk { abi: opsi.abi.clone(), opt_level: opsi.opt_level, rilis: opsi.rilis, aab: opsi.aab, aplikasi: app };
    apk::bangun(None, Path::new(&m.name), tujuan, &opsi, verbose)
}
