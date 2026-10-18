// src/diag.rs — diag.rs — infrastruktur diagnostik JRG

use crate::lexer::{Span, SourceFile};

// ─── Severity ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Program tidak bisa dilanjutkan dengan aman.
    Error,
    /// Program bisa lanjut, tapi ada yang mencurigakan — termasuk saat
    /// interpreter memulihkan kesalahan yang maknanya tidak ambigu.
    Warning,
    /// Konteks tambahan yang membantu memahami diagnostik utama.
    Note,
    /// Saran perbaikan konkret.
    Help,
}

impl Severity {
    fn label(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "peringatan",
            Severity::Note => "catatan",
            Severity::Help => "bantuan",
        }
    }
}

// ─── Berkas modul yang dimuat saat program berjalan ─────────────────────────

/// Berkas yang baru diketahui SAAT program berjalan — yaitu modul yang
/// di-`import`.
pub(crate) static BERKAS_MODUL: std::sync::OnceLock<std::sync::Mutex<Vec<SourceFile>>> =
    std::sync::OnceLock::new();

pub(crate) fn berkas_modul() -> &'static std::sync::Mutex<Vec<SourceFile>> {
    BERKAS_MODUL.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// Daftarkan sumber sebuah modul dan dapatkan `file_id` khusus untuknya.
///
/// Id 0 dipakai berkas utama, jadi penomoran di sini mulai dari 1.
pub fn register_module_source(name: String, src: String) -> u32 {
    let mut daftar = berkas_modul().lock().unwrap_or_else(|p| p.into_inner());
    let id = daftar.len() as u32 + 1;
    daftar.push(SourceFile { id, name, src });
    id
}

/// Salinan sumber modul ber-`id`, kalau pernah didaftarkan.
pub(crate) fn sumber_modul(id: u32) -> Option<SourceFile> {
    let daftar = berkas_modul().lock().unwrap_or_else(|p| p.into_inner());
    daftar.iter().find(|f| f.id == id).cloned()
}

// ─── SourceMap ──────────────────────────────────────────────────────────────

/// Cukup untuk menerjemahkan `Span` jadi baris/kolom dan mengambil teks
/// barisnya. Dipegang oleh pemanggil (main.rs / jrgm) dan dipinjamkan ke
/// perender.
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> Self {
        SourceMap { files: Vec::new() }
    }

    pub fn add(&mut self, file: SourceFile) {
        self.files.push(file);
    }

    pub fn with_file(file: SourceFile) -> Self {
        SourceMap { files: vec![file] }
    }

    fn file(&self, id: u32) -> Option<&SourceFile> {
        self.files.iter().find(|f| f.id == id)
    }

    /// Pastikan sumber modul ber-`id` ikut termuat, kalau memang ada.
    pub fn ensure_module(&mut self, id: u32) {
        if id != 0 && self.file(id).is_none() {
            if let Some(sf) = sumber_modul(id) {
                self.files.push(sf);
            }
        }
    }

    /// Baris (1-based), kolom (1-based, dihitung per karakter bukan per
    /// byte supaya sumber ber-Unicode tetap menunjuk ke tempat yang benar),
    /// dan teks baris itu tanpa newline.
    fn locate(&self, span: Span) -> Option<Located<'_>> {
        let f = self.file(span.file_id)?;
        let src = f.src.as_str();
        let lo = (span.lo.0 as usize).min(src.len());
        let hi = (span.hi.0 as usize).min(src.len()).max(lo);

        // Awal baris = setelah '\n' terakhir sebelum `lo`.
        let line_start = src[..lo].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line_end = src[lo..].find('\n').map(|i| lo + i).unwrap_or(src.len());
        let line_no = src[..line_start].bytes().filter(|b| *b == b'\n').count() + 1;
        let col = src[line_start..lo].chars().count() + 1;
        // Lebar caret dibatasi ke ujung baris: span yang membentang
        // beberapa baris (mis. blok yang tidak ditutup) hanya ditandai
        // pada baris pertamanya, karena itu yang paling informatif.
        let caret_end = hi.min(line_end);
        let width = src[lo..caret_end].chars().count().max(1);

        Some(Located {
            file_name: f.name.as_str(),
            line_no,
            col,
            line_text: &src[line_start..line_end],
            caret_width: width,
        })
    }
}

impl Default for SourceMap {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) struct Located<'a> {
    file_name: &'a str,
    line_no: usize,
    col: usize,
    line_text: &'a str,
    caret_width: usize,
}

/// Satu bingkai jejak pemanggilan.
#[derive(Debug, Clone, PartialEq)]
pub struct BingkaiJejak {
    pub nama: String,
    pub span: Option<Span>,
}

impl BingkaiJejak {
    pub fn baru(nama: impl Into<String>, span: Option<Span>) -> Self {
        BingkaiJejak { nama: nama.into(), span }
    }
}

