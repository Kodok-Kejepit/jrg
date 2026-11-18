// src/terjemah/mod.rs — bahasa pesan JRG; pesan DITULIS Indonesia, diterjemahkan di pintu keluar lewat `t()`.
// Rancangan dan aturannya di dokumentasi/src/terjemah/mod.md.

use std::sync::atomic::{AtomicU8, Ordering};

mod en;
pub mod inti;
mod mesin;

pub use mesin::{blob_tersaring, terjemah_ke_en};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bahasa { Id, En }

impl Bahasa {
    pub fn dari_nama(s: &str) -> Option<Bahasa> {
        match s.trim().to_ascii_lowercase().as_str() {
            "id" | "indonesia" | "indonesian" | "in" => Some(Bahasa::Id),
            "en" | "english" | "inggris" => Some(Bahasa::En),
            _ => None,
        }
    }
    pub fn kode(self) -> &'static str {
        match self { Bahasa::Id => "id", Bahasa::En => "en" }
    }
}

/// 0 = belum ditentukan, 1 = Indonesia, 2 = Inggris.
static AKTIF: AtomicU8 = AtomicU8::new(0);

/// Pasang bahasa untuk sisa proses (`--bahasa`, atau penanam seperti mesin Praktik).
pub fn set(b: Bahasa) {
    AKTIF.store(if b == Bahasa::En { 2 } else { 1 }, Ordering::Relaxed);
}

/// Bahasa yang berlaku: `set` > `JRG_BAHASA` > berkas setelan `jrgm language` > Inggris.
pub fn aktif() -> Bahasa {
    match AKTIF.load(Ordering::Relaxed) {
        1 => return Bahasa::Id,
        2 => return Bahasa::En,
        _ => {}
    }
    let b = dari_lingkungan().unwrap_or(Bahasa::En);
    set(b);
    b
}

fn dari_lingkungan() -> Option<Bahasa> {
    if let Ok(v) = std::env::var("JRG_BAHASA") {
        if let Some(b) = Bahasa::dari_nama(&v) { return Some(b); }
    }
    let isi = std::fs::read_to_string(berkas_setelan()?).ok()?;
    Bahasa::dari_nama(isi.lines().next()?)
}

/// `$XDG_CONFIG_HOME/jrg/bahasa`, atau `~/.config/jrg/bahasa` (`%APPDATA%\jrg\bahasa` di Windows).
pub fn berkas_setelan() -> Option<std::path::PathBuf> {
    let dasar = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(std::path::PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(dasar.join("jrg").join("bahasa"))
}

/// Simpan pilihan bahasa untuk semua jalan berikutnya (`jrgm language en`).
pub fn simpan_setelan(b: Bahasa) -> Result<std::path::PathBuf, String> {
    let p = berkas_setelan().ok_or("tidak tahu di mana folder setelan pemakai (HOME tidak ada)")?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {}", d.display(), crate::host::galat_io(&e)))?;
    }
    std::fs::write(&p, format!("{}\n", b.kode())).map_err(|e| format!("{}: {}", p.display(), crate::host::galat_io(&e)))?;
    Ok(p)
}

/// Teks untuk ditampilkan: apa adanya dalam bahasa Indonesia, diterjemahkan kalau bahasanya Inggris.
pub fn t(teks: &str) -> String {
    match aktif() {
        Bahasa::Id => teks.to_string(),
        Bahasa::En => terjemah_ke_en(teks),
    }
}

/// Pilih salah satu dari dua teks yang ditulis langsung (label pendek di pintu keluar).
pub fn pilih<'a>(id: &'a str, en: &'a str) -> &'a str {
    if aktif() == Bahasa::En { en } else { id }
}

/// `T::parse_from` yang teks bantuan dan galat argumennya ikut bahasa pesan (`--help`, opsi salah tulis).
pub fn urai_cli<T: clap::Parser>(argumen: Vec<String>) -> T {
    for (i, a) in argumen.iter().enumerate() {
        let nilai = match a.split_once('=') {
            Some(("--bahasa" | "--language", v)) => Some(v.to_string()),
            None if a == "--bahasa" || a == "--language" => argumen.get(i + 1).cloned(),
            _ => None,
        };
        if let Some(b) = nilai.as_deref().and_then(Bahasa::dari_nama) { set(b); }
        if a == "--" { break; }
    }
    match T::try_parse_from(argumen) {
        Ok(v) => v,
        Err(e) => {
            let teks = t(&e.render().to_string());
            if e.use_stderr() { eprint!("{teks}"); } else { print!("{teks}"); }
            crate::host::keluar(e.exit_code());
        }
    }
}
