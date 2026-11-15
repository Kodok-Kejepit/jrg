// src/dok/http.rs — dok::http — HTTP/1.1 client + server (TCP mentah, tanpa TLS, tanpa deps)

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::interpreter::{err_value, none_value, ok_value, some_value, Interpreter, RuntimeError, Signal, Value};

// ─── Handle helper (sama pola dengan dok::net) ─────────────────────────

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
fn next_id() -> u64 { NEXT_ID.fetch_add(1, Ordering::SeqCst) }

fn make_handle(type_name: &str, id: u64) -> Value {
    let mut fields = HashMap::new();
    fields.insert("id".to_string(), Value::Int(id as i128));
    Value::Struct { name: type_name.to_string(), fields: fields.into() }
}

/// `id` sebuah handle `dok::` berjenis `jenis` — satu definisi untuk router dan soket.
/// Nama struct-nya diperiksa: dulu cuma field `id` yang dibaca, jadi struct pemakai
/// atau dict ber-`id` diam-diam dipakai sebagai router/soket bernomor sama.
pub fn id_handle(v: &Value, fname: &str, jenis: &str) -> Result<u64, String> {
    match v {
        Value::Struct { name, fields } if name == jenis => match fields.get("id") {
            Some(Value::Int(n)) => u64::try_from(*n)
                .map_err(|_| format!("{}: handle `{}` tidak valid (`id` {} bukan nomor handle)", fname, jenis, n)),
            _ => Err(format!("{}: handle `{}` tidak valid (field `id` hilang)", fname, jenis)),
        },
        other => {
            let asal = match jenis {
                "DokRouter" => "dari `dok::router_new()`",
                "TcpConn" => "dari `dok::tcp_connect` atau `dok::tcp_accept`",
                "TcpListener" => "dari `dok::tcp_listen`",
                "UdpSock" => "dari `dok::udp_bind`",
                _ => "handle",
            };
            Err(format!("{}: butuh `{}` ({}), dapat `{}`", fname, jenis, asal, other.type_name()))
        }
    }
}

fn handle_id(v: &Value, fname: &str) -> Result<u64, String> {
    id_handle(v, fname, "DokRouter")
}

/// Nama builtin yang memasang route berkode `metode` — dipakai dua mesin, jadi
/// kalimatnya menyebut fungsi yang DITULIS, bukan `dok::router_*`.
pub fn nama_router(metode: &str) -> &'static str {
    match metode {
        "GET" => "dok::router_get",
        "POST" => "dok::router_post",
        "PUT" => "dok::router_put",
        "DELETE" => "dok::router_delete",
        "PATCH" => "dok::router_patch",
        "*" => "dok::router_any",
        "404" => "dok::router_not_found",
        "USE" => "dok::router_use",
        "AFTER" => "dok::router_after",
        "WS" => "dok::router_ws",
        _ => "dok::router",
    }
}

/// Argumen path sebuah route bukan teks.
pub fn pesan_path_route(nama: &str, dapat: &str) -> String {
    format!("{}: argumen ke-2 (path) harus string, dapat `{}`", nama, dapat)
}

/// Argumen handler sebuah route bukan fungsi.
pub fn pesan_handler_route(nama: &str, ke: usize, dapat: &str) -> String {
    format!("{}: argumen ke-{} (handler) harus fungsi atau closure, dapat `{}`", nama, ke, dapat)
}

// ─── URL encode/decode ──────────────────────────────────────────────────

fn url_encode_str(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => { out.push('%'); out.push_str(&format!("{:02X}", b)); }
        }
    }
    out
}

/// `%XX` dan `+` sebagai teks; `None` kalau byte hasilnya bukan UTF-8 sah.
pub(crate) fn url_decode_teks(s: &str) -> Option<String> { String::from_utf8(url_decode_byte(s)).ok() }

/// Teks atau `nil` untuk sebuah badan, dan array byte-nya kalau ia bukan teks: tepat satu
/// dari dua nilai ini terisi, supaya byte biner tidak pernah sampai sebagai teks rusak.
pub(crate) fn badan_nilai(b: Vec<u8>) -> (Value, Value) {
    match String::from_utf8(b) {
        Ok(t) => (Value::Str(std::sync::Arc::new(t)), Value::Nil),
        Err(e) => (Value::Nil, super::crypto::byte_ke_array(e.into_bytes())),
    }
}

/// Kepala dan badan sebuah jawaban HTTP mentah; kepalanya harus teks.
pub(crate) fn pisah_jawaban(resp: &[u8]) -> Result<(String, Vec<u8>), String> {
    let cari = |pola: &[u8]| resp.windows(pola.len()).position(|w| w == pola);
    let (kepala, badan) = match cari(b"\r\n\r\n") {
        Some(p) => (&resp[..p], &resp[p + 4..]),
        None => match cari(b"\n\n") { Some(p) => (&resp[..p], &resp[p + 2..]), None => (resp, &[][..]) },
    };
    let kepala = String::from_utf8(kepala.to_vec())
        .map_err(|_| "kepala jawaban HTTP bukan teks UTF-8 yang sah".to_string())?;
    // Badan berpotongan dulu sampai apa adanya, angka heksadesimal potongannya ikut di `body`.
    let berpotongan = kepala.lines().skip(1).any(|l| l.split_once(':').map_or(false, |(k, v)|
        k.trim().eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked")));
    if berpotongan { return Ok((kepala, super::klien::urai_berpotongan(badan)?)); }
    Ok((kepala, badan.to_vec()))
}

fn url_decode_byte(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                    if let Ok(b) = u8::from_str_radix(hex, 16) { out.push(b); i += 3; continue; }
                }
                out.push(bytes[i]); i += 1;
            }
            b'+' => { out.push(b' '); i += 1; }
            b => { out.push(b); i += 1; }
        }
    }
    out
}

