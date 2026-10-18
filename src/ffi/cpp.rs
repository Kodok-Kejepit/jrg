// src/ffi/cpp.rs
// Mangling nama Itanium C++ ABI.

use std::collections::HashMap;

use crate::parser::{PrimTy, TypeExpr};

/// Kode tipe dasar Itanium. Bukan calon substitusi — tipe bawaan tidak
/// pernah masuk tabel, dan itu yang membuat `f(int, int)` jadi `ii` alih-alih
/// `iS_`.
fn kode_dasar(t: &str) -> Option<&'static str> {
    Some(match t {
        "void" => "v",
        "bool" => "b",
        "char" => "c",
        "signed char" => "a",
        "unsigned char" => "h",
        "short" => "s",
        "unsigned short" => "t",
        "int" => "i",
        "unsigned int" => "j",
        "long" => "l",
        "unsigned long" => "m",
        "long long" => "x",
        "unsigned long long" => "y",
        "float" => "f",
        "double" => "d",
        "long double" => "e",
        "wchar_t" => "w",
        _ => return None,
    })
}

const DIGIT: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// `S_`, `S0_`, `S1_`, ... `S9_`, `SA_`, ...
fn rujuk(n: usize) -> Result<String, String> {
    if n == 0 { return Ok("S_".to_string()); }
    let i = n - 1;
    if i < DIGIT.len() {
        Ok(format!("S{}_", DIGIT[i] as char))
    } else {
        Err("tanda tangan C++ ini butuh lebih dari 37 substitusi".to_string())
    }
}

/// Pisahkan pembungkus (`*`, `&`, `&&`, `const`) dari tipe dasarnya.
/// Pembungkus dikembalikan dari LUAR ke dalam.
fn urai(t: &str) -> (String, Vec<char>) {
    let mut sisa = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut bungkus = Vec::new();
    loop {
        let s = sisa.trim();
        if let Some(x) = s.strip_suffix('*') {
            bungkus.push('P'); sisa = x.trim().to_string();
        } else if let Some(x) = s.strip_suffix("&&") {
            bungkus.push('O'); sisa = x.trim().to_string();
        } else if let Some(x) = s.strip_suffix('&') {
            bungkus.push('R'); sisa = x.trim().to_string();
        } else if let Some(x) = s.strip_suffix(" const") {
            bungkus.push('K'); sisa = x.trim().to_string();
        } else if let Some(x) = s.strip_prefix("const ") {
            bungkus.push('K'); sisa = x.trim().to_string();
        } else {
            break;
        }
    }
    (sisa.trim().to_string(), bungkus)
}

/// `ns::Vec` -> `2ns3Vec`. Ini KUNCI tabel substitusi, bukan keluarannya.
fn kunci_nama(n: &str) -> String {
    n.split("::").map(|b| format!("{}{}", b.len(), b)).collect()
}

/// Daftarkan tiap AWALAN namespace sebuah nama, kembalikan teks awalannya.
fn daftar_awalan(nama: &str, subs: &mut HashMap<String, usize>) -> Result<String, String> {
    let bagian: Vec<&str> = nama.split("::").collect();
    let mut penuh = String::new();
    let mut teks = String::new();
    for b in &bagian[..bagian.len() - 1] {
        penuh.push_str(&format!("{}{}", b.len(), b));
        if let Some(&i) = subs.get(&penuh) {
            teks = rujuk(i)?;
        } else {
            teks.push_str(&format!("{}{}", b.len(), b));
            let n = subs.len();
            subs.insert(penuh.clone(), n);
        }
    }
    Ok(teks)
}

/// (teks, kunci, boleh_masuk_tabel)
fn kode_inti(dasar: &str, subs: &mut HashMap<String, usize>)
    -> Result<(String, String, bool), String>
{
    if let Some(k) = kode_dasar(dasar) {
        return Ok((k.to_string(), k.to_string(), false));
    }
    if !dasar.contains("::") {
        let k = kunci_nama(dasar);
        if let Some(&i) = subs.get(&k) { return Ok((rujuk(i)?, k, true)); }
        return Ok((k.clone(), k, true));
    }
    let penuh = kunci_nama(dasar);
    // Nama bersarang yang UTUH sudah pernah muncul: rujuk telanjang,
    // bukan dibungkus `N...E` lagi.
    if let Some(&i) = subs.get(&penuh) { return Ok((rujuk(i)?, penuh, true)); }
    let teks = daftar_awalan(dasar, subs)?;
    let akhir = dasar.rsplit("::").next().unwrap_or(dasar);
    Ok((format!("N{}{}{}E", teks, akhir.len(), akhir), penuh, true))
}

