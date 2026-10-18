// src/llvm/sema.rs — jrgc LLVM backend — src/llvm/sema.rs

#![allow(dead_code)]

use std::collections::HashMap;
use std::rc::Rc;

use crate::parser::{
    ConstDecl, EnumDecl, FnDecl, Item, ImplDecl, ImplItem, NamedType, ParsedFile, PrimTy,
    ExternItem, StructDecl, StructField, TypeExpr, VarDeclItem,
};

use super::types::JrgType;

pub type TypeSubst = HashMap<String, JrgType>;

#[derive(Clone)]
pub struct GlobalTables {
    pub structs: HashMap<String, StructDecl>,
    pub enums:   HashMap<String, EnumDecl>,
    // `Rc`, not an owned `FnDecl`: every call-expression codegen'd looks its
    // target up here and `.cloned()`s it (see `call_named_function`/ …
    pub functions: HashMap<String, Rc<FnDecl>>,
    /// self-type name -> method name -> FnDecl
    pub impls: HashMap<String, HashMap<String, Rc<FnDecl>>>,
    pub consts: HashMap<String, ConstDecl>,
    /// `var` tingkat modul, dalam URUTAN DEKLARASI.
    pub vars: Vec<VarDeclItem>,
    /// Namespace yang di-`import` berkas ini.
    pub imports: Vec<String>,
    /// Per unit: alias -> nomor unit (Fase MODUL-LLVM). Indeks 0 berkas utama.
    pub alias_unit: Vec<HashMap<String, usize>>,
    /// Tipe yang dideklarasikan atau di-`impl` unit N > 0: method-nya berjalan di unit itu.
    pub unit_tipe: HashMap<String, usize>,
    /// (tipe, method) -> nama parameter tipe yang ditulis kepala `impl`-nya (`impl<U> Kotak<U>`).
    pub param_impl: HashMap<(String, String), Vec<String>>,
    /// Unit yang sedang dikumpulkan `collect_items`.
    unit_kumpul: usize,
    /// Nama fungsi yang dideklarasikan di blok `extern`.
    pub extern_fns: Vec<String>,
    /// Nama `extern fn` -> deskripsinya (`ffi::native::deskripsi_extern`), untuk dipanggil lewat jembatan.
    pub extern_desk: HashMap<String, String>,
    /// Nama trait — cuma untuk menjelaskan `T` yang dipakai sebagai nilai.
    pub traits: Vec<String>,
    /// Method DEFAULT tiap trait, dan pasangan (tipe, trait) dari `impl Trait for Tipe`.
    default_trait: HashMap<String, Vec<Rc<FnDecl>>>,
    impl_trait: Vec<(String, String)>,
}

impl GlobalTables {
    pub fn new() -> Self {
        GlobalTables {
            structs: HashMap::new(),
            enums: HashMap::new(),
            functions: HashMap::new(),
            imports: Vec::new(),
            alias_unit: Vec::new(),
            unit_tipe: HashMap::new(),
            param_impl: HashMap::new(),
            unit_kumpul: 0,
            impls: HashMap::new(),
            consts: HashMap::new(),
            vars: Vec::new(),
            extern_fns: Vec::new(),
            extern_desk: HashMap::new(),
            traits: Vec::new(),
            default_trait: HashMap::new(),
            impl_trait: Vec::new(),
        }
    }

    /// One flat pass over top-level items (and one level of inline `mod` blocks,
    /// flattened into the same namespace — v1 doesn't model real module scoping).
    pub fn collect(&mut self, file: &ParsedFile) {
        for import in &file.imports {
            let ns = import.alias.clone().unwrap_or_else(|| {
                crate::module::ModuleResolver::module_name(&import.path)
            });
            if !self.imports.contains(&ns) {
                self.imports.push(ns);
            }
        }
        self.collect_items(&file.items);
        self.tutup_trait();
    }

