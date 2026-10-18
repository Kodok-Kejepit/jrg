// src/ruang_tipe.rs — identitas TIPE per unit: satu penulis-ulang AST untuk dua mesin.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::lexer::Span;
use crate::parser::*;

/// Pemisah nama dan nomor di label tipe yang bertabrakan. Kode pemakaian pribadi:
/// tidak bisa ditulis di nama JRG dan bukan bagian teks mana pun.
pub const PENANDA: char = '\u{E000}';

/// Teks tanpa nomor label — yang dilihat pemakai. Tanpa penanda, teksnya dipinjam.
pub fn bersih(teks: &str) -> Cow<'_, str> {
    if !teks.contains(PENANDA) { return Cow::Borrowed(teks); }
    let mut hasil = String::with_capacity(teks.len());
    let mut buang = false;
    for c in teks.chars() {
        if c == PENANDA { buang = true; continue; }
        if buang && c.is_ascii_digit() { continue; }
        buang = false;
        hasil.push(c);
    }
    Cow::Owned(hasil)
}

/// `bersih` untuk teks yang dimiliki: tanpa penanda, tidak ada alokasi baru.
pub fn bersihkan(teks: String) -> String {
    if teks.contains(PENANDA) { bersih(&teks).into_owned() } else { teks }
}

#[derive(Clone, Copy, PartialEq)]
enum Bentuk { Polos, Tuple, Berfield }

#[derive(Default)]
struct Unit {
    nama: String,
    /// Nama yang ditulis → label internal (sama, kecuali namanya sudah diklaim unit lain).
    tipe: HashMap<String, String>,
    /// Varian → enum pemiliknya (nama yang ditulis) dan bentuknya.
    varian: HashMap<String, (String, Bentuk)>,
    impor: Vec<String>,
}

/// Keadaan satu program: siapa pemilik tiap nama tipe telanjang.
#[derive(Default)]
pub struct Ruang {
    pemilik: HashMap<String, String>,
    unit: HashMap<String, Unit>,
    nomor: usize,
}

pub type RuangBersama = Arc<Mutex<Ruang>>;

pub const UNIT_UTAMA: &str = "<utama>";

/// Hasil mencari sebuah nama tipe dari satu unit.
enum Temu { Label(String), Taksa(Vec<String>), Tidak }

impl Ruang {
    /// Daftarkan tipe milik `id`. Yang pertama mengklaim sebuah nama memegangnya
    /// telanjang; unit berikutnya mendapat label bernomor. Dipanggil lagi, nama yang BARU
    /// ditambahkan (REPL mendeklarasikan tipe satu baris demi satu baris).
    pub fn klaim(&mut self, id: &str, nama: &str, ast: &ParsedFile) {
        let mut u = self.unit.remove(id).unwrap_or_else(|| Unit { nama: nama.to_string(), ..Unit::default() });
        for item in &ast.items {
            let n = match item {
                Item::Struct(d) => &d.name,
                Item::Enum(d) => {
                    for v in &d.variants {
                        let bentuk = match v.kind {
                            EnumVariantKind::Unit => Bentuk::Polos,
                            EnumVariantKind::Tuple(_) => Bentuk::Tuple,
                            EnumVariantKind::Struct(_) => Bentuk::Berfield,
                        };
                        u.varian.entry(v.name.clone()).or_insert_with(|| (d.name.clone(), bentuk));
                    }
                    &d.name
                }
                // Trait juga milik unitnya: dulu tiga `trait Suara` di tiga berkas berbagi
                // satu tabel default di acuan, dan `--vm` menjawab lain.
                Item::Trait(d) => &d.name,
                _ => continue,
            };
            if u.tipe.contains_key(n) { continue; }
            let label = match self.pemilik.get(n) {
                Some(p) if p != id => {
                    self.nomor += 1;
                    format!("{}{}{}", n, PENANDA, self.nomor)
                }
                _ => {
                    self.pemilik.insert(n.clone(), id.to_string());
                    n.clone()
                }
            };
            u.tipe.insert(n.clone(), label);
        }
        self.unit.insert(id.to_string(), u);
    }

