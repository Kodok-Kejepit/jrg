// src/pipa.rs — jalur satu sumber: lex, parse, lalu interpreter, dengan
// diagnostik yang sama di tiap pintu (`jrgc`, `jrgm run`, dan peramban).

use crate::diag;
use crate::lexer::{lex, SourceFile, Token};
use crate::parser::ParseError;

/// Nama token internal -> karakter yang benar-benar diketik programmer.
/// `None` untuk token yang tidak punya wujud tunggal (mis. `Ident`).
pub fn token_symbol(name: &str) -> Option<&'static str> {
    match name {
        "RBrace" => Some("}"),
        "RParen" => Some(")"),
        "RBracket" => Some("]"),
        "LBrace" => Some("{"),
        "LParen" => Some("("),
        "LBracket" => Some("["),
        "Semi" => Some(";"),
        "Comma" => Some(","),
        "Colon" => Some(":"),
        "Eq" => Some("="),
        "FatArrow" => Some("=>"),
        "Arrow" => Some("->"),
        _ => None,
    }
}

/// Menulis ulang pesan parser supaya menyebut karakter yang diketik
/// programmer, bukan nama varian token internal.
pub fn humanize_parse_message(msg: &str) -> String {
    let expected = msg.split("expected `").nth(1).and_then(|s| s.split('`').next());
    let found = msg.split("found `").nth(1).and_then(|s| s.split('`').next());

    match (expected, found) {
        (Some(exp), Some("Eof")) => match token_symbol(exp) {
            Some(sym) => format!("kurang `{sym}` — file keburu habis sebelum semuanya ditutup"),
            None => format!("file keburu habis, masih butuh `{exp}` di sini"),
        },
        (Some(exp), Some(found_tok)) => {
            let exp_s = token_symbol(exp).map(|s| format!("`{s}`")).unwrap_or_else(|| format!("`{exp}`"));
            let found_s = token_symbol(found_tok)
                .map(|s| format!("`{s}`"))
                .unwrap_or_else(|| format!("`{found_tok}`"));
            format!("harusnya ada {exp_s} di sini, yang ketemu malah {found_s}")
        }
        _ => msg.to_string(),
    }
}

/// Menerjemahkan pesan parser yang berorientasi-token jadi saran yang bisa
/// langsung dikerjakan programmer.
pub fn parse_error_help(msg: &str) -> Option<String> {
    let delim = token_symbol;

    // Bentuk paling umum: "expected `X`, found `Y`".
    let expected = msg.split("expected `").nth(1)?.split('`').next()?;
    let found = msg.split("found `").nth(1).and_then(|s| s.split('`').next());

    match (delim(expected), found) {
        (Some(ch), Some("Eof")) => Some(format!(
            "sepertinya ada `{ch}` yang belum ditutup — file keburu habis. \
             Cek blok/kurung terakhir yang kamu buka"
        )),
        (Some(ch), _) => Some(format!("tambahkan `{ch}` di posisi ini")),
        _ => None,
    }
}

/// Lex `sf`; galatnya dilaporkan dengan lokasi dan caret.
pub fn lex_dilaporkan(sf: &SourceFile, peta: &diag::SourceMap) -> Option<Vec<Token>> {
    lex(sf).map_err(|e| {
        let mut baris = e.msg.lines();
        let d = diag::Diagnostic::error(baris.next().unwrap_or_default().to_string())
            .with_span(e.span);
        diag::tambah_baris_sisa(d, baris).emit(peta);
    }).ok()
}

/// Laporkan galat parse, dengan kalimat yang menyebut karakter yang diketik —
/// sebagai peringatan kalau `recover`.
pub fn laporkan_galat_parse(galat: &[ParseError], recover: bool, peta: &diag::SourceMap) {
    for e in galat {
        let severity = if recover { diag::Severity::Warning } else { diag::Severity::Error };
        let pesan = humanize_parse_message(&e.msg);
        let mut baris = pesan.lines();
        let mut d = diag::Diagnostic::new(severity, baris.next().unwrap_or_default().to_string())
            .with_code(diag::codes::UNEXPECTED_TOKEN)
            .with_span(e.span);
        d = diag::tambah_baris_sisa(d, baris);
        if let Some(help) = parse_error_help(&e.msg) {
            d = d.with_help(help);
        }
        d.emit(peta);
    }
}

/// Galat lex/parse sebuah modul yang di-`import`: kalimat yang sama dengan berkas
/// utama, span di BERKAS MODULNYA, dan galat berikutnya sebagai catatan berbaris.
/// Satu definisi untuk dua mesin — dulu tanpa lokasi, dengan dua kalimat berbeda.
pub fn galat_parse_modul(src: &str, galat: &[(String, crate::lexer::Span)]) -> (String, crate::lexer::Span) {
    let baris = |sp: &crate::lexer::Span| src[..(sp.lo.0 as usize).min(src.len())].matches('\n').count() + 1;
    let (msg, span) = &galat[0];
    let mut pesan = humanize_parse_message(msg);
    if let Some(h) = parse_error_help(msg) {
        pesan.push_str("\nbantuan: ");
        pesan.push_str(&h);
    }
    for (m, sp) in &galat[1..] {
        pesan.push_str(&format!("\ncatatan: baris {}: {}", baris(sp), humanize_parse_message(m)));
    }
    (pesan, *span)
}

