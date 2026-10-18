// src/interpreter/pola.rs — pengikatan dan pencocokan pola

use std::collections::HashMap;

use crate::parser::{
    AssignOp, BinaryOp, Block, BreakStmt, ContinueStmt, DeferStmt, EnumDecl, EnumVariantKind,
    Expr, ExternBlock, ExternItem, ExternParam, FnBody,
    ForStmt, IfStmt, ImplItem, Item, LetStmt, LitKind, LoopStmt,
    MatchArm, MatchBody, MatchStmt, NamedType, OwnershipQual, ParsedFile, Pattern, PrimTy,
    ReturnStmt, SelectCase, SelectStmt, ShortDeclStmt, SpawnStmt, Stmt, TypeExpr,
    UnaryOp, UsePath, VarStmt, WhileStmt,
    StructDecl, StructField, TraitDecl, TraitItem, Visibility,
};

use crate::interpreter::{
    Value, Env, Signal, RuntimeError, EvalResult, Interpreter, JrgFunction, DeferredCall,
    ok_value, err_value, some_value, none_value,
    ChannelHandle, channel_send, channel_recv,
    channel_try_send, channel_try_recv,
};
use crate::runtime_err;


use crate::interpreter::eval_stmt::*;

impl Interpreter {
    pub(crate) fn bind_pattern(&mut self, pat: &Pattern, val: Value, env: &mut Env) -> EvalResult {
        match pat {
            // `n @ pola` — `n` dapat nilai UTUHNYA, lalu polanya mengikat
            // apa pun yang ia ikat. Keduanya, bukan salah satu: itu
            // gunanya bentuk ini.
            Pattern::Binding { name, inner, .. } => {
                env.define(name, val.clone());
                self.bind_pattern(inner, val, env)
            }
            Pattern::Ident { name, .. } => {
                // Sama seperti di `pat_matches`: nama varian unit BUKAN
                // pengikat. Mengikatnya akan membuat `None` jadi variabel
                // yang membayangi variannya sendiri di badan arm.
                if self.varian_unit(name).is_some() {
                    return Ok(Value::Unit);
                }
                if name != "_" { env.define(name, val); }
                Ok(Value::Unit)
            }
            Pattern::Wildcard(_) => Ok(Value::Unit),

            Pattern::Tuple { elems, .. } => {
                if let Value::Tuple(vs) = val {
                    if vs.len() != elems.len() {
                        return runtime_err!(
                            "tuple destructure: butuh {} elemen, dapat {}",
                            elems.len(), vs.len()
                        );
                    }
                    for (p, v) in elems.iter().zip(vs) {
                        self.bind_pattern(p, v, env)?;
                    }
                    Ok(Value::Unit)
                } else {
                    runtime_err!("butuh tuple untuk destructure, dapat: {}", val)
                }
            }

            Pattern::Array { before, rest, after, .. } => {
                let Value::Array(vs) = val else {
                    return runtime_err!(
                        "destructure array: butuh array, dapat `{}` (tipe {})",
                        val, val.type_name()
                    );
                };
                let minimal = before.len() + after.len();
                match rest {
                    None => {
                        if vs.len() != before.len() {
                            return runtime_err!(
                                "destructure array: pola ini minta persis {} elemen, tapi arraynya {} elemen",
                                before.len(), vs.len()
                            );
                        }
                        for (p, v) in before.iter().zip(vs) {
                            self.bind_pattern(p, v, env)?;
                        }
                    }
                    Some(nama) => {
                        if vs.len() < minimal {
                            return runtime_err!(
                                "destructure array: pola ini butuh minimal {} elemen, tapi arraynya cuma {} elemen",
                                minimal, vs.len()
                            );
                        }
                        let ekor_mulai = vs.len() - after.len();
                        for (p, v) in before.iter().zip(vs[..before.len()].iter()) {
                            self.bind_pattern(p, v.clone(), env)?;
                        }
                        if let Some(n) = nama {
                            env.define(n, Value::Array(vs[before.len()..ekor_mulai].to_vec().into()));
                        }
                        for (p, v) in after.iter().zip(vs[ekor_mulai..].iter()) {
                            self.bind_pattern(p, v.clone(), env)?;
                        }
                    }
                }
                Ok(Value::Unit)
            }

            // `Pattern::Struct` covers two shapes of value: a plain struct
            // (`Value::Struct`), and an enum *struct-variant* (`Value::Enum` whose …
            Pattern::Struct { ty, fields, .. } => {
                let want_name = match ty {
                    TypeExpr::Named(n) => n.name.clone(),
                    _ => String::new(),
                };
                let target = match val {
                    Value::Enum { variant, payload, .. } => {
                        if variant != want_name {
                            return runtime_err!(
                                "varian enum tidak cocok saat destructure: harusnya `{}`, dapat `{}`",
                                want_name, variant
                            );
                        }
                        match payload {
                            Some(b) => *b,
                            None => Value::Unit,
                        }
                    }
                    other => other,
                };
                if let Value::Struct { fields: sf, .. } = target {
                    for fp in fields {
                        let fv = match sf.get(&fp.name) {
                            Some(v) => v.clone(),
                            None => return runtime_err!(
                                "struct tidak punya field `{}` untuk destructure", fp.name
                            ),
                        };
                        self.bind_pattern(&fp.pat, fv, env)?;
                    }
                    Ok(Value::Unit)
                } else {
                    runtime_err!("butuh struct untuk destructure, dapat: {}", target)
                }
            }

            Pattern::Enum { variant, inner, .. } => {
                if let Value::Enum { variant: vn, payload, .. } = val {
                    if &vn != variant {
                        return runtime_err!("varian enum tidak cocok: harusnya `{}`, dapat `{}`", variant, vn);
                    }
                    if let Some(ip) = inner {
                        let pv = payload.map(|b| *b).unwrap_or(Value::Unit);
                        self.bind_pattern(ip, pv, env)?;
                    }
                    Ok(Value::Unit)
                } else {
                    runtime_err!("butuh enum untuk destructure, dapat: {}", val)
                }
            }

            Pattern::Literal { .. } => Ok(Value::Unit), // literal pats don't bind
            Pattern::Range { .. }   => Ok(Value::Unit), // range pats don't bind vars

            Pattern::Or { pats, .. } => {
                for p in pats {
                    if self.pat_matches(p, &val) {
                        return self.bind_pattern(p, val, env);
                    }
                }
                runtime_err!("tidak ada or-pattern yang cocok untuk: {}", val)
            }

            Pattern::Ref { inner, .. } => self.bind_pattern(inner, val, env),
        }
    }