pub fn url_encode(args: &[Value]) -> Result<Value, String> {
    match args.get(0) {
        Some(Value::Str(s)) => Ok(Value::Str(std::sync::Arc::new(url_encode_str(s)))),
        other => Err(format!("dok::url_encode(s): butuh string, dapat `{}`",
            other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada argumen".to_string()))),
    }
}

pub fn url_decode(args: &[Value]) -> Result<Value, String> {
    match args.get(0) {
        Some(Value::Str(s)) => url_decode_teks(s).map(Value::teks).ok_or_else(|| format!(
            "dok::url_decode: byte hasil `%XX` di `{}` bukan teks UTF-8 yang sah", s)),
        other => Err(format!("dok::url_decode(s): butuh string, dapat `{}`",
            other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada argumen".to_string()))),
    }
}

/// Pasangan query; `None` kalau satu `%XX`-nya bukan teks (permintaannya dijawab 400).
fn parse_query(qs: &str) -> Option<Vec<Value>> {
    if qs.is_empty() { return Some(Vec::new()); }
    // Batas yang sama dengan `url::query_parse`; lebih dari itu dijawab 400.
    if qs.split('&').filter(|p| !p.is_empty()).nth(crate::stdlib::url::MAKS_PASANGAN).is_some() { return None; }
    qs.split('&').filter(|p| !p.is_empty()).map(|pair| {
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (url_decode_teks(k)?, url_decode_teks(v)?),
            None => (url_decode_teks(pair)?, String::new()),
        };
        Some(Value::Tuple(vec![Value::Str(std::sync::Arc::new(k)), Value::Str(std::sync::Arc::new(v))].into()))
    }).collect()
}

pub fn header_get(args: &[Value]) -> Result<Value, String> {
    let arr = match args.get(0) {
        Some(Value::Array(a)) => a,
        _ => return Err("dok::header_get(headers, name)".to_string()),
    };
    let name = match args.get(1) {
        Some(Value::Str(s)) => s.to_lowercase(),
        _ => return Err("dok::header_get(headers, name)".to_string()),
    };
    for item in arr {
        if let Value::Tuple(t) = item {
            if let (Some(Value::Str(k)), Some(v)) = (t.get(0), t.get(1)) {
                if k.to_lowercase() == name { return Ok(some_value(v.clone())); }
            }
        }
    }
    Ok(none_value())
}

pub fn query_get(args: &[Value]) -> Result<Value, String> {
    // Request utuh juga diterima, seperti `cookie_get(req, nama)`; `--doc` mengajarkan bentuk itu.
    let dari_req = match args.get(0) {
        Some(Value::Struct { fields, .. }) => fields.get("query").cloned(),
        _ => None,
    };
    let arr = match dari_req.as_ref().or(args.get(0)) {
        Some(Value::Array(a)) => a,
        _ => return Err("dok::query_get(query, name)".to_string()),
    };
    let name = match args.get(1) {
        Some(Value::Str(s)) => s.as_str(),
        _ => return Err("dok::query_get(query, name)".to_string()),
    };
    for item in arr {
        if let Value::Tuple(t) = item {
            if let (Some(Value::Str(k)), Some(v)) = (t.get(0), t.get(1)) {
                if k.as_str() == name { return Ok(some_value(v.clone())); }
            }
        }
    }
    Ok(none_value())
}

// ─── CLIENT ─────────────────────────────────────────────────────────────

fn http_request_raw(method: &str, url: &str, body: &str, ctype: &str, extra_headers: &[(String, String)]) -> Result<Value, String> {
    let mut kepala: Vec<(String, String)> = Vec::new();
    if !body.is_empty() {
        kepala.push(("Content-Type".into(), if ctype.is_empty() { "application/json" } else { ctype }.to_string()));
    }
    kepala.extend(extra_headers.iter().cloned());
    let response = super::klien::minta(method, url, body.as_bytes(), &kepala, "jrg-dok/1.0")?;
    parse_http_response(&response)
}

fn parse_http_response(resp: &[u8]) -> Result<Value, String> {
    let (head, body) = pisah_jawaban(resp)?;
    let status_line = head.lines().next().unwrap_or("");
    let status: i128 = status_line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);

    let mut headers = Vec::new();
    for line in head.lines().skip(1) {
        if let Some(colon) = line.find(':') {
            headers.push(Value::Tuple(vec![
                Value::Str(std::sync::Arc::new(line[..colon].trim().to_lowercase())),
                Value::Str(std::sync::Arc::new(line[colon + 1..].trim().to_string())),
            ].into()));
        }
    }

    let mut fields = HashMap::new();
    fields.insert("status".to_string(), Value::Int(status));
    let (teks, byte) = badan_nilai(body);
    fields.insert("body".to_string(), teks);
    fields.insert("body_bytes".to_string(), byte);
    fields.insert("ok".to_string(), Value::Bool((200..300).contains(&status)));
    fields.insert("headers".to_string(), Value::Array(headers.into()));
    Ok(Value::Struct { name: "DokHttpResp".to_string(), fields: fields.into() })
}

/// Isi yang DIRENDER (body): nilai apa pun ditampilkan, seperti `wok`.
fn str_arg(v: Option<&Value>) -> String {
    match v { Some(Value::Str(s)) => String::clone(&s.clone()), Some(other) => format!("{}", other), None => String::new() }
}

/// Argumen opsional yang harus teks (content type): yang salah tipe dulu diam-diam
/// jadi bawaannya.
fn teks_opsional(v: Option<&Value>, fungsi: &str, ke: usize, bawaan: &str) -> Result<String, String> {
    match v {
        None => Ok(bawaan.to_string()),
        Some(Value::Str(c)) => Ok(String::clone(&c.clone())),
        Some(lain) => Err(crate::interpreter::pesan_arg_harus_teks(
            fungsi, &format!("argumen ke-{}", ke), &lain.type_name())),
    }
}

pub fn http_get(args: &[Value]) -> Result<Value, String> {
    let url = match args.get(0) { Some(Value::Str(u)) => u.clone(), _ => return Err("dok::http_get(url)".to_string()) };
    match http_request_raw("GET", &url, "", "", &[]) {
        Ok(v) => Ok(ok_value(v)),
        Err(e) => Ok(err_value(format!("dok::http_get: {}", e))),
    }
}

pub fn http_post(args: &[Value]) -> Result<Value, String> {
    let url = match args.get(0) { Some(Value::Str(u)) => u.clone(), _ => return Err("dok::http_post(url, body?, ctype?)".to_string()) };
    let body = str_arg(args.get(1));
    let ctype = teks_opsional(args.get(2), "dok::http_post", 3, "application/json")?;
    match http_request_raw("POST", &url, &body, &ctype, &[]) {
        Ok(v) => Ok(ok_value(v)),
        Err(e) => Ok(err_value(format!("dok::http_post: {}", e))),
    }
}

pub fn http_put(args: &[Value]) -> Result<Value, String> {
    let url = match args.get(0) { Some(Value::Str(u)) => u.clone(), _ => return Err("dok::http_put(url, body?, ctype?)".to_string()) };
    let body = str_arg(args.get(1));
    let ctype = teks_opsional(args.get(2), "dok::http_put", 3, "application/json")?;
    match http_request_raw("PUT", &url, &body, &ctype, &[]) {
        Ok(v) => Ok(ok_value(v)),
        Err(e) => Ok(err_value(format!("dok::http_put: {}", e))),
    }
}

pub fn http_delete(args: &[Value]) -> Result<Value, String> {
    let url = match args.get(0) { Some(Value::Str(u)) => u.clone(), _ => return Err("dok::http_delete(url)".to_string()) };
    match http_request_raw("DELETE", &url, "", "", &[]) {
        Ok(v) => Ok(ok_value(v)),
        Err(e) => Ok(err_value(format!("dok::http_delete: {}", e))),
    }
}

pub fn http_request(args: &[Value]) -> Result<Value, String> {
    let method = match args.get(0) { Some(Value::Str(m)) => m.to_uppercase(), _ => return Err("dok::http_request butuh method".to_string()) };
    let url = match args.get(1) { Some(Value::Str(u)) => u.clone(), _ => return Err("dok::http_request butuh url".to_string()) };
    let body = str_arg(args.get(2));
    let ctype = teks_opsional(args.get(3), "dok::http_request", 4, "application/json")?;
    // Header yang bukan pasangan teks dulu DIBUANG diam-diam (`filter_map`).
    let extra: Vec<(String, String)> = match args.get(4) {
        None => Vec::new(),
        Some(Value::Array(a)) => a.iter().enumerate().map(|(i, v)| match v {
            Value::Tuple(t) if t.len() == 2 => match (&t[0], &t[1]) {
                (Value::Str(k), Value::Str(val)) => Ok((String::clone(k), String::clone(val))),
                _ => Err(format!("dok::http_request: header ke-{} harus pasangan (nama, nilai) \
                                  yang dua-duanya teks", i)),
            },
            lain => Err(format!("dok::http_request: header ke-{} harus pasangan (nama, nilai), \
                                 dapat `{}`", i, lain.type_name())),
        }).collect::<Result<_, _>>()?,
        Some(lain) => return Err(format!(
            "dok::http_request: argumen ke-5 (header) harus array pasangan (nama, nilai), dapat `{}`",
            lain.type_name())),
    };
    match http_request_raw(&method, &url, &body, &ctype, &extra) {
        Ok(v) => Ok(ok_value(v)),
        Err(e) => Ok(err_value(format!("dok::http_request: {}", e))),
    }
}

// ─── Response helpers (murni) ───────────────────────────────────────────

fn make_response(status: i128, body: String, content_type: &str, headers: Vec<Value>) -> Value {
    make_response_badan(status, body.into_bytes(), content_type, headers)
}

/// Badan teks jadi `body`; yang bukan teks jadi `body_bytes` dengan `body` nil (`badan_nilai`).
fn make_response_badan(status: i128, body: Vec<u8>, content_type: &str, headers: Vec<Value>) -> Value {
    let mut fields = HashMap::new();
    let (teks, byte) = badan_nilai(body);
    fields.insert("status".to_string(), Value::Int(status));
    fields.insert("body".to_string(), teks);
    fields.insert("body_bytes".to_string(), byte);
    fields.insert("content_type".to_string(), Value::Str(std::sync::Arc::new(content_type.to_string())));
    fields.insert("headers".to_string(), Value::Array(headers.into()));
    Value::Struct { name: "DokResponse".to_string(), fields: fields.into() }
}

pub fn response(args: &[Value]) -> Result<Value, String> {
    let status = match args.get(0) { Some(Value::Int(n)) => *n, _ => return Err("dok::response(status, body, content_type?)".to_string()) };
    let body = str_arg(args.get(1));
    let ctype = teks_opsional(args.get(2), "dok::response", 3, "text/plain; charset=utf-8")?;
    Ok(make_response(status, body, &ctype, Vec::new()))
}

/// `dok::binary(byte, content_type?)`: jawaban 200 berisi byte apa adanya.
pub fn binary(args: &[Value]) -> Result<Value, String> {
    let isi = match args.get(0) {
        Some(Value::Array(a)) => super::crypto::array_ke_byte(a, "dok::binary")?,
        lain => return Err(format!("dok::binary: argumen ke-1 harus array byte, dapat `{}`",
            lain.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".to_string()))),
    };
    let ctype = teks_opsional(args.get(1), "dok::binary", 2, "application/octet-stream")?;
    Ok(make_response_badan(200, isi, &ctype, Vec::new()))
}

pub fn text(args: &[Value]) -> Result<Value, String> {
    Ok(make_response(200, str_arg(args.get(0)), "text/plain; charset=utf-8", Vec::new()))
}

pub fn html(args: &[Value]) -> Result<Value, String> {
    Ok(make_response(200, str_arg(args.get(0)), "text/html; charset=utf-8", Vec::new()))
}

pub fn json_response(args: &[Value]) -> Result<Value, String> {
    let v = args.get(0).cloned().unwrap_or(Value::Nil);
    Ok(make_response(200, super::json::encode(&v)?, "application/json", Vec::new()))
}

pub fn redirect(args: &[Value]) -> Result<Value, String> {
    // URL tujuan adalah ISI, bukan teks yang dirender: dulu `dok::redirect(nil)`
    // mengirim pembacanya ke halaman bernama "nil".
    let url = match args.get(0) {
        Some(Value::Str(u)) => u.clone(),
        lain => return Err(crate::interpreter::pesan_arg_harus_teks("dok::redirect", "URL tujuannya",
            &lain.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".to_string()))),
    };
    let status = match args.get(1) {
        None => 302,
        Some(Value::Int(n)) => *n,
        Some(lain) => return Err(format!(
            "dok::redirect: argumen ke-2 (status) harus bilangan bulat, dapat `{}`", lain.type_name())),
    };
    // Kode lain tidak membuat peramban berpindah, jadi `Location`-nya diam-diam tak terpakai.
    if !matches!(status, 301 | 302 | 303 | 307 | 308) {
        return Err(format!("dok::redirect: status {} bukan kode pengalihan — yang sah 301, 302, 303, 307, 308", status));
    }
    if let Some(i) = url.bytes().position(|c| c < 0x20 || c == 0x7f) {
        return Err(format!("dok::redirect: URL tujuannya memuat karakter kendali di posisi {} — kepala `Location` \
                            bisa disisipi", i));
    }
    Ok(make_response(status, String::new(), "text/plain", vec![Value::Tuple(vec![Value::Str(std::sync::Arc::new("Location".to_string())), Value::Str(url)].into())]))
}

// ─── SERVER: router registry ────────────────────────────────────────────

#[derive(Clone)]
enum PatSeg { Lit(String), Param(String) }

/// Sebuah handler route, berikut BACKEND yang bisa memanggilnya.
#[derive(Clone)]
pub enum Handler {
    /// Fungsi milik interpreter, disimpan apa adanya.
    Interp(Value),
    /// Indeks ke tabel handler milik backend VM (`stdlib::dok`).
    Vm(u64),
}

/// Cara memanggil sebuah handler, dimiliki penuh oleh satu koneksi.
pub type PemanggilHandler =
    Box<dyn FnMut(&Handler, Vec<Value>) -> Result<Value, String> + Send>;

#[derive(Clone)]
struct RouteEntry { method: String, segs: Vec<PatSeg>, handler: Handler }

#[derive(Clone, Default)]
struct RouterData {
    routes: Vec<RouteEntry>,
    not_found: Option<Handler>,
    static_mounts: Vec<(String, PathBuf)>,
    /// Dijalankan SEBELUM route dicari, urut pemasangan.
    sebelum: Vec<Handler>,
    /// Dijalankan SESUDAH handler, urut pemasangan, menerima `(req, resp)`.
    sesudah: Vec<Handler>,
    /// Route WebSocket: pola path + handler yang menerima koneksinya.
    ws: Vec<(Vec<PatSeg>, Handler)>,
}

static ROUTERS: OnceLock<Mutex<HashMap<u64, RouterData>>> = OnceLock::new();
fn routers() -> &'static Mutex<HashMap<u64, RouterData>> {
    ROUTERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Jalur yang dibaca SAMA oleh router dan oleh pemeriksa izin di middleware: router membuang segmen kosong dan
/// mencocokkan sesudah `%XX` dibongkar, jadi `//x`, `/./x` dan `%2F` membuat `starts_with(req.path, "/x")` tidak
/// melihat rute yang tetap dicocokkan (kelas missing authorization, CWE-862). Bentuk itu ditolak 400 di pintu.
pub(crate) fn jalur_satu_ejaan(mentah: &str, jalur: &str) -> bool {
    let kecil = mentah.to_ascii_lowercase();
    if !jalur.starts_with('/') || jalur.contains('\\') || kecil.contains("%2f") || kecil.contains("%5c") { return false; }
    let seg: Vec<&str> = jalur[1..].split('/').collect();
    seg.iter().enumerate().all(|(i, s)| *s != "." && *s != ".." && (!s.is_empty() || i == seg.len() - 1))
}

fn compile_pattern(path: &str) -> Vec<PatSeg> {
    path.trim_matches('/').split('/').filter(|s| !s.is_empty()).map(|seg| {
        match seg.strip_prefix(':') {
            Some(name) => PatSeg::Param(name.to_string()),
            None => PatSeg::Lit(seg.to_string()),
        }
    }).collect()
}

fn match_route(segs: &[PatSeg], path: &str) -> Option<Vec<(String, String)>> {
    let parts: Vec<&str> = path.trim_matches('/').split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() != segs.len() { return None; }
    let mut params = Vec::new();
    for (seg, part) in segs.iter().zip(parts.iter()) {
        match seg {
            PatSeg::Lit(l) => if l != part { return None; },
            PatSeg::Param(name) => params.push((name.clone(), part.to_string())),
        }
    }
    Some(params)
}

pub fn router_new(_args: &[Value]) -> Result<Value, String> {
    let id = next_id();
    routers().lock().unwrap_or_else(|p| p.into_inner()).insert(id, RouterData::default());
    Ok(make_handle("DokRouter", id))
}

fn router_add_method(args: &[Value], method: &str) -> Result<Value, String> {
    let nama = nama_router(method);
    let id = handle_id(args.get(0).unwrap_or(&Value::Nil), nama)?;
    let path = match args.get(1) {
        Some(Value::Str(s)) => s.clone(),
        lain => return Err(pesan_path_route(nama, &lain.map(|v| v.type_name()).unwrap_or_default())),
    };
    pasang_route(id, method, &path, handler_interp(args.get(2), nama, 3)?)
}

fn handler_interp(v: Option<&Value>, nama: &str, ke: usize) -> Result<Handler, String> {
    match v {
        Some(v) if v.bisa_dipanggil() => Ok(Handler::Interp(v.clone())),
        lain => Err(pesan_handler_route(nama, ke, &lain.map(|v| v.type_name()).unwrap_or_default())),
    }
}

/// Menyimpan satu route. Dipakai kedua backend, jadi aturan "router mana,
/// pola apa" cuma punya satu definisi; yang berbeda antar-backend hanyalah
/// dari mana `handler` datang.
pub fn pasang_route(id: u64, method: &str, path: &str, handler: Handler) -> Result<Value, String> {
    let mut guard = routers().lock().unwrap_or_else(|p| p.into_inner());
    match guard.get_mut(&id) {
        Some(r) => {
            r.routes.push(RouteEntry { method: method.to_string(), segs: compile_pattern(path), handler });
            Ok(Value::Unit)
        }
        None => Err(format!("{}: router tidak ditemukan", nama_router(method))),
    }
}

/// `id` router dari sebuah handle, untuk sisi VM yang tidak lewat `args`.
pub fn id_router(v: &Value, siapa: &str) -> Result<u64, String> { handle_id(v, siapa) }

/// Pasang handler 404 dari sisi mana pun.
pub fn pasang_not_found(id: u64, handler: Handler) -> Result<Value, String> {
    let mut guard = routers().lock().unwrap_or_else(|p| p.into_inner());
    match guard.get_mut(&id) {
        Some(r) => { r.not_found = Some(handler); Ok(Value::Unit) }
        None => Err("dok::router_not_found: router tidak ditemukan".to_string()),
    }
}

/// Pasang sebuah middleware. `sebelum = true` untuk `dok::router_use`,
/// `false` untuk `dok::router_after`.
pub fn pasang_middleware(id: u64, handler: Handler, sebelum: bool) -> Result<Value, String> {
    let mut guard = routers().lock().unwrap_or_else(|p| p.into_inner());
    match guard.get_mut(&id) {
        Some(r) => {
            if sebelum { r.sebelum.push(handler); } else { r.sesudah.push(handler); }
            Ok(Value::Unit)
        }
        None => Err(format!("{}: router tidak ditemukan",
                            if sebelum { "dok::router_use" } else { "dok::router_after" })),
    }
}

fn router_middleware(args: &[Value], sebelum: bool) -> Result<Value, String> {
    let nama = if sebelum { "dok::router_use" } else { "dok::router_after" };
    let id = handle_id(args.get(0).unwrap_or(&Value::Nil), nama)?;
    pasang_middleware(id, handler_interp(args.get(1), nama, 2)?, sebelum)
}

/// `dok::router_use(r, f)` — middleware yang jalan SEBELUM route dicari.
pub fn router_ws(args: &[Value]) -> Result<Value, String> {
    const NAMA: &str = "dok::router_ws";
    let id = handle_id(args.get(0).unwrap_or(&Value::Nil), NAMA)?;
    let path = match args.get(1) {
        Some(Value::Str(s)) => s.clone(),
        lain => return Err(pesan_path_route(NAMA, &lain.map(|v| v.type_name()).unwrap_or_default())),
    };
    pasang_ws(id, &path, handler_interp(args.get(2), NAMA, 3)?)
}

pub fn pasang_ws(id: u64, path: &str, handler: Handler) -> Result<Value, String> {
    let mut guard = routers().lock().unwrap_or_else(|p| p.into_inner());
    match guard.get_mut(&id) {
        Some(r) => { r.ws.push((compile_pattern(path), handler)); Ok(Value::Unit) }
        None => Err("dok::router_ws: router tidak ditemukan".to_string()),
    }
}

pub fn router_use(args: &[Value]) -> Result<Value, String> { router_middleware(args, true) }

/// `dok::router_after(r, f)` — middleware yang jalan SESUDAH handler.
pub fn router_after(args: &[Value]) -> Result<Value, String> { router_middleware(args, false) }

pub fn router_get(args: &[Value]) -> Result<Value, String> { router_add_method(args, "GET") }
pub fn router_post(args: &[Value]) -> Result<Value, String> { router_add_method(args, "POST") }
pub fn router_put(args: &[Value]) -> Result<Value, String> { router_add_method(args, "PUT") }
pub fn router_delete(args: &[Value]) -> Result<Value, String> { router_add_method(args, "DELETE") }
pub fn router_patch(args: &[Value]) -> Result<Value, String> { router_add_method(args, "PATCH") }
pub fn router_any(args: &[Value]) -> Result<Value, String> { router_add_method(args, "*") }

pub fn router_not_found(args: &[Value]) -> Result<Value, String> {
    const NAMA: &str = "dok::router_not_found";
    let id = handle_id(args.get(0).unwrap_or(&Value::Nil), NAMA)?;
    pasang_not_found(id, handler_interp(args.get(1), NAMA, 2)?)
}

pub fn router_static(args: &[Value]) -> Result<Value, String> {
    let id = handle_id(args.get(0).unwrap_or(&Value::Nil), "dok::router_static")?;
    let teks = |i: usize, peran: &str| match args.get(i) {
        Some(Value::Str(s)) => Ok(s.to_string()),
        lain => Err(format!("dok::router_static: argumen ke-{} ({}) harus string, dapat `{}`",
                            i + 1, peran, lain.map(|v| v.type_name()).unwrap_or_default())),
    };
    let (prefix, dir) = (teks(1, "awalan URL")?, teks(2, "direktori")?);
    let mut guard = routers().lock().unwrap_or_else(|p| p.into_inner());
    match guard.get_mut(&id) {
        Some(r) => { r.static_mounts.push((prefix, PathBuf::from(dir))); Ok(Value::Unit) }
        None => Err("dok::router_static: router tidak ditemukan".to_string()),
    }
}

// ─── SERVER: request/response wire format ───────────────────────────────

struct ParsedRequest {
    method: String,
    path: String,
    query_string: String,
    headers: Vec<(String, String)>,
    query: Vec<Value>,
    body: Vec<u8>,
    /// Bagian anggaran badan yang dipegang permintaan ini sampai koneksinya selesai.
    _pesanan: Pesanan,
}

/// Byte badan yang sedang dipegang semua koneksi; anggarannya `MAKS_BADAN_SERENTAK`.
static BADAN_TERPAKAI: AtomicUsize = AtomicUsize::new(0);
/// N badan sebesar `MAKS_BADAN` sekaligus dulu menghabiskan memori, dan alokasi yang gagal menghentikan server.
pub const MAKS_BADAN_SERENTAK: usize = 256 * 1024 * 1024;
/// Koneksi hidup paling banyak; sisanya dijawab 503 tanpa membuat utas.
pub const MAKS_SAMBUNGAN: usize = 512;
const PENUH: Tolak = (503, "Service Unavailable");

/// Potongan anggaran yang dikembalikan saat dilepas.
struct Pesanan { hitung: &'static AtomicUsize, n: usize }

impl Pesanan {
    fn baru(hitung: &'static AtomicUsize) -> Self { Pesanan { hitung, n: 0 } }
    /// Menambah `k` byte kalau totalnya tetap di bawah `batas`.
    fn tambah(&mut self, k: usize, batas: usize) -> bool {
        let ok = self.hitung
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |t| t.checked_add(k).filter(|b| *b <= batas))
            .is_ok();
        if ok { self.n += k; }
        ok
    }
}

impl Drop for Pesanan {
    fn drop(&mut self) { self.hitung.fetch_sub(self.n, Ordering::AcqRel); }
}

/// Batas permintaan masuk. Tanpa ini satu klien menentukan berapa memori yang dipesan server.
const MAKS_BARIS: usize = 8 * 1024;
const MAKS_HEADER: usize = 100;
pub const MAKS_BADAN: usize = 64 * 1024 * 1024;
const TUNGGU_PERMINTAAN: std::time::Duration = std::time::Duration::from_secs(30);
/// Tiap byte yang datang menambah tenggat seluruh permintaan sebesar 1/`LAJU_MIN` detik.
const LAJU_MIN: f64 = 4096.0;

/// Batas tunggu per `read` saja dulu bisa ditahan klien yang menetes satu byte tiap 25 s (Slowloris,
/// kelas CVE-2018-12122) — 512 koneksi seperti itu membuat server menjawab 503 ke semua orang.
struct Tenggat { s: TcpStream, mulai: std::time::Instant, dibaca: u64, tunggu: std::time::Duration }

impl Tenggat {
    fn baru(s: TcpStream, tunggu: std::time::Duration) -> Self {
        Tenggat { s, mulai: std::time::Instant::now(), dibaca: 0, tunggu }
    }
}

impl Read for Tenggat {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let batas = self.mulai + self.tunggu + std::time::Duration::from_secs_f64(self.dibaca as f64 / LAJU_MIN);
        let sisa = batas.saturating_duration_since(std::time::Instant::now());
        if sisa.is_zero() { return Err(std::io::ErrorKind::TimedOut.into()); }
        self.s.set_read_timeout(Some(sisa.min(self.tunggu)))?;
        let n = self.s.read(buf)?;
        self.dibaca += n as u64;
        Ok(n)
    }
}

/// Permintaan yang ditolak sebelum sampai ke handler: status dan kalimatnya.
type Tolak = (i128, &'static str);

fn galat_baca(e: std::io::Error) -> Tolak {
    match e.kind() {
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => (408, "Request Timeout"),
        _ => (400, "Bad Request"),
    }
}

/// Isi baris tanpa CRLF penutupnya; LF tanpa CR dan CR telanjang di tengah jadi `None` (dibaca lain oleh proxy).
fn tanpa_crlf(s: &str) -> Option<&str> {
    let isi = s.strip_suffix("\r\n")?;
    (!isi.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0)).then_some(isi)
}

/// Satu baris, paling panjang `MAKS_BARIS` byte; `None` = koneksi sudah ditutup.
fn baca_baris<R: BufRead>(reader: &mut R, lewat: Tolak) -> Result<Option<String>, Tolak> {
    let mut buf = Vec::new();
    let n = (&mut *reader).take(MAKS_BARIS as u64 + 1).read_until(b'\n', &mut buf).map_err(galat_baca)?;
    if n == 0 { return Ok(None); }
    if buf.len() > MAKS_BARIS { return Err(lewat); }
    String::from_utf8(buf).map(Some).map_err(|_| (400, "Bad Request"))
}

fn parse_request<R: BufRead>(reader: &mut R) -> Result<ParsedRequest, Tolak> {
    parse_request_dgn(reader, &BADAN_TERPAKAI, MAKS_BADAN_SERENTAK)
}

fn parse_request_dgn<R: BufRead>(reader: &mut R, hitung: &'static AtomicUsize, batas: usize) -> Result<ParsedRequest, Tolak> {
    const SALAH: Tolak = (400, "Bad Request");
    let line = baca_baris(reader, (414, "URI Too Long"))?.ok_or(SALAH)?;
    let line = tanpa_crlf(&line).ok_or(SALAH)?;
    // RFC 9112 §3: tepat `metode SP target SP versi`; tab, spasi ganda, bagian keempat dan bentuk absolut dibaca
    // lain oleh proxy (kelas request smuggling), jadi ditolak.
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(versi), None) = (parts.next(), parts.next(), parts.next(), parts.next())
        else { return Err(SALAH) };
    if !nama_kepala_sah(method) || !target.starts_with('/') || target.bytes().any(|b| b < 0x21 || b == 0x7f) {
        return Err(SALAH);
    }
    match versi {
        "HTTP/1.1" | "HTTP/1.0" => {}
        v if v.starts_with("HTTP/") => return Err((505, "HTTP Version Not Supported")),
        _ => return Err(SALAH),
    }
    let (method, target) = (method.to_string(), target.to_string());

    let (raw_path, query_string) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target.clone(), String::new()),
    };
    let path = url_decode_teks(&raw_path).ok_or(SALAH)?;
    if !jalur_satu_ejaan(&raw_path, &path) { return Err(SALAH); }
    let query = parse_query(&query_string).ok_or(SALAH)?;

    let besar: Tolak = (431, "Request Header Fields Too Large");
    let mut headers = Vec::new();
    let mut content_length: Option<usize> = None;
    loop {
        let hline = baca_baris(reader, besar)?.ok_or(SALAH)?;
        let hline = tanpa_crlf(&hline).ok_or(SALAH)?;
        if hline.is_empty() { break; }
        if headers.len() >= MAKS_HEADER { return Err(besar); }
        // RFC 9112 §5: nama tanpa spasi sebelum `:`, tanpa lipatan baris — dua bentuk yang dibaca lain oleh proxy.
        let Some(colon) = hline.find(':') else { return Err(SALAH) };
        if !nama_kepala_sah(&hline[..colon]) { return Err(SALAH); }
        {
            let k = hline[..colon].to_lowercase();
            let v = hline[colon + 1..].trim_matches(|c| c == ' ' || c == '\t').to_string();
            // RFC 9110 §5.5: kendali selain tab di nilai kepala ditolak, tidak diteruskan ke handler.
            if v.bytes().any(|b| (b < 0x20 && b != b'\t') || b == 0x7f) { return Err(SALAH); }
            // RFC 9112 §3.2: `Host` kembar atau tak sah dijawab 400.
            if k == "host" && (headers.iter().any(|(n, _): &(String, String)| n == "host")
                || crate::stdlib::url::urai_kepala_host(&v).is_err()) { return Err(SALAH); }
            // Badan berpotongan belum dibaca server ini: dulu badannya diam-diam kosong.
            if k == "transfer-encoding" { return Err((501, "Not Implemented")); }
            if k == "content-length" {
                // Cuma digit: `parse` sendiri menerima `+5`, dan yang tak terbaca dulu dianggap 0.
                if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) { return Err(SALAH); }
                let n = v.parse::<usize>().ok().filter(|n| *n <= MAKS_BADAN)
                    .ok_or((413, "Payload Too Large"))?;
                if content_length.map_or(false, |lama| lama != n) { return Err(SALAH); }
                content_length = Some(n);
            }
            headers.push((k, v));
        }
    }

    // Badan dibaca sebanyak yang DATANG, bukan dipesan dulu sebesar yang diaku klien.
    // Anggarannya dipesan per potongan yang datang, jadi klien yang diam tidak memegang lebih dari yang ia kirim.
    let mut buf = Vec::new();
    let mut pesanan = Pesanan::baru(hitung);
    let n = content_length.unwrap_or(0);
    let mut potongan = [0u8; 64 * 1024];
    while buf.len() < n {
        let mau = (n - buf.len()).min(potongan.len());
        let k = reader.read(&mut potongan[..mau]).map_err(galat_baca)?;
        if k == 0 { return Err(SALAH); }
        if !pesanan.tambah(k, batas) || buf.try_reserve(k).is_err() { return Err(PENUH); }
        buf.extend_from_slice(&potongan[..k]);
    }
    // Badan yang bukan teks jadi array `Value` sebyte satu (`body_bytes`), jadi harganya dihitung sebesar itu.
    if std::str::from_utf8(&buf).is_err() {
        let harga = n.saturating_mul(std::mem::size_of::<Value>());
        if harga.saturating_add(n) > batas { return Err((413, "Payload Too Large")); }
        if !pesanan.tambah(harga, batas) { return Err(PENUH); }
    }
    Ok(ParsedRequest { method, path, query_string, headers, query, body: buf, _pesanan: pesanan })
}

fn status_reason(status: i128) -> &'static str {
    match status {
        200 => "OK", 201 => "Created", 202 => "Accepted", 204 => "No Content",
        301 => "Moved Permanently", 302 => "Found", 303 => "See Other", 304 => "Not Modified",
        307 => "Temporary Redirect", 308 => "Permanent Redirect",
        400 => "Bad Request", 401 => "Unauthorized", 403 => "Forbidden", 404 => "Not Found",
        405 => "Method Not Allowed", 406 => "Not Acceptable", 408 => "Request Timeout",
        409 => "Conflict", 410 => "Gone", 413 => "Payload Too Large", 414 => "URI Too Long", 415 => "Unsupported Media Type",
        422 => "Unprocessable Entity", 429 => "Too Many Requests", 431 => "Request Header Fields Too Large",
        500 => "Internal Server Error", 501 => "Not Implemented", 502 => "Bad Gateway",
        503 => "Service Unavailable", 504 => "Gateway Timeout", 505 => "HTTP Version Not Supported",
        _ => "Unknown",
    }
}

fn write_response(stream: &mut TcpStream, status: i128, content_type: &str, body: &str, extra_headers: &[(String, String)]) {
    write_response_bytes(stream, status, content_type, body.as_bytes(), extra_headers)
}

fn write_response_bytes(stream: &mut TcpStream, status: i128, content_type: &str, body: &[u8], extra_headers: &[(String, String)]) {
    let mut out = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        status, status_reason(status), content_type, body.len()
    );
    for (k, v) in extra_headers { out.push_str(&format!("{}: {}\r\n", k, v)); }
    out.push_str("\r\n");
    let mut semua = out.into_bytes();
    semua.extend_from_slice(body);
    let _ = stream.write_all(&semua);
    let _ = stream.flush();
}

