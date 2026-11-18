//! `json::merge` antara struct dan dict di backend LLVM. Acuan menjawab struct yang boleh membawa field tambahan; di
//! sini tata letak struct tetap, jadi yang ditutup cuma yang muat di struct itu, dan sisanya gagal saat jalan.

use super::codegen::{Codegen, Val};
use super::types::JrgType;
use crate::lexer::Span;
use crate::parser::*;

impl Codegen {
    /// `json::merge(d, p)` / `json::merge(p, d)` dengan `p` struct dan `d` dict: hasilnya struct `p` dengan nilai dict
    /// yang datang SESUDAHNYA menimpa field bernama sama. Kunci dict yang bukan field, atau yang jenis nilainya beda
    /// dengan field-nya, gagal saat jalan (acuan menjawab struct berfield tambahan atau berjenis lain).
    pub(crate) fn gabung_json_struct(&mut self, args: &[Expr], span: Span) -> Option<Result<Val, String>> {
        let mut nilai = Vec::new();
        for a in args {
            match self.codegen_expr(a, None) { Ok(v) => nilai.push(v), Err(g) => return Some(Err(g)) }
        }
        let (s, d) = match (&nilai[0].ty, &nilai[1].ty) {
            (JrgType::Struct(_), JrgType::Dict(_) | JrgType::DictCampur(_)) => (0, 1),
            (JrgType::Dict(_) | JrgType::DictCampur(_), JrgType::Struct(_)) => (1, 0),
            _ => return None,
        };
        let JrgType::Struct(nama) = nilai[s].ty.clone() else { return None };
        let field = self.types.structs.get(&nama)?.fields.clone();
        let mut id = Vec::new();
        for v in &nilai {
            let ptr = self.spill_to_place(v);
            id.push(self.lokal_sementara(ptr, v.ty.clone(), span));
        }
        let (ps, pd) = (id[s].clone(), id[d].clone());
        let tampil = crate::ruang_tipe::bersih(&nama).to_string();
        let teks = |t: &str| Expr::Literal(LitKind::Str(t.to_string()), span);
        let panggil = |jalur: &[&str], a: Vec<Expr>| Expr::Call {
            func: Box::new(Expr::PathExpr { segments: jalur.iter().map(|s| s.to_string()).collect(), span }), args: a, span };
        let panik = |t: String| Stmt::Expr(Box::new(panggil(&["", "panik"], vec![teks(&t)])));
        let blok = |stmts: Vec<Stmt>| Block { stmts, tail: None, span };
        let jika = |cond: Expr, then: Vec<Stmt>| Stmt::If(IfStmt {
            init: None, cond: Box::new(cond), then: blok(then), else_ifs: Vec::new(), else_: None, span });
        let ada = |f: &str| panggil(&["dict", "has"], vec![pd.clone(), teks(f)]);
        let n = Expr::Ident(format!("$gabung_n{}", self.penerima_lapis), span);
        let hasil = Expr::Ident(format!("$gabung{}", self.penerima_lapis), span);
        let ikat = |e: &Expr, init: Expr| {
            let Expr::Ident(nm, _) = e else { unreachable!() };
            Stmt::Let(LetStmt { is_mut: true, ownership: None, pattern: Pattern::Ident { name: nm.clone(), is_mut: true, span },
                ty: None, init: Box::new(init), lifetime: None, span })
        };
        let tambah = |e: &Expr| Stmt::Assign(AssignStmt {
            targets: vec![e.clone()], op: AssignOp::PlusEq, values: vec![Expr::Literal(LitKind::Int(1), span)], span });
        let mut stmts = vec![ikat(&hasil, ps.clone()), ikat(&n, Expr::Literal(LitKind::Int(0), span))];
        for (f, _) in &field { stmts.push(jika(ada(f), vec![tambah(&n)])); }
        let daftar: Vec<&str> = field.iter().map(|(f, _)| f.as_str()).collect();
        let beda = Expr::Binary { op: BinaryOp::Ne, lhs: Box::new(panggil(&["", "len"], vec![pd.clone()])),
            rhs: Box::new(n.clone()), span };
        stmts.push(jika(beda, vec![panik(format!(
            "`json::merge`: dict-nya membawa kunci yang bukan field struct `{}` (field-nya: {}) — di backend LLVM tata \
             letak struct tetap, jadi field tambahan belum bisa; ubah dulu jadi dict, mis. `dict {{ ...p }}`",
            tampil, daftar.join(", ")))]));
        // Dict yang datang sesudah struct menimpa field-nya; yang sebelum struct ditimpa struct itu.
        if d > s {
            for (f, ft) in &field {
                let baca = Expr::Index { obj: Box::new(pd.clone()), idx: Box::new(teks(f)), span };
                let jenis = match &nilai[d].ty {
                    JrgType::Dict(v) => Some((**v).clone()),
                    JrgType::DictCampur(k) => k.iter().find(|(n, _)| n == f).map(|(_, t)| t.clone()),
                    _ => None,
                };
                let isi = match jenis {
                    Some(t) if t == *ft => Stmt::Assign(AssignStmt {
                        targets: vec![Expr::Field { obj: Box::new(hasil.clone()), field: f.clone(), span }],
                        op: AssignOp::Eq, values: vec![baca], span }),
                    t => panik(format!(
                        "`json::merge`: kunci `{}` di dict bernilai `{}`, sedangkan field `{}.{}` bertipe `{}` — di backend \
                         LLVM tipe field struct tetap; ubah dulu jadi dict, mis. `dict {{ ...p }}`",
                        f, t.map(|t| t.nama_typeof()).unwrap_or_else(|| "nilai".into()), tampil, f, ft.nama_typeof())),
                };
                stmts.push(jika(ada(f), vec![isi]));
            }
        }
        let e = Expr::Block(Block { stmts, tail: Some(Box::new(hasil)), span });
        Some(self.codegen_expr(&e, None))
    }
}
