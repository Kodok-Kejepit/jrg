// src/stdlib/url.rs — `url::`: satu pengurai URL untuk pemakai DAN klien `http::`/`dok::http_*`.
// Kelas celah pengurai URL bahasa lain (dua pengurai membaca satu URL berbeda) ditolak, bukan ditebak.

use crate::kutip_inti::kutip;
use std::collections::HashMap;
use std::net::Ipv6Addr;

use crate::interpreter::{err_value, ok_value, Value};
use crate::vm::vm::Vm;

/// Pasangan query paling banyak (kelas CVE-2025-61726 di Go `net/url`: query raksasa menghabiskan memori).
pub const MAKS_PASANGAN: usize = 10_000;

pub fn register(vm: &mut Vm) {
    vm.def("url::parse", |a| crate::stdlib::sistem::lewat_hasil(a, u_parse));
    vm.def("url::build", |a| crate::stdlib::sistem::lewat_hasil(a, u_build));
    vm.def("url::join", |a| crate::stdlib::sistem::lewat_hasil(a, u_join));
    vm.def("url::query_parse", |a| crate::stdlib::sistem::lewat_hasil(a, u_query_parse));
    vm.def("url::query_build", |a| crate::stdlib::sistem::lewat_hasil(a, u_query_build));
}

/// URL yang sudah diurai dan dibakukan: skema dan host huruf kecil, port bawaan dibuang, `.`/`..` diselesaikan.
#[derive(Debug, Clone, PartialEq)]
pub struct Urai {
    pub skema: String,
    pub pengguna: Option<String>,
    pub sandi: Option<String>,
    /// `Some("")` untuk otoritas kosong (`file:///x`); IPv6 di dalam kurung siku.
    pub host: Option<String>,
    pub port: Option<u16>,
    pub jalur: String,
    pub query: Option<String>,
    pub fragmen: Option<String>,
}

/// Skema yang wajib berotoritas dan berhost — pengurai WHATWG memperlakukannya istimewa.
fn istimewa(skema: &str) -> bool { matches!(skema, "http" | "https" | "ws" | "wss" | "ftp") }

pub fn port_bawaan(skema: &str) -> Option<u16> {
    match skema { "http" | "ws" => Some(80), "https" | "wss" => Some(443), "ftp" => Some(21), _ => None }
}

/// Byte yang tidak pernah sah mentah di URL; pengurai lain membuangnya, menggantinya, atau berhenti di sana.
fn periksa_huruf(s: &str) -> Result<(), String> {
    let b = s.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        let apa = match c {
            b' ' => "spasi",
            0..=0x1f | 0x7f => "karakter kendali",
            0x80.. => "huruf non-ASCII",
            b'\\' => "garis miring terbalik `\\` (sebagian pengurai membacanya sebagai `/`)",
            b'"' | b'<' | b'>' | b'^' | b'`' | b'{' | b'|' | b'}' => "karakter yang wajib di-percent-encode",
            b'%' => {
                if i + 2 < b.len() && b[i + 1].is_ascii_hexdigit() && b[i + 2].is_ascii_hexdigit() { continue; }
                return Err(format!("`%` di posisi {} tidak diikuti dua digit heksa", i));
            }
            _ => continue,
        };
        return Err(format!("{} di posisi {} — tulis sebagai %XX (lihat `http::url_encode`)", apa, i));
    }
    Ok(())
}

