// src/lexer/span.rs — posisi byte, rentang, dan berkas sumber

#![allow(dead_code, unused_variables, unused_imports)]


// ─── Span / position tracking ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BytePos(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub lo: BytePos,
    pub hi: BytePos,
    pub file_id: u32,
}

impl Span {
    pub fn new(lo: BytePos, hi: BytePos, file_id: u32) -> Self {
        Span { lo, hi, file_id }
    }
    pub fn dummy() -> Self {
        Span::new(BytePos(0), BytePos(0), 0)
    }
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub id:      u32,
    pub name:    String,
    pub src:     String,
}