    /// Kumpulkan deklarasi sebuah MODUL (unit `n` > 0): nilai tingkat atasnya masuk dengan
    /// kunci ber-awalan `mN$`; tipe tetap satu ruang, dan yang pertama terdaftar menang.
    pub fn collect_unit(&mut self, file: &ParsedFile, n: usize) {
        self.unit_kumpul = n;
        self.collect_items(&file.items);
        self.unit_kumpul = 0;
        self.tutup_trait();
    }

    fn tutup_trait(&mut self) {
        // Default trait masuk SESUDAH semua item terbaca dan tidak menimpa method tipenya sendiri;
        // trait milik modul terdaftar sesudah `main`, jadi pasangan yang belum ketemu menunggu.
        let mut tunggu = Vec::new();
        for (tipe, tr) in std::mem::take(&mut self.impl_trait) {
            let Some(daftar) = self.default_trait.get(&tr).cloned() else { tunggu.push((tipe, tr)); continue };
            let bucket = self.impls.entry(tipe).or_insert_with(HashMap::new);
            for f in daftar {
                bucket.entry(f.name.clone()).or_insert(f);
            }
        }
        self.impl_trait = tunggu;
    }

    fn collect_items(&mut self, items: &[Item]) {
        for item in items {
            match item {
                Item::Fn(f) => {
                    self.functions.insert(super::modul::kunci(self.unit_kumpul, &f.name), Rc::new(f.clone()));
                }
                Item::Struct(s) => {
                    if self.unit_kumpul > 0 {
                        if self.structs.contains_key(&s.name) { continue; }
                        self.unit_tipe.insert(s.name.clone(), self.unit_kumpul);
                    }
                    self.structs.insert(s.name.clone(), s.clone());
                }
                Item::Enum(e) => {
                    if self.unit_kumpul > 0 {
                        if self.enums.contains_key(&e.name) { continue; }
                        self.unit_tipe.insert(e.name.clone(), self.unit_kumpul);
                    }
                    self.enums.insert(e.name.clone(), e.clone());
                }
                Item::Trait(t) => {
                    self.traits.push(t.name.clone());
                    let d: Vec<Rc<FnDecl>> = t.items.iter().filter_map(|i| match i {
                        crate::parser::TraitItem::DefaultMethod(f) => Some(Rc::new(f.clone())),
                        _ => None,
                    }).collect();
                    self.default_trait.insert(t.name.clone(), d);
                }
                Item::Const(c) => { self.consts.insert(super::modul::kunci(self.unit_kumpul, &c.name), c.clone()); }
                Item::Var(v) => {
                    let mut v = v.clone();
                    v.name = super::modul::kunci(self.unit_kumpul, &v.name);
                    self.vars.push(v);
                }
                Item::Impl(im) => self.collect_impl(im),
                Item::Extern(eb) => {
                    for it in &eb.items {
                        if let ExternItem::Fn { name, params, ret, .. } = it {
                            self.extern_fns.push(name.clone());
                            self.extern_desk.insert(name.clone(), crate::ffi::native::deskripsi_extern(
                                &eb.abi, eb.lib.as_deref(), name, params, ret));
                        }
                    }
                }
                Item::Mod(m) => {
                    if let Some(inner) = &m.items {
                        self.collect_items(inner);
                    }
                }
                _ => {} // Trait, TypeAlias, Var, Extern, Use, Error — not part of v1's core path
            }
        }
    }

    fn collect_impl(&mut self, im: &ImplDecl) {
        let self_name = match type_expr_base_name(&im.self_type) {
            Some(n) => n,
            None => return,
        };
        if let Some(tr) = im.trait_ref.as_ref().and_then(type_expr_base_name) {
            self.impl_trait.push((self_name.clone(), tr));
        }
        if self.unit_kumpul > 0 { self.unit_tipe.entry(self_name.clone()).or_insert(self.unit_kumpul); }
        let param: Vec<String> = match &im.self_type {
            TypeExpr::Named(nt) => nt.args.iter().filter_map(type_expr_base_name).collect(),
            _ => Vec::new(),
        };
        for item in &im.items {
            if let (ImplItem::Method(f), false) = (item, param.is_empty()) {
                self.param_impl.insert((self_name.clone(), f.name.clone()), param.clone());
            }
        }
        let bucket = self.impls.entry(self_name).or_insert_with(HashMap::new);
        for item in &im.items {
            if let ImplItem::Method(f) = item {
                bucket.insert(f.name.clone(), Rc::new(f.clone()));
            }
        }
    }

