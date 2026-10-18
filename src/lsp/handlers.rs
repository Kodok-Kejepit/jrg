// src/lsp/handlers.rs — src/lsp/handlers.rs — LSP request handlers

use std::collections::HashMap;
use super::protocol::*;
use super::{JsonValue, LspMessage, LspResponse, json_str, json_obj, json_arr};
use super::completions::{all_completions, hover_doc};
use super::simbol;

// ─── Document store ───────────────────────────────────────────────────────

struct Document {
    uri:     String,
    content: String,
    version: i64,
}

impl Document {
    fn lines(&self) -> Vec<&str> { self.content.lines().collect() }
}

// ─── LspState ─────────────────────────────────────────────────────────────

/// Batas item completion dalam satu respons.
const BATAS_COMPLETION: usize = 200;

pub struct LspState {
    pub should_exit: bool,
    initialized:     bool,
    documents:       HashMap<String, Document>,
    completions:     Vec<CompletionItem>,
}

impl LspState {
    pub fn new() -> Self {
        LspState {
            should_exit:  false,
            initialized:  false,
            documents:    HashMap::new(),
            completions:  all_completions(),
        }
    }

    pub fn handle(&mut self, msg: &JsonValue) -> Vec<LspResponse> {
        let lsp = match LspMessage::from_json(msg) {
            Some(m) => m,
            None    => return vec![LspResponse::err(None, -32700, "Parse error")],
        };

        match lsp.method.as_str() {
            "initialize"               => vec![self.handle_initialize(&lsp)],
            "initialized"              => { self.initialized = true; vec![] }
            "shutdown"                 => vec![LspResponse::null_result(lsp.id.as_ref())],
            "exit"                     => { self.should_exit = true; vec![] }
            "textDocument/didOpen"     => self.handle_did_open(&lsp),
            "textDocument/didChange"   => self.handle_did_change(&lsp),
            "textDocument/didClose"    => self.handle_did_close(&lsp),
            "textDocument/didSave"     => self.handle_did_save(&lsp),
            "textDocument/completion"  => vec![self.handle_completion(&lsp)],
            "textDocument/hover"       => vec![self.handle_hover(&lsp)],
            "textDocument/definition"  => vec![self.handle_definition(&lsp)],
            "textDocument/documentSymbol" => vec![self.handle_symbols(&lsp)],
            "$/cancelRequest"
            | "workspace/didChangeConfiguration"
            | "workspace/didChangeWatchedFiles" => vec![],
            method => {
                eprintln!("[jrg-lsp] unhandled: {}", method);
                if lsp.id.is_some() {
                    vec![LspResponse::err(lsp.id.as_ref(), -32601, "Method not found")]
                } else { vec![] }
            }
        }
    }

    // ── initialize ────────────────────────────────────────────────────────

    fn handle_initialize(&self, msg: &LspMessage) -> LspResponse {
        let caps = json_obj(&[
            ("textDocumentSync", json_obj(&[
                ("openClose", "true".to_string()),
                ("change",    "1".to_string()),   // 1=Full sync
                ("save",      json_obj(&[("includeText", "true".to_string())])),
            ])),
            ("completionProvider", json_obj(&[
                ("triggerCharacters", json_arr(&[
                    json_str(":"), json_str("."), json_str("("),
                ])),
                ("resolveProvider", "false".to_string()),
            ])),
            ("hoverProvider",          "true".to_string()),
            ("definitionProvider",     "true".to_string()),
            ("documentSymbolProvider", "true".to_string()),
            ("diagnosticProvider", json_obj(&[
                ("interFileDependencies", "false".to_string()),
                ("workspaceDiagnostics",  "false".to_string()),
            ])),
        ]);

        let result = json_obj(&[
            ("capabilities",  caps),
            ("serverInfo",    json_obj(&[
                ("name",    json_str("jrg-lsp")),
                ("version", json_str(env!("CARGO_PKG_VERSION"))),
            ])),
        ]);

        LspResponse::ok(msg.id.as_ref(), result)
    }

    // ── textDocument/didOpen ──────────────────────────────────────────────

