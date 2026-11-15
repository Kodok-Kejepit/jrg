// src/stdlib/jwt.rs — `jwt::`: JSON Web Token HS256 (RFC 7519), tanpa dependensi.
// Satu algoritma yang dipatok, bukan dibaca dari token: `alg: none` dan alg confusion tidak punya pintu masuk.

use crate::interpreter::{err_value, ok_value, Value};
use crate::vm::vm::Vm;

/// RFC 7518 §3.2: kunci HMAC-SHA256 paling pendek 256 bit.
pub const MIN_RAHASIA: usize = 32;

pub fn register(vm: &mut Vm) {
    vm.def("jwt::sign", |a| crate::stdlib::sistem::lewat_hasil(a, j_sign));
    vm.def("jwt::verify", |a| crate::stdlib::sistem::lewat_hasil(a, j_verify));
}

const KEPALA: &str = r#"{"alg":"HS256","typ":"JWT"}"#;

fn b64url(b: &[u8]) -> String { crate::dok::crypto::base64url_encode_bytes(b) }

/// Base64url ketat (satu definisi, `crypto::base64url_decode_str`) dengan kalimat milik `jwt::`.
fn b64url_ketat(s: &str, bagian: &str) -> Result<Vec<u8>, String> {
    if s.is_empty() { return Err(format!("{} token bukan base64url yang sah", bagian)); }
    crate::dok::crypto::base64url_decode_str(s).map_err(|e| if e.contains("bit sisa") {
        format!("{} token bukan base64url kanonik", bagian)
    } else {
        format!("{} token bukan base64url yang sah", bagian)
    })
}

/// Objek JSON tanpa kunci kembar — dua pembaca yang memilih kunci berbeda adalah dua token berbeda.
fn objek_json(teks: &[u8], bagian: &str) -> Result<Vec<(String, crate::json_inti::Json)>, String> {
    let s = std::str::from_utf8(teks).map_err(|_| format!("{} token bukan UTF-8", bagian))?;
    match crate::json_inti::urai(s) {
        Ok(crate::json_inti::Json::Obj(isi)) => {
            for (i, (k, _)) in isi.iter().enumerate() {
                if isi[..i].iter().any(|(j, _)| j == k) {
                    return Err(format!("{} token memuat kunci `{}` dua kali", bagian, k));
                }
            }
            Ok(isi)
        }
        Ok(_) => Err(format!("{} token bukan objek JSON", bagian)),
        Err(e) => Err(format!("{} token bukan JSON yang sah: {}", bagian, e)),
    }
}

fn teks(a: &[Value], ke: usize, nama: &str) -> Result<String, String> {
    match a.get(ke) {
        Some(Value::Str(s)) => Ok(s.to_string()),
        Some(lain) => Err(format!("{}: argumen ke-{} harus string, dapat `{}`", nama, ke + 1, lain.type_name())),
        None => Err(format!("{}: argumen ke-{} wajib diisi", nama, ke + 1)),
    }
}

fn rahasia(a: &[Value], nama: &str) -> Result<Vec<u8>, String> {
    let r = teks(a, 1, nama)?;
    if r.len() < MIN_RAHASIA {
        return Err(format!("{}: rahasianya {} byte, paling sedikit {} (256 bit, RFC 7518) — rahasia pendek bisa \
                            ditebak di luar jaringan dari satu token saja; buat dengan `crypto::random_hex(32)`",
                           nama, r.len(), MIN_RAHASIA));
    }
    Ok(r.into_bytes())
}

