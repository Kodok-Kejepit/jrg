// src/interpreter/stdlib_src.rs — interpreter/stdlib_src.rs

/// Nama modul (tanpa awalan `std/`) dan isi sumbernya.
pub const STDLIB_MODULES: &[(&str, &str)] = &[
    ("option",   include_str!("../../stdlib/option.jrg")),
    ("result",   include_str!("../../stdlib/result.jrg")),
    ("iterator", include_str!("../../stdlib/iterator.jrg")),
    ("testing",  include_str!("../../stdlib/testing.jrg")),
    // Wadah yang belum ada sebagai builtin.
    ("set",      include_str!("../../stdlib/set.jrg")),
    ("queue",    include_str!("../../stdlib/queue.jrg")),
    ("deque",    include_str!("../../stdlib/deque.jrg")),
    ("heap",     include_str!("../../stdlib/heap.jrg")),
    ("csv",      include_str!("../../stdlib/csv.jrg")),
    // Path sebagai TEKS (`fs::` cuma punya basename/dirname/join), dan perkakas
    // fungsional yang tidak bisa jadi builtin sama sekali: sebuah builtin VM …
    ("path",     include_str!("../../stdlib/path.jrg")),
    ("functional", include_str!("../../stdlib/functional.jrg")),
    // Satu pembungkus untuk array DAN teks: bagi yang baru mulai keduanya
    // sama-sama "daftar isi", dan dua nama untuk satu gagasan adalah dua hal
    // yang harus dihafal.
    ("teks",     include_str!("../../stdlib/teks.jrg")),
    // Uang sebagai bilangan BULAT dari satuan terkecil: seribu transaksi
    // Rp 0,10 dijumlah dengan float menghasilkan 99.9999999999986.
    ("uang",     include_str!("../../stdlib/uang.jrg")),
    // Lebar kolom dihitung dari isi TERLEBAR: menebaknya membuat kolom
    // menempel begitu ada yang lebih panjang dari tebakan.
    ("tabel",    include_str!("../../stdlib/tabel.jrg")),
    // Mesin game 2D di atas `canvas::`. Namespace-nya komponen
    // terakhir path: `import "std/mesin/vek"` memberi `vek::`.
    ("mesin",          include_str!("../../stdlib/mesin/mesin.jrg")),
    ("mesin/vek",      include_str!("../../stdlib/mesin/vek.jrg")),
    ("mesin/warna",    include_str!("../../stdlib/mesin/warna.jrg")),
    ("mesin/tween",    include_str!("../../stdlib/mesin/tween.jrg")),
    ("mesin/masukan",  include_str!("../../stdlib/mesin/masukan.jrg")),
    ("mesin/fisika",   include_str!("../../stdlib/mesin/fisika.jrg")),
    ("mesin/kamera",   include_str!("../../stdlib/mesin/kamera.jrg")),
    ("mesin/sprite",   include_str!("../../stdlib/mesin/sprite.jrg")),
    ("mesin/peta",     include_str!("../../stdlib/mesin/peta.jrg")),
    ("mesin/partikel", include_str!("../../stdlib/mesin/partikel.jrg")),
    ("mesin/ui",       include_str!("../../stdlib/mesin/ui.jrg")),
    ("mesin/adegan",   include_str!("../../stdlib/mesin/adegan.jrg")),
];

/// Namespace sebuah modul `std/`: komponen terakhir path-nya (`mesin/vek` -> `vek`).
pub fn namespace_modul(modul: &str) -> &str {
    modul.rsplit('/').next().unwrap_or(modul)
}

/// Satu fungsi publik sebuah modul `std/`, diturunkan dari sumbernya.
pub struct FungsiStd {
    /// `iterator::map`
    pub nama: String,
    /// `map(daftar, f)`
    pub tanda_tangan: String,
    /// Baris `///` pertama, kalau ada.
    pub ringkas: String,
    /// Nama modulnya, untuk `import "std/<modul>"`.
    pub modul: &'static str,
    /// Namespace-nya saat dipanggil (`mesin/vek` -> `vek`).
    pub namespace: &'static str,
}

