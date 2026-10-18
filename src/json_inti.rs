// src/json_inti.rs — SATU pengurai dan penyandi JSON: `json::parse`, `dok::json_decode`, dan
// runtime backend LLVM (salinan persisnya di `runtime/src/json_inti.rs`, dijaga tes).
// Ketat: yang bukan JSON ditolak dengan kalimat yang menyebut tempatnya, tidak ditebak.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Pohon hasil urai, sebelum dijadikan nilai milik masing-masing mesin.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i128),
    Float(f64),
    Str(String),
    Arr(Vec<Json>),
    /// Pasangan dalam urutan tulis; kunci kembar dibiarkan, pemakainya yang memutuskan.
    Obj(Vec<(String, Json)>),
}

const MAKS_DALAM: usize = 512;

/// Urai seluruh `s` sebagai SATU nilai JSON.
pub fn urai(s: &str) -> Result<Json, String> {
    let b = s.as_bytes();
    let mut pos = 0usize;
    let v = nilai(s, b, &mut pos, 0)?;
    lewati_spasi(b, &mut pos);
    if pos < b.len() {
        return Err(format!("ada teks berlebih sesudah nilainya, mulai posisi {}: `{}`", pos, potongan(s, pos)));
    }
    Ok(v)
}

/// Sampai 16 karakter mulai `pos`, untuk ditunjukkan di pesan.
fn potongan(s: &str, pos: usize) -> String {
    s.get(pos..).unwrap_or("").chars().take(16).collect()
}

fn lewati_spasi(b: &[u8], pos: &mut usize) {
    while *pos < b.len() && matches!(b[*pos], b' ' | b'\t' | b'\n' | b'\r') { *pos += 1; }
}

fn nilai(s: &str, b: &[u8], pos: &mut usize, dalam: usize) -> Result<Json, String> {
    if dalam > MAKS_DALAM {
        return Err(format!("JSON bersarang lebih dari {} tingkat", MAKS_DALAM));
    }
    lewati_spasi(b, pos);
    if *pos >= b.len() { return Err("teksnya habis sebelum nilainya lengkap".to_string()); }
    match b[*pos] {
        b'{' => objek(s, b, pos, dalam),
        b'[' => larik(s, b, pos, dalam),
        b'"' => teks(s, b, pos).map(Json::Str),
        b't' => kata(s, b, pos, "true").map(|_| Json::Bool(true)),
        b'f' => kata(s, b, pos, "false").map(|_| Json::Bool(false)),
        b'n' => kata(s, b, pos, "null").map(|_| Json::Null),
        b'-' | b'0'..=b'9' => angka(s, b, pos),
        _ => Err(format!("karakter `{}` tidak bisa mengawali sebuah nilai",
                         s.get(*pos..).and_then(|r| r.chars().next()).unwrap_or('?'))),
    }
}

fn kata(s: &str, b: &[u8], pos: &mut usize, lit: &str) -> Result<(), String> {
    let akhir = *pos + lit.len();
    let pas = akhir <= b.len() && &b[*pos..akhir] == lit.as_bytes()
        && !b.get(akhir).map_or(false, |c| c.is_ascii_alphanumeric() || *c == b'_');
    if !pas {
        return Err(format!("harusnya `{}` di posisi {}, yang tertulis `{}`", lit, *pos, potongan(s, *pos)));
    }
    *pos = akhir;
    Ok(())
}

fn objek(s: &str, b: &[u8], pos: &mut usize, dalam: usize) -> Result<Json, String> {
    *pos += 1;
    let mut isi = Vec::new();
    lewati_spasi(b, pos);
    if b.get(*pos) == Some(&b'}') { *pos += 1; return Ok(Json::Obj(isi)); }
    loop {
        lewati_spasi(b, pos);
        if b.get(*pos) != Some(&b'"') {
            return Err(if *pos >= b.len() { "objek tidak ditutup dengan `}`".to_string() }
                       else { "harusnya ada `\"` untuk mengawali kunci".to_string() });
        }
        let kunci = teks(s, b, pos)?;
        lewati_spasi(b, pos);
        if b.get(*pos) != Some(&b':') { return Err("harusnya ada `:` sesudah kunci".to_string()); }
        *pos += 1;
        let v = nilai(s, b, pos, dalam + 1)?;
        isi.push((kunci, v));
        lewati_spasi(b, pos);
        match b.get(*pos) {
            Some(b',') => *pos += 1,
            Some(b'}') => { *pos += 1; return Ok(Json::Obj(isi)); }
            None => return Err("objek tidak ditutup dengan `}`".to_string()),
            Some(_) => return Err("harusnya ada `,` atau `}` sesudah nilai".to_string()),
        }
    }
}

