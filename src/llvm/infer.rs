// src/llvm/infer.rs — llvm/infer.rs — menyimpulkan tipe return fungsi yang tidak beranotasi.

use std::collections::HashMap;

use crate::parser::*;

use super::codegen::Codegen;
use super::sema::{resolve_type_expr, TypeResolveCtx, TypeSubst};
use super::types::JrgType;

/// Segmen sebuah jalur tanpa penanda bawaan: `::dict::from_pairs` dibaca `dict::from_pairs`.
pub(crate) fn tanpa_awalan_bawaan(segments: &[String]) -> &[String] {
    match segments.first() {
        Some(s) if s.is_empty() => &segments[1..],
        _ => segments,
    }
}

// Dipecah ke dua berkas tetangga; `pub use` menjaga jalur `infer::X` yang
// sudah dipakai pemanggilnya tetap persis seperti sebelumnya.
pub use super::infer_tumbuh::*;

/// Kunci titipan di tabel lokal penyimpul: tipe sebuah lokal yang tumbuh (`$` tak bisa ditulis).
const AWALAN_TUMBUH: &str = "$tumbuh:";
pub use super::infer_bentrok::*;

/// Tipe variabel lokal yang sudah terbaca selama penelusuran.
pub type Lokal = HashMap<String, JrgType>;

impl Codegen {
    /// Tipe return `decl`, disimpulkan dari badannya. `None` = tidak yakin.
    pub fn simpulkan_tipe_return(&self, decl: &FnDecl, subst: &TypeSubst) -> Option<JrgType> {
        self.simpulkan_tipe_return_dgn(decl, subst, &[])
    }

    /// Sama, dengan lokal yang sudah diketahui lebih dulu.
    pub fn simpulkan_tipe_return_dgn(
        &self, decl: &FnDecl, subst: &TypeSubst, awal: &[(String, JrgType)],
    ) -> Option<JrgType> {
        let FnBody::Block(body) = &*decl.body else { return None };

        let mut lokal = Lokal::new();
        // `var` tingkat modul dulu: parameter dan tangkapan boleh membayanginya,
        // jadi keduanya ditulis SESUDAH ini.
        for (n, (_, t)) in &self.var_global {
            lokal.insert(n.clone(), t.clone());
        }
        for (n, t) in awal {
            lokal.insert(n.clone(), t.clone());
        }
        for (i, p) in decl.params.iter().enumerate() {
            let Some(nama) = pola_nama(&p.pattern) else { continue };
            // Dua sumber tipe parameter, dan yang kedua yang penting:
            let t = self.tipe_dari_anotasi(&p.ty, subst)
                .or_else(|| subst.get(&format!("$param{}", i)).cloned())
                .or_else(|| subst.get(&format!("$arrparam{}", i)).cloned()
                    .map(|elem| JrgType::Slice(Box::new(elem))));
            if let Some(t) = t {
                lokal.insert(nama, t);
            }
        }

        // `?` mengikat tipe galat fungsi ini ke tipe galat operandnya — itu aturan
        // bahasanya, bukan tebakan.
        let subst_ralat;
        let subst = match self.ralat_dari_propagasi(body, &lokal, subst) {
            Some(e) => {
                let mut m = subst.clone();
                m.insert("$ralat".to_string(), e);
                subst_ralat = m;
                &subst_ralat
            }
            None => subst,
        };

        for (n, t) in self.lokal_yang_tumbuh(decl, subst) {
            lokal.insert(format!("{}{}", AWALAN_TUMBUH, n), t);
        }
        let mut ketemu: Option<JrgType> = None;
        self.telusuri_blok(body, &mut lokal, subst, &mut ketemu, &decl.name);
        ketemu
    }

    /// Tipe galat dari operand `?` pertama yang bertipe `Result` di sebuah badan.
    pub(crate) fn ralat_dari_propagasi(
        &self, b: &Block, lokal: &Lokal, subst: &TypeSubst,
    ) -> Option<JrgType> {
        use crate::parser::Expr;
        fn cari(b: &Block, keluar: &mut Vec<Expr>) {
            fn e(x: &Expr, keluar: &mut Vec<Expr>) {
                if let Expr::Propagate { expr, .. } = x { keluar.push((**expr).clone()); }
                match x {
                    Expr::Propagate { expr, .. } | Expr::Unary { expr, .. }
                    | Expr::Cast { expr, .. } => e(expr, keluar),
                    Expr::Binary { lhs, rhs, .. } => { e(lhs, keluar); e(rhs, keluar); }
                    Expr::Call { func, args, .. } => { e(func, keluar); for a in args { e(a, keluar) } }
                    Expr::Method { obj, args, .. } => { e(obj, keluar); for a in args { e(a, keluar) } }
                    Expr::Block(bb) => cari(bb, keluar),
                    _ => {}
                }
            }
            for s in &b.stmts {
                match s {
                    crate::parser::Stmt::Let(l) => e(&l.init, keluar),
                    crate::parser::Stmt::Expr(x) => e(x, keluar),
                    crate::parser::Stmt::Return(r) => { for x in &r.values { e(x, keluar) } }
                    _ => {}
                }
            }
            if let Some(t) = &b.tail { e(t, keluar); }
        }
        let mut kandidat = Vec::new();
        cari(b, &mut kandidat);
        for k in kandidat {
            if let Some(JrgType::Enum(n)) = self.tipe_ekspresi(&k, lokal, subst, "") {
                if let Some((_, e)) = self.result_args.get(&n) {
                    return Some(e.clone());
                }
            }
        }
        None
    }