/// Panjang skema di awal `s` (tanpa `:`), kalau ada — `None` berarti rujukan relatif.
fn panjang_skema(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if !b.first()?.is_ascii_alphabetic() { return None; }
    let n = b.iter().position(|&c| !(c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.')))?;
    (b[n] == b':').then_some(n)
}

fn huruf_userinfo(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"-._~%!$&'()*+,;=".contains(&c)
}

/// Host: IPv6 berkurung, IPv4 empat angka desimal baku, atau nama huruf/angka/`-`/`_`/`.`.
fn periksa_host(h: &str, skema: &str) -> Result<String, String> {
    if let Some(dalam) = h.strip_prefix('[') {
        let dalam = dalam.strip_suffix(']').ok_or_else(|| format!("host `{}`: kurung siku IPv6 tidak ditutup", kutip(h)))?;
        if dalam.contains('%') { return Err(format!("host `{}`: zone id IPv6 (`%`) ditolak", kutip(h))); }
        let a: Ipv6Addr = dalam.parse().map_err(|_| format!("host `{}` bukan alamat IPv6 yang sah", kutip(h)))?;
        return Ok(format!("[{}]", a));
    }
    if h.is_empty() {
        return if istimewa(skema) { Err(format!("URL `{}:` tanpa nama host", kutip(&skema))) } else { Ok(String::new()) };
    }
    if h.contains('%') {
        return Err(format!("host `{}` ber-%XX ditolak — pengurai lain membongkarnya jadi host yang lain", kutip(h)));
    }
    if let Some(c) = h.bytes().find(|&c| !(c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b'_'))) {
        return Err(format!("host `{}` memuat `{}`; nama host cuma huruf, angka, `-`, `_` dan `.` (IPv6 di dalam `[ ]`)",
                           kutip(h), c as char));
    }
    let h = h.to_ascii_lowercase();
    let label: Vec<&str> = h.strip_suffix('.').unwrap_or(&h).split('.').collect();
    if label.iter().any(|l| l.is_empty()) { return Err(format!("host `{}` memuat label kosong", kutip(h))); }
    // Aturan WHATWG: label terakhir berupa angka berarti seluruh host dibaca sebagai IPv4 — juga oktal dan heksa.
    let akhir = label[label.len() - 1];
    let angka = akhir.bytes().all(|c| c.is_ascii_digit())
        || (akhir.len() > 2 && akhir[..2].eq_ignore_ascii_case("0x") && akhir[2..].bytes().all(|c| c.is_ascii_hexdigit()));
    if angka {
        let baku = label.len() == 4 && label.iter().all(|l| {
            l.bytes().all(|c| c.is_ascii_digit()) && l.len() <= 3 && (l.len() == 1 || !l.starts_with('0'))
                && l.parse::<u16>().map_or(false, |n| n <= 255)
        });
        if !baku || h.ends_with('.') {
            return Err(format!("host `{}` adalah angka IPv4 tak baku (oktal, heksa, kurang dari empat bagian, atau di \
                                atas 255) — pustaka lain membacanya sebagai alamat lain; tulis empat angka desimal", kutip(h)));
        }
    }
    Ok(h)
}

fn periksa_port(t: &str) -> Result<Option<u16>, String> {
    if t.is_empty() { return Ok(None); }
    match t.parse::<u16>() {
        Ok(p) if p > 0 && t.bytes().all(|c| c.is_ascii_digit()) => Ok(Some(p)),
        _ => Err(format!("port `{}` tidak sah — yang sah 1 sampai 65535", kutip(t))),
    }
}

/// Nilai kepala `Host` (RFC 9110 §7.2: `host[:port]`) → (host baku huruf kecil tanpa titik ujung, port).
/// Aturan host sama dengan URL; port kosong (`a:`) ditolak.
pub fn urai_kepala_host(v: &str) -> Result<(String, Option<u16>), String> {
    if let Some(c) = v.bytes().find(|&c| c <= b' ' || c >= 0x7f || matches!(c, b'@' | b'/' | b'\\' | b',' | b'?' | b'#')) {
        return Err(format!("host `{}` memuat `{}` — bukan nilai `Host` yang sah", kutip(v), (c as char).escape_debug()));
    }
    let (h, p) = if v.starts_with('[') {
        let tutup = v.find(']').map(|i| i + 1).ok_or_else(|| format!("host `{}`: kurung siku IPv6 tidak ditutup", kutip(v)))?;
        let sisa = &v[tutup..];
        let p = if sisa.is_empty() { None } else {
            Some(sisa.strip_prefix(':').ok_or_else(|| format!("host `{}`: sesudah `]` cuma boleh `:port`", kutip(v)))?)
        };
        (&v[..tutup], p)
    } else {
        match v.split_once(':') { Some((h, p)) => (h, Some(p)), None => (v, None) }
    };
    let port = match p {
        Some("") => return Err(format!("host `{}`: port kosong sesudah `:`", kutip(v))),
        Some(p) => periksa_port(p)?,
        None => None,
    };
    let h = periksa_host(h, "http")?;
    let h = h.strip_suffix('.').map(str::to_string).unwrap_or(h);
    Ok((h, port))
}

/// `.` dan `..` (juga `%2e`) diselesaikan sekali di sini, jadi jalur yang diperiksa = jalur yang dikirim.
fn buang_titik(jalur: &str) -> String {
    let titik = |s: &str| s == "." || s.eq_ignore_ascii_case("%2e");
    let dua = |s: &str| matches!(s.to_ascii_lowercase().as_str(), ".." | ".%2e" | "%2e." | "%2e%2e");
    let mutlak = jalur.starts_with('/');
    let bagian: Vec<&str> = jalur.split('/').collect();
    let mut keluar: Vec<&str> = Vec::new();
    let n = bagian.len();
    for (i, b) in bagian.iter().enumerate() {
        if i == 0 && mutlak { continue; }
        let akhir = i + 1 == n;
        if titik(b) { if akhir { keluar.push(""); } continue; }
        if dua(b) {
            keluar.pop();
            if akhir { keluar.push(""); }
            continue;
        }
        keluar.push(b);
    }
    let isi = keluar.join("/");
    if mutlak { format!("/{}", isi) } else { isi }
}

/// Mengurai URL mutlak (berskema). Yang ambigu jadi `Err` yang menyebut bagian dan alasannya.
pub fn urai(s: &str) -> Result<Urai, String> {
    if s.is_empty() { return Err("URL kosong".into()); }
    periksa_huruf(s)?;
    let n = panjang_skema(s).ok_or_else(|| format!(
        "`{}` tidak berskema (`http:`, `https:`, ...) — URL relatif digabung dengan dasarnya lewat `url::join`", s))?;
    let skema = s[..n].to_ascii_lowercase();
    let mut sisa = &s[n + 1..];
    let mut fragmen = None;
    if let Some(i) = sisa.find('#') {
        let f = &sisa[i + 1..];
        if f.contains('#') { return Err("fragmen memuat `#` kedua".into()); }
        fragmen = Some(f.to_string());
        sisa = &sisa[..i];
    }
    let mut query = None;
    if let Some(i) = sisa.find('?') {
        query = Some(sisa[i + 1..].to_string());
        sisa = &sisa[..i];
    }
    let (mut pengguna, mut sandi, mut host, mut port) = (None, None, None, None);
    let jalur_mentah = if let Some(r) = sisa.strip_prefix("//") {
        let ujung = r.find('/').unwrap_or(r.len());
        let (otoritas, jalur) = (&r[..ujung], &r[ujung..]);
        let host_port = match otoritas.matches('@').count() {
            0 => otoritas,
            1 => {
                let (info, hp) = otoritas.split_once('@').unwrap();
                if let Some(c) = info.bytes().find(|&c| !huruf_userinfo(c) && c != b':') {
                    return Err(format!("userinfo memuat `{}`", c as char));
                }
                let (u, p) = match info.split_once(':') { Some((u, p)) => (u, Some(p)), None => (info, None) };
                pengguna = Some(u.to_string());
                sandi = p.map(str::to_string);
                hp
            }
            k => return Err(format!("otoritas `{}` memuat {} `@` — host mana yang dimaksud ambigu; tulis `@` di \
                                     userinfo sebagai %40", kutip(samarkan_userinfo(otoritas)), k)),
        };
        let (h, p) = if host_port.starts_with('[') {
            match host_port.find(']') {
                Some(i) => {
                    let ekor = &host_port[i + 1..];
                    let p = if ekor.is_empty() { "" } else {
                        ekor.strip_prefix(':').ok_or_else(|| format!("sesudah `]` di `{}` harus `:port`", kutip(host_port)))?
                    };
                    (&host_port[..=i], p)
                }
                None => return Err(format!("host `{}`: kurung siku IPv6 tidak ditutup", kutip(host_port))),
            }
        } else {
            match host_port.matches(':').count() {
                0 => (host_port, ""),
                1 => host_port.split_once(':').unwrap(),
                _ => return Err(format!("`{}` memuat banyak `:` — alamat IPv6 ditulis di dalam `[ ]`", kutip(host_port))),
            }
        };
        host = Some(periksa_host(h, &skema)?);
        port = periksa_port(p)?;
        if port.is_some() && port == port_bawaan(&skema) { port = None; }
        jalur
    } else {
        if istimewa(&skema) {
            return Err(format!("`{}:` harus diikuti `//` dan nama host — `{}` dibaca berbeda oleh tiap pengurai", kutip(skema), kutip(s)));
        }
        sisa
    };
    let mut jalur = if host.is_some() || jalur_mentah.starts_with('/') { buang_titik(jalur_mentah) } else {
        jalur_mentah.to_string()
    };
    if jalur.is_empty() && istimewa(&skema) { jalur = "/".into(); }
    Ok(Urai { skema, pengguna, sandi, host, port, jalur, query, fragmen })
}

pub fn tulis(u: &Urai) -> String {
    let mut o = format!("{}:", u.skema);
    if let Some(h) = &u.host {
        o.push_str("//");
        if let Some(p) = &u.pengguna {
            o.push_str(p);
            if let Some(s) = &u.sandi { o.push(':'); o.push_str(s); }
            o.push('@');
        }
        o.push_str(h);
        if let Some(p) = u.port { o.push_str(&format!(":{}", p)); }
    }
    o.push_str(&u.jalur);
    if let Some(q) = &u.query { o.push('?'); o.push_str(q); }
    if let Some(f) = &u.fragmen { o.push('#'); o.push_str(f); }
    o
}

/// RFC 3986 §5.2: `rujukan` diselesaikan terhadap `dasar`; hasilnya diurai ulang dengan aturan `urai`.
pub fn gabung(dasar: &str, rujukan: &str) -> Result<String, String> {
    let d = urai(dasar).map_err(|e| format!("dasar: {}", e))?;
    periksa_huruf(rujukan).map_err(|e| format!("rujukan: {}", e))?;
    if panjang_skema(rujukan).is_some() { return urai(rujukan).map(|u| tulis(&u)); }
    let (r, fragmen) = match rujukan.split_once('#') { Some((a, f)) => (a, Some(f)), None => (rujukan, None) };
    let (r, query) = match r.split_once('?') { Some((a, q)) => (a, Some(q)), None => (r, None) };
    let mut o = format!("{}:", d.skema);
    if r.starts_with("//") {
        o.push_str(r);
    } else {
        let mut t = tulis(&Urai { query: None, fragmen: None, jalur: String::new(), ..d.clone() });
        if d.host.is_none() { t = format!("{}:", d.skema); }
        o = t;
        if r.is_empty() {
            o.push_str(&d.jalur);
            if query.is_none() { if let Some(q) = &d.query { o.push('?'); o.push_str(q); } }
        } else if r.starts_with('/') {
            o.push_str(r);
        } else {
            let dir = match d.jalur.rfind('/') { Some(i) => &d.jalur[..=i], None => if d.host.is_some() { "/" } else { "" } };
            o.push_str(dir);
            o.push_str(r);
        }
    }
    if let Some(q) = query { o.push('?'); o.push_str(q); }
    if let Some(f) = fragmen { o.push('#'); o.push_str(f); }
    urai(&o).map(|u| tulis(&u))
}

fn bongkar_persen(s: &str) -> Result<String, String> {
    let b = s.as_bytes();
    let mut o = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => { o.push(b' '); i += 1; }
            b'%' => {
                let h = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
                    .ok_or_else(|| format!("`%` di posisi {} tidak diikuti dua digit heksa", i))?;
                o.push(h);
                i += 3;
            }
            c => { o.push(c); i += 1; }
        }
    }
    String::from_utf8(o).map_err(|_| format!("`{}` bukan teks UTF-8 sesudah dibongkar", kutip(s)))
}