/// `jwt::sign(klaim, rahasia)` → `Result<string>`: `klaim` dict apa pun yang bisa jadi JSON.
fn j_sign(a: &[Value]) -> Result<Value, String> {
    let klaim = match a.first() {
        Some(v @ Value::Struct { name, .. }) if name == "dict" => v,
        Some(lain) => return Err(format!("jwt::sign: argumen ke-1 harus dict klaim, dapat `{}`", lain.type_name())),
        None => return Err("jwt::sign: argumen ke-1 wajib diisi".into()),
    };
    let kunci = rahasia(a, "jwt::sign")?;
    let muatan = match crate::dok::json::encode(klaim) { Ok(j) => j, Err(e) => return Ok(err_value(format!("jwt::sign: {}", e))) };
    let isi = format!("{}.{}", b64url(KEPALA.as_bytes()), b64url(muatan.as_bytes()));
    let tanda = crate::dok::crypto::hmac_sha256(&kunci, isi.as_bytes());
    Ok(ok_value(Value::teks(format!("{}.{}", isi, b64url(&tanda)))))
}

/// `jwt::verify(token, rahasia)` → `Result<dict>`: tanda tangan, lalu `exp`/`nbf`; galat apa pun jadi `Err`.
fn j_verify(a: &[Value]) -> Result<Value, String> {
    let token = teks(a, 0, "jwt::verify")?;
    let kunci = rahasia(a, "jwt::verify")?;
    Ok(match periksa(&token, &kunci, crate::host::sejak_epoch().map(|d| d.as_secs() as i128)) {
        Ok(muatan) => match crate::dok::json::decode(&muatan) {
            Ok(v) => ok_value(v),
            Err(e) => err_value(format!("jwt::verify: {}", e)),
        },
        Err(e) => err_value(format!("jwt::verify: {}", e)),
    })
}

/// Inti pemeriksaan, tanpa `Value`: teks muatan JSON kalau tokennya sah pada `kini` (detik Unix).
pub fn periksa(token: &str, kunci: &[u8], kini: Option<i128>) -> Result<String, String> {
    let bagian: Vec<&str> = token.split('.').collect();
    if bagian.len() != 3 {
        return Err(format!("token berisi {} bagian, JWT bertanda tangan punya tepat 3 (kepala.muatan.tanda)", bagian.len()));
    }
    let kepala = objek_json(&b64url_ketat(bagian[0], "kepala")?, "kepala")?;
    // Algoritmanya DIPATOK, tidak dipercaya dari token: `none`, `HS512`, `RS256` sama-sama ditolak.
    match kepala.iter().find(|(k, _)| k == "alg").map(|(_, v)| v) {
        Some(crate::json_inti::Json::Str(alg)) if alg == "HS256" => {}
        Some(crate::json_inti::Json::Str(alg)) =>
            return Err(format!("algoritma `{}` ditolak — `jwt::` cuma menerima HS256, dan tidak pernah memilih \
                                algoritma dari isi token (kelas `alg: none` dan alg confusion)", alg)),
        _ => return Err("kepala token tanpa `alg`".into()),
    }
    if kepala.iter().any(|(k, _)| k == "crit") {
        return Err("kepala token memuat `crit`, perluasan yang tidak dikenal `jwt::` (RFC 7515 §4.1.11)".into());
    }
    let tanda = b64url_ketat(bagian[2], "tanda tangan")?;
    let harus = crate::dok::crypto::hmac_sha256(kunci, format!("{}.{}", bagian[0], bagian[1]).as_bytes());
    if !crate::dok::crypto::sama_waktu_tetap(&harus, &tanda) {
        return Err("tanda tangan tidak cocok — isinya diubah, atau rahasianya bukan yang dipakai menandatangani".into());
    }
    let muatan_b = b64url_ketat(bagian[1], "muatan")?;
    let muatan = objek_json(&muatan_b, "muatan")?;
    // Waktu: tanpa jam yang bisa dibaca, token berbatas waktu tidak bisa dinyatakan sah.
    for (klaim, kedaluwarsa) in [("exp", true), ("nbf", false)] {
        let Some((_, v)) = muatan.iter().find(|(k, _)| k == klaim) else { continue };
        let detik = match v {
            crate::json_inti::Json::Int(n) => *n,
            crate::json_inti::Json::Float(f) if f.is_finite() => f.floor() as i128,
            _ => return Err(format!("klaim `{}` harus angka detik Unix", klaim)),
        };
        let kini = kini.ok_or("jam sistem tidak bisa dibaca, jadi token berbatas waktu tidak bisa diperiksa")?;
        if kedaluwarsa && kini >= detik { return Err(format!("token kedaluwarsa (exp {}, sekarang {})", detik, kini)); }
        if !kedaluwarsa && kini < detik { return Err(format!("token belum berlaku (nbf {}, sekarang {})", detik, kini)); }
    }
    Ok(String::from_utf8(muatan_b).expect("sudah diperiksa UTF-8"))
}

