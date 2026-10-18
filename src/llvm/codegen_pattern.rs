// src/llvm/codegen_pattern.rs — jrgc LLVM backend — src/llvm/codegen_pattern.rs
// `match` (statement + expression) and pattern testing/binding.

#![allow(dead_code)]

use crate::parser::*;
use crate::lexer::Span;

use super::codegen::{Codegen, Val};
use super::codegen_expr::fmt_hexfloat;
use super::types::*;

impl Codegen {
    pub fn codegen_match_stmt(&mut self, m: &MatchStmt) -> Result<(), String> {
        self.codegen_match_core(&m.scrutinee, &m.arms, None, m.span)?;
        Ok(())
    }

    pub fn codegen_match_expr(&mut self, scrutinee: &Expr, arms: &[MatchArm], expected: Option<&JrgType>, span: Span) -> Result<Val, String> {
        // `match` sebagai NILAI yang tipe lengannya tidak terbaca sekilas (`Some(v) => [v]`):
        // diturunkan dulu di salinan untuk membaca tipe lengannya. Dulu nilainya dibuang dan
        // hasilnya `()` tanpa pesan.
        if expected.is_none() && self.tipe_hasil_arm(arms).is_none() {
            let mut salinan = self.clone();
            salinan.rekam_lengan = Some(salinan.kedalaman_match + 1);
            salinan.lengan_terlihat = None;
            let _ = salinan.codegen_match_core(scrutinee, arms, None, span);
            if let Some(t) = salinan.lengan_terlihat {
                // Lengan berupa array boleh berbeda panjang (`[v]` lawan `[]`): hasilnya array biasa.
                let t = match t { JrgType::Array(e, _) => JrgType::Slice(e), lain => lain };
                return self.codegen_match_core(scrutinee, arms, Some(&t), span);
            }
        }
        self.codegen_match_core(scrutinee, arms, expected, span)
    }

    /// Tipe hasil sebuah `match` yang konteksnya tidak menyebutkan tipe.
    pub(crate) fn tipe_hasil_arm(&self, arms: &[MatchArm]) -> Option<JrgType> {
        self.tipe_hasil_arm_dgn(arms, None)
    }

    /// Sama, tapi tipe SKRUTININYA sudah diketahui.
    pub(crate) fn tipe_hasil_arm_dgn(
        &self, arms: &[MatchArm], skrutini: Option<&JrgType>,
    ) -> Option<JrgType> {
        for arm in arms {
            let t = match &arm.body {
                MatchBody::Expr(e) => self.peek_expr_type(e)
                    .or_else(|| super::codegen_expr::tipe_bawaan_ekspresi(e))
                    .or_else(|| self.tipe_nama_terikat(arm, e, skrutini)),
                MatchBody::Block(b) => b.tail.as_ref().and_then(|e| {
                    self.peek_expr_type(e)
                        .or_else(|| super::codegen_expr::tipe_bawaan_ekspresi(e))
                        .or_else(|| self.tipe_nama_terikat(arm, e, skrutini))
                }),
            };
            if let Some(t) = t {
                if t != JrgType::Unit && t != JrgType::Never {
                    return Some(t);
                }
            }
        }
        None
    }

    /// Tipe sebuah nama yang diikat pola arm ini, kalau badan arm-nya memang
    /// persis nama itu.
    fn tipe_nama_terikat(
        &self, arm: &MatchArm, badan: &Expr, skrutini: Option<&JrgType>,
    ) -> Option<JrgType> {
        let sty = skrutini?;
        let mut ikatan = Vec::new();
        for pola in &arm.patterns {
            self.tipe_pengikat(pola, sty, &mut ikatan);
        }
        let cari = |n: &str| ikatan.iter().find(|(m, _)| m == n).map(|(_, t)| t.clone());
        match badan {
            Expr::Ident(n, _) => cari(n),
            // `let (a, b) = … else …` diturunkan jadi lengan bertubuh TUPLE
            // dari nama-nama terikatnya, jadi bentuk ini bukan kasus khusus
            // yang dikarang — ia yang paling sering muncul.
            Expr::Tuple { elems, .. } => {
                let mut ts = Vec::with_capacity(elems.len());
                for e in elems {
                    let Expr::Ident(n, _) = e else { return None };
                    ts.push(cari(n)?);
                }
                Some(JrgType::Tuple(ts))
            }
            _ => None,
        }
    }

