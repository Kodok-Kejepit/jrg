// src/lexer/mod.rs — Compiler — src/lexer/mod.rs
// Complete Tokenizer Implementation

#![allow(dead_code, unused_variables, unused_imports)]

use unicode_xid::UnicodeXID;

// ─── Public entry point ────────────────────────────────────────────────────

/// Lex an entire source file. Returns all tokens including EOF, or the first
/// error encountered. In error-recovery mode (used by LSP) we accumulate
/// errors and emit ErrorToken nodes instead of stopping.
pub fn lex(source: &SourceFile) -> Result<Vec<Token>, LexError> {
    periksa_arah_teks(source)?;
    let mut lexer = Lexer::new(source);
    lexer.tokenize()
}

/// Karakter pengubah arah teks (U+202A–U+202E, U+2066–U+2069) di sumber ditolak, seperti rustc:
/// di dalam komentar atau teks ia membuat kode TAMPIL lain dari yang dijalankan (Trojan Source,
/// CVE-2021-42574). Yang memang perlu di dalam teks ditulis sebagai escape `\u202E`.
fn periksa_arah_teks(source: &SourceFile) -> Result<(), LexError> {
    let Some((i, c)) = source.src.char_indices().find(|(_, c)| matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
        else { return Ok(()) };
    let lo = BytePos(i as u32);
    Err(LexError {
        msg: format!("karakter kendali arah teks U+{:04X} di sumber — ia membuat kode tampil berbeda dari yang \
                      dijalankan (Trojan Source, CVE-2021-42574)\nbantuan: kalau memang dibutuhkan di dalam teks, \
                      tulis escape-nya: `\\u{:04X}`", c as u32, c as u32),
        span: Span::new(lo, BytePos((i + c.len_utf8()) as u32), source.id),
    })
}

/// Sama seperti [`lex`], tapi juga mengembalikan rentang tiap komentar.
pub fn lex_dengan_komentar(
    source: &SourceFile,
) -> Result<(Vec<Token>, Vec<Komentar>), LexError> {
    periksa_arah_teks(source)?;
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;
    Ok((tokens, std::mem::take(&mut lexer.komentar)))
}

// Definisi token, rentang, dan mesinnya dipecah ke tiga berkas;
// `pub use` menjaga jalur `lexer::X` tetap persis seperti sebelumnya.
mod token;
mod span;
mod mesin;
mod aksara;
mod aksara_tabel;

pub use token::*;
pub use span::*;

// ─── Lexer internals ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LexError {
    pub msg:  String,
    pub span: Span,
}

/// Di mana lexer sedang berada relatif terhadap template literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TmplState {
    /// Di bagian teks sebuah template, menunggu penutupnya atau `${`.
    Text { kutip_tiga: bool },
    /// Di dalam `${ ... }`. Angkanya = berapa `{` biasa yang masih
    /// terbuka di dalamnya, supaya `}` milik blok atau dict literal tidak
    /// disangka penutup interpolasi.
    Interp(u32),
}

pub(crate) struct Lexer<'src> {
    src:         &'src str,
    file_id:     u32,
    pos:         usize,          // current byte offset
    chars:       Vec<char>,      // pre-decoded codepoints
    char_pos:    usize,          // current index into chars
    // Tumpukan konteks template yang sedang terbuka (lihat `TmplState`).
    tmpl_stack:  Vec<TmplState>,
    // Antrean token yang sudah dihasilkan tapi belum sempat dikembalikan.
    pending:     std::collections::VecDeque<Token>,
    // Nested block comment depth (nestable comments)
    comment_depth: u32,
    // Auto-semicolon: insert ';' after certain tokens at newline
    last_token_auto_semi: bool,
    // Rentang tiap komentar yang DILEWATI, dicatat apa adanya.
    komentar: Vec<Komentar>,
}

/// Satu komentar, persis seperti yang tertulis di sumber.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Komentar {
    pub span: Span,
    pub blok: bool,
}

// Tokens that trigger automatic semicolon insertion when followed by a
// newline:
pub(crate) fn triggers_auto_semi(kind: &TokenKind) -> bool {
    matches!(kind,
        TokenKind::Ident(_)
        | TokenKind::IntLit(_) | TokenKind::FloatLit(_)
        | TokenKind::ImaginaryLit(_) | TokenKind::RuneLit(_)
        | TokenKind::StringLit(_) | TokenKind::RawStringLit(_)
        | TokenKind::BoolLit(_) | TokenKind::NilLit
        | TokenKind::TemplateLitEnd
        | TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace
        | TokenKind::KwBreak | TokenKind::KwReturn
        // `self` itu nilai seperti nama biasa, `continue` saudara `break`: tanpa ini
        // `let x = self` lalu `x` di baris berikutnya diurai jadi satu pernyataan.
        | TokenKind::KwSelf | TokenKind::KwContinue
        | TokenKind::Question  // trailing `?` error propagation
    )
}

/// Apakah token ini sebuah kata kunci? Sesudah `::` atau `.` ia dibaca sebagai NAMA
/// (`arr::map`, `d.type`) — oleh parser dan oleh titik-koma otomatis.
pub(crate) fn is_keyword_token(k: &TokenKind) -> bool {
    matches!(k,
        TokenKind::KwAsync | TokenKind::KwAwait | TokenKind::KwBreak | TokenKind::KwChan
        | TokenKind::KwConst | TokenKind::KwDefer | TokenKind::KwDo | TokenKind::KwElse
        | TokenKind::KwEnum | TokenKind::KwExtern | TokenKind::KwFn | TokenKind::KwFor
        | TokenKind::KwGo | TokenKind::KwIf | TokenKind::KwImpl | TokenKind::KwImport
        | TokenKind::KwIn | TokenKind::KwLet | TokenKind::KwLoop | TokenKind::KwMatch
        | TokenKind::KwMod | TokenKind::KwMove | TokenKind::KwMut | TokenKind::KwOwned
        | TokenKind::KwPackage | TokenKind::KwPub | TokenKind::KwRaw | TokenKind::KwRef
        | TokenKind::KwReturn | TokenKind::KwSelect | TokenKind::KwSelf | TokenKind::KwShared
        | TokenKind::KwGc | TokenKind::KwSpawn | TokenKind::KwStatic | TokenKind::KwStruct
        | TokenKind::KwTrait | TokenKind::KwType | TokenKind::KwUnsafe | TokenKind::KwUse
        | TokenKind::KwVar | TokenKind::KwWhere | TokenKind::KwWhile | TokenKind::KwYield
        | TokenKind::KwAs | TokenKind::KwCase | TokenKind::KwDefault
        | TokenKind::KwContinue
    )
}