/// Semua fungsi publik di semua modul `std/`, DITURUNKAN dari sumbernya.
pub fn fungsi_publik() -> Vec<FungsiStd> {
    let mut keluar = Vec::new();
    for (modul, src) in STDLIB_MODULES {
        let ns = namespace_modul(modul);
        let baris: Vec<&str> = src.lines().collect();
        // Tipe `impl` yang sedang dibaca, supaya method-nya ikut terpanen.
        let mut impl_kini: Option<String> = None;
        let mut dalam_impl = 0i32;
        for (i, l) in baris.iter().enumerate() {
            let potong = l.trim();
            if let Some(sisa_impl) = potong.strip_prefix("impl ") {
                if let Some(nama_tipe) = sisa_impl.split_whitespace().next() {
                    impl_kini = Some(nama_tipe.trim_end_matches('{').trim().to_string());
                    dalam_impl = 0;
                }
            }
            if impl_kini.is_some() {
                dalam_impl += l.matches('{').count() as i32;
                dalam_impl -= l.matches('}').count() as i32;
                if dalam_impl <= 0 && potong.starts_with('}') { impl_kini = None; }
            }
            let Some(sisa) = potong.strip_prefix("pub fn ") else { continue };
            let Some(kurung) = sisa.find('(') else { continue };
            let nama_fn = &sisa[..kurung];
            if nama_fn.starts_with('_') { continue; }
            let Some(tutup) = sisa.find(')') else { continue };
            // Doc-comment tepat di atasnya, baris `///` PERTAMA saja: satu
            // kalimat cukup untuk daftar, dan yang panjang ada di sumbernya.
            let mut ringkas = String::new();
            let mut j = i;
            while j > 0 {
                let atas = baris[j - 1].trim();
                if let Some(d) = atas.strip_prefix("///") {
                    let d = d.trim();
                    if !d.is_empty() { ringkas = d.to_string(); }
                    j -= 1;
                } else {
                    break;
                }
            }
            // Method ditulis `modul::Tipe.metode` supaya bentuk pemanggilannya
            // terbaca dari namanya sendiri.
            let (nama_penuh, tanda) = match (&impl_kini, l.starts_with("pub fn ")) {
                (Some(tipe), false) => {
                    // Tanda tangannya ditulis dalam BENTUK PEMANGGILAN: `self` dibuang karena ia
                    // penerimanya, bukan argumen yang ditulis.
                    let arg = &sisa[kurung + 1..tutup];
                    let sisa_arg: Vec<&str> = arg
                        .split(',')
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty() && *s != "self")
                        .collect();
                    (format!("{}::{}.{}", ns, tipe, nama_fn),
                     format!("{}.{}({})", tipe, nama_fn, sisa_arg.join(", ")))
                }
                _ => (format!("{}::{}", ns, nama_fn), sisa[..=tutup].to_string()),
            };
            keluar.push(FungsiStd {
                nama: nama_penuh,
                tanda_tangan: tanda,
                ringkas,
                modul,
                namespace: ns,
            });
        }
    }
    keluar.sort_by(|a, b| a.nama.cmp(&b.nama));
    keluar
}

/// Sumber modul stdlib bernama `nama`, kalau ada.
pub fn source_of(nama: &str) -> Option<&'static str> {
    // `std::mesin::vek` dan `std.mesin.vek` berarti `std/mesin/vek`, seperti di resolver berkas.
    let nama = nama.replace("::", "/").replace('.', "/");
    STDLIB_MODULES.iter().find(|(n, _)| *n == nama).map(|(_, src)| *src)
}

/// Daftar nama modul stdlib, terurut — dipakai di pesan error.
pub fn nama_modul() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = STDLIB_MODULES.iter().map(|(n, _)| *n).collect();
    v.sort();
    v
}

/// Pesan untuk `import "std/<nama>"` yang tidak ada.
pub fn pesan_modul_std_tak_ada(import_path: &str) -> String {
    let ditulis = crate::module::ModuleResolver::std_module_name(import_path)
        .unwrap_or(import_path);
    let ns = crate::module::namespace_bawaan();
    // Calonnya DUA himpunan sekaligus: modul JRG yang perlu di-import, dan
    // namespace bawaan yang tidak perlu.
    let saran = crate::diag::saran_nama_terdekat(
        ditulis,
        nama_modul().into_iter().chain(ns.iter().map(|s| s.as_str())),
    );
    let mut pesan = format!(
        "modul standar '{}' tidak ada. Yang tersedia: {}",
        import_path,
        nama_modul().iter().map(|n| format!("std/{}", n)).collect::<Vec<_>>().join(", ")
    );
    if let Some(s) = saran {
        if nama_modul().contains(&s.as_str()) {
            pesan.push_str(&format!(" — maksudnya `std/{}`?", s));
        } else {
            pesan.push_str(&format!(
                " — maksudnya namespace bawaan `{}::`? Itu tidak perlu di-import",
                s
            ));
        }
    }
    pesan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semua_modul_stdlib_ada_isinya() {
        for (nama, src) in STDLIB_MODULES {
            assert!(!src.trim().is_empty(), "modul stdlib `{}` kosong", nama);
            // Nama package harus satu identifier — parser JRG belum menerima titik di
            // dalamnya (`package std.option` gagal parse dengan "unexpected token: …
            let diharapkan = format!("package std_{}", nama.replace('/', "_"));
            assert!(
                src.contains(&diharapkan),
                "modul stdlib `{}` harusnya mendeklarasikan `{}`",
                nama, diharapkan
            );
        }
    }

    #[test]
    fn pencarian_nama_modul_bekerja() {
        assert!(source_of("option").is_some());
        assert!(source_of("result").is_some());
        assert!(source_of("tidak_ada").is_none());
    }

    /// Aturan proyek: pesan dan keluaran jrgc tidak memakai emoji.
    /// Modul stdlib ikut aturan yang sama karena keluarannya tercetak
    /// bersama keluaran program pemakainya.
    #[test]
    fn modul_stdlib_tidak_memakai_emoji() {
        for (nama, src) in STDLIB_MODULES {
            for c in src.chars() {
                let cp = c as u32;
                let emoji = (0x1F300..=0x1FAFF).contains(&cp)
                    || (0x2600..=0x27BF).contains(&cp)
                    || (0xFE00..=0xFE0F).contains(&cp);
                assert!(
                    !emoji,
                    "modul stdlib `{}` memakai emoji U+{:04X}",
                    nama, cp
                );
            }
        }
    }
}
