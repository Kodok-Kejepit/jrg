// src/interpreter/konversi.rs — aturan konversi antar-tipe, satu definisi untuk
// `x as T`, `int()`/`uint()`/`float()`/`rune()` dan pintu parse, di dua mesin.

use crate::interpreter::Value;
use crate::parser::{PrimTy, TypeExpr, OwnershipQual};

/// Lebar bilangan bulat yang dituju sebuah konversi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lebar { I8, I16, I32, I64, I128, U8, U16, U32, U64, U128 }

impl Lebar {
    pub fn jangkauan(self) -> (i128, i128) {
        match self {
            Lebar::I8   => (i8::MIN as i128, i8::MAX as i128),
            Lebar::I16  => (i16::MIN as i128, i16::MAX as i128),
            Lebar::I32  => (i32::MIN as i128, i32::MAX as i128),
            Lebar::I64  => (i64::MIN as i128, i64::MAX as i128),
            Lebar::I128 => (i128::MIN, i128::MAX),
            Lebar::U8   => (0, u8::MAX as i128),
            Lebar::U16  => (0, u16::MAX as i128),
            Lebar::U32  => (0, u32::MAX as i128),
            Lebar::U64  => (0, u64::MAX as i128),
            // Nilai JRG paling besar `i128::MAX`, jadi yang bisa dilanggar cuma tandanya.
            Lebar::U128 => (0, i128::MAX),
        }
    }

    pub fn bertanda(self) -> bool {
        matches!(self, Lebar::I8 | Lebar::I16 | Lebar::I32 | Lebar::I64 | Lebar::I128)
    }
}

/// Sasaran bilangan bulat: nama yang DITULIS pemakai plus lebarnya. `byte`
/// adalah bilangan 8 bit tak bertanda, sama dengan arti yang dipakai FFI.
pub const SASARAN_BULAT: [(&str, Lebar); 15] = [
    ("i8", Lebar::I8), ("i16", Lebar::I16), ("i32", Lebar::I32), ("i64", Lebar::I64),
    ("i128", Lebar::I128), ("int", Lebar::I128), ("isize", Lebar::I64),
    ("u8", Lebar::U8), ("byte", Lebar::U8), ("u16", Lebar::U16), ("u32", Lebar::U32),
    ("u64", Lebar::U64), ("usize", Lebar::U64), ("u128", Lebar::U128), ("uint", Lebar::U128),
];

/// Nomor sasaran di [`SASARAN_BULAT`]. VM membawa NOMOR ini ke builtin lebarnya,
/// supaya tiap cast tidak mencocokkan teks.
pub fn nomor_sasaran_bulat(p: &PrimTy) -> Option<usize> {
    Some(match p {
        PrimTy::I8 => 0, PrimTy::I16 => 1, PrimTy::I32 => 2, PrimTy::I64 => 3,
        PrimTy::I128 => 4, PrimTy::Int => 5, PrimTy::Isize => 6,
        PrimTy::U8 => 7, PrimTy::Byte => 8, PrimTy::U16 => 9, PrimTy::U32 => 10,
        PrimTy::U64 => 11, PrimTy::Usize => 12, PrimTy::U128 => 13, PrimTy::Uint => 14,
        _ => return None,
    })
}

pub fn sasaran_bulat(p: &PrimTy) -> Option<(&'static str, Lebar)> {
    nomor_sasaran_bulat(p).map(|i| SASARAN_BULAT[i])
}

/// Apakah `ty` sesuatu yang bisa dituju `as`. Ditanyakan PARSER, jadi ketiga
/// mesin menolak sisanya dengan satu kalimat sebelum programnya jalan.
pub fn sasaran_cast_sah(ty: &TypeExpr) -> bool {
    match ty {
        TypeExpr::Primitive(p) => !matches!(p, PrimTy::Unit | PrimTy::C64 | PrimTy::C128),
        TypeExpr::Pointer(_) => true,
        TypeExpr::Ownership(o) => matches!(o.qualifier, OwnershipQual::Raw),
        TypeExpr::Named(n) => crate::parser::prim_dari_nama(&n.name)
            .map_or(false, |p| !matches!(p, PrimTy::Unit)),
        _ => false,
    }
}

/// Nama-nama tipe dasar yang bisa dituju `as` — calon saran ejaan.
const NAMA_SASARAN: &[&str] = &[
    "int", "uint", "float", "bool", "string", "str", "rune", "byte",
    "i8", "i16", "i32", "i64", "i128", "isize",
    "u8", "u16", "u32", "u64", "u128", "usize", "f32", "f64", "f128",
];

