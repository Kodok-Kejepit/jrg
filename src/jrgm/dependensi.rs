// src/jrgm/dependensi.rs — graf dependensi lokal

use std::path::{Path, PathBuf};

use super::manifes::Manifest;

// ─── Graf dependensi lokal
// ─────────────────────────────────────────────────

/// Satu simpul graf: sebuah proyek beserta kedalamannya dari akar.
pub(crate) struct Simpul {
    pub(crate) nama: String,
    pub(crate) versi: String,
    pub(crate) akar: PathBuf,
    pub(crate) kedalaman: usize,
    /// Sudah pernah muncul di tempat lain di graf ini. Ditandai, bukan
    /// disembunyikan: pembacanya perlu tahu sebuah paket dipakai dua kali.
    pub(crate) berulang: bool,
}

/// Telusuri graf dependensi mulai dari `akar`, dalam urutan yang terbaca.
pub(crate) fn graf_dependensi(akar: &Path) -> Result<Vec<Simpul>, String> { graf(akar, false) }

/// Sama, ditambah `[dev-dependencies]` milik akar (untuk `jrgm test`/`lint`).
pub(crate) fn graf_dependensi_dev(akar: &Path) -> Result<Vec<Simpul>, String> { graf(akar, true) }

fn graf(akar: &Path, dev: bool) -> Result<Vec<Simpul>, String> {
    fn kanonik(p: &Path) -> PathBuf { p.canonicalize().unwrap_or_else(|_| p.to_path_buf()) }

    fn turun(
        akar: &Path, kedalaman: usize, jalur: &mut Vec<(String, PathBuf)>,
        terlihat: &mut Vec<PathBuf>, keluar: &mut Vec<Simpul>, kunci: &mut super::git::Kunci, dev: bool,
    ) -> Result<(), String> {
        let m = Manifest::load(akar)?;
        let mut semua = m.dependencies.clone();
        if dev { semua.extend(m.dev_dependencies.iter().cloned()); }
        for mut d in semua {
            if let Some((url, rev)) = &d.git {
                d.akar = super::git::siapkan(kunci, &d.nama, url, rev, false)
                    .map_err(|e| format!("dependency `{}`: {}", d.nama, e))?;
            }
            let k = kanonik(&d.akar);
            // Siklus: paket ini sedang berada di jalur yang sedang ditelusuri.
            if let Some(i) = jalur.iter().position(|(_, p)| *p == k) {
                let rantai: Vec<String> = jalur[i..].iter().map(|(n, _)| n.clone())
                    .chain(std::iter::once(d.nama.clone())).collect();
                return Err(format!(
                    "dependency melingkar: {}\n\
                     bantuan: putus salah satu tautan di rantai itu, atau pindahkan bagian \
                     yang dipakai bersama ke paket ketiga yang tidak mengimpor balik",
                    rantai.join(" -> ")
                ));
            }
            if !d.akar.join("jrgm.toml").is_file() {
                return Err(format!(
                    "dependency `{}` menunjuk `{}`, tapi di sana tidak ada `jrgm.toml`\n\
                     bantuan: path-nya relatif terhadap `jrgm.toml` yang menyebutnya, \
                     bukan terhadap direktori tempat perintah dijalankan",
                    d.nama, d.akar.display()
                ));
            }
            let berulang = terlihat.contains(&k);
            let dm = Manifest::load(&k)?;
            keluar.push(Simpul {
                nama: d.nama.clone(), versi: dm.version.clone(),
                // Bentuk kanonik, karena inilah yang dipakai `jrgm build` untuk menyebut
                // berkas yang gagal.
                akar: k.clone(), kedalaman, berulang,
            });
            if berulang { continue; }
            terlihat.push(k.clone());
            jalur.push((d.nama.clone(), k));
            turun(&d.akar, kedalaman + 1, jalur, terlihat, keluar, kunci, false)?;
            jalur.pop();
        }
        Ok(())
    }

    let mut keluar = Vec::new();
    // Nama paketnya sendiri, bukan sebuah penampung: rantai siklus dibaca
    // orang, dan `<akar> -> b -> c -> a` menyuruhnya menebak mana akarnya.
    let nama_akar = Manifest::load(akar).map(|m| m.name).unwrap_or_else(|_| "?".into());
    let mut jalur = vec![(nama_akar, kanonik(akar))];
    let mut terlihat = Vec::new();
    let mut kunci = super::git::Kunci::baca(akar);
    turun(akar, 1, &mut jalur, &mut terlihat, &mut keluar, &mut kunci, dev)?;
    kunci.simpan()?;
    Ok(keluar)
}