fn guess_content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css",
        "js" => "application/javascript",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "txt" => "text/plain; charset=utf-8",
        "pdf" => "application/pdf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "wasm" => "application/wasm",
        "xml" => "application/xml",
        _ => "application/octet-stream",
    }
}

/// Coba layani file statis. `None` kalau tidak ada mount yang cocok atau
/// filenya tidak ada — supaya caller bisa lanjut ke pencarian route biasa.
fn try_serve_static(mounts: &[(String, PathBuf)], path: &str) -> Option<(i128, String, Vec<u8>)> {
    for (prefix, dir) in mounts {
        let prefix_norm = if prefix.ends_with('/') { prefix.clone() } else { format!("{}/", prefix) };
        let path_norm = if path.ends_with('/') { path.to_string() } else { format!("{}/", path) };
        if !path_norm.starts_with(&prefix_norm) && path != prefix.trim_end_matches('/') { continue; }
        let rel = path.strip_prefix(prefix.as_str()).unwrap_or("").trim_start_matches('/');
        let sasaran = if rel.is_empty() { "index.html" } else { rel };
        let Some(dasar) = dir.to_str() else { continue };
        // `gabung_aman` menolak `..`, `\`, NUL, dan tautan simbolik yang keluar dari `dir`.
        let Ok(penuh) = crate::stdlib::fs::gabung_aman(dasar, sasaran) else {
            return Some((403, "text/plain".to_string(), b"Forbidden".to_vec()));
        };
        let full = Path::new(&penuh);
        if full.is_file() {
            if let Ok(m) = std::fs::metadata(full) {
                if m.len() > MAKS_BADAN as u64 {
                    return Some((413, "text/plain".to_string(), b"Payload Too Large".to_vec()));
                }
            }
            if let Ok(bytes) = std::fs::read(full) {
                let ctype = guess_content_type(full);
                return Some((200, ctype.to_string(), bytes));
            }
        }
    }
    None
}

