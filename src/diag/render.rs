// src/diag/render.rs — `Diagnostic` dan perakitan tampilannya

use crate::lexer::Span;
use super::*;

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: Option<String>,
    pub message: String,
    pub span: Option<Span>,
    /// Keterangan singkat yang dicetak tepat di sebelah caret.
    pub label: Option<String>,
    pub helps: Vec<String>,
    pub notes: Vec<String>,
    /// Rantai pemanggilan menuju galat ini, terdalam lebih dulu. Kosong
    /// untuk galat kompilasi dan untuk galat runtime di kedalaman nol.
    pub jejak: Vec<BingkaiJejak>,
    /// Berapa bingkai yang tidak disimpan karena melewati [`MAKS_BINGKAI`].
    pub jejak_sisa: usize,
    /// Dari fase mana diagnostik ini lahir — cuma memengaruhi nada.
    pub fase: Fase,
}

impl Diagnostic {
    pub fn new(severity: Severity, message: impl Into<String>) -> Self {
        Diagnostic {
            severity,
            code: None,
            message: message.into(),
            span: None,
            label: None,
            helps: Vec::new(),
            notes: Vec::new(),
            jejak: Vec::new(),
            jejak_sisa: 0,
            fase: Fase::Kompilasi,
        }
    }

    /// Tandai bahwa diagnostik ini lahir saat program BERJALAN, bukan saat
    /// dikompilasi. Cuma mengubah nada galak; isinya tidak tersentuh.
    pub fn saat_jalan(mut self) -> Self {
        self.fase = Fase::Jalan;
        self
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::new(Severity::Error, message)
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, message)
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    /// Sama seperti `with_span`, tapi menerima `Option` supaya pemanggil
    /// yang belum tentu punya posisi (mis. error runtime dari nilai yang
    /// tidak melacak span) tidak perlu bercabang sendiri.
    pub fn with_span_opt(mut self, span: Option<Span>) -> Self {
        self.span = span;
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.helps.push(help.into());
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn with_jejak(mut self, jejak: Vec<BingkaiJejak>, sisa: usize) -> Self {
        self.jejak = jejak;
        self.jejak_sisa = sisa;
        self
    }

    /// Susun blok jejak pemanggilan.
    fn render_jejak(
        &self, map: &SourceMap, gutter: &str, lebar_gutter: usize,
        sian: &str, reset: &str,
    ) -> String {
        if self.jejak.is_empty() { return String::new(); }
        let mut out = String::new();
        let sambung = " ".repeat(lebar_gutter + "jejak: ".len());
        // Bingkai tanpa lokasi dibuang, di SATU tempat untuk kedua backend.
        let tampak: Vec<&BingkaiJejak> =
            self.jejak.iter().filter(|f| f.span.is_some()).collect();
        // Tanpa satu pun bingkai tampak, angka sisa dicetak sendirian tanpa
        // rantai yang menjelaskannya — lebih membingungkan daripada diam.
        if tampak.is_empty() { return String::new(); }
        for (i, fr) in tampak.iter().enumerate() {
            let posisi = match fr.span.and_then(|sp| map.locate(sp)) {
                Some(loc) => format!(
                    " dipanggil di {}:{}:{}", loc.file_name, loc.line_no, loc.col
                ),
                None => String::new(),
            };
            if i == 0 {
                out.push_str(&format!(
                    "\n{gutter}{sian}jejak:{reset} `{}`{}", fr.nama, posisi
                ));
            } else {
                out.push_str(&format!("\n{sambung}`{}`{}", fr.nama, posisi));
            }
        }
        if self.jejak_sisa > 0 {
            out.push_str(&format!(
                "\n{sambung}… dan {} panggilan lagi", self.jejak_sisa
            ));
        }
        out
    }

    /// Render lengkap.
    pub fn render(&self, map: &SourceMap) -> String {
        self.render_penuh(map, nada(), 1)
    }

    /// Sama dengan `render`, tapi nadanya disuntikkan alih-alih dibaca dari
    /// setelan global.
    pub fn render_dengan_nada(&self, map: &SourceMap, nada: Nada) -> String {
        self.render_penuh(map, nada, 1)
    }

    fn render_penuh(&self, map: &SourceMap, nada: Nada, hitungan: u32) -> String {
        self.render_berwarna(map, nada, hitungan, false)
    }

    /// Perender sesungguhnya.
    fn render_berwarna(&self, map: &SourceMap, nada: Nada, hitungan: u32, warna: bool) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let c = |kode: &'static str| if warna { kode } else { "" };
        let reset = c("\x1b[0m");
        let dim   = c("\x1b[2m");
        let bold  = c("\x1b[1m");
        // Merah untuk error, kuning untuk peringatan — caret dan label
        // memakai warna yang SAMA, supaya keduanya terbaca sebagai satu
        // hal, bukan dua.
        let utama = match self.severity {
            Severity::Error   => c("\x1b[1;31m"),
            Severity::Warning => c("\x1b[1;33m"),
            _                 => c("\x1b[1;36m"),
        };
        let biru  = c("\x1b[34m");
        let hijau = c("\x1b[1;32m");
        let sian  = c("\x1b[36m");
        let kuning = c("\x1b[33m");

        let located = self.span.and_then(|s| map.locate(s));
        let kunci = kunci_error(self.code.as_deref(), &self.message);
        if nada != Nada::Sopan {
            let baris_omelan = omelan(nada, self.fase, located.as_ref().map(|l| l.line_no), kunci, hitungan);
            if !baris_omelan.is_empty() {
                let _ = writeln!(out, "{kuning}{}{reset}", baris_omelan);
            }
        }

        // Baris judul: `error[E0012]: pesan`
        let _ = write!(out, "{utama}{}{reset}", self.severity.label());
        if let Some(code) = &self.code {
            let _ = write!(out, "{dim}[{}]{reset}", code);
        }
        let _ = write!(out, "{bold}: {}{reset}", self.message);

        if let Some(loc) = located {
            let gutter_w = loc.line_no.to_string().len();
            let pad = " ".repeat(gutter_w);

            let _ = write!(
                out,
                "\n{pad}{biru}-->{reset} {}:{}:{}",
                loc.file_name, loc.line_no, loc.col
            );
            let _ = write!(out, "\n{pad} {biru}|{reset}");
            let _ = write!(out, "\n{biru}{}{reset} {biru}|{reset} {}", loc.line_no, loc.line_text);

            // Kolom di-hitung per karakter, jadi indentasi caret juga
            // harus per karakter — bukan per byte — supaya baris yang
            // mengandung karakter non-ASCII tetap sejajar.
            let caret_pad = " ".repeat(loc.col.saturating_sub(1));
            let carets = "^".repeat(loc.caret_width);
            let _ = write!(out, "\n{pad} {biru}|{reset} {}{utama}{}{reset}", caret_pad, carets);
            if let Some(label) = &self.label {
                let _ = write!(out, " {utama}{}{reset}", label);
            }
            let _ = write!(out, "\n{pad} {biru}|{reset}");
            for help in &self.helps {
                let _ = write!(out, "\n{pad} {biru}={reset} {hijau}bantuan:{reset} {}", help);
            }
            for note in &self.notes {
                let _ = write!(out, "\n{pad} {biru}={reset} {sian}catatan:{reset} {}", note);
            }
            let gut = format!("{pad} {biru}={reset} ");
            let _ = write!(out, "{}", self.render_jejak(
                map, &gut, pad.chars().count() + 3, sian, reset));
        } else {
            for help in &self.helps {
                let _ = write!(out, "\n  {biru}={reset} {hijau}bantuan:{reset} {}", help);
            }
            for note in &self.notes {
                let _ = write!(out, "\n  {biru}={reset} {sian}catatan:{reset} {}", note);
            }
            let gut = format!("  {biru}={reset} ");
            let _ = write!(out, "{}", self.render_jejak(map, &gut, 4, sian, reset));
        }

        out
    }