/// Direktori tambahan tempat `import "x"` boleh mencari, dari graf
/// dependensi.
pub(crate) fn path_dependensi(graf: &[Simpul]) -> Result<Vec<PathBuf>, String> {
    let mut path = Vec::new();
    let mut modul: Vec<(String, String)> = Vec::new();   // (nama modul, paket)
    let mut bentrok: Vec<String> = Vec::new();

    for s in graf {
        if s.berulang { continue; }
        for sub in ["src", ""] {
            let d = if sub.is_empty() { s.akar.clone() } else { s.akar.join(sub) };
            if d.is_dir() && !path.contains(&d) { path.push(d); }
        }
        let dir = s.akar.join("src");
        if let Ok(baca) = std::fs::read_dir(&dir) {
            for e in baca.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) != Some("jrg") { continue; }
                let Some(n) = p.file_stem().and_then(|x| x.to_str()) else { continue };
                if n == "main" { continue; }
                if let Some((_, lain)) = modul.iter().find(|(m, _)| m == n) {
                    bentrok.push(format!("  `{}` ada di `{}` dan `{}`", n, lain, s.nama));
                } else {
                    modul.push((n.to_string(), s.nama.clone()));
                }
            }
        }
    }
    if !bentrok.is_empty() {
        return Err(format!(
            "dua dependency menyediakan modul bernama sama:\n{}\n\
             bantuan: ganti nama salah satu berkasnya — `import` menyebut nama modul, \
             bukan nama paketnya, jadi tidak ada cara menunjuk yang mana yang dimaksud",
            bentrok.join("\n")
        ));
    }
    Ok(path)
}

/// Check syntax saja: Lex → Parse (tanpa interpret) Tambahkan jalur pencarian
/// modul milik dependensi lokal ke `v`.
pub(crate) fn tambah_path_dependensi(akar: &Path, v: &mut Vec<std::path::PathBuf>) {
    if !akar.join("jrgm.toml").is_file() { return; }
    if let Ok(g) = graf_dependensi(akar) {
        if let Ok(pd) = path_dependensi(&g) { v.extend(pd); }
    }
}

/// Sama, ditambah `[dev-dependencies]`.
pub(crate) fn tambah_path_dependensi_dev(akar: &Path, v: &mut Vec<std::path::PathBuf>) {
    if !akar.join("jrgm.toml").is_file() { return; }
    if let Ok(g) = graf_dependensi_dev(akar) {
        if let Ok(pd) = path_dependensi(&g) { v.extend(pd); }
    }
}

/// Mengumpulkan tiap `.jrg` di bawah sebuah direktori, rekursif.
pub(crate) fn kumpul_jrg(dir: &Path, keluar: &mut Vec<PathBuf>) {
    let Ok(baca) = std::fs::read_dir(dir) else { return };
    let mut isi: Vec<_> = baca.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    isi.sort();
    for p in isi {
        if p.is_dir() {
            // `target/` isinya keluaran build, bukan sumber orang.
            if p.file_name().map_or(false, |n| n == "target") { continue; }
            kumpul_jrg(&p, keluar);
        } else if p.extension().map_or(false, |e| e == "jrg") {
            keluar.push(p);
        }
    }
}