// ─── SERVER: dispatch ke handler JRG ─────────────────────────────────────

/// Kebalikan pembacaan header di `value_to_response`.
fn header_ke_value(h: Vec<(String, String)>) -> Vec<Value> {
    h.into_iter()
        .map(|(k, v)| Value::Tuple(vec![Value::Str(std::sync::Arc::new(k)), Value::Str(std::sync::Arc::new(v))].into()))
        .collect()
}

/// Nama struct sebuah nilai, atau `""`.
fn nama_struct(v: &Value) -> &str {
    match v { Value::Struct { name, .. } => name.as_str(), _ => "" }
}

/// Baca satu ladang string dari sebuah struct.
fn ambil_str(v: &Value, ladang: &str) -> Option<String> {
    match v {
        Value::Struct { fields, .. } => match fields.get(ladang) {
            Some(Value::Str(s)) => Some(String::clone(&s.clone())),
            _ => None,
        },
        _ => None,
    }
}

/// Pasang `params` hasil pencocokan route ke dalam request.
fn pasang_params(v: Value, params: Vec<(String, String)>) -> Value {
    let Value::Struct { name, fields } = v else { return v };
    let mut f = (*fields).clone();
    let mut pf: HashMap<String, Value> = HashMap::new();
    for (k, val) in params { pf.insert(k, Value::Str(std::sync::Arc::new(val))); }
    f.insert("params".to_string(),
        Value::Struct { name: super::json::OBJ_NAME.to_string(), fields: pf.into() });
    Value::Struct { name, fields: f.into() }
}

