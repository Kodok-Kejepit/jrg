// src/interpreter/capture.rs — interpreter/capture.rs

use std::collections::HashSet;

use crate::parser::{
    Block, Expr, ForStmt, FnBody, IfStmt, MatchBody, SelectCase, Stmt, TemplatePart,
};

/// Semua nama yang disebut di dalam sebuah ekspresi (over-approximation —
/// lihat catatan modul).
pub fn names_mentioned(e: &Expr) -> HashSet<String> {
    let mut out = HashSet::new();
    walk_expr(e, &mut out);
    out.retain(|n| !n.starts_with('\u{0}'));
    out
}

/// Nama tidak sah yang ditandai penelusur saat ia melewati sebuah closure.
const PENANDA_CLOSURE: &str = "\u{0}closure";

/// Nama tidak sah yang ditandai penelusur saat ia melewati sesuatu yang menjalankan kode lain.
const PENANDA_PANGGIL: &str = "\u{0}panggil";

/// Apakah menghitung `e` bisa menjalankan kode milik program (panggilan, method, hook tampilan
/// di templat, titik tunggu channel)? `math::`/`str::` tidak dihitung: tidak memanggil balik.
pub fn ada_panggilan(e: &Expr) -> bool {
    let mut out = HashSet::new();
    walk_expr(e, &mut out);
    out.contains(PENANDA_PANGGIL)
}

/// Apakah `e` memuat sebuah closure, di kedalaman mana pun?
pub fn ada_closure(e: &Expr) -> bool {
    let mut out = HashSet::new();
    walk_expr(e, &mut out);
    out.contains(PENANDA_CLOSURE)
}

/// Semua nama yang disebut di dalam sebuah blok.
pub fn kumpul_nama_blok(b: &Block, out: &mut HashSet<String>) {
    walk_block(b, out);
    out.retain(|n| !n.starts_with('\u{0}'));
}

/// Awalan penanda "nama ini jadi AKAR sebuah sasaran penugasan" (`x = …`, `x.a[0] = …`).
const PENANDA_TULIS: &str = "\u{0}tulis:";

/// Apakah blok ini menulis ke `nama` atau ke dalamnya, di kedalaman mana pun (closure ikut)?
pub fn blok_menulis(b: &Block, nama: &str) -> bool {
    let mut out = HashSet::new();
    walk_block(b, &mut out);
    out.contains(&format!("{}{}", PENANDA_TULIS, nama))
}

fn akar_sasaran(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(n, _) => Some(n.as_str()),
        Expr::Field { obj, .. } | Expr::Index { obj, .. } => akar_sasaran(obj),
        _ => None,
    }
}

const PENANDA_KEMBALI_DIRI: &str = "\u{0}kembali:self";
const PENANDA_KEMBALI_LAIN: &str = "\u{0}kembali:lain";

/// Badan method yang SELALU berakhir dengan mengembalikan `self` itu sendiri (dan menulis ke
/// sana): `x = x.m(..)` atasnya adalah perubahan `x` di tempat.
pub fn blok_mengubah_lalu_kembali_self(b: &Block) -> bool {
    let mut out = HashSet::new();
    walk_block(b, &mut out);
    if out.contains(PENANDA_KEMBALI_LAIN) || out.contains(PENANDA_KEMBALI_TUPEL) || out.contains(PENANDA_CLOSURE) { return false; }
    if !out.contains(&format!("{}{}", PENANDA_TULIS, "self")) { return false; }
    let ekor_diri = matches!(b.tail.as_deref(), Some(Expr::Ident(n, _)) if n == "self");
    let akhir_kembali = b.tail.is_none() && matches!(b.stmts.last(),
        Some(Stmt::Return(r)) if matches!(r.values.as_slice(), [Expr::Ident(n, _)] if n == "self"));
    ekor_diri || akhir_kembali
}

const PENANDA_KEMBALI_TUPEL: &str = "\u{0}kembali:tupel";

