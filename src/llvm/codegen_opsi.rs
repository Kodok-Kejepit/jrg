// src/llvm/codegen_opsi.rs — metode bawaan Option/Result dan string

#![allow(dead_code)]

use crate::parser::*;
use crate::lexer::Span;

use super::codegen::{Codegen, Val};
use super::types::*;

impl Codegen {
    // ── Option<T> / Result<T,E> built-in methods ────────────────────────

    /// `o.map(f)`, `r.map_err(f)`, `o.and_then(f)`: diturunkan ke `match` yang sama dengan tulisan
    /// tangannya. Penerima sudah dievaluasi, lalu fungsinya — urutan sebuah pemanggilan method.
    fn turunkan_peta(&mut self, optr: &str, ty: JrgType, method: &str, f: &Expr, opsi: bool,
                     span: Span) -> Result<Val, String> {
        let penerima = self.lokal_sementara(optr.to_string(), ty, span);
        let (nf, nv) = (format!("$petaf{}", self.penerima_lapis), format!("$petav{}", self.penerima_lapis));
        let id = |n: &str| Expr::Ident(n.to_string(), span);
        let pola = |varian: &str, isi: bool| if isi {
            Pattern::Enum { ty: TypeExpr::Named(NamedType { package: None, modul: None, name: varian.to_string(),
                args: Vec::new(), span }), variant: varian.to_string(),
                inner: Some(Box::new(Pattern::Ident { name: nv.clone(), is_mut: false, span })), span }
        } else { Pattern::Ident { name: varian.to_string(), is_mut: false, span } };
        let panggil = |g: &str, x: Expr| Expr::Call { func: Box::new(id(g)), args: vec![x], span };
        let lengan = |p: Pattern, badan: Expr| MatchArm { patterns: vec![p], guard: None, body: MatchBody::Expr(Box::new(badan)), span };
        let (ok, gagal) = if opsi { ("Some", "None") } else { ("Ok", "Err") };
        let f_v = panggil(&nf, id(&nv));
        let arms = match (method, opsi) {
            ("map", true) => vec![lengan(pola(ok, true), panggil(ok, f_v)), lengan(pola(gagal, false), id(gagal))],
            ("and_then", true) => vec![lengan(pola(ok, true), f_v), lengan(pola(gagal, false), id(gagal))],
            ("map", false) => vec![lengan(pola(ok, true), panggil(ok, f_v)), lengan(pola(gagal, true), panggil(gagal, id(&nv)))],
            ("map_err", false) => vec![lengan(pola(ok, true), panggil(ok, id(&nv))), lengan(pola(gagal, true), panggil(gagal, f_v))],
            _ => vec![lengan(pola(ok, true), f_v), lengan(pola(gagal, true), panggil(gagal, id(&nv)))],
        };
        let ikat = Stmt::Let(LetStmt { is_mut: false, ownership: None,
            pattern: Pattern::Ident { name: nf.clone(), is_mut: false, span }, ty: None,
            init: Box::new(f.clone()), lifetime: None, span });
        let blok = Expr::Block(Block { stmts: vec![ikat],
            tail: Some(Box::new(Expr::Match { scrutinee: Box::new(penerima), arms, span })), span });
        self.codegen_expr(&blok, None)
    }