pub fn pesan_sasaran_cast_tak_sah(nama: &str) -> String {
    // `5 as itn` salah ketik, bukan niat membangun tipe lain.
    let saran = crate::diag::saran_nama_terdekat(nama, NAMA_SASARAN.iter().copied())
        .map(|s| format!(" — maksudnya `{}`?", s)).unwrap_or_default();
    format!(
        "cast ke `{}` tidak didukung{}. `as` cuma untuk tipe dasar (int/uint dan \
         lebarnya, float/f32/f64, bool, string, rune/byte) dan tipe pointer; untuk \
         membangun nilai bertipe lain, panggil konstruktornya (mis. `Titik {{ x: 1 }}`), \
         jangan pakai `as`",
        nama, saran
    )
}

fn tulis_pecahan(f: f64) -> String { Value::Float(f).to_string() }

// Kalimat-kalimatnya dibangun fungsi yang menerima `asli` sebagai TEKS, jadi
// backend LLVM bisa memanggilnya saat kompilasi dengan penanda di tempat nilai
// lalu menyisipkan nilainya saat jalan — teksnya tetap satu definisi.

/// Angka yang tidak muat di `lebar`; `negatif` memilih kalimat tanda.
pub fn pesan_tak_muat(asli: &str, nama: &str, lebar: Lebar, negatif: bool) -> String {
    let (lo, hi) = lebar.jangkauan();
    if negatif && !lebar.bertanda() {
        return format!(
            "`{}` tidak muat di `{}` — angka negatif tidak punya padanan di tipe tak bertanda",
            asli, nama);
    }
    let jalan = if lebar.bertanda() { String::new() } else {
        format!(". Kalau yang dimaksud bit terbawahnya, tulis `x & {}`", hi)
    };
    format!(
        "`{}` tidak muat di `{}` — jangkauannya {} sampai {}, dan angkanya tidak \
         dipotong diam-diam{}", asli, nama, lo, hi, jalan)
}

/// Nilai `asal` (`float`/`bool`) di tempat yang butuh `sasaran` bulat, tanpa `as` yang
/// ditulis: acuan tidak pernah memotongnya diam-diam, jadi backend LLVM juga tidak.
pub fn pesan_tanpa_potong_diam(asal: &str, sasaran: &str) -> String {
    format!("nilai `{}` tidak bisa dipakai sebagai `{}` tanpa ditulis — JRG tidak pernah \
             memotong pecahan atau membaca `bool` sebagai angka diam-diam\n\
             bantuan: tulis `x as {}` kalau memang itu yang dimaksud", asal, sasaran, sasaran)
}

pub fn pesan_nan(nama: &str) -> String {
    format!(
        "`NaN` tidak bisa jadi `{}` — NaN memang bukan angka, jadi tidak ada \
         bilangan bulat yang mewakilinya", nama)
}

pub fn pesan_tak_hingga(asli: &str, nama: &str) -> String {
    format!("`{}` tidak bisa jadi `{}` — tak hingga tidak muat di bilangan bulat mana pun",
        asli, nama)
}

pub fn pesan_luar_128(asli: &str, nama: &str) -> String {
    format!(
        "`{}` tidak muat di `{}` — di luar jangkauan integer 128 bit, dan angkanya \
         tidak dijepit diam-diam ke batas", asli, nama)
}

pub fn pesan_bukan_kode_rune(asli: &str) -> String {
    format!(
        "`{}` bukan kode karakter Unicode yang sah, jadi tidak bisa jadi `rune` — \
         yang sah 0 sampai 1114111 (0x10FFFF), kecuali 55296 sampai 57343 \
         (0xD800–0xDFFF, separuh pasangan surrogate UTF-16)", asli)
}

/// Kenapa sebuah teks gagal jadi bilangan bulat. `Pecahan` dipisah karena
/// jalan keluarnya beda per pintu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GagalTeksBulat { Kosong, Luar128, BukanBulat, Pecahan }

/// Kenapa sebuah teks gagal jadi pecahan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GagalTeksPecahan { Kosong, BukanAngka }

fn sebab_teks_bulat(g: GagalTeksBulat, t: &str) -> String {
    match g {
        GagalTeksBulat::Kosong => "teks kosong bukan bilangan bulat".to_string(),
        GagalTeksBulat::Luar128 => format!("teks `{}` di luar jangkauan integer 128 bit", t),
        GagalTeksBulat::BukanBulat | GagalTeksBulat::Pecahan =>
            format!("teks `{}` bukan bilangan bulat", t),
    }
}

