// src/dok/form.rs — badan formulir (`dok::form`, `dok::multipart`), nama berkas unggahan, pengalihan lokal, `Host`.
// Bentuk yang dibaca lain oleh pengurai lain (kelas pengurai multipart bahasa lain) jadi `Err`, bukan ditebak.

use std::collections::HashMap;

use crate::interpreter::{err_value, ok_value, Value};

/// Bagian multipart paling banyak (kelas CVE-2023-24536 Go `mime/multipart`, CVE-2023-25577 Werkzeug).
pub const MAKS_BAGIAN: usize = 1000;
/// Kepala satu bagian paling besar, dalam byte, dan paling banyak barisnya.
pub const MAKS_KEPALA_BAGIAN: usize = 8 * 1024;
const MAKS_BARIS_KEPALA: usize = 16;

fn ladang<'a>(v: &'a Value, nama: &str) -> Option<&'a Value> {
    match v { Value::Struct { fields, .. } => fields.get(nama), _ => None }
}

fn dict(isi: Vec<(&str, Value)>) -> Value {
    let m: HashMap<String, Value> = isi.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    Value::Struct { name: "dict".to_string(), fields: m.into() }
}

/// Permintaan dari `dok::serve`, atau dict ber-`headers` yang ditulis tes handler.
fn req_arg<'a>(a: &'a [Value], nama: &str) -> Result<&'a Value, String> {
    match a.first() {
        Some(v @ Value::Struct { name, .. }) if name == "DokRequest" || name == "dict" => Ok(v),
        Some(lain) => Err(format!("{}: argumen ke-1 harus request (`DokRequest`, atau dict ber-`headers`), dapat `{}`",
                                  nama, lain.type_name())),
        None => Err(format!("{}: argumen ke-1 wajib diisi", nama)),
    }
}

/// Nilai kepala `nama` (huruf apa pun); dua kepala bernama sama jadi `Err` — server lain bisa memilih yang lain.
fn kepala(req: &Value, nama: &str) -> Result<Option<String>, String> {
    let mut ketemu = None;
    if let Some(Value::Array(hs)) = ladang(req, "headers") {
        for h in hs.iter() {
            let Value::Tuple(t) = h else { continue };
            let (Some(Value::Str(k)), Some(Value::Str(v))) = (t.get(0), t.get(1)) else { continue };
            if !k.eq_ignore_ascii_case(nama) { continue; }
            if ketemu.is_some() { return Err(format!("kepala `{}` dikirim dua kali — ambigu, ditolak", nama)); }
            ketemu = Some(v.to_string());
        }
    }
    Ok(ketemu)
}

/// Badan sebagai byte: `body` kalau teks, `body_bytes` kalau bukan.
fn badan(req: &Value) -> Result<Vec<u8>, String> {
    match (ladang(req, "body"), ladang(req, "body_bytes")) {
        (Some(Value::Str(s)), _) => Ok(s.as_bytes().to_vec()),
        (_, Some(Value::Array(b))) => super::crypto::array_ke_byte(b, "body_bytes"),
        _ => Ok(Vec::new()),
    }
}