/// `a=1&b=2` → pasangan urut tulis; kunci kembar TETAP dua pasangan (kelas HTTP parameter pollution).
pub fn urai_query(q: &str) -> Result<Vec<(String, String)>, String> {
    let q = q.strip_prefix('?').unwrap_or(q);
    if let Some(i) = q.bytes().position(|c| c <= b' ' || c >= 0x7f) {
        return Err(format!("karakter kendali, spasi atau non-ASCII di posisi {}", i));
    }
    let mut o = Vec::new();
    for bagian in q.split('&').filter(|b| !b.is_empty()) {
        if o.len() == MAKS_PASANGAN { return Err(format!("lebih dari {} pasangan", MAKS_PASANGAN)); }
        let (k, v) = bagian.split_once('=').unwrap_or((bagian, ""));
        o.push((bongkar_persen(k)?, bongkar_persen(v)?));
    }
    Ok(o)
}

fn sandikan(s: &str, o: &mut String) {
    for &c in s.as_bytes() {
        match c {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => o.push(c as char),
            b' ' => o.push('+'),
            _ => o.push_str(&format!("%{:02X}", c)),
        }
    }
}

fn teks_arg(a: &[Value], ke: usize, nama: &str) -> Result<String, String> {
    match a.get(ke) {
        Some(Value::Str(s)) => Ok(s.to_string()),
        Some(lain) => Err(format!("{}: argumen ke-{} harus string, dapat `{}`", nama, ke + 1, lain.type_name())),
        None => Err(format!("{}: argumen ke-{} wajib diisi", nama, ke + 1)),
    }
}