fn tipe_kode(t: &str, subs: &mut HashMap<String, usize>) -> Result<String, String> {
    let (dasar, bungkus) = urai(t);
    let kunci_inti = match kode_dasar(&dasar) {
        Some(k) => k.to_string(),
        None => {
            if dasar.is_empty() {
                return Err("tipe kosong di tanda tangan C++".to_string());
            }
            kunci_nama(&dasar)
        }
    };
    let mut kunci = kunci_inti.clone();
    for b in bungkus.iter().rev() { kunci = format!("{}{}", b, kunci); }
    if let Some(&i) = subs.get(&kunci) { return rujuk(i); }

    let (mut teks, k_inti, boleh) = kode_inti(&dasar, subs)?;
    if boleh && !subs.contains_key(&k_inti) {
        let n = subs.len();
        subs.insert(k_inti, n);
    }
    let mut k = kunci_inti;
    for b in bungkus.iter().rev() {
        k = format!("{}{}", b, k);
        if let Some(&i) = subs.get(&k) {
            teks = rujuk(i)?;
        } else {
            teks = format!("{}{}", b, teks);
            let n = subs.len();
            subs.insert(k.clone(), n);
        }
    }
    Ok(teks)
}

/// Nama simbol Itanium untuk sebuah fungsi BEBAS (boleh bernamespace).
pub fn mangle(nama: &str, params: &[String]) -> Result<String, String> {
    if nama.is_empty() {
        return Err("nama fungsi C++ kosong".to_string());
    }
    let mut subs: HashMap<String, usize> = HashMap::new();
    let bagian: Vec<&str> = nama.split("::").collect();
    let inti = if bagian.len() == 1 {
        format!("{}{}", bagian[0].len(), bagian[0])
    } else {
        let teks = daftar_awalan(nama, &mut subs)?;
        let akhir = bagian[bagian.len() - 1];
        format!("N{}{}{}E", teks, akhir.len(), akhir)
    };
    let nyata: Vec<&String> = params.iter().filter(|p| p.trim() != "void").collect();
    let arg = if nyata.is_empty() {
        "v".to_string()
    } else {
        let mut s = String::new();
        for p in nyata { s.push_str(&tipe_kode(p, &mut subs)?); }
        s
    };
    Ok(format!("_Z{}{}", inti, arg))
}

/// Nama tipe C++ untuk sebuah tipe JRG, khusus keperluan mangling.
pub fn tipe_cpp(ty: &TypeExpr) -> Result<String, String> {
    Ok(match ty {
        TypeExpr::Primitive(p) => tipe_cpp_primitif(p)?.to_string(),
        TypeExpr::Pointer(pt) => {
            let dalam = tipe_cpp(&pt.inner)?;
            if pt.is_mut { format!("{}*", dalam) } else { format!("const {}*", dalam) }
        }
        TypeExpr::Named(n) => match &n.package {
            Some(pkg) => format!("{}::{}", pkg, n.name),
            None => n.name.clone(),
        },
        lain => return Err(format!(
            "tipe {:?} belum bisa dipakai di blok extern \"C++\"", lain
        )),
    })
}