    /// Nama yang diikat `pola` beserta tipenya, kalau nilai yang dicocokkan
    /// bertipe `ty`.
    fn tipe_pengikat(&self, pola: &Pattern, ty: &JrgType, keluar: &mut Vec<(String, JrgType)>) {
        match pola {
            // Nama varian unit (`None`, `E::A` telanjang) MENGUJI, tidak mengikat — dulu `None => None`
            // terbaca bertipe skrutini, jadi `Some(v) => Some(str(v))` dipaksa ke tipe lamanya.
            Pattern::Ident { name, .. } if name == "None" || !self.enum_punya_varian(name, true).is_empty() => {}
            Pattern::Ident { name, .. } => {
                if name != "_" { keluar.push((name.clone(), ty.clone())); }
            }
            Pattern::Binding { name, inner, .. } => {
                keluar.push((name.clone(), ty.clone()));
                self.tipe_pengikat(inner, ty, keluar);
            }
            Pattern::Ref { inner, .. } => self.tipe_pengikat(inner, ty, keluar),
            Pattern::Tuple { elems, .. } => {
                if let JrgType::Tuple(ts) = ty {
                    for (p, t) in elems.iter().zip(ts.iter()) {
                        self.tipe_pengikat(p, t, keluar);
                    }
                }
            }
            Pattern::Enum { variant, inner: Some(dalam), .. } => {
                let JrgType::Enum(en) = ty else { return };
                let Some(layout) = self.types.enums.get(en) else { return };
                let Some(v) = layout.variant(variant) else { return };
                let f = v.payload.field_types();
                if f.len() == 1 {
                    self.tipe_pengikat(dalam, &f[0], keluar);
                } else if let Pattern::Tuple { elems, .. } = dalam.as_ref() {
                    for (p, t) in elems.iter().zip(f.iter()) {
                        self.tipe_pengikat(p, t, keluar);
                    }
                }
            }
            Pattern::Array { before, rest, after, .. } => {
                let elem = match ty {
                    JrgType::Array(e, _) | JrgType::Slice(e) => (**e).clone(),
                    _ => return,
                };
                for p in before { self.tipe_pengikat(p, &elem, keluar); }
                if let Some(Some(n)) = rest {
                    keluar.push((n.clone(), JrgType::Slice(Box::new(elem.clone()))));
                }
                for p in after { self.tipe_pengikat(p, &elem, keluar); }
            }
            Pattern::Struct { fields, .. } => {
                let JrgType::Struct(nama) = ty else { return };
                let Some(layout) = self.types.structs.get(nama).cloned() else { return };
                for f in fields {
                    if let Some(ft) = layout.field_type(&f.name) {
                        self.tipe_pengikat(&f.pat, &ft.clone(), keluar);
                    }
                }
            }
            _ => {}
        }
    }

    fn codegen_match_core(&mut self, scrutinee: &Expr, arms: &[MatchArm], expected: Option<&JrgType>, span: Span) -> Result<Val, String> {
        if arms.is_empty() {
            return Err(format!("[{}:{}] `match` butuh minimal satu arm", span.lo.0, span.hi.0));
        }
        self.kedalaman_match += 1;
        let hasil = self.codegen_match_inti(scrutinee, arms, expected);
        self.kedalaman_match = self.kedalaman_match.saturating_sub(1);
        hasil
    }

    fn codegen_match_inti(&mut self, scrutinee: &Expr, arms: &[MatchArm], expected: Option<&JrgType>) -> Result<Val, String> {
        let sv = self.codegen_expr(scrutinee, None)?;
        let splace = self.spill_to_place(&sv);
        let sty = sv.ty.clone();

        // Tanpa tipe harapan, tipe hasilnya DISIMPULKAN dari arm-nya.
        let result_ty = match expected.cloned() {
            Some(t) => t,
            None => self.tipe_hasil_arm_dgn(arms, Some(&sty)).unwrap_or(JrgType::Unit),
        };
        let need_result = result_ty != JrgType::Unit;
        let result_slot = if need_result {
            Some(self.alloca_typed("match.slot.", &result_ty))
        } else {
            None
        };

        let merge_lbl = self.fresh_label("match.end.");
        let n = arms.len();

        for (i, arm) in arms.iter().enumerate() {
            let next_lbl = self.fresh_label("match.next.");
            self.push_scope();
            self.try_arm(arm, &splace, &sty, &next_lbl)?;
            let bv = match &arm.body {
                MatchBody::Expr(e) => self.codegen_expr(e, if need_result { Some(&result_ty) } else { None })?,
                MatchBody::Block(b) => self.codegen_block_expr(b, if need_result { Some(&result_ty) } else { None })?,
            };
            if !need_result && self.rekam_lengan == Some(self.kedalaman_match) && self.lengan_terlihat.is_none()
                && !matches!(bv.ty, JrgType::Unit | JrgType::Never)
            {
                self.lengan_terlihat = Some(bv.ty.clone());
            }
            if !self.is_terminated() {
                if need_result {
                    let bv2 = self.coerce(&bv, &result_ty);
                    self.emit(format!("store {} {}, ptr {}", result_ty.llvm_ty(), bv2.text, result_slot.clone().unwrap()));
                }
                self.terminate(format!("br label %{}", merge_lbl));
            }
            self.pop_scope();
            self.start_block(next_lbl);
            if i + 1 == n {
                self.ensure_extern(
                    "declare void @jrg_panic(ptr, i64) cold nounwind noreturn",
                    "jrg_panic",
                );
                let msg = self.intern_string("match: tidak ada pattern yang cocok (non-exhaustive)");
                let (mp, ml) = self.split_str(&msg);
                self.emit(format!("call void @jrg_panic(ptr {}, i64 {})", mp, ml));
                self.terminate("unreachable".to_string());
            }
        }

        self.start_block(merge_lbl);
        if need_result {
            Ok(self.load_place(&result_slot.unwrap(), &result_ty))
        } else {
            Ok(Val::unit())
        }
    }

    /// Pola array: `[]`, `[a, b]`, `[k, ...sisa]`, `[k, ...]`, `[a, ..., z]`.
    fn pola_array(
        &mut self,
        before: &[Pattern],
        rest: &Option<Option<String>>,
        after: &[Pattern],
        place: &str,
        ty: &JrgType,
        on_fail: &str,
        span: Span,
    ) -> Result<(), String> {
        let elem = match ty {
            JrgType::Array(e, _) | JrgType::Slice(e) => (**e).clone(),
            other => return Err(super::galat::tandai(span, format!(
                "pola array tidak cocok dengan tipe `{}`", other.display_name()))),
        };
        let (dptr, len) = self.data_dan_panjang(place, ty, &elem);
        let ety = elem.llvm_ty();

        // ── panjang ──
        let n_awal = before.len();
        let n_akhir = after.len();
        let cocok = self.fresh("apl.");
        if rest.is_none() {
            self.emit(format!("{} = icmp eq i64 {}, {}", cocok, len, n_awal));
        } else {
            self.emit(format!("{} = icmp sge i64 {}, {}", cocok, len, n_awal + n_akhir));
        }
        self.branch_or_fail(cocok, on_fail);

        // ── elemen depan ──
        for (i, p) in before.iter().enumerate() {
            let ep = self.fresh("apd.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", ep, ety, dptr, i));
            self.test_pattern(p, &ep, &elem, on_fail)?;
        }