/// `tipe/sub; k=v; k="v"` → (tipe huruf kecil, parameter); parameter kembar jadi `Err`.
fn urai_jenis(s: &str) -> Result<(String, Vec<(String, String)>), String> {
    let mut it = s.splitn(2, ';');
    let jenis = it.next().unwrap_or("").trim().to_ascii_lowercase();
    let mut ps: Vec<(String, String)> = Vec::new();
    let mut sisa = it.next().unwrap_or("");
    loop {
        sisa = sisa.trim_start_matches([' ', '\t', ';']);
        if sisa.is_empty() { break; }
        let eq = sisa.find('=').ok_or_else(|| format!("parameter `{}` tanpa `=`", sisa.trim()))?;
        let k = sisa[..eq].trim().to_ascii_lowercase();
        if k.is_empty() || !k.bytes().all(|c| c.is_ascii_alphanumeric() || b"!#$&+-.^_`|~*".contains(&c)) {
            return Err(format!("nama parameter `{}` tidak sah", k));
        }
        let r = &sisa[eq + 1..];
        let (v, lanjut) = if let Some(q) = r.strip_prefix('"') {
            let tutup = q.find('"').ok_or_else(|| format!("nilai parameter `{}` tidak ditutup `\"`", k))?;
            let v = &q[..tutup];
            // `\"` dibaca lain oleh tiap pengurai: ada yang meloloskan, ada yang menutup di situ.
            if v.ends_with('\\') { return Err(format!("nilai parameter `{}` memuat `\\\"` — ambigu, ditolak", k)); }
            (v.to_string(), &q[tutup + 1..])
        } else {
            let akhir = r.find(';').unwrap_or(r.len());
            (r[..akhir].trim().to_string(), &r[akhir..])
        };
        if v.bytes().any(|c| c < 0x20 && c != b'\t' || c == 0x7f) {
            return Err(format!("nilai parameter `{}` memuat karakter kendali", k));
        }
        if ps.iter().any(|(pk, _)| *pk == k) { return Err(format!("parameter `{}` ditulis dua kali — ambigu, ditolak", k)); }
        ps.push((k, v));
        let l = lanjut.trim_start_matches([' ', '\t']);
        if !l.is_empty() && !l.starts_with(';') { return Err(format!("ada teks liar sesudah parameter `{}`", ps.last().unwrap().0)); }
        sisa = l;
    }
    Ok((jenis, ps))
}

fn param<'a>(ps: &'a [(String, String)], k: &str) -> Option<&'a str> {
    ps.iter().find(|(pk, _)| pk == k).map(|(_, v)| v.as_str())
}

/// `dok::form(req)` → `Result<array>` berisi `(kunci, nilai)` urut tulis, bentuk yang sama dengan `req.query`.
pub fn form(a: &[Value]) -> Result<Value, String> {
    let nama = "dok::form";
    let req = req_arg(a, nama)?;
    let hasil = (|| {
        let ct = kepala(req, "content-type")?.ok_or("permintaannya tanpa `Content-Type`")?;
        let (jenis, ps) = urai_jenis(&ct)?;
        if jenis != "application/x-www-form-urlencoded" {
            return Err(format!("`Content-Type`-nya `{}`, bukan `application/x-www-form-urlencoded`{}", jenis,
                               if jenis == "multipart/form-data" { " — baca dengan `dok::multipart`" } else { "" }));
        }
        if let Some(cs) = param(&ps, "charset") {
            if !cs.eq_ignore_ascii_case("utf-8") { return Err(format!("charset `{}` tidak didukung, cuma UTF-8", cs)); }
        }
        let b = badan(req)?;
        let teks = std::str::from_utf8(&b).map_err(|_| "badannya bukan teks".to_string())?;
        crate::stdlib::url::urai_query(teks)
    })();
    Ok(match hasil {
        Ok(ps) => ok_value(Value::Array(ps.into_iter()
            .map(|(k, v)| Value::Tuple(vec![Value::Str(std::sync::Arc::new(k)), Value::Str(std::sync::Arc::new(v))].into())).collect::<Vec<_>>().into())),
        Err(e) => err_value(format!("{}: {}", nama, e)),
    })
}

/// Boundary RFC 2046 §5.1.1: 1–70 karakter `bchars`, tidak diakhiri spasi.
fn boundary_sah(b: &str) -> bool {
    !b.is_empty() && b.len() <= 70 && !b.ends_with(' ')
        && b.bytes().all(|c| c.is_ascii_alphanumeric() || b"'()+_,-./:=? ".contains(&c))
}

fn cari(h: &[u8], jarum: &[u8], dari: usize) -> Option<usize> {
    if dari > h.len() { return None; }
    h[dari..].windows(jarum.len()).position(|w| w == jarum).map(|p| p + dari)
}

/// Satu bagian yang sudah diurai.
pub struct Bagian {
    pub nama: String,
    pub berkas: Option<String>,
    pub tipe: String,
    pub isi: Vec<u8>,
}