/// `return (self, ..)`: tuple yang elemen pertamanya `self` itu sendiri.
fn kembali_tuple_diri(r: &crate::parser::ReturnStmt) -> bool {
    match r.values.as_slice() {
        [Expr::Tuple { elems, .. }] => matches!(elems.first(), Some(Expr::Ident(n, _)) if n == "self"),
        _ => false,
    }
}

/// Badan method yang SELALU berakhir `return (self, ..)`: `x, v = x.m(..)` atasnya mengubah `x`
/// di tempat dan mengambil sisanya — bentuk `pop` yang mengonsumsi.
pub fn blok_kembali_tuple_self(b: &Block) -> bool {
    let mut out = HashSet::new();
    walk_block(b, &mut out);
    if out.contains(PENANDA_KEMBALI_LAIN) || out.contains(PENANDA_KEMBALI_DIRI) || out.contains(PENANDA_CLOSURE) { return false; }
    b.tail.is_none() && matches!(b.stmts.last(), Some(Stmt::Return(r)) if kembali_tuple_diri(r))
}

/// Awalan penanda "nama ini disebut di dalam closure atau `spawn`".
const PENANDA_TANGKAP: &str = "\u{0}tangkap:";

fn tandai_tertangkap(dalam: HashSet<String>, out: &mut HashSet<String>) {
    for n in dalam {
        if !n.starts_with('\u{0}') { out.insert(format!("{}{}", PENANDA_TANGKAP, n)); }
        out.insert(n);
    }
}

/// Per nama: apakah badan ini cuma MEMBACANYA — tidak menulisinya, dan tidak menyebutnya di
/// dalam closure atau `spawn` yang bisa memegangnya lebih lama dari badan ini.
pub fn cuma_dibaca(b: &FnBody, nama: &[&str]) -> Vec<bool> {
    let mut out = HashSet::new();
    walk_fn_body(b, &mut out);
    nama.iter().map(|n| !out.contains(&format!("{}{}", PENANDA_TULIS, n))
        && !out.contains(&format!("{}{}", PENANDA_TANGKAP, n))).collect()
}

/// Sama, untuk satu nama di sebuah blok.
pub fn blok_cuma_membaca(b: &Block, nama: &str) -> bool {
    let mut out = HashSet::new();
    walk_block(b, &mut out);
    !out.contains(&format!("{}{}", PENANDA_TULIS, nama)) && !out.contains(&format!("{}{}", PENANDA_TANGKAP, nama))
}

/// Sama, untuk badan sebuah fungsi.
pub fn blok_ada_closure(b: &Block) -> bool {
    let mut out = HashSet::new();
    walk_block(b, &mut out);
    out.contains(PENANDA_CLOSURE)
}