    /// `Option<T>` → `Option<nilai bertag>`: `Some(x)` jadi `Some(kotak(x))`, `None` tetap `None`.
    pub(crate) fn kotak_isi_opsi(&mut self, v: &Val, isi: &JrgType, sasaran: &str) -> Option<Val> {
        let asal_l = self.types.enums.get(match &v.ty { JrgType::Enum(n) => n, _ => return None })?.clone();
        let tuju_l = self.types.enums.get(sasaran)?.clone();
        let (vs_asal, vs_tuju) = (asal_l.variant("Some")?.clone(), tuju_l.variant("Some")?.clone());
        let tuju_ty = JrgType::Enum(sasaran.to_string());
        let tempat = self.spill_to_place(v);
        let slot = self.alloca_typed("kio.", &tuju_ty);
        let tag = self.get_tag(&tempat, &v.ty);
        let ada = self.fresh("kioa.");
        self.emit(format!("{} = icmp eq i32 {}, {}", ada, tag, vs_asal.tag));
        let (l_ada, l_tidak, l_end) = (self.fresh_label("kio.ada."), self.fresh_label("kio.tidak."), self.fresh_label("kio.end."));
        self.terminate(format!("br i1 {}, label %{}, label %{}", ada, l_ada, l_tidak));
        self.start_block(l_ada);
        let x = self.get_payload_field(&tempat, &v.ty, &vs_asal.payload.as_llvm_struct(), 0, isi);
        let nol = Span { lo: crate::lexer::BytePos(0), hi: crate::lexer::BytePos(0), file_id: 0 };
        let k = self.kotak_any(&x, nol).ok()?;
        let some = self.build_enum_value(sasaran, &tuju_l, &vs_tuju, &[k]).ok()?;
        self.emit(format!("store {} {}, ptr {}", tuju_ty.llvm_ty(), some.text, slot));
        self.terminate(format!("br label %{}", l_end));
        self.start_block(l_tidak);
        let none = self.build_enum_unit_variant(sasaran, "None").ok()?;
        self.emit(format!("store {} {}, ptr {}", tuju_ty.llvm_ty(), none.text, slot));
        self.terminate(format!("br label %{}", l_end));
        self.start_block(l_end);
        Some(self.load_place(&slot, &tuju_ty))
    }

