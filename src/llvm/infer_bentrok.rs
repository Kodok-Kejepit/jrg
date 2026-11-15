// src/llvm/infer_bentrok.rs — deteksi tipe yang bentrok, dan tipe argumen
// pemanggilan

use std::collections::HashMap;

use crate::parser::*;

use super::codegen::Codegen;
use super::infer::*;
use super::sema::TypeSubst;
use super::types::JrgType;

impl Codegen {
    /// Tipe sebuah fungsi bernama yang dipakai sebagai nilai.
    pub(crate) fn tipe_fungsi_bernama(&self, nama: &str, subst: &TypeSubst) -> Option<JrgType> {
        let decl = self.tables.functions.get(&self.k(nama))?;
        let mut ps = Vec::with_capacity(decl.params.len());
        for p in &decl.params {
            ps.push(self.tipe_dari_anotasi(&p.ty, subst)?);
        }
        let ret = match &decl.ret_type {
            Some(t) => self.tipe_dari_anotasi(t, subst)?,
            None => JrgType::Unit,
        };
        Some(JrgType::Fn(ps, Box::new(ret)))
    }
}

/// Tipe argumen pada pemanggilan pertama tiap nama, dalam sebuah blok.
pub fn skalar_global_bentrok(cg: &Codegen, nama: &str, awal: &JrgType) -> bool {
    // Koleksi punya jalurnya sendiri (`tipe_elemen_var_global`), dan mencampur
    // keduanya di sini berarti dua aturan di satu tempat.
    let skalar = |t: &JrgType| matches!(t, JrgType::Bool | JrgType::Str | JrgType::Rune
        | JrgType::Nil) || t.is_numeric();
    if !skalar(awal) { return false; }

    let subst = TypeSubst::new();
    let mut terlihat: Vec<JrgType> = vec![awal.clone()];

    pub(crate) fn di_blok(
        cg: &Codegen, b: &crate::parser::Block, nama: &str, subst: &TypeSubst,
        terlihat: &mut Vec<JrgType>,
    ) {
        use crate::parser::{Expr, ForStmt, MatchBody, Stmt};
        let lokal = Lokal::new();
        for st in &b.stmts {
            match st {
                Stmt::Assign(a) => {
                    for (t, val) in a.targets.iter().zip(a.values.iter()) {
                        let Expr::Ident(n, _) = t else { continue };
                        if n != nama { continue; }
                        if let Some(ty) = cg.tipe_ekspresi(val, &lokal, subst, "") {
                            if !terlihat.contains(&ty) { terlihat.push(ty); }
                        }
                    }
                }
                Stmt::Block(bb) => di_blok(cg, bb, nama, subst, terlihat),
                Stmt::If(i) => {
                    di_blok(cg, &i.then, nama, subst, terlihat);
                    for (_, _, bb) in &i.else_ifs { di_blok(cg, bb, nama, subst, terlihat); }
                    if let Some(e) = &i.else_ { di_blok(cg, e, nama, subst, terlihat); }
                }
                Stmt::While(w) => di_blok(cg, &w.body, nama, subst, terlihat),
                Stmt::Loop(l) => di_blok(cg, &l.body, nama, subst, terlihat),
                Stmt::For(f) => match f {
                    ForStmt::Classic { body, .. } | ForStmt::Range { body, .. } =>
                        di_blok(cg, body, nama, subst, terlihat),
                },
                // `if`/`match` di posisi pernyataan diurai jadi `Stmt::Expr`.
                Stmt::Expr(e) => match &**e {
                    Expr::If(i) => {
                        di_blok(cg, &i.then, nama, subst, terlihat);
                        for (_, _, bb) in &i.else_ifs { di_blok(cg, bb, nama, subst, terlihat); }
                        if let Some(x) = &i.else_ { di_blok(cg, x, nama, subst, terlihat); }
                    }
                    Expr::Block(bb) => di_blok(cg, bb, nama, subst, terlihat),
                    Expr::Match { arms, .. } => {
                        for a in arms {
                            if let MatchBody::Block(bb) = &a.body {
                                di_blok(cg, bb, nama, subst, terlihat);
                            }
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    let mut fs: Vec<_> = cg.tables.functions.values().cloned().collect();
    for m in cg.tables.impls.values() { fs.extend(m.values().cloned()); }
    fs.sort_by(|a, b| a.name.cmp(&b.name));
    for f in &fs {
        if let FnBody::Block(b) = &*f.body {
            di_blok(cg, b, nama, &subst, &mut terlihat);
        }
    }
    terlihat.len() > 1
}

/// Lokal skalar yang tipenya BERGANTI di dalam satu badan fungsi.
pub fn lokal_skalar_bentrok(
    cg: &Codegen, b: &crate::parser::Block, subst: &TypeSubst, awal: &Lokal,
) -> std::collections::HashSet<String> {
    let mut tipe: HashMap<String, Vec<JrgType>> = HashMap::new();
    let mut lahir: Vec<String> = Vec::new();

    pub(crate) fn jelajah(
        cg: &Codegen, b: &crate::parser::Block, lokal: &mut Lokal, subst: &TypeSubst,
        lahir: &mut Vec<String>, tipe: &mut HashMap<String, Vec<JrgType>>,
    ) {
        use crate::parser::{Expr, ForStmt, Stmt};
        fn catat(
            tipe: &mut HashMap<String, Vec<JrgType>>, n: &str, t: Option<JrgType>,
        ) {
            let Some(t) = t else { return };
            let e = tipe.entry(n.to_string()).or_default();
            if !e.contains(&t) { e.push(t); }
        }
        fn tipe_di_lokal(
            cg: &Codegen, e: &Expr, lokal: &Lokal, subst: &TypeSubst, harapan: Option<&JrgType>,
        ) -> Option<JrgType> {
            if let Some(t) = cg.tipe_ekspresi(e, lokal, subst, "") { return Some(t); }
            let Expr::Call { func, args, .. } = e else { return None };
            if args.len() != 1 { return None; }
            let ctor = match &**func {
                Expr::Ident(n, _) => n.as_str(),
                Expr::PathExpr { segments, .. } if segments.len() == 2
                    && matches!(segments[0].as_str(), "Result" | "Option") => segments[1].as_str(),
                _ => return None,
            };
            if !matches!(ctor, "Ok" | "Err" | "Some") { return None; }
            let isi = tipe_di_lokal(cg, &args[0], lokal, subst, None)?;
            // `Err(..)` ke `Result<T, any>` (dan `Ok(..)` ke `Result<any, E>`) muat tanpa bentrok.
            if let Some(JrgType::Enum(h)) = harapan {
                if (ctor == "Ok" && h.starts_with("Result$any$"))
                    || (ctor == "Err" && h.starts_with("Result$") && h.ends_with("$any"))
                {
                    return Some(JrgType::Enum(h.clone()));
                }
            }
            let m = match ctor {
                "Some" => super::mangle::mangle_option(&isi),
                "Ok" => super::mangle::mangle_result(&isi, &JrgType::Any),
                _ => super::mangle::mangle_result(&JrgType::Any, &isi),
            };
            Some(JrgType::Enum(m))
        }
        for st in &b.stmts {
            match st {
                Stmt::Var(v) if v.ty.is_none() => {
                    if let Some(init) = &v.init {
                        let t = tipe_di_lokal(cg, init, lokal, subst, None);
                        // Koleksi ikut hanya kalau ia berganti GOLONGAN (array jadi dict); tumbuhnya
                        // tetap urusan `lokal_yang_tumbuh`, supaya satu persoalan punya satu aturan.
                        if matches!(t, Some(JrgType::Bool) | Some(JrgType::Str)
                            | Some(JrgType::Rune) | Some(JrgType::Nil) | Some(JrgType::Array(..))
                            | Some(JrgType::Slice(_)) | Some(JrgType::Dict(_)) | Some(JrgType::DictCampur(_)))
                            || matches!(&t, Some(x) if x.is_numeric())
                            || matches!(&t, Some(JrgType::Enum(n)) if n.contains('$'))
                        {
                            lahir.push(v.name.clone());
                            catat(tipe, &v.name, t.clone());
                            if let Some(t) = t { lokal.insert(v.name.clone(), t); }
                        }
                    }
                }
                Stmt::Var(v) => {
                    if let Some(t) = v.ty.as_ref().and_then(|te| cg.tipe_dari_anotasi(te, subst)) {
                        lokal.insert(v.name.clone(), t);
                    }
                }
                Stmt::Let(l) => {
                    if let Some(n) = pola_nama(&l.pattern) {
                        if let Some(t) = l.ty.as_ref().and_then(|te| cg.tipe_dari_anotasi(te, subst))
                            .or_else(|| tipe_di_lokal(cg, &l.init, lokal, subst, None))
                        {
                            lokal.insert(n, t);
                        }
                    }
                }
                Stmt::Assign(a) => {
                    for (t, val) in a.targets.iter().zip(a.values.iter()) {
                        let Expr::Ident(n, _) = t else { continue };
                        if !lahir.contains(n) { continue; }
                        let harapan = lokal.get(n);
                        catat(tipe, n, tipe_di_lokal(cg, val, lokal, subst, harapan));
                    }
                }
                Stmt::Block(bb) => jelajah(cg, bb, lokal, subst, lahir, tipe),
                Stmt::If(i) => {
                    jelajah(cg, &i.then, lokal, subst, lahir, tipe);
                    for (_, _, bb) in &i.else_ifs { jelajah(cg, bb, lokal, subst, lahir, tipe); }
                    if let Some(e) = &i.else_ { jelajah(cg, e, lokal, subst, lahir, tipe); }
                }
                Stmt::While(w) => jelajah(cg, &w.body, lokal, subst, lahir, tipe),
                Stmt::Loop(l) => jelajah(cg, &l.body, lokal, subst, lahir, tipe),
                Stmt::For(f) => match f {
                    ForStmt::Classic { body, .. } => jelajah(cg, body, lokal, subst, lahir, tipe),
                    ForStmt::Range { vars, iter, body, .. } => {
                        cg.ikat_var_loop(vars, iter, lokal, subst, "");
                        jelajah(cg, body, lokal, subst, lahir, tipe)
                    }
                },
                // `if` di posisi pernyataan diurai jadi `Stmt::Expr(Expr::If)`, BUKAN
                // `Stmt::If` — yang terakhir dipakai bentuk lain.
                Stmt::Expr(e) => di_ekspr(cg, e, lokal, subst, lahir, tipe),
                _ => {}
            }
        }
        // `if .. else ..` di ujung blok diurai jadi EKOR blok, bukan pernyataan.
        if let Some(t) = &b.tail { di_ekspr(cg, t, lokal, subst, lahir, tipe); }
    }
    pub(crate) fn di_ekspr(
        cg: &Codegen, e: &crate::parser::Expr, lokal: &mut Lokal, subst: &TypeSubst,
        lahir: &mut Vec<String>, tipe: &mut HashMap<String, Vec<JrgType>>,
    ) {
        use crate::parser::{Expr, MatchBody};
        match e {
            Expr::If(i) => {
                jelajah(cg, &i.then, lokal, subst, lahir, tipe);
                for (_, _, bb) in &i.else_ifs { jelajah(cg, bb, lokal, subst, lahir, tipe); }
                if let Some(x) = &i.else_ { jelajah(cg, x, lokal, subst, lahir, tipe); }
            }
            Expr::Block(bb) => jelajah(cg, bb, lokal, subst, lahir, tipe),
            Expr::Match { arms, .. } => {
                for a in arms {
                    match &a.body {
                        MatchBody::Block(bb) => jelajah(cg, bb, lokal, subst, lahir, tipe),
                        MatchBody::Expr(x) => di_ekspr(cg, x, lokal, subst, lahir, tipe),
                    }
                }
            }
            _ => {}
        }
    }

    // Parameter ikut dikenal: `var v = data` (parameter) lalu `v = v[k]` juga pergantian jenis.
    let mut lokal = awal.clone();
    jelajah(cg, b, &mut lokal, subst, &mut lahir, &mut tipe);

    let skalar = |t: &JrgType| !matches!(t, JrgType::Array(..) | JrgType::Slice(_)
        | JrgType::Dict(_) | JrgType::DictCampur(_));
    // Separuh `Option`/`Result` (`None`, `Err(e)` tanpa sisi lain) bukan jenis tersendiri: ia
    // mengikuti instans yang lengkap. Dua instans lengkap yang berbeda (`Some(1)` lalu `Some(o)`) bentrok.
    let separuh = |t: &JrgType| matches!(t, JrgType::Enum(n) if n.contains('$') && t.mangle_frag().contains("never"));
    for v in tipe.values_mut() {
        if v.iter().any(|t| !separuh(t)) { v.retain(|t| !separuh(t)); }
    }
    tipe.into_iter()
        .filter(|(_, v)| v.len() > 1 && (v.iter().all(skalar)
            || v.iter().fold(JrgType::Never, |a, t| super::codegen_any::satukan_jenis(&a, t)) == JrgType::Any))
        .map(|(k, _)| k)
        .collect()
}

/// Lokal yang lahir sebagai dict atau array bertipe lalu ditulis nilai berjenis lain (`d["b"] = "x"` atas
/// `{"a": 1}`, `a[1] = "q"` atas `[1, 2]`): slotnya bernilai bertag sejak awal, seperti dua mesin acuan.
pub fn dict_ditulis_jenis_lain(cg: &Codegen, b: &crate::parser::Block, subst: &TypeSubst) -> Vec<(String, JrgType)> {
    let (mut lahir, mut tulis) = (Vec::new(), Vec::new());
    tulisan_dict(b, &mut lahir, &mut tulis);
    let lokal = Lokal::new();
    let mut hasil: Vec<(String, JrgType)> = Vec::new();
    for (n, init) in &lahir {
        if hasil.iter().any(|(h, _)| h == n) { continue; }
        let Some(awal @ (JrgType::Dict(_) | JrgType::Array(..) | JrgType::Slice(_)))
            = cg.tipe_ekspresi(init, &lokal, subst, "") else { continue };
        let akhir = tulis.iter().filter(|(t, _, _)| t == n).fold(awal.clone(), |ty, (_, d, v)|
            cg.tipe_ekspresi(v, &lokal, subst, "").and_then(|vt| isi_berganti(&ty, *d, &vt)).unwrap_or(ty));
        if akhir != awal { hasil.push((n.clone(), akhir)); }
    }
    hasil
}

/// `var` modul bertipe dict atau array yang ditulisi nilai berjenis lain dari fungsi mana pun.
pub fn dict_global_ditulis(cg: &Codegen, nama: &str, awal: &JrgType) -> Option<JrgType> {
    if !matches!(awal, JrgType::Dict(_) | JrgType::Array(..) | JrgType::Slice(_)) { return None; }
    let mut fs: Vec<_> = cg.tables.functions.values().cloned().collect();
    for m in cg.tables.impls.values() { fs.extend(m.values().cloned()); }
    fs.sort_by(|a, b| a.name.cmp(&b.name));
    let (subst, lokal) = (TypeSubst::new(), Lokal::new());
    let mut ty = awal.clone();
    for f in &fs {
        let FnBody::Block(b) = &*f.body else { continue };
        let (mut lahir, mut tulis) = (Vec::new(), Vec::new());
        tulisan_dict(b, &mut lahir, &mut tulis);
        // Lokal bernama sama membayangi `var` modulnya.
        if lahir.iter().any(|(n, _)| n == nama) { continue; }
        for (_, d, v) in tulis.iter().filter(|(t, _, _)| t == nama) {
            if let Some(t) = cg.tipe_ekspresi(v, &lokal, &subst, "").and_then(|vt| isi_berganti(&ty, *d, &vt)) { ty = t; }
        }
    }
    (ty != *awal).then_some(ty)
}

/// Tipe dict/array `t` sesudah isi di kedalaman `d` ditulisi nilai bertipe `v`, kalau jenisnya berganti.
fn isi_berganti(t: &JrgType, d: usize, v: &JrgType) -> Option<JrgType> {
    let (isi, bungkus): (&JrgType, fn(JrgType, &JrgType) -> JrgType) = match t {
        JrgType::Dict(i) => (i, |x, _| JrgType::Dict(Box::new(x))),
        // Array jadi slice: `var` bisa tumbuh sesudahnya (`e = arr::push(e, x)`).
        JrgType::Array(i, _) | JrgType::Slice(i) => (i, |x, _| JrgType::Slice(Box::new(x))),
        _ => return None,
    };
    if d <= 1 { return super::codegen_any::ganti_jenis_isi(isi, v).then(|| bungkus(JrgType::Any, t)); }
    isi_berganti(isi, d - 1, v).map(|x| bungkus(x, t))
}

/// Lokal yang lahir tanpa anotasi beserta nilai awalnya, dan tulisan `akar[..]..[..] = v`/`akar.f = v`
/// dengan kedalaman jalurnya.
fn tulisan_dict(b: &crate::parser::Block, lahir: &mut Vec<(String, crate::parser::Expr)>,
                tulis: &mut Vec<(String, usize, crate::parser::Expr)>) {
    use crate::parser::{Expr, MatchBody, Stmt};
    fn akar(e: &Expr, d: usize) -> Option<(String, usize)> {
        match e {
            Expr::Ident(n, _) if d > 0 => Some((n.clone(), d)),
            Expr::Index { obj, .. } | Expr::Field { obj, .. } => akar(obj, d + 1),
            _ => None,
        }
    }
    for s in &b.stmts {
        match s {
            Stmt::Let(l) => if let Some(n) = super::codegen::pattern_simple_name(&l.pattern) {
                if l.ty.is_none() { lahir.push((n, (*l.init).clone())); }
            },
            Stmt::Var(v) => if let (Some(init), None) = (&v.init, &v.ty) { lahir.push((v.name.clone(), (**init).clone())); },
            Stmt::Assign(a) => for (t, val) in a.targets.iter().zip(a.values.iter()) {
                if let Some((n, d)) = akar(t, 0) { tulis.push((n, d, val.clone())); }
                // `d = dict::set(d, k, v)` menulis `v` ke isi `d`, sama dengan `d[k] = v`.
                if let (Expr::Ident(n, _), Expr::Call { func, args, .. }) = (t, val) {
                    let set = matches!(&**func, Expr::PathExpr { segments, .. }
                        if segments.len() == 2 && segments[0] == "dict" && segments[1] == "set");
                    if set && args.len() == 3 && matches!(&args[0], Expr::Ident(m, _) if m == n) {
                        tulis.push((n.clone(), 1, args[2].clone()));
                    }
                }
            },
            Stmt::Block(bb) => tulisan_dict(bb, lahir, tulis),
            Stmt::While(w) => tulisan_dict(&w.body, lahir, tulis),
            Stmt::Loop(l) => tulisan_dict(&l.body, lahir, tulis),
            Stmt::For(crate::parser::ForStmt::Classic { body, .. })
            | Stmt::For(crate::parser::ForStmt::Range { body, .. }) => tulisan_dict(body, lahir, tulis),
            Stmt::If(i) => {
                tulisan_dict(&i.then, lahir, tulis);
                for (_, _, bb) in &i.else_ifs { tulisan_dict(bb, lahir, tulis); }
                if let Some(bb) = &i.else_ { tulisan_dict(bb, lahir, tulis); }
            }
            Stmt::Expr(e) => match &**e {
                Expr::If(i) => {
                    tulisan_dict(&i.then, lahir, tulis);
                    for (_, _, bb) in &i.else_ifs { tulisan_dict(bb, lahir, tulis); }
                    if let Some(x) = &i.else_ { tulisan_dict(x, lahir, tulis); }
                }
                Expr::Block(bb) => tulisan_dict(bb, lahir, tulis),
                Expr::Match { arms, .. } => for a in arms {
                    if let MatchBody::Block(bb) = &a.body { tulisan_dict(bb, lahir, tulis); }
                },
                _ => {}
            },
            _ => {}
        }
    }
}

/// Lokal yang lahir sebagai `dict::new()` lalu diisi lewat `d[k] = v`.
pub fn dict_kosong_yang_diisi(
    cg: &Codegen, b: &crate::parser::Block, subst: &TypeSubst,
) -> HashMap<String, JrgType> {
    use crate::parser::Expr;
    let mut lahir: Vec<String> = Vec::new();
    // Tipe tiap tulisan, dibaca dengan lokal yang terlihat di titik itu; `None` = tak terbaca.
    let mut tulis: Vec<(String, Option<JrgType>)> = Vec::new();
    let mut lokal = Lokal::new();

    fn dict_new(e: &Expr) -> bool {
        matches!(e, Expr::PathExpr { segments, .. }
            if segments.len() == 2 && segments[0] == "dict" && segments[1] == "new")
            || matches!(e, Expr::Call { func, args, .. } if args.is_empty()
                && matches!(&**func, Expr::PathExpr { segments, .. }
                    if segments.len() == 2 && segments[0] == "dict" && segments[1] == "new"))
    }
    fn tipe_tulis(cg: &Codegen, v: &Expr, lokal: &Lokal, subst: &TypeSubst) -> Option<JrgType> {
        match v {
            Expr::Array { elems, .. } if elems.is_empty() => Some(JrgType::Slice(Box::new(JrgType::Any))),
            _ => cg.tipe_ekspresi(v, lokal, subst, ""),
        }
    }
    pub(crate) fn jelajah(cg: &Codegen, b: &crate::parser::Block, subst: &TypeSubst, lokal: &mut Lokal,
                          lahir: &mut Vec<String>, tulis: &mut Vec<(String, Option<JrgType>)>) {
        // Lokal yang lahir dari `d[..]` atas dict yang sedang dihitung: menulisnya balik ke `d` tidak membawa
        // jenis baru (`var isi = h[b]; isi[k] = v; h[b] = isi`). Dicatat sebagai lokal bertanda `\u{0}dari:d`.
        fn asal(lokal: &Lokal, x: &Expr) -> Option<String> {
            let Expr::Ident(n, _) = x else { return None };
            match lokal.get(&format!("\u{0}dari:{}", n)) { Some(JrgType::Struct(d)) => Some(d.clone()), _ => None }
        }
        use crate::parser::{Expr, Stmt};
        for s in &b.stmts {
            match s {
                Stmt::Let(l) => {
                    let n = super::codegen::pattern_simple_name(&l.pattern);
                    if dict_new(&l.init) { if let Some(n) = &n { lahir.push(n.clone()); } }
                    if let (Some(n), Expr::Index { obj, .. }) = (&n, &*l.init) {
                        if let Expr::Ident(d, _) = &**obj { if lahir.contains(d) {
                            lokal.insert(format!("\u{0}dari:{}", n), JrgType::Struct(d.clone()));
                        } }
                    }
                    if let (Some(n), Some(t)) = (n, cg.tipe_ekspresi(&l.init, lokal, subst, "")) { lokal.insert(n, t); }
                }
                Stmt::Var(v) => {
                    if let Some(init) = &v.init {
                        if dict_new(init) { lahir.push(v.name.clone()); }
                        if let Expr::Index { obj, .. } = &**init {
                            if let Expr::Ident(d, _) = &**obj { if lahir.contains(d) {
                                lokal.insert(format!("\u{0}dari:{}", v.name), JrgType::Struct(d.clone()));
                            } }
                        }
                        if let Some(t) = cg.tipe_ekspresi(init, lokal, subst, "") { lokal.insert(v.name.clone(), t); }
                    }
                }
                Stmt::Assign(a) => {
                    for (t, val) in a.targets.iter().zip(a.values.iter()) {
                        if let Expr::Index { obj, .. } = t {
                            if let Expr::Ident(n, _) = &**obj {
                                if asal(lokal, val).as_deref() == Some(n.as_str()) { continue; }
                                tulis.push((n.clone(), tipe_tulis(cg, val, lokal, subst)));
                            }
                        }
                        // `d = dict::set(d, k, v)` menulis `v` ke `d`.
                        if let (Expr::Ident(n, _), Expr::Call { func, args, .. }) = (t, val) {
                            let set = matches!(&**func, Expr::PathExpr { segments, .. }
                                if matches!(tanpa_awalan_bawaan(segments), [x, y] if x == "dict" && y == "set"));
                            if set && args.len() == 3 && matches!(&args[0], Expr::Ident(m, _) if m == n) {
                                tulis.push((n.clone(), tipe_tulis(cg, &args[2], lokal, subst)));
                            }
                        }
                    }
                }
                Stmt::Block(bb) => jelajah(cg, bb, subst, lokal, lahir, tulis),
                Stmt::While(w) => jelajah(cg, &w.body, subst, lokal, lahir, tulis),
                Stmt::Loop(l) => jelajah(cg, &l.body, subst, lokal, lahir, tulis),
                Stmt::For(crate::parser::ForStmt::Classic { body, .. })
                | Stmt::For(crate::parser::ForStmt::Range { body, .. }) => jelajah(cg, body, subst, lokal, lahir, tulis),
                // Tulisan di cabang `if` (pernyataan `if` diurai jadi `Stmt::Expr(Expr::If)`).
                Stmt::If(i) => if_cabang(cg, i, subst, lokal, lahir, tulis),
                Stmt::Expr(e) => if let Expr::If(i) = &**e { if_cabang(cg, i, subst, lokal, lahir, tulis) },
                _ => {}
            }
        }
        // `if .. else ..` di ujung blok diurai jadi EKOR blok, bukan pernyataan.
        if let Some(Expr::If(i)) = b.tail.as_deref() { if_cabang(cg, i, subst, lokal, lahir, tulis) }
    }
    fn if_cabang(cg: &Codegen, i: &crate::parser::IfStmt, subst: &TypeSubst, lokal: &mut Lokal,
                 lahir: &mut Vec<String>, tulis: &mut Vec<(String, Option<JrgType>)>) {
        jelajah(cg, &i.then, subst, lokal, lahir, tulis);
        for (_, _, bb) in &i.else_ifs { jelajah(cg, bb, subst, lokal, lahir, tulis); }
        if let Some(e) = &i.else_ { jelajah(cg, e, subst, lokal, lahir, tulis); }
    }
    jelajah(cg, b, subst, &mut lokal, &mut lahir, &mut tulis);

    // Semua tulisan disatukan (`h["a"] = false` lalu `h["b"] = []`: bernilai bertag). Satu tulisan yang
    // tipenya tak terbaca membuat dict-nya tetap bernilai bertag — tipenya tidak ditebak dari tulisan lain.
    let mut hasil = HashMap::new();
    for n in lahir {
        let mut isi: Option<JrgType> = None;
        let mut buta = false;
        for (t, v) in &tulis {
            if *t != n { continue; }
            match v {
                None => buta = true,
                Some(v) => isi = Some(match isi { None => v.clone(), Some(lama) => super::codegen_any::satukan_jenis(&lama, v) }),
            }
        }
        if buta { continue; }
        if let Some(v) = isi { hasil.insert(n.clone(), JrgType::Dict(Box::new(v))); }
    }
    hasil
}

/// Menaruh rantai tipe argumen closure-berantai ke dalam `subst`.
pub(crate) fn semai_rantai(subst: &mut TypeSubst, rantai: &[Vec<JrgType>]) {
    for (tingkat, ts) in rantai.iter().enumerate() {
        for (i, t) in ts.iter().enumerate() {
            subst.insert(format!("$rantai{}param{}", tingkat, i), t.clone());
        }
    }
}

/// `subst` untuk SATU tingkat lebih dalam: tingkat 1 jadi 0, 2 jadi 1, dan
/// tingkat 0 dibuang karena baru saja dipakai.
pub(crate) fn geser_rantai(subst: &TypeSubst) -> TypeSubst {
    let mut baru = TypeSubst::new();
    for (k, v) in subst {
        match k.strip_prefix("$rantai") {
            None => { baru.insert(k.clone(), v.clone()); }
            Some(sisa) => {
                if let Some((tingkat, ekor)) = sisa.split_once("param") {
                    if let Ok(n) = tingkat.parse::<usize>() {
                        if n > 0 {
                            baru.insert(format!("$rantai{}param{}", n - 1, ekor), v.clone());
                        }
                    }
                }
            }
        }
    }
    baru
}

/// Nama dasar sebuah callee, dan BERAPA lapis panggilan membungkusnya.
pub(crate) fn nama_dasar_panggilan(callee: &crate::parser::Expr) -> Option<(&str, usize)> {
    use crate::parser::Expr;
    match callee {
        Expr::Ident(n, _) => Some((n.as_str(), 0)),
        Expr::Call { func, .. } => nama_dasar_panggilan(func).map(|(n, d)| (n, d + 1)),
        _ => None,
    }
}

/// Hasil satu kali penjelajahan situs panggilan.
pub(crate) fn paruh_konstruktor(
    cg: &Codegen, e: &crate::parser::Expr, lokal: &Lokal, subst: &TypeSubst,
) -> Option<(Option<JrgType>, Option<JrgType>)> {
    use crate::parser::Expr;
    let Expr::Call { func, args, .. } = e else { return None };
    let Expr::Ident(n, _) = &**func else { return None };
    if args.len() != 1 { return None; }
    let t = cg.tipe_ekspresi(&args[0], lokal, subst, "");
    match n.as_str() {
        "Ok" | "Some" => Some((t, None)),
        "Err" => Some((None, t)),
        _ => None,
    }
}

pub(crate) struct Kumpulan {
    arg: HashMap<String, Vec<JrgType>>,
    paruh: PetaParuh,
}

pub fn tipe_argumen_pemanggilan(
    cg: &Codegen, b: &crate::parser::Block, lokal: &Lokal, subst: &TypeSubst,
) -> HashMap<String, Vec<JrgType>> {
    tipe_argumen_dan_paruh(cg, b, lokal, subst).0
}

/// Separuh tipe `Result`/`Option` yang terbaca dari sebuah argumen.
pub type PetaParuh = HashMap<(String, usize), (Option<JrgType>, Option<JrgType>)>;

/// Sama dengan [`tipe_argumen_pemanggilan`], plus paruh `Result`/`Option`.
pub fn tipe_argumen_dan_paruh(
    cg: &Codegen, b: &crate::parser::Block, lokal: &Lokal, subst: &TypeSubst,
) -> (HashMap<String, Vec<JrgType>>, PetaParuh) {
    use crate::parser::{Expr, ForStmt, MatchBody, Stmt};
    let mut out = Kumpulan { arg: HashMap::new(), paruh: HashMap::new() };

    pub(crate) fn di_ekspr(
        cg: &Codegen, e: &Expr, lokal: &Lokal, subst: &TypeSubst,
        out: &mut Kumpulan,
    ) {
        match e {
            Expr::Call { func, args, .. } => {
                // Kunci untuk panggilan BERANTAI: `luar(3)(4)`.
                if let Some((n, tingkat)) = nama_dasar_panggilan(func) {
                    let kunci = if tingkat == 0 {
                        n.to_string()
                    } else {
                        format!("{}{}", n, "$hasil".repeat(tingkat))
                    };
                    if !out.arg.contains_key(&kunci) {
                        let mut ts = Vec::with_capacity(args.len());
                        let mut lengkap = true;
                        for a in args {
                            match cg.tipe_ekspresi(a, lokal, subst, "") {
                                Some(t) => ts.push(t),
                                None => { lengkap = false; break; }
                            }
                        }
                        if lengkap { out.arg.insert(kunci, ts); }
                    }
                    // Paruh `Result`/`Option` per posisi argumen, dikumpulkan dari SEMUA situs —
                    // bukan cuma yang pertama seperti peta di atas.
                    if tingkat == 0 {
                        for (i, a) in args.iter().enumerate() {
                            if let Some((t, e)) = paruh_konstruktor(cg, a, lokal, subst) {
                                let slot = out.paruh
                                    .entry((n.to_string(), i))
                                    .or_insert((None, None));
                                if slot.0.is_none() { slot.0 = t; }
                                if slot.1.is_none() { slot.1 = e; }
                            }
                        }
                    }
                }
                for a in args { di_ekspr(cg, a, lokal, subst, out); }
                di_ekspr(cg, func, lokal, subst, out);
            }
            Expr::Binary { lhs, rhs, .. } => {
                di_ekspr(cg, lhs, lokal, subst, out);
                di_ekspr(cg, rhs, lokal, subst, out);
            }
            Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => di_ekspr(cg, expr, lokal, subst, out),
            Expr::Method { obj, args, .. } => {
                di_ekspr(cg, obj, lokal, subst, out);
                for a in args { di_ekspr(cg, a, lokal, subst, out); }
            }
            Expr::Index { obj, idx, .. } => {
                di_ekspr(cg, obj, lokal, subst, out);
                di_ekspr(cg, idx, lokal, subst, out);
            }
            Expr::Array { elems, .. } | Expr::Tuple { elems, .. } => {
                for x in elems { di_ekspr(cg, x, lokal, subst, out); }
            }
            Expr::Block(b) => di_blok(cg, b, lokal, subst, out),
            Expr::If(i) => {
                di_ekspr(cg, &i.cond, lokal, subst, out);
                di_blok(cg, &i.then, lokal, subst, out);
                for (_, c, bb) in &i.else_ifs {
                    di_ekspr(cg, c, lokal, subst, out);
                    di_blok(cg, bb, lokal, subst, out);
                }
                if let Some(e) = &i.else_ { di_blok(cg, e, lokal, subst, out); }
            }
            Expr::Match { scrutinee, arms, .. } => {
                di_ekspr(cg, scrutinee, lokal, subst, out);
                let ts = cg.tipe_ekspresi(scrutinee, lokal, subst, "");
                for a in arms {
                    // Nama yang diikat pola ikut dikenal di lengannya (`Ok(v) => f(v)`).
                    let mut dalam = lokal.clone();
                    if let ([p], Some(t)) = (a.patterns.as_slice(), ts.as_ref()) {
                        for (n, tn) in cg.ikatan_pola(p, t).unwrap_or_default() { dalam.insert(n, tn); }
                    }
                    match &a.body {
                        MatchBody::Block(bb) => di_blok(cg, bb, &dalam, subst, out),
                        MatchBody::Expr(x) => di_ekspr(cg, x, &dalam, subst, out),
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) fn di_blok(
        cg: &Codegen, b: &crate::parser::Block, lokal: &Lokal, subst: &TypeSubst,
        out: &mut Kumpulan,
    ) {
        // Lokal yang dideklarasikan di blok ini ikut dikenal pernyataan sesudahnya
        // (`var a = awal` lalu `f(a, x)`); tipenya dari nilai awalnya.
        let mut milik = lokal.clone();
        for st in &b.stmts {
            let lokal = &milik.clone();
            match st {
                Stmt::Expr(e) => di_ekspr(cg, e, lokal, subst, out),
                Stmt::Let(l) => {
                    di_ekspr(cg, &l.init, lokal, subst, out);
                    if let (crate::parser::Pattern::Ident { name, .. }, Some(t)) =
                        (&l.pattern, cg.tipe_ekspresi(&l.init, lokal, subst, "")) {
                        milik.insert(name.clone(), t);
                    }
                }
                Stmt::Var(v) => {
                    if let Some(x) = &v.init {
                        di_ekspr(cg, x, lokal, subst, out);
                        if let Some(t) = cg.tipe_ekspresi(x, lokal, subst, "") { milik.insert(v.name.clone(), t); }
                    }
                }
                Stmt::Assign(a) => { for v in &a.values { di_ekspr(cg, v, lokal, subst, out) } }
                Stmt::Return(r) => { for v in &r.values { di_ekspr(cg, v, lokal, subst, out) } }
                Stmt::Block(bb) => di_blok(cg, bb, lokal, subst, out),
                Stmt::If(i) => {
                    di_ekspr(cg, &i.cond, lokal, subst, out);
                    di_blok(cg, &i.then, lokal, subst, out);
                    for (_, c, bb) in &i.else_ifs {
                        di_ekspr(cg, c, lokal, subst, out);
                        di_blok(cg, bb, lokal, subst, out);
                    }
                    if let Some(e) = &i.else_ { di_blok(cg, e, lokal, subst, out); }
                }
                Stmt::While(w) => { di_ekspr(cg, &w.cond, lokal, subst, out); di_blok(cg, &w.body, lokal, subst, out); }
                Stmt::For(ForStmt::Classic { body, .. }) => di_blok(cg, body, lokal, subst, out),
                // Variabel loop ikut dikenal di badannya: satu nama = indeks, dua nama =
                // (indeks atau kunci, nilai). Tanpa ini `f(x)` di `for _, x in daftar` buta.
                Stmt::For(ForStmt::Range { vars, iter, body, .. }) => {
                    let mut dalam = lokal.clone();
                    // `range(a, b)` dan `a..b` berisi bilangan bulat, apa pun yang dibaca penyimpul.
                    let rentang = matches!(&**iter, Expr::Range { .. })
                        || matches!(&**iter, Expr::Call { func, .. } if matches!(&**func,
                            Expr::Ident(n, _) if n == "range" || n == "range_inclusive"));
                    let tipe_iter = if rentang { Some(JrgType::Slice(Box::new(JrgType::I64))) }
                        else { cg.tipe_ekspresi(iter, lokal, subst, "") };
                    let (kunci, nilai) = match tipe_iter {
                        Some(JrgType::Array(e, _)) | Some(JrgType::Slice(e)) => (JrgType::I64, Some(*e)),
                        Some(JrgType::Str) => (JrgType::I64, Some(JrgType::Rune)),
                        Some(JrgType::Dict(v)) => (JrgType::Str, Some(*v)),
                        _ => (JrgType::I64, None),
                    };
                    if let Some(v0) = vars.first() { dalam.insert(v0.clone(), kunci); }
                    if let (Some(v1), Some(t)) = (vars.get(1), nilai) { dalam.insert(v1.clone(), t); }
                    di_blok(cg, body, &dalam, subst, out)
                }
                Stmt::Loop(l) => di_blok(cg, &l.body, lokal, subst, out),
                Stmt::Match(m) => {
                    di_ekspr(cg, &m.scrutinee, lokal, subst, out);
                    for a in &m.arms {
                        match &a.body {
                            MatchBody::Block(bb) => di_blok(cg, bb, lokal, subst, out),
                            MatchBody::Expr(x) => di_ekspr(cg, x, lokal, subst, out),
                        }
                    }
                }
                Stmt::Defer(d) => di_ekspr(cg, &d.expr, lokal, subst, out),
                _ => {}
            }
        }
        if let Some(t) = &b.tail { di_ekspr(cg, t, &milik, subst, out); }
    }

    di_blok(cg, b, lokal, subst, &mut out);
    (out.arg, out.paruh)
}

/// Panggilan `g(…, nama, …)` di dalam `b`: fungsi bernama yang MENERIMA `nama` sebagai
/// argumen, beserta posisinya. Dipakai untuk mengikuti parameter-fungsi yang diteruskan.
pub fn cari_penerusan(b: &crate::parser::Block, nama: &str, out: &mut Vec<(String, Vec<crate::parser::Expr>, usize)>) {
    use crate::parser::{Expr, ForStmt, MatchBody, Stmt};
    fn e(x: &Expr, nama: &str, out: &mut Vec<(String, Vec<Expr>, usize)>) {
        match x {
            Expr::Call { func, args, .. } => {
                if let Expr::Ident(g, _) = &**func {
                    if let Some(k) = args.iter().position(|a| matches!(a, Expr::Ident(n, _) if n == nama)) {
                        out.push((g.clone(), args.clone(), k));
                    }
                }
                for a in args { e(a, nama, out); }
            }
            Expr::Binary { lhs, rhs, .. } => { e(lhs, nama, out); e(rhs, nama, out); }
            Expr::Unary { expr, .. } => e(expr, nama, out),
            Expr::Ternary { cond, then, else_, .. } => { e(cond, nama, out); e(then, nama, out); e(else_, nama, out); }
            Expr::Block(bb) => cari_penerusan(bb, nama, out),
            Expr::If(i) => {
                cari_penerusan(&i.then, nama, out);
                for (_, _, bb) in &i.else_ifs { cari_penerusan(bb, nama, out); }
                if let Some(x) = &i.else_ { cari_penerusan(x, nama, out); }
            }
            Expr::Match { scrutinee, arms, .. } => {
                e(scrutinee, nama, out);
                for a in arms {
                    match &a.body { MatchBody::Block(bb) => cari_penerusan(bb, nama, out), MatchBody::Expr(x) => e(x, nama, out) }
                }
            }
            _ => {}
        }
    }
    for st in &b.stmts {
        match st {
            Stmt::Expr(x) => e(x, nama, out),
            Stmt::Let(l) => e(&l.init, nama, out),
            Stmt::Var(v) => { if let Some(x) = &v.init { e(x, nama, out) } }
            Stmt::Assign(a) => { for v in &a.values { e(v, nama, out) } }
            Stmt::Return(r) => { for v in &r.values { e(v, nama, out) } }
            Stmt::Block(bb) => cari_penerusan(bb, nama, out),
            Stmt::While(w) => cari_penerusan(&w.body, nama, out),
            Stmt::For(ForStmt::Classic { body, .. }) | Stmt::For(ForStmt::Range { body, .. }) =>
                cari_penerusan(body, nama, out),
            _ => {}
        }
    }
    if let Some(t) = &b.tail { e(t, nama, out); }
}

/// Ekspresi ekor sebuah badan closure, kalau ia sebuah blok.
pub(crate) fn pembungkus_ekor(body: &crate::parser::Expr, _span: crate::lexer::Span) -> crate::parser::Expr {
    match body {
        crate::parser::Expr::Block(b) => match &b.tail {
            Some(t) => (**t).clone(),
            None => body.clone(),
        },
        lain => lain.clone(),
    }
}

impl Codegen {
    /// Tipe elemen sebuah `var` tingkat modul yang nilai awalnya `[]`.
    pub fn tipe_elemen_var_global(&self, nama: &str) -> Option<JrgType> {
        let mut buang = Vec::new();
        self.tipe_elemen_var_global_dgn(nama, &mut buang)
    }

    pub(crate) fn tipe_elemen_var_global_dgn(&self, nama: &str, hasil: &mut Vec<JrgType>) -> Option<JrgType> {
        let subst = TypeSubst::new();
        let mut calon: HashMap<String, Option<JrgType>> = HashMap::new();
        // `var` milik modul berkunci `mN$nama`, tapi badan fungsinya menulis nama telanjang —
        // dan cuma fungsi unit yang sama yang bisa menyentuhnya.
        let unit = super::modul::unit_dari(nama);
        let nama = super::modul::tampil(nama);
        calon.insert(nama.to_string(), None);
        let mut ditugasi: HashMap<String, Vec<(crate::parser::Expr, Option<JrgType>)>> = HashMap::new();

        let mut fungsi: Vec<_> = self.tables.functions.iter()
            .filter(|(k, _)| super::modul::unit_dari(k) == unit).map(|(_, f)| f.clone()).collect();
        // Urutan tetap: IR yang sama untuk sumber yang sama.
        fungsi.sort_by(|a, b| a.name.cmp(&b.name));
        for f in &fungsi {
            let FnBody::Block(b) = &*f.body else { continue };
            let mut lokal = Lokal::new();
            for (i, p) in f.params.iter().enumerate() {
                if let Some(n) = pola_nama(&p.pattern) {
                    if let Some(t) = self.tipe_dari_anotasi(&p.ty, &subst) {
                        lokal.insert(n, t);
                    } else {
                        let _ = i;
                    }
                }
            }
            self.kumpul_calon(b, &mut lokal, &subst, &f.name, &mut calon, &mut ditugasi);
        }

        let nilai = ditugasi.get(nama)?;
        let mut semua: Vec<JrgType> = Vec::new();
        for (e, t_saat_itu) in nilai {
            let lokal = Lokal::new();
            let t = t_saat_itu.clone()
                .or_else(|| self.tipe_ekspresi(e, &lokal, &subst, ""));
            if let Some(t) = t {
                let elem = match t {
                    JrgType::Array(x, n) if n > 0 => Some(*x),
                    JrgType::Slice(x) => Some(*x),
                    _ => None,
                };
                if let Some(x) = elem {
                    if !semua.contains(&x) { semua.push(x); }
                }
            }
            for d in super::infer_tumbuh::elemen_didorong(e, nama) {
                if let Some(tx) = self.tipe_ekspresi(d, &lokal, &subst, "") {
                    let x = super::codegen_any::luruh(&tx);
                    if !semua.contains(&x) { semua.push(x); }
                }
            }
        }
        // Sumber tipe yang TIDAK terbaca dari penugasannya sendiri.
        for (nama_p, i) in self.param_yang_didorong_ke(nama) {
            for t in self.tipe_argumen_semua_situs(&nama_p, i) {
                if !semua.contains(&t) { semua.push(t); }
            }
        }
        *hasil = semua.clone();
        semua.into_iter().next()
    }

    /// `(nama fungsi, indeks parameter)` untuk tiap `global = arr::push(global, p)`
    /// yang mendorong sebuah PARAMETER ke dalam `global`.
    pub(crate) fn param_yang_didorong_ke(&self, global: &str) -> Vec<(String, usize)> {
        use crate::parser::{Expr, Stmt};
        let mut out = Vec::new();
        let mut fungsi: Vec<_> = self.tables.functions.values().cloned().collect();
        fungsi.sort_by(|a, b| a.name.cmp(&b.name));
        for f in &fungsi {
            let FnBody::Block(b) = &*f.body else { continue };
            for st in &b.stmts {
                let Stmt::Assign(a) = st else { continue };
                for (t, v) in a.targets.iter().zip(a.values.iter()) {
                    if !matches!(t, Expr::Ident(n, _) if n == global) { continue; }
                    let Expr::Call { func, args, .. } = v else { continue };
                    let cocok = matches!(&**func, Expr::PathExpr { segments, .. }
                        if segments.len() == 2 && segments[0] == "arr"
                            && segments[1] == "push");
                    if !cocok || args.len() != 2 { continue; }
                    let Expr::Ident(dorong, _) = &args[1] else { continue };
                    if let Some(i) = f.params.iter().position(
                        |p| pola_nama(&p.pattern).as_deref() == Some(dorong.as_str()))
                    {
                        out.push((f.name.clone(), i));
                    }
                }
            }
        }
        out
    }

    /// Tipe argumen ke-`i` di SEMUA situs panggilan sebuah fungsi.
    pub(crate) fn tipe_argumen_semua_situs(&self, fungsi: &str, i: usize) -> Vec<JrgType> {
        use crate::parser::{Expr, Stmt};
        let subst = TypeSubst::new();
        let lokal = Lokal::new();
        let mut out: Vec<JrgType> = Vec::new();

        fn di_ekspr(
            cg: &Codegen, e: &Expr, fungsi: &str, i: usize,
            lokal: &Lokal, subst: &TypeSubst, out: &mut Vec<JrgType>,
        ) {
            if let Expr::Call { func, args, .. } = e {
                if matches!(&**func, Expr::Ident(n, _) if n == fungsi) {
                    if let Some(a) = args.get(i) {
                        if let Some(t) = cg.tipe_ekspresi(a, lokal, subst, "") {
                            if !out.contains(&t) { out.push(t); }
                        }
                    }
                }
                for a in args { di_ekspr(cg, a, fungsi, i, lokal, subst, out); }
            }
            match e {
                Expr::Binary { lhs, rhs, .. } => {
                    di_ekspr(cg, lhs, fungsi, i, lokal, subst, out);
                    di_ekspr(cg, rhs, fungsi, i, lokal, subst, out);
                }
                Expr::Unary { expr, .. } | Expr::Cast { expr, .. } =>
                    di_ekspr(cg, expr, fungsi, i, lokal, subst, out),
                Expr::Block(b) => di_blok(cg, b, fungsi, i, lokal, subst, out),
                _ => {}
            }
        }
        fn di_blok(
            cg: &Codegen, b: &crate::parser::Block, fungsi: &str, i: usize,
            lokal: &Lokal, subst: &TypeSubst, out: &mut Vec<JrgType>,
        ) {
            // Lokal blok ini dicatat sambil jalan. `defer _catat(n)` atas
            // sebuah `var n = 1` tidak bisa ditipekan tanpa itu — dan
            // justru situs ITU satu-satunya yang menyerahkan `int`.
            let mut lokal = lokal.clone();
            for st in &b.stmts {
                match st {
                    Stmt::Var(v) => {
                        if let Some(t) = v.ty.as_ref()
                            .and_then(|te| cg.tipe_dari_anotasi(te, subst))
                            .or_else(|| v.init.as_ref()
                                .and_then(|e| cg.tipe_ekspresi(e, &lokal, subst, "")))
                        {
                            lokal.insert(v.name.clone(), t);
                        }
                    }
                    Stmt::Let(l) => {
                        if let Some(n) = pola_nama(&l.pattern) {
                            if let Some(t) = l.ty.as_ref()
                                .and_then(|te| cg.tipe_dari_anotasi(te, subst))
                                .or_else(|| cg.tipe_ekspresi(&l.init, &lokal, subst, ""))
                            {
                                lokal.insert(n, t);
                            }
                        }
                    }
                    _ => {}
                }
                let lokal = &lokal;
                match st {
                    Stmt::Expr(e) => di_ekspr(cg, e, fungsi, i, lokal, subst, out),
                    Stmt::Let(l) => di_ekspr(cg, &l.init, fungsi, i, lokal, subst, out),
                    Stmt::Var(v) => { if let Some(e) = &v.init {
                        di_ekspr(cg, e, fungsi, i, lokal, subst, out) } }
                    Stmt::Assign(a) => { for v in &a.values {
                        di_ekspr(cg, v, fungsi, i, lokal, subst, out) } }
                    Stmt::Block(bb) => di_blok(cg, bb, fungsi, i, lokal, subst, out),
                    // `defer _catat(n)` sebuah pemanggilan juga — dan justru di situ
                    // satu-satunya situs yang menyerahkan `int`.
                    Stmt::Defer(d) => di_ekspr(cg, &d.expr, fungsi, i, lokal, subst, out),
                    Stmt::Return(r) => { for e in &r.values {
                        di_ekspr(cg, e, fungsi, i, lokal, subst, out) } }
                    Stmt::If(x) => {
                        di_blok(cg, &x.then, fungsi, i, lokal, subst, out);
                        for (_, c, bb) in &x.else_ifs {
                            di_ekspr(cg, c, fungsi, i, lokal, subst, out);
                            di_blok(cg, bb, fungsi, i, lokal, subst, out);
                        }
                        if let Some(e) = &x.else_ { di_blok(cg, e, fungsi, i, lokal, subst, out); }
                    }
                    Stmt::While(w) => di_blok(cg, &w.body, fungsi, i, lokal, subst, out),
                    Stmt::Loop(l) => di_blok(cg, &l.body, fungsi, i, lokal, subst, out),
                    Stmt::For(f) => match f {
                        crate::parser::ForStmt::Classic { body, .. }
                        | crate::parser::ForStmt::Range { body, .. } =>
                            di_blok(cg, body, fungsi, i, lokal, subst, out),
                    },
                    _ => {}
                }
            }
            if let Some(t) = &b.tail { di_ekspr(cg, t, fungsi, i, &lokal, subst, out); }
        }

        // Fungsi bebas DAN method `impl`. `_catat("method:" + self.nama)`
        // di dalam sebuah `impl` adalah situs panggilan yang sama sahnya, dan
        // method tidak ada di `tables.functions`.
        let mut fs: Vec<_> = self.tables.functions.values().cloned().collect();
        for m in self.tables.impls.values() {
            fs.extend(m.values().cloned());
        }
        fs.sort_by(|a, b| a.name.cmp(&b.name));
        for f in &fs {
            if let FnBody::Block(b) = &*f.body {
                di_blok(self, b, fungsi, i, &lokal, &subst, &mut out);
            }
        }
        out
    }

    /// Tipe elemen sebuah `var` tingkat modul, plus SEMUA tipe berbeda yang
    /// pernah ditugaskan padanya.
    pub fn tipe_elemen_var_global_konflik(&self, nama: &str) -> Vec<JrgType> {
        let mut semua = Vec::new();
        let _ = self.tipe_elemen_var_global_dgn(nama, &mut semua);
        semua
    }
}

impl Codegen {
    /// Lokal yang dideklarasikan `nil` lalu diisi sebuah closure.
    pub fn tipe_var_dari_closure(
        &self, decl: &FnDecl, subst: &TypeSubst, arg: &HashMap<String, Vec<JrgType>>,
    ) -> HashMap<String, JrgType> {
        use crate::parser::{Expr, Stmt};
        let mut hasil = HashMap::new();
        let FnBody::Block(body) = &*decl.body else { return hasil };

        // Nama yang dideklarasikan tanpa nilai berguna.
        let mut nil_var: Vec<String> = Vec::new();
        for st in &body.stmts {
            if let Stmt::Var(v) = st {
                let kosong = match &v.init {
                    None => true,
                    Some(e) => matches!(&**e, Expr::Literal(crate::parser::LitKind::Nil, _)),
                };
                if kosong && v.ty.is_none() { nil_var.push(v.name.clone()); }
            }
        }
        if nil_var.is_empty() { return hasil; }

        for st in &body.stmts {
            let Stmt::Assign(a) = st else { continue };
            for (t, val) in a.targets.iter().zip(a.values.iter()) {
                let Expr::Ident(n, _) = t else { continue };
                if !nil_var.contains(n) || hasil.contains_key(n) { continue; }
                let Expr::Closure { params, body: cb, span, .. } = val else { continue };
                let Some(ptys) = arg.get(n) else { continue };
                if ptys.len() != params.len() { continue; }

                let palsu = FnDecl {
                    vis: crate::parser::Visibility::Private,
                    is_async: false,
                    name: "$closure".to_string(),
                    generics: Default::default(),
                    params: params.clone(),
                    ret_type: None,
                    where_clause: Vec::new(),
                    body: match &**cb {
                        Expr::Block(b) => FnBody::Block(b.clone()).into(),
                        lain => FnBody::Block(crate::parser::Block {
                            stmts: Vec::new(),
                            tail: Some(Box::new(lain.clone())),
                            span: *span,
                        }).into(),
                    },
                    span: *span,
                };
                let mut s2 = subst.clone();
                for (i, t) in ptys.iter().enumerate() {
                    s2.insert(format!("$param{}", i), t.clone());
                }
                let awal: Vec<(String, JrgType)> = params.iter().enumerate()
                    .filter_map(|(i, p)| pola_nama(&p.pattern).map(|n| (n, ptys[i].clone())))
                    .collect();
                let Some(ret) = self.simpulkan_tipe_return_dgn(&palsu, &s2, &awal) else { continue };
                hasil.insert(n.clone(), JrgType::Fn(ptys.clone(), Box::new(ret)));
            }
        }
        hasil
    }
}