// src/llvm/codegen_agregat.rs — literal agregat, konstruksi enum, dan `?`

#![allow(dead_code)]

use crate::parser::*;
use crate::lexer::Span;

use std::collections::HashMap;

use super::codegen::{Codegen, Val};
use super::sema::TypeSubst;
use super::types::*;

impl Codegen {
    // ── aggregate literals ───────────────────────────────────────────────

    /// Literal `Kotak { .. }` atas struct GENERIK tanpa argumen tertulis: instansnya dari tipe
    /// yang diharapkan, lalu dari nilai field-nya, lalu dari parameter tipe fungsi yang memuatnya.
    fn instans_literal(&mut self, nt: &NamedType, fields: &[FieldInit], expected: Option<&JrgType>,
                       siap: &mut HashMap<String, Val>, span: Span) -> Result<JrgType, String> {
        let params = self.param_struct(&nt.name).unwrap_or_default();
        if let Some(JrgType::Struct(inst)) = expected {
            if self.struct_instans.get(inst).map_or(false, |(d, _)| *d == nt.name) {
                return Ok(JrgType::Struct(inst.clone()));
            }
        }
        let mut subst = TypeSubst::new();
        let deklarasi = self.tables.field_rata(&nt.name);
        // Dua putaran: closure menunggu, supaya ia diturunkan DENGAN tipe yang sudah
        // terikat dari field lain (`Pilih { f: |s| len(s), bawaan: Some("xy") }`).
        for putaran in 0..2 {
            for fi in fields {
                let Some((_, f)) = deklarasi.iter().find(|(_, f)| f.name == fi.name) else { continue };
                if !Self::menyebut_param(&f.ty, &params) || siap.contains_key(&fi.name) { continue; }
                let tertunda = matches!(fi.value, Expr::Closure { .. });
                if tertunda != (putaran == 1) { continue; }
                if params.iter().all(|p| subst.contains_key(p)) { continue; }
                let v = self.codegen_expr(&fi.value, None)?;
                self.satukan(&f.ty, &v.ty, &params, &mut subst);
                siap.insert(fi.name.clone(), v);
            }
        }
        let luar = self.cur_fn_generics.clone();
        let mut args = Vec::with_capacity(params.len());
        for p in &params {
            // Tidak terlihat dari mana pun: nilai bertag, seperti parameter tipe fungsi.
            args.push(subst.get(p).or_else(|| luar.get(p)).cloned().unwrap_or(JrgType::Any));
        }
        let _ = span;
        Ok(JrgType::Struct(self.instantiate_struct(&nt.name, &args)?))
    }

    pub(crate) fn menyebut_param(te: &TypeExpr, params: &[String]) -> bool {
        match te {
            TypeExpr::Named(nt) => params.contains(&nt.name) || nt.args.iter().any(|a| Self::menyebut_param(a, params)),
            TypeExpr::Slice(s) => Self::menyebut_param(&s.elem, params),
            TypeExpr::Array(a) => Self::menyebut_param(&a.elem, params),
            TypeExpr::Tuple(es) => es.iter().any(|e| Self::menyebut_param(e, params)),
            TypeExpr::Fn(f) => f.params.iter().any(|e| Self::menyebut_param(e, params))
                || f.ret.as_ref().map_or(false, |r| Self::menyebut_param(r, params)),
            TypeExpr::Pointer(p) => Self::menyebut_param(&p.inner, params),
            TypeExpr::Ownership(o) => Self::menyebut_param(&o.inner, params),
            _ => false,
        }
    }