    /// Kalau `name` sebenarnya nama VARIAN ENUM TANPA PAYLOAD (`None`, atau
    /// varian unit bikinan pemakai yang disebut telanjang), kembalikan (nama …
    pub(crate) fn varian_unit(&self, name: &str) -> Option<(String, String)> {
        match self.env.get(name) {
            Some(Value::Enum { type_name, variant, payload: None }) => Some((type_name, variant)),
            _ => None,
        }
    }

    /// Nama telanjang yang sebenarnya varian milik modul. Sebagai NILAI ia
    /// sudah ditolak; di posisi POLA ia lebih berbahaya, karena bukannya gagal
    /// ia MENGIKAT dan cocok dengan apa pun.
    pub(crate) fn varian_modul_telanjang(&self, name: &str) -> Option<(String, String)> {
        let akhir = format!("::{}", name);
        for (kunci, v) in &self.varian_modul {
            if !kunci.ends_with(&akhir) { continue; }
            if let Value::Enum { type_name, variant, payload: None } = v {
                return Some((type_name.clone(), variant.clone()));
            }
        }
        None
    }

    /// Nama varian cocok tapi jumlah sub-polanya tidak: itu salah ketik, bukan
    /// "tidak cocok". Dua mesin acuan dulu melewatinya diam-diam; backend LLVM
    /// sudah menolaknya sejak dulu, dan kalimatnya kini satu definisi.
    pub(crate) fn cek_pola(&self, pat: &Pattern, val: &Value) -> Result<(), String> {
        if let Pattern::Ident { name, .. } = pat {
            // Aturannya satu dengan posisi nilai: kalau tidak telanjang di
            // sana, tidak telanjang di sini juga.
            if self.varian_unit(name).is_none() {
                if let Some((tipe, _)) = self.varian_modul_telanjang(name) {
                    return Err(match self.non_value_names.get(name) {
                        Some(dasar) => format!("{} (di posisi pola nama telanjang malah MENGIKAT \
                                                dan cocok dengan apa pun, jadi lengan ini akan \
                                                menelan semuanya)", dasar),
                        None => crate::interpreter::pesan_varian_modul_pola(name, &tipe, "yang diimpor"),
                    });
                }
            }
            return Ok(());
        }
        // Tiap bagian pola majemuk ikut ditelusuri; pemeriksaan nama
        // telanjang tidak butuh nilainya sama sekali.
        match pat {
            Pattern::Binding { inner, .. } => return self.cek_pola(inner, val),
            Pattern::Tuple { elems, .. } => {
                let kosong = Value::Nil;
                for (k, e) in elems.iter().enumerate() {
                    let v = match val { Value::Tuple(t) => t.get(k).unwrap_or(&kosong), _ => &kosong };
                    self.cek_pola(e, v)?;
                }
                return Ok(());
            }
            Pattern::Array { before, after, .. } => {
                let kosong = Value::Nil;
                for e in before.iter().chain(after.iter()) { self.cek_pola(e, &kosong)?; }
                return Ok(());
            }
            Pattern::Struct { ty, fields, .. } => {
                // Field yang tidak ada dulu cuma membuat lengannya dilewati:
                // salah ketik satu huruf jadi cabang yang tak pernah jalan.
                let isi = match val {
                    Value::Struct { name, fields: f } => Some((name.clone(), f)),
                    Value::Enum { type_name, variant, payload: Some(p) } => match &**p {
                        Value::Struct { fields: f, .. } if self.tipe_pola_cocok(ty, type_name) =>
                            Some((variant.clone(), f)),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some((nama, punya)) = isi {
                    let cocok_tipe = match ty {
                        crate::parser::TypeExpr::Named(nt) => nt.name == nama,
                        _ => true,
                    };
                    if cocok_tipe {
                        let mut ada: Vec<String> = punya.keys().cloned().collect();
                        ada.sort();
                        for f in fields {
                            if punya.contains_key(&f.name) { continue; }
                            let saran = crate::diag::saran_nama_terdekat(
                                &f.name, ada.iter().map(|x| x.as_str()));
                            return Err(crate::interpreter::galat_index::field_pola_tak_ada(
                                &nama, &f.name, &ada, saran.as_deref()));
                        }
                    }
                }
                let kosong = Value::Nil;
                for f in fields {
                    let v = match val {
                        Value::Struct { fields: sf, .. } => sf.get(&f.name).unwrap_or(&kosong),
                        _ => &kosong,
                    };
                    self.cek_pola(&f.pat, v)?;
                }
                return Ok(());
            }
            _ => {}
        }
        let Pattern::Enum { ty, variant, inner, .. } = pat else { return Ok(()) };
        let Value::Enum { type_name, variant: vn, payload } = val else { return Ok(()) };
        if vn != variant || !self.tipe_pola_cocok(ty, type_name) { return Ok(()); }
        let dapat = match inner {
            None => 0,
            Some(p) => match &**p {
                Pattern::Tuple { elems, .. } => elems.len(),
                _ => 1,
            },
        };
        let butuh = match payload {
            None => 0,
            Some(v) => match &**v {
                Value::Tuple(t) => t.len(),
                _ => 1,
            },
        };
        // Satu sub-pola untuk payload majemuk MENGIKAT tuple-nya utuh; itu
        // bentuk yang sudah berlaku di dua mesin dan tidak diubah di sini.
        if dapat != butuh && !(dapat == 1 && butuh > 1) {
            return Err(crate::interpreter::pesan_subpola_varian(
                &format!("{}::{}", type_name, variant), butuh, dapat));
        }
        // Lapis dalamnya ikut: VM memeriksa semua lapis karena mengompilasi
        // tiap sub-pola, jadi berhenti di lapis teratas jadi divergensi baru.
        match (inner, payload) {
            (Some(ip), Some(pv)) => self.cek_pola(ip, pv),
            _ => Ok(()),
        }
    }

    /// Pola berkualifikasi (`E::V`) cuma cocok dengan nilai bertipe `E`. Awalan yang
    /// ternyata alias modul (REPL, yang tidak ditulis ulang `ruang_tipe`) dibiarkan.
    fn tipe_pola_cocok(&self, ty: &TypeExpr, tipe: &str) -> bool {
        match ty {
            TypeExpr::Named(nt) => match &nt.package {
                Some(p) => p == tipe || p == "Self" || self.alias_dikenal.contains(p),
                None => true,
            },
            _ => true,
        }
    }

    pub(crate) fn pat_matches(&self, pat: &Pattern, val: &Value) -> bool {
        match pat {
            // Pengikatnya tidak pernah menolak; yang menentukan cocok atau
            // tidak adalah pola di sebelah kanan `@`.
            Pattern::Binding { inner, .. } => self.pat_matches(inner, val),
            Pattern::Wildcard(_)         => true,
            // Identifier telanjang biasanya MENGIKAT dan tidak pernah menolak — kecuali
            // kalau namanya ternyata varian enum tanpa payload.
            Pattern::Ident { name, .. } => match self.varian_unit(name) {
                Some((ty, var)) => matches!(
                    val,
                    Value::Enum { type_name, variant, payload: None }
                        if *type_name == ty && *variant == var
                ),
                None => true,
            },

            Pattern::Literal { lit, .. } => match (lit, val) {
                (LitKind::Int(a),   Value::Int(b))   => *a as i128 == *b,
                (LitKind::Float(a), Value::Float(b)) => a == b,
                (LitKind::Bool(a),  Value::Bool(b))  => a == b,
                (LitKind::Str(a),   Value::Str(b))   => a == b,
                (LitKind::Nil,      Value::Nil)       => true,
                (LitKind::Rune(a),  Value::Rune(b))  => a == b,
                _                                    => false,
            },

            Pattern::Enum { ty, variant, inner, .. } => {
                if let Value::Enum { type_name, variant: vn, payload } = val {
                    if vn != variant || !self.tipe_pola_cocok(ty, type_name) { return false; }
                    if let Some(ip) = inner {
                        if let Some(pv) = payload { return self.pat_matches(ip, pv); }
                        return false;
                    }
                    true
                } else { false }
            }

            Pattern::Tuple { elems, .. } => {
                if let Value::Tuple(vs) = val {
                    elems.len() == vs.len()
                        && elems.iter().zip(vs.iter()).all(|(p, v)| self.pat_matches(p, v))
                } else { false }
            }

            Pattern::Array { before, rest, after, .. } => {
                let Value::Array(vs) = val else { return false };
                match rest {
                    // Tanpa `...`: panjangnya harus persis.
                    None => {
                        before.len() == vs.len()
                            && before.iter().zip(vs.iter()).all(|(p, v)| self.pat_matches(p, v))
                    }
                    // Dengan `...`: minimal sepanjang bagian tetapnya.
                    Some(_) => {
                        if vs.len() < before.len() + after.len() { return false; }
                        let ekor_mulai = vs.len() - after.len();
                        before.iter().zip(vs.iter()).all(|(p, v)| self.pat_matches(p, v))
                            && after.iter().zip(vs[ekor_mulai..].iter()).all(|(p, v)| self.pat_matches(p, v))
                    }
                }
            }

            Pattern::Or { pats, .. }  => pats.iter().any(|p| self.pat_matches(p, val)),

            // Both numeric/rune ranges (`0..10`, `'a'..='z'`) and floating-point ranges
            // (`0.0..1.0`) are supported; either bound may be omitted for an open-ended …
            Pattern::Range { lo, hi, inclusive, .. } => match val {
                // Rune cuma lawan batas rune, angka cuma lawan batas angka: `'a' < 100`
                // ditolak bahasa ini, jadi rentang rune tidak boleh cocok dengan `100`.
                _ if pola_rentang_rune(lo, hi) != matches!(val, Value::Rune(_)) => false,
                Value::Int(n) => {
                    let n = *n;
                    let lo_ok = lo.as_deref().map_or(true, |p| banding_batas(n, p).map_or(false, |o| o.is_ge()));
                    let hi_ok = hi.as_deref().map_or(true, |p| banding_batas(n, p).map_or(false, |o| if *inclusive { o.is_le() } else { o.is_lt() }));
                    lo_ok && hi_ok
                }
                Value::Rune(c) => {
                    let n = *c as i128;
                    let lo_ok = lo.as_deref().map_or(true, |p| pattern_bound_i128(p).map_or(false, |a| n >= a));
                    let hi_ok = hi.as_deref().map_or(true, |p| pattern_bound_i128(p).map_or(false, |a| if *inclusive { n <= a } else { n < a }));
                    lo_ok && hi_ok
                }
                Value::Float(f) => {
                    let f = *f;
                    let lo_ok = lo.as_deref().map_or(true, |p| pattern_bound_f64(p).map_or(false, |a| f >= a));
                    let hi_ok = hi.as_deref().map_or(true, |p| pattern_bound_f64(p).map_or(false, |a| if *inclusive { f <= a } else { f < a }));
                    lo_ok && hi_ok
                }
                _ => false,
            },

            // `Pattern::Struct` covers both a plain struct pattern and an enum
            // struct-variant pattern — see `bind_pattern`'s comment on the same arm for …
            Pattern::Struct { ty, fields, .. } => {
                let want_name = match ty { TypeExpr::Named(n) => n.name.as_str(), _ => return false };
                let target: &Value = match val {
                    Value::Enum { type_name, variant, payload } => {
                        if variant != want_name || !self.tipe_pola_cocok(ty, type_name) { return false; }
                        match payload.as_deref() {
                            Some(inner) => inner,
                            None => return false,
                        }
                    }
                    other => other,
                };
                match target {
                    Value::Struct { name, fields: sf } => {
                        name == want_name && fields.iter().all(|fp| {
                            sf.get(&fp.name).map_or(false, |fv| self.pat_matches(&fp.pat, fv))
                        })
                    }
                    _ => false,
                }
            }

            // A reference pattern matches based on its inner pattern — this used to
            // unconditionally return `true`, ignoring the inner pattern entirely.
            Pattern::Ref { inner, .. } => self.pat_matches(inner, val),
        }
    }
}