    /// Cetak ke stderr.
    pub fn emit(&self, map: &SourceMap) {
        let nada = nada();
        // Hitungan pengulangan berlaku untuk SEMUA nada bersapaan, bukan
        // cuma galak — eskalasinya adalah informasi ("pesan ini tidak
        // nyampe"), dan informasi itu tidak berubah kalau nadanya diganti.
        let hitungan = if nada == Nada::Sopan {
            1
        } else {
            naikkan_hitungan(kunci_error(self.code.as_deref(), &self.message))
        };
        let warna = pakai_warna();
        let mut teks = self.render_berwarna(map, nada, hitungan, warna);

        // Tingkat paling panas menambah BANTUAN, bukan cuma volume. Kalau
        // cara menyampaikan yang sekarang sudah gagal tujuh kali,
        // mengulanginya lebih keras bukan jawaban.
        if nada != Nada::Sopan && tingkat_dari(hitungan) == Tingkat::Ngamuk {
            let kunci = kunci_error(self.code.as_deref(), &self.message);
            let pad = " ".repeat(
                self.span.and_then(|s| map.locate(s))
                    .map(|l| l.line_no.to_string().len())
                    .unwrap_or(1)
            );
            let hijau = if warna { "\x1b[1;32m" } else { "" };
            let biru  = if warna { "\x1b[34m" } else { "" };
            let reset = if warna { "\x1b[0m" } else { "" };
            teks.push_str(&format!(
                "\n{pad} {biru}={reset} {hijau}bantuan:{reset} {}", bantuan_tambahan(kunci)
            ));
        }
        crate::host::galat(&format!("{}{}\n\n", pembatas(self.severity, warna), teks));
        self.catat_galat_ke(map);
    }