    /// Tulis ulang `ast` milik unit `id`: tiap nama tipe diganti labelnya. `impor`
    /// adalah `(alias, identitas)` import milik berkas ini sendiri.
    pub fn olah(
        &mut self, id: &str, nama: &str, ast: &mut ParsedFile, impor: &[(String, String)],
    ) -> Result<(), (String, Span)> {
        self.klaim(id, nama, ast);
        if let Some(u) = self.unit.get_mut(id) {
            u.impor = impor.iter().map(|(_, i)| i.clone()).collect();
        }
        let mut p = Penulis { ruang: self, id, impor, ingat: HashMap::new(), galat: None, diri: None, di_lengan: false };
        for item in &mut ast.items { p.item(item); }
        match p.galat { Some(g) => Err(g), None => Ok(()) }
    }

    /// Milik unit ini dulu, lalu import terdekat yang punya nama itu; dua pemilik
    /// di jarak yang sama berarti taksa.
    fn cari(&self, id: &str, nama: &str) -> Temu {
        let Some(u) = self.unit.get(id) else { return Temu::Tidak };
        if let Some(l) = u.tipe.get(nama) { return Temu::Label(l.clone()); }
        let mut lihat: HashSet<&str> = HashSet::from([id]);
        let mut lapis: Vec<&str> = u.impor.iter().map(|s| s.as_str()).collect();
        while !lapis.is_empty() {
            let mut dapat: Vec<(&str, &str)> = Vec::new();
            let mut berikut: Vec<&str> = Vec::new();
            for i in lapis {
                if !lihat.insert(i) { continue; }
                let Some(m) = self.unit.get(i) else { continue };
                if let Some(l) = m.tipe.get(nama) { dapat.push((m.nama.as_str(), l.as_str())); }
                berikut.extend(m.impor.iter().map(|s| s.as_str()));
            }
            match dapat.len() {
                0 => lapis = berikut,
                1 => return Temu::Label(dapat[0].1.to_string()),
                _ => return Temu::Taksa(dapat.iter().map(|(n, _)| n.to_string()).collect()),
            }
        }
        Temu::Tidak
    }
}

/// Kalimat untuk nama tipe yang dimiliki dua modul yang sama dekatnya.
pub fn pesan_taksa(nama: &str, pemilik: &[String]) -> String {
    let daftar: Vec<String> = pemilik.iter().map(|p| format!("`{}`", p)).collect();
    let jalan: Vec<String> = pemilik.iter().map(|p| format!("`{}::{}`", p, nama)).collect();
    format!(
        "nama tipe `{}` taksa di sini: modul {} sama-sama mendeklarasikannya\n\
         bantuan: sebut modulnya — {} (modulnya harus di-`import` berkas ini)",
        nama, daftar.join(" dan "), jalan.join(" atau "))
}

struct Penulis<'a> {
    ruang: &'a Ruang,
    id: &'a str,
    impor: &'a [(String, String)],
    ingat: HashMap<String, Option<String>>,
    galat: Option<(String, Span)>,
    /// Label tipe `impl` yang sedang dijelajahi: arti `Self` di pola.
    diri: Option<String>,
    di_lengan: bool,
}

