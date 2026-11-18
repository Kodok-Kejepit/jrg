// src/terjemah/mesin.rs — tabel `en.rs` di atas pencocok `inti.rs`: pola "teks {} teks" dicari DI DALAM pesan.
// Rinciannya di dokumentasi/src/terjemah/mod.md.

use std::sync::OnceLock;

use super::inti::{self, Pola};

pub(crate) fn pola() -> &'static [Pola<'static>] {
    static P: OnceLock<Vec<Pola<'static>>> = OnceLock::new();
    P.get_or_init(|| inti::siapkan(super::en::TABEL.iter().copied()))
}

/// Terjemahkan `teks` sejauh tabel mengenalnya (`inti::terjemah`).
pub fn terjemah_ke_en(teks: &str) -> String {
    inti::terjemah(pola(), super::en::KATA, teks)
}

/// Tabel untuk biner `--emit-llvm` (`inti::urai_blob`): cuma pola yang `pakai` terima, ditambah daftar kata utuh.
pub fn blob_tersaring(pakai: impl Fn(&str) -> bool) -> String {
    let mut out = String::new();
    for (id, en) in super::en::TABEL {
        if pakai(id) {
            if !out.is_empty() { out.push('\u{1E}'); }
            out.push_str(id);
            out.push('\u{1F}');
            out.push_str(en);
        }
    }
    out.push('\u{1D}');
    let kata: Vec<String> = super::en::KATA.iter().map(|(id, en)| format!("{id}\u{1F}{en}")).collect();
    out.push_str(&kata.join("\u{1E}"));
    out
}

#[cfg(test)]
pub(crate) fn urai_en_uji(en: &'static str, n: usize) -> (Vec<&'static str>, Vec<usize>) { inti::urai_en(en, n) }

#[cfg(test)]
mod tests {
    use super::terjemah_ke_en;

    #[test]
    fn pola_ditemukan_di_dalam_pesan_dan_isinya_ikut() {
        assert_eq!(terjemah_ke_en("`f` dipanggil di a.jrg:3:1"), "`f` called at a.jrg:3:1");
        assert_eq!(terjemah_ke_en("… dan 4 panggilan lagi"), "… and 4 more calls");
        assert_eq!(terjemah_ke_en("teks yang tidak dikenal tabel"), "teks yang tidak dikenal tabel");
        assert_eq!(terjemah_ke_en(""), "");
        assert_eq!(terjemah_ke_en("[jrgm] 1 test di 1 berkas"), "[jrgm] 1 test in 1 file");
        assert_eq!(terjemah_ke_en("[jrgm] 11 test di 21 berkas"), "[jrgm] 11 tests in 21 files");
    }
}
