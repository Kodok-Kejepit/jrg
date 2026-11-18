// src/opsi.rs — method `Option`/`Result` yang ditulis dalam JRG (`opsi.jrg`), satu sumber untuk tiga
// mesin: method bawaan yang tidak ada atas `Option`/`Result` dicari di sini sebagai `<Tipe>_<method>`.

/// Sumber prelude, tertanam di biner.
pub const SUMBER: &str = include_str!("opsi.jrg");
/// Nama unit/berkasnya di diagnostik dan jalur `import` dalamnya (tidak bisa ditulis sebagai `std/`).
pub const NAMA: &str = "<opsi>";
/// Alias modulnya di tabel global; `#` tidak bisa ditulis di nama JRG, jadi tidak bertabrakan.
pub const ALIAS: &str = "opsi#";

/// Nama fungsi prelude untuk `method` atas nilai bertipe `tipe`, kalau tipenya `Option`/`Result`.
pub fn nama_fungsi(tipe: &str, method: &str) -> Option<String> {
    if (tipe != "Option" && tipe != "Result") || method.starts_with('_') { return None; }
    let nama = format!("{}_{}", tipe, method);
    SUMBER.contains(&format!("pub fn {}<", nama)).then_some(nama)
}

/// Method prelude milik `tipe`, untuk saran ejaan.
pub fn method_milik(tipe: &str) -> Vec<String> {
    let awal = format!("pub fn {}_", tipe);
    SUMBER.lines().filter_map(|b| b.strip_prefix(&awal))
        .filter_map(|s| s.split('<').next()).map(|s| s.to_string()).collect()
}

/// Kalimat jumlah argumen method prelude — sama dengan method bawaan (`value::pesan_metode_argumen`).
pub fn pesan_argumen(method: &str, param_sesudah_penerima: &[String], dapat: usize) -> String {
    format!("method `{}` butuh {} argumen ({}), dapat {}", method, param_sesudah_penerima.len(),
            param_sesudah_penerima.join(", "), dapat)
}

/// Sumber prelude tanpa baris `package`, untuk digabung ke unit lain (lapis LLVM).
pub fn badan() -> String {
    SUMBER.lines().filter(|b| !b.starts_with("package ")).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    /// Tiap `pub fn Tipe_m` terjangkau lewat `nama_fungsi`, dan tidak satu pun membayangi method bawaan.
    #[test]
    fn prelude_tidak_membayangi_method_bawaan() {
        for tipe in ["Option", "Result"] {
            let milik = super::method_milik(tipe);
            assert!(milik.len() >= 10, "{tipe}: cuma {} method prelude terbaca", milik.len());
            for m in &milik {
                assert_eq!(super::nama_fungsi(tipe, m).as_deref(), Some(format!("{tipe}_{m}").as_str()));
                assert!(!crate::interpreter::calon_method_enum(tipe).contains(&m.as_str()),
                    "{tipe}::{m} sudah method bawaan — prelude tidak akan pernah dicapai");
            }
        }
        assert!(super::nama_fungsi("Option", "_fungsi").is_none());
        assert!(super::nama_fungsi("Titik", "or").is_none());
    }
}