    /// Silsilah sebuah struct: dirinya dulu, lalu induknya ke atas.
    pub fn silsilah(&self, name: &str) -> Vec<String> {
        let mut hasil: Vec<String> = Vec::new();
        // Instans struct generik (`Kotak$i64`) dicari lewat nama yang ditulis pemakainya.
        let mut kini = Some(super::generik::dasar(name).to_string());
        while let Some(n) = kini {
            if hasil.iter().any(|x| x == &n) { break; }
            kini = self.structs.get(&n).and_then(|d| d.parent.clone());
            hasil.push(n);
        }
        hasil
    }

    /// Field sebuah struct SESUDAH warisan diratakan, sebagai (pemilik, field):
    /// milik leluhur paling atas dulu, lalu turun, lalu miliknya sendiri.
    pub fn field_rata(&self, name: &str) -> Vec<(String, StructField)> {
        let mut hasil: Vec<(String, StructField)> = Vec::new();
        for t in self.silsilah(name).into_iter().rev() {
            let Some(d) = self.structs.get(&t) else { continue };
            for f in &d.fields {
                match hasil.iter().position(|(_, x)| x.name == f.name) {
                    Some(i) => hasil[i] = (t.clone(), f.clone()),
                    None => hasil.push((t.clone(), f.clone())),
                }
            }
        }
        hasil
    }

    /// Leluhur yang tidak bisa diratakan, kalau ada — yaitu leluhur GENERIK,
    /// karena struct generik dilewati pendaftaran layout di backend ini.
    pub fn leluhur_bermasalah(&self, name: &str) -> Option<(String, &'static str)> {
        let name = super::generik::dasar(name);
        let mut kini = self.structs.get(name).and_then(|d| d.parent.clone());
        let mut lihat: Vec<String> = vec![name.to_string()];
        while let Some(n) = kini {
            if lihat.iter().any(|x| x == &n) { break; }
            match self.structs.get(&n) {
                None => return None,
                Some(d) if !d.generics.params.is_empty() =>
                    return Some((n, "generik, dan mewarisi struct generik belum didukung backend ini")),
                Some(d) => kini = d.parent.clone(),
            }
            lihat.push(n);
        }
        None
    }

    /// Method milik tipe itu sendiri menang; sesudah itu naik ke induknya.
    pub fn find_method(&self, self_ty_name: &str, method: &str) -> Option<&Rc<FnDecl>> {
        for t in self.silsilah(self_ty_name) {
            if let Some(d) = self.impls.get(&t).and_then(|m| m.get(method)) {
                return Some(d);
            }
        }
        None
    }
}

/// Best-effort "what named type is this" — used to bucket `impl` blocks by
/// their self type without needing a fully resolved JrgType yet.
pub fn type_expr_base_name(te: &TypeExpr) -> Option<String> {
    match te {
        TypeExpr::Named(NamedType { name, .. }) => Some(name.clone()),
        _ => None,
    }
}

// ─── TypeExpr -> JrgType ────────────────────────────────────────────────────

pub struct TypeResolveCtx<'a> {
    pub tables: &'a GlobalTables,
    /// Names currently bound to a concrete type: generic function type
    /// parameters mid-instantiation, plus `Option`/`Result`'s own T/E
    /// while resolving their built-in method bodies.
    pub subst: &'a TypeSubst,
}

