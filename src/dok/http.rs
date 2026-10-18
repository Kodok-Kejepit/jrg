// src/dok/http.rs — dok::http — HTTP/1.1 client + server (TCP mentah, tanpa TLS, tanpa deps)

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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
        Ok(t) => (Value::Str(t), Value::Nil),
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
        Some(Value::Str(s)) => Ok(Value::Str(url_encode_str(s))),
        other => Err(format!("dok::url_encode(s): butuh string, dapat `{}`",
            other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada argumen".to_string()))),
    }
}

pub fn url_decode(args: &[Value]) -> Result<Value, String> {
    match args.get(0) {
        Some(Value::Str(s)) => url_decode_teks(s).map(Value::Str).ok_or_else(|| format!(
            "dok::url_decode: byte hasil `%XX` di `{}` bukan teks UTF-8 yang sah", s)),
        other => Err(format!("dok::url_decode(s): butuh string, dapat `{}`",
            other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada argumen".to_string()))),
    }
}

/// Pasangan query; `None` kalau satu `%XX`-nya bukan teks (permintaannya dijawab 400).
fn parse_query(qs: &str) -> Option<Vec<Value>> {
    if qs.is_empty() { return Some(Vec::new()); }
    qs.split('&').filter(|p| !p.is_empty()).map(|pair| {
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (url_decode_teks(k)?, url_decode_teks(v)?),
            None => (url_decode_teks(pair)?, String::new()),
        };
        Some(Value::Tuple(vec![Value::Str(k), Value::Str(v)].into()))
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
                if k == name { return Ok(some_value(v.clone())); }
            }
        }
    }
    Ok(none_value())
}

// ─── CLIENT ─────────────────────────────────────────────────────────────

/// `host` atau `host:port` dari sebuah URL. Port yang tidak sah DITOLAK — dulu diam-diam
/// jadi 80, jadi permintaannya sampai ke layanan lain.
pub fn urai_host_port(bagian: &str) -> Result<(String, u16), String> {
    let Some(titik) = bagian.rfind(':') else { return Ok((bagian.to_string(), 80)) };
    let teks = &bagian[titik + 1..];
    match teks.parse::<u16>() {
        Ok(p) if p > 0 => Ok((bagian[..titik].to_string(), p)),
        _ => Err(format!("port `{}` tidak sah — yang sah 1 sampai 65535", teks)),
    }
}

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
                Value::Str(line[..colon].trim().to_lowercase()),
                Value::Str(line[colon + 1..].trim().to_string()),
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
    match v { Some(Value::Str(s)) => s.clone(), Some(other) => format!("{}", other), None => String::new() }
}