fn dict(isi: Vec<(&str, Value)>) -> Value {
    let m: HashMap<String, Value> = isi.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    Value::Struct { name: "dict".to_string(), fields: m.into() }
}

/// `url::parse(s)` → `Result<dict>`: `skema`, `jalur`, dan `pengguna`/`sandi`/`host`/`port`/`query`/`fragmen` yang ada.
fn u_parse(a: &[Value]) -> Result<Value, String> {
    let s = teks_arg(a, 0, "url::parse")?;
    Ok(match urai(&s) {
        Err(e) => err_value(format!("url::parse: {}", e)),
        Ok(u) => {
            let teks = |t: &str| Value::teks(t);
            let mut f = vec![("skema", teks(&u.skema)), ("jalur", teks(&u.jalur))];
            for (k, v) in [("pengguna", &u.pengguna), ("sandi", &u.sandi), ("host", &u.host), ("query", &u.query),
                           ("fragmen", &u.fragmen)] {
                if let Some(v) = v { f.push((k, teks(v))); }
            }
            if let Some(p) = u.port.or(u.host.as_ref().and_then(|_| port_bawaan(&u.skema))) {
                f.push(("port", Value::Int(p as i128)));
            }
            ok_value(dict(f))
        }
    })
}

const KUNCI_BUILD: [&str; 8] = ["skema", "pengguna", "sandi", "host", "port", "jalur", "query", "fragmen"];