fn walk_expr(e: &Expr, out: &mut HashSet<String>) {
    match e {
        Expr::Ident(name, _) => { out.insert(name.clone()); }

        // `a::b` selalu diselesaikan lewat globals (namespace bawaan atau
        // modul), tidak pernah lewat scope lokal — jadi tidak ada yang
        // perlu ditangkap.
        Expr::PathExpr { .. } => {}
        Expr::Literal(..) | Expr::Error(_) => {}

        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } | Expr::Propagate { expr, .. } => walk_expr(expr, out),
        Expr::Await { expr, .. } => { out.insert(PENANDA_PANGGIL.to_string()); walk_expr(expr, out) }

        Expr::Binary { lhs, rhs, .. } => { walk_expr(lhs, out); walk_expr(rhs, out); }
        Expr::Pipe { lhs, rhs, .. } => {
            out.insert(PENANDA_PANGGIL.to_string());
            walk_expr(lhs, out); walk_expr(rhs, out);
        }
        Expr::Ternary { cond, then, else_, .. } => {
            walk_expr(cond, out); walk_expr(then, out); walk_expr(else_, out);
        }
        Expr::Field { obj, .. } => walk_expr(obj, out),
        Expr::Method { obj, args, .. } => {
            out.insert(PENANDA_PANGGIL.to_string());
            walk_expr(obj, out);
            for a in args { walk_expr(a, out); }
        }
        Expr::Index { obj, idx, .. } => { walk_expr(obj, out); walk_expr(idx, out); }
        Expr::Slice { obj, lo, hi, .. } => {
            walk_expr(obj, out);
            if let Some(l) = lo { walk_expr(l, out); }
            if let Some(h) = hi { walk_expr(h, out); }
        }
        Expr::Call { func, args, .. } => {
            let murni = matches!(&**func, Expr::PathExpr { segments, .. }
                if matches!(segments.as_slice(), [ns, _] if ns == "math" || ns == "str"));
            if !murni { out.insert(PENANDA_PANGGIL.to_string()); }
            walk_expr(func, out);
            for a in args { walk_expr(a, out); }
        }
        // Closure bersarang: namanya ikut dikumpulkan juga.
        Expr::Closure { body, .. } => {
            out.insert(PENANDA_CLOSURE.to_string());
            let mut dalam = HashSet::new();
            walk_expr(body, &mut dalam);
            tandai_tertangkap(dalam, out);
        }
        Expr::Block(b) => walk_block(b, out),
        Expr::Struct { fields, spread, .. } => {
            for f in fields { walk_expr(&f.value, out); }
            if let Some(s) = spread { walk_expr(s, out); }
        }
        Expr::Array { elems, .. } | Expr::Tuple { elems, .. } => {
            for x in elems { walk_expr(x, out); }
        }
        Expr::ArrayRepeat { elem, count, .. } => { walk_expr(elem, out); walk_expr(count, out); }
        Expr::Range { lo, hi, .. } => {
            if let Some(l) = lo { walk_expr(l, out); }
            if let Some(h) = hi { walk_expr(h, out); }
        }
        Expr::MakeChan { cap, .. } => { if let Some(c) = cap { walk_expr(c, out); } }
        Expr::Send { chan, val, .. } => {
            out.insert(PENANDA_PANGGIL.to_string());
            walk_expr(chan, out); walk_expr(val, out);
        }
        Expr::Recv { chan, .. } => { out.insert(PENANDA_PANGGIL.to_string()); walk_expr(chan, out) }
        Expr::Match { scrutinee, arms, .. } => {
            walk_expr(scrutinee, out);
            for a in arms {
                if let Some(g) = &a.guard { walk_expr(g, out); }
                match &a.body {
                    MatchBody::Expr(x) => walk_expr(x, out),
                    MatchBody::Block(b) => walk_block(b, out),
                }
            }
        }
        Expr::If(s) => walk_if(s, out),
        Expr::Template { parts, .. } => {
            out.insert(PENANDA_PANGGIL.to_string());
            for p in parts {
                if let TemplatePart::Expr(x) = p { walk_expr(x, out); }
            }
        }
    }
}

fn walk_block(b: &Block, out: &mut HashSet<String>) {
    for s in &b.stmts { walk_stmt(s, out); }
    if let Some(t) = &b.tail { walk_expr(t, out); }
}

fn walk_if(s: &IfStmt, out: &mut HashSet<String>) {
    if let Some(init) = &s.init { walk_stmt(init, out); }
    walk_expr(&s.cond, out);
    walk_block(&s.then, out);
    for (init, cond, blk) in &s.else_ifs {
        if let Some(i) = init { walk_stmt(i, out); }
        walk_expr(cond, out);
        walk_block(blk, out);
    }
    if let Some(b) = &s.else_ { walk_block(b, out); }
}

fn walk_fn_body(b: &FnBody, out: &mut HashSet<String>) {
    match b {
        FnBody::Block(bl) => walk_block(bl, out),
        FnBody::Expr(e) => walk_expr(e, out),
        // Tanda tangan abstrak tidak punya badan, jadi tidak menyebut nama apa pun —
        // tidak ada yang bisa ditangkap.
        FnBody::Abstrak => {}
    }
}

