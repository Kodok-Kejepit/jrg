// src/lebar.rs — lebar TAMPIL teks di terminal: berapa kolom layar yang ditempatinya.
// Tabelnya dibangkitkan dari `unicodedata` Python (`perkakas/lebar_unicode.py`).

mod tabel;

pub use tabel::VERSI_UNICODE;

fn di_dalam(daftar: &[(u32, u32)], cp: u32) -> bool {
    daftar.binary_search_by(|(awal, akhir)| {
        if cp < *awal { std::cmp::Ordering::Greater }
        else if cp > *akhir { std::cmp::Ordering::Less }
        else { std::cmp::Ordering::Equal }
    }).is_ok()
}

/// Kolom layar satu karakter: 2 untuk huruf lebar (Asia Timur, emoji), 0 untuk tanda
/// gabung dan karakter kendali (tab dan baris baru tidak punya lebar tetap), 1 sisanya.
pub fn karakter(c: char) -> usize {
    let cp = c as u32;
    // Jalur cepat: hampir semua kode adalah ASCII yang tercetak.
    if (0x20..0x7f).contains(&cp) { return 1; }
    if cp < 0x300 { return if cp < 0x20 || (0x7f..0xa0).contains(&cp) { 0 } else { 1 }; }
    if di_dalam(tabel::NOL, cp) { 0 } else if di_dalam(tabel::DUA, cp) { 2 } else { 1 }
}

/// Jumlah kolom layar seluruh teks.
pub fn teks(s: &str) -> usize {
    s.chars().map(karakter).sum()
}

#[cfg(test)]
mod tes {
    use super::*;

    #[test]
    fn lebar_tiap_golongan() {
        assert_eq!(karakter('a'), 1);
        assert_eq!(karakter('é'), 1);
        assert_eq!(karakter('«'), 1);
        assert_eq!(karakter('日'), 2);
        assert_eq!(karakter('한'), 2);
        assert_eq!(karakter('😀'), 2);
        assert_eq!(karakter('Ａ'), 2);
        assert_eq!(karakter('\u{0301}'), 0);
        assert_eq!(karakter('\u{200d}'), 0);
        assert_eq!(karakter('\u{fe0f}'), 0);
        assert_eq!(karakter('\t'), 0);
        assert_eq!(karakter('\n'), 0);
        assert_eq!(karakter('\u{7f}'), 0);
        assert_eq!(karakter('\u{ad}'), 1);
        assert_eq!(teks("halo"), 4);
        assert_eq!(teks("日本語"), 6);
        assert_eq!(teks("e\u{0301}"), 1);
        assert_eq!(teks(""), 0);
    }

    /// Tabelnya terurut dan tidak bertindih — syarat pencarian biner. Tabel yang salah
    /// urut tidak gagal: ia menjawab lebar yang salah untuk sebagian karakter saja.
    #[test]
    fn tabel_terurut_dan_tidak_bertindih() {
        for daftar in [tabel::NOL, tabel::DUA] {
            for w in daftar.windows(2) {
                assert!(w[0].0 <= w[0].1 && w[0].1 < w[1].0, "rentang {:x?} lalu {:x?}", w[0], w[1]);
            }
        }
        for (a, b) in tabel::NOL {
            assert!(!di_dalam(tabel::DUA, *a) && !di_dalam(tabel::DUA, *b),
                "{:x}..{:x} ada di dua tabel", a, b);
        }
    }

    /// Jalur cepat di bawah U+0300 harus sepakat dengan tabelnya sendiri.
    #[test]
    fn jalur_cepat_sepakat_dengan_tabel() {
        for cp in 0..0x300u32 {
            let c = char::from_u32(cp).unwrap();
            let dari_tabel = if di_dalam(tabel::NOL, cp) { 0 }
                else if di_dalam(tabel::DUA, cp) { 2 } else { 1 };
            assert_eq!(karakter(c), dari_tabel, "U+{:04X}", cp);
        }
    }

    /// Tabelnya masih hasil pembangkitnya. Yang jadi acuan `unicodedata` Python, bukan
    /// tes ini sendiri; melewat sambil menyebut sebabnya kalau python3 atau versinya beda.
    #[test]
    fn tabel_masih_hasil_pembangkitnya() {
        let akar = env!("CARGO_MANIFEST_DIR");
        let keluar = std::process::Command::new("python3")
            .arg("perkakas/lebar_unicode.py").arg("--periksa")
            .current_dir(akar).output();
        let Ok(k) = keluar else {
            eprintln!("LEWAT: python3 tidak ada, tabel lebar tidak diperiksa terhadap unicodedata");
            return;
        };
        let teks = String::from_utf8_lossy(&k.stdout);
        match k.status.code() {
            Some(0) => {}
            Some(3) => eprintln!("LEWAT: {} (tabel dibangun dari Unicode {})", teks.trim(), VERSI_UNICODE),
            lain => panic!("tabel lebar menyimpang dari pembangkitnya ({:?}): {}{}",
                lain, teks, String::from_utf8_lossy(&k.stderr)),
        }
    }
}