/// Kepala satu bagian: `Content-Disposition` wajib sekali, `Content-Type` paling banyak sekali.
fn urai_kepala_bagian(k: &[u8], ke: usize) -> Result<(String, Option<String>, String), String> {
    let teks = std::str::from_utf8(k).map_err(|_| format!("kepala bagian ke-{} bukan teks UTF-8", ke))?;
    let mut disp = None;
    let mut tipe = None;
    let baris: Vec<&str> = if teks.is_empty() { Vec::new() } else { teks.split("\r\n").collect() };
    if baris.len() > MAKS_BARIS_KEPALA { return Err(format!("kepala bagian ke-{} lebih dari {} baris", ke, MAKS_BARIS_KEPALA)); }
    for b in baris {
        if b.starts_with([' ', '\t']) { return Err(format!("kepala bagian ke-{} memakai lipatan baris — ditolak", ke)); }
        if b.contains('\n') || b.contains('\r') { return Err(format!("kepala bagian ke-{} memuat CR/LF telanjang", ke)); }
        let (n, v) = b.split_once(':').ok_or_else(|| format!("baris kepala bagian ke-{} tanpa `:`", ke))?;
        if n.is_empty() || n.ends_with([' ', '\t']) {
            return Err(format!("nama kepala `{}` di bagian ke-{} tidak sah", n, ke));
        }
        let v = v.trim().to_string();
        let slot = match n.to_ascii_lowercase().as_str() {
            "content-disposition" => &mut disp,
            "content-type" => &mut tipe,
            "content-transfer-encoding" => {
                if !matches!(v.to_ascii_lowercase().as_str(), "7bit" | "8bit" | "binary") {
                    return Err(format!("`Content-Transfer-Encoding: {}` di bagian ke-{} tidak didukung (RFC 7578 §4.7)", v, ke));
                }
                continue;
            }
            _ => continue,
        };
        if slot.is_some() { return Err(format!("kepala `{}` di bagian ke-{} ditulis dua kali — ambigu, ditolak", n, ke)); }
        *slot = Some(v);
    }
    let disp = disp.ok_or_else(|| format!("bagian ke-{} tanpa `Content-Disposition`", ke))?;
    let (jenis, ps) = urai_jenis(&disp).map_err(|e| format!("`Content-Disposition` bagian ke-{}: {}", ke, e))?;
    if jenis != "form-data" { return Err(format!("`Content-Disposition` bagian ke-{} `{}`, bukan `form-data`", ke, jenis)); }
    if param(&ps, "filename*").is_some() {
        return Err(format!("bagian ke-{} memakai `filename*` — RFC 7578 §4.2 melarangnya, dan pengurai lain memilih \
                            nama yang berbeda", ke));
    }
    let nama = param(&ps, "name").ok_or_else(|| format!("bagian ke-{} tanpa parameter `name`", ke))?.to_string();
    let berkas = param(&ps, "filename").map(str::to_string);
    let tipe = tipe.unwrap_or_else(|| "text/plain".into());
    Ok((nama, berkas, tipe))
}

/// Urai badan `multipart/form-data` (RFC 7578) berpembatas `boundary`; baris wajib CRLF.
pub fn urai_multipart(b: &[u8], boundary: &str) -> Result<Vec<Bagian>, String> {
    let pembatas = format!("--{}", boundary).into_bytes();
    let mut sela = b"\r\n".to_vec();
    sela.extend_from_slice(&pembatas);
    // Pembatas pertama: di awal badan, atau sesudah CRLF pembuka (preamble diabaikan, RFC 2046).
    let mut pos = if b.starts_with(&pembatas) { 0 } else {
        cari(b, &sela, 0).ok_or("pembatas pertamanya tidak ditemukan")? + 2
    };
    let mut hasil = Vec::new();
    loop {
        pos += pembatas.len();
        if b[pos..].starts_with(b"--") { return Ok(hasil); }
        while pos < b.len() && (b[pos] == b' ' || b[pos] == b'\t') { pos += 1; }
        if !b[pos..].starts_with(b"\r\n") { return Err("pembatas tidak diikuti CRLF".into()); }
        pos += 2;
        let ke = hasil.len() + 1;
        if ke > MAKS_BAGIAN { return Err(format!("lebih dari {} bagian", MAKS_BAGIAN)); }
        let (akhir_kepala, mulai_isi) = if b[pos..].starts_with(b"\r\n") { (pos, pos + 2) } else {
            let batas = (pos + MAKS_KEPALA_BAGIAN + 4).min(b.len());
            let p = cari(&b[..batas], b"\r\n\r\n", pos)
                .ok_or_else(|| format!("kepala bagian ke-{} tidak diakhiri baris kosong dalam {} byte", ke, MAKS_KEPALA_BAGIAN))?;
            (p, p + 4)
        };
        let (nama, berkas, tipe) = urai_kepala_bagian(&b[pos..akhir_kepala], ke)?;
        let akhir_isi = cari(b, &sela, mulai_isi)
            .ok_or_else(|| format!("bagian ke-{} tidak ditutup pembatas — badannya terpotong", ke))?;
        hasil.push(Bagian { nama, berkas, tipe, isi: b[mulai_isi..akhir_isi].to_vec() });
        pos = akhir_isi + 2;
    }
}

