//! `dok::` untuk web: cookie, sesi bertanda tangan, kata sandi, dan template.

use crate::interpreter::{err_value, ok_value, Value};

use super::crypto::{base64_decode_str, base64_encode_bytes, hmac_sha256, secure_random_bytes};

// ─────────────────────────────────────────────────────────────────────────
// Pembantu
// ─────────────────────────────────────────────────────────────────────────

/// Argumen yang ISINYA dipakai (nama, rahasia, kata sandi, template, path) harus
/// teks: dulu nilai apa pun di-Display, jadi `dok::password_hash(f)` meng-hash
/// teks `<closure>` dan `dok::cookie_get(req, nil)` mencari cookie bernama "nil".
fn teks_arg(v: Option<&Value>, nama: &str, peran: &str) -> Result<String, String> {
    match v {
        Some(Value::Str(s)) => Ok(s.clone()),
        Some(lain) => Err(crate::interpreter::pesan_arg_harus_teks(nama, peran, &lain.type_name())),
        None => Err(format!("{}: argumennya kurang", nama)),
    }
}

/// Argumen yang DIRENDER jadi teks (nilai cookie/header, isi yang di-escape).
fn render_arg(v: Option<&Value>, nama: &str) -> Result<String, String> {
    match v {
        Some(Value::Str(s)) => Ok(s.clone()),
        Some(lain) => Ok(format!("{}", lain)),
        None => Err(format!("{}: argumennya kurang", nama)),
    }
}

fn ladang<'a>(v: &'a Value, nama: &str) -> Option<&'a Value> {
    match v {
        Value::Struct { fields, .. } => fields.get(nama),
        _ => None,
    }
}

/// Base64 varian URL tanpa padding — aman ditaruh di dalam cookie, yang
/// melarang `=` tanpa tanda kutip dan memperlakukan `/` sebagai pemisah path.
fn b64url(b: &[u8]) -> String {
    base64_encode_bytes(b).replace('+', "-").replace('/', "_").trim_end_matches('=').to_string()
}

fn b64url_balik(s: &str) -> Result<Vec<u8>, String> {
    let mut t = s.replace('-', "+").replace('_', "/");
    while t.len() % 4 != 0 { t.push('='); }
    base64_decode_str(&t)
}

/// Perbandingan yang waktunya tidak bergantung pada isinya.
fn sama_waktu_tetap(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut beda = 0u8;
    for i in 0..a.len() { beda |= a[i] ^ b[i]; }
    beda == 0
}

// ─────────────────────────────────────────────────────────────────────────
// Cookie
// ─────────────────────────────────────────────────────────────────────────

/// `dok::cookie_get(req, nama)` → `Option<string>`.
pub fn cookie_get(args: &[Value]) -> Result<Value, String> {
    let req = args.get(0).ok_or("dok::cookie_get(req, nama)")?;
    // `DokRequest` dari servernya, atau dict yang ditulis tes handler; struct lain
    // (router, soket) dulu diterima dan dijawab `None`.
    if !matches!(req, Value::Struct { name, .. } if name == "DokRequest" || name == "dict") {
        return Err(format!("dok::cookie_get: argumen ke-1 harus request (`DokRequest`, atau dict \
                            ber-`headers`), dapat `{}`", req.type_name()));
    }
    let nama = teks_arg(args.get(1), "dok::cookie_get", "nama cookie-nya")?;
    let Some(Value::Array(hs)) = ladang(req, "headers") else {
        return Ok(Value::Enum { type_name: "Option".into(), variant: "None".into(), payload: None });
    };
    for h in hs.iter() {
        let Value::Tuple(t) = h else { continue };
        if t.len() != 2 { continue; }
        let (Value::Str(k), Value::Str(v)) = (&t[0], &t[1]) else { continue };
        if !k.eq_ignore_ascii_case("cookie") { continue; }
        for bagian in v.split(';') {
            let bagian = bagian.trim();
            let Some((bk, bv)) = bagian.split_once('=') else { continue };
            if bk.trim() == nama {
                return Ok(Value::Enum {
                    type_name: "Option".into(), variant: "Some".into(),
                    payload: Some(Box::new(Value::Str(bv.trim().to_string()))),
                });
            }
        }
    }
    Ok(Value::Enum { type_name: "Option".into(), variant: "None".into(), payload: None })
}