    pub(crate) fn get_tag(&mut self, place: &str, ty: &JrgType) -> String {
        let tp = self.fresh("mtag.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 0", tp, ty.llvm_ty(), place));
        let tv = self.fresh("mtagv.");
        self.emit(format!("{} = load i32, ptr {}", tv, tp));
        tv
    }

    pub(crate) fn get_payload_field(&mut self, place: &str, ty: &JrgType, payload_llvm_struct: &str, field_idx: usize, field_ty: &JrgType) -> Val {
        let blob = self.fresh("mblob.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", blob, ty.llvm_ty(), place));
        let fp = self.fresh("mpf.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", fp, payload_llvm_struct, blob, field_idx));
        self.load_place(&fp, field_ty)
    }

    pub(crate) fn codegen_option_method(&mut self, optr: &str, ename: &str, t: &JrgType, method: &str, args: &[Expr], span: Span) -> Result<Val, String> {
        let ety = JrgType::Enum(ename.to_string());
        let payload_struct = format!("{{ {} }}", t.llvm_ty());
        match method {
            "is_some" | "is_none" => {
                let tagv = self.get_tag(optr, &ety);
                let cmp = self.fresh("opc.");
                self.emit(format!("{} = icmp eq i32 {}, 0", cmp, tagv));
                let out = self.from_i1(&cmp);
                if method == "is_none" {
                    let r = self.fresh("opn.");
                    let i1v = self.to_i1(&out);
                    self.emit(format!("{} = xor i1 {}, true", r, i1v));
                    Ok(self.from_i1(&r))
                } else { Ok(out) }
            }
            "unwrap" => {
                let tagv = self.get_tag(optr, &ety);
                let cmp = self.fresh("opu.");
                self.emit(format!("{} = icmp eq i32 {}, 1", cmp, tagv));
                let bad = self.fresh_label("optnone.");
                let ok = self.fresh_label("optsome.");
                self.terminate_unlikely_true(&cmp, &bad, &ok);
                self.start_block(bad);
                self.ensure_extern(
                    "declare void @jrg_panic_unwrap_none() cold nounwind noreturn",
                    "jrg_panic_unwrap_none",
                );
                self.emit("call void @jrg_panic_unwrap_none()".to_string());
                self.terminate("unreachable".to_string());
                self.start_block(ok);
                Ok(self.get_payload_field(optr, &ety, &payload_struct, 0, t))
            }
            "expect" => {
                if args.len() != 1 { return Err(format!("[{}:{}] `expect` butuh 1 argumen (pesan)", span.lo.0, span.hi.0)); }
                let msg = self.codegen_expr(&args[0], Some(&JrgType::Str))?;
                let tagv = self.get_tag(optr, &ety);
                let cmp = self.fresh("ope.");
                self.emit(format!("{} = icmp eq i32 {}, 1", cmp, tagv));
                let bad = self.fresh_label("optenone.");
                let ok = self.fresh_label("optesome.");
                self.terminate_unlikely_true(&cmp, &bad, &ok);
                self.start_block(bad);
                self.ensure_extern(
                    "declare void @jrg_panic(ptr, i64) cold nounwind noreturn",
                    "jrg_panic",
                );
                let (mp, ml) = self.split_str(&msg);
                self.emit(format!("call void @jrg_panic(ptr {}, i64 {})", mp, ml));
                self.terminate("unreachable".to_string());
                self.start_block(ok);
                Ok(self.get_payload_field(optr, &ety, &payload_struct, 0, t))
            }
            "unwrap_or" => {
                if args.len() != 1 { return Err(format!("[{}:{}] `unwrap_or` butuh 1 argumen", span.lo.0, span.hi.0)); }
                let tagv = self.get_tag(optr, &ety);
                let cmp = self.fresh("opuo.");
                self.emit(format!("{} = icmp eq i32 {}, 0", cmp, tagv));
                let some_lbl = self.fresh_label("uosome.");
                let none_lbl = self.fresh_label("uonone.");
                let merge_lbl = self.fresh_label("uoend.");
                let slot = self.alloca_typed("uoslot.", t);
                // Argumennya diturunkan SEBELUM percabangan.
                let dv = self.codegen_expr(&args[0], Some(t))?;
                let dv = self.coerce(&dv, t);
                self.terminate(format!("br i1 {}, label %{}, label %{}", cmp, some_lbl, none_lbl));
                self.start_block(some_lbl);
                let sv = self.get_payload_field(optr, &ety, &payload_struct, 0, t);
                self.emit(format!("store {} {}, ptr {}", t.llvm_ty(), sv.text, slot));
                self.terminate(format!("br label %{}", merge_lbl));
                self.start_block(none_lbl);
                self.emit(format!("store {} {}, ptr {}", t.llvm_ty(), dv.text, slot));
                self.terminate(format!("br label %{}", merge_lbl));
                self.start_block(merge_lbl);
                Ok(self.load_place(&slot, t))
            }
            "ok" if args.is_empty() => Ok(self.load_place(optr, &ety)),
            "map" | "and_then" if args.len() == 1 => self.turunkan_peta(optr, ety, method, &args[0], true, span),
            _ => Err(format!("[{}:{}] Option method `{}` belum didukung backend LLVM v1", span.lo.0, span.hi.0, method)),
        }
    }

    pub(crate) fn codegen_result_method(&mut self, optr: &str, ename: &str, t: &JrgType, e: &JrgType, method: &str, args: &[Expr], span: Span) -> Result<Val, String> {
        let ety = JrgType::Enum(ename.to_string());
        let ok_struct = format!("{{ {} }}", t.llvm_ty());
        let err_struct = format!("{{ {} }}", e.llvm_ty());
        match method {
            "is_ok" | "is_err" => {
                let tagv = self.get_tag(optr, &ety);
                let cmp = self.fresh("rec.");
                self.emit(format!("{} = icmp eq i32 {}, 0", cmp, tagv));
                let out = self.from_i1(&cmp);
                if method == "is_err" {
                    let r = self.fresh("ren.");
                    let i1v = self.to_i1(&out);
                    self.emit(format!("{} = xor i1 {}, true", r, i1v));
                    Ok(self.from_i1(&r))
                } else { Ok(out) }
            }
            "unwrap" => {
                let tagv = self.get_tag(optr, &ety);
                let cmp = self.fresh("reu.");
                self.emit(format!("{} = icmp eq i32 {}, 1", cmp, tagv));
                let bad = self.fresh_label("resuerr.");
                let ok = self.fresh_label("resuok.");
                self.terminate_unlikely_true(&cmp, &bad, &ok);
                self.start_block(bad);
                let errv = self.get_payload_field(optr, &ety, &err_struct, 0, e);
                let errs = self.val_to_str(&errv)?;
                self.ensure_extern(
                    "declare void @jrg_panic_unwrap_err(ptr, i64) cold nounwind noreturn",
                    "jrg_panic_unwrap_err",
                );
                let (mp, ml) = self.split_str(&errs);
                self.emit(format!("call void @jrg_panic_unwrap_err(ptr {}, i64 {})", mp, ml));
                self.terminate("unreachable".to_string());
                self.start_block(ok);
                Ok(self.get_payload_field(optr, &ety, &ok_struct, 0, t))
            }
            "unwrap_err" => {
                let tagv = self.get_tag(optr, &ety);
                let cmp = self.fresh("reue.");
                self.emit(format!("{} = icmp eq i32 {}, 0", cmp, tagv));
                let bad = self.fresh_label("resueok.");
                let ok = self.fresh_label("resueerr.");
                self.terminate_unlikely_true(&cmp, &bad, &ok);
                self.start_block(bad);
                self.ensure_extern(
                    "declare void @jrg_panic(ptr, i64) cold nounwind noreturn",
                    "jrg_panic",
                );
                let msg = self.intern_string("called `Result::unwrap_err()` on an `Ok` value");
                let (mp, ml) = self.split_str(&msg);
                self.emit(format!("call void @jrg_panic(ptr {}, i64 {})", mp, ml));
                self.terminate("unreachable".to_string());
                self.start_block(ok);
                Ok(self.get_payload_field(optr, &ety, &err_struct, 0, e))
            }
            "expect" => {
                if args.len() != 1 { return Err(format!("[{}:{}] `expect` butuh 1 argumen (pesan)", span.lo.0, span.hi.0)); }
                let msg = self.codegen_expr(&args[0], Some(&JrgType::Str))?;
                let tagv = self.get_tag(optr, &ety);
                let cmp = self.fresh("ree.");
                self.emit(format!("{} = icmp eq i32 {}, 1", cmp, tagv));
                let bad = self.fresh_label("reseerr.");
                let ok = self.fresh_label("reseok.");
                self.terminate_unlikely_true(&cmp, &bad, &ok);
                self.start_block(bad);
                self.ensure_extern(
                    "declare void @jrg_panic(ptr, i64) cold nounwind noreturn",
                    "jrg_panic",
                );
                let (mp, ml) = self.split_str(&msg);
                self.emit(format!("call void @jrg_panic(ptr {}, i64 {})", mp, ml));
                self.terminate("unreachable".to_string());
                self.start_block(ok);
                Ok(self.get_payload_field(optr, &ety, &ok_struct, 0, t))
            }
            "unwrap_or" => {
                if args.len() != 1 { return Err(format!("[{}:{}] `unwrap_or` butuh 1 argumen", span.lo.0, span.hi.0)); }
                let tagv = self.get_tag(optr, &ety);
                let cmp = self.fresh("reuo.");
                self.emit(format!("{} = icmp eq i32 {}, 0", cmp, tagv));
                let ok_lbl = self.fresh_label("ruook.");
                let err_lbl = self.fresh_label("ruoerr.");
                let merge_lbl = self.fresh_label("ruoend.");
                let slot = self.alloca_typed("ruoslot.", t);
                // Sama seperti `unwrap_or` pada Option: argumen sebuah
                // pemanggilan method dievaluasi tanpa kecuali, jadi ia
                // diturunkan SEBELUM percabangan.
                let dv = self.codegen_expr(&args[0], Some(t))?;
                let dv = self.coerce(&dv, t);
                self.terminate(format!("br i1 {}, label %{}, label %{}", cmp, ok_lbl, err_lbl));
                self.start_block(ok_lbl);
                let sv = self.get_payload_field(optr, &ety, &ok_struct, 0, t);
                self.emit(format!("store {} {}, ptr {}", t.llvm_ty(), sv.text, slot));
                self.terminate(format!("br label %{}", merge_lbl));
                self.start_block(err_lbl);
                self.emit(format!("store {} {}, ptr {}", t.llvm_ty(), dv.text, slot));
                self.terminate(format!("br label %{}", merge_lbl));
                self.start_block(merge_lbl);
                Ok(self.load_place(&slot, t))
            }
            // `r.ok()` / `r.err()`: paruh yang diminta sebagai `Option`.
            "ok" | "err" if args.is_empty() => {
                let (isi, st, tag) = if method == "ok" { (t, &ok_struct, 0) } else { (e, &err_struct, 1) };
                let en = self.instantiate_option(isi);
                let oty = JrgType::Enum(en.clone());
                let slot = self.alloca_typed("reo.", &oty);
                let kosong = self.build_enum_unit_variant(&en, "None")?;
                self.emit(format!("store {} {}, ptr {}", oty.llvm_ty(), kosong.text, slot));
                let tagv = self.get_tag(optr, &ety);
                let cmp = self.fresh("reoc.");
                self.emit(format!("{} = icmp eq i32 {}, {}", cmp, tagv, tag));
                let (ya, usai) = (self.fresh_label("reo.ya."), self.fresh_label("reo.usai."));
                self.terminate(format!("br i1 {}, label %{}, label %{}", cmp, ya, usai));
                self.start_block(ya);
                let v = self.get_payload_field(optr, &ety, st, 0, isi);
                let layout = self.types.enums.get(&en).cloned().ok_or_else(|| format!("enum `{}` tidak dikenal", en))?;
                let varian = layout.variant("Some").cloned().ok_or_else(|| format!("enum `{}` tidak punya variant `Some`", en))?;
                let some = self.build_enum_value(&en, &layout, &varian, &[v])?;
                self.emit(format!("store {} {}, ptr {}", oty.llvm_ty(), some.text, slot));
                self.terminate(format!("br label %{}", usai));
                self.start_block(usai);
                Ok(self.load_place(&slot, &oty))
            }
            "map" | "map_err" | "and_then" if args.len() == 1 => self.turunkan_peta(optr, ety, method, &args[0], false, span),
            _ => Err(format!("[{}:{}] Result method `{}` belum didukung backend LLVM v1", span.lo.0, span.hi.0, method)),
        }
    }

    /// Method string yang menerima argumen. Terpisah dari `try_str_method`
    /// hanya karena yang itu tidak melihat `args` sama sekali.
    pub(crate) fn metode_str_berargumen(&mut self, optr: &str, method: &str, args: &[Expr], span: Span)
        -> Option<Result<Val, String>>
    {
        // Nama predikat (`contains` `starts_with` `ends_with`) sama persis
        // dengan method array, jadi arity diperiksa DULU: kalau tidak cocok,
        // `None` — yang berarti "bukan urusan saya", bukan "salah pakai".
        let butuh = match method {
            "contains" | "starts_with" | "ends_with" | "split" | "find" | "index_of" => 1,
            "replace" => 2,
            _ => return None,
        };
        if args.len() != butuh { return None; }
        Some((|| {
            let sv = self.load_place(optr, &JrgType::Str);
            let (sp, sl) = self.split_str(&sv);
            let av = self.codegen_expr(&args[0], Some(&JrgType::Str))?;
            if av.ty != JrgType::Str {
                return Err(super::galat::tandai(span, format!(
                    "`{}` pada string butuh argumen string, diberi `{}`",
                    method, av.ty.display_name())));
            }
            let (ap, al) = self.split_str(&av);
            match method {
                // `find`/`index_of` — dua ejaan untuk satu hal, seperti di dua backend lain.
                "find" | "index_of" => {
                    self.ensure_extern(
                        "declare i64 @jrg_str_find(ptr, i64, ptr, i64)", "jrg_str_find");
                    let r = self.fresh("msf.");
                    self.emit(format!("{} = call i64 @jrg_str_find(ptr {}, i64 {}, ptr {}, i64 {})",
                        r, sp, sl, ap, al));
                    // `Option`, seperti dua backend lain: `-1` adalah indeks
                    // yang SAH di JRG, jadi sentinel di sini diam-diam jadi
                    // karakter terakhir.
                    self.indeks_ke_option(&r)
                }
                "contains" | "starts_with" | "ends_with" => {
                    let f = match method {
                        "contains" => "jrg_str_contains",
                        "starts_with" => "jrg_str_starts_with",
                        _ => "jrg_str_ends_with",
                    };
                    self.ensure_extern(&format!("declare i32 @{}(ptr, i64, ptr, i64)", f), f);
                    let r = self.fresh("msb.");
                    self.emit(format!("{} = call i32 @{}(ptr {}, i64 {}, ptr {}, i64 {})",
                        r, f, sp, sl, ap, al));
                    let b = self.fresh("msb8.");
                    self.emit(format!("{} = trunc i32 {} to i8", b, r));
                    Ok(Val { text: b, ty: JrgType::Bool })
                }
                "split" => {
                    self.ensure_extern(
                        "declare void @jrg_str_split(ptr, i64, ptr, i64, ptr, ptr)",
                        "jrg_str_split");
                    let op = self.alloca_raw("mspp.", "ptr", Some(8));
                    let ol = self.alloca_raw("mspl.", "i64", Some(8));
                    self.emit(format!(
                        "call void @jrg_str_split(ptr {}, i64 {}, ptr {}, i64 {}, ptr {}, ptr {})",
                        sp, sl, ap, al, op, ol));
                    let d = self.fresh("mspd.");
                    self.emit(format!("{} = load ptr, ptr {}", d, op));
                    let n = self.fresh("mspn.");
                    self.emit(format!("{} = load i64, ptr {}", n, ol));
                    let a = self.fresh("msps.");
                    self.emit(format!("{} = insertvalue %JrgSlice undef, ptr {}, 0", a, d));
                    let b = self.fresh("msps.");
                    self.emit(format!("{} = insertvalue %JrgSlice {}, i64 {}, 1", b, a, n));
                    Ok(Val { text: b, ty: JrgType::Slice(Box::new(JrgType::Str)) })
                }
                "replace" => {
                    let bv = self.codegen_expr(&args[1], Some(&JrgType::Str))?;
                    if bv.ty != JrgType::Str {
                        return Err(super::galat::tandai(span, format!(
                            "`replace` butuh argumen kedua string, diberi `{}`",
                            bv.ty.display_name())));
                    }
                    let (bp, bl) = self.split_str(&bv);
                    self.ensure_extern(
                        "declare void @jrg_str_replace(ptr, i64, ptr, i64, ptr, i64, ptr, ptr)",
                        "jrg_str_replace");
                    let op = self.alloca_raw("msrp.", "ptr", Some(8));
                    let ol = self.alloca_raw("msrl.", "i64", Some(8));
                    self.emit(format!(
                        "call void @jrg_str_replace(ptr {}, i64 {}, ptr {}, i64 {}, ptr {}, i64 {}, ptr {}, ptr {})",
                        sp, sl, ap, al, bp, bl, op, ol));
                    self.join_str(&op, &ol)
                }
                _ => Err(super::galat::tandai(span, format!(
                    "method string `{}` belum didukung backend LLVM v1", method))),
            }
        })())
    }

    pub(crate) fn try_str_method(&mut self, optr: &str, method: &str, _span: Span) -> Option<Result<Val, String>> {
        let mapped = match method {
            "upper" | "to_uppercase" => "jrg_str_to_upper",
            "lower" | "to_lowercase" => "jrg_str_to_lower",
            "trim" => "jrg_str_trim",
            "reverse" => "jrg_str_reverse",
            "len" => {
                self.ensure_extern("declare i64 @jrg_str_char_count(ptr, i64)", "jrg_str_char_count");
                let sv = self.load_place(optr, &JrgType::Str);
                let (p, l) = self.split_str(&sv);
                let r = self.fresh("mslen.");
                self.emit(format!("{} = call i64 @jrg_str_char_count(ptr {}, i64 {})", r, p, l));
                return Some(Ok(Val { text: r, ty: JrgType::I64 }));
            }
            _ => return None,
        };
        let sv = self.load_place(optr, &JrgType::Str);
        Some(self.builtin_str1_val(&sv, mapped))
    }

}
