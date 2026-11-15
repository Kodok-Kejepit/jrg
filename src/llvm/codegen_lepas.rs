// src/llvm/codegen_lepas.rs — handle FFI (`py::`/`js::`/`php::`/`lua::`) yang TERBUKTI tidak keluar dari bloknya
// dilepas di ujung normal blok itu. LLVM tidak punya titik "nilai mati" (nilai disalin bita demi bita), jadi
// yang dilepas cuma handle yang sisa bloknya pakai sebagai argumen FFI, penerima method, atau di `wok`/`str`.

use crate::parser::*;

const NS_FFI: [&str; 4] = ["py", "js", "php", "lua"];

/// `ns::f(..)` milik FFI (juga `ns::f(..)?` dan `ns::f(..).unwrap()`): hasilnya kotak titipan BARU dari jembatan,
/// tidak pernah alias argumennya, dan `Result` sementaranya tidak dipegang siapa pun.
fn panggilan_ffi(e: &Expr) -> bool {
    let e = match e {
        Expr::Propagate { expr, .. } => &**expr,
        Expr::Method { obj, method, args, .. } if args.is_empty() && method == "unwrap" => &**obj,
        lain => lain,
    };
    let Expr::Call { func, .. } = e else { return false };
    let Expr::PathExpr { segments, .. } = &**func else { return false };
    segments.len() >= 2 && NS_FFI.contains(&segments[segments.len() - 2].as_str())
}

fn bawaan_baca(e: &Expr) -> bool {
    matches!(e, Expr::Ident(n, _) if matches!(n.as_str(), "wok" | "wok_" | "str" | "typeof"))
}

fn kosong() -> Expr { Expr::Literal(LitKind::Nil, crate::lexer::Span::dummy()) }

/// Ganti kemunculan `h` di tempat yang tidak menyalinnya; yang tersisa sesudahnya berarti `h` bisa keluar.
fn ganti_e(e: &mut Expr, h: &str) {
    let tepat = |x: &Expr| matches!(x, Expr::Ident(n, _) if n == h);
    match e {
        Expr::Call { func, args, .. } if panggilan_ffi_kepala(func) || bawaan_baca(func) => {
            for a in args.iter_mut() { if tepat(a) { *a = kosong(); } else { ganti_e(a, h); } }
        }
        Expr::Call { func, args, .. } => { ganti_e(func, h); for a in args.iter_mut() { ganti_e(a, h); } }
        Expr::Method { obj, args, .. } => {
            if tepat(obj) { **obj = kosong(); } else { ganti_e(obj, h); }
            for a in args.iter_mut() { ganti_e(a, h); }
        }
        Expr::Field { obj, .. } if tepat(obj) => **obj = kosong(),
        Expr::Field { obj, .. } | Expr::Unary { expr: obj, .. } => ganti_e(obj, h),
        Expr::Binary { lhs, rhs, .. } => { ganti_e(lhs, h); ganti_e(rhs, h); }
        Expr::Ternary { cond, then, else_, .. } => { ganti_e(cond, h); ganti_e(then, h); ganti_e(else_, h); }
        // `if`/blok di EKOR blok diurai sebagai ekspresi, bukan pernyataan.
        Expr::If(i) => ganti_if(i, h),
        Expr::Block(b) => ganti_blok(b, h),
        _ => {}
    }
}

fn ganti_if(i: &mut IfStmt, h: &str) {
    ganti_e(&mut i.cond, h);
    ganti_blok(&mut i.then, h);
    for (_, c, b) in i.else_ifs.iter_mut() { ganti_e(c, h); ganti_blok(b, h); }
    if let Some(b) = i.else_.as_mut() { ganti_blok(b, h); }
}

fn panggilan_ffi_kepala(func: &Expr) -> bool {
    matches!(func, Expr::PathExpr { segments, .. }
        if segments.len() >= 2 && NS_FFI.contains(&segments[segments.len() - 2].as_str()))
}