#[cfg(test)]
mod tes {
    use super::*;

    const K: &[u8] = b"rahasia-yang-panjangnya-32-byte!";

    fn token(kepala: &str, muatan: &str) -> String {
        let isi = format!("{}.{}", b64url(kepala.as_bytes()), b64url(muatan.as_bytes()));
        format!("{}.{}", isi, b64url(&crate::dok::crypto::hmac_sha256(K, isi.as_bytes())))
    }

    #[test]
    fn token_sah_dan_waktunya() {
        assert_eq!(periksa(&token(KEPALA, r#"{"sub":"a","exp":200}"#), K, Some(100)).unwrap(), r#"{"sub":"a","exp":200}"#);
        assert!(periksa(&token(KEPALA, r#"{"exp":100}"#), K, Some(100)).unwrap_err().contains("kedaluwarsa"));
        assert!(periksa(&token(KEPALA, r#"{"nbf":150}"#), K, Some(100)).unwrap_err().contains("belum berlaku"));
        assert!(periksa(&token(KEPALA, r#"{"exp":"besok"}"#), K, Some(100)).unwrap_err().contains("angka detik"));
        assert!(periksa(&token(KEPALA, r#"{"exp":200}"#), K, None).unwrap_err().contains("jam sistem"));
    }

    #[test]
    fn serangan_klasik_ditolak() {
        // `alg: none` tanpa tanda tangan, dan alg lain yang ditandatangani dengan rahasia yang sama.
        let none = format!("{}.{}.", b64url(br#"{"alg":"none"}"#), b64url(br#"{"admin":true}"#));
        assert!(periksa(&none, K, Some(0)).unwrap_err().contains("`none` ditolak"));
        assert!(periksa(&token(r#"{"alg":"HS512"}"#, "{}"), K, Some(0)).unwrap_err().contains("`HS512` ditolak"));
        assert!(periksa(&token(r#"{"alg":"RS256"}"#, "{}"), K, Some(0)).unwrap_err().contains("`RS256` ditolak"));
        // Muatan diganti, tanda tangan lama dipakai ulang.
        let asli = token(KEPALA, r#"{"admin":false}"#);
        let mut b: Vec<&str> = asli.split('.').collect();
        let palsu = b64url(br#"{"admin":true}"#);
        b[1] = &palsu;
        assert!(periksa(&b.join("."), K, Some(0)).unwrap_err().contains("tidak cocok"));
        // Kunci kembar: dua pembaca bisa memilih `admin` yang berbeda.
        assert!(periksa(&token(KEPALA, r#"{"admin":false,"admin":true}"#), K, Some(0)).unwrap_err().contains("dua kali"));
        assert!(periksa(&token(r#"{"alg":"HS256","alg":"none"}"#, "{}"), K, Some(0)).unwrap_err().contains("dua kali"));
        assert!(periksa(&token(r#"{"alg":"HS256","crit":["x"]}"#, "{}"), K, Some(0)).unwrap_err().contains("crit"));
        // Ejaan kedua: padding dan alfabet base64 biasa.
        let t = token(KEPALA, "{}");
        assert!(periksa(&format!("{}=", t), K, Some(0)).is_err());
        assert!(periksa(&t.replace('-', "+"), K, Some(0)).is_err() || !t.contains('-'));
        assert!(periksa("a.b", K, Some(0)).unwrap_err().contains("tepat 3"));
    }
}
