// src/llvm/codegen_statis.rs — kondisi `if` yang jawabannya sudah pasti saat kompilasi.
// Fungsi tanpa anotasi diturunkan per susunan tipe argumen, jadi di satu instans `typeof(x) == "string"`
// sudah pasti; cabang yang tidak mungkin diambil tidak diturunkan (isinya bisa tidak sah untuk tipe itu).

use crate::parser::*;

use super::codegen::Codegen;
use super::types::JrgType;

impl Codegen {
    /// `typeof` di sini memang bawaan: tidak dibayangi lokal maupun fungsi milik program.
    fn typeof_bawaan(&self, ada_lokal: &dyn Fn(&str) -> bool) -> bool {
        !ada_lokal("typeof") && self.cari_fn_bersarang("typeof").is_none()
            && !self.tables.functions.contains_key(&self.k("typeof"))
            && !self.tables.extern_desk.contains_key("typeof")
    }

    /// `Some(b)` kalau `e` pasti bernilai `b`; `tipe` menjawab tipe statis sebuah nama lokal.
    pub(crate) fn kondisi_statis(&self, e: &Expr, tipe: &dyn Fn(&str) -> Option<JrgType>) -> Option<bool> {
        match e {
            Expr::Unary { op: UnaryOp::Not, expr, .. } => self.kondisi_statis(expr, tipe).map(|b| !b),
            // Ruas kanan cuma dilipat kalau ruas kiri sudah memutuskan atau ikut pasti.
            Expr::Binary { op: BinaryOp::And, lhs, rhs, .. } => match self.kondisi_statis(lhs, tipe)? {
                false => Some(false),
                true => self.kondisi_statis(rhs, tipe),
            },
            Expr::Binary { op: BinaryOp::Or, lhs, rhs, .. } => match self.kondisi_statis(lhs, tipe)? {
                true => Some(true),
                false => self.kondisi_statis(rhs, tipe),
            },
            Expr::Binary { op: op @ (BinaryOp::Eq | BinaryOp::Ne), lhs, rhs, .. } => {
                let (nama, lit) = match (self.nama_typeof_statis(lhs, tipe), teks_literal(rhs)) {
                    (Some(n), Some(l)) => (n, l),
                    _ => (self.nama_typeof_statis(rhs, tipe)?, teks_literal(lhs)?),
                };
                Some((nama == lit) == (*op == BinaryOp::Eq))
            }
            _ => None,
        }
    }

    /// Jawaban `typeof(nama)` kalau sudah pasti dari tipe statis nama itu.
    fn nama_typeof_statis(&self, e: &Expr, tipe: &dyn Fn(&str) -> Option<JrgType>) -> Option<String> {
        let Expr::Call { func, args, .. } = e else { return None };
        let Expr::Ident(f, _) = &**func else { return None };
        if f != "typeof" || args.len() != 1 || !self.typeof_bawaan(&|n| tipe(n).is_some()) { return None; }
        let Expr::Ident(n, _) = &args[0] else { return None };
        let t = tipe(n)?;
        // Nilai bertag dan nilai fungsi dijawab saat jalan (tag; ada/tidaknya lingkungan).
        if matches!(t, JrgType::Any | JrgType::Fn(..) | JrgType::Never) { return None; }
        if let JrgType::Kotak(d) = &t { if matches!(**d, JrgType::Any | JrgType::Fn(..)) { return None; } }
        Some(t.nama_typeof())
    }

    /// Kondisi statis dengan tipe dari lokal yang sedang terlihat di codegen.
    pub(crate) fn kondisi_statis_kini(&self, e: &Expr) -> Option<bool> {
        self.kondisi_statis(e, &|n| self.lookup_local(n).map(|(_, t)| t))
    }
}

/// Blok yang berakhir `return` atau `panik(..)`: tidak pernah jatuh ke ujungnya. `panik` diturunkan
/// dengan blok mati sesudahnya, jadi cabang pasti yang menyimpang harus diakhiri pemanggilnya.
pub(crate) fn blok_menyimpang(b: &Block) -> bool {
    let panik = |e: &Expr| matches!(e, Expr::Call { func, .. } if matches!(&**func, Expr::Ident(n, _) if n == "panik"));
    match (b.tail.as_deref(), b.stmts.last()) {
        (Some(t), _) => panik(t),
        (None, Some(Stmt::Return(_))) => true,
        (None, Some(Stmt::Expr(e))) => panik(e),
        _ => false,
    }
}

fn teks_literal(e: &Expr) -> Option<&str> {
    match e {
        Expr::Literal(LitKind::Str(s), _) | Expr::Literal(LitKind::RawStr(s), _) => Some(s),
        _ => None,
    }
}
