// src/llvm/infer_tumbuh.rs — lokal yang tumbuh dan calon tipenya

use std::collections::HashMap;

use crate::parser::*;

use super::codegen::Codegen;
use super::infer::*;
use super::sema::TypeSubst;
use super::types::JrgType;

impl Codegen {
    /// Lokal mana yang harus jadi ARRAY BERTUMBUH, dan tipe elemennya.
    pub fn lokal_yang_tumbuh(&self, decl: &FnDecl, subst: &TypeSubst)
        -> HashMap<String, JrgType>
    {
        let FnBody::Block(body) = &*decl.body else { return HashMap::new() };

        let mut lokal = Lokal::new();
        for (i, p) in decl.params.iter().enumerate() {
            if let Some(nama) = pola_nama(&p.pattern) {
                if let Some(t) = self.tipe_dari_anotasi(&p.ty, subst)
                    .or_else(|| subst.get(&format!("$param{}", i)).cloned())
                {
                    lokal.insert(nama, t);
                }
            }
        }

        let mut calon: HashMap<String, Option<JrgType>> = HashMap::new();
        let mut ditugasi: HashMap<String, Vec<(Expr, Option<JrgType>)>> = HashMap::new();
        self.kumpul_calon(body, &mut lokal, subst, &decl.name, &mut calon, &mut ditugasi);
        let calon2 = calon.clone();

        let mut hasil = HashMap::new();
        // Beberapa putaran: lokal bertumbuh yang isinya lokal bertumbuh LAIN baru terbaca
        // sesudah yang di dalam punya tipe.
        for _putaran in 0..3 {
        let sebelum = hasil.len();
        for (nama, elem_awal) in calon.clone() {
            if hasil.contains_key(&nama) { continue; }
            let Some(nilai) = ditugasi.get(&nama) else { continue };
            if nilai.is_empty() {
                continue;
            }
            // Tipe elemennya diambil dari penugasan mana pun yang bisa dibaca.
            let mut elem = elem_awal;
            for (e, t_saat_itu) in nilai {
                if elem.is_some() { break; }
                // Tipe yang dicatat SAAT penelusuran lebih dipercaya daripada
                // yang dibaca ulang sekarang: `lokal` sudah berubah sejak itu.
                let t = t_saat_itu.clone()
                    .or_else(|| self.tipe_ekspresi(e, &lokal, subst, &decl.name));
                if let Some(t) = t {
                    elem = match t {
                        JrgType::Array(x, _) | JrgType::Slice(x) => Some(*x),
                        _ => None,
                    };
                }
            }
            if let Some(t) = elem {
                // Elemen berukuran tetap diluruhkan: yang didorong belakangan boleh beda panjang.
                let t = match t {
                    JrgType::Array(x, _) => JrgType::Slice(x),
                    lain => lain,
                };
                lokal.insert(nama.clone(), JrgType::Slice(Box::new(t.clone())));
                hasil.insert(nama, JrgType::Slice(Box::new(t)));
            }
        }
        if hasil.len() == sebelum { break; }
        }
        // `var a = []` yang ditugasi ulang tapi elemennya tak terbaca dari mana pun: array bertag.
        for (nama, elem_awal) in &calon {
            let ditulis = ditugasi.get(nama).map(|v| !v.is_empty()).unwrap_or(false);
            if elem_awal.is_none() && ditulis && !hasil.contains_key(nama) {
                hasil.insert(nama.clone(), JrgType::Slice(Box::new(JrgType::Any)));
            }
        }

        // Lokal yang ELEMENNYA ditumbuhkan.
        for (kunci, _) in &ditugasi {
            let Some(nama) = kunci.strip_prefix("[]") else { continue };
            if hasil.contains_key(nama) { continue; }
            let Some(Some(elem)) = calon2.get(nama) else { continue };
            let dalam = match elem {
                JrgType::Array(x, _) | JrgType::Slice(x) => (**x).clone(),
                _ => continue,
            };
            hasil.insert(nama.to_string(),
                JrgType::Slice(Box::new(JrgType::Slice(Box::new(dalam)))));
        }
        hasil
    }

