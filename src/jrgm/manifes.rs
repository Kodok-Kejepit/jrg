// src/jrgm/manifes.rs — manifest `jrgm.toml` dan pembacanya

use std::path::{Path, PathBuf};

// ─── Manifest ─────────────────────────────────────────────────────────────

pub(crate) struct Manifest {
    pub(crate) name:    String,
    pub(crate) version: String,
    pub(crate) entry:   PathBuf,
    /// Dari `[python]` / `packages = [...]` di jrgm.toml — daftar package Python
    /// yang harus bisa di-`import` sebelum project ini jalan (lihat …
    pub(crate) python_packages: Vec<String>,
    /// Dependensi lokal, dari `[dependencies]`: `nama = { path = "../x" }`.
    pub(crate) dependencies: Vec<Dependensi>,
    /// `[dev-dependencies]`: cuma untuk `jrgm test` dan `jrgm lint` proyek ini, tidak transitif.
    pub(crate) dev_dependencies: Vec<Dependensi>,
    /// `[package] repositori`: tujuan `jrgm publish` (bawaan: remote `origin`).
    pub(crate) repositori: Option<String>,
    /// `[android]`: identitas APK untuk `jrgm apk`.
    pub(crate) android: Android,
    /// `[package] bahasa`: `None`/`"jrg"` proyek JRG, selain itu resep di `bahasa.rs`.
    pub(crate) bahasa: Option<String>,
    /// `[perintah]`: langkah yang menimpa resep bahasanya (`jalan`, `bangun`, `uji`, `rapikan`).
    pub(crate) perintah: Vec<(String, Vec<String>)>,
    pub(crate) perintah_asing: Vec<String>,
}

#[derive(Default, Clone, Debug)]
pub(crate) struct Android {
    pub(crate) nama: Option<String>,
    pub(crate) paket: Option<String>,
    pub(crate) versi: Option<String>,
    pub(crate) kode: Option<String>,
    pub(crate) orientasi: Option<String>,
    /// Sudah digabung relatif terhadap akar proyek.
    pub(crate) ikon: Option<PathBuf>,
    pub(crate) abi: Vec<String>,
    /// Pustaka native proyek, relatif terhadap akar; `{abi}` diganti nama ABI.
    pub(crate) pustaka: Vec<String>,
    pub(crate) kunci: Option<PathBuf>,
    pub(crate) alias: Option<String>,
    pub(crate) izin: Vec<String>,
    /// `"kanvas"` (bawaan, lewat LLVM) atau `"web"` (WebView + program sebagai pelayan loopback).
    pub(crate) tampilan: Option<String>,
    pub(crate) port: Option<String>,
    pub(crate) mesin: Option<String>,
    /// `[android.pustaka_c]`: `libnama = ["a.c", "b.cpp"]`, jalur relatif akar.
    pub(crate) pustaka_c: Vec<(String, Vec<PathBuf>)>,
    /// `[android.pustaka_rust]`: `libnama = "rust/crate"`.
    pub(crate) pustaka_rust: Vec<(String, PathBuf)>,
    /// `[android.program_c]`: `libnama_exe = ["a.c"]`, executable yang ikut di APK.
    pub(crate) program_c: Vec<(String, Vec<PathBuf>)>,
    /// `bahasa = ["lua", "js", "sql"]`: penjalan bahasa lain yang ikut di APK.
    pub(crate) bahasa: Vec<String>,
    pub(crate) target_sdk: Option<String>,
    /// Jalur relatif akar yang tidak ikut ke `assets/proyek/` (tampilan web).
    pub(crate) abaikan: Vec<String>,
    pub(crate) tautan: Vec<String>,
    pub(crate) ikon_latar: Option<String>,
    pub(crate) ikon_depan: Option<PathBuf>,
    /// Kunci yang tidak dikenal: dilaporkan, bukan dilewati diam-diam.
    pub(crate) asing: Vec<String>,
}

/// Satu dependensi lokal yang sudah dibaca dari manifes.
#[derive(Clone, Debug)]
pub(crate) struct Dependensi {
    pub(crate) nama: String,
    /// Akar proyeknya, sudah digabung relatif terhadap manifes yang menyebut
    /// — jadi `path = "../bantu"` di `a/jrgm.toml` menunjuk tempat yang sama
    /// dari direktori kerja mana pun.
    pub(crate) akar: PathBuf,
    /// `{ git = "url", rev = "..." }`: akarnya baru diketahui sesudah diambil (`git::siapkan`).
    pub(crate) git: Option<(String, String)>,
}

/// Buang komentar ujung baris (`# ...` di luar tanda kutip), seperti TOML.
pub(crate) fn buang_komentar(val: &str) -> &str {
    let mut kutip = false;
    for (i, c) in val.char_indices() {
        match c {
            '"' => kutip = !kutip,
            '#' if !kutip => return val[..i].trim_end(),
            _ => {}
        }
    }
    val
}