fn tipe_cpp_primitif(p: &PrimTy) -> Result<&'static str, String> {
    Ok(match p {
        PrimTy::I8 => "signed char",
        PrimTy::I16 => "short",
        PrimTy::I32 => "int",
        PrimTy::I64 | PrimTy::Isize | PrimTy::Int => "long",
        PrimTy::U8 | PrimTy::Byte => "unsigned char",
        PrimTy::U16 => "unsigned short",
        PrimTy::U32 => "unsigned int",
        PrimTy::U64 | PrimTy::Usize | PrimTy::Uint => "unsigned long",
        PrimTy::Bool => "bool",
        PrimTy::F32 => "float",
        PrimTy::F64 => "double",
        PrimTy::Str | PrimTy::String_ => "const char*",
        PrimTy::Unit => "void",
        // Ditolak dengan sebabnya masing-masing, bukan dipetakan ke sesuatu
        // yang mirip: nama simbol yang salah gagal sebagai "simbol tidak
        // ada", dan itu mengirim pembacanya memeriksa pustakanya.
        PrimTy::I128 | PrimTy::U128 => return Err(
            "128-bit tidak punya padanan yang portabel di ABI C++".to_string()),
        PrimTy::F128 => return Err("`f128` tidak ada di ABI C++".to_string()),
        PrimTy::C64 | PrimTy::C128 => return Err(
            "tipe kompleks belum didukung di blok extern \"C++\"".to_string()),
        PrimTy::Rune => return Err(
            "`rune` itu titik-kode Unicode, bukan tipe C++ — pakai `i32` kalau \
             fungsinya memang menerima `int`".to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(n: &str, p: &[&str]) -> String {
        mangle(n, &p.iter().map(|s| s.to_string()).collect::<Vec<_>>()).expect("mangle")
    }

    /// Nilai harapan di sini BUKAN dibaca dari spesifikasi: tiap satu diambil
    /// dari keluaran `nm` atas berkas objek yang dikompilasi `g++`.
    #[test]
    fn cocok_dengan_keluaran_g_plus_plus() {
        assert_eq!(m("add", &["int", "int"]), "_Z3addii");
        assert_eq!(m("f", &[]), "_Z1fv");
        assert_eq!(m("f2", &["void"]), "_Z2f2v");
        assert_eq!(m("g", &["double"]), "_Z1gd");
        assert_eq!(m("h", &["const char*"]), "_Z1hPKc");
        assert_eq!(m("h2", &["const char*", "const char*"]), "_Z2h2PKcS0_");
        assert_eq!(m("h3", &["const char*", "int", "const char*"]), "_Z2h3PKciS0_");
        assert_eq!(m("q", &["int*", "int*"]), "_Z1qPiS_");
        assert_eq!(m("r", &["const int&"]), "_Z1rRKi");
        assert_eq!(m("s", &["bool", "float", "long", "unsigned long"]), "_Z1sbflm");
        assert_eq!(m("t", &["long long", "unsigned long long", "short", "unsigned short"]),
                   "_Z1txyst");
        assert_eq!(m("u", &["unsigned char", "signed char", "char"]), "_Z1uhac");
        assert_eq!(m("math::add", &["int", "int"]), "_ZN4math3addEii");
        assert_eq!(m("a::b::c", &["double", "double"]), "_ZN1a1b1cEdd");
        assert_eq!(m("v", &["void*", "void*"]), "_Z1vPvS_");
        assert_eq!(m("w", &["char**"]), "_Z1wPPc");
        assert_eq!(m("x", &["const char**"]), "_Z1xPPKc");
        assert_eq!(m("y", &["int", "const char*", "double", "const char*"]), "_Z1yiPKcdS0_");
        assert_eq!(m("z", &["unsigned int", "unsigned int"]), "_Z1zjj");
    }

    /// Substitusi adalah bagian yang paling mudah dibuat "hampir benar",
    /// dan hampir benar di sini berarti simbol yang tidak pernah ketemu.
    #[test]
    fn substitusi_cocok_dengan_g_plus_plus() {
        assert_eq!(m("k1", &["const char*", "char*", "const char*"]), "_Z2k1PKcPcS0_");
        assert_eq!(m("k2", &["int*", "const int*", "int*"]), "_Z2k2PiPKiS_");
        assert_eq!(m("k3", &["char**", "char**"]), "_Z2k3PPcS0_");
        assert_eq!(m("k4", &["const char**", "const char*"]), "_Z2k4PPKcS0_");
        assert_eq!(m("k5", &["Foo", "Foo"]), "_Z2k53FooS_");
        assert_eq!(m("k6", &["Foo*", "Foo*"]), "_Z2k6P3FooS0_");
        assert_eq!(m("k7", &["const Foo&", "const Foo&"]), "_Z2k7RK3FooS1_");
        assert_eq!(m("k8", &["ns::Vec", "ns::Vec"]), "_Z2k8N2ns3VecES0_");
        assert_eq!(m("k9", &["ns::Vec*", "ns::Vec"]), "_Z2k9PN2ns3VecES0_");
        assert_eq!(m("k10", &["ns::in::Deep", "ns::Vec"]), "_Z3k10N2ns2in4DeepENS_3VecE");
        assert_eq!(m("k11", &["double*", "double*", "double*"]), "_Z3k11PdS_S_");
        assert_eq!(m("k12", &["const double*", "double*", "const double*"]), "_Z3k12PKdPdS0_");
        assert_eq!(m("k13", &["Foo", "ns::Vec", "Foo", "ns::Vec"]), "_Z3k133FooN2ns3VecES_S1_");
        assert_eq!(m("k14", &["int&", "int&"]), "_Z3k14RiS_");
        assert_eq!(m("k15", &["void*", "char*", "void*"]), "_Z3k15PvPcS_");
        // Nama fungsi bernamespace MENGISI tabel substitusi lebih dulu.
        assert_eq!(m("ns::k16", &["ns::Vec", "int"]), "_ZN2ns3k16ENS_3VecEi");
        assert_eq!(m("k17", &["const char* const*"]), "_Z3k17PKPKc");
        assert_eq!(m("k18", &["Foo**", "Foo*"]), "_Z3k18PP3FooS0_");
    }

    #[test]
    fn substitusi_melewati_sepuluh() {
        let p: Vec<&str> = vec!["A1*","A2*","A3*","A4*","A5*","A6*","A7*","A8*",
                                "A9*","A10*","A11*","A12*","A1*","A11*"];
        assert_eq!(
            m("m1", &p),
            "_Z2m1P2A1P2A2P2A3P2A4P2A5P2A6P2A7P2A8P2A9P3A10P3A11P3A12S0_SK_"
        );
    }

    #[test]
    fn tanda_tangan_tak_terdukung_ditolak_bukan_ditebak() {
        assert!(mangle("", &[]).is_err());
        assert!(tipe_cpp(&TypeExpr::Primitive(PrimTy::I128)).is_err());
        assert!(tipe_cpp(&TypeExpr::Primitive(PrimTy::Rune)).is_err());
        assert_eq!(tipe_cpp(&TypeExpr::Primitive(PrimTy::I32)).unwrap(), "int");
        assert_eq!(tipe_cpp(&TypeExpr::Primitive(PrimTy::Str)).unwrap(), "const char*");
    }
}