        // ── elemen belakang, dihitung dari ujung ──
        for (j, p) in after.iter().enumerate() {
            let mundur = n_akhir - j;
            let idx = self.fresh("api.");
            self.emit(format!("{} = sub i64 {}, {}", idx, len, mundur));
            let ep = self.fresh("apb.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", ep, ety, dptr, idx));
            self.test_pattern(p, &ep, &elem, on_fail)?;
        }

        // ── sisanya, kalau diberi nama ──
        if let Some(Some(nama)) = rest {
            let hi = self.fresh("aph.");
            self.emit(format!("{} = sub i64 {}, {}", hi, len, n_akhir));
            let panjang = self.fresh("apn.");
            self.emit(format!("{} = sub i64 {}, {}", panjang, hi, n_awal));

            let ukuran = self.types.sizeof(&elem);
            let align = self.types.alignof(&elem);
            self.ensure_extern("declare ptr @jrg_alloc(i64, i64)", "jrg_alloc");
            self.ensure_extern(
                "declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)",
                "llvm.memcpy.p0.p0.i64");
            let byte = self.fresh("apby.");
            self.emit(format!("{} = mul i64 {}, {}", byte, panjang, ukuran));
            let buf = self.fresh("apbuf.");
            self.emit(format!("{} = call ptr @jrg_alloc(i64 {}, i64 {})", buf, byte, align));
            let asal = self.fresh("apsrc.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", asal, ety, dptr, n_awal));
            self.emit(format!(
                "call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)",
                buf, asal, byte));

            let sty = JrgType::Slice(Box::new(elem.clone()));
            let a = self.fresh("apv.");
            self.emit(format!("{} = insertvalue %JrgSlice undef, ptr {}, 0", a, buf));
            let bb = self.fresh("apv.");
            self.emit(format!("{} = insertvalue %JrgSlice {}, i64 {}, 1", bb, a, panjang));
            let slot = self.alloca_local(nama, sty.clone());
            self.emit(format!("store {} {}, ptr {}", sty.llvm_ty(), bb, slot));
        }
        Ok(())
    }

    /// (penunjuk elemen pertama, panjang) dari sebuah TEMPAT bertipe koleksi.
    pub fn data_dan_panjang(&mut self, place: &str, ty: &JrgType, elem: &JrgType)
        -> (String, String)
    {
        match ty {
            JrgType::Array(_, n) => {
                let p = self.fresh("dpa.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i64 0, i64 0",
                                  p, ty.llvm_ty(), place));
                (p, n.to_string())
            }
            JrgType::Slice(_) => {
                let pf = self.fresh("dps.");
                self.emit(format!("{} = getelementptr %JrgSlice, ptr {}, i32 0, i32 0", pf, place));
                let p = self.fresh("dpp.");
                self.emit(format!("{} = load ptr, ptr {}", p, pf));
                let lf = self.fresh("dpl.");
                self.emit(format!("{} = getelementptr %JrgSlice, ptr {}, i32 0, i32 1", lf, place));
                let l = self.fresh("dpn.");
                self.emit(format!("{} = load i64, ptr {}", l, lf));
                (p, l)
            }
            _ => {
                let _ = elem;
                (place.to_string(), "0".to_string())
            }
        }
    }

    fn try_arm(&mut self, arm: &MatchArm, splace: &str, sty: &JrgType, on_fail: &str) -> Result<(), String> {
        if arm.patterns.len() == 1 {
            self.test_pattern(&arm.patterns[0], splace, sty, on_fail)?;
        } else {
            let mut acc: Option<String> = None;
            for p in &arm.patterns {
                let c = self.pattern_condition_only(p, splace, sty)?;
                acc = Some(match acc {
                    None => c,
                    Some(a) => { let r = self.fresh("armor."); self.emit(format!("{} = or i1 {}, {}", r, a, c)); r }
                });
            }
            let cond = acc.unwrap_or_else(|| "true".to_string());
            self.branch_or_fail(cond, on_fail);
        }
        if let Some(g) = &arm.guard {
            let gv = self.codegen_expr(g, Some(&JrgType::Bool))?;
            let gv = self.coerce_longgar(&gv, &JrgType::Bool);
            let gi1 = self.to_i1(&gv);
            self.branch_or_fail(gi1, on_fail);
        }
        Ok(())
    }

    fn branch_or_fail(&mut self, cond_i1: String, on_fail: &str) {
        let cont = self.fresh_label("test.ok.");
        self.terminate(format!("br i1 {}, label %{}, label %{}", cond_i1, cont, on_fail));
        self.start_block(cont);
    }

    // ── binding-free condition (used for `pat1 | pat2 | ...`) ────────────

    fn pattern_condition_only(&mut self, pat: &Pattern, place: &str, ty: &JrgType) -> Result<String, String> {
        match pat {
            Pattern::Wildcard(_) => Ok("true".to_string()),
            Pattern::Ident { name, .. } if name == "_" => Ok("true".to_string()),
            Pattern::Literal { lit, .. } => self.literal_pattern_cond(lit, place, ty),
            Pattern::Range { lo, hi, inclusive, .. } => self.range_pattern_cond(lo, hi, *inclusive, place, ty),
            Pattern::Ref { inner, .. } => self.pattern_condition_only(inner, place, ty),
            Pattern::Or { pats, .. } => {
                let mut acc: Option<String> = None;
                for p in pats {
                    let c = self.pattern_condition_only(p, place, ty)?;
                    acc = Some(match acc {
                        None => c,
                        Some(a) => { let r = self.fresh("orc."); self.emit(format!("{} = or i1 {}, {}", r, a, c)); r }
                    });
                }
                Ok(acc.unwrap_or_else(|| "true".to_string()))
            }
            _ => Err("pattern dengan binding (identifier/tuple/struct/enum) tidak didukung di dalam alternasi `pat1 | pat2` atau arm multi-pattern — pisahkan jadi arm tersendiri".into()),
        }
    }

    // ── full test + bind ──────────────────────────────────────────────────

    pub(crate) fn test_pattern(&mut self, pat: &Pattern, place: &str, ty: &JrgType, on_fail: &str) -> Result<(), String> {
        // Pola varian atas nilai BERTAG (`match hasil[1] { Pindah::Tetap => .. }`): tipe enumnya dari
        // pola itu sendiri; kotak berisi tipe lain berarti pola ini tidak cocok, bukan galat.
        if *ty == JrgType::Any {
            if let Pattern::Enum { ty: pty, .. } = pat {
                if let Some(et) = self.enum_dari_pola(pty) {
                    let kata = self.kata_buram(&et);
                    self.pakai_buram = true;
                    let v = self.load_place(place, &JrgType::Any);
                    let (w, p) = self.bagian_buram(&v);
                    let cocok = self.fresh("pbe.");
                    self.emit(format!("{} = icmp eq i32 {}, {}", cocok, w, kata));
                    let lanjut = self.fresh_label("pbe.ok.");
                    self.terminate(format!("br i1 {}, label %{}, label %{}", cocok, lanjut, on_fail));
                    self.start_block(lanjut);
                    let isi = self.load_place(&p, &et);
                    let tempat = self.spill_to_place(&isi);
                    return self.test_pattern(pat, &tempat, &et, on_fail);
                }
            }
        }
        match pat {
            Pattern::Wildcard(_) => Ok(()),
            // `n @ pola`. Diikat lebih dulu, lalu polanya diuji: kalau
            // ujinya gagal kita melompat ke `on_fail` dan slot itu tidak
            // pernah dibaca, jadi urutannya tidak bisa membocorkan apa pun.
            Pattern::Binding { name, inner, .. } => {
                if name != "_" {
                    let v = self.load_place(place, ty);
                    // Nama baru = pemilik baru: tanpa salinan, menulis ke nama ini (atau
                    // mengembalikannya) mengubah nilai yang dicocokkan.
                    let v = if self.butuh_salin(ty) { self.salin_dalam(&v) } else { v };
                    let slot = self.alloca_local(name, ty.clone());
                    if *ty != JrgType::Unit {
                        self.emit(format!("store {} {}, ptr {}", ty.llvm_ty(), v.text, slot));
                    }
                }
                self.test_pattern(inner, place, ty, on_fail)
            }
            // Ditolak terang-terangan: backend yang belum mendukung sebuah
            // konstruksi harus bilang, bukan diam-diam menghasilkan
            // program dengan arti berbeda dari interpreter.
            Pattern::Array { before, rest, after, span } =>
                self.pola_array(before, rest, after, place, ty, on_fail, *span),
            Pattern::Ident { name, .. } => {
                // Nama telanjang biasanya MENGIKAT dan tidak pernah menolak — kecuali kalau
                // ia ternyata varian enum TANPA PAYLOAD.
                if name != "_" {
                    if let JrgType::Enum(ename) = ty {
                        if let Some(layout) = self.types.enums.get(ename).cloned() {
                            if let Some(v) = layout.variant(name).cloned() {
                                if v.payload.field_types().is_empty() {
                                    self.test_enum_tag(place, ty, &v, on_fail);
                                    return Ok(());
                                }
                            }
                        }
                    }
                    // Varian unit milik enum LAIN: nama itu tetap menguji, jadi tidak pernah cocok
                    // dengan nilai berjenis lain — dulu ia mengikat dan menelan lengannya.
                    let varian_unit = name == "None" || self.tables.enums.values().any(|en|
                        en.variants.iter().any(|v| &v.name == name
                            && matches!(v.kind, EnumVariantKind::Unit)));
                    if varian_unit {
                        self.branch_or_fail("false".to_string(), on_fail);
                        return Ok(());
                    }
                    let v = self.load_place(place, ty);
                    // Nama baru = pemilik baru: tanpa salinan, menulis ke nama ini (atau
                    // mengembalikannya) mengubah nilai yang dicocokkan.
                    let v = if self.butuh_salin(ty) { self.salin_dalam(&v) } else { v };
                    let slot = self.alloca_local(name, ty.clone());
                    if *ty != JrgType::Unit {
                        self.emit(format!("store {} {}, ptr {}", ty.llvm_ty(), v.text, slot));
                    }
                }
                Ok(())
            }
            Pattern::Ref { inner, .. } => self.test_pattern(inner, place, ty, on_fail),
            Pattern::Literal { lit, .. } => {
                let cond = self.literal_pattern_cond(lit, place, ty)?;
                self.branch_or_fail(cond, on_fail);
                Ok(())
            }
            Pattern::Range { lo, hi, inclusive, .. } => {
                let cond = self.range_pattern_cond(lo, hi, *inclusive, place, ty)?;
                self.branch_or_fail(cond, on_fail);
                Ok(())
            }
            Pattern::Or { pats, .. } => {
                let mut acc: Option<String> = None;
                for p in pats {
                    let c = self.pattern_condition_only(p, place, ty)?;
                    acc = Some(match acc {
                        None => c,
                        Some(a) => { let r = self.fresh("orc."); self.emit(format!("{} = or i1 {}, {}", r, a, c)); r }
                    });
                }
                let cond = acc.unwrap_or_else(|| "true".to_string());
                self.branch_or_fail(cond, on_fail);
                Ok(())
            }
            Pattern::Tuple { elems, .. } => {
                let tys = match ty {
                    JrgType::Tuple(ts) => ts.clone(),
                    other => return Err(format!("pola tuple tidak cocok dengan tipe `{}`", other.display_name())),
                };
                if tys.len() != elems.len() {
                    return Err("jumlah elemen pola tuple tidak cocok dengan tipe".into());
                }
                for (i, (p, fty)) in elems.iter().zip(tys.iter()).enumerate() {
                    let fp = self.fresh("tpf.");
                    self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", fp, ty.llvm_ty(), place, i));
                    self.test_pattern(p, &fp, fty, on_fail)?;
                }
                Ok(())
            }
            Pattern::Struct { ty: pty, fields, .. } => {
                match ty {
                    JrgType::Struct(sname) => {
                        let layout = self.types.structs.get(sname).cloned().ok_or_else(|| format!("struct `{}` tidak dikenal", sname))?;
                        let ada: Vec<String> = {
                            let mut v: Vec<String> = layout.fields.iter().map(|(n, _)| n.clone()).collect();
                            v.sort();
                            v
                        };
                        for fp in fields {
                            let idx = layout.field_index(&fp.name).ok_or_else(|| {
                                let saran = crate::diag::saran_nama_terdekat(
                                    &fp.name, ada.iter().map(|x| x.as_str()));
                                crate::interpreter::galat_index::field_pola_tak_ada(
                                    sname, &fp.name, &ada, saran.as_deref())
                            })?;
                            let fty = layout.fields[idx].1.clone();
                            let addr = self.fresh("spf.");
                            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", addr, ty.llvm_ty(), place, idx));
                            self.test_pattern(&fp.pat, &addr, &fty, on_fail)?;
                        }
                        Ok(())
                    }
                    JrgType::Enum(ename) => {
                        let variant_name = type_expr_pattern_name(pty)?;
                        let layout = self.types.enums.get(ename).cloned().ok_or_else(|| format!("enum `{}` tidak dikenal", ename))?;
                        let variant = layout.variant(&variant_name).cloned()
                            .ok_or_else(|| format!("enum `{}` tidak punya variant `{}`", ename, variant_name))?;
                        self.test_enum_tag(place, ty, &variant, on_fail);
                        let payload_struct = variant.payload.as_llvm_struct();
                        let blob = self.fresh("spb.");
                        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", blob, ty.llvm_ty(), place));
                        let field_tys = variant.payload.field_types();
                        let ada: Vec<String> = {
                            let mut v = variant.payload.field_names();
                            v.sort();
                            v
                        };
                        for fp in fields {
                            let fidx = variant.payload.field_index_by_name(&fp.name)
                                .ok_or_else(|| {
                                    let saran = crate::diag::saran_nama_terdekat(
                                        &fp.name, ada.iter().map(|x| x.as_str()));
                                    crate::interpreter::galat_index::field_pola_tak_ada(
                                        &variant_name, &fp.name, &ada, saran.as_deref())
                                })?;
                            let fty = field_tys[fidx].clone();
                            let addr = self.fresh("spvf.");
                            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", addr, payload_struct, blob, fidx));
                            self.test_pattern(&fp.pat, &addr, &fty, on_fail)?;
                        }
                        Ok(())
                    }
                    other => Err(format!("pola struct `{{ .. }}` tidak cocok dengan tipe `{}`", other.display_name())),
                }
            }
            Pattern::Enum { ty: _pty, variant, inner, .. } => {
                match ty {
                    JrgType::Enum(ename) => {
                        let layout = self.types.enums.get(ename).cloned().ok_or_else(|| format!("enum `{}` tidak dikenal", ename))?;
                        let v = layout.variant(variant).cloned()
                            .ok_or_else(|| format!("enum `{}` tidak punya variant `{}`", ename, variant))?;
                        self.test_enum_tag(place, ty, &v, on_fail);
                        if let Some(subpat) = inner {
                            let fields = v.payload.field_types();
                            let payload_struct = v.payload.as_llvm_struct();
                            let blob = self.fresh("epb.");
                            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", blob, ty.llvm_ty(), place));
                            if fields.len() == 1 {
                                let addr = self.fresh("epf.");
                                self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 0", addr, payload_struct, blob));
                                self.test_pattern(subpat, &addr, &fields[0], on_fail)?;
                            } else if fields.len() > 1 {
                                match subpat.as_ref() {
                                    Pattern::Tuple { elems, .. } => {
                                        if elems.len() != fields.len() {
                                            return Err(crate::interpreter::pesan_subpola_varian(
                                                &format!("{}::{}", ename, variant), fields.len(), elems.len()));
                                        }
                                        for (i, (p, fty)) in elems.iter().zip(fields.iter()).enumerate() {
                                            let addr = self.fresh("epfm.");
                                            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", addr, payload_struct, blob, i));
                                            self.test_pattern(p, &addr, fty, on_fail)?;
                                        }
                                    }
                                    Pattern::Ident { name, .. } if name == "_" => {}
                                    Pattern::Wildcard(_) => {}
                                    _ => return Err(format!("variant `{}::{}` punya {} field — gunakan pola tuple `(a, b, ..)`", ename, variant, fields.len())),
                                }
                            }
                        }
                        Ok(())
                    }
                    JrgType::Any => Err(format!("pola enum tidak cocok dengan tipe `{}`", ty.display_name())),
                    // Nilai yang tipenya pasti bukan enum tidak pernah cocok dengan pola varian:
                    // jawaban dua mesin lain (`match p { E::Y => .. }` atas struct jatuh ke `_`).
                    _ => { self.branch_or_fail("false".to_string(), on_fail); Ok(()) }
                }
            }
        }
    }

    fn test_enum_tag(&mut self, place: &str, ty: &JrgType, variant: &EnumVariantLayout, on_fail: &str) {
        let tagp = self.fresh("etp.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 0", tagp, ty.llvm_ty(), place));
        let tagv = self.fresh("etv.");
        self.emit(format!("{} = load i32, ptr {}", tagv, tagp));
        let cmp = self.fresh("etc.");
        self.emit(format!("{} = icmp eq i32 {}, {}", cmp, tagv, variant.tag));
        self.branch_or_fail(cmp, on_fail);
    }

    /// Pola literal/rentang atas nilai BERTAG: tagnya harus segolongan dengan polanya (angka
    /// bulat dan pecahan saling terbanding, seperti di dua mesin lain), baru isinya dibandingkan.
    fn kondisi_pola_any(
        &mut self, contoh: &LitKind, semua_bulat: bool, place: &str,
        badan: &dyn Fn(&mut Self, &str, &JrgType) -> Result<String, String>,
    ) -> Result<String, String> {
        use super::codegen_any::{TAG_BOOL, TAG_FLOAT, TAG_INT, TAG_RUNE, TAG_STR};
        let nol = Span { lo: crate::lexer::BytePos(0), hi: crate::lexer::BytePos(0), file_id: 0 };
        let v = self.load_place(place, &JrgType::Any);
        let tag = self.tag_any(&v);
        let hasil = self.alloca_raw("pany.", "i1", Some(1));
        self.emit(format!("store i1 false, ptr {}", hasil));
        let l_end = self.fresh_label("pany.end.");
        let golongan: Vec<(i32, JrgType, bool)> = match contoh {
            LitKind::Int(_) | LitKind::Float(_) => vec![(TAG_INT, JrgType::I64, !semua_bulat), (TAG_FLOAT, JrgType::F64, false)],
            LitKind::Bool(_) => vec![(TAG_BOOL, JrgType::Bool, false)],
            LitKind::Str(_) | LitKind::RawStr(_) => vec![(TAG_STR, JrgType::Str, false)],
            LitKind::Rune(_) => vec![(TAG_RUNE, JrgType::Rune, false)],
            _ => return Err("pola ini tidak didukung atas nilai bertag".into()),
        };
        let label: Vec<String> = golongan.iter().map(|(t, _, _)| self.fresh_label(&format!("pany.{}.", t))).collect();
        let kasus: Vec<String> = golongan.iter().zip(&label).map(|((t, _, _), l)| format!("i32 {}, label %{}", t, l)).collect();
        self.terminate(format!("switch i32 {}, label %{} [ {} ]", tag.text, l_end, kasus.join(" ")));
        for ((_, t, ke_pecahan), l) in golongan.iter().zip(&label) {
            self.start_block(l.clone());
            let isi = self.buka_any(&v, t, nol)?;
            let isi = if *ke_pecahan { self.coerce_longgar(&isi, &JrgType::F64) } else { isi };
            let tempat = self.spill_to_place(&isi);
            let c = badan(self, &tempat, &isi.ty)?;
            self.emit(format!("store i1 {}, ptr {}", c, hasil));
            self.terminate(format!("br label %{}", l_end));
        }
        self.start_block(l_end);
        let r = self.fresh("panyr.");
        self.emit(format!("{} = load i1, ptr {}", r, hasil));
        Ok(r)
    }

    fn literal_pattern_cond(&mut self, lit: &LitKind, place: &str, ty: &JrgType) -> Result<String, String> {
        if *ty == JrgType::Any {
            let bulat = matches!(lit, LitKind::Int(_));
            return self.kondisi_pola_any(lit, bulat, place, &|cg, p, t| cg.literal_pattern_cond(lit, p, t));
        }
        let v = self.load_place(place, ty);
        match lit {
            LitKind::Int(n) => {
                if ty.is_float() {
                    let target = fmt_hexfloat(*n as f64);
                    let r = self.fresh("plc.");
                    self.emit(format!(
                        "{} = fcmp{} oeq {} {}, {}",
                        r,
                        self.fast_math_flag(),
                        ty.llvm_ty(),
                        v.text,
                        target
                    ));
                    Ok(r)
                } else {
                    let r = self.fresh("plc.");
                    self.emit(format!("{} = icmp eq {} {}, {}", r, ty.llvm_ty(), v.text, n));
                    Ok(r)
                }
            }
            LitKind::Float(f) => {
                let target = fmt_hexfloat(*f);
                let r = self.fresh("plc.");
                self.emit(format!(
                    "{} = fcmp{} oeq {} {}, {}",
                    r,
                    self.fast_math_flag(),
                    ty.llvm_ty(),
                    v.text,
                    target
                ));
                Ok(r)
            }
            LitKind::Bool(b) => {
                let r = self.fresh("plc.");
                self.emit(format!("{} = icmp eq i8 {}, {}", r, v.text, if *b { 1 } else { 0 }));
                Ok(r)
            }
            LitKind::Str(s) | LitKind::RawStr(s) => {
                self.ensure_extern("declare i32 @jrg_str_eq(ptr, i64, ptr, i64)", "jrg_str_eq");
                let sv = self.intern_string(s);
                let (ap, al) = self.split_str(&v);
                let (bp, bl) = self.split_str(&sv);
                let r0 = self.fresh("plc0.");
                self.emit(format!("{} = call i32 @jrg_str_eq(ptr {}, i64 {}, ptr {}, i64 {})", r0, ap, al, bp, bl));
                let r = self.fresh("plc.");
                self.emit(format!("{} = icmp ne i32 {}, 0", r, r0));
                Ok(r)
            }
            LitKind::Rune(c) => {
                let r = self.fresh("plc.");
                self.emit(format!("{} = icmp eq {} {}, {}", r, ty.llvm_ty(), v.text, *c as u32));
                Ok(r)
            }
            LitKind::Imag(_) => Err("pola bilangan kompleks tidak didukung".into()),
            LitKind::Nil => Err("pola `nil` tidak didukung".into()),
        }
    }

    fn range_pattern_cond(&mut self, lo: &Option<Box<Pattern>>, hi: &Option<Box<Pattern>>, inclusive: bool, place: &str, ty: &JrgType) -> Result<String, String> {
        if *ty == JrgType::Any {
            let batas: Vec<LitKind> = [lo, hi].iter().filter_map(|b| b.as_ref().and_then(|p| extract_lit(p).ok())).collect();
            let Some(contoh) = batas.first().cloned() else { return Ok("true".to_string()); };
            let bulat = batas.iter().all(|l| matches!(l, LitKind::Int(_)));
            return self.kondisi_pola_any(&contoh, bulat, place, &|cg, p, t| cg.range_pattern_cond(lo, hi, inclusive, p, t));
        }
        // Bilangan bulat lawan batas PECAHAN: dibandingkan sebagai pecahan, seperti dua mesin lain.
        let pecah = [lo, hi].iter().any(|b| b.as_ref().is_some_and(|p| matches!(extract_lit(p), Ok(LitKind::Float(_)))));
        if ty.is_int() && pecah {
            let v = self.load_place(place, ty);
            let (d, tempat) = (self.fresh("rpf."), self.alloca_typed("rpfp.", &JrgType::F64));
            let ubah = if ty.is_signed_int() { "sitofp" } else { "uitofp" };
            self.emit(format!("{} = {} {} {} to double", d, ubah, ty.llvm_ty(), v.text));
            self.emit(format!("store double {}, ptr {}", d, tempat));
            return self.range_pattern_cond(lo, hi, inclusive, &tempat, &JrgType::F64);
        }
        let v = self.load_place(place, ty);
        let mut acc: Option<String> = None;
        if let Some(l) = lo {
            let lit = extract_lit(l)?;
            let ltext = lit_operand(&lit, ty)?;
            let cmpop = if ty.is_float() { "fcmp" } else { "icmp" };
            let fast_math = if ty.is_float() { self.fast_math_flag() } else { "" };
            let pred = if ty.is_float() { "oge" } else if ty.is_signed_int() { "sge" } else { "uge" };
            let r = self.fresh("rlo.");
            self.emit(format!(
                "{} = {}{} {} {} {}, {}",
                r,
                cmpop,
                fast_math,
                pred,
                ty.llvm_ty(),
                v.text,
                ltext
            ));
            acc = Some(r);
        }
        if let Some(h) = hi {
            let lit = extract_lit(h)?;
            let htext = lit_operand(&lit, ty)?;
            let cmpop = if ty.is_float() { "fcmp" } else { "icmp" };
            let fast_math = if ty.is_float() { self.fast_math_flag() } else { "" };
            let pred = if ty.is_float() {
                if inclusive { "ole" } else { "olt" }
            } else if ty.is_signed_int() {
                if inclusive { "sle" } else { "slt" }
            } else if inclusive { "ule" } else { "ult" };
            let r = self.fresh("rhi.");
            self.emit(format!(
                "{} = {}{} {} {} {}, {}",
                r,
                cmpop,
                fast_math,
                pred,
                ty.llvm_ty(),
                v.text,
                htext
            ));
            acc = Some(match acc {
                None => r,
                Some(a) => { let r2 = self.fresh("rand."); self.emit(format!("{} = and i1 {}, {}", r2, a, r)); r2 }
            });
        }
        Ok(acc.unwrap_or_else(|| "true".to_string()))
    }

    // ── bare path expression (non-call): enum unit variants, math consts ──

    pub fn codegen_path_expr(&mut self, segments: &[String], _expected: Option<&JrgType>, span: Span) -> Result<Val, String> {
        // `::nama`: bawaan bahasa. Backend ini tidak punya modul, jadi yang perlu
        // dibuang cuma segmen kosongnya.
        if segments.first().is_some_and(|s| s.is_empty()) {
            if segments.len() > 2 {
                return self.codegen_path_expr(&segments[1..], _expected, span);
            }
            let nama = segments.get(1).map(|s| s.as_str()).unwrap_or("");
            let pesan = if crate::vm::ada_builtin(nama) {
                format!("`::{}` dipakai sebagai NILAI (tanpa dipanggil), dan backend `--emit-llvm` belum \
                         mendukung fungsi bawaan sebagai nilai\n\
                         bantuan: panggil langsung — `::{}(x)` — atau jalankan tanpa `--emit-llvm`", nama, nama)
            } else {
                crate::interpreter::pesan_bukan_bawaan(nama, None)
            };
            return Err(crate::llvm::galat::tandai(span, pesan));
        }
        // `modul::Enum::Varian` sebagai nilai: segmen modulnya dibuang, seperti pemanggilannya.
        if segments.len() == 3 && self.unit_alias(&segments[0]).is_some() {
            return self.codegen_path_expr(&segments[1..], _expected, span);
        }
        if segments.len() == 2 {
            if let Some(u) = self.unit_alias(&segments[0]) {
                return self.nilai_anggota_modul(u, &segments[0], &segments[1], _expected, span);
            }
            match (segments[0].as_str(), segments[1].as_str()) {
                // Daftarnya sengaja LENGKAP sesuai yang ada di dua backend lain.
                ("math", "PI") => return Ok(Val { text: fmt_hexfloat(std::f64::consts::PI), ty: JrgType::F64 }),
                ("math", "E") => return Ok(Val { text: fmt_hexfloat(std::f64::consts::E), ty: JrgType::F64 }),
                ("math", "TAU") => return Ok(Val { text: fmt_hexfloat(std::f64::consts::TAU), ty: JrgType::F64 }),
                ("math", "INF") => return Ok(Val { text: fmt_hexfloat(f64::INFINITY), ty: JrgType::F64 }),
                ("math", "NAN") => return Ok(Val { text: fmt_hexfloat(f64::NAN), ty: JrgType::F64 }),
                // `Option::None` berkualifikasi: aturan yang sama dengan `None` telanjang.
                ("Option", "None") => {
                    let t = match _expected {
                        Some(JrgType::Enum(en)) if self.option_args.contains_key(en) => self.option_args[en].clone(),
                        _ if self.mode_duga => return Ok(Self::nilai_belum_tahu()),
                        _ => JrgType::Any,
                    };
                    let en = self.instantiate_option(&t);
                    return self.build_enum_unit_variant(&en, "None");
                }
                _ => {}
            }
            if let Some(en) = self.tables.enums.get(&segments[0]).cloned() {
                if en.variants.iter().any(|v| v.name == segments[1] && matches!(v.kind, EnumVariantKind::Unit)) {
                    return self.build_enum_unit_variant(&segments[0], &segments[1]);
                }
            }
        }
        let jalur = segments.join("::");
        // Method milik program sebagai nilai: closure tanpa tangkapan, bernama seperti acuan
        // (`<fn T::m>`). Fungsi PUSTAKA sebagai nilai masih celah backend ini.
        if segments.len() == 2 {
            if let Some(decl) = self.tables.find_method(&segments[0], &segments[1]).cloned() {
                return self.nilai_metode(&segments[0], &decl, _expected, span);
            }
        }
        if segments.len() == 2 && crate::vm::ada_builtin(&jalur) {
            return Err(crate::llvm::galat::tandai(span, format!(
                "`{}` dipakai sebagai NILAI (tanpa dipanggil), dan backend `--emit-llvm` belum \
                 mendukung fungsi atau method sebagai nilai dalam bentuk path\n\
                 bantuan: panggil langsung — `{}(x)` — atau jalankan tanpa `--emit-llvm`", jalur, jalur)));
        }
        if let Some(r) = self.nilai_konstruktor(&jalur, _expected, span) { return r; }
        if let Some(p) = self.penjelasan_bukan_nilai(&jalur) {
            return Err(crate::llvm::galat::tandai(span, p));
        }
        Err(format!("[{}:{}] path `{}` tidak dikenal", span.lo.0, span.hi.0, jalur))
    }
}