/// Argumen opsional yang harus teks (content type): yang salah tipe dulu diam-diam
/// jadi bawaannya.
fn teks_opsional(v: Option<&Value>, fungsi: &str, ke: usize, bawaan: &str) -> Result<String, String> {
    match v {
        None => Ok(bawaan.to_string()),
        Some(Value::Str(c)) => Ok(c.clone()),
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
                (Value::Str(k), Value::Str(val)) => Ok((k.clone(), val.clone())),
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
    fields.insert("content_type".to_string(), Value::Str(content_type.to_string()));
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
    Ok(make_response(status, String::new(), "text/plain", vec![Value::Tuple(vec![Value::Str("Location".to_string()), Value::Str(url)].into())]))
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
}

/// Batas permintaan masuk. Tanpa ini satu klien menentukan berapa memori yang dipesan server.
const MAKS_BARIS: usize = 8 * 1024;
const MAKS_HEADER: usize = 100;
pub const MAKS_BADAN: usize = 64 * 1024 * 1024;
const TUNGGU_PERMINTAAN: std::time::Duration = std::time::Duration::from_secs(30);

/// Permintaan yang ditolak sebelum sampai ke handler: status dan kalimatnya.
type Tolak = (i128, &'static str);

fn galat_baca(e: std::io::Error) -> Tolak {
    match e.kind() {
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => (408, "Request Timeout"),
        _ => (400, "Bad Request"),
    }
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
    const SALAH: Tolak = (400, "Bad Request");
    let line = baca_baris(reader, (414, "URI Too Long"))?.ok_or(SALAH)?;
    let line = line.trim_end();
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    if method.is_empty() { return Err(SALAH); }

    let (raw_path, query_string) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target.clone(), String::new()),
    };
    let path = url_decode_teks(&raw_path).ok_or(SALAH)?;
    let query = parse_query(&query_string).ok_or(SALAH)?;

    let besar: Tolak = (431, "Request Header Fields Too Large");
    let mut headers = Vec::new();
    let mut content_length: Option<usize> = None;
    loop {
        let hline = baca_baris(reader, besar)?.ok_or(SALAH)?;
        let hline = hline.trim_end();
        if hline.is_empty() { break; }
        if headers.len() >= MAKS_HEADER { return Err(besar); }
        if let Some(colon) = hline.find(':') {
            let k = hline[..colon].trim().to_lowercase();
            let v = hline[colon + 1..].trim().to_string();
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
    let mut buf = Vec::new();
    let n = content_length.unwrap_or(0);
    if n > 0 {
        (&mut *reader).take(n as u64).read_to_end(&mut buf).map_err(galat_baca)?;
        if buf.len() != n { return Err(SALAH); }
    }
    Ok(ParsedRequest { method, path, query_string, headers, query, body: buf })
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
        503 => "Service Unavailable", 504 => "Gateway Timeout",
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
        // Cegah path traversal: tolak segmen ".." mentah-mentah sebelum
        // menyentuh filesystem sama sekali.
        if rel.split('/').any(|seg| seg == "..") { return Some((403, "text/plain".to_string(), b"Forbidden".to_vec())); }
        let full = dir.join(if rel.is_empty() { "index.html" } else { rel });
        if full.is_file() {
            if let Ok(bytes) = std::fs::read(&full) {
                let ctype = guess_content_type(&full);
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
        .map(|(k, v)| Value::Tuple(vec![Value::Str(k), Value::Str(v)].into()))
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
            Some(Value::Str(s)) => Some(s.clone()),
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
    for (k, val) in params { pf.insert(k, Value::Str(val)); }
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
                (_, Some(Value::Str(s))) => s.clone().into_bytes(),
                (_, Some(Value::Nil)) | (_, None) => Vec::new(),
                (_, Some(o)) => format!("{}", o).into_bytes(),
            };
            let ctype = match fields.get("content_type") { Some(Value::Str(s)) => s.clone(), _ => "text/plain; charset=utf-8".to_string() };
            let headers = match fields.get("headers") {
                Some(Value::Array(a)) => a.iter().filter_map(|v| match v {
                    Value::Tuple(t) if t.len() == 2 => match (&t[0], &t[1]) {
                        (Value::Str(k), Value::Str(val)) => Some((k.clone(), val.clone())),
                        _ => None,
                    },
                    _ => None,
                }).collect(),
                _ => Vec::new(),
            };
            (status, ctype, body, headers)
        }
        Value::Enum { variant, payload, .. } if variant == "Ok" => {
            value_to_response(payload.map(|b| *b).unwrap_or(Value::Nil))
        }
        Value::Enum { variant, payload, .. } if variant == "Err" => {
            let msg = payload.map(|b| format!("{}", *b)).unwrap_or_else(|| "error".to_string());
            (500, "text/plain; charset=utf-8".to_string(), msg.into_bytes(), Vec::new())
        }
        Value::Str(s) => (200, "text/plain; charset=utf-8".to_string(), s.into_bytes(), Vec::new()),
        other => (200, "text/plain; charset=utf-8".to_string(), format!("{}", other).into_bytes(), Vec::new()),
    }
}

fn build_request_value(pr: &ParsedRequest, params: Vec<(String, String)>, remote_addr: &str) -> Value {
    let mut fields = HashMap::new();
    fields.insert("method".to_string(), Value::Str(pr.method.clone()));
    fields.insert("path".to_string(), Value::Str(pr.path.clone()));
    fields.insert("query_string".to_string(), Value::Str(pr.query_string.clone()));
    fields.insert("query".to_string(), Value::Array(pr.query.clone().into()));
    let (teks, byte) = badan_nilai(pr.body.clone());
    fields.insert("body".to_string(), teks);
    fields.insert("body_bytes".to_string(), byte);
    fields.insert("remote_addr".to_string(), Value::Str(remote_addr.to_string()));
    fields.insert("headers".to_string(), Value::Array(
        pr.headers.iter().map(|(k, v)| Value::Tuple(vec![Value::Str(k.clone()), Value::Str(v.clone())].into())).collect()
    ));
    let mut param_fields = HashMap::new();
    for (k, v) in params { param_fields.insert(k, Value::Str(v)); }
    fields.insert("params".to_string(), Value::Struct { name: super::json::OBJ_NAME.to_string(), fields: param_fields.into() });
    Value::Struct { name: "DokRequest".to_string(), fields: fields.into() }
}

fn handle_connection(panggil: &mut PemanggilHandler, router_id: u64, mut stream: TcpStream) {
    let remote_addr = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();
    let mut reader = BufReader::new(match stream.try_clone() { Ok(s) => s, Err(_) => return });

    // Klien yang diam tidak boleh menahan thread selamanya; batasnya dilepas sesudah permintaan terbaca.
    stream.set_read_timeout(Some(TUNGGU_PERMINTAAN)).ok();
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
    let mut req_val = build_request_value(&pr, Vec::new(), &remote_addr);
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
    write_response_bytes(&mut stream, status, &ctype, &body, &extra_headers);
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
    let host = match args.get(2) { Some(Value::Str(s)) => s.clone(), _ => "0.0.0.0".to_string() };

    let addr = format!("{}:{}", host, port);
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => return Ok(err_value(format!("dok::serve: gagal bind '{}': {}", addr, crate::host::galat_io(&e)))),
    };
    eprintln!("[dok] listening on http://{}", addr);

    for incoming in listener.incoming() {
        let stream = match incoming { Ok(s) => s, Err(_) => continue };
        // Pemanggilnya dibuat SEBELUM thread lahir, karena pabriknya meminjam
        // keadaan pemanggil (`Interpreter` atau `Vm`) yang tidak ikut pindah.
        let mut pemanggil = buat();
        std::thread::spawn(move || {
            handle_connection(&mut pemanggil, router_id, stream);
        });
    }

    Ok(ok_value(Value::Nil))
}

#[cfg(test)]
mod tes_permintaan {
    use super::*;

    fn urai(b: &[u8]) -> Result<ParsedRequest, Tolak> { parse_request(&mut std::io::Cursor::new(b)) }
    fn kode(b: &[u8]) -> i128 { urai(b).err().map_or(200, |t| t.0) }

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
}