fn sebab_teks_pecahan(g: GagalTeksPecahan, t: &str) -> String {
    match g {
        GagalTeksPecahan::Kosong => "teks kosong bukan angka".to_string(),
        GagalTeksPecahan::BukanAngka => format!("teks `{}` bukan angka", t),
    }
}

/// `"x" as T` dan `int("x")` yang gagal; `t` teks yang sudah di-trim.
pub fn pesan_teks_jadi_bulat(g: GagalTeksBulat, t: &str, nama: &str) -> String {
    format!("{}, jadi tidak bisa jadi `{}`{}", sebab_teks_bulat(g, t), nama,
        if g == GagalTeksBulat::Pecahan {
            " — kalau pecahannya memang dimaksud, jadikan `float` dulu \
             (`x as float as int` memotongnya ke arah nol)"
        } else { "" })
}

pub fn pesan_teks_jadi_pecahan(g: GagalTeksPecahan, t: &str) -> String {
    format!("{}, jadi tidak bisa jadi `float`", sebab_teks_pecahan(g, t))
}

/// Pintu parse (`str::to_int`) atas teks yang gagal.
pub fn pesan_parse_teks_bulat(pintu: &str, g: GagalTeksBulat, t: &str) -> String {
    format!("{}: {}{}", pintu, sebab_teks_bulat(g, t),
        if g == GagalTeksBulat::Pecahan {
            " — kalau pecahannya memang dimaksud, pakai `str::to_float`"
        } else { "" })
}

pub fn pesan_parse_teks_pecahan(pintu: &str, g: GagalTeksPecahan, t: &str) -> String {
    format!("{}: {}", pintu, sebab_teks_pecahan(g, t))
}

/// Pintu parse atas nilai yang bukan teks.
pub fn pesan_parse_butuh_teks(pintu: &str, tipe: &str) -> String {
    let jalan = if tipe == "float" {
        " — untuk memotong pecahan jadi bilangan bulat tulis `x as int`"
    } else { "" };
    format!("{}: butuh teks, dapat `{}`{}", pintu, tipe, jalan)
}

/// Apakah `n` muat di `lebar`.
pub fn cek_muat(n: i128, nama: &str, lebar: Lebar) -> Result<i128, String> {
    cek_muat_asli(n, nama, lebar, || n.to_string())
}

/// Sama, dengan `asli` — nilai sebelum dikonversi (`-5.9`, bukan `-5`) — yang
/// baru ditulis kalau memang gagal, jadi jalur yang muat tidak mengalokasi.
pub fn cek_muat_asli(n: i128, nama: &str, lebar: Lebar, asli: impl FnOnce() -> String)
    -> Result<i128, String>
{
    let (lo, hi) = lebar.jangkauan();
    if n < lo || n > hi {
        return Err(pesan_tak_muat(&asli(), nama, lebar, n < 0));
    }
    Ok(n)
}

/// Pecahan → bilangan bulat, dipotong ke arah nol. NaN, tak hingga dan yang di
/// luar jangkauan DITOLAK — dijepit ke batas berarti angka yang tidak ada
/// hubungannya dengan nilai aslinya.
pub fn float_ke_bulat(f: f64, nama: &str, lebar: Lebar) -> Result<i128, String> {
    if f.is_nan() { return Err(pesan_nan(nama)); }
    if f.is_infinite() { return Err(pesan_tak_hingga(&tulis_pecahan(f), nama)); }
    // Dibandingkan sebagai float: `i128::MAX as f64` membulat ke ATAS.
    const BATAS_ATAS: f64 = 170141183460469231731687303715884105727.0;
    const BATAS_BAWAH: f64 = -170141183460469231731687303715884105728.0;
    let t = f.trunc();
    if t >= BATAS_ATAS || t < BATAS_BAWAH {
        return Err(pesan_luar_128(&tulis_pecahan(f), nama));
    }
    cek_muat_asli(t as i128, nama, lebar, || tulis_pecahan(f))
}

/// Teks → bilangan bulat, untuk `"12" as int` dan `int("12")`.
pub fn teks_ke_bulat(s: &str, nama: &str, lebar: Lebar) -> Result<i128, String> {
    match bulat_dari_teks(s) {
        Ok(n) => cek_muat_asli(n, nama, lebar, || format!("\"{}\"", s.trim())),
        Err(g) => Err(pesan_teks_jadi_bulat(g, s.trim(), nama)),
    }
}

/// Teks → pecahan, untuk `"1.5" as float` dan `float("1.5")`.
pub fn teks_ke_float(s: &str) -> Result<f64, String> {
    float_dari_teks(s).map_err(|g| pesan_teks_jadi_pecahan(g, s.trim()))
}

