//! Mode uji — satu definisi untuk "apa itu sebuah test" dan bagaimana
//! hasilnya dilaporkan.

use crate::parser::{Item, ParsedFile};

/// Apakah `nama` sebuah fungsi test?
pub fn adalah_uji(nama: &str) -> bool {
    nama.starts_with("test_") || nama.starts_with("uji_")
}

/// Nama tiap fungsi test di `file`, **urut sesuai sumbernya**.
pub fn daftar_uji(file: &ParsedFile) -> Vec<String> {
    file.items.iter().filter_map(|it| match it {
        Item::Fn(f) if adalah_uji(&f.name) => Some(f.name.clone()),
        _ => None,
    }).collect()
}

/// Letak tiap akhir baris sebuah sumber, dibangun sekali per jalan: memindai dari awal
/// untuk tiap test yang gagal membuat berkas uji besar kuadratik (20 ribu gagal: ±29 s).
pub struct IndeksBaris(Vec<usize>);

impl IndeksBaris {
    pub fn baru(src: &str) -> Self {
        IndeksBaris(src.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i).collect())
    }

    /// Nomor baris (mulai dari 1) untuk sebuah offset byte.
    pub fn baris(&self, offset: usize) -> usize {
        self.0.partition_point(|&p| p < offset) + 1
    }
}

/// Satu baris hasil untuk satu test.
pub fn baris_hasil(nama: &str, hasil: &Result<(), String>, baris: Option<usize>) -> String {
    match hasil {
        Ok(()) => format!("  ok    {}", nama),
        Err(p) => {
            let sebab = p.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
            match baris {
                Some(n) => format!("  GAGAL {} (baris {}) — {}", nama, n, sebab),
                None    => format!("  GAGAL {} — {}", nama, sebab),
            }
        }
    }
}

/// Rangkuman satu berkas. Dikembalikan sebagai teks, bukan dicetak, supaya
/// pemanggilnya yang menentukan ke mana ia pergi.
pub fn rangkuman(lulus: usize, gagal: usize) -> String {
    format!("--- {} test: {} lulus, {} gagal ---", lulus + gagal, lulus, gagal)
}

/// Kalimat yang dipakai kalau sebuah berkas dijalankan dengan `--uji` tapi
/// tidak berisi satu pun fungsi test.
pub fn tidak_ada_uji() -> String {
    "tidak ada fungsi test di berkas ini — namanya harus diawali `test_` atau `uji_`".to_string()
}