fn larik(s: &str, b: &[u8], pos: &mut usize, dalam: usize) -> Result<Json, String> {
    *pos += 1;
    let mut isi = Vec::new();
    lewati_spasi(b, pos);
    if b.get(*pos) == Some(&b']') { *pos += 1; return Ok(Json::Arr(isi)); }
    loop {
        isi.push(nilai(s, b, pos, dalam + 1)?);
        lewati_spasi(b, pos);
        match b.get(*pos) {
            Some(b',') => *pos += 1,
            Some(b']') => { *pos += 1; return Ok(Json::Arr(isi)); }
            None => return Err("array tidak ditutup dengan `]`".to_string()),
            Some(_) => return Err("harusnya ada `,` atau `]` sesudah elemen".to_string()),
        }
    }
}

/// Empat digit heksadesimal sesudah `\u`.
fn heks4(b: &[u8], pos: &mut usize) -> Result<u32, String> {
    let galat = || "escape `\\u` butuh empat digit heksadesimal".to_string();
    let empat = b.get(*pos..*pos + 4).ok_or_else(galat)?;
    let t = core::str::from_utf8(empat).map_err(|_| galat())?;
    if !t.bytes().all(|c| c.is_ascii_hexdigit()) { return Err(galat()); }
    *pos += 4;
    u32::from_str_radix(t, 16).map_err(|_| galat())
}

fn teks(s: &str, b: &[u8], pos: &mut usize) -> Result<String, String> {
    *pos += 1;
    let mut keluar = String::new();
    loop {
        // Bita biasa disalin per potongan UTF-8 utuh, bukan bita demi bita.
        let mulai = *pos;
        while *pos < b.len() && b[*pos] != b'"' && b[*pos] != b'\\' { *pos += 1; }
        keluar.push_str(s.get(mulai..*pos).unwrap_or(""));
        match b.get(*pos) {
            None => return Err("teks tidak ditutup dengan `\"`".to_string()),
            Some(b'"') => { *pos += 1; return Ok(keluar); }
            Some(_) => {
                *pos += 1;
                let Some(&e) = b.get(*pos) else {
                    return Err("teksnya habis di tengah escape `\\`".to_string());
                };
                *pos += 1;
                match e {
                    b'"' => keluar.push('"'),
                    b'\\' => keluar.push('\\'),
                    b'/' => keluar.push('/'),
                    b'n' => keluar.push('\n'),
                    b'r' => keluar.push('\r'),
                    b't' => keluar.push('\t'),
                    b'b' => keluar.push('\u{8}'),
                    b'f' => keluar.push('\u{c}'),
                    b'u' => {
                        let n = heks4(b, pos)?;
                        let kode = if (0xD800..0xDC00).contains(&n) {
                            // Separuh atas pasangan surrogate: separuh bawahnya harus menyusul.
                            if b.get(*pos) == Some(&b'\\') && b.get(*pos + 1) == Some(&b'u') {
                                *pos += 2;
                                let bawah = heks4(b, pos)?;
                                if !(0xDC00..0xE000).contains(&bawah) {
                                    return Err(format!("escape `\\u{:04X}` bukan separuh bawah pasangan surrogate", bawah));
                                }
                                0x10000 + ((n - 0xD800) << 10) + (bawah - 0xDC00)
                            } else {
                                return Err(format!("escape `\\u{:04X}` adalah separuh pasangan surrogate tanpa pasangannya", n));
                            }
                        } else { n };
                        match char::from_u32(kode) {
                            Some(c) => keluar.push(c),
                            None => return Err(format!("escape `\\u{:04X}` bukan karakter Unicode", n)),
                        }
                    }
                    lain => {
                        let c = s.get(*pos - 1..).and_then(|r| r.chars().next()).unwrap_or(lain as char);
                        return Err(format!("escape `\\{}` tidak dikenal JSON", c));
                    }
                }
            }
        }
    }
}