pub fn resolve_prim(p: &PrimTy) -> JrgType {
    match p {
        PrimTy::I8 => JrgType::I8, PrimTy::I16 => JrgType::I16, PrimTy::I32 => JrgType::I32,
        PrimTy::I64 => JrgType::I64, PrimTy::I128 => JrgType::I128, PrimTy::Isize => JrgType::I64,
        PrimTy::U8 => JrgType::U8, PrimTy::U16 => JrgType::U16, PrimTy::U32 => JrgType::U32,
        PrimTy::U64 => JrgType::U64, PrimTy::U128 => JrgType::U128, PrimTy::Usize => JrgType::U64,
        PrimTy::Int => JrgType::I64, PrimTy::Uint => JrgType::U64,
        // `f32` di JRG anotasi, bukan tempat penyimpanan: acuannya memegang
        // `f64`, dan yang membulatkan ke 32 bit cuma batas FFI.
        PrimTy::F32 => JrgType::F64, PrimTy::F64 => JrgType::F64,
        PrimTy::F128 => JrgType::F64, // narrowed; f128 has no dedicated codegen path in v1
        PrimTy::C64 => JrgType::F32,  PrimTy::C128 => JrgType::F64, // placeholders; complex numbers unsupported (flagged where actually used)
        PrimTy::Bool => JrgType::Bool,
        PrimTy::Byte => JrgType::U8,
        PrimTy::Rune => JrgType::Rune,
        PrimTy::Str | PrimTy::String_ => JrgType::Str,
        PrimTy::Unit => JrgType::Unit,
    }
}

/// Resolve a parsed `TypeExpr` to a concrete `JrgType`.
pub fn resolve_type_expr(te: &TypeExpr, ctx: &TypeResolveCtx) -> Result<JrgType, String> {
    match te {
        TypeExpr::Primitive(p) => Ok(resolve_prim(p)),
        TypeExpr::Inferred => Err("tipe tidak bisa disimpulkan di posisi ini (butuh anotasi eksplisit)".into()),
        TypeExpr::Never => Ok(JrgType::Never),
        TypeExpr::Tuple(elems) => {
            let mut out = Vec::with_capacity(elems.len());
            for e in elems { out.push(resolve_type_expr(e, ctx)?); }
            Ok(JrgType::Tuple(out))
        }
        TypeExpr::Array(a) => {
            let elem = resolve_type_expr(&a.elem, ctx)?;
            let n = const_eval_array_len(&a.len)?;
            Ok(JrgType::Array(Box::new(elem), n))
        }
        TypeExpr::Slice(s) => {
            let elem = resolve_type_expr(&s.elem, ctx)?;
            Ok(JrgType::Slice(Box::new(elem)))
        }
        TypeExpr::Pointer(p) => resolve_type_expr(&p.inner, ctx), // v1: raw pointers behave like their pointee's owning value; see mod.rs notes
        TypeExpr::Ownership(o) => resolve_type_expr(&o.inner, ctx), // ownership qualifiers parsed, not yet semantically enforced (see LIMITATIONS)
        TypeExpr::Map(_) => Err("tipe `map` belum didukung backend LLVM".into()),
        TypeExpr::Chan(c) => Ok(JrgType::Chan(Box::new(resolve_type_expr(&c.elem, ctx)?))),
        TypeExpr::Fn(f) => {
            if !f.generics.params.is_empty() {
                return Err("tipe fungsi generik belum didukung backend LLVM".into());
            }
            let mut ps = Vec::with_capacity(f.params.len());
            for p in &f.params {
                ps.push(resolve_type_expr(p, ctx)?);
            }
            let ret = match &f.ret {
                Some(r) => resolve_type_expr(r, ctx)?,
                None => JrgType::Unit,
            };
            Ok(JrgType::Fn(ps, Box::new(ret)))
        }
        TypeExpr::Error(_) => Err("tipe tidak valid (parse error)".into()),
        TypeExpr::Named(nt) => resolve_named(nt, ctx),
    }
}

