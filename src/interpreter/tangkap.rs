// src/interpreter/tangkap.rs — `tangkap(f)`: jalankan `f`, galat saat jalannya jadi `Err(Galat)`. Satu definisi rekaman
// dan kalimatnya untuk tiga mesin; `exit()` dan memori habis tidak pernah sampai ke sini. Dok: dokumentasi/src/tangkap.md.

use crate::lexer::Span;

/// Nama struct `Galat` dan urutan field-nya (sama di tiga mesin).
pub const NAMA: &str = "Galat";
pub const FIELD: [&str; 4] = ["pesan", "berkas", "baris", "kolom"];

/// Isi `Galat` untuk galat berkalimat `pesan` di `span`: kalimat pertamanya dalam bahasa pesan, dan lokasinya
/// (`""`/0/0 kalau tidak diketahui — aturan yang sama di tiga mesin).
pub struct Rekaman { pub pesan: String, pub berkas: String, pub baris: i128, pub kolom: i128 }

pub fn rekam(pesan: &str, span: Option<Span>) -> Rekaman {
    let kalimat = pesan.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let (berkas, baris, kolom) = span.and_then(crate::diag::lokasi).unwrap_or((String::new(), 0, 0));
    Rekaman { pesan: crate::terjemah::t(kalimat), berkas, baris: baris as i128, kolom: kolom as i128 }
}

/// Kalimat `panik(g)` atas sebuah `Galat`: kalimat aslinya, ditambah tempat asalnya kalau diketahui.
pub fn pesan_lempar_ulang(pesan: &str, berkas: &str, baris: i128, kolom: i128) -> String {
    if baris <= 0 { return pesan.to_string(); }
    format!("{}{}{}:{}:{}{}", pesan, CATATAN_AWAL, berkas, baris, kolom, CATATAN_AKHIR)
}

/// Potongan `pesan_lempar_ulang` di sekitar `berkas:baris:kolom` (LLVM merakitnya saat jalan dari potongan yang sama).
pub const CATATAN_AWAL: &str = "\n  catatan: galat ini ditangkap `tangkap` dari ";
pub const CATATAN_AKHIR: &str = ", lalu dilempar lagi lewat `panik`";

/// `tangkap(f)` atas fungsi yang butuh `n` argumen: salah pakai, bukan galat di dalam `f`, jadi juga TIDAK ditangkap.
pub fn pesan_butuh_argumen(n: usize) -> String {
    format!("tangkap butuh fungsi tanpa argumen, dapat fungsi yang butuh {} argumen\n  bantuan: bungkus panggilannya: \
        `tangkap(|| f(x))`", n)
}

/// `tangkap(x)` atas yang bukan fungsi. Galat ini milik pemanggilnya, jadi TIDAK ditangkap.
pub fn pesan_bukan_fungsi(tipe: &str) -> String {
    format!("tangkap butuh fungsi tanpa argumen, dapat `{}`\n  bantuan: bungkus kodenya: `tangkap(|| ...)`", tipe)
}
