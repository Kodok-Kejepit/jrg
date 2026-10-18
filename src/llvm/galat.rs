// src/llvm/galat.rs — llvm/galat.rs — error backend LLVM yang membawa lokasi.

use crate::lexer::{BytePos, Span};

/// Tempelkan penanda lokasi ke sebuah pesan error.
pub fn tandai(sp: Span, msg: impl AsRef<str>) -> String {
    // Span TERBALIK tidak ditandai sama sekali.
    if sp.hi.0 < sp.lo.0 {
        return msg.as_ref().to_string();
    }
    format!("[{}:{}] {}", sp.lo.0, sp.hi.0, msg.as_ref())
}

/// Lekatkan nomor berkas modul ke penanda `[lo:hi]` yang belum punya.
pub fn beri_berkas(pesan: String, berkas: u32) -> String {
    if berkas == 0 || !pesan.starts_with('[') { return pesan; }
    match pesan.find(']') {
        Some(t) if !pesan[..t].contains('@') && pesan[1..t].contains(':') =>
            format!("{}@{}{}", &pesan[..t], berkas, &pesan[t..]),
        _ => pesan,
    }
}

/// Apakah pesan ini sudah membawa penanda lokasi?
fn sudah_bertanda(pesan: &str) -> bool {
    GalatLlvm::dari_pesan(pesan).span.is_some()
}

/// Beri lokasi ke sebuah error HANYA kalau ia belum punya.
pub fn beri_span_bila_kosong(pesan: String, sp: Span) -> String {
    if sudah_bertanda(&pesan) { return pesan; }
    // Penanda KOSONG (`[0:0]`, dari jalur yang tidak membawa span) dilepas dulu,
    // supaya yang tertinggal di pesannya bukan dua penanda.
    tandai(sp, GalatLlvm::dari_pesan(&pesan).msg)
}

/// Jelaskan `ns::item` yang tidak ditemukan backend LLVM.
pub fn jelaskan_path_hilang(ns: &str, item: &str, di_import: bool) -> String {
    let penuh = format!("{}::{}", ns, item);

    // `py::` dan `js::` cuma ada di interpreter: pekerjanya harus memanggil balik fungsi JRG
    // di tengah permintaan. Diperiksa DULU, karena registry juga memuat nama keduanya.
    if matches!(ns, "py" | "js") {
        let apa = if ns == "py" { "interop Python" } else { "interop JavaScript" };
        return format!(
            "namespace `{}::` ({}) cuma ada di interpreter, karena pekerjanya harus memanggil \
             balik fungsi JRG di tengah permintaan\n\
             bantuan: jalankan tanpa `--emit-llvm` dan tanpa `--vm`\n\
             catatan: `{}` memang ada di sana; yang tidak ada adalah dukungannya di backend ini",
            ns, apa, penuh
        );
    }

    // "Ada" diturunkan dari registry yang sedang berjalan — termasuk HOF seperti `arr::map`,
    // yang bukan `def` dan dulu dijawab "tidak ditemukan ... periksa ejaan".
    if crate::interpreter::builtin::jembatan_stdlib::ada_di_stdlib(&penuh) || crate::vm::ada_builtin(&penuh) {
        return format!(
            "`{}` ada di pustaka standar (namespace `{}::`), tapi backend `--emit-llvm` belum \
             mengimplementasikannya\n\
             bantuan: jalankan tanpa `--emit-llvm` (interpreter) atau dengan `--vm`\n\
             catatan: ini batas cakupan backend, bukan salah tulis nama — `{}` memang ada",
            penuh, ns, penuh
        );
    }

    // Berkas ini benar-benar meng-`import` namespace itu, jadi jawabannya PASTI
    // dan tidak perlu ditawar.
    if di_import {
        // Yang PASTI di sini cuma soal namespace-nya, dan pesannya berhenti persis
        // di situ.
        return format!(
            "namespace `{}::` berasal dari modul yang di-`import` berkas ini, dan \
             backend `--emit-llvm` tidak memuat modul antar-berkas — jadi `{}` \
             tidak bisa dipanggil dari sini\n\
             bantuan: jalankan tanpa `--emit-llvm` (interpreter) atau dengan `--vm` — \
             keduanya memuat modulnya\n\
             catatan: ini batas cakupan backend; apakah `{}` sendiri ada di modul \
             itu belum diperiksa, karena modulnya memang tidak dibaca",
            ns, penuh, item
        );
    }

    // Bukan stdlib, dan namespace-nya tidak di-`import` berkas ini.
    format!(
        "`{}` tidak ditemukan\n\
         bantuan: kalau ini dari berkas lain, `import` modulnya dulu di berkas ini\n\
         catatan: kalau `{}` mestinya ada di berkas ini, periksa ejaan nama \
         namespace-nya",
        penuh, penuh
    )
}

#[derive(Debug, Clone)]
pub struct GalatLlvm {
    pub msg: String,
    pub span: Option<Span>,
}

impl GalatLlvm {
    /// Ambil sebuah pesan codegen, pisahkan penanda `[lo:hi]` kalau ada.
    pub fn dari_pesan(pesan: &str) -> GalatLlvm {
        // Label tipe `ruang_tipe` tidak pernah sampai ke pembaca; ini satu-satunya pintu keluar.
        let mut g = Self::dari_pesan_mentah(pesan);
        g.msg = crate::ruang_tipe::bersihkan(g.msg);
        g
    }