    /// Tipe variabel PERTAMA sebuah `for … in …`.
    pub(crate) fn tipe_var_loop_pertama(
        &self, iter: &Expr, lokal: &Lokal, subst: &TypeSubst, nama_diri: &str,
    ) -> JrgType {
        match self.tipe_ekspresi(iter, lokal, subst, nama_diri) {
            Some(JrgType::Dict(_)) | Some(JrgType::DictCampur(_)) => JrgType::Str,
            _ => JrgType::I64,
        }
    }

    pub(crate) fn tipe_dari_anotasi(&self, te: &TypeExpr, subst: &TypeSubst) -> Option<JrgType> {
        if matches!(te, TypeExpr::Inferred) { return None; }
        let ctx = TypeResolveCtx { tables: &self.tables, subst };
        resolve_type_expr(te, &ctx).ok()
    }

    /// Telusuri satu blok, mencatat lokal dan mengumpulkan kandidat tipe.
    pub(crate) fn telusuri_blok(
        &self,
        b: &Block,
        lokal: &mut Lokal,
        subst: &TypeSubst,
        ketemu: &mut Option<JrgType>,
        nama_diri: &str,
    ) {
        for s in &b.stmts {
            self.telusuri_stmt(s, lokal, subst, ketemu, nama_diri);
        }
        // Ekor blok fungsi juga menghasilkan nilai return.
        if let Some(t) = &b.tail {
            if let Some(ty) = self.tipe_ekspresi(t, lokal, subst, nama_diri) {
                pilih(ketemu, ty);
            }
        }
    }

    /// Badan perulangan: ekornya bukan nilai kembalian fungsi, jadi cuma pernyataannya yang ditelusuri.
    fn telusuri_ulang(&self, b: &Block, lokal: &mut Lokal, subst: &TypeSubst, ketemu: &mut Option<JrgType>, nama_diri: &str) {
        for s in &b.stmts {
            self.telusuri_stmt(s, lokal, subst, ketemu, nama_diri);
        }
    }

    pub(crate) fn telusuri_stmt(
        &self,
        s: &Stmt,
        lokal: &mut Lokal,
        subst: &TypeSubst,
        ketemu: &mut Option<JrgType>,
        nama_diri: &str,
    ) {
        match s {
            Stmt::Let(l) => {
                let t = self.tipe_dari_anotasi(&l.ty.clone().unwrap_or(TypeExpr::Inferred), subst)
                    .or_else(|| self.tipe_ekspresi(&l.init, lokal, subst, nama_diri));
                if let (Some(nama), Some(t)) = (pola_nama(&l.pattern), t) {
                    lokal.insert(nama, t);
                }
            }
            Stmt::Var(v) => {
                // Lokal yang tumbuh bertipe hasil analisisnya, bukan tipe nilai awalnya.
                let t = self.tipe_dari_anotasi(&v.ty.clone().unwrap_or(TypeExpr::Inferred), subst)
                    .or_else(|| lokal.get(&format!("{}{}", AWALAN_TUMBUH, v.name)).cloned())
                    .or_else(|| v.init.as_ref().and_then(|e| self.tipe_ekspresi(e, lokal, subst, nama_diri)));
                if let Some(t) = t {
                    lokal.insert(v.name.clone(), t);
                }
            }
            Stmt::ShortDecl(sd) => {
                // Bentuk jamak (`a, b := f()`) dilewati: memecah tuple hasil
                // panggilan ke nama-namanya butuh tahu bentuk tuple-nya, dan
                // menebak salah satu nama saja akan mencatat tipe yang keliru.
                if sd.names.len() == 1 && sd.values.len() == 1 {
                    if let Some(t) = self.tipe_ekspresi(&sd.values[0], lokal, subst, nama_diri) {
                        lokal.insert(sd.names[0].clone(), t);
                    }
                }
            }
            Stmt::Return(r) => {
                // `return` telanjang mengembalikan Unit; ia tidak memberi
                // informasi apa pun, jadi tidak mengubah kandidat.
                if r.values.is_empty() { return; }

                // Return multi-nilai membentuk tuple — bentuk yang sama
                // dengan yang dipakai `codegen_return`, jadi kesimpulan di
                // sini tidak bisa berbeda dari yang nanti dihasilkan.
                let mut bagian = Vec::with_capacity(r.values.len());
                for e in &r.values {
                    match self.tipe_ekspresi(e, lokal, subst, nama_diri) {
                        Some(t) => bagian.push(t),
                        None => {
                            // Ada `return <expr>` yang tipenya tidak terbaca.
                            // Menganggapnya Unit akan salah, dan memilih
                            // kandidat lain juga salah. Gugurkan.
                            *ketemu = Some(JrgType::Never);
                            return;
                        }
                    }
                }
                let ty = if bagian.len() == 1 {
                    bagian.pop().expect("baru diperiksa tidak kosong")
                } else {
                    JrgType::Tuple(bagian)
                };
                pilih(ketemu, ty);
            }
            Stmt::Block(b) => self.telusuri_blok(b, lokal, subst, ketemu, nama_diri),
            Stmt::If(i) => {
                self.telusuri_blok(&i.then, lokal, subst, ketemu, nama_diri);
                for (_, _, blk) in &i.else_ifs {
                    self.telusuri_blok(blk, lokal, subst, ketemu, nama_diri);
                }
                if let Some(e) = &i.else_ { self.telusuri_blok(e, lokal, subst, ketemu, nama_diri); }
            }
            Stmt::While(w) => self.telusuri_ulang(&w.body, lokal, subst, ketemu, nama_diri),
            Stmt::Loop(l) => self.telusuri_ulang(&l.body, lokal, subst, ketemu, nama_diri),
            Stmt::For(f) => match f {
                ForStmt::Classic { body, .. } =>
                    self.telusuri_ulang(body, lokal, subst, ketemu, nama_diri),
                ForStmt::Range { vars, iter, body, .. } => {
                    // Variabel loop DIIKAT sebelum badannya ditelusuri.
                    for (k, nama) in vars.iter().enumerate() {
                        let t = if k == 0 {
                            self.tipe_var_loop_pertama(iter, lokal, subst, nama_diri)
                        } else {
                            match self.tipe_ekspresi(iter, lokal, subst, nama_diri) {
                                Some(JrgType::Array(e, _)) | Some(JrgType::Slice(e)) => *e,
                                Some(JrgType::Str) => JrgType::Rune,
                                // `for k, v in d`: yang pertama kunci,
                                // yang kedua NILAINYA.
                                Some(JrgType::Dict(v)) => *v,
                                Some(JrgType::DictCampur(_)) => JrgType::Any,
                                _ => continue,
                            }
                        };
                        lokal.insert(nama.clone(), t);
                    }
                    self.telusuri_ulang(body, lokal, subst, ketemu, nama_diri)
                }
            },
            Stmt::Match(m) => {
                for arm in &m.arms {
                    match &arm.body {
                        MatchBody::Block(b) => self.telusuri_blok(b, lokal, subst, ketemu, nama_diri),
                        MatchBody::Expr(_) => {}
                    }
                }
            }
            _ => {}
        }
    }