fn ganti_blok(b: &mut Block, h: &str) {
    for s in b.stmts.iter_mut() { ganti_s(s, h); }
    if let Some(t) = b.tail.as_deref_mut() { ganti_e(t, h); }
}

fn ganti_s(s: &mut Stmt, h: &str) {
    match s {
        Stmt::Expr(e) => ganti_e(e, h),
        Stmt::Let(l) => ganti_e(&mut l.init, h),
        Stmt::Var(v) => if let Some(i) = v.init.as_deref_mut() { ganti_e(i, h) },
        Stmt::Assign(a) => for v in a.values.iter_mut() { ganti_e(v, h) },
        Stmt::Return(r) => for v in r.values.iter_mut() { ganti_e(v, h) },
        Stmt::Block(b) => ganti_blok(b, h),
        Stmt::If(i) => ganti_if(i, h),
        Stmt::While(w) => { ganti_e(&mut w.cond, h); ganti_blok(&mut w.body, h); }
        Stmt::Loop(l) => ganti_blok(&mut l.body, h),
        Stmt::For(ForStmt::Range { iter, body, .. }) => { ganti_e(iter, h); ganti_blok(body, h); }
        _ => {}
    }
}

/// Nama yang boleh dilepas di ujung blok sesudah `s` diturunkan: `let h = ns::f(..)` (FFI) yang sisa bloknya
/// (`sesudah`, `ekor`) tidak pernah menyalin `h`.
pub(crate) fn calon_lepas(s: &Stmt, sesudah: &[Stmt], ekor: Option<&Expr>) -> Option<String> {
    let Stmt::Let(l) = s else { return None };
    let Pattern::Ident { name, .. } = &l.pattern else { return None };
    if l.is_mut || name == "_" || !panggilan_ffi(&l.init) { return None; }
    let mut sisa = Block { stmts: sesudah.to_vec(), tail: ekor.map(|e| Box::new(e.clone())), span: crate::lexer::Span::dummy() };
    ganti_blok(&mut sisa, name);
    let mut disebut = std::collections::HashSet::new();
    crate::interpreter::capture::kumpul_nama_blok(&sisa, &mut disebut);
    (!disebut.contains(name)).then(|| name.clone())
}

impl super::codegen::Codegen {
    /// `calon` (nama dari `calon_lepas`) dicatat kalau lokalnya memang nilai bertag milik fungsi ini.
    pub(crate) fn catat_lepas(&self, calon: Option<String>, lepas: &mut Vec<(String, String)>) {
        let Some(n) = calon else { return };
        if self.tangkapan.contains(&n) { return; }
        if let Some((slot, super::types::JrgType::Any)) = self.lookup_local(&n) { lepas.push((n, slot)); }
    }

    /// Lepas kotak titipan di slot-slot ini (yang terakhir dulu) kalau blok berakhir normal.
    pub(crate) fn lepas_titipan(&mut self, lepas: &[(String, String)]) {
        if lepas.is_empty() || self.terminated { return; }
        self.ensure_extern("declare void @jrg_jembatan_lepas(ptr)", "jrg_jembatan_lepas");
        for (_, slot) in lepas.iter().rev() { self.emit(format!("call void @jrg_jembatan_lepas(ptr {})", slot)); }
    }

    /// `return v` sebagai pernyataan TERAKHIR badan fungsi: pelepasan dipancarkan sebelumnya kalau `v` tidak
    /// menyebut nama yang dilepas (nilainya dihitung sesudah pelepasan).
    pub(crate) fn lepas_sebelum_kembali(&mut self, s: &Stmt, lepas: &mut Vec<(String, String)>) {
        let Stmt::Return(r) = s else { return };
        let disebut: std::collections::HashSet<String> = r.values.iter()
            .flat_map(crate::interpreter::capture::names_mentioned).collect();
        if lepas.iter().any(|(n, _)| disebut.contains(n)) { return; }
        self.lepas_titipan(lepas);
        lepas.clear();
    }
}