    /// Kumpulkan calon lokal bertumbuh beserta penugasan ulangnya.
    pub(crate) fn kumpul_calon(
        &self,
        b: &Block,
        lokal: &mut Lokal,
        subst: &TypeSubst,
        nama_diri: &str,
        calon: &mut HashMap<String, Option<JrgType>>,
        ditugasi: &mut HashMap<String, Vec<(Expr, Option<JrgType>)>>,
    ) {
        for st in &b.stmts {
            // Tipe lokal biasa dicatat lebih dulu, supaya statement SESUDAHNYA bisa
            // membacanya.
            match st {
                Stmt::Var(v) => {
                    if let Some(t) = v.ty.as_ref().and_then(|te| self.tipe_dari_anotasi(te, subst))
                        .or_else(|| v.init.as_ref()
                            .and_then(|e| self.tipe_ekspresi(e, lokal, subst, nama_diri)))
                    {
                        lokal.insert(v.name.clone(), t);
                    }
                }
                Stmt::Let(l) => {
                    if let Some(nama) = pola_nama(&l.pattern) {
                        if let Some(t) = l.ty.as_ref().and_then(|te| self.tipe_dari_anotasi(te, subst))
                            .or_else(|| self.tipe_ekspresi(&l.init, lokal, subst, nama_diri))
                        {
                            lokal.insert(nama, t);
                        }
                    }
                }
                _ => {}
            }
            match st {
                Stmt::Var(v) if v.ty.is_none() => {
                    if let Some(init) = &v.init {
                        let arr = matches!(**init, Expr::Array { .. })
                            || matches!(self.tipe_ekspresi(init, lokal, subst, nama_diri),
                                        Some(JrgType::Array(_, _)) | Some(JrgType::Slice(_)));
                        if arr {
                            let elem = match self.tipe_ekspresi(init, lokal, subst, nama_diri) {
                                // Array KOSONG tidak memberi tahu tipe
                                // elemennya — justru itu yang harus dicari
                                // dari penugasan berikutnya.
                                Some(JrgType::Array(_, 0)) => None,
                                Some(JrgType::Array(e, _)) | Some(JrgType::Slice(e)) => Some(*e),
                                _ => None,
                            };
                            calon.insert(v.name.clone(), elem);
                        }
                    }
                }
                Stmt::Assign(a) => {
                    for (t, val) in a.targets.iter().zip(a.values.iter()) {
                        match t {
                            Expr::Ident(n, _) => {
                                if calon.contains_key(n) {
                                    // Tipenya dibaca DI SINI, bukan belakangan.
                                    let t = self.tipe_ekspresi(val, lokal, subst, nama_diri);
                                    ditugasi.entry(n.clone()).or_default().push((val.clone(), t));
                                }
                            }
                            // `l12[0] = l12[0] + [2]` menumbuhkan ELEMEN, bukan `l12` sendiri.
                            Expr::Index { obj, .. } => {
                                if let Expr::Ident(n, _) = &**obj {
                                    if calon.contains_key(n) {
                                        ditugasi.entry(format!("[]{}", n)).or_default()
                                            .push((val.clone(), None));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Stmt::Block(bb) => self.kumpul_calon(bb, lokal, subst, nama_diri, calon, ditugasi),
                Stmt::If(i) => {
                    self.kumpul_calon(&i.then, lokal, subst, nama_diri, calon, ditugasi);
                    for (_, _, blk) in &i.else_ifs {
                        self.kumpul_calon(blk, lokal, subst, nama_diri, calon, ditugasi);
                    }
                    if let Some(e) = &i.else_ {
                        self.kumpul_calon(e, lokal, subst, nama_diri, calon, ditugasi);
                    }
                }
                Stmt::While(w) => self.kumpul_calon(&w.body, lokal, subst, nama_diri, calon, ditugasi),
                Stmt::Loop(l) => self.kumpul_calon(&l.body, lokal, subst, nama_diri, calon, ditugasi),
                Stmt::For(f) => match f {
                    ForStmt::Classic { body, .. } =>
                        self.kumpul_calon(body, lokal, subst, nama_diri, calon, ditugasi),
                    ForStmt::Range { vars, iter, body, .. } => {
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
                        self.kumpul_calon(body, lokal, subst, nama_diri, calon, ditugasi)
                    }
                },
                Stmt::Match(m) => {
                    let ts = self.tipe_ekspresi(&m.scrutinee, lokal, subst, nama_diri);
                    for arm in &m.arms {
                        if let MatchBody::Block(bb) = &arm.body {
                            // Nama yang diikat pola ikut dikenal di badan lengannya.
                            let mut dalam = lokal.clone();
                            if let ([p], Some(t)) = (arm.patterns.as_slice(), ts.as_ref()) {
                                for (n, tn) in self.ikatan_pola(p, t).unwrap_or_default() { dalam.insert(n, tn); }
                            }
                            self.kumpul_calon(bb, &mut dalam, subst, nama_diri, calon, ditugasi);
                        }
                    }
                }
                // Sebuah `if`/`match`/blok yang dipakai sebagai NILAI
                // muncul di sini, bukan di `tail` — dan penugasan di
                // dalamnya sama nyatanya.
                Stmt::Expr(e) => self.kumpul_calon_ekspr(e, lokal, subst, nama_diri, calon, ditugasi),
                // Inisialisasi `let`/`var` juga bisa memuat penugasan: sebuah
                // closure yang menumbuhkan tangkapan (`let f = || { g = g + [1] }`)
                // duduk persis di sini.
                Stmt::Let(l) =>
                    self.kumpul_calon_ekspr(&l.init, lokal, subst, nama_diri, calon, ditugasi),
                Stmt::Var(v) => {
                    if let Some(e) = &v.init {
                        self.kumpul_calon_ekspr(e, lokal, subst, nama_diri, calon, ditugasi);
                    }
                }
                Stmt::Return(r) => {
                    for v in &r.values {
                        self.kumpul_calon_ekspr(v, lokal, subst, nama_diri, calon, ditugasi);
                    }
                }
                _ => {}
            }
        }
        // Ekor blok bukan pelengkap: `for i in ..
        if let Some(t) = &b.tail {
            self.kumpul_calon_ekspr(t, lokal, subst, nama_diri, calon, ditugasi);
        }
    }

    /// Telusuri blok-blok yang bersembunyi di dalam sebuah ekspresi.
    pub(crate) fn kumpul_calon_ekspr(
        &self,
        e: &Expr,
        lokal: &mut Lokal,
        subst: &TypeSubst,
        nama_diri: &str,
        calon: &mut HashMap<String, Option<JrgType>>,
        ditugasi: &mut HashMap<String, Vec<(Expr, Option<JrgType>)>>,
    ) {
        match e {
            Expr::Block(b) => self.kumpul_calon(b, lokal, subst, nama_diri, calon, ditugasi),
            Expr::If(i) => {
                self.kumpul_calon(&i.then, lokal, subst, nama_diri, calon, ditugasi);
                for (_, _, blk) in &i.else_ifs {
                    self.kumpul_calon(blk, lokal, subst, nama_diri, calon, ditugasi);
                }
                if let Some(x) = &i.else_ {
                    self.kumpul_calon(x, lokal, subst, nama_diri, calon, ditugasi);
                }
            }
            Expr::Match { scrutinee, arms, .. } => {
                let ts = self.tipe_ekspresi(scrutinee, lokal, subst, nama_diri);
                for arm in arms {
                    // Nama yang diikat pola ikut dikenal di badan lengannya.
                    let mut dalam = lokal.clone();
                    if let ([p], Some(t)) = (arm.patterns.as_slice(), ts.as_ref()) {
                        for (n, tn) in self.ikatan_pola(p, t).unwrap_or_default() { dalam.insert(n, tn); }
                    }
                    match &arm.body {
                        MatchBody::Block(b) =>
                            self.kumpul_calon(b, &mut dalam, subst, nama_diri, calon, ditugasi),
                        MatchBody::Expr(x) =>
                            self.kumpul_calon_ekspr(x, &mut dalam, subst, nama_diri, calon, ditugasi),
                    }
                }
            }
            // Closure menangkap PENGIKATAN, jadi `g = g + [x]` di dalam badannya
            // menumbuhkan `g` milik fungsi LUAR.
            Expr::Closure { body, .. } =>
                self.kumpul_calon_ekspr(body, lokal, subst, nama_diri, calon, ditugasi),
            Expr::Call { args, .. } | Expr::Method { args, .. } => {
                for a in args {
                    self.kumpul_calon_ekspr(a, lokal, subst, nama_diri, calon, ditugasi);
                }
            }
            Expr::Binary { lhs, rhs, .. } => {
                self.kumpul_calon_ekspr(lhs, lokal, subst, nama_diri, calon, ditugasi);
                self.kumpul_calon_ekspr(rhs, lokal, subst, nama_diri, calon, ditugasi);
            }
            _ => {}
        }
    }
}

/// Gabungkan sebuah kandidat ke kesimpulan yang sedang dikumpulkan.
pub(crate) fn pilih(ketemu: &mut Option<JrgType>, kandidat: JrgType) {
    match ketemu {
        None => *ketemu = Some(kandidat),
        Some(JrgType::Never) => {}
        Some(lama) if *lama == kandidat => {}
        Some(_) => *ketemu = Some(JrgType::Never),
    }
}

/// Nama tunggal sebuah pola, kalau polanya memang cuma satu nama.
pub(crate) fn pola_nama(p: &Pattern) -> Option<String> {
    match p {
        Pattern::Ident { name, .. } => Some(name.clone()),
        _ => None,
    }
}

/// Apakah blok ini memuat `return <ekspresi>` di mana pun?
pub fn blok_punya_return_bernilai(b: &crate::parser::Block) -> bool {
    use crate::parser::{Expr, ForStmt, MatchBody, Stmt};

    pub(crate) fn di_ekspr(e: &Expr) -> bool {
        match e {
            Expr::Closure { .. } => false,
            Expr::Block(b) => blok_punya_return_bernilai(b),
            Expr::If(i) => {
                blok_punya_return_bernilai(&i.then)
                    || i.else_ifs.iter().any(|(_, _, b)| blok_punya_return_bernilai(b))
                    || i.else_.as_ref().map_or(false, blok_punya_return_bernilai)
            }
            Expr::Match { arms, .. } => arms.iter().any(|a| match &a.body {
                MatchBody::Block(b) => blok_punya_return_bernilai(b),
                MatchBody::Expr(e) => di_ekspr(e),
            }),
            _ => false,
        }
    }
    pub(crate) fn di_stmt(s: &Stmt) -> bool {
        match s {
            Stmt::Return(r) => !r.values.is_empty(),
            Stmt::Block(b) => blok_punya_return_bernilai(b),
            Stmt::If(i) => {
                blok_punya_return_bernilai(&i.then)
                    || i.else_ifs.iter().any(|(_, _, b)| blok_punya_return_bernilai(b))
                    || i.else_.as_ref().map_or(false, blok_punya_return_bernilai)
            }
            Stmt::While(w) => blok_punya_return_bernilai(&w.body),
            Stmt::For(ForStmt::Classic { body, .. })
            | Stmt::For(ForStmt::Range { body, .. }) => blok_punya_return_bernilai(body),
            Stmt::Loop(l) => blok_punya_return_bernilai(&l.body),
            Stmt::Match(m) => m.arms.iter().any(|a| match &a.body {
                MatchBody::Block(b) => blok_punya_return_bernilai(b),
                MatchBody::Expr(e) => di_ekspr(e),
            }),
            Stmt::Expr(e) => di_ekspr(e),
            _ => false,
        }
    }
    b.stmts.iter().any(di_stmt) || b.tail.as_deref().map_or(false, di_ekspr)
}
