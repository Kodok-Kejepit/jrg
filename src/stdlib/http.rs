// src/stdlib/http.rs
// HTTP client via raw TCP (HTTP/1.1, no TLS, no deps). Jawabannya `Result` seperti `dok::http_get`:
// gagal sambung, URL tak sah, dan jawaban rusak adalah `Err`; cuma argumen bertipe salah yang galat keras.

use crate::vm::vm::Vm;
use crate::vm::value::VmValue;
use std::sync::Arc;
use std::collections::HashMap;

pub fn register(vm: &mut Vm) {
    use VmValue::*;

    vm.def("http::get", |a| match a.get(0) {
        Some(Str(url)) => Ok(hasil("http::get", http_request("GET", &url, "", ""))),
        _ => Err("http::get(url: string)".to_string()),
    });

    vm.def("http::post", |a| match a.get(0) {
        Some(Str(url)) => {
            let body  = match a.get(1) { Some(Str(b)) => b.as_str().to_string(), Some(v) => format!("{}", v), None => String::new() };
            let ctype = match a.get(2) { Some(Str(c)) => c.as_str().to_string(), _ => "application/json".to_string() };
            Ok(hasil("http::post", http_request("POST", &url, &body, &ctype)))
        }
        _ => Err("http::post(url, body?, content_type?)".to_string()),
    });

    vm.def("http::put", |a| match a.get(0) {
        Some(Str(url)) => {
            let body  = match a.get(1) { Some(Str(b)) => b.as_str().to_string(), Some(v) => format!("{}", v), None => String::new() };
            let ctype = match a.get(2) { Some(Str(c)) => c.as_str().to_string(), _ => "application/json".to_string() };
            Ok(hasil("http::put", http_request("PUT", &url, &body, &ctype)))
        }
        _ => Err("http::put(url, body?, content_type?)".to_string()),
    });

    vm.def("http::delete", |a| match a.get(0) {
        Some(Str(url)) => Ok(hasil("http::delete", http_request("DELETE", &url, "", ""))),
        _ => Err("http::delete(url)".to_string()),
    });

    vm.def("http::request", |a| {
        let method = match a.get(0) { Some(Str(m)) => m.to_uppercase(), _ => return Err("http::request butuh method".to_string()) };
        let url    = match a.get(1) { Some(Str(u)) => u.as_str().to_string(), _ => return Err("http::request butuh url".to_string()) };
        let body   = match a.get(2) { Some(Str(b)) => b.as_str().to_string(), Some(v) => format!("{}", v), None => String::new() };
        let ctype  = match a.get(3) { Some(Str(c)) => c.as_str().to_string(), _ => "application/json".to_string() };
        Ok(hasil("http::request", http_request(&method, &url, &body, &ctype)))
    });

    vm.def("http::unduh", |a| match (a.get(0), a.get(1)) {
        (Some(Str(url)), Some(Str(berkas))) => Ok(match crate::dok::klien::unduh(url, berkas, "jrg/0.1") {
            Ok(h) => {
                let mut m = HashMap::new();
                m.insert("status".to_string(), VmValue::Int(h.status as i128));
                m.insert("ukuran".to_string(), VmValue::Int(h.ukuran as i128));
                m.insert("url".to_string(), VmValue::str(h.url));
                VmValue::varian("Result", "Ok", Some(VmValue::map(m)))
            }
            Err(e) => VmValue::varian("Result", "Err", Some(VmValue::str(format!("http::unduh: {}", e)))),
        }),
        _ => Err("http::unduh(url: string, berkas: string)".to_string()),
    });

    // Helpers
    vm.def("http::url_encode", |a| match a.get(0) {
        Some(Str(s)) => Ok(Str(Arc::new(url_encode(&s)))),
        _ => Err("http::url_encode(&s)".to_string()),
    });

    vm.def("http::url_decode", |a| match a.get(0) {
        Some(Str(s)) => crate::dok::http::url_decode_teks(&s).map(|t| Str(Arc::new(t))).ok_or_else(|| format!(
            "http::url_decode: byte hasil `%XX` di `{}` bukan teks UTF-8 yang sah", s)),
        _ => Err("http::url_decode(&s)".to_string()),
    });
}

/// `Ok(jawaban)` atau `Err("http::get: sebabnya")`.
fn hasil(nama: &str, r: Result<VmValue, String>) -> VmValue {
    match r {
        Ok(v) => VmValue::varian("Result", "Ok", Some(v)),
        Err(e) => VmValue::varian("Result", "Err", Some(VmValue::str(format!("{}: {}", nama, e)))),
    }
}

// ─── HTTP/1.1 TCP Client ──────────────────────────────────────────────────

fn http_request(method: &str, url: &str, body: &str, ctype: &str) -> Result<VmValue, String> {
    let kepala: Vec<(String, String)> = if body.is_empty() { Vec::new() }
        else { vec![("Content-Type".into(), if ctype.is_empty() { "application/json" } else { ctype }.to_string())] };
    let response = crate::dok::klien::minta(method, url, body.as_bytes(), &kepala, "jrg/0.1")?;
    parse_http_response(&response)
}

fn parse_http_response(resp: &[u8]) -> Result<VmValue, String> {
    let mut headers_map = HashMap::new();
    let (headers_str, body) = crate::dok::http::pisah_jawaban(resp)?;

    // Parse status line
    let status_line = headers_str.lines().next().unwrap_or("");
    let status_code: i64 = status_line.split_whitespace()
        .nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);

    // Parse headers
    for line in headers_str.lines().skip(1) {
        if let Some(colon) = line.find(':') {
            let key   = line[..colon].trim().to_lowercase();
            let value = line[colon+1..].trim().to_string();
            headers_map.insert(key, VmValue::str(value));
        }
    }

    let mut map = HashMap::new();
    map.insert("status".to_string(),  VmValue::Int(status_code as i128));
    // Badan yang bukan teks: `body` nil dan byte-nya di `body_bytes` (aturan `dok::http`).
    let (teks, byte) = match String::from_utf8(body) {
        Ok(t) => (VmValue::str(t), VmValue::Nil),
        Err(e) => (VmValue::Nil, VmValue::array(e.into_bytes().into_iter().map(|b| VmValue::Int(b as i128)).collect())),
    };
    map.insert("body".to_string(),    teks);
    map.insert("body_bytes".to_string(), byte);
    map.insert("ok".to_string(),      VmValue::Bool(status_code >= 200 && status_code < 300));
    map.insert("headers".to_string(), VmValue::map(headers_map));

    Ok(VmValue::map(map))
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _    => { out.push('%'); out.push_str(&format!("{:02X}", b)); }
        }
    }
    out
}