fn value_to_response(v: Value) -> (i128, String, Vec<u8>, Vec<(String, String)>) {
    match v {
        Value::Struct { name, fields } if name == "DokResponse" => {
            let status = match fields.get("status") { Some(Value::Int(n)) => *n, _ => 200 };
            let body = match (fields.get("body_bytes"), fields.get("body")) {
                (Some(Value::Array(a)), _) => match super::crypto::array_ke_byte(a, "body_bytes") {
                    Ok(b) => b,
                    Err(e) => return (500, "text/plain; charset=utf-8".to_string(), e.into_bytes(), Vec::new()),
                },
                (_, Some(Value::Str(s))) => s.as_bytes().to_vec(),
                (_, Some(Value::Nil)) | (_, None) => Vec::new(),
                (_, Some(o)) => format!("{}", o).into_bytes(),
            };
            let ctype = match fields.get("content_type") { Some(Value::Str(s)) => s.clone(), _ => std::sync::Arc::new("text/plain; charset=utf-8".to_string()) };
            let headers = match fields.get("headers") {
                Some(Value::Array(a)) => a.iter().filter_map(|v| match v {
                    Value::Tuple(t) if t.len() == 2 => match (&t[0], &t[1]) {
                        (Value::Str(k), Value::Str(val)) => Some((String::clone(k), String::clone(val))),
                        _ => None,
                    },
                    _ => None,
                }).collect(),
                _ => Vec::new(),
            };
            (status, String::clone(&ctype), body, headers)
        }
        Value::Enum { variant, payload, .. } if variant == "Ok" => {
            value_to_response(payload.map(|b| *b).unwrap_or(Value::Nil))
        }
        Value::Enum { variant, payload, .. } if variant == "Err" => {
            let msg = payload.map(|b| format!("{}", *b)).unwrap_or_else(|| "error".to_string());
            (500, "text/plain; charset=utf-8".to_string(), msg.into_bytes(), Vec::new())
        }
        Value::Str(s) => (200, "text/plain; charset=utf-8".to_string(), s.as_bytes().to_vec(), Vec::new()),
        other => (200, "text/plain; charset=utf-8".to_string(), format!("{}", other).into_bytes(), Vec::new()),
    }
}

