// src/ffi/kotlin.rs Kotlin lewat JVM.

use crate::interpreter::{err_value, ok_value, Value};

pub fn tersedia() -> bool { super::jvm::tersedia() }

/// Fungsi top-level Kotlin di `Foo.kt` mendarat sebagai metode STATIS di kelas
/// `FooKt`. Menerima kedua bentuk: pemakai boleh menulis `uji.Hitung` atau
/// `uji.HitungKt`, karena yang pertama itulah yang tertulis di berkasnya.
pub fn kelas_berkas(nama: &str) -> String {
    if nama.ends_with("Kt") { nama.to_string() } else { format!("{}Kt", nama) }
}

fn teks(v: &Value, nama: &str, ke: usize) -> Result<String, String> {
    match v {
        Value::Str(s) => Ok(s.to_string()),
        lain => Err(format!("{}: argumen ke-{} harus string, dapat `{}`",
                            nama, ke + 1, lain.type_name())),
    }
}

fn args_array(a: Option<&Value>, nama: &str, ke: usize) -> Result<Vec<Value>, String> {
    match a {
        None => Ok(Vec::new()),
        Some(Value::Array(arr)) => Ok(arr.iter().cloned().collect()),
        Some(lain) => Err(format!("{}: argumen ke-{} harus array, dapat `{}`",
                                  nama, ke, lain.type_name())),
    }
}

pub fn b_tersedia(a: &[Value]) -> Result<Value, String> {
    if !a.is_empty() { return Err("kotlin::tersedia()".to_string()); }
    Ok(Value::Bool(tersedia()))
}

/// `kotlin::fungsi("uji.Hitung", "tambah", "(II)I", [3, 4])`
pub fn b_fungsi(a: &[Value]) -> Result<Value, String> {
    if a.len() < 3 || a.len() > 4 {
        return Err("kotlin::fungsi(berkas, nama, tanda_tangan, args) — `berkas` itu \
                    nama berkas .kt-nya (mis. \"uji.Hitung\" untuk Hitung.kt)".to_string());
    }
    let berkas = teks(&a[0], "kotlin::fungsi", 0)?;
    let nama = teks(&a[1], "kotlin::fungsi", 1)?;
    let sig = teks(&a[2], "kotlin::fungsi", 2)?;
    let args = args_array(a.get(3), "kotlin::fungsi", 4)?;
    match super::jvm::panggil_statis(&[], &kelas_berkas(&berkas), &nama, &sig, &args) {
        Ok(v) => Ok(ok_value(v)),
        Err(e) => Ok(err_value(e)),
    }
}

/// `object Bar` di Kotlin adalah satu instance tunggal, diekspos sebagai field
/// statis `INSTANCE`. Tanpa ini `object` sama sekali tak terjangkau.
pub fn b_objek(a: &[Value]) -> Result<Value, String> {
    if a.len() != 1 {
        return Err("kotlin::objek(nama) — mis. kotlin::objek(\"uji.Kalkulator\")".to_string());
    }
    let nama = teks(&a[0], "kotlin::objek", 0)?;
    let tipe = format!("L{};", nama.replace('.', "/"));
    match super::jvm::field_statis(&[], &nama, "INSTANCE", &tipe) {
        Ok(v) => Ok(ok_value(v)),
        Err(e) => Ok(err_value(format!(
            "{} — pastikan `{}` memang sebuah `object` Kotlin, bukan `class`", e, nama))),
    }
}

/// `companion object` diekspos sebagai field statis `Companion` pada kelasnya.
pub fn b_companion(a: &[Value]) -> Result<Value, String> {
    if a.len() != 1 {
        return Err("kotlin::companion(kelas) — mis. kotlin::companion(\"uji.Titik\")".to_string());
    }
    let kelas = teks(&a[0], "kotlin::companion", 0)?;
    let tipe = format!("L{}$Companion;", kelas.replace('.', "/"));
    match super::jvm::field_statis(&[], &kelas, "Companion", &tipe) {
        Ok(v) => Ok(ok_value(v)),
        Err(e) => Ok(err_value(format!(
            "{} — pastikan `{}` punya blok `companion object`", e, kelas))),
    }
}

/// `val x` Kotlin dibaca lewat getter `getX()`, bukan sebagai field — jadi
/// mengaksesnya sebagai field akan gagal walau propertinya jelas ada.
pub fn b_properti(a: &[Value]) -> Result<Value, String> {
    if a.len() != 3 {
        return Err("kotlin::properti(objek, nama, tipe) — mis. \
                    kotlin::properti(t, \"x\", \"D\")".to_string());
    }
    let nama = teks(&a[1], "kotlin::properti", 1)?;
    let tipe = teks(&a[2], "kotlin::properti", 2)?;
    let getter = nama_getter(&nama, &tipe);
    let sig = format!("(){}", tipe);
    let id = match super::jvm::id_dari_handle_pub(&a[0], "kotlin::properti") {
        Ok(i) => i, Err(e) => return Ok(err_value(e)),
    };
    match super::jvm::panggil_objek(&[], id, &getter, &sig, &[]) {
        Ok(v) => Ok(ok_value(v)),
        Err(e) => Ok(err_value(format!(
            "{} — properti `{}` dibaca lewat getter `{}`", e, nama, getter))),
    }
}

/// Diukur dari keluaran `javap`, BUKAN dari aturan JavaBeans: Kotlin memakai
/// `getJauh()` untuk `val jauh: Boolean`. Awalan `is` hanya dipertahankan
/// kalau nama propertinya MEMANG sudah diawali `is` — lalu dipakai apa adanya.
pub fn nama_getter(prop: &str, _tipe: &str) -> String {
    if prop.starts_with("is") && prop.len() > 2
        && prop.as_bytes()[2].is_ascii_uppercase() {
        return prop.to_string();
    }
    let mut c = prop.chars();
    match c.next() {
        Some(k) => format!("get{}{}", k.to_uppercase(), c.as_str()),
        None => prop.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nama_kelas_berkas_menambah_kt_sekali_saja() {
        assert_eq!(kelas_berkas("uji.Hitung"), "uji.HitungKt");
        assert_eq!(kelas_berkas("uji.HitungKt"), "uji.HitungKt");
    }

    #[test]
    fn getter_mengikuti_aturan_kotlin() {
        assert_eq!(nama_getter("x", "D"), "getX");
        assert_eq!(nama_getter("nama", "Ljava/lang/String;"), "getNama");
        // Diukur dari `javap`: Boolean TETAP `get…`, bukan `is…`.
        assert_eq!(nama_getter("aktif", "Z"), "getAktif");
        // Yang sudah diawali `is` dipakai apa adanya, tidak digandakan.
        assert_eq!(nama_getter("isAktif", "Z"), "isAktif");
    }
}