fn angka(s: &str, b: &[u8], pos: &mut usize) -> Result<Json, String> {
    let mulai = *pos;
    let mut pecahan = false;
    if b.get(*pos) == Some(&b'-') { *pos += 1; }
    let digit_mulai = *pos;
    while *pos < b.len() && b[*pos].is_ascii_digit() { *pos += 1; }
    let digit = *pos - digit_mulai;
    let tak_lengkap = |pos: usize| format!("angka tidak lengkap: `{}`", s.get(mulai..pos).unwrap_or(""));
    if digit == 0 { return Err(tak_lengkap(*pos)); }
    if digit > 1 && b[digit_mulai] == b'0' {
        return Err(format!("angka JSON tidak boleh berawalan nol: `{}`", s.get(mulai..*pos).unwrap_or("")));
    }
    if b.get(*pos) == Some(&b'.') {
        pecahan = true;
        *pos += 1;
        let m = *pos;
        while *pos < b.len() && b[*pos].is_ascii_digit() { *pos += 1; }
        if *pos == m { return Err(tak_lengkap(*pos)); }
    }
    if matches!(b.get(*pos), Some(b'e') | Some(b'E')) {
        pecahan = true;
        *pos += 1;
        if matches!(b.get(*pos), Some(b'+') | Some(b'-')) { *pos += 1; }
        let m = *pos;
        while *pos < b.len() && b[*pos].is_ascii_digit() { *pos += 1; }
        if *pos == m { return Err(tak_lengkap(*pos)); }
    }
    let t = s.get(mulai..*pos).unwrap_or("");
    if pecahan {
        match t.parse::<f64>() {
            Ok(f) if f.is_finite() => Ok(Json::Float(f)),
            Ok(_) => Err(format!("pecahan `{}` di luar jangkauan 64 bit", t)),
            Err(_) => Err(format!("`{}` bukan angka", t)),
        }
    } else {
        t.parse::<i128>().map(Json::Int)
            .map_err(|_| format!("bilangan bulat `{}` di luar jangkauan 128 bit", t))
    }
}