    fn handle_did_open(&mut self, msg: &LspMessage) -> Vec<LspResponse> {
        if let Some(params) = &msg.params {
            if let Some(td) = params.get("textDocument") {
                let uri     = td.get("uri")  .and_then(|v| v.as_str()).unwrap_or("").to_string();
                let text    = td.get("text") .and_then(|v| v.as_str()).unwrap_or("").to_string();
                let version = td.get("version").and_then(|v| v.as_i64()).unwrap_or(0);
                let doc = Document { uri: uri.clone(), content: text, version };
                let diags = self.lint(&doc);
                self.documents.insert(uri.clone(), doc);
                return vec![publish_diagnostics(&uri, &diags)];
            }
        }
        vec![]
    }

    // ── textDocument/didChange ─────────────────────────────────────────────

    fn handle_did_change(&mut self, msg: &LspMessage) -> Vec<LspResponse> {
        if let Some(params) = &msg.params {
            let uri = params.get("textDocument")
                .and_then(|v| v.get("uri"))
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            let version = params.get("textDocument")
                .and_then(|v| v.get("version"))
                .and_then(|v| v.as_i64()).unwrap_or(0);

            // Full sync: take last change content
            let text = params.get("contentChanges")
                .and_then(|v| v.as_arr())
                .and_then(|arr| arr.last())
                .and_then(|c| c.get("text"))
                .and_then(|v| v.as_str()).unwrap_or("").to_string();

            let doc = Document { uri: uri.clone(), content: text, version };
            let diags = self.lint(&doc);
            self.documents.insert(uri.clone(), doc);
            return vec![publish_diagnostics(&uri, &diags)];
        }
        vec![]
    }

    fn handle_did_close(&mut self, msg: &LspMessage) -> Vec<LspResponse> {
        if let Some(params) = &msg.params {
            let uri = params.get("textDocument")
                .and_then(|v| v.get("uri"))
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            self.documents.remove(&uri);
            // Clear diagnostics when closed
            return vec![publish_diagnostics(&uri, &[])];
        }
        vec![]
    }

    fn handle_did_save(&mut self, msg: &LspMessage) -> Vec<LspResponse> {
        if let Some(params) = &msg.params {
            let uri = params.get("textDocument")
                .and_then(|v| v.get("uri"))
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            // If text included, update
            if let Some(text) = params.get("text").and_then(|v| v.as_str()) {
                if let Some(doc) = self.documents.get_mut(&uri) {
                    doc.content = text.to_string();
                }
            }
            if let Some(doc) = self.documents.get(&uri) {
                let content = doc.content.clone();
                let doc2 = Document { uri: uri.clone(), content, version: 0 };
                let diags = self.lint(&doc2);
                return vec![publish_diagnostics(&uri, &diags)];
            }
        }
        vec![]
    }

    // ── textDocument/completion ───────────────────────────────────────────

    fn handle_completion(&self, msg: &LspMessage) -> LspResponse {
        let prefix = extract_word_prefix(msg, &self.documents);

        // Disaring DULU, baru dipotong.
        let cocok: Vec<&crate::lsp::protocol::CompletionItem> = self.completions.iter()
            .filter(|item| prefix.is_empty() || item.label.starts_with(&prefix))
            .collect();
        let terpotong = cocok.len() > BATAS_COMPLETION;
        let items: Vec<String> = cocok.iter().take(BATAS_COMPLETION)
            .map(|item| item.to_json())
            .collect();

        // `isIncomplete` dijawab JUJUR.
        let result = json_obj(&[
            ("isIncomplete", terpotong.to_string()),
            ("items",        json_arr(&items)),
        ]);

        LspResponse::ok(msg.id.as_ref(), result)
    }

    // ── textDocument/hover ────────────────────────────────────────────────

    fn handle_hover(&self, msg: &LspMessage) -> LspResponse {
        let Some(word) = extract_word_at_cursor(msg, &self.documents) else {
            return LspResponse::ok(msg.id.as_ref(), "null".to_string());
        };

        // Builtin lebih dulu: dokumentasinya ditulis tangan dan lebih berguna
        // daripada baris deklarasi mana pun.
        let isi = hover_doc(&word).or_else(|| {
            // Lalu simbol yang dideklarasikan di berkas ini. Sebelumnya cabang
            // ini tidak ada sama sekali, jadi hover di atas fungsi, struct,
            // atau metode buatan sendiri selalu menjawab `null`.
            let uri = uri_dari(msg)?;
            let doc = self.documents.get(&uri)?;
            let daftar = self.simbol_dokumen(doc);
            let s = simbol::cari(&daftar, &word)?;
            Some(format!(
                "**{}**\n\n```jrg\n{}\n```",
                s.nama_tampil(),
                baris_pertama_span(&doc.content, s.span)
            ))
        });

        match isi {
            Some(teks) => {
                let result = json_obj(&[
                    ("contents", json_obj(&[
                        ("kind",  json_str("markdown")),
                        ("value", json_str(&teks)),
                    ])),
                ]);
                LspResponse::ok(msg.id.as_ref(), result)
            }
            None => LspResponse::ok(msg.id.as_ref(), "null".to_string()),
        }
    }