fn build_request_value(pr: &ParsedRequest, body: Vec<u8>, params: Vec<(String, String)>, remote_addr: &str) -> Value {
    let mut fields = HashMap::new();
    fields.insert("method".to_string(), Value::Str(std::sync::Arc::new(pr.method.clone())));
    fields.insert("path".to_string(), Value::Str(std::sync::Arc::new(pr.path.clone())));
    fields.insert("query_string".to_string(), Value::Str(std::sync::Arc::new(pr.query_string.clone())));
    fields.insert("query".to_string(), Value::Array(pr.query.clone().into()));
    let (teks, byte) = badan_nilai(body);
    fields.insert("body".to_string(), teks);
    fields.insert("body_bytes".to_string(), byte);
    fields.insert("remote_addr".to_string(), Value::Str(std::sync::Arc::new(remote_addr.to_string())));
    fields.insert("headers".to_string(), Value::Array(
        pr.headers.iter().map(|(k, v)| Value::Tuple(vec![Value::Str(std::sync::Arc::new(k.clone())), Value::Str(std::sync::Arc::new(v.clone()))].into())).collect()
    ));
    let mut param_fields = HashMap::new();
    for (k, v) in params { param_fields.insert(k, Value::Str(std::sync::Arc::new(v))); }
    fields.insert("params".to_string(), Value::Struct { name: super::json::OBJ_NAME.to_string(), fields: param_fields.into() });
    Value::Struct { name: "DokRequest".to_string(), fields: fields.into() }
}