    /// Tipe sebuah ekspresi, sejauh bisa dipastikan tanpa menurunkannya.
    pub(crate) fn tipe_ekspresi(
        &self,
        e: &Expr,
        lokal: &Lokal,
        subst: &TypeSubst,
        nama_diri: &str,
    ) -> Option<JrgType> {
        match e {
            Expr::Literal(LitKind::Int(_), _) => Some(JrgType::I64),
            Expr::Literal(LitKind::Float(_), _) => Some(JrgType::F64),
            Expr::Literal(LitKind::Bool(_), _) => Some(JrgType::Bool),
            Expr::Literal(LitKind::Rune(_), _) => Some(JrgType::Rune),
            Expr::Literal(LitKind::Str(_), _) | Expr::Literal(LitKind::RawStr(_), _) => Some(JrgType::Str),

            Expr::Ident(n, _) => lokal.get(n).cloned()
                // Sebuah NAMA FUNGSI juga sebuah nilai. Tanpa lengan ini,
                // `var fs = []` yang diisi fungsi bernama tidak bisa
                // menyimpulkan tipe elemennya.
                .or_else(|| self.tipe_fungsi_bernama(n, subst)),

            // Closure: tipe parameter dari anotasinya, tipe hasil dari badannya lewat
            // penyimpul yang sama.
            Expr::Closure { params, body, span, .. } => {
                // Parameter tanpa anotasi boleh datang dari rantai yang dititipkan
                // pembuatnya — itulah `|b|` di `let luar = |a| { return |b| { return a + b } …
                let mut ps = Vec::with_capacity(params.len());
                for (i, p) in params.iter().enumerate() {
                    let t = match self.tipe_dari_anotasi(&p.ty, subst) {
                        Some(t) => t,
                        None => subst.get(&format!("$rantai0param{}", i))?.clone(),
                    };
                    ps.push(t);
                }
                // Badannya dilihat satu tingkat lebih dalam.
                let subst_dalam = geser_rantai(subst);
                let subst = &subst_dalam;
                let palsu = crate::parser::FnDecl {
                    vis: crate::parser::Visibility::Private,
                    is_async: false,
                    name: "$closure".to_string(),
                    generics: Default::default(),
                    params: params.clone(),
                    ret_type: None,
                    where_clause: Vec::new(),
                    body: match &**body {
                        Expr::Block(b) => FnBody::Block(b.clone()).into(),
                        lain => FnBody::Block(crate::parser::Block {
                            stmts: Vec::new(),
                            tail: Some(Box::new(lain.clone())),
                            span: *span,
                        }).into(),
                    },
                    span: *span,
                };
                let awal: Vec<(String, JrgType)> =
                    lokal.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                let ret = self.simpulkan_tipe_return_dgn(&palsu, subst, &awal)
                    .or_else(|| match &*palsu.body {
                        FnBody::Block(b) if b.tail.is_none()
                            && !blok_punya_return_bernilai(b) => Some(JrgType::Unit),
                        _ => None,
                    })?;
                Some(JrgType::Fn(ps, Box::new(ret)))
            }

            // `e?` menghasilkan paruh SUKSES-nya. Tanpa lengan ini
            // `let v = r?` tidak menipekan `v`, dan semua yang memakainya
            // ikut gugur.
            Expr::Propagate { expr, .. } => {
                let JrgType::Enum(n) = self.tipe_ekspresi(expr, lokal, subst, nama_diri)? else {
                    return None;
                };
                self.result_args.get(&n).map(|(t, _)| t.clone())
                    .or_else(|| self.option_args.get(&n).cloned())
            }

            // `Option::Some(x)` berkualifikasi: sama dengan `Some(x)`.
            Expr::Call { func, args, .. }
                if args.len() == 1 && matches!(&**func, Expr::PathExpr { segments, .. }
                    if segments.len() == 2 && segments[0] == "Option" && segments[1] == "Some") =>
            {
                let t = self.tipe_ekspresi(&args[0], lokal, subst, nama_diri)?;
                let nama = crate::llvm::mangle::mangle_option(&t);
                self.opsi_tertunda.borrow_mut().insert(nama.clone(), t);
                Some(JrgType::Enum(nama))
            }

            // `Ok(x)` / `Some(x)`: paruh yang tertulis diambil dari argumennya, paruh
            // galatnya dari `$ralat` — yaitu tipe galat yang sudah diikat `?` di badan …
            Expr::Call { func, args, .. }
                if args.len() == 1
                    && matches!(&**func, Expr::Ident(n, _)
                        if matches!(n.as_str(), "Ok" | "Err" | "Some")) =>
            {
                let Expr::Ident(n, _) = &**func else { return None };
                let t = self.tipe_ekspresi(&args[0], lokal, subst, nama_diri)?;
                match n.as_str() {
                    "Ok" => {
                        let e = subst.get("$ralat")?;
                        Some(JrgType::Enum(crate::llvm::mangle::mangle_result(&t, e)))
                    }
                    "Some" => {
                        let nama = crate::llvm::mangle::mangle_option(&t);
                        self.opsi_tertunda.borrow_mut().insert(nama.clone(), t);
                        Some(JrgType::Enum(nama))
                    }
                    "Err" => {
                        // Paruh suksesnya tidak ada di baris ini. Digugurkan,
                        // bukan ditebak.
                        None
                    }
                    _ => None,
                }
            }

            // Template literal selalu string, apa pun isinya — itu memang
            // gunanya. Tanpa lengan ini `t = t + [«${k}=${v}»]` tidak bisa
            // disimpulkan, dan `t` jatuh ke bawaan `array<int>`.
            Expr::Template { .. } => Some(JrgType::Str),

            Expr::Unary { op: UnaryOp::Neg, expr, .. } => self.tipe_ekspresi(expr, lokal, subst, nama_diri),
            Expr::Unary { op: UnaryOp::Not, .. } => Some(JrgType::Bool),

            Expr::Binary { op, lhs, rhs, .. } => match op {
                BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le
                | BinaryOp::Gt | BinaryOp::Ge | BinaryOp::And | BinaryOp::Or => Some(JrgType::Bool),
                _ => {
                    let kiri = self.tipe_ekspresi(lhs, lokal, subst, nama_diri);
                    let kanan = self.tipe_ekspresi(rhs, lokal, subst, nama_diri);
                    match (&kiri, &kanan) {
                        (Some(a), _) if *op == BinaryOp::Pow && a.is_int()
                            && super::codegen_expr::eksponen_negatif_tertulis(rhs).is_some() => Some(JrgType::F64),
                        // Bulat lawan pecahan menjawab pecahan, di sisi mana pun pecahannya.
                        (Some(a), Some(b)) if a.is_int() && b.is_float() => kanan,
                        _ => kiri.or(kanan),
                    }
                }
            },

            Expr::Cast { ty, .. } => self.tipe_dari_anotasi(ty, subst),

            Expr::Array { elems, span: _ } => {
                let first = elems.first()?;
                let et = self.tipe_ekspresi(first, lokal, subst, nama_diri)?;
                // Struct/enum/tuple bercampur dengan tipe lain di satu literal: elemennya bertag,
                // sama dengan penurun literalnya (`[k, Pindah::Tetap]`).
                let buram = |t: &JrgType| matches!(t, JrgType::Struct(_) | JrgType::Enum(_) | JrgType::Tuple(_));
                // Varian enum lewat path (`modul::Enum::V(..)`, `Enum::V`): golongannya sudah jelas
                // walau tipe lengkapnya belum terbaca di sini.
                let varian = |e: &Expr| -> Option<JrgType> {
                    let segs = match e {
                        Expr::Call { func, .. } => match &**func { Expr::PathExpr { segments, .. } => segments, _ => return None },
                        Expr::PathExpr { segments, .. } => segments,
                        _ => return None,
                    };
                    let en = segs.get(segs.len().checked_sub(2)?)?;
                    self.tables.enums.contains_key(en).then(|| JrgType::Enum(en.clone()))
                };
                for e in &elems[1..] {
                    match self.tipe_ekspresi(e, lokal, subst, nama_diri).or_else(|| varian(e)) {
                        Some(t) if t != et && (buram(&t) || buram(&et)) =>
                            return Some(JrgType::Array(Box::new(JrgType::Any), elems.len() as u64)),
                        _ => {}
                    }
                }
                Some(JrgType::Array(Box::new(et), elems.len() as u64))
            }

            Expr::Tuple { elems, .. } if elems.is_empty() => Some(JrgType::Unit),
            Expr::Tuple { elems, .. } => {
                let mut out = Vec::with_capacity(elems.len());
                for el in elems {
                    out.push(self.tipe_ekspresi(el, lokal, subst, nama_diri)?);
                }
                Some(JrgType::Tuple(out))
            }

            Expr::Struct { ty, .. } => self.tipe_dari_anotasi(ty, subst),

            // Method yang tipenya bisa dijawab tanpa menurunkan apa pun.
            Expr::Method { obj, method, args, .. } => {
                let ot = self.tipe_ekspresi(obj, lokal, subst, nama_diri);
                // Method pemakai atas struct MENANG atas nama bawaan, dan tipenya dari
                // ANOTASI-nya (warisan lewat `find_method`). Tanpa anotasi: gugur.
                if let Some(JrgType::Struct(nama)) = &ot {
                    if let Some(m) = self.tables.find_method(nama, method) {
                        return self.tipe_dari_anotasi(m.ret_type.as_ref()?, subst);
                    }
                }
                match (method.as_str(), &ot) {
                    ("len", _) | ("byte_len", _) => Some(JrgType::I64),
                    // `find`/`index_of` menjawab `Option<int>`, bukan `int`:
                    // `-1` adalah indeks yang SAH di JRG.
                    ("find", Some(JrgType::Str)) | ("index_of", Some(JrgType::Str)) => {
                        Some(JrgType::Enum(crate::llvm::mangle::mangle_option(&JrgType::I64)))
                    }
                    ("is_empty", _) | ("contains", Some(JrgType::Str))
                        | ("starts_with", _) | ("ends_with", _) => Some(JrgType::Bool),
                    ("to_uppercase", _) | ("to_lowercase", _) | ("trim", _)
                        | ("trim_start", _) | ("trim_end", _) | ("reverse", Some(JrgType::Str))
                        | ("to_string", _) | ("replace", Some(JrgType::Str))
                        | ("sub", Some(JrgType::Str)) | ("substring", Some(JrgType::Str)) =>
                            Some(JrgType::Str),
                    ("split", Some(JrgType::Str)) => Some(JrgType::Slice(Box::new(JrgType::Str))),
                    ("map", Some(JrgType::Array(_, _))) | ("map", Some(JrgType::Slice(_))) => {
                        // Tipe elemen hasilnya = tipe hasil closure-nya, yang
                        // parameternya bertipe elemen koleksi ini.
                        let el = match ot.as_ref()? {
                            JrgType::Array(e, _) | JrgType::Slice(e) => (**e).clone(),
                            _ => return None,
                        };
                        let mut l2 = lokal.clone();
                        if let Expr::Closure { params, body, span, .. } = args.first()? {
                            if params.len() != 1 { return None; }
                            if let Some(n) = pola_nama(&params[0].pattern) { l2.insert(n, el); }
                            let r = self.tipe_ekspresi(&pembungkus_ekor(body, *span), &l2, subst, nama_diri)?;
                            return Some(JrgType::Slice(Box::new(r)));
                        }
                        None
                    }
                    ("filter", Some(JrgType::Array(e, _))) => Some(JrgType::Slice(e.clone())),
                    ("filter", Some(JrgType::Slice(e))) => Some(JrgType::Slice(e.clone())),
                    _ => None,
                }
            }

            // Memotong array atau slice menghasilkan slice — tipe elemennya
            // ikut. Tanpa lengan ini, `fn f() -> array { return a[0:3] }`
            // tidak bisa disimpulkan sama sekali.
            Expr::Slice { obj, .. } => match self.tipe_ekspresi(obj, lokal, subst, nama_diri)? {
                JrgType::Array(e, _) | JrgType::Slice(e) => Some(JrgType::Slice(e)),
                JrgType::Str => Some(JrgType::Str),
                _ => None,
            },

            Expr::Index { obj, .. } => match self.tipe_ekspresi(obj, lokal, subst, nama_diri)? {
                JrgType::Array(e, _) | JrgType::Slice(e) => Some(*e),
                JrgType::Str => Some(JrgType::Rune),
                _ => None,
            },

            Expr::Field { obj, field, .. } => {
                match self.tipe_ekspresi(obj, lokal, subst, nama_diri)? {
                    JrgType::Struct(nama) => self.types.structs.get(&nama)?.field_type(field).cloned(),
                    JrgType::Tuple(ts) => ts.get(field.parse::<usize>().ok()?).cloned(),
                    _ => None,
                }
            }

            // Fungsi `arr::` punya tipe kembalian yang bisa dipastikan dari argumennya.
            Expr::Call { func, args, .. }
                if matches!(func.as_ref(), Expr::PathExpr { segments, .. }
                            if segments.len() == 2 && segments[0] == "arr") =>
            {
                let Expr::PathExpr { segments, .. } = func.as_ref() else { return None };
                let item = segments[1].as_str();
                let pertama = args.first()?;
                match item {
                    "push" => {
                        // Tipe elemennya bisa datang dari koleksinya, atau —
                        // kalau koleksinya belum diketahui, yang justru
                        // kasus yang menarik — dari nilai yang didorong.
                        let el = match self.tipe_ekspresi(pertama, lokal, subst, nama_diri) {
                            Some(JrgType::Array(e, _)) | Some(JrgType::Slice(e)) => Some(*e),
                            _ => None,
                        };
                        let el = el.or_else(|| {
                            self.tipe_ekspresi(args.get(1)?, lokal, subst, nama_diri)
                        })?;
                        Some(JrgType::Slice(Box::new(el)))
                    }
                    "reverse" | "sort" => {
                        match self.tipe_ekspresi(pertama, lokal, subst, nama_diri)? {
                            JrgType::Array(e, _) | JrgType::Slice(e) => Some(JrgType::Slice(e)),
                            _ => None,
                        }
                    }
                    "sum" | "min" | "max" => {
                        match self.tipe_ekspresi(pertama, lokal, subst, nama_diri)? {
                            JrgType::Array(e, _) | JrgType::Slice(e) => Some(*e),
                            _ => None,
                        }
                    }
                    "contains" => Some(JrgType::Bool),
                    _ => None,
                }
            }

            // Literal dict diturunkan parser jadi `dict::from_pairs([[k,v],…])`, jadi
            // tanpa lengan ini `let d = {"a": 1}` tidak punya tipe di mata penyimpul …
            Expr::Call { func, args, .. }
                if matches!(func.as_ref(), Expr::PathExpr { segments, .. }
                    if matches!(tanpa_awalan_bawaan(segments), [a, b] if a == "dict" && b == "from_pairs")) =>
            {
                let Some(Expr::Array { elems, .. }) = args.first() else { return None };
                let mut vt: Option<JrgType> = None;
                for pasang in elems {
                    let Expr::Array { elems: kv, .. } = pasang else { return None };
                    if kv.len() != 2 { return None; }
                    let t = self.tipe_ekspresi(&kv[1], lokal, subst, nama_diri)?;
                    match &vt {
                        None => vt = Some(t),
                        // Campur tipe: nilainya dikotak, jadi tipe nilainya
                        // `Any`. Yang memakai jawaban ini cuma butuh tahu ia
                        // sebuah dict.
                        Some(x) if *x != t => { vt = Some(JrgType::Any); break; }
                        _ => {}
                    }
                }
                Some(JrgType::Dict(Box::new(vt?)))
            }
            // `dict::values(d)` / `dict::keys(d)`: tipe elemennya dari tipe nilai dict-nya.
            Expr::Call { func, args, .. }
                if args.len() == 1 && matches!(func.as_ref(), Expr::PathExpr { segments, .. }
                    if matches!(tanpa_awalan_bawaan(segments), [a, b] if a == "dict" && (b == "values" || b == "keys"))) =>
            {
                let Expr::PathExpr { segments, .. } = func.as_ref() else { return None };
                let kunci = tanpa_awalan_bawaan(segments)[1] == "keys";
                match self.tipe_ekspresi(&args[0], lokal, subst, nama_diri)? {
                    JrgType::Dict(_) | JrgType::DictCampur(_) if kunci => Some(JrgType::Slice(Box::new(JrgType::Str))),
                    JrgType::Dict(v) => Some(JrgType::Slice(v)),
                    JrgType::DictCampur(_) => Some(JrgType::Slice(Box::new(JrgType::Any))),
                    _ => None,
                }
            }
            // `Atomic::new(..)`/`Mutex::new()` dan yang dijawab `atomic::`/`mutex::lock`/`unlock`.
            Expr::Call { func, .. }
                if matches!(func.as_ref(), Expr::PathExpr { segments, .. }
                    if matches!(tanpa_awalan_bawaan(segments), [a, _] if a == "Atomic" || a == "Mutex" || a == "atomic")
                        || matches!(tanpa_awalan_bawaan(segments), [a, b] if a == "mutex" && b != "with_lock")) =>
            {
                let Expr::PathExpr { segments, .. } = func.as_ref() else { return None };
                let [a, b] = tanpa_awalan_bawaan(segments) else { return None };
                Some(match (a.as_str(), b.as_str()) {
                    ("Atomic", "new") => JrgType::Sinkron(false),
                    ("Mutex", "new") => JrgType::Sinkron(true),
                    ("atomic", "get" | "add" | "sub") => JrgType::I64,
                    ("atomic", "cas") => JrgType::Bool,
                    ("atomic", "set") | ("mutex", _) => JrgType::Unit,
                    _ => return None,
                })
            }
            // `Future::new(f, ..)`/`Task::spawn(f, ..)` atas fungsi bernama beranotasi: tugas berhasil itu.
            Expr::Call { func, args, .. }
                if matches!(func.as_ref(), Expr::PathExpr { segments, .. }
                    if matches!(tanpa_awalan_bawaan(segments), [a, b]
                        if (a == "Future" && b == "new") || (a == "Task" && b == "spawn"))) =>
            {
                let Expr::PathExpr { segments, .. } = func.as_ref() else { return None };
                let tugas = tanpa_awalan_bawaan(segments)[0] == "Task";
                let Expr::Ident(n, _) = args.first()? else { return None };
                let f = self.tables.functions.get(&self.k(n))?;
                let ret = self.tipe_dari_anotasi(f.ret_type.as_ref()?, subst)?;
                Some(JrgType::Masa(Box::new(ret), tugas))
            }
            // `dict::get_or(d, k, bawaan)` menjawab tipe NILAI dict-nya.
            Expr::Call { func, args, .. }
                if args.len() == 3 && matches!(func.as_ref(), Expr::PathExpr { segments, .. }
                    if matches!(tanpa_awalan_bawaan(segments), [a, b] if a == "dict" && b == "get_or")) =>
            {
                match self.tipe_ekspresi(&args[0], lokal, subst, nama_diri)? {
                    JrgType::Dict(v) => Some(*v),
                    JrgType::DictCampur(_) => Some(JrgType::Any),
                    _ => None,
                }
            }
            // `dict::new()` tanpa petunjuk bernilai bertag; `dict::set`/`remove` menjawab tipe dict-nya.
            Expr::Call { func, args, .. }
                if matches!(func.as_ref(), Expr::PathExpr { segments, .. }
                    if matches!(tanpa_awalan_bawaan(segments), [a, b]
                        if a == "dict" && (b == "new" || b == "set" || b == "remove"))) =>
            {
                match args.first() {
                    None => Some(JrgType::Dict(Box::new(JrgType::Any))),
                    Some(d) => match self.tipe_ekspresi(d, lokal, subst, nama_diri)? {
                        t @ (JrgType::Dict(_) | JrgType::DictCampur(_)) => Some(t),
                        _ => None,
                    },
                }
            }
            // `fmt::sprint(..)`, `str::to_string(..)` dan `str::join(..)` selalu menjawab teks.
            Expr::Call { func, .. }
                if matches!(func.as_ref(), Expr::PathExpr { segments, .. }
                    if matches!(tanpa_awalan_bawaan(segments), [a, b]
                        if (a == "fmt" && b == "sprint") || (a == "str" && (b == "to_string" || b == "join")))) => Some(JrgType::Str),
            // `str::split(s, p)` adalah `s.split(p)`: satu tabel tipe untuk dua bentuknya.
            Expr::Call { func, args, span }
                if !args.is_empty() && matches!(func.as_ref(), Expr::PathExpr { segments, .. }
                    if matches!(tanpa_awalan_bawaan(segments), [a, _] if a == "str"))
                    && self.tipe_ekspresi(&args[0], lokal, subst, nama_diri) == Some(JrgType::Str) =>
            {
                let Expr::PathExpr { segments, .. } = func.as_ref() else { return None };
                let method = tanpa_awalan_bawaan(segments)[1].clone();
                let sebagai = Expr::Method { obj: Box::new(args[0].clone()), method, args: args[1..].to_vec(), span: *span };
                self.tipe_ekspresi(&sebagai, lokal, subst, nama_diri)
            }
            // `Tipe::method(x)`: tipenya dari ANOTASI method itu, seperti fungsi
            // bernama. Namespace bawaan (`math::abs`) tidak punya `impl`, jadi gugur.
            Expr::Call { func, .. }
                if matches!(func.as_ref(), Expr::PathExpr { segments, .. } if segments.len() == 2) =>
            {
                let Expr::PathExpr { segments, .. } = func.as_ref() else { return None };
                let m = self.tables.find_method(&segments[0], &segments[1])?;
                self.tipe_dari_anotasi(m.ret_type.as_ref()?, subst)
            }
            Expr::Call { func, .. } => {
                let Expr::Ident(n, _) = func.as_ref() else { return None };
                // NILAI fungsi di lokal (parameter, closure): tipe hasilnya sudah
                // tertulis di tipenya — `fn jalankan(f) { return f() }`.
                if let Some(JrgType::Fn(_, ret)) = lokal.get(n) {
                    return Some((**ret).clone());
                }
                // Rekursi: menyimpulkan tipe `f` dari `return f(..)` butuh
                // tipe `f` itu sendiri. Digugurkan, tidak ditebak.
                if n == nama_diri { return None; }
                let f = self.tables.functions.get(&self.k(n))?;
                self.tipe_dari_anotasi(f.ret_type.as_ref()?, subst)
            }

            // Ternary, `if … else`, dan `match`: tipe lengannya, kalau SEMUA lengan terbaca dan
            // sepakat. Dulu tidak dikenal, jadi `fn f(n) { return n == 0 $ true : false }`
            // ditebak `()` dan nilainya dibuang tanpa kata.
            Expr::Ternary { then, else_, .. } => {
                // `c $ Some(x) : None`: `None` sendirian tidak bertipe, sisi lainnya yang memberi.
                let kosong = |e: &Expr| match e {
                    Expr::Ident(n, _) => n == "None",
                    Expr::PathExpr { segments, .. } => segments.len() == 2 && segments[0] == "Option" && segments[1] == "None",
                    _ => false,
                };
                let opsi = |t: &JrgType| matches!(t, JrgType::Enum(n) if n.starts_with("Option$"));
                if kosong(then) || kosong(else_) {
                    let lain = if kosong(then) { else_ } else { then };
                    return self.tipe_ekspresi(lain, lokal, subst, nama_diri).filter(opsi);
                }
                let a = self.tipe_ekspresi(then, lokal, subst, nama_diri)?;
                let b = self.tipe_ekspresi(else_, lokal, subst, nama_diri)?;
                (a == b).then_some(a)
            }
            Expr::If(i) => {
                let mut blok = vec![&i.then];
                blok.extend(i.else_ifs.iter().map(|(_, _, b)| b));
                blok.push(i.else_.as_ref()?);
                sepakat(blok.into_iter().map(|b| self.tipe_ekor_murni(b, lokal, subst, nama_diri)))
            }
            // Lengan yang polanya MENGIKAT nama digugurkan: tipe nama itu tidak ada di `lokal`,
            // dan nama luar bernama sama akan dibaca sebagai gantinya.
            Expr::Match { scrutinee, arms, .. } => {
                let ts = self.tipe_ekspresi(scrutinee, lokal, subst, nama_diri);
                sepakat(arms.iter().map(|a| {
                    if a.guard.is_some() { return None; }
                    // Lengan yang polanya mengikat nama: tipenya dibaca dari tipe skrutini
                    // (`Some(v)` atas `Option<int>`); pola yang tidak dipahami digugurkan.
                    let dalam = if a.patterns.iter().all(pola_tanpa_ikatan) { None } else {
                        let [p] = a.patterns.as_slice() else { return None };
                        let mut l = lokal.clone();
                        for (n, t) in self.ikatan_pola(p, ts.as_ref()?)? { l.insert(n, t); }
                        Some(l)
                    };
                    let lokal = dalam.as_ref().unwrap_or(lokal);
                    match &a.body {
                        MatchBody::Expr(e) => self.tipe_ekspresi(e, lokal, subst, nama_diri),
                        MatchBody::Block(b) => self.tipe_ekor_murni(b, lokal, subst, nama_diri),
                    }
                }))
            }

            _ => None,
        }
    }