/// `dok::multipart(req)` → `Result<array>` dict per bagian: `nama`, `tipe`, `berkas` kalau ada, dan
/// `isi` (teks) atau `isi_bytes` (bukan teks).
pub fn multipart(a: &[Value]) -> Result<Value, String> {
    let nama = "dok::multipart";
    let req = req_arg(a, nama)?;
    let hasil = (|| {
        let ct = kepala(req, "content-type")?.ok_or("permintaannya tanpa `Content-Type`")?;
        let (jenis, ps) = urai_jenis(&ct)?;
        if jenis != "multipart/form-data" {
            return Err(format!("`Content-Type`-nya `{}`, bukan `multipart/form-data`{}", jenis,
                               if jenis == "application/x-www-form-urlencoded" { " — baca dengan `dok::form`" } else { "" }));
        }
        let bd = param(&ps, "boundary").ok_or("`Content-Type`-nya tanpa `boundary`")?;
        if !boundary_sah(bd) { return Err(format!("boundary `{}` tidak sah (RFC 2046: 1–70 karakter tertentu)", bd)); }
        urai_multipart(&badan(req)?, bd)
    })();
    Ok(match hasil {
        Err(e) => err_value(format!("{}: {}", nama, e)),
        Ok(bs) => ok_value(Value::Array(bs.into_iter().map(|b| {
            let mut f = vec![("nama", Value::Str(std::sync::Arc::new(b.nama))), ("tipe", Value::Str(std::sync::Arc::new(b.tipe)))];
            if let Some(n) = b.berkas { f.push(("berkas", Value::Str(std::sync::Arc::new(n)))); }
            match String::from_utf8(b.isi) {
                Ok(t) => f.push(("isi", Value::Str(std::sync::Arc::new(t)))),
                Err(e) => f.push(("isi_bytes", super::crypto::byte_ke_array(e.into_bytes()))),
            }
            dict(f)
        }).collect::<Vec<_>>().into())),
    })
}

/// Nama berkas yang aman ditulis di satu folder: bagian sesudah `/` atau `\` terakhir, huruf ASCII, angka, `.`,
/// `-`, `_` (yang lain jadi `_`), tanpa titik di depan, bukan nama perangkat Windows; `None` kalau tak tersisa.
pub fn nama_berkas_aman(s: &str) -> Option<String> {
    let dasar = s.rsplit(['/', '\\']).next().unwrap_or("");
    let bersih: String = dasar.chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).collect();
    let bersih = bersih.trim_start_matches(['.', '_']).trim_end_matches(['.', ' ']).to_string();
    if bersih.is_empty() || bersih.len() > 255 { return None; }
    let akar = bersih.split('.').next().unwrap_or("").to_ascii_uppercase();
    let perangkat = matches!(akar.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (akar.len() == 4 && (akar.starts_with("COM") || akar.starts_with("LPT")) && akar.as_bytes()[3].is_ascii_digit());
    Some(if perangkat { format!("_{}", bersih) } else { bersih })
}

/// `dok::berkas_aman(nama)` → `Option<string>`.
pub fn berkas_aman(a: &[Value]) -> Result<Value, String> {
    let s = match a.first() {
        Some(Value::Str(s)) => s.to_string(),
        Some(lain) => return Err(crate::interpreter::pesan_arg_harus_teks("dok::berkas_aman", "nama berkasnya",
                                                                           &lain.type_name())),
        None => return Err("dok::berkas_aman: argumen ke-1 wajib diisi".into()),
    };
    Ok(match nama_berkas_aman(&s) {
        Some(n) => Value::Enum { type_name: "Option".into(), variant: "Some".into(), payload: Some(Box::new(Value::Str(std::sync::Arc::new(n)))) },
        None => Value::Enum { type_name: "Option".into(), variant: "None".into(), payload: None },
    })
}