fn handle_connection(panggil: &mut PemanggilHandler, router_id: u64, mut stream: TcpStream) {
    let remote_addr = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();
    let salinan = match stream.try_clone() { Ok(s) => s, Err(_) => return };
    let mut reader = BufReader::new(Tenggat::baru(salinan, TUNGGU_PERMINTAAN));

    // Klien yang diam atau menetes tidak boleh menahan thread; batasnya dilepas sesudah permintaan terbaca.
    let pr = match parse_request(&mut reader) {
        Ok(p) => p,
        Err((kode, teks)) => { write_response(&mut stream, kode, "text/plain", teks, &[]); return; }
    };
    stream.set_read_timeout(None).ok();

    let router = { routers().lock().unwrap_or_else(|p| p.into_inner()).get(&router_id).cloned() };
    let router = match router {
        Some(r) => r,
        None => { write_response(&mut stream, 500, "text/plain", "router hilang", &[]); return; }
    };

    // 1) Middleware `sebelum`, urut pemasangan — SEBELUM file statis, supaya
    // sebuah middleware otentikasi benar-benar menutupi semuanya.
    let mut pr = pr;
    let badan = std::mem::take(&mut pr.body);
    let mut req_val = build_request_value(&pr, badan, Vec::new(), &remote_addr);
    let mut hasil_awal: Option<Value> = None;
    for mw in &router.sebelum {
        match panggil(mw, vec![req_val.clone()]) {
            Ok(Value::Nil) | Ok(Value::Unit) => {}
            Ok(v @ Value::Struct { .. }) if nama_struct(&v) == "DokRequest" => req_val = v,
            Ok(v) => { hasil_awal = Some(v); break; }
            Err(e) => {
                hasil_awal = Some(make_response(
                    500, format!("middleware error: {}", e),
                    "text/plain; charset=utf-8", Vec::new()));
                break;
            }
        }
    }

    // ── Upgrade ke WebSocket ─────────────────────────────────────────
    if hasil_awal.is_none() && !router.ws.is_empty() {
        if let Some(kunci) = super::ws::minta_upgrade(&pr.headers) {
            let jalur = ambil_str(&req_val, "path").unwrap_or_else(|| pr.path.clone());
            let cocok = router.ws.iter()
                .find_map(|(segs, h)| match_route(segs, &jalur).map(|p| (h.clone(), p)));
            if let Some((handler, params)) = cocok {
                if !super::ws::origin_sah(&pr.headers) {
                    write_response(&mut stream, 403, "text/plain; charset=utf-8",
                        "Origin WebSocket ditolak: host-nya harus sama dengan Host", &[]);
                    return;
                }
                let s2 = match stream.try_clone() { Ok(s) => s, Err(_) => return };
                let id = match super::ws::terima(s2, &kunci) { Ok(i) => i, Err(_) => return };
                let req = pasang_params(req_val.clone(), params);
                let conn = super::ws::nilai_handle(id);
                // Nilai balik handler diabaikan: sesudah upgrade tidak ada
                // respons HTTP yang bisa ditulis, dan mengarang satu justru
                // akan mengirim byte HTTP ke tengah aliran frame.
                let _ = panggil(&handler, vec![conn, req]);
                super::ws::lepas(id);
                return;
            }
        }
    }

    // `path`/`method` dibaca ULANG dari request sesudah middleware: sebuah
    // middleware yang menulis ulang URL harus benar-benar mengubah route mana
    // yang dipilih, bukan cuma apa yang dilihat handler.
    let path_kini = ambil_str(&req_val, "path").unwrap_or_else(|| pr.path.clone());
    let method_kini = ambil_str(&req_val, "method").unwrap_or_else(|| pr.method.clone());

    // Berkas statis yang BUKAN teks (font, gambar) disimpan sebagai byte: dulu ia dipaksa jadi
    // teks UTF-8 dan tiap byte tak sah diganti tiga byte — berkasnya rusak tanpa suara.
    let (status, ctype, body, extra_headers) = if let Some(v) = hasil_awal {
        value_to_response(v)
    } else if let Some((status, ctype, bytes)) =
        try_serve_static(&router.static_mounts, &path_kini)
    {
        // 2) File statis, supaya route dinamis tidak perlu tahu-menahu soal itu.
        (status, ctype, bytes, Vec::new())
    } else {
        // 3) Cari route yang cocok (method + pattern path).
        let mut matched: Option<(Handler, Vec<(String, String)>)> = None;
        for route in &router.routes {
            if route.method != "*" && route.method != method_kini { continue; }
            if let Some(params) = match_route(&route.segs, &path_kini) {
                matched = Some((route.handler.clone(), params));
                break;
            }
        }
        match matched {
            Some((handler, params)) => {
                let req = pasang_params(req_val.clone(), params);
                req_val = req.clone();
                match panggil(&handler, vec![req]) {
                    Ok(v) => value_to_response(v),
                    // Jejaknya juga di stderr pelayan: klien yang tidak membaca badan 500 tidak
                    // meninggalkan apa pun, dan di APK cuma stderr yang sampai ke logcat.
                    Err(e) => {
                        // Awalan milik interpreter dibuang: badan 500 sama di dua mesin.
                        let e = e.to_string();
                        let e = e.strip_prefix("runtime error: ").unwrap_or(&e);
                        crate::host::galat_baris(&format!(
                            "dok::serve: {} {} gagal di handler: {}", method_kini, path_kini, e));
                        (500, "text/plain; charset=utf-8".to_string(), format!("handler error: {}", e).into_bytes(), Vec::new())
                    }
                }
            }
            None => match router.not_found.clone() {
                Some(handler) => match panggil(&handler, vec![req_val.clone()]) {
                    Ok(v) => value_to_response(v),
                    Err(_) => (404, "text/plain; charset=utf-8".to_string(), b"Not Found".to_vec(), Vec::new()),
                },
                None => (404, "text/plain; charset=utf-8".to_string(), format!("404 Not Found: {} {}", method_kini, path_kini).into_bytes(), Vec::new()),
            },
        }
    };

    // 4) Middleware `sesudah`, urut pemasangan, masing-masing menerima
    //    `(req, resp)` dan mengembalikan respons yang dipakai berikutnya.
    let mut resp = make_response_badan(status, body, &ctype, header_ke_value(extra_headers));
    for mw in &router.sesudah {
        match panggil(mw, vec![req_val.clone(), resp.clone()]) {
            Ok(Value::Nil) | Ok(Value::Unit) => {}
            Ok(v) => resp = v,
            Err(e) => {
                resp = make_response(
                    500, format!("middleware error: {}", e),
                    "text/plain; charset=utf-8", Vec::new());
                break;
            }
        }
    }
    let (status, ctype, body, extra_headers) = value_to_response(resp);
    // Kepala dari handler yang memuat CR/LF menyisipkan kepala lain (`Set-Cookie`) — dijawab 500, tidak dikirim.
    let salah = std::iter::once(("Content-Type", ctype.as_str()))
        .chain(extra_headers.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .find_map(|(k, v)| kepala_respons_salah(k, v));
    if let Some(alasan) = salah {
        crate::host::galat_baris(&format!("dok::serve: {} {}: {}", method_kini, path_kini, alasan));
        write_response(&mut stream, 500, "text/plain; charset=utf-8", &format!("handler error: {}", alasan), &[]);
        return;
    }
    write_response_bytes(&mut stream, status, &ctype, &body, &extra_headers);
}

/// Token nama kepala menurut RFC 9110 §5.6.2.
pub(crate) fn nama_kepala_sah(k: &str) -> bool {
    !k.is_empty() && k.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// Alasan sebuah kepala respons tidak boleh dikirim; `None` kalau boleh.
pub(crate) fn kepala_respons_salah(k: &str, v: &str) -> Option<String> {
    if !nama_kepala_sah(k) {
        return Some(format!("nama kepala respons {:?} tidak sah — cuma huruf, angka, dan `-_.!#$%&'*+^`|~`", k));
    }
    if v.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0) {
        return Some(format!("kepala respons `{}` memuat CR, LF atau NUL, yang akan menyisipkan kepala lain — \
            loloskan dulu nilai dari pemakai (mis. `dok::url_encode`)", k));
    }
    if k.eq_ignore_ascii_case("content-length") || k.eq_ignore_ascii_case("transfer-encoding") {
        return Some(format!("kepala respons `{}` diatur `dok::serve` sendiri dari badannya", k));
    }
    None
}

/// `dok::serve(router, port, host?)` — BLOCKING.
pub fn serve(interp: &mut Interpreter, args: &[Value]) -> Result<Value, Signal> {
    // Pembungkus tipis: yang membedakan interpreter dari VM cuma cara sebuah
    // handler dipanggil, jadi hanya itu yang diberikan ke `serve_dgn`.
    let mut buat = || -> PemanggilHandler {
        let mut thread_interp = interp.clone();
        Box::new(move |h: &Handler, argumen: Vec<Value>| match h {
            Handler::Interp(f) => thread_interp
                .call_fn(f.clone(), argumen)
                .map_err(|e| format!("{}", e)),
            // Tidak bisa terjadi lewat jalur ini: route yang dipasang dari interpreter
            // selalu `Handler::Interp`.
            Handler::Vm(_) => Err("handler ini milik backend VM, tapi server \
                                   dijalankan di interpreter"
                .to_string()),
        })
    };
    serve_dgn(&mut buat, args)
}

/// Inti `dok::serve`, dengan cara-memanggil-handler diserahkan ke pemanggil.
pub fn serve_dgn(
    buat: &mut dyn FnMut() -> PemanggilHandler,
    args: &[Value],
) -> Result<Value, Signal> {
    let router_id = handle_id(args.get(0).unwrap_or(&Value::Nil), "dok::serve")
        .map_err(|e| Signal::RuntimeError(RuntimeError::new(e)))?;
    let port = match args.get(1) {
        Some(Value::Int(n)) if *n >= 0 && *n <= 65535 => *n as u16,
        _ => return Err(Signal::RuntimeError(RuntimeError::new("dok::serve: port harus int 0..65535"))),
    };
    // Bawaan loopback (keputusan 2026-11-14, seperti Flask/Django/Rails): membuka ke jaringan ditulis `"0.0.0.0"`.
    let host = match args.get(2) { Some(Value::Str(s)) => s.clone(), _ => std::sync::Arc::new("127.0.0.1".to_string()) };

    let addr = format!("{}:{}", host, port);
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => return Ok(err_value(format!("dok::serve: gagal bind '{}': {}", addr, crate::host::galat_io(&e)))),
    };
    eprintln!("[dok] listening on http://{}", addr);

    let hidup = std::sync::Arc::new(AtomicUsize::new(0));
    for incoming in listener.incoming() {
        let mut stream = match incoming { Ok(s) => s, Err(_) => continue };
        if hidup.fetch_add(1, Ordering::AcqRel) >= MAKS_SAMBUNGAN {
            hidup.fetch_sub(1, Ordering::AcqRel);
            stream.set_write_timeout(Some(std::time::Duration::from_secs(1))).ok();
            write_response(&mut stream, PENUH.0, "text/plain", PENUH.1, &[("Retry-After".to_string(), "1".to_string())]);
            continue;
        }
        let jaga = Sambungan(hidup.clone());
        // Pemanggilnya dibuat SEBELUM thread lahir, karena pabriknya meminjam
        // keadaan pemanggil (`Interpreter` atau `Vm`) yang tidak ikut pindah.
        let mut pemanggil = buat();
        // Handler JRG berjalan di utas ini: stack-nya sebesar utas utama interpreter.
        let utas = std::thread::Builder::new()
            .stack_size(crate::interpreter::Interpreter::INTERPRETER_STACK_SIZE)
            .spawn(move || { crate::interpreter::catat_puncak_stack(); let _jaga = jaga; handle_connection(&mut pemanggil, router_id, stream) });
        if let Err(e) = utas {
            eprintln!("[dok] tidak bisa membuat utas untuk koneksi baru: {}", crate::host::galat_io(&e));
        }
    }

    Ok(ok_value(Value::Nil))
}

/// Satu koneksi hidup; hitungannya turun saat utasnya selesai, juga kalau utasnya gagal dibuat.
struct Sambungan(std::sync::Arc<AtomicUsize>);

impl Drop for Sambungan {
    fn drop(&mut self) { self.0.fetch_sub(1, Ordering::AcqRel); }
}

#[cfg(test)]
mod tes_permintaan {
    #[test]
    fn jalur_cuma_satu_ejaan() {
        use super::jalur_satu_ejaan as j;
        for (m, p) in [("/", "/"), ("/admin", "/admin"), ("/admin/", "/admin/"), ("/a/b", "/a/b"),
                       ("/%61dmin", "/admin"), ("/a%20b", "/a b"), ("/f.txt", "/f.txt"), ("/..x", "/..x")] {
            assert!(j(m, p), "harusnya sah: {}", m);
        }
        for (m, p) in [("//admin", "//admin"), ("/a//b", "/a//b"), ("/./admin", "/./admin"), ("/x/../admin", "/x/../admin"),
                       ("/%2fadmin", "//admin"), ("/a%2Fb", "/a/b"), ("/a%5cb", "/a\\b"), ("/a\\b", "/a\\b"),
                       ("/admin/.", "/admin/."), ("x", "x")] {
            assert!(!j(m, p), "harusnya ditolak: {}", m);
        }
    }

    use super::*;