/// Satu baris logis per kunci: array yang dibuka `[` lalu ditutup di baris lain digabung dulu
/// (dulu baris lanjutannya dibuang diam-diam, dan `jalan = [` terbaca sebagai langkah kosong).
fn baris_logis(content: &str) -> Vec<String> {
    let selisih = |s: &str| {
        let (mut n, mut kutip, mut lolos) = (0i32, false, false);
        for c in s.chars() {
            match c {
                _ if lolos => lolos = false,
                '\\' if kutip => lolos = true,
                '"' => kutip = !kutip,
                '[' if !kutip => n += 1,
                ']' if !kutip => n -= 1,
                _ => {}
            }
        }
        n
    };
    let mut hasil: Vec<String> = Vec::new();
    let mut buka = 0;
    for line in content.lines() {
        let t = line.trim();
        if buka > 0 {
            let isi = buang_komentar(t);
            buka += selisih(isi);
            if let Some(akhir) = hasil.last_mut() { akhir.push(' '); akhir.push_str(isi); }
            continue;
        }
        match t.split_once('=') {
            Some((k, v)) if !t.starts_with('#') && buang_komentar(v.trim()).starts_with('[') => {
                let v = buang_komentar(v.trim());
                buka = selisih(v).max(0);
                hasil.push(format!("{}= {}", k, v));
            }
            _ => hasil.push(t.to_string()),
        }
    }
    hasil
}

/// Buang tanda kutip pembungkus + spasi dari sebuah value TOML sederhana.
pub(crate) fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').trim().to_string()
}

/// Nilai kunci `kunci` dari sebuah value inline-table `{ path = "../x" }`, kalau ada.
pub(crate) fn nilai_inline(val: &str, kunci: &str) -> Option<String> {
    let v = val.trim();
    let dalam = v.strip_prefix('{')?.strip_suffix('}')?;
    for bagian in dalam.split(',') {
        let (k, x) = bagian.split_once('=')?;
        if k.trim() == kunci {
            let p = unquote(x);
            if !p.is_empty() { return Some(p); }
        }
    }
    None
}

/// Satu baris `[dependencies]`: `{ path = .. }` atau `{ git = .., rev = .. | version = .. }`.
fn dependensi(nama: &str, val: &str, root: &Path) -> Result<Dependensi, String> {
    if let Some(rel) = nilai_inline(val, "path") {
        return Ok(Dependensi { nama: nama.to_string(), akar: root.join(rel), git: None });
    }
    if let Some(url) = nilai_inline(val, "git") {
        let rev = nilai_inline(val, "rev").or_else(|| nilai_inline(val, "version").map(|v| super::git::rev_versi(&v)))
            .unwrap_or_default();
        return Ok(Dependensi { nama: nama.to_string(), akar: PathBuf::new(), git: Some((url, rev)) });
    }
    Err(format!("dependensi `{}` = {} tidak dimengerti\n  bantuan: `{{ path = \"../x\" }}` atau \
        `{{ git = \"url\", version = \"0.1.0\" }}` — registry belum ada, jadi versi saja tidak cukup", nama, val))
}

/// Parser sangat minim untuk value array-of-strings satu baris:
/// `["a", "b", "c"]`. Tidak mendukung array multi-baris — kalau
/// dibutuhkan nanti, ini titik yang tepat untuk diperluas.
pub(crate) fn parse_string_array(val: &str) -> Vec<String> {
    let val = val.trim();
    let inner = val.strip_prefix('[').and_then(|v| v.strip_suffix(']')).unwrap_or(val);
    inner
        .split(',')
        .map(|s| unquote(s))
        .filter(|s| !s.is_empty())
        .collect()
}

/// Nama kanonik sebuah kunci `[perintah]` (Indonesia atau Inggris).
fn kunci_perintah(k: &str) -> Option<&'static str> {
    Some(match k {
        "jalan" | "run" => "jalan",
        "bangun" | "build" => "bangun",
        "uji" | "test" => "uji",
        "rapikan" | "fmt" => "rapikan",
        "lint" | "periksa" => "lint",
        _ => return None,
    })
}

/// `"satu perintah"` atau `["langkah 1", "langkah 2"]`; `\"` di dalam teks berarti tanda kutip.
pub(crate) fn daftar_perintah(val: &str) -> Vec<String> {
    let v = val.trim();
    let dalam = match v.strip_prefix('[').and_then(|x| x.strip_suffix(']')) { Some(d) => d, None => v };
    let mut hasil = Vec::new();
    let mut kini: Option<String> = None;
    let mut lolos = false;
    for c in dalam.chars() {
        match (&mut kini, c) {
            (Some(s), _) if lolos => { s.push(c); lolos = false; }
            (Some(_), '\\') => lolos = true,
            (Some(_), '"') => hasil.push(kini.take().unwrap()),
            (Some(s), c) => s.push(c),
            (None, '"') => kini = Some(String::new()),
            (None, _) => {}
        }
    }
    hasil
}