    fn dari_pesan_mentah(pesan: &str) -> GalatLlvm {
        if let Some(sisa) = pesan.strip_prefix('[') {
            if let Some(tutup) = sisa.find(']') {
                let dalam = &sisa[..tutup];
                if let Some((a, b)) = dalam.split_once(':') {
                    // `[lo:hi@berkas]`: galat dari dalam modul yang di-`import` (Fase MODUL-LLVM).
                    let (b, berkas) = match b.split_once('@') {
                        Some((b, f)) => (b, f.trim().parse::<u32>().unwrap_or(0)),
                        None => (b, 0),
                    };
                    if let (Ok(lo), Ok(hi)) = (a.trim().parse::<u32>(), b.trim().parse::<u32>()) {
                        // `hi >= lo` dijaga: span terbalik akan membuat lebar caret negatif dan
                        // perendernya menjepitnya jadi 1 — lokasi yang menyesatkan.
                        if lo == 0 && hi == 0 {
                            return GalatLlvm { msg: sisa[tutup + 1..].trim_start().to_string(), span: None };
                        }
                        if hi >= lo {
                            return GalatLlvm {
                                msg: sisa[tutup + 1..].trim_start().to_string(),
                                span: Some(Span::new(BytePos(lo), BytePos(hi), berkas)),
                            };
                        }
                    }
                }
            }
        }
        GalatLlvm { msg: pesan.to_string(), span: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn penanda_dipisah_dari_pesan() {
        let g = GalatLlvm::dari_pesan("[926:937] `jawa::alloc` tidak ditemukan");
        assert_eq!(g.msg, "`jawa::alloc` tidak ditemukan");
        let sp = g.span.expect("span harus terbaca");
        assert_eq!((sp.lo.0, sp.hi.0), (926, 937));
    }

    /// Pesan tanpa penanda tetap utuh, dan TIDAK mengarang lokasi.
    #[test]
    fn tanpa_penanda_tidak_mengarang_lokasi() {
        for pesan in [
            "closure belum didukung backend LLVM v1",
            "perbandingan `==` tidak didukung untuk tipe `[2]i64`",
            "tipe `[N]T` butuh panjang",
        ] {
            let g = GalatLlvm::dari_pesan(pesan);
            assert_eq!(g.msg, pesan, "pesan tanpa penanda tidak boleh dipotong");
            assert!(g.span.is_none(), "tidak boleh mengarang span untuk: {}", pesan);
        }
    }

    /// Span terbalik tidak pernah jadi penanda.
    #[test]
    fn span_terbalik_tidak_ditandai() {
        let terbalik = Span::new(BytePos(85), BytePos(84), 0);
        let hasil = tandai(terbalik, "ada yang salah");
        assert_eq!(hasil, "ada yang salah", "span terbalik tidak boleh jadi penanda");
        assert!(GalatLlvm::dari_pesan(&hasil).span.is_none());

        // Dan yang WAJAR tetap ditandai seperti biasa.
        let wajar = Span::new(BytePos(10), BytePos(20), 0);
        assert_eq!(tandai(wajar, "x"), "[10:20] x");
    }

    /// Bentuk yang MIRIP penanda tapi bukan — tidak boleh dipotong.
    #[test]
    fn bentuk_mirip_penanda_tidak_ikut_terpotong() {
        for pesan in [
            "[2]i64 tidak bisa dibandingkan",
            "[abc:def] bukan angka",
            "[12] cuma satu angka",
            "[] kosong",
            "[9:3] terbalik",
        ] {
            let g = GalatLlvm::dari_pesan(pesan);
            assert_eq!(g.msg, pesan, "pesan `{}` tidak boleh dipotong", pesan);
            assert!(g.span.is_none(), "pesan `{}` tidak boleh dianggap punya span", pesan);
        }
    }

    /// Penanda KOSONG adalah lokasi yang hilang, bukan baris 1 kolom 1 — dan ia
    /// diganti, bukan ditumpuk, oleh span pembungkusnya.
    #[test]
    fn penanda_kosong_diganti_span_pembungkus() {
        let g = GalatLlvm::dari_pesan("[0:0] mencetak tipe `()`");
        assert!(g.span.is_none(), "[0:0] dibaca sebagai lokasi");
        assert_eq!(g.msg, "mencetak tipe `()`");
        let sp = Span::new(BytePos(40), BytePos(52), 0);
        assert_eq!(beri_span_bila_kosong("[0:0] x".to_string(), sp), "[40:52] x");
        // Lokasi yang NYATA tidak diganti.
        assert_eq!(beri_span_bila_kosong("[3:9] x".to_string(), sp), "[3:9] x");
    }
}

#[cfg(test)]
mod tes_path_hilang {
    /// Tiap nama berpath di registry harus diakui ADA — dulu 40 HOF seperti `arr::map`
    /// dijawab "tidak ditemukan ... periksa ejaan", kalimat yang salah tentang bahasanya.
    #[test]
    fn nama_terdaftar_tidak_dituduh_hilang() {
        let mut salah = Vec::new();
        let mut diperiksa = 0;
        for nama in crate::vm::daftar_builtin() {
            let Some((ns, item)) = nama.split_once("::") else { continue };
            diperiksa += 1;
            let pesan = super::jelaskan_path_hilang(ns, item, false);
            if pesan.contains("tidak ditemukan") || pesan.contains("periksa ejaan") {
                salah.push(nama.clone());
            }
        }
        assert!(diperiksa > 400, "cuma {} nama berpath yang diperiksa", diperiksa);
        assert!(salah.is_empty(), "{} nama nyata dituduh hilang: {:?}", salah.len(), salah);
    }
}
