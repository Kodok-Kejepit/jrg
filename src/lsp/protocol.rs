// src/lsp/protocol.rs — src/lsp/protocol.rs — LSP protocol types + message builder

use super::{JsonValue, json_str, json_obj};

// ─── LSP Message (inbound) ────────────────────────────────────────────────

#[derive(Debug)]
pub struct LspMessage {
    pub id:     Option<JsonValue>,   // null for notifications
    pub method: String,
    pub params: Option<JsonValue>,
}

impl LspMessage {
    pub fn from_json(v: &JsonValue) -> Option<Self> {
        let method = v.get("method")?.as_str()?.to_string();
        let id     = v.get("id").cloned();
        let params = v.get("params").cloned();
        Some(LspMessage { id, method, params })
    }
}

// ─── LSP Response (outbound) ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LspResponse {
    pub id:     Option<JsonValue>,
    pub result: Option<String>,   // pre-serialized JSON
    pub error:  Option<LspError>,
}

#[derive(Debug, Clone)]
pub struct LspError {
    pub code:    i32,
    pub message: String,
}

impl LspResponse {
    pub fn ok(id: Option<&JsonValue>, result: String) -> Self {
        LspResponse { id: id.cloned(), result: Some(result), error: None }
    }

    pub fn null_result(id: Option<&JsonValue>) -> Self {
        LspResponse { id: id.cloned(), result: Some("null".to_string()), error: None }
    }

    pub fn err(id: Option<&JsonValue>, code: i32, msg: &str) -> Self {
        LspResponse {
            id:     id.cloned(),
            result: None,
            error:  Some(LspError { code, message: msg.to_string() }),
        }
    }

    pub fn notification(method: &str, params: String) -> Self {
        // Notifications don't have id or result/error in the traditional sense
        // We encode as a special response that just sends a notification
        LspResponse {
            id:     Some(JsonValue::Str(format!("notif:{}", method))),
            result: Some(params),
            error:  None,
        }
    }

    pub fn to_json(&self) -> String {
        // Check if this is a notification
        if let Some(JsonValue::Str(s)) = &self.id {
            if let Some(notif_method) = s.strip_prefix("notif:") {
                let params = self.result.as_deref().unwrap_or("{}");
                return json_obj(&[
                    ("jsonrpc", json_str("2.0")),
                    ("method",  json_str(notif_method)),
                    ("params",  params.to_string()),
                ]);
            }
        }

        let id_str = match &self.id {
            Some(JsonValue::Number(n)) => format!("{}", *n as i64),
            Some(JsonValue::Str(s))    => json_str(s),
            Some(JsonValue::Null) | None => "null".to_string(),
            _ => "null".to_string(),
        };

        if let Some(err) = &self.error {
            return json_obj(&[
                ("jsonrpc", json_str("2.0")),
                ("id",      id_str),
                ("error",   json_obj(&[
                    ("code",    err.code.to_string()),
                    ("message", json_str(&err.message)),
                ])),
            ]);
        }

        json_obj(&[
            ("jsonrpc", json_str("2.0")),
            ("id",      id_str),
            ("result",  self.result.clone().unwrap_or("null".to_string())),
        ])
    }
}

// ─── LSP Position + Range ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default)]
pub struct Position { pub line: u32, pub character: u32 }

#[derive(Debug, Clone, Copy, Default)]
pub struct Range { pub start: Position, pub end: Position }

impl Position {
    pub fn to_json(&self) -> String {
        json_obj(&[
            ("line",      self.line.to_string()),
            ("character", self.character.to_string()),
        ])
    }
}

impl Range {
    pub fn new(sl: u32, sc: u32, el: u32, ec: u32) -> Self {
        Range { start: Position { line: sl, character: sc },
                end:   Position { line: el, character: ec } }
    }
    pub fn to_json(&self) -> String {
        json_obj(&[
            ("start", self.start.to_json()),
            ("end",   self.end.to_json()),
        ])
    }
}

// ─── LSP Diagnostic ───────────────────────────────────────────────────────

pub struct Diagnostic {
    pub range:   Range,
    pub severity: u8,     // 1=Error, 2=Warning, 3=Info, 4=Hint
    pub message: String,
    pub source:  String,
}

impl Diagnostic {
    pub fn error(range: Range, msg: &str) -> Self {
        Diagnostic { range, severity: 1, message: msg.to_string(), source: "jrg".to_string() }
    }
    pub fn to_json(&self) -> String {
        json_obj(&[
            ("range",    self.range.to_json()),
            ("severity", self.severity.to_string()),
            ("message",  json_str(&self.message)),
            ("source",   json_str(&self.source)),
        ])
    }
}

// ─── LSP Completion Item ──────────────────────────────────────────────────

pub struct CompletionItem {
    pub label:         String,
    pub kind:          u8,     // 1=Text,2=Method,3=Fn,4=Constructor,5=Field,6=Var,14=Keyword
    pub detail:        Option<String>,
    pub documentation: Option<String>,
}

impl CompletionItem {
    pub fn keyword(kw: &str) -> Self {
        CompletionItem { label: kw.to_string(), kind: 14,
            detail: None, documentation: None }
    }
    pub fn function(name: &str, sig: &str, doc: &str) -> Self {
        CompletionItem {
            label: name.to_string(), kind: 3,
            detail: Some(sig.to_string()),
            documentation: Some(doc.to_string()),
        }
    }
    pub fn to_json(&self) -> String {
        let mut pairs = vec![
            ("label", json_str(&self.label)),
            ("kind",  self.kind.to_string()),
        ];
        let detail_str;
        let doc_str;
        if let Some(d) = &self.detail {
            detail_str = json_str(d);
            pairs.push(("detail", detail_str));
        }
        if let Some(d) = &self.documentation {
            doc_str = json_obj(&[
                ("kind",  json_str("markdown")),
                ("value", json_str(d)),
            ]);
            pairs.push(("documentation", doc_str));
        }
        let pairs_ref: Vec<(&str, String)> = pairs.into_iter().collect();
        json_obj(&pairs_ref)
    }
}

// ─── DocumentSymbol ───────────────────────────────────────────────────────

pub struct DocumentSymbol {
    pub name:  String,
    pub kind:  u8,   // 5=Class,12=Function,6=Method,14=Const
    pub range: Range,
}

impl DocumentSymbol {
    pub fn to_json(&self) -> String {
        json_obj(&[
            ("name",           json_str(&self.name)),
            ("kind",           self.kind.to_string()),
            ("range",          self.range.to_json()),
            ("selectionRange", self.range.to_json()),
        ])
    }
}