    /// Cetak diagnostik ini TANPA sapaan dan tanpa menaikkan hitungan.
    pub fn emit_tanpa_sapaan(&self, map: &SourceMap) {
        let warna = pakai_warna();
        let teks = self.render_berwarna(map, Nada::Sopan, 1, warna);
        crate::host::galat(&format!("{}{}\n\n", pembatas(self.severity, warna), teks));
        self.catat_galat_ke(map);
    }

    /// `JRG_GALAT_KE=berkas`: tiap galat BERLOKASI juga ditambahkan ke berkas itu sebagai
    /// `berkas<TAB>baris<TAB>kolom<TAB>pesan`, supaya penyunting bisa melompat ke tempatnya.
    /// Gagal menulisnya ditelan — diagnostiknya sendiri sudah tercetak.
    fn catat_galat_ke(&self, map: &SourceMap) {
        if self.severity != Severity::Error { return; }
        let Some(tujuan) = std::env::var_os("JRG_GALAT_KE") else { return };
        if tujuan.is_empty() { return; }
        let Some(loc) = self.span.and_then(|s| map.locate(s)) else { return };
        let pesan = self.message.replace(['\t', '\n', '\r'], " ");
        let baris = format!("{}\t{}\t{}\t{}\n", loc.file_name, loc.line_no, loc.col, pesan);
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(&tujuan) {
            let _ = std::io::Write::write_all(&mut f, baris.as_bytes());
        }
    }
}

/// Baris ringkasan untuk batch error, mis. `error: 4 error menghentikan
/// kompilasi`.
pub(crate) fn ringkas_jumlah(n: usize) -> String {
    let warna = pakai_warna();
    let merah = if warna { "\x1b[1;31m" } else { "" };
    let tebal = if warna { "\x1b[1m" } else { "" };
    let reset = if warna { "\x1b[0m" } else { "" };
    format!("{merah}error{reset}{tebal}:{reset} {tebal}{n} error menghentikan kompilasi{reset}")
}

/// Apakah keluaran diagnostik boleh berwarna?
pub(crate) fn pakai_warna() -> bool {
    use std::io::IsTerminal;
    if std::env::var_os("NO_COLOR").is_some() { return false; }
    if matches!(std::env::var("TERM").as_deref(), Ok("dumb")) { return false; }
    std::io::stderr().is_terminal()
}

/// Sudah pernah mencetak pembatas di proses ini?
pub(crate) static SUDAH_ADA_PEMBATAS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Garis pembatas antara keluaran PROGRAM dan diagnostik.
pub(crate) fn pembatas(sev: Severity, warna: bool) -> String {
    if !warna { return String::new(); }
    let pertama = !SUDAH_ADA_PEMBATAS.swap(true, std::sync::atomic::Ordering::Relaxed);
    let lebar = lebar_terminal().min(100);
    let (kode, label) = match sev {
        Severity::Error   => ("\x1b[1;31m", " ada yang salah "),
        Severity::Warning => ("\x1b[1;33m", " perhatian "),
        _                 => ("\x1b[1;36m", " catatan "),
    };
    let dim = "\x1b[2m";
    let reset = "\x1b[0m";
    if pertama {
        let sisa = lebar.saturating_sub(label.chars().count() + 4);
        format!(
            "\n{dim}{}{reset}{kode}{}{reset}{dim}{}{reset}\n",
            "─".repeat(3), label, "─".repeat(sisa)
        )
    } else {
        format!("{dim}{}{reset}\n", "┈".repeat(lebar))
    }
}

/// Lebar terminal, kalau bisa diketahui. Bawaannya 80 — lebar yang aman
/// di mana pun, dan lebih baik terlalu pendek daripada membungkus baris.
pub(crate) fn lebar_terminal() -> usize {
    if let Ok(v) = std::env::var("COLUMNS") {
        if let Ok(n) = v.trim().parse::<usize>() {
            if n >= 20 { return n; }
        }
    }
    80
}