    /// Indeks simbol untuk sebuah dokumen.
    fn simbol_dokumen(&self, doc: &Document) -> Vec<simbol::Simbol> {
        use crate::lexer::{lex, SourceFile};
        use crate::parser::parse;

        let sf = SourceFile { id: 0, name: doc.uri.clone(), src: doc.content.clone() };
        let Ok(tokens) = lex(&sf) else { return Vec::new() };
        // `recover: true` — dokumen yang sedang diketik hampir selalu rusak
        // sesaat, dan daftar simbol yang lenyap tiap kurung belum ditutup
        // membuat sidebar berkedip-kedip.
        let (file, _) = parse(tokens, true);
        simbol::dari_ast(&file)
    }

    // ── textDocument/definition ──────────────────────────────────────────

    fn handle_definition(&self, msg: &LspMessage) -> LspResponse {
        let kosong = || LspResponse::ok(msg.id.as_ref(), "null".to_string());
        let Some(word) = extract_word_at_cursor(msg, &self.documents) else { return kosong() };
        let Some(uri) = uri_dari(msg) else { return kosong() };
        let Some(doc) = self.documents.get(&uri) else { return kosong() };

        // Pencocokan nama PERSIS, lewat indeks simbol yang sama dengan yang dipakai
        // hover dan documentSymbol.
        let daftar = self.simbol_dokumen(doc);
        let Some(s) = simbol::cari(&daftar, &word) else { return kosong() };

        let (l0, c0) = posisi_dari_byte(&doc.content, s.span.lo.0 as usize);
        let (l1, c1) = posisi_dari_byte(&doc.content, s.span.hi.0 as usize);
        let result = json_obj(&[
            ("uri",   json_str(&uri)),
            ("range", Range::new(l0, c0, l1, c1).to_json()),
        ]);
        LspResponse::ok(msg.id.as_ref(), result)
    }

    // ── textDocument/documentSymbol ───────────────────────────────────────

    fn handle_symbols(&self, msg: &LspMessage) -> LspResponse {
        let uri = msg.params.as_ref()
            .and_then(|p| p.get("textDocument"))
            .and_then(|v| v.get("uri"))
            .and_then(|v| v.as_str())
            .unwrap_or("").to_string();

        let mut symbols: Vec<String> = Vec::new();

        // Indeks yang sama dengan hover dan definition.
        if let Some(doc) = self.documents.get(&uri) {
            for s in self.simbol_dokumen(doc) {
                let (l0, c0) = posisi_dari_byte(&doc.content, s.span.lo.0 as usize);
                let (l1, c1) = posisi_dari_byte(&doc.content, s.span.hi.0 as usize);
                symbols.push(
                    DocumentSymbol {
                        name: s.nama_tampil(),
                        kind: s.jenis,
                        range: Range::new(l0, c0, l1, c1),
                    }
                    .to_json(),
                );
            }
        }

        LspResponse::ok(msg.id.as_ref(), json_arr(&symbols))
    }

    // ── Lint (diagnostics) ───────────────────────────────────────────────