/// Jalur lokal: diawali satu `/`, tanpa `\`, kendali atau spasi, dan tetap di host yang sama sesudah diselesaikan
/// pengurai `url::` (`//evil`, `/\evil`, `/%09/evil` tidak lolos).
pub fn jalur_lokal(s: &str) -> bool {
    let b = s.as_bytes();
    if b.first() != Some(&b'/') || matches!(b.get(1), Some(b'/') | Some(b'\\')) { return false; }
    if b.iter().any(|&c| c <= b' ' || c == 0x7f || c == b'\\') { return false; }
    const DASAR: &str = "http://jrg-lokal.invalid/";
    match crate::stdlib::url::gabung(DASAR, s) {
        Ok(u) => u.strip_prefix(DASAR).map_or(false, |sisa| !sisa.starts_with('/')),
        Err(_) => false,
    }
}

/// `dok::safe_redirect(tujuan, cadangan, kode?)`: ke `tujuan` kalau jalur lokal, ke `cadangan` kalau bukan.
pub fn safe_redirect(a: &[Value]) -> Result<Value, String> {
    let nama = "dok::safe_redirect";
    let teks = |i: usize, peran: &str| match a.get(i) {
        Some(Value::Str(s)) => Ok(s.to_string()),
        Some(lain) => Err(crate::interpreter::pesan_arg_harus_teks(nama, peran, &lain.type_name())),
        None => Err(format!("{}: argumen ke-{} wajib diisi", nama, i + 1)),
    };
    let tujuan = teks(0, "tujuannya")?;
    let cadangan = teks(1, "tujuan cadangannya")?;
    if !jalur_lokal(&cadangan) {
        return Err(format!("{}: cadangan `{}` bukan jalur lokal (diawali satu `/`, mis. `/`) — cadangan ditulis \
                            programnya, jadi harus selalu aman", nama, cadangan));
    }
    let ke = if jalur_lokal(&tujuan) { tujuan } else { cadangan };
    let mut arg = vec![Value::Str(std::sync::Arc::new(ke))];
    if let Some(k) = a.get(2) { arg.push(k.clone()); }
    super::http::redirect(&arg).map_err(|e| e.replace("dok::redirect", nama))
}

/// Satu entri daftar izin `dok::host_check`: (host, port, termasuk subdomain).
fn pola_host(p: &str) -> Result<(String, Option<u16>, bool), String> {
    let nama = "dok::host_check";
    if p == "*" || p.contains('*') {
        return Err(format!("{}: pola `{}` ditolak — `*` mengizinkan host apa pun, jadi pemeriksaannya tidak memeriksa; \
                            tulis `.contoh.id` untuk contoh.id beserta subdomainnya", nama, p));
    }
    let (sub, inti) = match p.strip_prefix('.') { Some(r) => (true, r), None => (false, p) };
    let (h, port) = crate::stdlib::url::urai_kepala_host(inti).map_err(|e| format!("{}: pola `{}` tidak sah: {}", nama, p, e))?;
    if sub && h.starts_with('[') { return Err(format!("{}: pola `{}`: alamat IPv6 tidak punya subdomain", nama, p)); }
    Ok((h, port, sub))
}

