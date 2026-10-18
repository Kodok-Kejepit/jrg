// src/lsp/mod.rs
// Language Server Protocol (LSP) — JSON-RPC 2.0 over stdio

mod protocol;
mod handlers;
mod completions;
pub mod simbol;

use std::collections::HashMap;
use std::io::{self, BufRead, Read, Write};

pub use protocol::*;
pub use handlers::LspState;

// ─── Main LSP server loop ─────────────────────────────────────────────────

pub fn run_server() {
    eprintln!(
        "[jrg-lsp] JawaRustGo++ Language Server v{} starting...",
        env!("CARGO_PKG_VERSION")
    );
    eprintln!("[jrg-lsp] Transport: stdio JSON-RPC 2.0");

    let stdin  = io::stdin();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let mut state = LspState::new();

    let mut reader = io::BufReader::new(stdin.lock());

    loop {
        // ── Baca header ────────────────────────────────────────────────
        let mut content_length: Option<usize> = None;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => return,             // stdin ditutup
                Ok(_) => {}
                Err(_) => return,
            }
            let t = line.trim_end_matches(['\r', '\n']);
            if t.is_empty() { break; }       // baris kosong = akhir header
            if let Some(v) = t.strip_prefix("Content-Length:") {
                content_length = v.trim().parse::<usize>().ok();
            }
            // Header lain (`Content-Type`) diabaikan, sesuai spesifikasi.
        }
        let Some(content_length) = content_length else { continue };
        if content_length == 0 { continue; }

        // ── Baca badan: TEPAT sebanyak byte yang diumumkan ──────────────
        const BATAS: usize = 64 * 1024 * 1024;
        if content_length > BATAS {
            eprintln!(
                "[jrg-lsp] Content-Length {} di luar batas wajar — header rusak, sesi dihentikan",
                content_length
            );
            return;
        }
        let mut buf = vec![0u8; content_length];
        if reader.read_exact(&mut buf).is_err() {
            eprintln!("[jrg-lsp] aliran terpotong di tengah badan pesan");
            return;
        }
        let body = match String::from_utf8(buf) {
            Ok(s) => s,
            Err(_) => {
                eprintln!("[jrg-lsp] badan pesan bukan UTF-8 yang sah");
                continue;
            }
        };

        // ── Parse JSON-RPC ────────────────────────────────────────────
        let msg = match parse_message(&body) {
            Some(m) => m,
            None    => {
                eprintln!("[jrg-lsp] JSON parse error: {}", &body[..body.len().min(80)]);
                continue;
            }
        };

        // ── Dispatch ──────────────────────────────────────────────────
        let response = state.handle(&msg);

        // ── Send response ─────────────────────────────────────────────
        for resp in response {
            let json = resp.to_json();
            let _ = write!(out, "Content-Length: {}\r\n\r\n{}", json.len(), json);
            let _ = out.flush();
        }

        // Exit jika shutdown + exit diterima
        if state.should_exit { break; }
    }
    eprintln!("[jrg-lsp] Server exiting.");
}

// ─── Minimal JSON parser (no serde dep) ──────────────────────────────────

#[derive(Debug, Clone)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Array(Vec<JsonValue>),
    Object(HashMap<String, JsonValue>),
}

impl JsonValue {
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        if let JsonValue::Object(m) = self { m.get(key) } else { None }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let JsonValue::Str(s) = self { Some(s.as_str()) } else { None }
    }
    pub fn as_i64(&self) -> Option<i64> {
        if let JsonValue::Number(n) = self { Some(*n as i64) } else { None }
    }
    pub fn as_arr(&self) -> Option<&Vec<JsonValue>> {
        if let JsonValue::Array(a) = self { Some(a) } else { None }
    }
    pub fn as_obj(&self) -> Option<&HashMap<String, JsonValue>> {
        if let JsonValue::Object(o) = self { Some(o) } else { None }
    }
    pub fn is_null(&self) -> bool { matches!(self, JsonValue::Null) }
}