impl Manifest {
    pub(crate) fn parse_from_str(content: &str, root: &Path) -> Result<Self, String> {
        let mut name    = String::from("unnamed");
        let mut version = String::from("0.0.0");
        let mut entry   = String::new();
        let mut python_packages: Vec<String> = Vec::new();
        let mut dependencies: Vec<Dependensi> = Vec::new();
        let mut dev_dependencies: Vec<Dependensi> = Vec::new();
        let mut repositori = None;
        let mut android = Android::default();
        let mut section = String::new();
        let mut bahasa = None;
        let mut perintah: Vec<(String, Vec<String>)> = Vec::new();
        let mut perintah_asing = Vec::new();

        for line in baris_logis(content) {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() { continue; }
            if line.starts_with('[') && line.ends_with(']') {
                section = line[1..line.len() - 1].trim().to_string();
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                let key = key.trim();
                let val = buang_komentar(val.trim());
                match (section.as_str(), key) {
                    ("package", "name")    => name    = unquote(val),
                    ("package", "version") => version = unquote(val),
                    ("package", "entry")   => entry   = unquote(val),
                    ("python", "packages") => python_packages = parse_string_array(val),
                    ("package", "bahasa") | ("package", "language") => bahasa = Some(unquote(val)),
                    ("package", "repositori") | ("package", "repository") => repositori = Some(unquote(val)),
                    ("perintah", k) => match kunci_perintah(k) {
                        Some(k) => { perintah.retain(|(x, _)| x != k); perintah.push((k.to_string(), daftar_perintah(val))); }
                        None => perintah_asing.push(k.to_string()),
                    },
                    ("android", "nama")      => android.nama      = Some(unquote(val)),
                    ("android", "paket")     => android.paket     = Some(unquote(val)),
                    ("android", "versi")     => android.versi     = Some(unquote(val)),
                    ("android", "kode")      => android.kode      = Some(unquote(val)),
                    ("android", "orientasi") => android.orientasi = Some(unquote(val)),
                    ("android", "ikon")      => android.ikon      = Some(root.join(unquote(val))),
                    ("android", "abi")       => android.abi       = parse_string_array(val),
                    ("android", "pustaka")   => android.pustaka   = parse_string_array(val).iter()
                        .map(|p| root.join(p).to_string_lossy().to_string()).collect(),
                    ("android", "kunci")     => android.kunci     = Some(root.join(unquote(val))),
                    ("android", "alias")     => android.alias     = Some(unquote(val)),
                    ("android", "izin")      => android.izin      = parse_string_array(val),
                    ("android", "tampilan")  => android.tampilan  = Some(unquote(val)),
                    ("android", "port")      => android.port      = Some(unquote(val)),
                    ("android", "mesin")     => android.mesin     = Some(unquote(val)),
                    ("android", "bahasa")    => android.bahasa    = parse_string_array(val),
                    ("android", "target_sdk") => android.target_sdk = Some(unquote(val)),
                    ("android", "abaikan")   => android.abaikan   = parse_string_array(val),
                    ("android", "tautan")    => android.tautan    = parse_string_array(val),
                    // Warna (`#0B1230`) tetap teks; PNG digabung relatif akar proyek.
                    ("android", "ikon_latar") => android.ikon_latar = Some({
                        let v = unquote(val);
                        if v.starts_with('#') { v } else { root.join(v).to_string_lossy().to_string() }
                    }),
                    ("android", "ikon_depan") => android.ikon_depan = Some(root.join(unquote(val))),
                    ("android.program_c", lib) => android.program_c.push((lib.to_string(),
                        parse_string_array(val).iter().map(|f| root.join(f)).collect())),
                    ("android.pustaka_c", lib) => android.pustaka_c.push((lib.to_string(),
                        parse_string_array(val).iter().map(|f| root.join(f)).collect())),
                    ("android.pustaka_rust", lib) => android.pustaka_rust.push((lib.to_string(), root.join(unquote(val)))),
                    (bagian, k) if bagian.starts_with("android.") => android.asing.push(format!("[{}] {}", bagian, k)),
                    ("android", lain)        => android.asing.push(lain.to_string()),
                    // `std` ikut di dalam jrgc (`import "std/..."`); rangka lama menulisnya di sini.
                    ("dependencies", "std") => {}
                    ("dependencies", _) => dependencies.push(dependensi(key, val, root)?),
                    ("dev-dependencies", _) => dev_dependencies.push(dependensi(key, val, root)?),
                    _ => {}
                }
            }
        }

        if entry.is_empty() {
            if let Some(r) = bahasa.as_deref().and_then(|b| super::bahasa::resep(b).ok()) { entry = r.entri.to_string(); }
        }
        if entry.is_empty() { entry = "src/main.jrg".to_string(); }

        Ok(Manifest { name, version, entry: root.join(&entry), python_packages, dependencies, dev_dependencies, repositori, android,
                      bahasa, perintah, perintah_asing })
    }

    pub(crate) fn load(project_root: &Path) -> Result<Self, String> {
        let manifest_path = project_root.join("jrgm.toml");
        let content = std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("jrgm: tidak bisa baca `{}`: {}", manifest_path.display(), jrgc::host::galat_io(&e)))?;
        Self::parse_from_str(&content, project_root)
    }
}