/// `url::build(d)` → `Result<string>`: kebalikan `url::parse`; bagian yang memuat pemisah bagian lain ditolak.
fn u_build(a: &[Value]) -> Result<Value, String> {
    let isi = match a.first() {
        Some(Value::Struct { name, fields }) if name == "dict" => fields,
        Some(lain) => return Err(format!("url::build: argumen ke-1 harus dict, dapat `{}`", lain.type_name())),
        None => return Err("url::build: argumen ke-1 wajib diisi".into()),
    };
    if let Some(k) = isi.keys().find(|k| !KUNCI_BUILD.contains(&k.as_str())) {
        return Err(format!("url::build: kunci `{}` tidak dikenal; yang ada {}", kutip(k), KUNCI_BUILD.join(", ")));
    }
    let ambil = |k: &str| -> Result<Option<String>, String> {
        match isi.get(k) {
            None => Ok(None),
            Some(Value::Str(s)) => Ok(Some(s.to_string())),
            Some(Value::Int(n)) if k == "port" => Ok(Some(n.to_string())),
            Some(lain) => Err(format!("url::build: `{}` harus string, dapat `{}`", kutip(k), lain.type_name())),
        }
    };
    let gagal = |e: String| Ok(err_value(format!("url::build: {}", e)));
    let Some(skema) = ambil("skema")? else { return Err("url::build: kunci `skema` wajib ada".into()) };
    let (host, port, pengguna, sandi) = (ambil("host")?, ambil("port")?, ambil("pengguna")?, ambil("sandi")?);
    let jalur = ambil("jalur")?.unwrap_or_default();
    let (query, fragmen) = (ambil("query")?, ambil("fragmen")?);
    let larang = |nama: &str, t: &Option<String>, pemisah: &str| -> Result<(), String> {
        match t.as_deref().and_then(|t| t.chars().find(|c| pemisah.contains(*c))) {
            Some(c) => Err(format!("`{}` memuat `{}`, pemisah bagian lain — sandikan sebagai %XX", kutip(nama), c)),
            None => Ok(()),
        }
    };
    let cek = (|| -> Result<(), String> {
        if panjang_skema(&format!("{}:", skema)) != Some(skema.len()) { return Err(format!("skema `{}` tidak sah", kutip(&skema))); }
        larang("pengguna", &pengguna, ":@/?#[]")?;
        larang("sandi", &sandi, "@/?#[]")?;
        larang("host", &host, "@/?#\\")?;
        larang("query", &query, "#")?;
        larang("fragmen", &fragmen, "#")?;
        larang("jalur", &Some(jalur.clone()), "?#")?;
        if host.is_none() && (port.is_some() || pengguna.is_some()) {
            return Err("`port`/`pengguna` butuh `host`".into());
        }
        if sandi.is_some() && pengguna.is_none() { return Err("`sandi` butuh `pengguna`".into()); }
        if host.is_some() && !(jalur.is_empty() || jalur.starts_with('/')) {
            return Err(format!("jalur `{}` harus diawali `/` kalau ada host", kutip(&jalur)));
        }
        if host.is_none() && jalur.starts_with("//") { return Err("jalur tanpa host tidak boleh diawali `//`".into()); }
        Ok(())
    })();
    if let Err(e) = cek { return gagal(e); }
    let mut o = format!("{}:", skema);
    if let Some(h) = &host {
        o.push_str("//");
        if let Some(p) = &pengguna {
            o.push_str(p);
            if let Some(s) = &sandi { o.push(':'); o.push_str(s); }
            o.push('@');
        }
        o.push_str(h);
        if let Some(p) = &port { o.push(':'); o.push_str(p); }
    }
    o.push_str(&jalur);
    if let Some(q) = &query { o.push('?'); o.push_str(q); }
    if let Some(f) = &fragmen { o.push('#'); o.push_str(f); }
    match urai(&o) {
        Ok(u) => Ok(ok_value(Value::Str(tulis(&u).into()))),
        Err(e) => gagal(e),
    }
}