pub fn parse_message(s: &str) -> Option<JsonValue> {
    let s = s.trim();
    parse_value(&mut s.chars().peekable())
}

type Chars<'a> = std::iter::Peekable<std::str::Chars<'a>>;

fn parse_value(it: &mut Chars) -> Option<JsonValue> {
    skip_ws(it);
    match it.peek()? {
        '{' => parse_object(it),
        '[' => parse_array(it),
        '"' => parse_string(it).map(JsonValue::Str),
        't' => { consume_lit(it, "true");  Some(JsonValue::Bool(true))  }
        'f' => { consume_lit(it, "false"); Some(JsonValue::Bool(false)) }
        'n' => { consume_lit(it, "null");  Some(JsonValue::Null)        }
        '-' | '0'..='9' => parse_number(it).map(JsonValue::Number),
        _ => None,
    }
}

fn skip_ws(it: &mut Chars) {
    while it.peek().map(|c| c.is_whitespace()).unwrap_or(false) { it.next(); }
}

fn consume_lit(it: &mut Chars, s: &str) {
    for _ in s.chars() { it.next(); }
}

fn parse_object(it: &mut Chars) -> Option<JsonValue> {
    it.next(); // '{'
    let mut map = HashMap::new();
    skip_ws(it);
    if it.peek() == Some(&'}') { it.next(); return Some(JsonValue::Object(map)); }
    loop {
        skip_ws(it);
        let key = parse_string(it)?;
        skip_ws(it);
        if it.next() != Some(':') { return None; } // ':'
        skip_ws(it);
        let val = parse_value(it)?;
        map.insert(key, val);
        skip_ws(it);
        match it.peek() {
            Some(&',') => { it.next(); }
            Some(&'}') => { it.next(); break; }
            _ => return None,
        }
    }
    Some(JsonValue::Object(map))
}

fn parse_array(it: &mut Chars) -> Option<JsonValue> {
    it.next(); // '['
    let mut arr = Vec::new();
    skip_ws(it);
    if it.peek() == Some(&']') { it.next(); return Some(JsonValue::Array(arr)); }
    loop {
        skip_ws(it);
        arr.push(parse_value(it)?);
        skip_ws(it);
        match it.peek() {
            Some(&',') => { it.next(); }
            Some(&']') => { it.next(); break; }
            _ => return None,
        }
    }
    Some(JsonValue::Array(arr))
}

fn parse_string(it: &mut Chars) -> Option<String> {
    if it.next() != Some('"') { return None; }
    let mut s = String::new();
    loop {
        match it.next()? {
            '"'  => break,
            '\\' => match it.next()? {
                '"'  => s.push('"'),
                '\\' => s.push('\\'),
                '/'  => s.push('/'),
                'n'  => s.push('\n'),
                'r'  => s.push('\r'),
                't'  => s.push('\t'),
                'u'  => {
                    let hex: String = (0..4).filter_map(|_| it.next()).collect();
                    if let Ok(n) = u32::from_str_radix(&hex, 16) {
                        if let Some(c) = char::from_u32(n) { s.push(c); }
                    }
                }
                c => s.push(c),
            },
            c => s.push(c),
        }
    }
    Some(s)
}

fn parse_number(it: &mut Chars) -> Option<f64> {
    let mut n = String::new();
    while it.peek().map(|c| matches!(c, '-'|'+'|'.'|'e'|'E'|'0'..='9')).unwrap_or(false) {
        n.push(it.next().unwrap());
    }
    n.parse::<f64>().ok()
}

// ─── JSON serializer ─────────────────────────────────────────────────────

pub fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"'  => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c    => o.push(c),
        }
    }
    o.push('"');
    o
}

pub fn json_obj(pairs: &[(&str, String)]) -> String {
    let inner: Vec<String> = pairs.iter()
        .map(|(k, v)| format!("{}:{}", json_str(k), v))
        .collect();
    format!("{{{}}}", inner.join(","))
}

pub fn json_arr(items: &[String]) -> String {
    format!("[{}]", items.join(","))
}