impl Penulis<'_> {
    /// Label untuk nama telanjang; `None` kalau bukan tipe yang terlihat dari sini.
    fn label(&mut self, nama: &str, span: Span) -> Option<String> {
        if let Some(l) = self.ingat.get(nama) { return l.clone(); }
        let hasil = match self.ruang.cari(self.id, nama) {
            Temu::Label(l) => Some(l),
            Temu::Tidak => None,
            Temu::Taksa(p) => {
                if self.galat.is_none() { self.galat = Some((pesan_taksa(nama, &p), span)); }
                return None;
            }
        };
        self.ingat.insert(nama.to_string(), hasil.clone());
        hasil
    }

    /// Modul di balik sebuah alias milik berkas ini.
    fn modul(&self, alias: &str) -> Option<&Unit> {
        let (_, id) = self.impor.iter().rev().find(|(a, _)| a == alias)?;
        self.ruang.unit.get(id)
    }

    /// Label enum milik unit ini yang punya varian `nama` dengan bentuk yang diminta.
    fn varian_sendiri(&self, nama: &str, bentuk: Bentuk) -> Option<String> {
        let u = self.ruang.unit.get(self.id)?;
        let (e, b) = u.varian.get(nama)?;
        if *b == bentuk { u.tipe.get(e).cloned() } else { None }
    }

    /// Varian milik modul yang diimpor, ditulis TELANJANG di literal atau pola: ditolak sebelum
    /// `main` dengan kalimat varian polos. Dulu `Kotak { s }` cocok di acuan dan tidak di `--vm`.
    fn tolak_varian_modul(&mut self, ty: &TypeExpr, bentuk: Bentuk) {
        let TypeExpr::Named(nt) = ty else { return };
        if nt.package.is_some() || nt.modul.is_some() || self.galat.is_some() { return; }
        if !matches!(self.ruang.cari(self.id, &nt.name), Temu::Tidak) || self.varian_sendiri(&nt.name, bentuk).is_some() { return; }
        for (alias, id) in self.impor {
            let Some(m) = self.ruang.unit.get(id) else { continue };
            let Some((e, b)) = m.varian.get(&nt.name) else { continue };
            if *b != bentuk { continue; }
            let e = bersih(e).into_owned();
            self.galat = Some((crate::interpreter::pesan_varian_modul(&nt.name, &e, alias), nt.span));
            return;
        }
    }

    /// `alias::Nama` di posisi tipe: label tipenya, atau label enum pemilik varian itu.
    fn lewat_alias(&self, alias: &str, nama: &str, varian_saja: bool) -> Option<(Option<String>, String)> {
        let m = self.modul(alias)?;
        if !varian_saja {
            if let Some(l) = m.tipe.get(nama) { return Some((None, l.clone())); }
        }
        let (e, _) = m.varian.get(nama)?;
        Some((m.tipe.get(e).cloned(), nama.to_string()))
    }

    /// Nama tipe di ANOTASI (parameter, hasil, `let`, field, payload): label tipenya kalau
    /// jelas. Yang taksa dibiarkan — anotasi tidak ditegakkan di dua mesin acuan, cuma di LLVM.
    fn anotasi(&mut self, ty: &mut TypeExpr) {
        match ty {
            TypeExpr::Named(nt) => {
                for a in &mut nt.args { self.anotasi(a); }
                match nt.package.clone() {
                    None => if let Temu::Label(l) = self.ruang.cari(self.id, &nt.name) { nt.name = l; },
                    Some(p) => if let Some((None, l)) = self.lewat_alias(&p, &nt.name, false) {
                        nt.package = None;
                        nt.name = l;
                    },
                }
            }
            TypeExpr::Ownership(o) => self.anotasi(&mut o.inner),
            TypeExpr::Pointer(p) => self.anotasi(&mut p.inner),
            TypeExpr::Array(a) => self.anotasi(&mut a.elem),
            TypeExpr::Slice(s) => self.anotasi(&mut s.elem),
            TypeExpr::Map(m) => { self.anotasi(&mut m.key); self.anotasi(&mut m.val); }
            TypeExpr::Chan(c) => self.anotasi(&mut c.elem),
            TypeExpr::Fn(f) => {
                for p in &mut f.params { self.anotasi(p); }
                if let Some(r) = &mut f.ret { self.anotasi(r); }
            }
            TypeExpr::Tuple(v) => for t in v { self.anotasi(t); },
            TypeExpr::Primitive(_) | TypeExpr::Never | TypeExpr::Inferred | TypeExpr::Error(_) => {}
        }
    }

    /// Nama tipe di literal struct, pola struct, kepala `impl`.
    fn tipe_bernama(&mut self, ty: &mut TypeExpr, varian_saja: bool) {
        let TypeExpr::Named(nt) = ty else { return };
        // `alias::Tipe::Varian`: tipenya dicari di modul yang disebut, bukan ditebak.
        if let (Some(alias), Some(p)) = (nt.modul.take(), nt.package.clone()) {
            if let Some(l) = self.modul(&alias).and_then(|m| m.tipe.get(&p)).cloned() {
                nt.package = Some(l);
                return;
            }
        }
        match nt.package.clone() {
            Some(p) if p == "Self" => if let Some(d) = self.diri.clone() { nt.package = Some(d); },
            None => match self.label(&nt.name, nt.span) {
                Some(l) => nt.name = l,
                // Varian milik unit ini yang ditulis telanjang: enum pemiliknya disebut,
                // supaya varian bernama sama di modul lain tidak ikut cocok.
                None => {
                    let bentuk = if varian_saja { Bentuk::Tuple } else { Bentuk::Berfield };
                    if let Some(l) = self.varian_sendiri(&nt.name, bentuk) { nt.package = Some(l); }
                }
            },
            Some(p) => match self.lewat_alias(&p, &nt.name, varian_saja) {
                Some((paket, nama)) => { nt.package = paket; nt.name = nama; }
                None => if let Some(l) = self.label(&p, nt.span) { nt.package = Some(l); },
            },
        }
    }

    fn item(&mut self, item: &mut Item) {
        match item {
            Item::Fn(f) => self.fungsi(f),
            Item::Struct(d) => {
                if let Some(l) = self.label(&d.name, d.span) { d.name = l; }
                if let Some(induk) = d.parent.clone() {
                    if let Some(l) = self.label(&induk, d.span) { d.parent = Some(l); }
                }
                for f in &mut d.fields {
                    self.anotasi(&mut f.ty);
                    if let Some(e) = &mut f.default { self.expr(e); }
                }
            }
            Item::Enum(d) => {
                if let Some(l) = self.label(&d.name, d.span) { d.name = l; }
                for v in &mut d.variants {
                    match &mut v.kind {
                        EnumVariantKind::Struct(fs) => for f in fs {
                            self.anotasi(&mut f.ty);
                            if let Some(e) = &mut f.default { self.expr(e); }
                        },
                        EnumVariantKind::Tuple(ts) => for t in ts { self.anotasi(t); },
                        EnumVariantKind::Unit => {}
                    }
                }
            }
            Item::Trait(t) => {
                if let Some(l) = self.label(&t.name, t.span) { t.name = l; }
                for it in &mut t.items {
                    match it { TraitItem::Method(f) | TraitItem::DefaultMethod(f) => self.fungsi(f) }
                }
            }
            Item::Impl(d) => {
                if let Some(tr) = &mut d.trait_ref { self.tipe_bernama(tr, false); }
                self.tipe_bernama(&mut d.self_type, false);
                self.diri = match &d.self_type { TypeExpr::Named(nt) => Some(nt.name.clone()), _ => None };
                for it in &mut d.items {
                    match it {
                        ImplItem::Method(f) => self.fungsi(f),
                        ImplItem::Const(c) => self.expr(&mut c.value),
                        ImplItem::TypeAlias(_) => {}
                    }
                }
                self.diri = None;
            }
            Item::Const(c) => {
                if let Some(t) = &mut c.ty { self.anotasi(t); }
                self.expr(&mut c.value);
            }
            Item::Var(v) => {
                if let Some(t) = &mut v.ty { self.anotasi(t); }
                if let Some(e) = &mut v.value { self.expr(e); }
            }
            Item::Mod(m) => if let Some(isi) = &mut m.items { for it in isi { self.item(it); } },
            Item::TypeAlias(_) | Item::Extern(_) | Item::Use(_) | Item::Error(_) => {}
        }
    }

    fn fungsi(&mut self, f: &mut FnDecl) {
        self.param(&mut f.params);
        if let Some(r) = &mut f.ret_type { self.anotasi(r); }
        match Arc::make_mut(&mut f.body) {
            FnBody::Block(b) => self.blok(b),
            FnBody::Expr(e) => self.expr(e),
            FnBody::Abstrak => {}
        }
    }

    fn param(&mut self, ps: &mut [FnParam]) {
        for p in ps {
            self.anotasi(&mut p.ty);
            self.pola(&mut p.pattern);
            if let Some(e) = &mut p.default { self.expr(e); }
        }
    }

    fn blok(&mut self, b: &mut Block) {
        for s in &mut b.stmts { self.stmt(s); }
        if let Some(e) = &mut b.tail { self.expr(e); }
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match s {
            Stmt::Let(l) => self.let_(l),
            Stmt::Var(v) => {
                if let Some(t) = &mut v.ty { self.anotasi(t); }
                if let Some(e) = &mut v.init { self.expr(e); }
            }
            Stmt::Const(c) => self.expr(&mut c.value),
            Stmt::Expr(e) => self.expr(e),
            Stmt::Assign(a) => {
                for e in &mut a.targets { self.expr(e); }
                for e in &mut a.values { self.expr(e); }
            }
            Stmt::ShortDecl(d) => for e in &mut d.values { self.expr(e); },
            Stmt::If(i) => self.jika(i),
            Stmt::For(ForStmt::Classic { init, cond, post, body, .. }) => {
                if let Some(s) = init { self.stmt(s); }
                if let Some(e) = cond { self.expr(e); }
                if let Some(s) = post { self.stmt(s); }
                self.blok(body);
            }
            Stmt::For(ForStmt::Range { iter, body, .. }) => { self.expr(iter); self.blok(body); }
            Stmt::While(w) => { self.expr(&mut w.cond); self.blok(&mut w.body); }
            Stmt::Loop(l) => self.blok(&mut l.body),
            Stmt::Match(m) => { self.expr(&mut m.scrutinee); self.lengan(&mut m.arms); }
            Stmt::Return(r) => for e in &mut r.values { self.expr(e); },
            Stmt::Spawn(sp) => {
                if let Some(ps) = &mut sp.params { self.param(ps); }
                self.blok(&mut sp.body);
            }
            Stmt::Select(sel) => {
                for c in &mut sel.cases {
                    match c {
                        SelectCase::Send { chan, val, body, .. } => {
                            self.expr(chan); self.expr(val); self.blok(body);
                        }
                        SelectCase::Recv { chan, body, .. } => { self.expr(chan); self.blok(body); }
                        SelectCase::Let { stmt, body, .. } => { self.let_(stmt); self.blok(body); }
                    }
                }
                if let Some(b) = &mut sel.default { self.blok(b); }
            }
            Stmt::Defer(d) => self.expr(&mut d.expr),
            Stmt::Block(b) => self.blok(b),
            Stmt::Fn(f) => self.fungsi(f),
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::Error(_) => {}
        }
    }

    fn let_(&mut self, l: &mut LetStmt) {
        if let Some(t) = &mut l.ty { self.anotasi(t); }
        self.pola(&mut l.pattern);
        self.expr(&mut l.init);
    }

    fn jika(&mut self, i: &mut IfStmt) {
        if let Some(s) = &mut i.init { self.stmt(s); }
        self.expr(&mut i.cond);
        self.blok(&mut i.then);
        for (init, cond, blok) in &mut i.else_ifs {
            if let Some(s) = init { self.stmt(s); }
            self.expr(cond);
            self.blok(blok);
        }
        if let Some(b) = &mut i.else_ { self.blok(b); }
    }

    fn lengan(&mut self, arms: &mut [MatchArm]) {
        for a in arms {
            for p in &mut a.patterns {
                self.di_lengan = true;
                self.pola(p);
                self.di_lengan = false;
            }
            if let Some(g) = &mut a.guard { self.expr(g); }
            match &mut a.body {
                MatchBody::Expr(e) => self.expr(e),
                MatchBody::Block(b) => self.blok(b),
            }
        }
    }

    fn pola(&mut self, p: &mut Pattern) {
        // Varian polos milik unit ini di lengan `match`: MENGUJI, jadi enumnya disebut —
        // sama dengan menulis `E::V`, dan tidak lagi bergantung siapa yang memuat modulnya.
        if let (true, Pattern::Ident { name, span, .. }) = (self.di_lengan, &*p) {
            if let Some(l) = self.varian_sendiri(name, Bentuk::Polos) {
                let ty = TypeExpr::Named(NamedType {
                    package: Some(l), modul: None, name: name.clone(), args: Vec::new(), span: *span });
                *p = Pattern::Enum { ty, variant: name.clone(), inner: None, span: *span };
                return;
            }
        }
        match p {
            Pattern::Tuple { elems, .. } | Pattern::Or { pats: elems, .. } => {
                for e in elems { self.pola(e); }
            }
            Pattern::Struct { ty, fields, .. } => {
                self.tolak_varian_modul(ty, Bentuk::Berfield);
                self.tipe_bernama(ty, false);
                for f in fields { self.pola(&mut f.pat); }
            }
            Pattern::Enum { ty, inner, .. } => {
                // `Varian(..)` telanjang membawa namanya sendiri sebagai tipe: bukan tipe.
                self.tolak_varian_modul(ty, Bentuk::Tuple);
                match ty {
                    TypeExpr::Named(nt) if nt.package.is_none() => {
                        if let Some(l) = self.varian_sendiri(&nt.name, Bentuk::Tuple) { nt.package = Some(l); }
                    }
                    _ => self.tipe_bernama(ty, true),
                }
                if let Some(i) = inner { self.pola(i); }
            }
            Pattern::Range { lo, hi, .. } => {
                if let Some(x) = lo { self.pola(x); }
                if let Some(x) = hi { self.pola(x); }
            }
            Pattern::Ref { inner, .. } | Pattern::Binding { inner, .. } => self.pola(inner),
            Pattern::Array { before, after, .. } => {
                for e in before { self.pola(e); }
                for e in after { self.pola(e); }
            }
            Pattern::Ident { .. } | Pattern::Literal { .. } | Pattern::Wildcard(_) => {}
        }
    }

    fn opsi(&mut self, e: &mut Option<Box<Expr>>) {
        if let Some(x) = e { self.expr(x); }
    }

    fn expr(&mut self, e: &mut Expr) {
        match e {
            Expr::Literal(..) | Expr::Ident(..) | Expr::Error(_) => {}
            Expr::Unary { expr, .. } | Expr::Cast { expr, .. } | Expr::Field { obj: expr, .. }
            | Expr::Await { expr, .. } | Expr::Propagate { expr, .. }
            | Expr::Recv { chan: expr, .. } => self.expr(expr),
            Expr::Binary { lhs, rhs, .. } | Expr::Pipe { lhs, rhs, .. }
            | Expr::Index { obj: lhs, idx: rhs, .. } | Expr::Send { chan: lhs, val: rhs, .. }
            | Expr::ArrayRepeat { elem: lhs, count: rhs, .. } => { self.expr(lhs); self.expr(rhs); }
            Expr::Ternary { cond, then, else_, .. } => {
                self.expr(cond); self.expr(then); self.expr(else_);
            }
            Expr::Method { obj, args, .. } | Expr::Call { func: obj, args, .. } => {
                self.expr(obj);
                for a in args { self.expr(a); }
            }
            Expr::Slice { obj, lo, hi, .. } => { self.expr(obj); self.opsi(lo); self.opsi(hi); }
            Expr::Closure { params, body, .. } => {
                self.param(params);
                self.expr(Arc::make_mut(body));
            }
            Expr::Block(b) => self.blok(b),
            Expr::Struct { ty, fields, spread, .. } => {
                self.tolak_varian_modul(ty, Bentuk::Berfield);
                self.tipe_bernama(ty, false);
                for f in fields { self.expr(&mut f.value); }
                self.opsi(spread);
            }
            Expr::Array { elems, .. } | Expr::Tuple { elems, .. } => {
                for x in elems { self.expr(x); }
            }
            Expr::Range { lo, hi, .. } => { self.opsi(lo); self.opsi(hi); }
            Expr::MakeChan { cap, .. } => self.opsi(cap),
            Expr::PathExpr { segments, span } => self.jalur(segments, *span),
            Expr::Match { scrutinee, arms, .. } => { self.expr(scrutinee); self.lengan(arms); }
            Expr::If(i) => self.jika(i),
            Expr::Template { parts, .. } => for p in parts {
                if let TemplatePart::Expr(x) = p { self.expr(x); }
            },
        }
    }

    /// `Tipe::x` dan `alias::Tipe::x`. `alias::nama` dua segmen adalah NILAI milik
    /// modulnya, bukan jalur lewat tipe: tidak disentuh.
    fn jalur(&mut self, seg: &mut [String], span: Span) {
        if seg.len() < 2 { return; }
        if seg.len() >= 3 {
            if let Some(l) = self.modul(&seg[0]).and_then(|m| m.tipe.get(&seg[1])).cloned() {
                seg[1] = l;
                return;
            }
        }
        if self.modul(&seg[0]).is_some() { return; }
        let kepala = seg[0].clone();
        if let Some(l) = self.label(&kepala, span) { seg[0] = l; }
    }
}

#[cfg(test)]
mod uji {
    use super::*;

    #[test]
    fn bersih_cuma_membuang_nomor_label() {
        let l = format!("Titik{}12", PENANDA);
        assert_eq!(bersih(&l), "Titik");
        assert_eq!(bersih(&format!("struct:{}::nama 12", l)), "struct:Titik::nama 12");
        assert!(matches!(bersih("Titik#12"), Cow::Borrowed(_)));
    }
}