/// `url::join(dasar, rujukan)` → `Result<string>` (RFC 3986 §5.2).
fn u_join(a: &[Value]) -> Result<Value, String> {
    let (d, r) = (teks_arg(a, 0, "url::join")?, teks_arg(a, 1, "url::join")?);
    Ok(match gabung(&d, &r) {
        Ok(s) => ok_value(Value::teks(s)),
        Err(e) => err_value(format!("url::join: {}", e)),
    })
}

/// `url::query_parse(q)` → `Result<array>` berisi `[kunci, nilai]`; `dict::from_pairs` kalau mau yang terakhir menang.
fn u_query_parse(a: &[Value]) -> Result<Value, String> {
    let q = teks_arg(a, 0, "url::query_parse")?;
    Ok(match urai_query(&q) {
        Err(e) => err_value(format!("url::query_parse: {}", e)),
        Ok(p) => ok_value(Value::Array(p.into_iter().map(|(k, v)| {
            Value::Array(vec![Value::teks(k), Value::teks(v)].into())
        }).collect::<Vec<_>>().into())),
    })
}

/// `url::query_build(pasangan)` → `string`; dict (urut kunci) atau array `[kunci, nilai]` (urut tulis).
fn u_query_build(a: &[Value]) -> Result<Value, String> {
    let nama = "url::query_build";
    let teks = |v: &Value, apa: &str| -> Result<String, String> {
        match v {
            Value::Str(s) => Ok(s.to_string()),
            Value::Int(n) => Ok(n.to_string()),
            lain => Err(format!("{}: {} harus string atau int, dapat `{}`", nama, apa, lain.type_name())),
        }
    };
    let mut pasangan: Vec<(String, String)> = Vec::new();
    match a.first() {
        Some(Value::Struct { name, fields }) if name == "dict" => {
            let mut k: Vec<&String> = fields.keys().collect();
            k.sort();
            for k in k { pasangan.push((k.clone(), teks(&fields[k], "nilai")?)); }
        }
        Some(Value::Array(x)) => for (i, p) in x.iter().enumerate() {
            match p {
                Value::Array(kv) if kv.len() == 2 => pasangan.push((teks(&kv[0], "kunci")?, teks(&kv[1], "nilai")?)),
                lain => return Err(format!("{}: elemen ke-{} harus `[kunci, nilai]`, dapat `{}`", nama, i, lain.type_name())),
            }
        },
        Some(lain) => return Err(format!("{}: argumen ke-1 harus dict atau array pasangan, dapat `{}`", nama, lain.type_name())),
        None => return Err(format!("{}: argumen ke-1 wajib diisi", nama)),
    }
    let mut o = String::new();
    for (i, (k, v)) in pasangan.iter().enumerate() {
        if i > 0 { o.push('&'); }
        sandikan(k, &mut o);
        o.push('=');
        sandikan(v, &mut o);
    }
    Ok(Value::teks(o))
}

