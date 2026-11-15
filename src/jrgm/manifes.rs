// src/jrgm/manifes.rs — manifest `jrgm.toml` dan pembacanya

use std::path::{Path, PathBuf};

use crate::stdlib::toml::{self, Simpul, Tabel};

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

/// Nama dependensi jadi bagian jalur tembolok, jadi `../` atau `/` di sana menulis di luarnya.
pub(crate) fn periksa_nama_dependensi(nama: &str) -> Result<(), String> {
    let sah = !nama.is_empty() && !nama.starts_with('-') && nama.len() <= 64
        && nama.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if sah { return Ok(()); }
    Err(format!("nama dependensi {:?} tidak sah — cuma huruf, angka, `_` dan `-`, tidak diawali `-`, \
        paling panjang 64", nama))
}

/// URL dan rev diserahkan ke `git`: yang diawali `-` akan dibacanya sebagai opsi (`--upload-pack=..`).
pub(crate) fn periksa_git(nama: &str, url: &str, rev: &str) -> Result<(), String> {
    let kendali = |s: &str| s.chars().any(|c| c.is_control());
    if url.starts_with('-') || kendali(url) || url.trim() != url {
        return Err(format!("dependensi `{}`: url git {:?} tidak sah — tidak boleh diawali `-` atau \
            memuat spasi tepi/karakter kendali, karena git akan membacanya sebagai opsi", nama, url));
    }
    if rev.starts_with('-') || kendali(rev) || rev.contains(char::is_whitespace) || rev.contains("..") {
        return Err(format!("dependensi `{}`: rev {:?} tidak sah — cabang, tag atau commit tidak \
            diawali `-` dan tanpa spasi atau `..`", nama, rev));
    }
    Ok(())
}

fn jenis(v: &Simpul) -> &'static str {
    match v {
        Simpul::Tabel(_) => "tabel", Simpul::Larik { .. } => "array", Simpul::Teks(_) => "teks",
        Simpul::Bulat(_) => "bilangan bulat", Simpul::Pecahan(_) => "pecahan", Simpul::Benar(_) => "bool",
        Simpul::Waktu(_) => "tanggal",
    }
}

/// Teks, atau bilangan bulat yang ditulis tanpa kutip (`kode = 3`); jenis lain ditolak menyebut kuncinya.
fn teks(v: &Simpul, kunci: &str) -> Result<String, String> {
    match v {
        Simpul::Teks(s) => Ok(s.clone()),
        Simpul::Bulat(n) => Ok(n.to_string()),
        _ => Err(format!("`{}` harus teks, dapat {}", kunci, jenis(v))),
    }
}

/// Array teks; satu teks dibaca sebagai array satu elemen.
fn larik_teks(v: &Simpul, kunci: &str) -> Result<Vec<String>, String> {
    match v {
        Simpul::Teks(s) => Ok(vec![s.clone()]),
        Simpul::Larik { isi, .. } => isi.iter().enumerate()
            .map(|(i, x)| match x {
                Simpul::Teks(s) => Ok(s.clone()),
                _ => Err(format!("`{}`[{}] harus teks, dapat {}", kunci, i, jenis(x))),
            })
            .collect(),
        _ => Err(format!("`{}` harus array teks, dapat {}", kunci, jenis(v))),
    }
}

fn tabel<'a>(v: &'a Simpul, kunci: &str) -> Result<&'a Tabel, String> {
    match v { Simpul::Tabel(t) => Ok(t), _ => Err(format!("`[{}]` harus tabel, dapat {}", kunci, jenis(v))) }
}