/// Batas banyak bingkai yang DISIMPAN, bukan cuma yang dicetak.
pub const MAKS_BINGKAI: usize = 10;

// ─── Diagnostic ─────────────────────────────────────────────────────────────

/// Satu pesan lengkap untuk programmer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fase {
    /// Lexer dan parser — ketahuan sebelum program jalan.
    Kompilasi,
    /// Saat program berjalan.
    Jalan,
}

/// Nada penyampaian diagnostik. Isi pesannya sama persis di kedua nada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nada {
    /// Datar: langsung ke pokok masalah, tanpa sapaan sama sekali.
    Sopan,
    /// **Bawaan JRG.** Teman yang rese: menggoda, lalu mengajak.
    Galak,
    /// Ketus dan pendek. Tidak basa-basi, tapi tidak pernah menghina.
    Judes,
    /// Menenangkan. Untuk yang baru belajar, atau yang sudah lelah.
    Hangat,
    /// Tenang dan minim. Menyebut fakta, tanpa dramatisasi.
    Cool,
    /// Bercanda — TAPI tidak pada kemunculan pertama. Lihat aturan 1 di
    /// tabel sapaan: saat orang pertama kali menabrak error, lelucon
    /// menunda satu-satunya hal yang dia butuhkan.
    Lawak,
}

impl Nada {
    /// Nama yang dipakai di CLI dan variabel lingkungan.
    pub fn dari_nama(s: &str) -> Option<Nada> {
        match s.trim().to_ascii_lowercase().as_str() {
            "sopan"  => Some(Nada::Sopan),
            "galak"  => Some(Nada::Galak),
            "judes"  => Some(Nada::Judes),
            "hangat" => Some(Nada::Hangat),
            "cool"   => Some(Nada::Cool),
            "lawak"  => Some(Nada::Lawak),
            _ => None,
        }
    }
    pub fn nama(self) -> &'static str {
        match self {
            Nada::Sopan => "sopan", Nada::Galak => "galak", Nada::Judes => "judes",
            Nada::Hangat => "hangat", Nada::Cool => "cool", Nada::Lawak => "lawak",
        }
    }
    /// Semua nama yang sah — dipakai pesan error CLI supaya daftarnya
    /// punya satu sumber, bukan disalin ke tiap tempat yang menyebutnya.
    pub const SEMUA: [&'static str; 6] = ["sopan", "galak", "judes", "hangat", "cool", "lawak"];
}

/// Nada bawaan JRG, dan ini keputusan desain — bukan setelan yang kebetulan
/// begini.
pub(crate) const NADA_BAWAAN: Nada = Nada::Galak;

/// Nada yang dipaksa lewat CLI.
pub(crate) static PAKSA: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(u8::MAX);

pub(crate) fn kode_nada(n: Nada) -> u8 {
    match n {
        Nada::Sopan => 0, Nada::Galak => 1, Nada::Judes => 2,
        Nada::Hangat => 3, Nada::Cool => 4, Nada::Lawak => 5,
    }
}
pub(crate) fn nada_dari_kode(k: u8) -> Option<Nada> {
    Some(match k {
        0 => Nada::Sopan, 1 => Nada::Galak, 2 => Nada::Judes,
        3 => Nada::Hangat, 4 => Nada::Cool, 5 => Nada::Lawak,
        _ => return None,
    })
}

/// Setel nada global. Dipanggil sekali dari `main` sesudah argumen dibaca.
pub fn set_nada(n: Nada) {
    PAKSA.store(kode_nada(n), std::sync::atomic::Ordering::Relaxed);
}

/// Nada yang berlaku.
pub fn nada() -> Nada {
    use std::sync::atomic::Ordering::Relaxed;
    if let Some(n) = nada_dari_kode(PAKSA.load(Relaxed)) { return n; }
    // `JRG_NADA=hangat` — satu variabel untuk semua nada.
    if let Ok(v) = std::env::var("JRG_NADA") {
        if let Some(n) = Nada::dari_nama(&v) { return n; }
    }
    // Dua variabel lama tetap dihormati: mereka sudah dipakai skrip dan
    // CI, dan mematikannya tanpa perlu cuma memindahkan kerja ke orang
    // lain.
    let aktif = |k: &str| matches!(std::env::var(k), Ok(v) if v == "1" || v.eq_ignore_ascii_case("true"));
    if aktif("JRG_SOPAN") { return Nada::Sopan; }
    if aktif("JRG_GALAK") { return Nada::Galak; }
    NADA_BAWAAN
}

// Sisanya dipecah ke empat berkas; `pub use` menjaga jalur `diag::X`
// tetap persis seperti sebelum dipecah.
mod nada;
mod render;
mod kode;
pub use kode::codes;
mod lapor;

pub(crate) use nada::*;
pub use render::*;
pub use lapor::*;
