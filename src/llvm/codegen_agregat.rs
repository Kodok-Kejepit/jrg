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
            if nt.name == "dict" && nt.package.is_none() && nt.args.is_empty() && !self.tables.structs.contains_key("dict") {
                return self.literal_dict_berfield(fields, spread, expected, span);
            }
            if let Some(en) = nt.package.clone().filter(|e| nt.args.is_empty()
                && (self.types.enums.contains_key(e) || self.param_enum(e).is_some())) {
                self.harapan_ktor = expected.cloned();
                return self.literal_varian_berfield(&en, &nt.name, fields, spread.as_deref(), span);
            }
            // `V { a: 1 }` telanjang: struct bernama `V` menang, lalu varian ber-field satu enum.
            if nt.package.is_none() && nt.args.is_empty() && !self.tables.structs.contains_key(&nt.name) {
                let pemilik: Vec<String> = self.enum_punya_varian(&nt.name, false).into_iter()
                    .filter(|e| self.tables.enums.get(e).map_or(false, |ed| ed.variants.iter()
                        .any(|v| v.name == nt.name && matches!(v.kind, crate::parser::EnumVariantKind::Struct(_)))))
                    .collect();
                match pemilik.len() {
                    0 => {}
                    1 => {
                        self.harapan_ktor = expected.cloned();
                        return self.literal_varian_berfield(&pemilik[0], &nt.name, fields, spread.as_deref(), span);
                    }
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
            // Field yang ditulis literalnya tidak dibaca dari basis, jadi tipenya tidak perlu sama.
            let ditulis: std::collections::HashSet<&str> = fields.iter().map(|f| f.name.as_str()).collect();
            for (i, (fname, fty)) in asal.fields.iter().enumerate() {
                let ke = layout.field_index(fname).expect("sudah diperiksa di atas");
                if ditulis.contains(fname.as_str()) { continue; }
                let slot_ty = layout.fields[ke].1.clone();
                let tujuan = slot_ty.tanpa_kotak();
                let fty = &fty.tanpa_kotak();
                let src = self.fresh("spf.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", src, spty.llvm_ty(), sptr, i));
                let v = self.load_place(&src, &asal.fields[i].1);
                let v = if *fty == tujuan { v } else {
                    let buram = super::codegen_any::tag_untuk(&tujuan) == Some(super::codegen_any::TAG_BURAM)
                        || self.kata_kotak(&tujuan).is_some();
                    // Dua skalar berbeda jenis tidak bisa disamakan `coerce`: sebut field-nya di sini.
                    let skalar = |t: &JrgType| super::codegen_any::tag_untuk(t)
                        .is_some_and(|g| g != super::codegen_any::TAG_BURAM) && !matches!(t,
                        JrgType::Any | JrgType::Slice(_) | JrgType::Array(..) | JrgType::Dict(_) | JrgType::DictCampur(_));
                    let c = match (*fty == JrgType::Any, buram) {
                        _ if skalar(fty) && skalar(&tujuan) && !(fty.is_int() && tujuan.is_int()) => v.clone(),
                        (true, true) => self.buka_any(&v, &tujuan, span)?,
                        (true, false) => self.buka_terperiksa(&v, &tujuan).unwrap_or(v),
                        (false, _) => self.coerce(&v, &tujuan),
                    };
                    if c.ty != tujuan {
                        return Err(super::galat::tandai(span, format!(
                            "field `{}` dari basis sebar bertipe `{}`, sedangkan `{}.{}` bertipe `{}` — di backend \
                             `--emit-llvm` tipe sebuah field tetap (interpreter dan `--vm` menyimpan tipe bersama \
                             nilainya)\nbantuan: tulis field itu di literalnya, atau samakan tipenya",
                            fname, fty.nama_typeof(), sname, fname, tujuan.nama_typeof())));
                    }
                    c
                };
                let dst = self.fresh("spfd.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", dst, sty.llvm_ty(), slot, ke));
                self.simpan_slot(&dst, &slot_ty, &v);
                set_mask[ke] = true;
            }
        }
        for fi in fields {
            let idx = layout.field_index(&fi.name).ok_or_else(|| format!("struct `{}` tidak punya field `{}`", sname, fi.name))?;
            let slot_ty = layout.fields[idx].1.clone();
            let fty = slot_ty.tanpa_kotak();
            let v = match siap.remove(&fi.name) {
                Some(v) => v,
                None => self.codegen_expr(&fi.value, Some(&fty))?,
            };
            let v = self.coerce(&v, &fty);
            let dst = self.fresh("sfd.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", dst, sty.llvm_ty(), slot, idx));
            self.simpan_slot(&dst, &slot_ty, &v);
            set_mask[idx] = true;
        }
        // `field_rata` yang SAMA dipakai waktu layout ini didaftarkan, jadi
        // indeksnya sejajar — itu yang membuat nilai default field WARISAN ketemu.
        let raw = self.tables.field_rata(&sname);
        for (i, done) in set_mask.iter().enumerate() {
            if *done { continue; }
            let slot_ty = layout.fields[i].1.clone();
            let fty = slot_ty.tanpa_kotak();
            let default_expr = raw.get(i).and_then(|(_, f)| f.default.clone());
            let v = match default_expr {
                Some(e) => { let v = self.codegen_expr(&e, Some(&fty))?; self.coerce(&v, &fty) }
                None => return Err(crate::interpreter::galat_index::field_wajib(&sname, &layout.fields[i].0)),
            };
            let dst = self.fresh("sfdflt.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", dst, sty.llvm_ty(), slot, i));
            self.simpan_slot(&dst, &slot_ty, &v);
        }
        Ok(self.load_place(&slot, &sty))
    }

    pub fn codegen_array_lit(&mut self, elems: &[Expr], expected: Option<&JrgType>, _span: Span) -> Result<Val, String> {
        // Diambil SEKARANG: literal di dalam elemennya tidak boleh ikut memakainya.
        let pindah = self.elemen_pindah.take()
            .filter(|n| elems.iter().any(|e| matches!(e, Expr::Ident(m, _) if m == n)));
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
        } else if !campur && matches!(&elem_hint, Some(h) if *h != JrgType::Any) {
            // Literal teks bersama angka/bool (`[2, "b"]`) tidak bisa jadi tipe petunjuknya: bertag.
            let ts: Option<Vec<JrgType>> = elems.iter().map(tipe_literal).collect();
            ts.filter(|ts| ts.iter().any(|t| *t == JrgType::Str)
                && ts.iter().any(|t| t.is_numeric() || *t == JrgType::Bool)).map(|_| JrgType::Any)
        } else { None };
        // Elemen yang BENTUK sintaksnya beda-beda (`[1, P { .. }, (3, 4)]`) dan bukan semuanya
        // literal: tipenya dibaca dengan menurunkan tiap elemen di SATU salinan, lalu disatukan.
        let ragam = |e: &Expr| -> u8 { match inti(e) {
            Expr::Literal(..) => 1, Expr::Struct { .. } => 2, Expr::Tuple { .. } => 3, Expr::Array { .. } => 4,
            Expr::Call { .. } | Expr::PathExpr { .. } => 5, _ => 0 } };
        // Literal struct bertipe BERBEDA (`[Circle { .. }, Square { .. }]`) juga beragam.
        let nama_struct = |e: &Expr| match inti(e) { Expr::Struct { ty, .. } => super::sema::type_expr_base_name(ty), _ => None };
        // Nama dan jalur field yang tipenya terbaca tanpa memancarkan IR (`[d, i, k]`, `[k, c.cap]`) juga.
        let tipe_cepat = |cg: &Self, e: &Expr| match e {
            Expr::Field { .. } | Expr::Index { .. } => cg.tipe_jalur(e),
            lain => cg.peek_expr_type(lain),
        };
        let beda_tipe = elems.len() > 1 && {
            let ts: Option<Vec<JrgType>> = elems.iter().map(|e| tipe_cepat(self, e)).collect();
            ts.is_some_and(|ts| ts.iter().any(|t| *t != ts[0]))
        };
        // Literal array berbeda panjang (`[[t], [t, t]]` atas variabel/tuple) jadi slice; dulu ditolak `[2]T` vs `[1]T`.
        let panjang = |e: &Expr| match inti(e) { Expr::Array { elems, .. } => Some(elems.len()), _ => None };
        let beda_panjang = elems.iter().all(|e| panjang(e).is_some()) && elems.iter().any(|e| panjang(e) != panjang(&elems[0]));
        let beragam = elems.len() > 1 && (elems.iter().any(|e| ragam(e) != ragam(&elems[0])) || beda_panjang
            || elems.iter().any(|e| nama_struct(e).is_some() && nama_struct(e) != nama_struct(&elems[0]))
            || beda_tipe);
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
                // Teks bersama angka/bool tidak pernah bisa jadi tipe petunjuknya: bertag, bukan ditolak.
                (Some(_), Some(ts)) if ts.iter().any(|t| *t == JrgType::Str)
                    && ts.iter().any(|t| t.is_numeric() || *t == JrgType::Bool) => Some(JrgType::Any),
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
        if let Some(n) = &pindah {
            for (e, v) in elems.iter().zip(vals.iter_mut()) {
                if !matches!(e, Expr::Ident(m, _) if m == n) { *v = self.ambil_milik(e, v.clone())?; }
            }
        }
        let aty = JrgType::Array(Box::new(ety.clone()), vals.len() as u64);
        let slot = self.alloca_typed("arrlit.", &aty);
        for (i, v) in vals.iter().enumerate() {
            let dst = self.fresh("ali.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i64 0, i64 {}", dst, aty.llvm_ty(), slot, i));
            self.emit(format!("store {} {}, ptr {}", ety.llvm_ty(), v.text, dst));
        }
        self.lit_milik = pindah.is_some();
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
            let tipe = variant.payload.field_types();
            let blobptr = self.fresh("eblob.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", blobptr, ety.llvm_ty(), slot));
            for (i, v) in field_vals.iter().enumerate() {
                let fp = self.fresh("epf.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", fp, payload_struct, blobptr, i));
                match tipe.get(i) {
                    Some(t @ JrgType::Kotak(_)) => self.simpan_slot(&fp, t, v),
                    _ => self.emit(format!("store {} {}, ptr {}", v.ty.llvm_ty(), v.text, fp)),
                }
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
    /// `dict { a: 1, ...b }` = `dict::merge(b, dict::from_pairs([["a", 1]]))`: basisnya
    /// dievaluasi dulu dan field tertulis menang, aturan acuannya.
    fn literal_dict_berfield(&mut self, fields: &[FieldInit], spread: &Option<Box<Expr>>, expected: Option<&JrgType>, span: Span) -> Result<Val, String> {
        let pasangan = Expr::Array { elems: fields.iter().map(|f| Expr::Array {
            elems: vec![Expr::Literal(LitKind::Str(f.name.clone()), f.span), f.value.clone()], span: f.span,
        }).collect(), span };
        let Some(basis) = spread else { return self.dict_dari_pasangan(&pasangan, expected, span) };
        // Basis struct: field-nya (yang privat juga, seperti acuan) jadi pasangan di DEPAN field
        // tertulis, yang menang; basisnya dievaluasi dulu.
        let v = self.codegen_expr(basis, None)?;
        let tempat = self.spill_to_place(&v);
        if let JrgType::Struct(n) = v.ty.clone() {
            let layout = self.types.structs.get(&n).cloned().ok_or("struct hilang")?;
            let mut isi = Vec::new();
            for (i, (f, ft)) in layout.fields.iter().enumerate() {
                let fp = self.fresh("sbd.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", fp, v.ty.llvm_ty(), tempat, i));
                let x = self.load_place(&fp, ft);
                let p = self.spill_to_place(&x);
                let e = self.lokal_sementara(p, x.ty.clone(), span);
                isi.push(Expr::Array { elems: vec![Expr::Literal(LitKind::Str(f.clone()), span), e], span });
            }
            if let Expr::Array { elems, .. } = pasangan { isi.extend(elems); }
            return self.dict_dari_pasangan(&Expr::Array { elems: isi, span }, expected, span);
        }
        let basis = &Box::new(self.lokal_sementara(tempat, v.ty.clone(), span));
        if fields.is_empty() { return self.codegen_expr(basis, expected); }
        let jalur = |item: &str| Box::new(Expr::PathExpr {
            segments: vec![String::new(), "dict".into(), item.into()], span });
        let gabung = Expr::Call { func: jalur("merge"), span, args: vec![(**basis).clone(),
            Expr::Call { func: jalur("from_pairs"), args: vec![pasangan], span }] };
        self.codegen_expr(&gabung, expected).map_err(|e| if !e.contains("`dict::merge` butuh") { e } else { super::galat::tandai(span,
            "sebar di literal `dict { .., ...basis }` belum didukung backend `--emit-llvm` kalau basisnya bukan dict \
             atau tipe nilainya berbeda dari field yang ditulis\nbantuan: jalankan tanpa `--emit-llvm` (interpreter) atau dengan `--vm`".to_string()) })
    }

    fn literal_varian_berfield(&mut self, en: &str, varian: &str, fields: &[FieldInit], sebar: Option<&Expr>, span: Span) -> Result<Val, String> {
        let tandai = |m: String| super::galat::tandai(span, m);
        let harap = self.harapan_ktor.take();
        // Enum generik: tata letaknya dari instans yang diharapkan, kalau ada; selain itu bertag
        // dulu, cuma untuk membaca nama field — instans sebenarnya dipilih konstruktornya.
        let en_layout = match self.param_enum(en) {
            Some(ps) => match &harap {
                Some(JrgType::Enum(i)) if self.enum_instans.get(i).map_or(false, |(d, _)| d == en) => i.clone(),
                _ => self.instantiate_enum(en, &vec![JrgType::Any; ps.len()]).map_err(tandai)?,
            },
            None => en.to_string(),
        };
        let generik = en_layout != en;
        let layout = self.types.enums.get(&en_layout).cloned().ok_or_else(|| tandai(format!("enum `{}` tidak dikenal", en)))?;
        let v = layout.variant(varian).cloned()
            .ok_or_else(|| tandai(format!("enum `{}` tidak punya varian `{}`", en, varian)))?;
        let nama = v.payload.field_names();
        if nama.is_empty() {
            return Err(tandai(format!("varian `{}::{}` tidak punya field bernama — tulis `{}::{}(..)` atau `{}::{}`", en, varian, en, varian, en, varian)));
        }
        if let Some(basis) = sebar {
            if generik {
                return Err(tandai(format!(
                    "spread (`...x`) di literal varian enum generik `{}::{}` belum didukung backend `--emit-llvm`\n\
                     bantuan: tulis tiap field-nya, atau jalankan tanpa `--emit-llvm`", en, varian)));
            }
            self.harapan_ktor = harap;
            return self.literal_varian_sebar(en, varian, fields, basis, span);
        }
        let tipe: Vec<JrgType> = v.payload.field_types().iter().map(|t| t.tanpa_kotak()).collect();
        let mut tempat: HashMap<String, Expr> = HashMap::new();
        for fi in fields {
            let Some(i) = v.payload.field_index_by_name(&fi.name) else {
                return Err(tandai(format!("varian `{}::{}` tidak punya field `{}` (yang ada: {})", en, varian, fi.name, nama.join(", "))));
            };
            let harap_isi = (!generik || matches!(&harap, Some(JrgType::Enum(_)))).then(|| tipe[i].clone());
            let nilai = self.codegen_expr(&fi.value, harap_isi.as_ref())?;
            let nilai = if generik { nilai } else { self.coerce(&nilai, &tipe[i]) };
            let ty = nilai.ty.clone();
            let p = self.spill_to_place(&nilai);
            tempat.insert(fi.name.clone(), self.lokal_sementara(p, ty, span));
        }
        let mut urut = Vec::with_capacity(nama.len());
        for n in &nama {
            urut.push(tempat.remove(n).ok_or_else(|| tandai(crate::interpreter::galat_index::field_wajib_varian(&format!("{}::{}", en, varian), n)))?);
        }
        self.harapan_ktor = harap;
        self.construct_user_enum_variant(en, varian, &urut, span)
    }

    /// `E::V { a: 1, ...b }`: basis dulu, lalu field tertulis; field basis di luar `V` dan field `V` yang
    /// tidak terisi gagal SAAT JALAN dengan kalimat acuan. Basis enum yang sama dipilih variannya saat jalan.
    fn literal_varian_sebar(&mut self, en: &str, varian: &str, fields: &[FieldInit], basis: &Expr, span: Span) -> Result<Val, String> {
        use crate::interpreter::galat_index as gi;
        let tandai = |m: String| super::galat::tandai(span, m);
        let layout = self.types.enums.get(en).cloned().ok_or_else(|| tandai(format!("enum `{}` tidak dikenal", en)))?;
        let v = layout.variant(varian).cloned().ok_or_else(|| tandai(format!("enum `{}` tidak punya varian `{}`", en, varian)))?;
        let milik = v.payload.field_names();
        let sebutan = format!("varian `{}::{}`", super::modul::tampil(en), varian);
        let b = self.codegen_expr(basis, None)?;
        let bty = b.ty.clone();
        let tempat_b = self.spill_to_place(&b);
        // Isi satu varian sasaran dari daftar field basis yang diketahui di cabang ini.
        let rakit = |cg: &mut Self, dari_basis: &[(String, Expr)]| -> Result<Val, String> {
            let punya: Vec<String> = dari_basis.iter().map(|(n, _)| n.clone()).collect();
            if let Some(f) = gi::field_sebar_liar(punya.iter(), &milik) {
                cg.panik(&gi::sebar_field_liar(&sebutan, f));
                return Ok(Val::unit());
            }
            let tipe: Vec<JrgType> = v.payload.field_types().iter().map(|t| t.tanpa_kotak()).collect();
            let mut isi: HashMap<String, Expr> = dari_basis.iter().cloned().collect();
            for fi in fields {
                let i = v.payload.field_index_by_name(&fi.name).ok_or_else(|| tandai(format!(
                    "varian `{}::{}` tidak punya field `{}` (yang ada: {})", en, varian, fi.name, milik.join(", "))))?;
                let x = cg.codegen_expr(&fi.value, Some(&tipe[i]))?;
                let x = cg.coerce(&x, &tipe[i]);
                let ty = x.ty.clone();
                let p = cg.spill_to_place(&x);
                isi.insert(fi.name.clone(), cg.lokal_sementara(p, ty, span));
            }
            let mut urut = Vec::with_capacity(milik.len());
            for n in &milik {
                match isi.remove(n) {
                    Some(e) => urut.push(e),
                    None => {
                        cg.panik(&gi::field_wajib_varian(&format!("{}::{}", super::modul::tampil(en), varian), n));
                        return Ok(Val::unit());
                    }
                }
            }
            cg.construct_user_enum_variant(en, varian, &urut, span)
        };
        match &bty {
            JrgType::Struct(sn) => {
                // Field privat ikut tersalin, seperti di acuan: dibaca lewat tempatnya, bukan `b.f`.
                let fs = self.types.structs.get(sn).map(|l| l.fields.clone()).unwrap_or_default();
                let mut dari = Vec::with_capacity(fs.len());
                for (idx, (f, fty)) in fs.into_iter().enumerate() {
                    let r = self.fresh("sebar.f.");
                    self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", r, bty.llvm_ty(), tempat_b, idx));
                    let (p, t) = match fty { JrgType::Kotak(t) => (self.isi_kotak(&r), *t), t => (r, t) };
                    dari.push((f, self.lokal_sementara(p, t, span)));
                }
                let hasil_ty = JrgType::Enum(en.to_string());
                let slot = self.alloca_typed("sebar.", &hasil_ty);
                let akhir = self.fresh_label("sebar.end.");
                let r = rakit(self, &dari)?;
                if !self.is_terminated() {
                    let r = self.coerce(&r, &hasil_ty);
                    self.emit(format!("store {} {}, ptr {}", hasil_ty.llvm_ty(), r.text, slot));
                    self.terminate(format!("br label %{}", akhir));
                }
                self.start_block(akhir);
                Ok(self.load_place(&slot, &hasil_ty))
            }
            JrgType::Enum(bn) if bn == en => {
                let hasil_ty = JrgType::Enum(en.to_string());
                let slot = self.alloca_typed("sebar.", &hasil_ty);
                let tag = self.get_tag(&tempat_b, &bty);
                let akhir = self.fresh_label("sebar.end.");
                let label: Vec<String> = layout.variants.iter().map(|_| self.fresh_label("sebar.v.")).collect();
                let kasus: Vec<String> = layout.variants.iter().zip(&label).map(|(w, l)| format!("i32 {}, label %{}", w.tag, l)).collect();
                let mustahil = self.fresh_label("sebar.x.");
                self.terminate(format!("switch i32 {}, label %{} [ {} ]", tag, mustahil, kasus.join(" ")));
                for (w, l) in layout.variants.iter().zip(label) {
                    self.start_block(l);
                    let nama = w.payload.field_names();
                    let r = if nama.is_empty() {
                        self.panik(&crate::interpreter::pesan_sebar_bukan_wadah(&format!("enum:{}", super::modul::tampil(en))));
                        Val::unit()
                    } else {
                        let sl = w.payload.as_llvm_struct();
                        let mut dari = Vec::with_capacity(nama.len());
                        for (i, (f, ft)) in nama.iter().zip(w.payload.field_types()).enumerate() {
                            let x = self.get_payload_field(&tempat_b, &bty, &sl, i, &ft);
                            let ty = x.ty.clone();
                            let p = self.spill_to_place(&x);
                            dari.push((f.clone(), self.lokal_sementara(p, ty, span)));
                        }
                        rakit(self, &dari)?
                    };
                    if !self.is_terminated() {
                        let r = self.coerce(&r, &hasil_ty);
                        self.emit(format!("store {} {}, ptr {}", hasil_ty.llvm_ty(), r.text, slot));
                        self.terminate(format!("br label %{}", akhir));
                    }
                }
                self.start_block(mustahil);
                self.terminate("unreachable".to_string());
                self.start_block(akhir);
                Ok(self.load_place(&slot, &hasil_ty))
            }
            _ => Err(tandai(format!(
                "spread (`...x`) di literal varian `{}::{}` dari `{}` belum didukung backend `--emit-llvm` — yang bisa: \
                 struct, atau varian enum `{}` yang sama\nbantuan: tulis tiap field-nya, atau jalankan tanpa `--emit-llvm`",
                en, varian, bty.display_name(), en))),
        }
    }

    pub(crate) fn construct_user_enum_variant(&mut self, enum_name: &str, variant_name: &str, args: &[Expr], span: Span) -> Result<Val, String> {
        let harap = self.harapan_ktor.take();
        if self.param_enum(enum_name).is_some() {
            return self.ktor_enum_generik(enum_name, variant_name, args, harap.as_ref(), span);
        }
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
            let fty = &fty.tanpa_kotak();
            let v = self.codegen_expr(a, Some(fty))?;
            vals.push(self.coerce(&v, fty));
        }
        self.build_enum_value(enum_name, &layout, &variant, &vals)
    }

    /// `E::V(args)` atas enum generik: argumen dievaluasi SEKALI urut tulis, instansnya dari
    /// tipe yang diharapkan atau dari argumen itu (`generik::instans_ktor`).
    fn ktor_enum_generik(&mut self, dasar: &str, varian: &str, args: &[Expr], expected: Option<&JrgType>, span: Span) -> Result<Val, String> {
        let decl = self.tables.enums.get(dasar).cloned().ok_or_else(|| format!("enum `{}` tidak dikenal", dasar))?;
        let v = decl.variants.iter().find(|v| v.name == varian)
            .ok_or_else(|| format!("enum `{}` tidak punya variant `{}`", dasar, varian))?;
        let jumlah = match &v.kind { EnumVariantKind::Unit => 0, EnumVariantKind::Tuple(t) => t.len(), EnumVariantKind::Struct(f) => f.len() };
        if jumlah != args.len() {
            let pesan = if jumlah == 0 { crate::interpreter::pesan_bukan_fungsi(&format!("enum:{}", dasar)) }
                else { crate::interpreter::pesan_konstruktor_argumen(dasar, varian, jumlah, args.len()) };
            return Err(format!("[{}:{}] {}", span.lo.0, span.hi.0, pesan));
        }
        let tahu = match expected { Some(JrgType::Enum(e)) if self.enum_instans.get(e).map_or(false, |(d, _)| d == dasar) => Some(e.clone()), _ => None };
        let tipe_harap: Vec<Option<JrgType>> = match tahu.as_ref().and_then(|e| self.types.enums.get(e)).and_then(|l| l.variant(varian)) {
            Some(lv) => lv.payload.field_types().iter().map(|t| Some(t.tanpa_kotak())).collect(),
            None => vec![None; args.len()],
        };
        let mut vals = Vec::with_capacity(args.len());
        for (a, h) in args.iter().zip(&tipe_harap) { vals.push(self.codegen_expr(a, h.as_ref())?); }
        let en = self.instans_ktor(dasar, varian, &vals, expected)?;
        let layout = self.types.enums.get(&en).cloned().ok_or_else(|| format!("enum `{}` tidak dikenal", en))?;
        let variant = layout.variant(varian).cloned().ok_or("varian hilang")?;
        let tys = variant.payload.field_types();
        let vals: Vec<Val> = vals.iter().zip(&tys).map(|(x, t)| self.coerce(x, &t.tanpa_kotak())).collect();
        self.build_enum_value(&en, &layout, &variant, &vals)
    }

    /// Penanda penduga untuk nilai yang separuh tipenya belum terlihat (`None`, `Ok(..)`,
    /// `Err(..)` tanpa harapan): `Never` mengalah pada tipe dari `return` lain.
    pub(crate) fn nilai_belum_tahu() -> Val { Val { text: "undef".into(), ty: JrgType::Never } }

    /// Gabungkan dua tipe yang dilihat penduga: `Never` di mana pun diisi dari sisi lain.
    pub(crate) fn gabung_duga(lama: &JrgType, baru: &JrgType) -> JrgType {
        super::codegen_any::satukan_jenis(lama, baru)
    }

    /// `?` di `main` yang menemukan `Err`/`None`: program berhenti dengan kalimat yang sama dengan
    /// dua mesin lain (`interpreter::pesan_main_gagal`), lewat jalur panik — `defer` tetap jalan.
    fn main_berhenti(&mut self, tipe: &str, varian: &str, muatan: Option<Val>, span: Span) -> Result<(), String> {
        let teks = |t: String| Expr::Literal(LitKind::Str(t), span);
        let utuh = crate::interpreter::pesan_main_gagal(tipe, varian, if muatan.is_some() { ": \u{1}" } else { "" });
        let (awal, akhir) = utuh.split_once('\u{1}').map(|(a, b)| (a.to_string(), b.to_string()))
            .unwrap_or((utuh.clone(), String::new()));
        let pesan = match muatan {
            Some(v) => {
                let tempat = self.spill_to_place(&v);
                let isi = self.lokal_sementara(tempat, v.ty.clone(), span);
                let tampil = Expr::Call { func: Box::new(Expr::PathExpr {
                    segments: vec![String::new(), "str".into()], span }), args: vec![isi], span };
                let tengah = Expr::Binary { op: BinaryOp::Add, lhs: Box::new(teks(awal)), rhs: Box::new(tampil), span };
                Expr::Binary { op: BinaryOp::Add, lhs: Box::new(tengah), rhs: Box::new(teks(akhir)), span }
            }
            None => teks(awal),
        };
        let p = self.codegen_expr(&pesan, None)?;
        self.ensure_extern("declare void @jrg_panic(ptr, i64) cold nounwind noreturn", "jrg_panic");
        let (mp, ml) = self.split_str(&p);
        let situs_lama = std::mem::replace(&mut self.tanpa_situs, true);
        self.emit(format!("call void @jrg_panic(ptr {}, i64 {})", mp, ml));
        self.tanpa_situs = situs_lama;
        self.terminate("unreachable".to_string());
        Ok(())
    }

    /// `main -> Result/Option` yang pulang: `Err`/`None` berhenti seperti `?` di `main`,
    /// varian lain keluar 0 (acuan membuang isinya). `None` kalau tipenya bukan keduanya.
    pub(crate) fn main_pulang_opsi(&mut self, v: &Val) -> Option<Result<(), String>> {
        let JrgType::Enum(en) = &v.ty else { return None };
        let opsi = self.option_args.contains_key(en);
        let galat = self.result_args.get(en).map(|(_, e)| e.clone());
        if !opsi && galat.is_none() { return None; }
        let span = self.span_kini.unwrap_or_else(Span::dummy);
        let place = self.spill_to_place(v);
        let tagptr = self.fresh("mtag.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 0", tagptr, v.ty.llvm_ty(), place));
        let tagv = self.fresh("mtagv.");
        self.emit(format!("{} = load i32, ptr {}", tagv, tagptr));
        let buruk = self.fresh("mburuk.");
        self.emit(format!("{} = icmp eq i32 {}, 1", buruk, tagv));
        let gagal_lbl = self.fresh_label("mgagal.");
        let baik_lbl = self.fresh_label("mbaik.");
        self.terminate(format!("br i1 {}, label %{}, label %{}", buruk, gagal_lbl, baik_lbl));
        self.start_block(gagal_lbl);
        let hasil = match galat {
            None => self.main_berhenti("Option", "None", None, span),
            Some(e_ty) => {
                let blob = self.fresh("meblob.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", blob, v.ty.llvm_ty(), place));
                let fp = self.fresh("mefp.");
                self.emit(format!("{} = getelementptr {{ {} }}, ptr {}, i32 0, i32 0", fp, e_ty.llvm_ty(), blob));
                let ev = self.load_place(&fp, &e_ty);
                self.main_berhenti("Result", "Err", Some(ev), span)
            }
        };
        self.start_block(baik_lbl);
        self.emit_void_return();
        Some(hasil)
    }

    /// Satu separuh `Result` yang terlihat penduga: `Ok(None)` (isinya `!`) tidak memakukan tipe `Ok(Some(..))`.
    fn catat_separuh(slot: &mut Option<JrgType>, t: JrgType) {
        *slot = Some(match slot.take() { Some(lama) => Self::gabung_duga(&lama, &t), None => t });
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
                    Self::catat_separuh(&mut self.separuh_duga.0, t);
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
                    Self::catat_separuh(&mut self.separuh_duga.1, e);
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
        // Di penduga, `Ok(..)` tanpa harapan belum bertipe; isinya pun belum.
        if v.ty == JrgType::Never && self.mode_duga { return Ok(Self::nilai_belum_tahu()); }
        // Nilai bertag (hasil method atas handle asing): dibuka sebagai `Result`, tagnya diperiksa saat jalan.
        let v = if v.ty == JrgType::Any {
            let en = self.instantiate_result(&JrgType::Any, &JrgType::Any);
            self.buka_any(&v, &JrgType::Enum(en), span)?
        } else { v };
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
                JrgType::Enum(ren) if self.option_args.contains_key(ren) => Some(self.build_enum_unit_variant(ren, "None")?),
                // Penduga tipe kembalian: `return Some(..)` di tempat lain yang menyebut tipenya.
                _ if self.mode_duga && !self.is_main_fn => { self.terminate("unreachable".to_string()); None }
                _ if self.is_main_fn => None,
                // Badan `spawn`: hasilnya dibuang, jadi `None` cuma mengakhiri goroutine-nya (seperti acuan).
                JrgType::Unit if self.badan_spawn => Some(Val::unit()),
                JrgType::Any => {
                    let ren = self.instantiate_option(&t);
                    let n = self.build_enum_unit_variant(&ren, "None")?;
                    Some(self.kotak_any(&n, span)?)
                }
                _ => return Err(format!("[{}:{}] `?` pada Option butuh fungsi ini mengembalikan `Option<..>` juga", span.lo.0, span.hi.0)),
            };
            match noneval {
                Some(n) => self.emit_return(&n, &ret_ty),
                None if self.is_terminated() => {}
                None => self.main_berhenti("Option", "None", None, span)?,
            }

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
            match &ret_ty {
                JrgType::Enum(ren) if self.result_args.contains_key(ren) => {
                    let rlayout = self.types.enums.get(ren).cloned().unwrap();
                    let rvariant = rlayout.variant("Err").cloned().unwrap();
                    let new_err = self.build_enum_value(ren, &rlayout, &rvariant, &[errval])?;
                    self.emit_return(&new_err, &ret_ty);
                }
                _ if self.is_main_fn => self.main_berhenti("Result", "Err", Some(errval), span)?,
                // Penduga tipe kembalian: `?` menyumbang separuh `E`, `return Ok(..)` separuh `T`.
                _ if self.mode_duga => {
                    Self::catat_separuh(&mut self.separuh_duga.1, e_ty.clone());
                    self.terminate("unreachable".to_string());
                }
                JrgType::Unit if self.badan_spawn => self.emit_return(&Val::unit(), &ret_ty),
                // Fungsi yang hasilnya nilai bertag (`return db` di cabang lain): `Err`-nya ikut dikotak.
                JrgType::Any => {
                    let ren = self.instantiate_result(&JrgType::Any, &e_ty);
                    let rlayout = self.types.enums.get(&ren).cloned().unwrap();
                    let rvariant = rlayout.variant("Err").cloned().unwrap();
                    let new_err = self.build_enum_value(&ren, &rlayout, &rvariant, &[errval])?;
                    let k = self.kotak_any(&new_err, span)?;
                    self.emit_return(&k, &ret_ty);
                }
                _ => return Err(format!("[{}:{}] `?` pada Result butuh fungsi ini mengembalikan `Result<..>` juga", span.lo.0, span.hi.0)),
            }

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
