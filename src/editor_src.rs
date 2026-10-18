// src/editor_src.rs — sumber penyunting `jrgedit`, ditanam di biner seperti modul `std/`.
// `jrgc --sunting` menjalankannya dari akar maya `module::AKAR_EDITOR`, dengan tata letak
// yang sama dengan `editor/` di repo — jadi `import "sunting/x"` berarti hal yang sama.

/// `(jalur relatif terhadap `editor/`, isi)`. Dikunci tes: tiap berkas di `editor/sunting/`
/// wajib ada di sini, karena yang terlewat baru gagal saat `--sunting` dipakai.
const BERKAS: &[(&str, &str)] = &[
    ("jrgedit.jrg", include_str!("../editor/jrgedit.jrg")),
    ("sunting/aksara.jrg", include_str!("../editor/sunting/aksara.jrg")),
    ("sunting/berkas.jrg", include_str!("../editor/sunting/berkas.jrg")),
    ("sunting/cari.jrg", include_str!("../editor/sunting/cari.jrg")),
    ("sunting/daftar.jrg", include_str!("../editor/sunting/daftar.jrg")),
    ("sunting/dialog.jrg", include_str!("../editor/sunting/dialog.jrg")),
    ("sunting/garis.jrg", include_str!("../editor/sunting/garis.jrg")),
    ("sunting/jalan.jrg", include_str!("../editor/sunting/jalan.jrg")),
    ("sunting/keadaan.jrg", include_str!("../editor/sunting/keadaan.jrg")),
    ("sunting/layar.jrg", include_str!("../editor/sunting/layar.jrg")),
    ("sunting/lengkap.jrg", include_str!("../editor/sunting/lengkap.jrg")),
    ("sunting/penyangga.jrg", include_str!("../editor/sunting/penyangga.jrg")),
    ("sunting/perintah.jrg", include_str!("../editor/sunting/perintah.jrg")),
    ("sunting/sorot.jrg", include_str!("../editor/sunting/sorot.jrg")),
    ("sunting/tema.jrg", include_str!("../editor/sunting/tema.jrg")),
    ("sunting/tombol.jrg", include_str!("../editor/sunting/tombol.jrg")),
];

/// Isi berkas `rel` (relatif terhadap `editor/`), kalau ia bagian dari penyunting.
pub fn source_of(rel: &str) -> Option<&'static str> {
    BERKAS.iter().find(|(n, _)| *n == rel).map(|(_, isi)| *isi)
}

/// Program utama penyunting.
pub fn utama() -> &'static str {
    source_of("jrgedit.jrg").expect("jrgedit.jrg selalu ada di tabel")
}

/// Semua jalur di tabel, untuk tes kelengkapan.
pub fn semua() -> impl Iterator<Item = &'static str> {
    BERKAS.iter().map(|(n, _)| *n)
}

#[cfg(test)]
mod tes {
    /// Berkas di `editor/sunting/` yang lupa didaftarkan baru gagal saat `jrgc --sunting`
    /// dijalankan dari biner terpasang — dari repo semuanya tetap jalan lewat disk.
    #[test]
    fn tiap_modul_penyunting_tertanam() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("editor/sunting");
        let mut di_disk: Vec<String> = std::fs::read_dir(&dir).expect("editor/sunting")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".jrg"))
            .map(|n| format!("sunting/{}", n))
            .collect();
        di_disk.sort();
        let mut tertanam: Vec<String> = super::semua()
            .filter(|n| n.starts_with("sunting/")).map(|n| n.to_string()).collect();
        tertanam.sort();
        assert_eq!(di_disk, tertanam, "tabel `editor_src::BERKAS` tidak sama dengan isi editor/sunting/");
    }
}