fn resolve_named(nt: &NamedType, ctx: &TypeResolveCtx) -> Result<JrgType, String> {
    let name = nt.name.as_str();

    // Bound generic parameter (mid-instantiation of a generic fn, or
    // Option/Result's own T/E while lowering a built-in method body).
    if nt.args.is_empty() {
        if let Some(bound) = ctx.subst.get(name) {
            return Ok(bound.clone());
        }
    }

    match name {
        "Atomic" | "Mutex" if nt.args.is_empty() && !ctx.tables.structs.contains_key(name) =>
            Ok(JrgType::Sinkron(name == "Mutex")),
        "array" if nt.args.is_empty() => {
            // Sentinel: pemanggil yang menangani `array` telanjang (parameter, tipe
            // return) menggantinya dengan variabel elemen sintetis sebelum sampai ke …
            // Di posisi lain (field struct, isi `Option`, elemen) ia array bertag, seperti `dict`.
            Ok(JrgType::Slice(Box::new(JrgType::Any)))
        }
        // `dict` telanjang berisi nilai bertag; `dict<V>` dan `dict<string, V>` menyebut V.
        "dict" => match nt.args.len() {
            0 => Ok(JrgType::Dict(Box::new(JrgType::Any))),
            n @ (1 | 2) => Ok(JrgType::Dict(Box::new(resolve_type_expr(&nt.args[n - 1], ctx)?))),
            _ => Err("`dict<V>` butuh satu tipe nilai (atau `dict<string, V>`)".into()),
        },
        "Option" => {
            if nt.args.len() != 1 {
                return Err("Option<T> butuh tepat satu parameter tipe".into());
            }
            let t = resolve_type_expr(&nt.args[0], ctx)?;
            Ok(JrgType::Enum(super::mangle::mangle_option(&t))) // layout registered lazily by Codegen::instantiate_option
        }
        "Result" => {
            if nt.args.len() != 2 {
                return Err("Result<T, E> butuh tepat dua parameter tipe".into());
            }
            let t = resolve_type_expr(&nt.args[0], ctx)?;
            let e = resolve_type_expr(&nt.args[1], ctx)?;
            Ok(JrgType::Enum(super::mangle::mangle_result(&t, &e)))
        }
        "int" => Ok(JrgType::I64),
        "uint" => Ok(JrgType::U64),
        "float" => Ok(JrgType::F64),
        "bool" => Ok(JrgType::Bool),
        "byte" => Ok(JrgType::U8),
        "rune" => Ok(JrgType::I32),
        "str" | "string" | "String" => Ok(JrgType::Str),
        // Handle milik `jrgc` (lewat jembatan) dan nama `typeof` yang tidak punya tata letak di sini:
        // nilainya bertag, tipenya dibawa saat jalan.
        "Canvas" | "canvas" | "tuple" | "function" | "closure" | "native_fn" => Ok(JrgType::Any),
        _ => {
            if let Some(s) = ctx.tables.structs.get(name) {
                if !s.generics.params.is_empty() {
                    // Tata letaknya didaftarkan `Codegen::instantiate_struct`; di sini cuma namanya.
                    if nt.args.len() != s.generics.params.len() {
                        return Err(format!(
                            "struct `{}` generik: tulis parameter tipenya, mis. `{}<int>` ({} parameter)",
                            name, name, s.generics.params.len()));
                    }
                    let mut args = Vec::with_capacity(nt.args.len());
                    for a in &nt.args { args.push(resolve_type_expr(a, ctx)?); }
                    return Ok(JrgType::Struct(super::generik::nama_instans(name, &args)));
                }
                return Ok(JrgType::Struct(name.to_string()));
            }
            if let Some(en) = ctx.tables.enums.get(name) {
                if !en.generics.params.is_empty() {
                    return Err(format!(
                        "enum generik `{}` belum didukung backend LLVM (selain Option/Result)", name));
                }
                return Ok(JrgType::Enum(name.to_string()));
            }
            Err(format!("tipe `{}` tidak dikenal", name))
        }
    }
}

/// `[N]T` array lengths are, syntactically, arbitrary expressions — but v1
/// only supports the overwhelmingly common case of a plain integer literal …
fn const_eval_array_len(e: &crate::parser::Expr) -> Result<u64, String> {
    use crate::parser::{Expr, LitKind};
    match e {
        Expr::Literal(LitKind::Int(n), _) if *n <= i64::MAX as u128 => Ok(*n as u64),
        Expr::Literal(LitKind::Int(_), _) => {
            Err("panjang array terlalu besar untuk indeks i64".into())
        }
        _ => Err("panjang array harus berupa literal integer konstan".into()),
    }
}