/// `dok::cookie_set(resp, nama, nilai, opsi?)` → respons baru.
pub fn cookie_set(args: &[Value]) -> Result<Value, String> {
    let resp = args.get(0).cloned().ok_or("dok::cookie_set(resp, nama, nilai, opsi?)")?;
    let nama = teks_arg(args.get(1), "dok::cookie_set", "nama cookie-nya")?;
    let nilai = render_arg(args.get(2), "dok::cookie_set")?;
    if nama.is_empty() || nama.contains(['=', ';', ' ']) {
        return Err(format!("dok::cookie_set: nama cookie `{}` tidak sah", nama));
    }

    let opsi = args.get(3);
    // Opsi yang salah nama atau salah tipe dulu DIABAIKAN diam-diam (`maxAge`,
    // `secure: "ya"`) — cookie-nya tetap terpasang, tanpa pengaturan yang ditulis.
    if let Some(o) = opsi {
        let Value::Struct { fields, .. } = o else {
            return Err(format!("dok::cookie_set: argumen ke-4 (opsi) harus dict, dapat `{}`", o.type_name()));
        };
        for (k, v) in fields.iter() {
            let sah = match k.as_str() {
                "path" | "domain" | "same_site" => matches!(v, Value::Str(_)),
                "max_age" => matches!(v, Value::Int(_)),
                "http_only" | "secure" => matches!(v, Value::Bool(_)),
                _ => return Err(format!("dok::cookie_set: opsi `{}` tidak dikenal — yang ada: \
                                         path, domain, max_age, http_only, secure, same_site", k)),
            };
            if !sah {
                return Err(format!("dok::cookie_set: opsi `{}` bertipe salah, dapat `{}`", k, v.type_name()));
            }
        }
    }
    let ambil = |k: &str| -> Option<Value> { opsi.and_then(|o| ladang(o, k).cloned()) };
    let bool_opsi = |k: &str, bawaan: bool| -> bool {
        match ambil(k) { Some(Value::Bool(b)) => b, _ => bawaan }
    };

    let mut c = format!("{}={}", nama, nilai);
    let path = match ambil("path") { Some(Value::Str(s)) => s, _ => "/".to_string() };
    c.push_str(&format!("; Path={}", path));
    if let Some(Value::Int(n)) = ambil("max_age") { c.push_str(&format!("; Max-Age={}", n)); }
    if let Some(Value::Str(d)) = ambil("domain") { c.push_str(&format!("; Domain={}", d)); }
    if bool_opsi("http_only", true) { c.push_str("; HttpOnly"); }
    if bool_opsi("secure", false) { c.push_str("; Secure"); }
    let same = match ambil("same_site") { Some(Value::Str(s)) => s, _ => "Lax".to_string() };
    if !same.is_empty() { c.push_str(&format!("; SameSite={}", same)); }

    tambah_header(resp, "Set-Cookie", &c, "dok::cookie_set")
}

/// `dok::header_set(resp, nama, nilai)` → respons baru.
pub fn header_set(args: &[Value]) -> Result<Value, String> {
    let resp = args.get(0).cloned().ok_or("dok::header_set(resp, nama, nilai)")?;
    let nama = teks_arg(args.get(1), "dok::header_set", "nama header-nya")?;
    let nilai = render_arg(args.get(2), "dok::header_set")?;
    tambah_header(resp, &nama, &nilai, "dok::header_set")
}

/// `fungsi`: nama yang DITULIS pemakai — dulu `cookie_set` melapor atas nama `header_set`.
fn tambah_header(resp: Value, nama: &str, nilai: &str, fungsi: &str) -> Result<Value, String> {
    let Value::Struct { name, fields } = resp else {
        return Err(format!("{}: argumen ke-1 harus respons (mis. dari `dok::text`), dapat `{}`",
                           fungsi, resp.type_name()));
    };
    if name != "DokResponse" {
        return Err(format!("{}: argumen ke-1 harus respons, dapat `{}`", fungsi, name));
    }
    let mut f = (*fields).clone();
    let mut hs = match f.get("headers") { Some(Value::Array(a)) => (**a).clone(), _ => Vec::new() };
    hs.push(Value::Tuple(vec![Value::Str(nama.to_string()), Value::Str(nilai.to_string())].into()));
    f.insert("headers".to_string(), Value::Array(hs.into()));
    Ok(Value::Struct { name, fields: f.into() })
}