/// `dok::host_check(req, izin)` → `Ok(host)` kalau kepala `Host` ada di daftar izin, `Err(alasan)` kalau tidak.
/// `X-Forwarded-Host` tidak dibaca: kepala itu ditulis klien kecuali proxy-nya menimpanya.
pub fn host_check(a: &[Value]) -> Result<Value, String> {
    let nama = "dok::host_check";
    let req = req_arg(a, nama)?;
    let pola: Vec<(String, Option<u16>, bool)> = match a.get(1) {
        Some(Value::Array(xs)) if !xs.is_empty() => xs.iter().map(|x| match x {
            Value::Str(p) => pola_host(p),
            lain => Err(format!("{}: tiap pola izin harus teks, dapat `{}`", nama, lain.type_name())),
        }).collect::<Result<_, _>>()?,
        Some(Value::Array(_)) => return Err(format!("{}: daftar izin kosong — tidak ada host yang bisa lolos; tulis \
                                                     host situsnya, mis. [\"contoh.id\"]", nama)),
        Some(lain) => return Err(format!("{}: argumen ke-2 harus array host yang diizinkan, dapat `{}`", nama,
                                         lain.type_name())),
        None => return Err(format!("{}: argumen ke-2 wajib diisi", nama)),
    };
    let tolak = |m: String| Ok(err_value(format!("{}: {}", nama, m)));
    let v = match kepala(req, "host") {
        Ok(Some(v)) => v,
        Ok(None) => return tolak("permintaan tanpa kepala `Host`".into()),
        Err(e) => return tolak(e),
    };
    let (h, port) = match crate::stdlib::url::urai_kepala_host(&v) { Ok(x) => x, Err(e) => return tolak(e) };
    let cocok = pola.iter().any(|(ph, pp, sub)| {
        let nama_cocok = h == *ph || (*sub && h.len() > ph.len() && h.ends_with(ph.as_str())
            && h.as_bytes()[h.len() - ph.len() - 1] == b'.');
        nama_cocok && pp.map_or(true, |p| port == Some(p))
    });
    if !cocok { return tolak(format!("host `{}` tidak ada di daftar izin", v.escape_debug())); }
    Ok(ok_value(Value::teks(match port { Some(p) => format!("{}:{}", h, p), None => h })))
}

#[cfg(test)]
mod tes {
    use super::*;

    #[test]
    fn host_dicocokkan_dengan_daftar_izin() {
        let req = |h: &[&str]| dict(vec![("headers", Value::Array(h.iter()
            .map(|v| Value::Tuple(vec![Value::teks("Host"), Value::Str(std::sync::Arc::new(v.to_string()))].into())).collect::<Vec<_>>().into()))]);
        let izin = |ps: &[&str]| Value::Array(ps.iter().map(|p| Value::Str(std::sync::Arc::new(p.to_string()))).collect::<Vec<_>>().into());
        let tanya = |h: &[&str], ps: &[&str]| -> Result<String, String> {
            match host_check(&[req(h), izin(ps)])? {
                Value::Enum { variant, payload: Some(p), .. } => match *p {
                    Value::Str(s) if variant == "Ok" => Ok(s.to_string()),
                    lain => Err(format!("{}", lain)),
                },
                lain => panic!("{:?}", lain),
            }
        };
        assert_eq!(tanya(&["Contoh.ID."], &["contoh.id"]), Ok("contoh.id".into()));
        assert_eq!(tanya(&["a.b.contoh.id:8080"], &[".contoh.id"]), Ok("a.b.contoh.id:8080".into()));
        assert_eq!(tanya(&["[0::1]:80"], &["[::1]:80"]), Ok("[::1]:80".into()));
        for (h, ps) in [(&["jahatcontoh.id"][..], &[".contoh.id"][..]), (&["contoh.id.evil"], &["contoh.id"]),
                        (&["contoh.id:81"], &["contoh.id:80"]), (&["contoh.id"], &["contoh.id:80"]),
                        (&["contoh.id", "evil"], &["contoh.id"]), (&[], &["contoh.id"]),
                        (&["contoh.id@evil"], &["contoh.id"]), (&["0x7f.1"], &["127.0.0.1"])] {
            assert!(tanya(h, ps).is_err(), "{:?} {:?}", h, ps);
        }
        for ps in [&["*"][..], &[], &["a/b"], &[".[::1]"], &["*.contoh.id"]] {
            assert!(host_check(&[req(&["a"]), izin(ps)]).is_err(), "{:?}", ps);
        }
    }