/// URL atau otoritas untuk pesan galat: userinfo (nama dan sandi) diganti `***`, karena pesan sampai ke log
/// (kelas CVE-2026-123871 Node.js). Batas otoritasnya `/`, `?`, `#` pertama sesudah `//`; `@` terakhir memisah.
pub fn samarkan_userinfo(teks: &str) -> String {
    let (awal, sisa) = match teks.find("//") { Some(i) => teks.split_at(i + 2), None => ("", teks) };
    let ujung = sisa.find(['/', '?', '#']).unwrap_or(sisa.len());
    match sisa[..ujung].rfind('@') {
        Some(at) => format!("{}***{}", awal, &sisa[at..]),
        None => teks.to_string(),
    }
}

#[cfg(test)]
mod tes {
    use super::*;

    #[test]
    fn diurai_dibakukan_dan_digabung() {
        let u = urai("HTTPS://Pak:rahasia@Contoh.ID:443/a/./b/../c?x=1&y#f").unwrap();
        assert_eq!((u.skema.as_str(), u.host.as_deref(), u.port, u.jalur.as_str()), ("https", Some("contoh.id"), None, "/a/c"));
        assert_eq!((u.pengguna.as_deref(), u.sandi.as_deref(), u.query.as_deref(), u.fragmen.as_deref()),
                   (Some("Pak"), Some("rahasia"), Some("x=1&y"), Some("f")));
        assert_eq!(tulis(&urai("http://h:8080").unwrap()), "http://h:8080/");
        assert_eq!(tulis(&urai("http://[0:0::1]:81/%2e%2E/x").unwrap()), "http://[::1]:81/x");
        assert_eq!(tulis(&urai("mailto:a@b.c").unwrap()), "mailto:a@b.c");
        assert_eq!(tulis(&urai("file:///etc/x").unwrap()), "file:///etc/x");
        let d = "http://a/b/c/d;p?q";
        for (r, harap) in [("g", "http://a/b/c/g"), ("./g", "http://a/b/c/g"), ("g/", "http://a/b/c/g/"),
                           ("/g", "http://a/g"), ("//g", "http://g/"), ("?y", "http://a/b/c/d;p?y"),
                           ("g?y", "http://a/b/c/g?y"), ("#s", "http://a/b/c/d;p?q#s"), ("", "http://a/b/c/d;p?q"),
                           (".", "http://a/b/c/"), ("..", "http://a/b/"), ("../..", "http://a/"),
                           ("../../../g", "http://a/g"), ("/./g", "http://a/g"), ("g;x=1/../y", "http://a/b/c/y"),
                           ("HTTPS://X/", "https://x/")] {
            assert_eq!(gabung(d, r).unwrap(), harap, "rujukan {:?}", r);
        }
    }