fn walk_stmt(s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::Let(l) => walk_expr(&l.init, out),
        Stmt::Var(v) => { if let Some(x) = &v.init { walk_expr(x, out); } }
        Stmt::Const(c) => walk_expr(&c.value, out),
        Stmt::Expr(e) => walk_expr(e, out),
        Stmt::Assign(a) => {
            for t in &a.targets {
                if let Some(n) = akar_sasaran(t) { out.insert(format!("{}{}", PENANDA_TULIS, n)); }
                walk_expr(t, out);
            }
            for v in &a.values { walk_expr(v, out); }
        }
        Stmt::ShortDecl(d) => { for v in &d.values { walk_expr(v, out); } }
        Stmt::If(i) => walk_if(i, out),
        Stmt::For(f) => match f {
            ForStmt::Classic { init, cond, post, body, .. } => {
                if let Some(i) = init { walk_stmt(i, out); }
                if let Some(c) = cond { walk_expr(c, out); }
                if let Some(p) = post { walk_stmt(p, out); }
                walk_block(body, out);
            }
            ForStmt::Range { iter, body, .. } => { walk_expr(iter, out); walk_block(body, out); }
        },
        Stmt::While(w) => { walk_expr(&w.cond, out); walk_block(&w.body, out); }
        Stmt::Loop(l) => walk_block(&l.body, out),
        Stmt::Match(m) => {
            walk_expr(&m.scrutinee, out);
            for a in &m.arms {
                if let Some(g) = &a.guard { walk_expr(g, out); }
                match &a.body {
                    MatchBody::Expr(x) => walk_expr(x, out),
                    MatchBody::Block(b) => walk_block(b, out),
                }
            }
        }
        Stmt::Return(r) => {
            let diri = matches!(r.values.as_slice(), [Expr::Ident(n, _)] if n == "self");
            out.insert(if diri { PENANDA_KEMBALI_DIRI } else if kembali_tuple_diri(r) { PENANDA_KEMBALI_TUPEL }
                       else { PENANDA_KEMBALI_LAIN }.to_string());
            for v in &r.values { walk_expr(v, out); }
        }
        Stmt::Defer(d) => walk_expr(&d.expr, out),
        Stmt::Spawn(sp) => {
            let mut dalam = HashSet::new();
            walk_block(&sp.body, &mut dalam);
            tandai_tertangkap(dalam, out);
        }
        // Deklarasi `fn` bersarang: badannya ikut dijelajahi supaya nama
        // dari scope luar yang dipakainya juga ikut tertangkap.
        Stmt::Fn(fd) => walk_fn_body(&fd.body, out),
        Stmt::Select(sel) => {
            for c in &sel.cases {
                match c {
                    SelectCase::Send { chan, val, body, .. } => {
                        walk_expr(chan, out); walk_expr(val, out); walk_block(body, out);
                    }
                    SelectCase::Recv { chan, body, .. } => {
                        walk_expr(chan, out); walk_block(body, out);
                    }
                    SelectCase::Let { stmt, body, .. } => {
                        walk_expr(&stmt.init, out); walk_block(body, out);
                    }
                }
            }
            if let Some(d) = &sel.default { walk_block(d, out); }
        }
        Stmt::Block(b) => walk_block(b, out),
        // Sisanya tidak memuat ekspresi yang bisa merujuk variabel lokal.
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::Error(_) => {}
    }
}

/// Operan kanan rantai `nama + a + b + …` (urut tulis), kalau daun paling kirinya `nama` —
/// bentuk yang ditumbuhkan di tempat oleh kedua mesin. `None` untuk bentuk lain.
pub fn rantai_tambah<'e>(e: &'e Expr, nama: &str) -> Option<Vec<&'e Expr>> {
    let mut operan = Vec::new();
    let mut kini = e;
    loop {
        match kini {
            Expr::Binary { op: crate::parser::BinaryOp::Add, lhs, rhs, .. } => {
                operan.push(&**rhs);
                kini = lhs;
            }
            Expr::Ident(n, _) if n == nama && !operan.is_empty() => {
                operan.reverse();
                return Some(operan);
            }
            _ => return None,
        }
    }
}