    /// Nama yang diikat pola `p` atas nilai bertipe `t`, beserta tipenya. `None` kalau bentuk
    /// polanya tidak dipahami di sini — pemanggilnya menyerah, tidak menebak.
    pub(crate) fn ikatan_pola(&self, p: &crate::parser::Pattern, t: &JrgType) -> Option<Vec<(String, JrgType)>> {
        use crate::parser::Pattern;
        match p {
            Pattern::Wildcard(_) | Pattern::Literal { .. } | Pattern::Range { .. } => Some(Vec::new()),
            Pattern::Ident { name, .. } if name.starts_with(char::is_uppercase) => Some(Vec::new()),
            Pattern::Ident { name, .. } => Some(vec![(name.clone(), t.clone())]),
            Pattern::Enum { inner: None, .. } => Some(Vec::new()),
            Pattern::Enum { variant, inner: Some(dalam), .. } => {
                let JrgType::Enum(en) = t else { return None };
                let muatan = match variant.as_str() {
                    "Some" => self.option_args.get(en).cloned()
                        .or_else(|| self.opsi_tertunda.borrow().get(en).cloned())?,
                    "Ok" => self.result_args.get(en)?.0.clone(),
                    "Err" => self.result_args.get(en)?.1.clone(),
                    _ => return None,
                };
                self.ikatan_pola(dalam, &muatan)
            }
            _ => None,
        }
    }