/// Satu entri `[dependencies]`: `{ path = .. }` atau `{ git = .., rev = .. | version = .. }`.
fn dependensi(nama: &str, val: &Simpul, root: &Path) -> Result<Dependensi, String> {
    periksa_nama_dependensi(nama)?;
    let bantuan = "bantuan: `{ path = \"../x\" }` atau `{ git = \"url\", version = \"0.1.0\" }` — registry \
        belum ada, jadi versi saja tidak cukup";
    let Simpul::Tabel(t) = val else {
        return Err(format!("dependensi `{}` harus tabel, dapat {}\n  {}", nama, jenis(val), bantuan));
    };
    let ambil = |k: &str| t.isi.get(k).map(|v| teks(v, &format!("{}.{}", nama, k))).transpose();
    let (path, git, rev, versi) = (ambil("path")?, ambil("git")?, ambil("rev")?, ambil("version")?);
    if let Some(asing) = t.isi.keys().find(|k| !["path", "git", "rev", "version"].contains(&k.as_str())) {
        return Err(format!("dependensi `{}`: kunci `{}` tidak dikenal\n  {}", nama, asing, bantuan));
    }
    match (path, git) {
        (Some(_), Some(_)) => Err(format!("dependensi `{}` menulis `path` dan `git` sekaligus — pilih satu", nama)),
        (Some(p), None) if rev.is_some() || versi.is_some() =>
            Err(format!("dependensi `{}` = {{ path = {:?} }} tidak memakai `rev`/`version`", nama, p)),
        (Some(p), None) if !p.is_empty() => Ok(Dependensi { nama: nama.to_string(), akar: root.join(p), git: None }),
        (None, Some(url)) if !url.is_empty() => {
            if rev.is_some() && versi.is_some() {
                return Err(format!("dependensi `{}`: pilih salah satu, `version` atau `rev`", nama));
            }
            let rev = match (rev, versi) {
                (Some(r), _) => r,
                (None, Some(v)) => super::git::rev_versi(&v).map_err(|e| format!("dependensi `{}`: {}", nama, e))?,
                (None, None) => String::new(),
            };
            periksa_git(nama, &url, &rev)?;
            Ok(Dependensi { nama: nama.to_string(), akar: PathBuf::new(), git: Some((url, rev)) })
        }
        _ => Err(format!("dependensi `{}` tidak dimengerti\n  {}", nama, bantuan)),
    }
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

fn android(t: &Tabel, root: &Path) -> Result<Android, String> {
    let mut a = Android::default();
    let jalur = |v: &Simpul, k: &str| teks(v, k).map(|s| root.join(s));
    for (k, v) in &t.isi {
        let kn = format!("android.{}", k);
        let kn = kn.as_str();
        match k.as_str() {
            "nama" => a.nama = Some(teks(v, kn)?),
            "paket" => a.paket = Some(teks(v, kn)?),
            "versi" => a.versi = Some(teks(v, kn)?),
            "kode" => a.kode = Some(teks(v, kn)?),
            "orientasi" => a.orientasi = Some(teks(v, kn)?),
            "ikon" => a.ikon = Some(jalur(v, kn)?),
            "abi" => a.abi = larik_teks(v, kn)?,
            "pustaka" => a.pustaka = larik_teks(v, kn)?.iter().map(|p| root.join(p).to_string_lossy().to_string()).collect(),
            "kunci" => a.kunci = Some(jalur(v, kn)?),
            "alias" => a.alias = Some(teks(v, kn)?),
            "izin" => a.izin = larik_teks(v, kn)?,
            "tampilan" => a.tampilan = Some(teks(v, kn)?),
            "port" => a.port = Some(teks(v, kn)?),
            "mesin" => a.mesin = Some(teks(v, kn)?),
            "bahasa" => a.bahasa = larik_teks(v, kn)?,
            "target_sdk" => a.target_sdk = Some(teks(v, kn)?),
            "abaikan" => a.abaikan = larik_teks(v, kn)?,
            "tautan" => a.tautan = larik_teks(v, kn)?,
            // Warna (`#0B1230`) tetap teks; PNG digabung relatif akar proyek.
            "ikon_latar" => a.ikon_latar = Some({
                let s = teks(v, kn)?;
                if s.starts_with('#') { s } else { root.join(s).to_string_lossy().to_string() }
            }),
            "ikon_depan" => a.ikon_depan = Some(jalur(v, kn)?),
            "program_c" | "pustaka_c" => for (lib, isi) in &tabel(v, kn)?.isi {
                let berkas = larik_teks(isi, &format!("{}.{}", kn, lib))?.iter().map(|f| root.join(f)).collect();
                if k == "program_c" { a.program_c.push((lib.clone(), berkas)) } else { a.pustaka_c.push((lib.clone(), berkas)) }
            },
            "pustaka_rust" => for (lib, isi) in &tabel(v, kn)?.isi {
                a.pustaka_rust.push((lib.clone(), jalur(isi, &format!("{}.{}", kn, lib))?));
            },
            lain => match v {
                Simpul::Tabel(sub) => a.asing.extend(sub.isi.keys().map(|x| format!("[android.{}] {}", lain, x))),
                _ => a.asing.push(lain.to_string()),
            },
        }
    }
    Ok(a)
}

impl Manifest {
    /// Diurai `toml::urai` — pengurai TOML 1.0 yang sama dengan `toml::parse` — jadi kunci kembar, tabel yang
    /// didefinisikan ulang dan nilai yang tak berkutip jadi galat berbaris, bukan dibaca lain diam-diam.
    pub(crate) fn parse_from_str(content: &str, root: &Path) -> Result<Self, String> {
        let akar = match toml::urai(content).map_err(|e| format!("jrgm.toml: {}", e))? {
            Simpul::Tabel(t) => t,
            _ => unreachable!("akar dokumen TOML selalu tabel"),
        };
        let galat = |e: String| format!("jrgm.toml: {}", e);
        let mut m = Manifest {
            name: "unnamed".into(), version: "0.0.0".into(), entry: PathBuf::new(), python_packages: Vec::new(),
            dependencies: Vec::new(), dev_dependencies: Vec::new(), repositori: None, android: Android::default(),
            bahasa: None, perintah: Vec::new(), perintah_asing: Vec::new(),
        };
        let mut entry = String::new();
        for (bagian, isi) in &akar.isi {
            let Simpul::Tabel(t) = isi else { continue };
            for (k, v) in &t.isi {
                let kn = format!("{}.{}", bagian, k);
                match (bagian.as_str(), k.as_str()) {
                    ("package", "name") => m.name = teks(v, &kn).map_err(galat)?,
                    ("package", "version") => m.version = teks(v, &kn).map_err(galat)?,
                    ("package", "entry") => entry = teks(v, &kn).map_err(galat)?,
                    ("package", "bahasa" | "language") => m.bahasa = Some(teks(v, &kn).map_err(galat)?),
                    ("package", "repositori" | "repository") => m.repositori = Some(teks(v, &kn).map_err(galat)?),
                    ("python", "packages") => m.python_packages = larik_teks(v, &kn).map_err(galat)?,
                    ("perintah", k) => match kunci_perintah(k) {
                        Some(kk) => {
                            let langkah = larik_teks(v, &kn).map_err(galat)?;
                            m.perintah.retain(|(x, _)| x != kk);
                            m.perintah.push((kk.to_string(), langkah));
                        }
                        None => m.perintah_asing.push(k.to_string()),
                    },
                    // `std` ikut di dalam jrgc (`import "std/..."`); rangka lama menulisnya di sini.
                    ("dependencies", "std") => {}
                    ("dependencies", _) => m.dependencies.push(dependensi(k, v, root)?),
                    ("dev-dependencies", _) => m.dev_dependencies.push(dependensi(k, v, root)?),
                    _ => {}
                }
            }
            if bagian == "android" { m.android = android(t, root).map_err(galat)?; }
        }
        if entry.is_empty() {
            if let Some(r) = m.bahasa.as_deref().and_then(|b| super::bahasa::resep(b).ok()) { entry = r.entri.to_string(); }
        }
        if entry.is_empty() { entry = "src/main.jrg".to_string(); }
        m.entry = root.join(&entry);
        Ok(m)
    }

    pub(crate) fn load(project_root: &Path) -> Result<Self, String> {
        let manifest_path = project_root.join("jrgm.toml");
        let content = std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("jrgm: tidak bisa baca `{}`: {}", manifest_path.display(), crate::host::galat_io(&e)))?;
        Self::parse_from_str(&content, project_root)
    }
}