/// Teks sebagai literal JSON, berkutip.
pub fn sandi_teks(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Pecahan sebagai angka JSON; `None` untuk NaN dan tak hingga, yang tidak punya padanan.
pub fn sandi_pecahan(f: f64) -> Option<String> {
    if f.is_nan() || f.is_infinite() { return None; }
    // Tanpa `fract()`/`abs()`: keduanya tidak ada di `core`, dan berkas ini juga milik runtime.
    let bulat = f == (f as i64) as f64 && f < 1e15 && f > -1e15;
    Some(if bulat { format!("{:.1}", f) } else { f.to_string() })
}

/// Kalimat untuk nilai yang tidak bisa disandikan.
pub fn pesan_tak_bisa(apa: &str, jalan: &str) -> String {
    format!(
        "`{a}` tidak punya padanan di JSON, jadi ia tidak bisa disandikan\nbantuan: {j}\n\
         catatan: menulisnya apa adanya menghasilkan teks yang bukan JSON, dan \
         menggantinya `null` diam-diam adalah jawaban salah yang tidak melapor",
        a = apa, j = jalan)
}

/// Jalan keluar untuk fungsi, channel, dan pegangan `Atomic`/`Mutex` — satu definisi untuk
/// tiga mesin (backend LLVM menolaknya saat kompilasi dengan kalimat yang sama).
pub const JALAN_FUNGSI: &str = "kirim hasil pemanggilannya, atau sebuah nama yang mewakilinya, bukan fungsinya sendiri";
pub const JALAN_CHANNEL: &str = "kirim isinya, bukan channelnya";
pub const JALAN_PEGANGAN: &str = "kirim nilai yang dijaganya, bukan pegangannya";

/// Jalan keluar yang disebut untuk pecahan tak terhingga atau NaN.
pub const JALAN_PECAHAN: &str = "saring dulu dengan `math::is_finite(x)`, atau ganti sendiri dengan \
    nilai yang kamu maksud sebelum menyandikannya";

/// Tulis pohon `Json`: rapat kalau `indent` `None`, selain itu rapi dengan lebar itu.
pub fn tulis(j: &Json, indent: Option<usize>, dalam: usize) -> Result<String, String> {
    let rapat = |x: &Json| tulis(x, None, 0);
    match j {
        Json::Null => Ok("null".to_string()),
        Json::Bool(b) => Ok(b.to_string()),
        Json::Int(n) => Ok(n.to_string()),
        Json::Float(f) => sandi_pecahan(*f).ok_or_else(|| pesan_tak_bisa(&format!("{}", f), JALAN_PECAHAN)),
        Json::Str(s) => Ok(sandi_teks(s)),
        Json::Arr(a) => match indent {
            None => {
                let isi: Result<Vec<String>, String> = a.iter().map(rapat).collect();
                Ok(format!("[{}]", isi?.join(",")))
            }
            Some(_) if a.is_empty() => Ok("[]".to_string()),
            Some(n) => {
                let (pad, dlm) = (" ".repeat(dalam * n), " ".repeat((dalam + 1) * n));
                let mut isi = Vec::new();
                for x in a { isi.push(format!("{}{}", dlm, tulis(x, indent, dalam + 1)?)); }
                Ok(format!("[\n{}\n{}]", isi.join(",\n"), pad))
            }
        },
        Json::Obj(o) => match indent {
            None => {
                let mut isi = Vec::new();
                for (k, v) in o { isi.push(format!("{}:{}", sandi_teks(k), rapat(v)?)); }
                Ok(format!("{{{}}}", isi.join(",")))
            }
            Some(_) if o.is_empty() => Ok("{}".to_string()),
            Some(n) => {
                let (pad, dlm) = (" ".repeat(dalam * n), " ".repeat((dalam + 1) * n));
                let mut isi = Vec::new();
                for (k, v) in o { isi.push(format!("{}{}: {}", dlm, sandi_teks(k), tulis(v, indent, dalam + 1)?)); }
                Ok(format!("{{\n{}\n{}}}", isi.join(",\n"), pad))
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yang_sah_terurai() {
        assert_eq!(urai(" [1, -2.5, \"é😀\", true, null, {\"a\": []}] ").unwrap(),
            Json::Arr(vec![Json::Int(1), Json::Float(-2.5), Json::Str("é😀".into()), Json::Bool(true),
                           Json::Null, Json::Obj(vec![("a".into(), Json::Arr(vec![]))])]));
        assert_eq!(urai("\"\\ud83d\\ude00\\u00e9\\n\"").unwrap(), Json::Str("😀é\n".into()));
        assert_eq!(urai("1e3").unwrap(), Json::Float(1000.0));
        assert_eq!(urai("-0").unwrap(), Json::Int(0));
    }

    #[test]
    fn penyandi_meng_escape_dan_menolak_nan() {
        assert_eq!(sandi_teks("a\"b\\\n\u{1}é"), "\"a\\\"b\\\\\\n\\u0001é\"");
        assert_eq!(sandi_pecahan(2.0).as_deref(), Some("2.0"));
        assert_eq!(sandi_pecahan(2.5).as_deref(), Some("2.5"));
        assert_eq!(sandi_pecahan(1e20).as_deref(), Some("100000000000000000000"));
        assert!(sandi_pecahan(f64::NAN).is_none());
        let j = urai("{\"a\":[1,2.0,\"x\"],\"b\":{}}").unwrap();
        assert_eq!(tulis(&j, None, 0).unwrap(), "{\"a\":[1,2.0,\"x\"],\"b\":{}}");
        assert_eq!(tulis(&j, Some(2), 0).unwrap(), "{\n  \"a\": [\n    1,\n    2.0,\n    \"x\"\n  ],\n  \"b\": {}\n}");
    }

    #[test]
    fn yang_bukan_json_ditolak() {
        for salah in ["tidak", "fx", "nul", "truex", "1 2", "[1] x", "{", "[1,", "01", "-", "1.", "1e",
                      "\"\\ud83d\"", "\"\\uZZZZ\"", "\"\\x\"", "\"a", "{\"a\":1,}", "[1 2]", "", "{\"a\" 1}"] {
            assert!(urai(salah).is_err(), "`{}` seharusnya ditolak", salah);
        }
        let dalam = "[".repeat(600);
        assert!(urai(&dalam).unwrap_err().contains("bersarang"));
    }
}
