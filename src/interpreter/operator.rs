// src/interpreter/operator.rs — src/interpreter/operator.rs — operator yang bisa didefinisikan pengguna.

use crate::parser::{BinaryOp, UnaryOp};

/// Ejaan JRG sebuah operator biner, untuk dipakai DI DALAM pesan.
pub fn simbol_biner(op: &BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",   BinaryOp::Sub => "-",   BinaryOp::Mul => "*",
        BinaryOp::Div => "/",   BinaryOp::Rem => "%",   BinaryOp::Pow => "**",
        BinaryOp::BitAnd => "&", BinaryOp::BitOr => "|", BinaryOp::BitXor => "^",
        BinaryOp::Shl => "<<",  BinaryOp::Shr => ">>",
        BinaryOp::Eq => "==",   BinaryOp::Ne => "!=",
        BinaryOp::Lt => "<",    BinaryOp::Le => "<=",
        BinaryOp::Gt => ">",    BinaryOp::Ge => ">=",
        BinaryOp::And => "&&",  BinaryOp::Or => "||",
        BinaryOp::AndNot => "&^", BinaryOp::Chan => "<-",
    }
}

/// Metode untuk operator biner, atau `None` kalau operator itu tidak bisa
/// didefinisikan ulang.
pub fn nama_biner(op: &BinaryOp) -> Option<&'static str> {
    Some(match op {
        BinaryOp::Add => "add",
        BinaryOp::Sub => "sub",
        BinaryOp::Mul => "mul",
        BinaryOp::Div => "div",
        BinaryOp::Rem => "rem",
        BinaryOp::Pow => "pow",
        BinaryOp::BitAnd => "bit_and",
        BinaryOp::BitOr => "bit_or",
        BinaryOp::BitXor => "bit_xor",
        BinaryOp::Shl => "shl",
        BinaryOp::Shr => "shr",
        // `==` dan `!=` sama-sama lewat `eq`; `!=` membalik hasilnya, supaya
        // mustahil menulis tipe yang bilang dua nilai sekaligus sama DAN
        // tidak sama.
        BinaryOp::Eq | BinaryOp::Ne => "eq",
        // Keempat pembanding urutan lewat SATU metode yang mengembalikan
        // bilangan bertanda, dengan alasan yang sama: empat metode terpisah
        // memungkinkan `a < b` dan `a >= b` dua-duanya benar.
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => "cmp",
        _ => return None,
    })
}

/// Metode untuk operator awalan.
pub fn nama_unary(op: &UnaryOp) -> Option<&'static str> {
    Some(match op {
        UnaryOp::Neg => "neg",
        UnaryOp::Not => "not",
        UnaryOp::BitNot => "bit_not",
        _ => return None,
    })
}

/// Apakah operator ini memakai `cmp` (yang hasilnya perlu ditafsirkan).
pub fn pakai_cmp(op: &BinaryOp) -> bool {
    matches!(op, BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge)
}

/// Apakah operator ini memakai `eq`.
pub fn pakai_eq(op: &BinaryOp) -> bool {
    matches!(op, BinaryOp::Eq | BinaryOp::Ne)
}

/// Terjemahkan hasil `cmp` (bilangan bertanda) jadi jawaban operatornya.
pub fn tafsir_cmp(op: &BinaryOp, urutan: i128) -> bool {
    match op {
        BinaryOp::Lt => urutan < 0,
        BinaryOp::Le => urutan <= 0,
        BinaryOp::Gt => urutan > 0,
        BinaryOp::Ge => urutan >= 0,
        _ => false,
    }
}

/// Pesan untuk metode operator yang bentuknya salah.
pub fn pesan_arity(tipe: &str, metode: &str, diminta: usize) -> String {
    format!(
        "`{}::{}` dipakai sebagai operator, jadi ia harus menerima `self` dan \
         tepat satu operand — yang ini minta {} parameter",
        tipe, metode, diminta
    )
}

pub fn pesan_arity_unary(tipe: &str, metode: &str, diminta: usize) -> String {
    format!(
        "`{}::{}` dipakai sebagai operator awalan, jadi ia harus menerima \
         `self` saja — yang ini minta {} parameter",
        tipe, metode, diminta
    )
}

pub fn pesan_hasil(tipe: &str, metode: &str, harus: &str, dapat: &str) -> String {
    format!(
        "`{}::{}` dipakai sebagai operator, jadi ia harus mengembalikan `{}` — \
         yang ini mengembalikan `{}`",
        tipe, metode, harus, dapat
    )
}