    #[test]
    fn multipart_diurai_dan_yang_ambigu_ditolak() {
        let bd = "XyZ";
        let b = b"preamble\r\n--XyZ\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nsatu\r\n--XyZ\r\n\
Content-Disposition: form-data; name=\"f\"; filename=\"../../x.txt\"\r\nContent-Type: application/octet-stream\r\n\r\n\
\xff\x00\r\n--XyZ--\r\nepilog";
        let bs = urai_multipart(b, bd).unwrap();
        assert_eq!(bs.len(), 2);
        assert_eq!((bs[0].nama.as_str(), bs[0].isi.as_slice(), bs[0].tipe.as_str()), ("a", &b"satu"[..], "text/plain"));
        assert_eq!(bs[1].berkas.as_deref(), Some("../../x.txt"));
        assert_eq!(bs[1].isi, vec![0xff, 0]);
        let galat = |b: &[u8]| urai_multipart(b, bd).err().unwrap();
        assert!(galat(b"--XyZ\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nx").contains("terpotong"));
        assert!(galat(b"--XyZ\r\nContent-Disposition: form-data; name=\"a\"; name=\"b\"\r\n\r\nx\r\n--XyZ--").contains("dua kali"));
        assert!(galat(b"--XyZ\r\nContent-Disposition: form-data; name=\"a\"; filename*=UTF-8''x\r\n\r\nx\r\n--XyZ--")
            .contains("filename*"));
        assert!(galat(b"--XyZ\r\nContent-Disposition: form-data; name=\"a\\\"\r\n\r\nx\r\n--XyZ--").contains("ambigu"));
        assert!(galat(b"--XyZ\r\nContent-Type: a\r\nContent-Type: b\r\nContent-Disposition: form-data; name=a\r\n\r\nx\r\n--XyZ--")
            .contains("dua kali"));
        assert!(galat(b"--XyZ\r\nContent-Disposition: form-data;\r\n name=a\r\n\r\nx\r\n--XyZ--").contains("lipatan"));
        assert!(galat(b"--XyZ\nContent-Disposition: form-data; name=a\n\nx\n--XyZ--").contains("CRLF"));
        let mut banyak = Vec::new();
        for _ in 0..=MAKS_BAGIAN { banyak.extend_from_slice(b"--XyZ\r\nContent-Disposition: form-data; name=a\r\n\r\n\r\n"); }
        banyak.extend_from_slice(b"--XyZ--");
        assert!(galat(&banyak).contains("lebih dari"));
        assert!(!boundary_sah("") && !boundary_sah(&"a".repeat(71)) && !boundary_sah("a\"b") && boundary_sah("--abc'()"));
    }

    #[test]
    fn kode_pengalihan_dan_kendali_ditolak() {
        let r = |u: &str, k: i128| super::super::http::redirect(&[Value::teks(u), Value::Int(k)]);
        assert!(r("/x", 303).is_ok() && r("/x", 308).is_ok());
        assert!(r("/x", 200).unwrap_err().contains("bukan kode pengalihan"));
        assert!(r("/x\r\nSet-Cookie: a=1", 302).unwrap_err().contains("kendali"));
        assert!(safe_redirect(&[Value::teks("/a"), Value::teks("//evil")]).unwrap_err().contains("cadangan"));
    }

    #[test]
    fn nama_berkas_dan_jalur_lokal() {
        assert_eq!(nama_berkas_aman("../../etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(nama_berkas_aman("C:\\Users\\a\\foto saya.JPG").as_deref(), Some("foto_saya.JPG"));
        assert_eq!(nama_berkas_aman(".bashrc").as_deref(), Some("bashrc"));
        assert_eq!(nama_berkas_aman("CON.txt").as_deref(), Some("_CON.txt"));
        assert_eq!(nama_berkas_aman("com1").as_deref(), Some("_com1"));
        assert_eq!(nama_berkas_aman("ü.png").as_deref(), Some("png"));
        assert_eq!(nama_berkas_aman(".."), None);
        assert_eq!(nama_berkas_aman("a/"), None);
        for ok in ["/", "/a/b?x=1#f", "/a/../b", "/%2F%2Fx"] { assert!(jalur_lokal(ok), "{}", ok); }
        for buruk in ["//evil.com", "/\\evil.com", "\\\\evil", "https://evil.com", "evil.com", "/\tx", "javascript:x",
                      "", "/a b", "/\u{3000}x", "/..//evil"] {
            assert!(!jalur_lokal(buruk), "{}", buruk);
        }
    }
}