    pub fn codegen_struct_lit(&mut self, ty: &TypeExpr, fields: &[FieldInit], spread: &Option<Box<Expr>>, expected: Option<&JrgType>, span: Span) -> Result<Val, String> {
        // `E::V { a: 1, b: 2 }`: varian enum ber-field, bukan struct bernama `V`.
        if let TypeExpr::Named(nt) = ty {
            if let Some(en) = nt.package.clone().filter(|e| nt.args.is_empty() && self.types.enums.contains_key(e)) {
                return self.literal_varian_berfield(&en, &nt.name, fields, spread.is_some(), span);
            }
            // `V { a: 1 }` telanjang: struct bernama `V` menang, lalu varian ber-field satu enum.
            if nt.package.is_none() && nt.args.is_empty() && !self.tables.structs.contains_key(&nt.name) {
                let pemilik: Vec<String> = self.enum_punya_varian(&nt.name, false).into_iter()
                    .filter(|e| self.tables.enums.get(e).map_or(false, |ed| ed.variants.iter()
                        .any(|v| v.name == nt.name && matches!(v.kind, crate::parser::EnumVariantKind::Struct(_)))))
                    .collect();
                match pemilik.len() {
                    0 => {}
                    1 => return self.literal_varian_berfield(&pemilik[0], &nt.name, fields, spread.is_some(), span),
                    _ => return Err(super::galat::tandai(span, format!(
                        "`{}` ambigu — varian ber-field dengan nama itu ada di {}\nbantuan: tulis lengkap, mis. `{}::{}`",
                        nt.name, pemilik.join(" dan "), pemilik[0], nt.name))),
                }
            }
        }
        // Nilai field yang sudah diturunkan untuk membaca tipenya dipakai lagi di bawah.
        let mut siap: HashMap<String, Val> = HashMap::new();
        let sty = match ty {
            TypeExpr::Named(nt) if nt.args.is_empty() && self.param_struct(&nt.name).is_some() =>
                self.instans_literal(nt, fields, expected, &mut siap, span)?,
            _ => self.resolve_type_pub(ty)?,
        };
        let sname = match &sty { JrgType::Struct(n) => n.clone(), _ => return Err("literal `Type { .. }` butuh tipe struct".into()) };
        let layout = self.types.structs.get(&sname).cloned().ok_or_else(|| format!("struct `{}` tidak dikenal", sname))?;
        let slot = self.alloca_typed("slit.", &sty);

        let mut set_mask = vec![false; layout.fields.len()];
        if let Some(sp) = spread {
            let (sptr, spty) = self.get_place(sp)?;
            // Basis bertipe struct LAIN: field-nya harus bagian dari sasaran, dengan tipe yang sama.
            let asal = match &spty {
                JrgType::Struct(n) => self.types.structs.get(n).cloned(),
                _ => None,
            }.ok_or_else(|| format!("[{}:{}] `..` spread tidak cocok dengan tipe struct `{}`", span.lo.0, span.hi.0, sname))?;
            let mut liar: Vec<&String> = asal.fields.iter().map(|(n, _)| n).filter(|n| layout.field_index(n).is_none()).collect();
            liar.sort();
            if let Some(f) = liar.first() {
                return Err(super::galat::tandai(span, crate::interpreter::galat_index::sebar_field_liar(
                    &format!("struct `{}`", sname), f)));
            }
            for (i, (fname, fty)) in asal.fields.iter().enumerate() {
                let ke = layout.field_index(fname).expect("sudah diperiksa di atas");
                if layout.fields[ke].1 != *fty {
                    return Err(format!("[{}:{}] `..` spread tidak cocok dengan tipe struct `{}`", span.lo.0, span.hi.0, sname));
                }
                let src = self.fresh("spf.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", src, spty.llvm_ty(), sptr, i));
                let v = self.load_place(&src, fty);
                let dst = self.fresh("spfd.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", dst, sty.llvm_ty(), slot, ke));
                if *fty != JrgType::Unit { self.emit(format!("store {} {}, ptr {}", fty.llvm_ty(), v.text, dst)); }
                set_mask[ke] = true;
            }
        }
        for fi in fields {
            let idx = layout.field_index(&fi.name).ok_or_else(|| format!("struct `{}` tidak punya field `{}`", sname, fi.name))?;
            let fty = layout.fields[idx].1.clone();
            let v = match siap.remove(&fi.name) {
                Some(v) => v,
                None => self.codegen_expr(&fi.value, Some(&fty))?,
            };
            let v = self.coerce(&v, &fty);
            let dst = self.fresh("sfd.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", dst, sty.llvm_ty(), slot, idx));
            if fty != JrgType::Unit { self.emit(format!("store {} {}, ptr {}", fty.llvm_ty(), v.text, dst)); }
            set_mask[idx] = true;
        }
        // `field_rata` yang SAMA dipakai waktu layout ini didaftarkan, jadi
        // indeksnya sejajar — itu yang membuat nilai default field WARISAN ketemu.
        let raw = self.tables.field_rata(&sname);
        for (i, done) in set_mask.iter().enumerate() {
            if *done { continue; }
            let fty = layout.fields[i].1.clone();
            let default_expr = raw.get(i).and_then(|(_, f)| f.default.clone());
            let v = match default_expr {
                Some(e) => { let v = self.codegen_expr(&e, Some(&fty))?; self.coerce(&v, &fty) }
                None => return Err(crate::interpreter::galat_index::field_wajib(&sname, &layout.fields[i].0)),
            };
            let dst = self.fresh("sfdflt.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", dst, sty.llvm_ty(), slot, i));
            if fty != JrgType::Unit { self.emit(format!("store {} {}, ptr {}", fty.llvm_ty(), v.text, dst)); }
        }
        Ok(self.load_place(&slot, &sty))
    }

    pub fn codegen_array_lit(&mut self, elems: &[Expr], expected: Option<&JrgType>, _span: Span) -> Result<Val, String> {
        /// Tipe sebuah ekspresi yang seluruhnya literal; `None` begitu ada yang bukan literal.
        fn tipe_literal(e: &Expr) -> Option<JrgType> {
            Some(match e {
                Expr::Literal(LitKind::Int(_), _) => JrgType::I64,
                Expr::Literal(LitKind::Float(_), _) => JrgType::F64,
                Expr::Literal(LitKind::Bool(_), _) => JrgType::Bool,
                Expr::Literal(LitKind::Str(_), _) | Expr::Literal(LitKind::RawStr(_), _) => JrgType::Str,
                Expr::Literal(LitKind::Nil, _) => JrgType::Nil,
                Expr::Unary { op: UnaryOp::Neg, expr, .. } => return tipe_literal(expr),
                // Literal dict (`{"a": 1}` diturunkan parser jadi `::dict::from_pairs`).
                Expr::Call { func, .. } if matches!(func.as_ref(), Expr::PathExpr { segments, .. }
                    if segments.last().map(|x| x.as_str()) == Some("from_pairs")) =>
                    JrgType::Dict(Box::new(JrgType::Any)),
                Expr::Array { elems, .. } => {
                    let ts: Option<Vec<JrgType>> = elems.iter().map(tipe_literal).collect();
                    let ts = ts?;
                    let isi = if ts.is_empty() { JrgType::Never } else { satukan_literal(&ts) };
                    JrgType::Array(Box::new(isi), ts.len() as u64)
                }
                _ => return None,
            })
        }
        /// Tipe elemen untuk literal-literal ini: sama → itu; array beda panjang → slice; lainnya bertag.
        fn satukan_literal(ts: &[JrgType]) -> JrgType {
            if ts.iter().all(|t| *t == ts[0]) { return ts[0].clone(); }
            let isi: Option<Vec<JrgType>> = ts.iter().map(|t| match t {
                JrgType::Array(e, _) => Some(super::codegen_any::luruh(e)),
                _ => None,
            }).collect();
            if let Some(isi) = isi {
                let nyata: Vec<&JrgType> = isi.iter().filter(|t| **t != JrgType::Never).collect();
                if let Some(pertama) = nyata.first() {
                    if nyata.iter().all(|t| t == pertama) {
                        return JrgType::Slice(Box::new((*pertama).clone()));
                    }
                }
            }
            JrgType::Any
        }
        let elem_hint = match expected {
            Some(JrgType::Array(e, _)) => Some((**e).clone()),
            Some(JrgType::Slice(e)) => Some((**e).clone()),
            _ => None,
        };
        if elems.is_empty() {
            // Tanpa konteks, tipe elemennya memakai BAWAAN — aturan yang sama persis
            // dengan yang sudah dipakai literal angka (`tipe_literal_tanpa_konteks`).
            let ety = elem_hint.unwrap_or(JrgType::I64);
            return Ok(Val { text: "zeroinitializer".into(), ty: JrgType::Array(Box::new(ety), 0) });
        }
        // Literal bulat dan pecahan bercampur (`[1, 2.5, 0]`): acuan menyimpan tiap elemen
        // dengan jenisnya sendiri, jadi elemennya bertag — bukan dipotong, bukan dinaikkan.
        fn inti(e: &Expr) -> &Expr {
            match e { Expr::Unary { op: UnaryOp::Neg, expr, .. } => inti(expr), lain => lain }
        }
        let pecahan = |e: &Expr| matches!(inti(e), Expr::Literal(LitKind::Float(_), _));
        let bulat = |e: &Expr| matches!(inti(e), Expr::Literal(LitKind::Int(_), _));
        let campur = elem_hint.is_none() && elems.iter().any(pecahan) && elems.iter().any(bulat);
        // Literal yang jenisnya beda-beda: array beda panjang jadi array biasa, sisanya bertag.
        let dari_literal = if elem_hint.is_none() && !campur {
            let ts: Option<Vec<JrgType>> = elems.iter().map(tipe_literal).collect();
            ts.filter(|ts| ts.iter().any(|t| *t != ts[0])).map(|ts| satukan_literal(&ts))
        } else { None };
        // Elemen yang BENTUK sintaksnya beda-beda (`[1, P { .. }, (3, 4)]`) dan bukan semuanya
        // literal: tipenya dibaca dengan menurunkan tiap elemen di SATU salinan, lalu disatukan.
        let ragam = |e: &Expr| -> u8 { match inti(e) {
            Expr::Literal(..) => 1, Expr::Struct { .. } => 2, Expr::Tuple { .. } => 3, Expr::Array { .. } => 4,
            Expr::Call { .. } | Expr::PathExpr { .. } => 5, _ => 0 } };
        let beragam = elems.len() > 1 && elems.iter().any(|e| ragam(e) != ragam(&elems[0]));
        // Petunjuk dari luar tidak menang atas isi yang TIDAK bisa menjadi tipenya: struct dan
        // enum berbeda di satu literal (`[menu_baru, Option::Some(a)]`) tetap bertag.
        let buram = |t: &JrgType| matches!(t, JrgType::Struct(_) | JrgType::Enum(_) | JrgType::Tuple(_));
        let hint_lemah = matches!(&elem_hint, Some(h) if *h != JrgType::Any);
        let dari_literal = if dari_literal.is_none() && (elem_hint.is_none() || hint_lemah) && !campur && beragam {
            let mut salinan = self.clone();
            salinan.mode_duga = true;
            let ts: Option<Vec<JrgType>> = elems.iter().map(|e| salinan.codegen_expr(e, None).ok().map(|v| v.ty)).collect();
            let ts = ts.filter(|ts| ts.iter().any(|t| *t != ts[0]));
            match (&elem_hint, ts) {
                (None, ts) => ts.map(|ts| satukan_literal(&ts)),
                (Some(_), Some(ts)) if ts.iter().filter(|t| buram(t)).count() >= 1
                    && ts.iter().any(|t| *t != ts[0] && (buram(t) || buram(&ts[0]))) => Some(JrgType::Any),
                _ => None,
            }
        } else { dari_literal };
        let elem_hint = if dari_literal == Some(JrgType::Any) { None } else { elem_hint };
        let campur = campur || dari_literal == Some(JrgType::Any);
        let elem_hint = if campur { Some(JrgType::Any) } else { elem_hint.or(dari_literal) };
        let first = if campur { self.codegen_expr(&elems[0], None)? }
                    else { self.codegen_expr(&elems[0], elem_hint.as_ref())? };
        let ety = elem_hint.unwrap_or_else(|| first.ty.clone());
        let first = self.coerce(&first, &ety);
        let mut vals = vec![first];
        for e in &elems[1..] {
            let v = if campur { self.codegen_expr(e, None)? } else { self.codegen_expr(e, Some(&ety))? };
            vals.push(self.coerce(&v, &ety));
        }
        let aty = JrgType::Array(Box::new(ety.clone()), vals.len() as u64);
        let slot = self.alloca_typed("arrlit.", &aty);
        for (i, v) in vals.iter().enumerate() {
            let dst = self.fresh("ali.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i64 0, i64 {}", dst, aty.llvm_ty(), slot, i));
            self.emit(format!("store {} {}, ptr {}", ety.llvm_ty(), v.text, dst));
        }
        Ok(self.load_place(&slot, &aty))
    }

    pub fn codegen_array_repeat(&mut self, elem: &Expr, count: &Expr, expected: Option<&JrgType>, span: Span) -> Result<Val, String> {
        let n = match count {
            Expr::Literal(LitKind::Int(v), _) if *v <= i64::MAX as u128 => *v as u64,
            Expr::Literal(LitKind::Int(_), _) => {
                return Err(format!(
                    "[{}:{}] `[elem; N]`: N terlalu besar untuk indeks i64",
                    span.lo.0, span.hi.0
                ));
            }
            _ => return Err(format!("[{}:{}] `[elem; N]`: N harus literal integer konstan", span.lo.0, span.hi.0)),
        };
        let elem_hint = match expected { Some(JrgType::Array(e, _)) => Some((**e).clone()), _ => None };
        let ev = self.codegen_expr(elem, elem_hint.as_ref())?;
        let ety = elem_hint.unwrap_or_else(|| ev.ty.clone());
        let ev = self.coerce(&ev, &ety);
        let aty = JrgType::Array(Box::new(ety.clone()), n);
        let slot = self.alloca_typed("arep.", &aty);
        for i in 0..n {
            let dst = self.fresh("arepi.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i64 0, i64 {}", dst, aty.llvm_ty(), slot, i));
            self.emit(format!("store {} {}, ptr {}", ety.llvm_ty(), ev.text, dst));
        }
        Ok(self.load_place(&slot, &aty))
    }

    pub fn codegen_tuple_lit(&mut self, elems: &[Expr], expected: Option<&JrgType>, _span: Span) -> Result<Val, String> {
        let hints: Vec<Option<JrgType>> = match expected {
            Some(JrgType::Tuple(ts)) if ts.len() == elems.len() => ts.iter().map(|t| Some(t.clone())).collect(),
            _ => vec![None; elems.len()],
        };
        let mut vals = Vec::with_capacity(elems.len());
        for (e, hint) in elems.iter().zip(hints.iter()) {
            let v = self.codegen_expr(e, hint.as_ref())?;
            let v = match hint { Some(h) => self.coerce(&v, h), None => v };
            vals.push(v);
        }
        let tty = JrgType::Tuple(vals.iter().map(|v| v.ty.clone()).collect());
        let mut agg = "undef".to_string();
        for (i, v) in vals.iter().enumerate() {
            let r = self.fresh("tup.");
            self.emit(format!("{} = insertvalue {} {}, {} {}, {}", r, tty.llvm_ty(), agg, v.ty.llvm_ty(), v.text, i));
            agg = r;
        }
        Ok(Val { text: agg, ty: tty })
    }

    // ── enum construction ────────────────────────────────────────────────

    pub fn build_enum_value(&mut self, enum_name: &str, _layout: &EnumLayout, variant: &EnumVariantLayout, field_vals: &[Val]) -> Result<Val, String> {
        let ety = JrgType::Enum(enum_name.to_string());
        let slot = self.alloca_typed("elit.", &ety);
        let tagptr = self.fresh("etag.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 0", tagptr, ety.llvm_ty(), slot));
        self.emit(format!("store i32 {}, ptr {}", variant.tag, tagptr));
        if !field_vals.is_empty() {
            let payload_struct = variant.payload.as_llvm_struct();
            let blobptr = self.fresh("eblob.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", blobptr, ety.llvm_ty(), slot));
            for (i, v) in field_vals.iter().enumerate() {
                let fp = self.fresh("epf.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", fp, payload_struct, blobptr, i));
                self.emit(format!("store {} {}, ptr {}", v.ty.llvm_ty(), v.text, fp));
            }
        }
        Ok(self.load_place(&slot, &ety))
    }

    pub fn build_enum_unit_variant(&mut self, enum_name: &str, variant_name: &str) -> Result<Val, String> {
        let layout = self.types.enums.get(enum_name).cloned().ok_or_else(|| format!("enum `{}` tidak dikenal", enum_name))?;
        let variant = layout.variant(variant_name).cloned().ok_or_else(|| format!("enum `{}` tidak punya variant `{}`", enum_name, variant_name))?;
        self.build_enum_value(enum_name, &layout, &variant, &[])
    }

    pub fn construct_user_enum_variant_pub(&mut self, enum_name: &str, variant_name: &str, args: &[Expr], span: Span) -> Result<Val, String> {
        self.construct_user_enum_variant(enum_name, variant_name, args, span)
    }

    /// Literal varian ber-field: nilainya dievaluasi urut TULIS, lalu disusun urut deklarasi.
    fn literal_varian_berfield(&mut self, en: &str, varian: &str, fields: &[FieldInit], sebar: bool, span: Span) -> Result<Val, String> {
        let tandai = |m: String| super::galat::tandai(span, m);
        let layout = self.types.enums.get(en).cloned().ok_or_else(|| tandai(format!("enum `{}` tidak dikenal", en)))?;
        let v = layout.variant(varian).cloned()
            .ok_or_else(|| tandai(format!("enum `{}` tidak punya varian `{}`", en, varian)))?;
        let nama = v.payload.field_names();
        if nama.is_empty() {
            return Err(tandai(format!("varian `{}::{}` tidak punya field bernama — tulis `{}::{}(..)` atau `{}::{}`", en, varian, en, varian, en, varian)));
        }
        if sebar {
            return Err(tandai(format!(
                "spread (`...x`) di literal varian `{}::{}` belum didukung backend `--emit-llvm`\n\
                 bantuan: tulis tiap field-nya, atau jalankan tanpa `--emit-llvm`", en, varian)));
        }
        let tipe = v.payload.field_types();
        let mut tempat: HashMap<String, Expr> = HashMap::new();
        for fi in fields {
            let Some(i) = v.payload.field_index_by_name(&fi.name) else {
                return Err(tandai(format!("varian `{}::{}` tidak punya field `{}` (yang ada: {})", en, varian, fi.name, nama.join(", "))));
            };
            let nilai = self.codegen_expr(&fi.value, Some(&tipe[i]))?;
            let nilai = self.coerce(&nilai, &tipe[i]);
            let p = self.spill_to_place(&nilai);
            tempat.insert(fi.name.clone(), self.lokal_sementara(p, tipe[i].clone(), span));
        }
        let mut urut = Vec::with_capacity(nama.len());
        for n in &nama {
            urut.push(tempat.remove(n).ok_or_else(|| tandai(crate::interpreter::galat_index::field_wajib_varian(&format!("{}::{}", en, varian), n)))?);
        }
        self.construct_user_enum_variant(en, varian, &urut, span)
    }

    pub(crate) fn construct_user_enum_variant(&mut self, enum_name: &str, variant_name: &str, args: &[Expr], span: Span) -> Result<Val, String> {
        let layout = self.types.enums.get(enum_name).cloned().ok_or_else(|| format!("enum `{}` tidak dikenal", enum_name))?;
        let variant = layout.variant(variant_name).cloned()
            .ok_or_else(|| format!("enum `{}` tidak punya variant `{}`", enum_name, variant_name))?;
        let field_tys = variant.payload.field_types();
        if field_tys.len() != args.len() {
            // Kalimat milik interpreter: varian tanpa payload memang bukan fungsi.
            let pesan = if field_tys.is_empty() {
                crate::interpreter::pesan_bukan_fungsi(&format!("enum:{}", enum_name))
            } else {
                crate::interpreter::pesan_konstruktor_argumen(enum_name, variant_name, field_tys.len(), args.len())
            };
            return Err(format!("[{}:{}] {}", span.lo.0, span.hi.0, pesan));
        }
        let mut vals = Vec::with_capacity(args.len());
        for (a, fty) in args.iter().zip(field_tys.iter()) {
            let v = self.codegen_expr(a, Some(fty))?;
            vals.push(self.coerce(&v, fty));
        }
        self.build_enum_value(enum_name, &layout, &variant, &vals)
    }

    /// Penanda penduga untuk nilai yang separuh tipenya belum terlihat (`None`, `Ok(..)`,
    /// `Err(..)` tanpa harapan): `Never` mengalah pada tipe dari `return` lain.
    pub(crate) fn nilai_belum_tahu() -> Val { Val { text: "undef".into(), ty: JrgType::Never } }

    /// Gabungkan dua tipe yang dilihat penduga: `Never` di mana pun diisi dari sisi lain.
    pub(crate) fn gabung_duga(lama: &JrgType, baru: &JrgType) -> JrgType {
        match (lama, baru) {
            (JrgType::Never, b) => b.clone(),
            // `return [v]` di satu cabang dan `return []` di cabang lain: array biasa.
            (JrgType::Array(a, n), JrgType::Array(b, m)) if n != m =>
                JrgType::Slice(if *n == 0 { b.clone() } else { a.clone() }),
            (JrgType::Array(a, 0), JrgType::Slice(_)) => { let _ = a; baru.clone() }
            // Struct/enum di satu cabang dan `nil` di cabang lain: nilai bertag.
            (JrgType::Nil, b) | (b, JrgType::Nil) if !matches!(b, JrgType::Nil | JrgType::Never) => JrgType::Any,
            (JrgType::Tuple(a), JrgType::Tuple(b)) if a.len() == b.len() =>
                JrgType::Tuple(a.iter().zip(b).map(|(x, y)| Self::gabung_duga(x, y)).collect()),
            (a, _) => a.clone(),
        }
    }

    pub(crate) fn memuat_never(t: &JrgType) -> bool {
        match t {
            JrgType::Never => true,
            JrgType::Tuple(ts) => ts.iter().any(Self::memuat_never),
            _ => false,
        }
    }

    /// "Butuh anotasi" di fungsi yang penduganya berhenti oleh galat LAIN: galat itu sebabnya.
    pub(crate) fn galat_separuh(&self, bawaan: String) -> String {
        match &self.galat_duga_kini {
            Some(g) if self.ret_ditebak && !self.mode_duga => g.clone(),
            _ => bawaan,
        }
    }

    pub fn construct_option_result(&mut self, name: &str, args: &[Expr], expected: Option<&JrgType>, span: Span) -> Result<Val, String> {
        match name {
            "Some" => {
                if args.len() != 1 { return Err(format!("[{}:{}] {}", span.lo.0, span.hi.0,
                    crate::interpreter::pesan_konstruktor_argumen("Option", "Some", 1, args.len()))); }
                let t_hint = match expected { Some(JrgType::Enum(en)) => self.option_args.get(en).cloned(), _ => None };
                let v = self.codegen_expr(&args[0], t_hint.as_ref())?;
                let t = t_hint.unwrap_or_else(|| v.ty.clone());
                let v = self.coerce(&v, &t);
                let en = self.instantiate_option(&t);
                let layout = self.types.enums.get(&en).cloned().unwrap();
                let variant = layout.variant("Some").cloned().unwrap();
                self.build_enum_value(&en, &layout, &variant, &[v])
            }
            "None" => {
                if !args.is_empty() { return Err(format!("[{}:{}] {}", span.lo.0, span.hi.0,
                    crate::interpreter::pesan_bukan_fungsi("enum:Option"))); }
                let t = match expected {
                    Some(JrgType::Enum(en)) if self.option_args.contains_key(en) => self.option_args[en].clone(),
                    _ if self.mode_duga => return Ok(Self::nilai_belum_tahu()),
                    // Tidak ada yang menyebut isinya: nilai bertag (lapis B), seperti parameter tipe
                    // yang tidak terlihat. Anotasi memberi tipe yang cepat.
                    _ => { let _ = span; JrgType::Any }
                };
                let en = self.instantiate_option(&t);
                self.build_enum_unit_variant(&en, "None")
            }
            "Ok" => {
                if args.len() != 1 { return Err(format!("[{}:{}] {}", span.lo.0, span.hi.0,
                    crate::interpreter::pesan_konstruktor_argumen("Result", "Ok", 1, args.len()))); }
                let (t_hint, e_hint) = match expected {
                    Some(JrgType::Enum(en)) => self.result_args.get(en).cloned().map(|(t, e)| (Some(t), Some(e))).unwrap_or((None, None)),
                    _ => (None, None),
                };
                let v = self.codegen_expr(&args[0], t_hint.as_ref())?;
                let t = t_hint.unwrap_or_else(|| v.ty.clone());
                let v = self.coerce(&v, &t);
                if e_hint.is_none() && self.mode_duga {
                    self.separuh_duga.0.get_or_insert(t);
                    return Ok(Self::nilai_belum_tahu());
                }
                let e = e_hint.unwrap_or(JrgType::Any);
                let en = self.instantiate_result(&t, &e);
                let layout = self.types.enums.get(&en).cloned().unwrap();
                let variant = layout.variant("Ok").cloned().unwrap();
                self.build_enum_value(&en, &layout, &variant, &[v])
            }
            "Err" => {
                if args.len() != 1 { return Err(format!("[{}:{}] {}", span.lo.0, span.hi.0,
                    crate::interpreter::pesan_konstruktor_argumen("Result", "Err", 1, args.len()))); }
                let (t_hint, e_hint) = match expected {
                    Some(JrgType::Enum(en)) => self.result_args.get(en).cloned().map(|(t, e)| (Some(t), Some(e))).unwrap_or((None, None)),
                    _ => (None, None),
                };
                let v = self.codegen_expr(&args[0], e_hint.as_ref())?;
                let e = e_hint.unwrap_or_else(|| v.ty.clone());
                let v = self.coerce(&v, &e);
                if t_hint.is_none() && self.mode_duga {
                    self.separuh_duga.1.get_or_insert(e);
                    return Ok(Self::nilai_belum_tahu());
                }
                let t = t_hint.unwrap_or(JrgType::Any);
                let en = self.instantiate_result(&t, &e);
                let layout = self.types.enums.get(&en).cloned().unwrap();
                let variant = layout.variant("Err").cloned().unwrap();
                self.build_enum_value(&en, &layout, &variant, &[v])
            }
            _ => unreachable!(),
        }
    }

    // ── `?` propagate ────────────────────────────────────────────────────

    pub fn codegen_propagate(&mut self, e: &Expr, span: Span) -> Result<Val, String> {
        let v = self.codegen_expr(e, None)?;
        let ename = match &v.ty { JrgType::Enum(n) => n.clone(), other => return Err(format!("[{}:{}] `?` hanya berlaku pada Option/Result, dapat `{}`", span.lo.0, span.hi.0, other.display_name())) };
        let place = self.spill_to_place(&v);
        let tagptr = self.fresh("qtag.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 0", tagptr, v.ty.llvm_ty(), place));
        let tagv = self.fresh("qtagv.");
        self.emit(format!("{} = load i32, ptr {}", tagv, tagptr));

        if let Some(t) = self.option_args.get(&ename).cloned() {
            let is_none = self.fresh("qisnone.");
            self.emit(format!("{} = icmp eq i32 {}, 1", is_none, tagv));
            let none_lbl = self.fresh_label("qnone.");
            let some_lbl = self.fresh_label("qsome.");
            self.terminate(format!("br i1 {}, label %{}, label %{}", is_none, none_lbl, some_lbl));

            self.start_block(none_lbl);
            let ret_ty = self.cur_ret_ty();
            let noneval = match &ret_ty {
                JrgType::Enum(ren) if self.option_args.contains_key(ren) => self.build_enum_unit_variant(ren, "None")?,
                _ => return Err(format!("[{}:{}] `?` pada Option butuh fungsi ini mengembalikan `Option<..>` juga", span.lo.0, span.hi.0)),
            };
            self.emit_return(&noneval, &ret_ty);

            self.start_block(some_lbl);
            let blobptr = self.fresh("qblob.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", blobptr, v.ty.llvm_ty(), place));
            let payload_struct = format!("{{ {} }}", t.llvm_ty());
            let fp = self.fresh("qfp.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 0", fp, payload_struct, blobptr));
            Ok(self.load_place(&fp, &t))
        } else if let Some((t, e_ty)) = self.result_args.get(&ename).cloned() {
            let is_err = self.fresh("qiserr.");
            self.emit(format!("{} = icmp eq i32 {}, 1", is_err, tagv));
            let err_lbl = self.fresh_label("qerr.");
            let ok_lbl = self.fresh_label("qok.");
            self.terminate(format!("br i1 {}, label %{}, label %{}", is_err, err_lbl, ok_lbl));

            self.start_block(err_lbl);
            let blobptr_e = self.fresh("qeblob.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", blobptr_e, v.ty.llvm_ty(), place));
            let err_payload_struct = format!("{{ {} }}", e_ty.llvm_ty());
            let efp = self.fresh("qefp.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 0", efp, err_payload_struct, blobptr_e));
            let errval = self.load_place(&efp, &e_ty);
            let ret_ty = self.cur_ret_ty();
            let new_err = match &ret_ty {
                JrgType::Enum(ren) if self.result_args.contains_key(ren) => {
                    let rlayout = self.types.enums.get(ren).cloned().unwrap();
                    let rvariant = rlayout.variant("Err").cloned().unwrap();
                    self.build_enum_value(ren, &rlayout, &rvariant, &[errval])?
                }
                _ => return Err(format!("[{}:{}] `?` pada Result butuh fungsi ini mengembalikan `Result<..>` juga", span.lo.0, span.hi.0)),
            };
            self.emit_return(&new_err, &ret_ty);

            self.start_block(ok_lbl);
            let blobptr_ok = self.fresh("qoblob.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", blobptr_ok, v.ty.llvm_ty(), place));
            let ok_payload_struct = format!("{{ {} }}", t.llvm_ty());
            let ofp = self.fresh("qofp.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 0", ofp, ok_payload_struct, blobptr_ok));
            Ok(self.load_place(&ofp, &t))
        } else {
            Err(format!("[{}:{}] `?` hanya berlaku pada Option/Result", span.lo.0, span.hi.0))
        }
    }

}