/// Satu definisi "teks apa yang sah jadi bilangan bulat", dipakai juga runtime
/// LLVM lewat kode gagalnya (lihat `GagalTeksBulat`).
pub fn bulat_dari_teks(s: &str) -> Result<i128, GagalTeksBulat> {
    use std::num::IntErrorKind;
    let t = s.trim();
    t.parse::<i128>().map_err(|e| match e.kind() {
        IntErrorKind::Empty => GagalTeksBulat::Kosong,
        IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => GagalTeksBulat::Luar128,
        _ if t.parse::<f64>().is_ok() => GagalTeksBulat::Pecahan,
        _ => GagalTeksBulat::BukanBulat,
    })
}

fn float_dari_teks(s: &str) -> Result<f64, GagalTeksPecahan> {
    let t = s.trim();
    if t.is_empty() { return Err(GagalTeksPecahan::Kosong); }
    t.parse::<f64>().map_err(|_| GagalTeksPecahan::BukanAngka)
}

/// Bilangan bulat → `rune`. Yang bukan kode karakter Unicode ditolak; dulu
/// semuanya diam-diam jadi karakter NUL.
pub fn kode_ke_rune(n: i128) -> Result<char, String> {
    u32::try_from(n).ok().and_then(char::from_u32)
        .ok_or_else(|| pesan_bukan_kode_rune(&n.to_string()))
}

/// Alamat untuk cast ke tipe pointer: 0 berarti `nil`.
pub fn alamat_dari_bulat(n: i128) -> Result<Option<usize>, String> {
    let n = cek_muat(n, "pointer", Lebar::U64)?;
    Ok(if n == 0 { None } else { Some(n as usize) })
}

/// `nil` ke sasaran angka.
pub fn pesan_nil_jadi(nama: &str) -> String {
    format!(
        "`nil` tidak bisa jadi `{}` — `nil` bukan angka. Kalau maksudnya pakai 0 \
         sebagai cadangan, tulis itu eksplisit di program", nama)
}

/// Tipe yang memang tidak punya konversi ke sasarannya.
pub fn pesan_tak_bisa_jadi(tipe: &str, nama: &str) -> String {
    let jalan = match (tipe, nama) {
        ("string", "rune") => " — satu karakter dari sebuah teks diambil dengan `str::chars(s)[i]`",
        ("float", "rune")  => " — kode karakter harus bilangan bulat, jadi tulis `x as int as rune`",
        _ => "",
    };
    format!("`{}` tidak bisa jadi `{}`{}", tipe, nama, jalan)
}

/// Pintu parse (`str::to_int`, alias `parse_int`, dan `.to_int()`): teks
/// diurai, bilangan bulat dijawab dirinya, sisanya ditolak — termasuk pecahan,
/// yang dulu dipotong diam-diam sementara teksnya sendiri ditolak.
pub fn parse_bulat(nama: &str, v: &Value) -> Result<i128, String> {
    match v {
        Value::Str(s) => bulat_dari_teks(s)
            .map_err(|g| pesan_parse_teks_bulat(nama, g, s.trim())),
        Value::Int(n) => Ok(*n),
        lain => Err(pesan_parse_butuh_teks(nama, &lain.type_name())),
    }
}

/// Pintu parse untuk pecahan (`str::to_float`, alias `parse_float`, `.to_float()`).
pub fn parse_float(nama: &str, v: &Value) -> Result<f64, String> {
    match v {
        Value::Str(s) => float_dari_teks(s)
            .map_err(|g| pesan_parse_teks_pecahan(nama, g, s.trim())),
        Value::Float(f) => Ok(*f),
        Value::Int(n) => Ok(*n as f64),
        lain => Err(pesan_parse_butuh_teks(nama, &lain.type_name())),
    }
}

#[cfg(test)]
mod tes {
    use super::*;

    /// Tabel nomor dan pemetaannya dua daftar; yang menjaga keduanya tetap
    /// sepakat adalah nama yang ditulis parser untuk tipe yang sama.
    #[test]
    fn nomor_sasaran_menunjuk_nama_yang_ditulis() {
        use PrimTy::*;
        for p in [I8, I16, I32, I64, I128, Int, Isize, U8, Byte, U16, U32, U64, Usize, U128, Uint] {
            let (nama, _) = sasaran_bulat(&p).expect("sasaran bilangan bulat");
            assert_eq!(nama, crate::parser::nama_tipe(&TypeExpr::Primitive(p)));
        }
    }
}