    #[test]
    fn userinfo_tidak_sampai_ke_pesan() {
        for (t, harap) in [("http://a:rahasia@h:1/x?y@z", "http://***@h:1/x?y@z"), ("a:rahasia@@h", "***@h"),
                           ("http://h/x@y", "http://h/x@y"), ("http://u@h", "http://***@h"), ("h", "h")] {
            assert_eq!(samarkan_userinfo(t), harap);
        }
        assert!(!urai("http://a:rahasia@@h/").unwrap_err().contains("rahasia"));
        assert!(!crate::dok::klien::urai_url("http://a:rahasia@h/").unwrap_err().contains("rahasia"));
    }

    #[test]
    fn url_ambigu_ditolak() {
        let galat = |s: &str| urai(s).unwrap_err();
        assert!(galat(" http://a/").contains("spasi"));
        assert!(galat("http://a/\tb").contains("kendali"));
        assert!(galat("http://a\\@b/").contains("terbalik"));
        assert!(galat("http://a@b@c/").contains("ambigu"));
        assert!(galat("http:a/").contains("`//`"));
        assert!(galat("//a/b").contains("berskema"));
        for h in ["0x7f.1", "2130706433", "0177.0.0.1", "127.1", "1.2.3.256", "1.2.3.4."] {
            assert!(galat(&format!("http://{}/", h)).contains("tak baku"), "{}", h);
        }
        assert!(galat("http://%31%32%37.0.0.1/").contains("%XX"));
        assert!(galat("http://ä.id/").contains("non-ASCII"));
        assert!(galat("http://a:b:c/").contains("`[ ]`"));
        assert!(galat("http://[fe80::1%25eth0]/").contains("zone"));
        assert!(galat("http://a:0/").contains("port"));
        assert!(galat("http://a:99999/").contains("port"));
        assert!(galat("http://a/%zz").contains("heksa"));
        assert!(galat("http:///x").contains("tanpa nama host"));
        assert!(urai("http://1.2.3.4/").is_ok() && urai("http://a1.b2/").is_ok());
    }

    #[test]
    fn query_urut_dan_berbatas() {
        let p = urai_query("?a=1&a=2;b=3&&c&d=x+y%2B%C3%A9").unwrap();
        let p: Vec<(&str, &str)> = p.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(p, [("a", "1"), ("a", "2;b=3"), ("c", ""), ("d", "x y+é")]);
        assert!(urai_query("a=%ff").unwrap_err().contains("UTF-8"));
        assert!(urai_query("a=%2").unwrap_err().contains("heksa"));
        assert!(urai_query(&"a&".repeat(MAKS_PASANGAN + 1)).unwrap_err().contains("pasangan"));
        let mut o = String::new();
        sandikan("x y+é&=", &mut o);
        assert_eq!(o, "x+y%2B%C3%A9%26%3D");
        assert_eq!(bongkar_persen(&o).unwrap(), "x y+é&=");
    }
}
