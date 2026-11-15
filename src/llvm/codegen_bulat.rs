//! Pecahan di tempat bilangan bulat sebuah builtin: gagal SAAT JALAN dengan kalimat acuan.
//! Daftarnya dikunci tes yang menurunkan himpunan yang sama dari mesin acuan.

use crate::parser::*;
use crate::lexer::Span;
use super::codegen::{Codegen, Val};
use super::types::*;

/// Builtin dan posisi argumennya (dari 0, penerima method = 0) yang di acuan WAJIB bulat:
/// pecahan di sana ditolak `pesan_bukan_bulat`, bukan diterima sebagai nilai.
pub const SLOT_BULAT: &[(&str, &[usize])] = &[
    ("arr::chunks", &[1]), ("arr::drop", &[1]), ("arr::fill", &[1]), ("arr::get", &[1]),
    ("arr::get_or", &[1]), ("arr::insert", &[1]), ("arr::remove", &[1]), ("arr::repeat", &[1]),
    ("arr::skip", &[1]), ("arr::swap", &[1, 2]), ("arr::take", &[1]), ("arr::windows", &[1]),
    ("math::factorial", &[0]), ("math::gcd", &[0, 1]), ("math::lcm", &[0, 1]),
    ("math::wrap_add", &[0, 1]), ("math::wrap_mul", &[0, 1]), ("math::wrap_sub", &[0, 1]),
    ("rand::int", &[0, 1]), ("rand::sample", &[1]),
    ("str::center", &[1]), ("str::char_at", &[1]), ("str::ljust", &[1]), ("str::pad_left", &[1]),
    ("str::pad_right", &[1]), ("str::repeat", &[1]), ("str::rjust", &[1]), ("str::substr", &[1, 2]),
    ("arr::rotate", &[1]), ("range", &[0, 1, 2]), ("range_inclusive", &[0, 1]),
    ("jawa::array", &[0]), ("jawa::array_get", &[1]), ("jawa::array_set", &[1]), ("jawa::buffer", &[0]),
    ("jawa::buffer_fill", &[1]), ("jawa::buffer_get", &[1]), ("jawa::buffer_set", &[1, 2]),
];

fn slot(nama: &str) -> Option<&'static [usize]> {
    SLOT_BULAT.iter().find(|(n, _)| *n == nama).map(|(_, p)| *p)
}

/// Kalimat acuan untuk pecahan `p` di argumen ke-`pos` (dari 0) builtin `nama`. Alias
/// menyebut kembarannya, seperti di acuan; `range` menyebut nomor argumennya.
pub fn kalimat_slot(nama: &str, pos: usize, p: &str) -> String {
    let nama = match nama { "str::ljust" => "str::pad_right", "str::rjust" => "str::pad_left", n => n };
    if matches!(nama, "range" | "range_inclusive") {
        return crate::interpreter::pesan_bukan_bulat_ke(nama, pos + 1, p);
    }
    crate::interpreter::pesan_bukan_bulat(nama, "float", Some(p))
}

impl Codegen {
    pub(crate) fn pasti_pecahan(&self, e: &Expr) -> bool {
        match e {
            Expr::Literal(LitKind::Float(_), _) => true,
            Expr::Unary { op: UnaryOp::Neg, expr, .. } => self.pasti_pecahan(expr),
            _ => self.peek_expr_type(e).map_or(false, |t| t.is_float()),
        }
    }

    /// `{ let $b0 = a0; …; ::panik(<kalimat acuan atas $bk>) }`: semua argumen dievaluasi
    /// lebih dulu, urut tulis, seperti di acuan, lalu gagal di argumen pecahan pertama.
    pub(crate) fn slot_bulat_pecahan(&mut self, nama: &str, semua: &[Expr], expected: Option<&JrgType>, span: Span)
        -> Option<Result<Val, String>> {
        let k = *slot(nama)?.iter().find(|&&p| semua.get(p).map_or(false, |e| self.pasti_pecahan(e)))?;
        self.penerima_lapis += 1;
        let nomor = self.penerima_lapis;
        let id = |i: usize| Expr::Ident(format!("$bulat{}_{}", nomor, i), span);
        let mut stmts: Vec<Stmt> = semua.iter().enumerate().map(|(i, e)| Stmt::Let(LetStmt {
            is_mut: false, ownership: None,
            pattern: Pattern::Ident { name: format!("$bulat{}_{}", nomor, i), is_mut: false, span },
            ty: None, init: Box::new(e.clone()), lifetime: None, span })).collect();
        const TANDA: &str = "\u{1}pecahan\u{1}";
        let kalimat = kalimat_slot(nama, k, TANDA);
        let bawaan = |n: &str, a: Vec<Expr>| Expr::Call {
            func: Box::new(Expr::PathExpr { segments: vec![String::new(), n.to_string()], span }), args: a, span };
        let mut pesan: Option<Expr> = None;
        let potongan: Vec<&str> = kalimat.split(TANDA).collect();
        for (i, p) in potongan.iter().enumerate() {
            let mut bagian = vec![Expr::Literal(LitKind::Str(p.to_string()), span)];
            if i + 1 < potongan.len() { bagian.push(bawaan("str", vec![id(k)])); }
            for b in bagian {
                pesan = Some(match pesan.take() {
                    None => b,
                    Some(a) => Expr::Binary { op: BinaryOp::Add, lhs: Box::new(a), rhs: Box::new(b), span },
                });
            }
        }
        stmts.push(Stmt::Expr(Box::new(bawaan("panik", vec![pesan?]))));
        self.slot_bulat_gagal = Some(span);
        Some(self.codegen_expr(&Expr::Block(Block { stmts, tail: None, span }), expected))
    }