// ─────────────────────────────────────────────────────────────────────────
// Sesi bertanda tangan
// ─────────────────────────────────────────────────────────────────────────

/// `dok::session_encode(data, rahasia)` → string `payload.tandatangan`.
pub fn session_encode(args: &[Value]) -> Result<Value, String> {
    let data = args.get(0).ok_or("dok::session_encode(data, rahasia)")?;
    let rahasia = teks_arg(args.get(1), "dok::session_encode", "rahasianya")?;
    if rahasia.len() < 16 {
        return Err("dok::session_encode: rahasianya minimal 16 karakter — kunci pendek \
                    bisa ditebak, dan tanda tangan yang bisa ditebak sama saja dengan \
                    tidak ada tanda tangan".to_string());
    }
    let json = super::json::encode(data)?;
    let muatan = b64url(json.as_bytes());
    let mac = hmac_sha256(rahasia.as_bytes(), muatan.as_bytes());
    Ok(Value::Str(format!("{}.{}", muatan, b64url(&mac))))
}

/// `dok::session_decode(teks, rahasia)` → `Result<data, string>`.
pub fn session_decode(args: &[Value]) -> Result<Value, String> {
    let teks = teks_arg(args.get(0), "dok::session_decode", "teks sesinya")?;
    let rahasia = teks_arg(args.get(1), "dok::session_decode", "rahasianya")?;
    let Some((muatan, tanda)) = teks.rsplit_once('.') else {
        return Ok(err_value("sesi tidak berbentuk `muatan.tandatangan`"));
    };
    let harusnya = b64url(&hmac_sha256(rahasia.as_bytes(), muatan.as_bytes()));
    if !sama_waktu_tetap(harusnya.as_bytes(), tanda.as_bytes()) {
        return Ok(err_value("tanda tangan sesi tidak cocok — isinya diubah, atau \
                             rahasianya bukan yang dipakai waktu menandatangani"));
    }
    let byte = match b64url_balik(muatan) { Ok(b) => b, Err(e) => return Ok(err_value(e)) };
    let json = match String::from_utf8(byte) {
        Ok(s) => s,
        Err(_) => return Ok(err_value("muatan sesi bukan UTF-8")),
    };
    match super::json::decode(&json) {
        Ok(v) => Ok(ok_value(v)),
        Err(e) => Ok(err_value(format!("muatan sesi bukan JSON yang sah: {}", e))),
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Kata sandi
// ─────────────────────────────────────────────────────────────────────────

/// Jumlah putaran PBKDF2 bawaan.
const PUTARAN_BAWAAN: u32 = 210_000;

/// PBKDF2-HMAC-SHA256, ditulis di sini karena satu-satunya bahan yang
/// dibutuhkannya — HMAC-SHA256 — sudah ada.
fn pbkdf2(kata: &[u8], garam: &[u8], putaran: u32, panjang: usize) -> Vec<u8> {
    let mut keluar = Vec::with_capacity(panjang);
    let mut blok: u32 = 1;
    while keluar.len() < panjang {
        let mut pesan = garam.to_vec();
        pesan.extend_from_slice(&blok.to_be_bytes());
        let mut u = hmac_sha256(kata, &pesan);
        let mut t = u;
        for _ in 1..putaran {
            u = hmac_sha256(kata, &u);
            for i in 0..32 { t[i] ^= u[i]; }
        }
        keluar.extend_from_slice(&t);
        blok += 1;
    }
    keluar.truncate(panjang);
    keluar
}

/// `dok::password_hash(kata, putaran?)` → string.
pub fn password_hash(args: &[Value]) -> Result<Value, String> {
    let kata = teks_arg(args.get(0), "dok::password_hash", "kata sandinya")?;
    let putaran = match args.get(1) {
        Some(Value::Int(n)) if *n >= 1_000 && *n <= 10_000_000 => *n as u32,
        Some(Value::Int(n)) => return Err(format!(
            "dok::password_hash: putaran {} di luar 1000..10000000 — terlalu sedikit berarti \
             tidak melindungi, terlalu banyak berarti login yang menggantung", n)),
        // Dulu putaran yang bukan bilangan bulat diam-diam jadi bawaan.
        Some(lain) => return Err(format!(
            "dok::password_hash: argumen ke-2 (putaran) harus bilangan bulat, dapat `{}`", lain.type_name())),
        None => PUTARAN_BAWAAN,
    };
    let garam = secure_random_bytes(16);
    let hash = pbkdf2(kata.as_bytes(), &garam, putaran, 32);
    Ok(Value::Str(format!(
        "pbkdf2$sha256${}${}${}", putaran, b64url(&garam), b64url(&hash))))
}

/// `dok::password_verify(kata, hash)` → bool.
pub fn password_verify(args: &[Value]) -> Result<Value, String> {
    let kata = teks_arg(args.get(0), "dok::password_verify", "kata sandinya")?;
    let simpan = teks_arg(args.get(1), "dok::password_verify", "hash-nya")?;
    let bagian: Vec<&str> = simpan.split('$').collect();
    if bagian.len() != 5 || bagian[0] != "pbkdf2" || bagian[1] != "sha256" {
        return Ok(Value::Bool(false));
    }
    let Ok(putaran) = bagian[2].parse::<u32>() else { return Ok(Value::Bool(false)) };
    if !(1_000..=10_000_000).contains(&putaran) { return Ok(Value::Bool(false)); }
    let (Ok(garam), Ok(harusnya)) = (b64url_balik(bagian[3]), b64url_balik(bagian[4])) else {
        return Ok(Value::Bool(false));
    };
    let hitung = pbkdf2(kata.as_bytes(), &garam, putaran, harusnya.len().max(1));
    Ok(Value::Bool(sama_waktu_tetap(&hitung, &harusnya)))
}

// ─────────────────────────────────────────────────────────────────────────
// Template
// ─────────────────────────────────────────────────────────────────────────

/// Loloskan HTML.
pub fn loloskan_html(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

/// `dok::escape_html(s)` → string.
pub fn escape_html(args: &[Value]) -> Result<Value, String> {
    let s = render_arg(args.get(0), "dok::escape_html")?;
    Ok(Value::Str(loloskan_html(&s)))
}

fn nilai_ke_teks(v: &Value) -> String {
    match v { Value::Str(s) => s.clone(), lain => format!("{}", lain) }
}

fn benar(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Nil => false,
        Value::Int(n) => *n != 0,
        Value::Float(f) => crate::interpreter::float_truthy(*f),
        Value::Str(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Enum { variant, .. } => variant != "None",
        _ => true,
    }
}

/// Cari `nama` di dalam `data`, mendukung `a.b.c` dan `.` (nilai sekarang).
fn cari(data: &[(&str, Value)], konteks: &Value, jalur: &str) -> Option<Value> {
    if jalur == "." { return Some(konteks.clone()); }
    let mut bagian = jalur.split('.');
    let pertama = bagian.next()?;
    let mut kini = data.iter().rev().find(|(k, _)| *k == pertama).map(|(_, v)| v.clone())
        .or_else(|| ladang(konteks, pertama).cloned())?;
    for b in bagian {
        kini = ladang(&kini, b).cloned()?;
    }
    Some(kini)
}

/// Data template harus dict atau struct: nilai lain dulu dibaca sebagai konteks KOSONG,
/// jadi `dok::render("halo {{ x }}", 5)` menjawab `halo ` tanpa sepatah kata.
fn data_template(v: Option<&Value>, fungsi: &str) -> Result<Value, String> {
    match v {
        None => Ok(Value::Nil),
        Some(d @ Value::Struct { .. }) => Ok(d.clone()),
        Some(lain) => Err(format!("{}: argumen ke-2 (datanya) harus dict atau struct, dapat `{}`",
                                  fungsi, lain.type_name())),
    }
}

/// `dok::render(template, data)` → `Result<string, string>`.
pub fn render(args: &[Value]) -> Result<Value, String> {
    let tpl = teks_arg(args.get(0), "dok::render", "template-nya")?;
    let data = data_template(args.get(1), "dok::render")?;
    match render_str(&tpl, &data) {
        Ok(s) => Ok(ok_value(Value::Str(s))),
        Err(e) => Ok(err_value(e)),
    }
}

/// `dok::render_file(path, data)` → `Result<string, string>`.
pub fn render_file(args: &[Value]) -> Result<Value, String> {
    let path = teks_arg(args.get(0), "dok::render_file", "path-nya")?;
    let data = data_template(args.get(1), "dok::render_file")?;
    let tpl = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => return Ok(err_value(format!("dok::render_file `{}`: {}", path, crate::host::galat_io(&e)))),
    };
    match render_str(&tpl, &data) {
        Ok(s) => Ok(ok_value(Value::Str(s))),
        Err(e) => Ok(err_value(e)),
    }
}

pub fn render_str(tpl: &str, data: &Value) -> Result<String, String> {
    let mut lingkup: Vec<(&str, Value)> = Vec::new();
    render_bagian(tpl, data, &mut lingkup, 0)
}

/// Batas kedalaman blok bersarang.
const MAKS_SARANG: usize = 64;

fn render_bagian(
    tpl: &str, konteks: &Value, lingkup: &mut Vec<(&str, Value)>, dalam: usize,
) -> Result<String, String> {
    if dalam > MAKS_SARANG {
        return Err(format!("dok::render: blok bersarang lebih dari {} tingkat", MAKS_SARANG));
    }
    let mut out = String::with_capacity(tpl.len());
    let mut sisa = tpl;
    while let Some(i) = sisa.find("{{") {
        out.push_str(&sisa[..i]);
        let ekor = &sisa[i + 2..];
        let Some(j) = ekor.find("}}") else {
            return Err("dok::render: ada `{{` yang tidak ditutup `}}`".to_string());
        };
        let isi = ekor[..j].trim();
        let setelah = &ekor[j + 2..];

        if let Some(nama) = isi.strip_prefix('!') {
            let _ = nama;
            sisa = setelah;
            continue;
        }
        if let Some(sisa_tag) = isi.strip_prefix('#') {
            let tag = sisa_tag.trim();
            let (kata, arg) = tag.split_once(char::is_whitespace)
                .map(|(a, b)| (a.trim(), b.trim()))
                .ok_or_else(|| format!("dok::render: blok `{{{{#{}}}}}` butuh nama", tag))?;
            let (badan, lanjut) = ambil_badan(setelah, kata)?;
            match kata {
                "if" => {
                    let (benar_bagian, salah_bagian) = pisah_else(badan);
                    let v = cari(lingkup, konteks, arg).unwrap_or(Value::Nil);
                    let dipakai = if benar(&v) { benar_bagian } else { salah_bagian };
                    out.push_str(&render_bagian(dipakai, konteks, lingkup, dalam + 1)?);
                }
                "each" => {
                    let v = cari(lingkup, konteks, arg).unwrap_or(Value::Nil);
                    // Tipenya diambil SEBELUM `v` dipindah oleh `let ...
                    let tipe = v.type_name();
                    let Value::Array(items) = v else {
                        return Err(format!(
                            "dok::render: `{{{{#each {}}}}}` butuh array, dapat `{}`",
                            arg, tipe));
                    };
                    for (idx, it) in items.iter().enumerate() {
                        lingkup.push(("@index", Value::Int(idx as i128)));
                        let s = render_bagian(badan, it, lingkup, dalam + 1)?;
                        lingkup.pop();
                        out.push_str(&s);
                    }
                }
                lain => return Err(format!("dok::render: blok `{}` tidak dikenal", lain)),
            }
            sisa = lanjut;
            continue;
        }
        if isi.starts_with('/') {
            return Err(format!("dok::render: `{{{{{}}}}}` tanpa blok pembuka", isi));
        }
        // Nilai.
        let (jalur, mentah) = match isi.strip_prefix('&') {
            Some(sisa_j) => (sisa_j.trim(), true),
            None => (isi, false),
        };
        let v = cari(lingkup, konteks, jalur);
        let teks = match v {
            Some(x) => nilai_ke_teks(&x),
            // Nama yang tidak ada jadi kosong, bukan galat.
            None => String::new(),
        };
        out.push_str(&if mentah { teks } else { loloskan_html(&teks) });
        sisa = setelah;
    }
    out.push_str(sisa);
    Ok(out)
}

/// Ambil badan sebuah blok sampai `{{/kata}}` yang sepadan, hormati sarang.
fn ambil_badan<'a>(s: &'a str, kata: &str) -> Result<(&'a str, &'a str), String> {
    let buka = format!("#{}", kata);
    let tutup = format!("/{}", kata);
    let mut dalam = 1usize;
    let mut pos = 0usize;
    while let Some(i) = s[pos..].find("{{") {
        let mulai = pos + i;
        let Some(j) = s[mulai + 2..].find("}}") else { break };
        let isi = s[mulai + 2..mulai + 2 + j].trim();
        let akhir = mulai + 2 + j + 2;
        let tanpa_spasi: String = isi.chars().filter(|c| !c.is_whitespace()).collect();
        if tanpa_spasi.starts_with(&buka) { dalam += 1; }
        else if tanpa_spasi == tutup {
            dalam -= 1;
            if dalam == 0 { return Ok((&s[..mulai], &s[akhir..])); }
        }
        pos = akhir;
    }
    Err(format!("dok::render: blok `{{{{#{}}}}}` tidak ditutup `{{{{/{}}}}}`", kata, kata))
}

/// Pisahkan badan `if` di `{{else}}` teratas.
fn pisah_else(badan: &str) -> (&str, &str) {
    let mut dalam = 0usize;
    let mut pos = 0usize;
    while let Some(i) = badan[pos..].find("{{") {
        let mulai = pos + i;
        let Some(j) = badan[mulai + 2..].find("}}") else { break };
        let isi = badan[mulai + 2..mulai + 2 + j].trim();
        let akhir = mulai + 2 + j + 2;
        if isi.starts_with('#') { dalam += 1; }
        else if isi.starts_with('/') { dalam = dalam.saturating_sub(1); }
        else if dalam == 0 && isi == "else" {
            return (&badan[..mulai], &badan[akhir..]);
        }
        pos = akhir;
    }
    (badan, "")
}

#[cfg(test)]
mod tes {
    use super::*;
    use std::collections::HashMap;

    fn obj(pasang: &[(&str, Value)]) -> Value {
        let mut f = HashMap::new();
        for (k, v) in pasang { f.insert(k.to_string(), v.clone()); }
        Value::Struct { name: "JsonObject".to_string(), fields: f.into() }
    }

    #[test]
    fn html_diloloskan_secara_bawaan() {
        let d = obj(&[("n", Value::Str("<script>alert(1)</script>".into()))]);
        let hasil = render_str("halo {{n}}", &d).unwrap();
        assert!(!hasil.contains("<script>"), "HTML tidak diloloskan: {}", hasil);
        assert!(hasil.contains("&lt;script&gt;"), "{}", hasil);
        // Dan yang mentah harus ditulis eksplisit.
        let mentah = render_str("halo {{& n}}", &d).unwrap();
        assert!(mentah.contains("<script>"), "{}", mentah);
    }

    #[test]
    fn blok_bersarang_dan_else() {
        let d = obj(&[
            ("ada", Value::Bool(true)),
            ("kosong", Value::Bool(false)),
            ("xs", Value::Array(vec![Value::Int(1), Value::Int(2)].into())),
        ]);
        assert_eq!(render_str("{{#if ada}}Y{{else}}N{{/if}}", &d).unwrap(), "Y");
        assert_eq!(render_str("{{#if kosong}}Y{{else}}N{{/if}}", &d).unwrap(), "N");
        assert_eq!(render_str("{{#each xs}}[{{.}}:{{@index}}]{{/each}}", &d).unwrap(),
                   "[1:0][2:1]");
        // `if` di dalam `each`, dan penutup yang sepadan.
        assert_eq!(
            render_str("{{#each xs}}{{#if .}}{{.}}{{/if}}{{/each}}", &d).unwrap(), "12");
    }

    #[test]
    fn blok_tak_ditutup_ditolak_bukan_diam() {
        assert!(render_str("{{#if x}}halo", &obj(&[])).is_err());
        assert!(render_str("halo {{x", &obj(&[])).is_err());
        assert!(render_str("{{/if}}", &obj(&[])).is_err());
    }

    #[test]
    fn pbkdf2_cocok_dengan_vektor_rfc6070_gaya_sha256() {
        // Vektor uji PBKDF2-HMAC-SHA256 yang dikenal luas (password "password",
        // salt "salt", 1 putaran, 32 byte).
        let h = pbkdf2(b"password", b"salt", 1, 32);
        let hex: String = h.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(hex, "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b");
    }

    #[test]
    fn sandi_diverifikasi_dan_garamnya_berbeda() {
        // Putaran kecil supaya tesnya tidak jadi tolok ukur.
        // `Value` tidak punya `PartialEq` — dibandingkan lewat isinya, bukan
        // lewat `assert_eq!`.
        fn teks(v: &Value) -> String { match v { Value::Str(s) => s.clone(), l => format!("{}", l) } }
        fn benar_bool(v: &Value) -> bool { matches!(v, Value::Bool(true)) }

        let a = password_hash(&[Value::Str("rahasia".into()), Value::Int(1000)]).unwrap();
        let b = password_hash(&[Value::Str("rahasia".into()), Value::Int(1000)]).unwrap();
        assert_ne!(teks(&a), teks(&b),
            "dua hash sandi yang sama identik — garamnya tidak dipakai");
        let sa = teks(&a);
        assert!(benar_bool(&password_verify(
            &[Value::Str("rahasia".into()), Value::Str(sa.clone())]).unwrap()),
            "sandi yang benar tidak diterima");
        assert!(!benar_bool(&password_verify(
            &[Value::Str("salah".into()), Value::Str(sa.clone())]).unwrap()),
            "sandi yang salah diterima");
        // Hash yang bentuknya rusak menjawab false, bukan panik atau true.
        for rusak in ["", "bukan-hash", "pbkdf2$sha256$x$y$z", "pbkdf2$sha256$1000$!!$!!"] {
            assert!(!benar_bool(&password_verify(
                &[Value::Str("rahasia".into()), Value::Str(rusak.into())]).unwrap()),
                "hash rusak `{}` tidak menjawab false", rusak);
        }
    }

    #[test]
    fn sesi_menolak_muatan_yang_diubah() {
        let d = obj(&[("user", Value::Str("budi".into()))]);
        let rahasia = Value::Str("rahasia-yang-cukup-panjang".into());
        let Value::Str(t) = session_encode(&[d.clone(), rahasia.clone()]).unwrap() else { panic!() };
        // Yang utuh terbaca kembali.
        let ok = session_decode(&[Value::Str(t.clone()), rahasia.clone()]).unwrap();
        assert!(matches!(&ok, Value::Enum { variant, .. } if variant == "Ok"), "{:?}", ok);
        // Muatan yang diubah ditolak, walau bentuknya masih sah.
        let (muatan, tanda) = t.rsplit_once('.').unwrap();
        let palsu = format!("{}.{}", b64url(br#"{"user":"admin"}"#), tanda);
        let hasil = session_decode(&[Value::Str(palsu), rahasia.clone()]).unwrap();
        assert!(matches!(&hasil, Value::Enum { variant, .. } if variant == "Err"),
                "muatan yang diubah diterima: {:?}", hasil);
        let _ = muatan;
        // Rahasia yang salah juga ditolak.
        let salah = session_decode(&[
            Value::Str(t), Value::Str("rahasia-lain-yang-panjang".into())]).unwrap();
        assert!(matches!(&salah, Value::Enum { variant, .. } if variant == "Err"), "{:?}", salah);
    }

    #[test]
    fn rahasia_pendek_ditolak() {
        let d = obj(&[]);
        assert!(session_encode(&[d, Value::Str("pendek".into())]).is_err());
    }
}