    fn lint(&self, doc: &Document) -> Vec<Diagnostic> {
        use crate::lexer::{lex, SourceFile};
        use crate::parser::parse;

        let sf = SourceFile {
            id:   0,
            name: doc.uri.clone(),
            src:  doc.content.clone(),
        };

        let mut diags = Vec::new();

        // Rentang diagnostik diambil dari span error yang SEBENARNYA, bukan dikorek
        // dari teks pesannya.
        let rentang = |span: crate::lexer::Span| {
            let (l0, c0) = posisi_dari_byte(&doc.content, span.lo.0 as usize);
            let (l1, c1) = posisi_dari_byte(&doc.content, span.hi.0 as usize);
            // Span kosong (lo == hi) tetap harus terlihat: editor tidak
            // menggambar apa pun untuk rentang selebar nol.
            if (l0, c0) == (l1, c1) {
                Range::new(l0, c0, l1, c1 + 1)
            } else {
                Range::new(l0, c0, l1, c1)
            }
        };

        // Lex errors
        let tokens = match lex(&sf) {
            Ok(t) => t,
            Err(e) => {
                diags.push(Diagnostic::error(rentang(e.span), &format!("lex: {}", e.msg)));
                return diags;
            }
        };

        // Parse errors
        let (_, errs) = parse(tokens, true);
        for e in errs {
            diags.push(Diagnostic::error(rentang(e.span), &e.msg));
        }

        diags
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────

fn publish_diagnostics(uri: &str, diags: &[Diagnostic]) -> LspResponse {
    let items: Vec<String> = diags.iter().map(|d| d.to_json()).collect();
    let params = json_obj(&[
        ("uri",         json_str(uri)),
        ("diagnostics", json_arr(&items)),
    ]);
    LspResponse::notification("textDocument/publishDiagnostics", params)
}

/// URI dokumen dari sebuah permintaan.
fn uri_dari(msg: &LspMessage) -> Option<String> {
    Some(
        msg.params
            .as_ref()?
            .get("textDocument")?
            .get("uri")?
            .as_str()?
            .to_string(),
    )
}

/// Baris pertama dari rentang sebuah deklarasi — yaitu tanda tangannya.
fn baris_pertama_span(src: &str, span: crate::lexer::Span) -> String {
    let lo = (span.lo.0 as usize).min(src.len());
    let sisa = &src[lo..];
    let baris = sisa.lines().next().unwrap_or("").trim_end();
    // Buang `{` pembuka badan supaya yang tampil murni tanda tangannya.
    baris.trim_end_matches('{').trim_end().to_string()
}

/// Kata yang sudah diketik SEBELUM kursor — awalan yang sedang dilengkapi.
fn extract_word_prefix(msg: &LspMessage, docs: &HashMap<String, Document>) -> String {
    let ambil = || -> Option<String> {
        let params = msg.params.as_ref()?;
        let uri = params.get("textDocument")?.get("uri")?.as_str()?;
        let pos = params.get("position")?;
        let line_no = pos.get("line")?.as_i64()? as usize;
        let char_no = pos.get("character")?.as_i64()? as usize;
        let doc = docs.get(uri)?;
        let line = doc.lines().get(line_no)?.to_string();
        let chars: Vec<char> = line.chars().collect();
        let end = char_no.min(chars.len());
        let start = (0..end).rev()
            .find(|&i| !chars[i].is_alphanumeric() && chars[i] != '_' && chars[i] != ':')
            .map(|i| i + 1)
            .unwrap_or(0);
        Some(chars[start..end].iter().collect())
    };
    ambil().unwrap_or_default()
}

fn extract_word_at_cursor(
    msg: &LspMessage,
    docs: &HashMap<String, Document>
) -> Option<String> {
    let params = msg.params.as_ref()?;
    let uri = params.get("textDocument")?.get("uri")?.as_str()?;
    let pos = params.get("position")?;
    let line_no = pos.get("line")?.as_i64()? as usize;
    let char_no = pos.get("character")?.as_i64()? as usize;

    let doc = docs.get(uri)?;
    let line = doc.lines().get(line_no)?.to_string();

    // Extract word around cursor position
    let chars: Vec<char> = line.chars().collect();
    let end = char_no.min(chars.len());

    let start = (0..end).rev()
        .find(|&i| !chars[i].is_alphanumeric() && chars[i] != '_' && chars[i] != ':')
        .map(|i| i + 1)
        .unwrap_or(0);
    let word_end = (end..chars.len())
        .find(|&i| !chars[i].is_alphanumeric() && chars[i] != '_')
        .unwrap_or(chars.len());

    let word: String = chars[start..word_end].iter().collect();
    if word.is_empty() { None } else { Some(word) }
}

/// Ubah offset byte jadi posisi LSP.
fn posisi_dari_byte(src: &str, byte: usize) -> (u32, u32) {
    // Rapatkan ke batas karakter terdekat.
    let mut byte = byte.min(src.len());
    while byte > 0 && !src.is_char_boundary(byte) {
        byte -= 1;
    }

    let mut baris = 0u32;
    let mut awal_baris = 0usize;
    for (i, c) in src.char_indices() {
        if i >= byte { break; }
        if c == '\n' {
            baris += 1;
            awal_baris = i + 1;
        }
    }
    let kolom = src[awal_baris..byte].encode_utf16().count() as u32;
    (baris, kolom)
}