impl Codegen {
    /// Tipe enum yang disebut sebuah pola varian, kalau ia enum TIDAK generik milik program.
    fn enum_dari_pola(&mut self, pty: &TypeExpr) -> Option<JrgType> {
        // Di pola, `package` memegang nama ENUM-nya dan `name` variannya (`Pindah::Tetap`).
        let TypeExpr::Named(nt) = pty else { return None };
        let en = nt.package.clone()?;
        if matches!(en.as_str(), "Option" | "Result") { return None; }
        let te = TypeExpr::Named(NamedType { package: None, modul: None, name: en, args: Vec::new(), span: nt.span });
        match self.resolve_type_pub(&te) {
            Ok(t @ JrgType::Enum(_)) => Some(t),
            _ => None,
        }
    }
}

fn type_expr_pattern_name(te: &TypeExpr) -> Result<String, String> {
    match te {
        TypeExpr::Named(nt) => Ok(nt.name.clone()),
        _ => Err("pola struct/enum tidak valid".into()),
    }
}

fn extract_lit(p: &Pattern) -> Result<LitKind, String> {
    match p {
        Pattern::Literal { lit, .. } => Ok(lit.clone()),
        _ => Err("batas pola range harus berupa literal (angka)".into()),
    }
}

fn lit_operand(lit: &LitKind, ty: &JrgType) -> Result<String, String> {
    match lit {
        LitKind::Int(n) => {
            if ty.is_float() { Ok(fmt_hexfloat(*n as f64)) } else { Ok(n.to_string()) }
        }
        LitKind::Float(f) => Ok(fmt_hexfloat(*f)),
        LitKind::Rune(c) => Ok((*c as u32).to_string()),
        _ => Err("batas pola range harus literal angka/rune".into()),
    }
}