    /// Tipe ekor sebuah blok yang ISINYA cuma ekor — blok berpernyataan digugurkan.
    fn tipe_ekor_murni(&self, b: &Block, lokal: &Lokal, subst: &TypeSubst, nama_diri: &str)
        -> Option<JrgType>
    {
        if !b.stmts.is_empty() { return None; }
        self.tipe_ekspresi(b.tail.as_ref()?, lokal, subst, nama_diri)
    }
}

/// Satu tipe kalau semua kandidat terbaca dan sama; selain itu `None`.
fn sepakat(mut tipe: impl Iterator<Item = Option<JrgType>>) -> Option<JrgType> {
    let pertama = tipe.next()??;
    for t in tipe { if t? != pertama { return None; } }
    Some(pertama)
}

/// Pola yang tidak mengikat nama apa pun: literal, `_`, rentang, varian unit, alternasinya.
fn pola_tanpa_ikatan(p: &crate::parser::Pattern) -> bool {
    use crate::parser::Pattern;
    match p {
        Pattern::Literal { .. } | Pattern::Range { .. } | Pattern::Wildcard(_) => true,
        Pattern::Ident { name, .. } => name.starts_with(char::is_uppercase),
        Pattern::Enum { inner: None, .. } => true,
        Pattern::Or { pats, .. } => pats.iter().all(pola_tanpa_ikatan),
        _ => false,
    }
}