    /// Tanpa tenggat total, klien ini ditunggu sampai ia sendiri berhenti (3 s) lalu dijawab 400.
    #[test]
    fn klien_yang_menetes_dihentikan() {
        use std::time::{Duration, Instant};
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let a = l.local_addr().unwrap();
        let t = std::thread::spawn(move || {
            let mut c = TcpStream::connect(a).unwrap();
            let _ = c.write_all(b"GET / HTTP/1.1\r\n");
            for _ in 0..60 {
                if c.write_all(b"X").is_err() { break; }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        let (s, _) = l.accept().unwrap();
        let mulai = Instant::now();
        let hasil = parse_request(&mut BufReader::new(Tenggat::baru(s, Duration::from_millis(300))));
        assert_eq!(hasil.err().map(|t| t.0), Some(408));
        assert!(mulai.elapsed() < Duration::from_secs(2), "{:?}", mulai.elapsed());
        t.join().ok();
    }

    fn urai(b: &[u8]) -> Result<ParsedRequest, Tolak> { parse_request(&mut std::io::Cursor::new(b)) }
    fn kode(b: &[u8]) -> i128 { urai(b).err().map_or(200, |t| t.0) }

    #[test]
    fn baris_permintaan_dan_host_yang_ambigu_ditolak() {
        for b in [&b"GET  / HTTP/1.1\r\n\r\n"[..], b"GET\t/ HTTP/1.1\r\n\r\n", b"GET / HTTP/1.1 x\r\n\r\n",
                  b"GET /\r\n\r\n", b"GET http://evil/ HTTP/1.1\r\n\r\n", b"GET * HTTP/1.1\r\n\r\n",
                  b"G(T / HTTP/1.1\r\n\r\n", b"GET / HTTP/1.1\n\n", b"GET / HTTP/1.1\r\nX: a\n\r\n",
                  b"GET / HTTP/1.1\r\nX: a\rb\r\n\r\n", b"GET / HTTP/1.1\r\nX: a\x00b\r\n\r\n",
                  b"GET / HTTP/1.1\r\nX: a\x1bb\r\n\r\n", b"GET / FTP/1.1\r\n\r\n",
                  b"GET / HTTP/1.1\r\nHost: a\r\nHost: b\r\n\r\n", b"GET / HTTP/1.1\r\nHost: a@b\r\n\r\n",
                  b"GET / HTTP/1.1\r\nHost: a,b\r\n\r\n", b"GET / HTTP/1.1\r\nHost: 127.1\r\n\r\n",
                  b"GET / HTTP/1.1\r\nHost: a:\r\n\r\n", b"GET / HTTP/1.1\r\nHost: \r\n\r\n",
                  b"GET / HTTP/1.1\r\nHost: [::1%25lo]\r\n\r\n"] {
            assert_eq!(kode(b), 400, "{:?}", String::from_utf8_lossy(b));
        }
        assert_eq!(kode(b"GET / HTTP/2.0\r\n\r\n"), 505);
        for b in [&b"GET / HTTP/1.0\r\n\r\n"[..], b"GET /a?b HTTP/1.1\r\nHost: Contoh.ID.:8080\r\n\r\n",
                  b"GET / HTTP/1.1\r\nHost: [::1]:80\r\nX:\tnilai\t\r\n\r\n"] {
            assert_eq!(kode(b), 200, "{:?}", String::from_utf8_lossy(b));
        }
    }

    #[test]
    fn kepala_permintaan_yang_dibaca_lain_oleh_proxy_ditolak() {
        // Badan berpotongan dulu dibaca kosong; tiga bentuk kepala cacat dulu diterima atau dibuang diam-diam.
        assert_eq!(kode(b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhalo!\r\n0\r\n\r\n"), 501);
        assert_eq!(kode(b"POST / HTTP/1.1\r\nContent-Length : 5\r\n\r\nhalo!"), 400);
        assert_eq!(kode(b"POST / HTTP/1.1\r\nSampah\r\nContent-Length: 5\r\n\r\nhalo!"), 400);
        assert_eq!(kode(b"POST / HTTP/1.1\r\nX-A: 1\r\n  lanjut\r\n\r\n"), 400);
        assert_eq!(kode(b"POST / HTTP/1.1\r\nX-A:1\r\nContent-Length: 5\r\n\r\nhalo!"), 200);
    }

    #[test]
    fn kepala_respons_tidak_bisa_menyisipkan_kepala_lain() {
        assert!(super::kepala_respons_salah("Location", "/x\r\nSet-Cookie: a=1").is_some());
        assert!(super::kepala_respons_salah("X-A", "a\nb").is_some());
        assert!(super::kepala_respons_salah("X A", "b").is_some());
        assert!(super::kepala_respons_salah("Content-Length", "5").is_some());
        assert!(super::kepala_respons_salah("Set-Cookie", "a=1; Path=/").is_none());
    }

    #[test]
    fn panjang_badan_dari_klien_tidak_dipercaya() {
        let p = urai(b"POST /a?x=1 HTTP/1.1\r\nContent-Length: 3\r\nContent-Length: 3\r\n\r\nabcSISA").unwrap();
        assert_eq!((p.method.as_str(), p.path.as_str(), p.query_string.as_str(), p.body.as_slice()), ("POST", "/a", "x=1", &b"abc"[..]));
        for cl in ["9223372036854775813", "1099511627776", "67108865"] {
            assert_eq!(kode(format!("POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\nabc", cl).as_bytes()), 413, "{}", cl);
        }
        for cl in ["abc", "-1", "+3", "", "3 3", "0x3"] {
            assert_eq!(kode(format!("POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\nabc", cl).as_bytes()), 400, "{:?}", cl);
        }
        assert_eq!(kode(b"POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 3\r\n\r\nabc"), 400);
        assert_eq!(kode(b"POST / HTTP/1.1\r\nContent-Length: 10\r\n\r\nabc"), 400, "badan kurang dari yang diaku");
        assert_eq!(kode(b""), 400);
        assert_eq!(kode(b"GET /\xff HTTP/1.1\r\n\r\n"), 400);
    }

    #[test]
    fn baris_dan_jumlah_header_berbatas() {
        let baris = |n: usize| format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(n - 16));
        assert_eq!(kode(baris(MAKS_BARIS).as_bytes()), 200);
        assert_eq!(kode(baris(MAKS_BARIS + 1).as_bytes()), 414);
        assert_eq!(kode(format!("GET /{}", "a".repeat(1 << 20)).as_bytes()), 414, "tanpa akhir baris");
        assert_eq!(kode(format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(MAKS_BARIS)).as_bytes()), 431);
        let banyak = |n: usize| format!("GET / HTTP/1.1\r\n{}\r\n", "X: 1\r\n".repeat(n));
        assert_eq!(urai(banyak(MAKS_HEADER).as_bytes()).unwrap().headers.len(), MAKS_HEADER);
        assert_eq!(kode(banyak(MAKS_HEADER + 1).as_bytes()), 431);
    }

    #[test]
    fn badan_serentak_berbatas_dan_dikembalikan() {
        static HITUNG: AtomicUsize = AtomicUsize::new(0);
        let urai_b = |b: &[u8]| parse_request_dgn(&mut std::io::Cursor::new(b.to_vec()), &HITUNG, 10);
        let enam = b"POST / HTTP/1.1\r\nContent-Length: 6\r\n\r\nabcdef";
        let pertama = urai_b(enam).unwrap();
        assert_eq!(HITUNG.load(Ordering::Acquire), 6);
        assert_eq!(urai_b(enam).err(), Some(PENUH), "anggaran sudah terpakai");
        assert_eq!(HITUNG.load(Ordering::Acquire), 6, "yang ditolak mengembalikan bagiannya");
        drop(pertama);
        assert_eq!(HITUNG.load(Ordering::Acquire), 0);
        assert_eq!(urai_b(enam).unwrap().body, b"abcdef");
        assert_eq!(HITUNG.load(Ordering::Acquire), 0);
        let biner = |n: usize| [format!("POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n", n).into_bytes(), vec![0xff; n]].concat();
        let muat = 1000 / (1 + std::mem::size_of::<Value>());
        let p = parse_request_dgn(&mut std::io::Cursor::new(biner(muat)), &HITUNG, 1000).unwrap();
        assert_eq!(HITUNG.load(Ordering::Acquire), muat * (1 + std::mem::size_of::<Value>()), "biner dihitung seharga array-nya");
        assert_eq!(parse_request_dgn(&mut std::io::Cursor::new(biner(1)), &HITUNG, 1000).err(), Some(PENUH));
        drop(p);
        assert_eq!(parse_request_dgn(&mut std::io::Cursor::new(biner(muat + 1)), &HITUNG, 1000).err(), Some((413, "Payload Too Large")));
        assert_eq!(HITUNG.load(Ordering::Acquire), 0);
    }

    #[test]
    fn berkas_statis_menolak_tautan_simbolik_ke_luar_dan_lintas_jalur() {
        let akar = std::env::temp_dir().join(format!("jrg-uji-statis-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&akar);
        let (publik, luar) = (akar.join("publik"), akar.join("luar"));
        std::fs::create_dir_all(&publik).unwrap();
        std::fs::create_dir_all(&luar).unwrap();
        std::fs::write(publik.join("biasa.txt"), b"aman").unwrap();
        std::fs::write(luar.join("rahasia.txt"), b"bocor").unwrap();
        let mounts = vec![("/aset".to_string(), publik.clone())];
        assert_eq!(try_serve_static(&mounts, "/aset/biasa.txt").map(|x| x.0), Some(200));
        assert_eq!(try_serve_static(&mounts, "/aset/../luar/rahasia.txt").map(|x| x.0), Some(403));
        assert_eq!(try_serve_static(&mounts, "/aset/a\\b").map(|x| x.0), Some(403));
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(luar.join("rahasia.txt"), publik.join("tautan.txt")).unwrap();
            symlink(&luar, publik.join("folder_luar")).unwrap();
            assert_eq!(try_serve_static(&mounts, "/aset/tautan.txt").map(|x| x.0), Some(403));
            assert_eq!(try_serve_static(&mounts, "/aset/folder_luar/rahasia.txt").map(|x| x.0), Some(403));
        }
        let _ = std::fs::remove_dir_all(&akar);
    }
}