    /// Bentuk method (`a.get(1.5)`): penerimanya argumen ke-0 builtin namespace-nya.
    pub(crate) fn slot_bulat_metode(&mut self, obj: &Expr, method: &str, args: &[Expr], expected: Option<&JrgType>, span: Span)
        -> Option<Result<Val, String>> {
        let ns = if matches!(obj, Expr::Array { .. } | Expr::ArrayRepeat { .. }) { "arr" } else { match self.peek_expr_type(obj)? {
            JrgType::Str => "str",
            JrgType::Array(..) | JrgType::Slice(_) => "arr",
            _ => return None,
        } };
        let nama = format!("{}::{}", ns, method);
        slot(&nama)?;
        let mut semua = Vec::with_capacity(args.len() + 1);
        semua.push(obj.clone());
        semua.extend(args.iter().cloned());
        self.slot_bulat_pecahan(&nama, &semua, expected, span)
    }
}

impl Codegen {
    /// `s.upper(1)`, `"a-b".split()`: jumlah argumen method bawaan yang salah gagal SAAT JALAN
    /// dengan kalimat acuan, sesudah penerima dan argumennya dievaluasi — dulu yang lebih dibuang.
    pub(crate) fn jumlah_arg_metode_salah(&mut self, obj: &Expr, method: &str, args: &[Expr], expected: Option<&JrgType>, span: Span)
        -> Option<Result<Val, String>> {
        use crate::interpreter::{pesan_arg_metode_skalar, pesan_jumlah_arg_metode};
        let n = args.len();
        let pesan = if matches!(obj, Expr::Array { .. } | Expr::ArrayRepeat { .. }) {
            pesan_jumlah_arg_metode("arr", method, n)
        } else {
            match self.peek_expr_type(obj)? {
                JrgType::Str => pesan_jumlah_arg_metode("str", method, n),
                JrgType::Array(..) | JrgType::Slice(_) => pesan_jumlah_arg_metode("arr", method, n),
                t if t.is_float() => pesan_arg_metode_skalar("float", method, n),
                t if t.is_int() => pesan_arg_metode_skalar("int", method, n),
                // `impl dict` milik program menang atas bawaan bernama sama.
                JrgType::Dict(_) | JrgType::DictCampur(_) if !self.ada_metode_program(method, n) =>
                    pesan_jumlah_arg_metode("dict", method, n),
                JrgType::Enum(e) if e.starts_with("Option$") => pesan_jumlah_arg_metode("Option", method, n),
                JrgType::Enum(e) if e.starts_with("Result$") => pesan_jumlah_arg_metode("Result", method, n),
                _ => None,
            }
        }?;
        self.penerima_lapis += 1;
        let nomor = self.penerima_lapis;
        let mut stmts: Vec<Stmt> = std::iter::once(obj).chain(args.iter()).enumerate().map(|(i, e)| Stmt::Let(LetStmt {
            is_mut: false, ownership: None,
            pattern: Pattern::Ident { name: format!("$argm{}_{}", nomor, i), is_mut: false, span },
            ty: None, init: Box::new(e.clone()), lifetime: None, span })).collect();
        let panik = Expr::Call {
            func: Box::new(Expr::PathExpr { segments: vec![String::new(), "panik".to_string()], span }),
            args: vec![Expr::Literal(LitKind::Str(pesan), span)], span };
        stmts.push(Stmt::Expr(Box::new(panik)));
        Some(self.codegen_expr(&Expr::Block(Block { stmts, tail: None, span }), expected))
    }
}
