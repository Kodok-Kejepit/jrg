// src/interpreter/eval_expr.rs — interpreter/eval_expr.rs
// Evaluasi ekspresi: `eval_expr` (dispatcher utama), literal/unary/binary

use std::collections::HashMap;

use crate::parser::{
    AssignOp, BinaryOp, Block, BreakStmt, ContinueStmt, EnumDecl, EnumVariantKind,
    Expr, ExternBlock, ExternItem, ExternParam, FnBody,
    ForStmt, IfStmt, ImplItem, Item, LetStmt, LitKind, LoopStmt,
    MatchArm, MatchBody, MatchStmt, NamedType, OwnershipQual, ParsedFile, Pattern, PrimTy,
    ReturnStmt, SelectCase, SelectStmt, ShortDeclStmt, SpawnStmt, Stmt, TemplatePart, TypeExpr,
    UnaryOp, UsePath, VarStmt, WhileStmt,
    StructDecl, StructField, TraitDecl, TraitItem, Visibility,
};

use crate::interpreter::{
    Value, Env, Signal, RuntimeError, EvalResult, Interpreter, JrgFunction,
    ok_value, err_value, some_value, none_value,
    ChannelHandle, channel_new, channel_send, channel_recv,
    channel_try_send, channel_try_recv,
    enum_payload, enum_parts,
};
use crate::interpreter::builtin::{NativeFnKind, fisher_yates_shuffle};
use crate::runtime_err;

impl Interpreter {
    /// Evaluasi ekspresi, sekaligus menempelkan lokasi ke error yang lewat.
    pub fn eval_expr(&mut self, expr: &Expr, env: &mut Env) -> EvalResult {
        match self.eval_expr_inner(expr, env) {
            // `isi_span_bingkai` dipasangkan di sini, bukan di `call_jrg`: yang tahu
            // SITUS PANGGIL sebuah fungsi adalah ekspresi pemanggilnya, dan pembungkus …
            Err(Signal::RuntimeError(e)) => Err(Signal::RuntimeError(
                e.fill_span(expr.span()).isi_span_bingkai(expr.span())
            )),
            other => other,
        }
    }

    /// Literal struct `T { a: 1, ...basis }`. Di luar `eval_expr_inner` supaya bingkai fungsi
    /// rekursif itu tidak ikut membesar oleh lokal lengan ini.
    #[inline(never)]
    /// Field pertama (akar dulu, sama dengan `resolved_struct_fields` di VM) yang tidak ada
    /// di `fm`; `None` juga kalau `tname` bukan struct milik program. Tanpa alokasi.
    fn field_wajib_hilang(&self, tname: &str, fm: &HashMap<String, Value>, dalam: usize) -> Option<&str> {
        let d = self.struct_defs.get(tname)?;
        if dalam < 64 {
            if let Some(f) = d.parent.as_deref().and_then(|p| self.field_wajib_hilang(p, fm, dalam + 1)) {
                return Some(f);
            }
        }
        d.fields.iter().find(|f| !fm.contains_key(f.name.as_str())).map(|f| f.name.as_str())
    }

    fn eval_literal_struct(&mut self, ty: &TypeExpr, fields: &[crate::parser::FieldInit],
                           spread: &Option<Box<Expr>>, env: &mut Env) -> EvalResult {
        let tname = match ty {
            // `Self { .. }` di dalam method berarti tipe pemilik method-nya.
            TypeExpr::Named(n) if n.name == "Self" && self.ruang_kini.is_some() =>
                self.ruang_kini.as_deref().cloned().unwrap_or_default(),
            TypeExpr::Named(n) => n.name.clone(),
            _ => "anonymous".to_string(),
        };
        // Nama yang tidak dideklarasikan dulu diterima diam-diam: `Titk { x: 1 }` (salah
        // ketik `Titik`) jalan, dan `Ptr { id: 1 }` memalsukan handle bawaan.
        if matches!(ty, TypeExpr::Named(_))
            && tname != crate::interpreter::builtin::dict::DICT_TYPE_NAME
            && !crate::interpreter::galat_index::STRUCT_DATA_PUSTAKA.contains(&tname.as_str())
            && !self.struct_defs.contains_key(&tname)
            && !self.varian_struct_field.contains_key(&tname)
            && !self.struct_variant_owner.contains_key(&tname)
        {
            let saran = crate::diag::saran_nama_terdekat(
                &tname, self.struct_defs.keys().map(|x| x.as_str()));
            return runtime_err!("{}", crate::interpreter::galat_index::struct_tak_dideklarasikan(
                &tname, saran.as_deref()));
        }
        // Varian ber-field menolak field yang tidak dideklarasikannya, sebelum apa pun
        // dievaluasi — dulu `E::V { a: 1, zz: 2 }` diterima diam-diam (VM menolak).
        // `E::V { .. }` dicari lewat nama lengkapnya; yang telanjang lewat nama variannya.
        let kunci_varian = match ty {
            TypeExpr::Named(n) => n.package.as_ref().map(|p| format!("{}::{}", p, n.name)),
            _ => None,
        }.filter(|k| self.struct_variant_owner.contains_key(k)).unwrap_or_else(|| tname.clone());
        if let Some(milik) = self.varian_struct_field.get(&kunci_varian) {
            if let Some(fi) = fields.iter().find(|fi| !milik.contains(&fi.name)) {
                return runtime_err!("{}", crate::interpreter::pesan_field_varian(
                    &fi.name, &crate::vm::compiler::qualified_type_expr_name(ty)));
            }
        }
        let mut fm: HashMap<String, Value> = HashMap::new();

        if let Some(sp) = spread {
            let sv = self.eval_expr(sp, env)?;
            let sf = match sv {
                Value::Struct { fields: sf, .. } => sf,
                Value::Enum { payload: Some(p), .. } if matches!(*p, Value::Struct { .. }) => {
                    let Value::Struct { fields: sf, .. } = *p else { unreachable!() };
                    sf
                }
                // Dulu diabaikan diam-diam: `P { a: 1, ...5 }` jadi no-op.
                lain => return runtime_err!("{}",
                    crate::interpreter::pesan_sebar_bukan_wadah(&lain.type_name())),
            };
            // Basis tidak boleh menyelundupkan field di luar deklarasi sasarannya.
            let sasaran = match (self.varian_struct_field.get(&kunci_varian), self.struct_variant_owner.get(&kunci_varian)) {
                (Some(milik), Some(pemilik)) => Some((milik.iter().cloned().collect::<Vec<String>>(),
                    format!("varian `{}::{}`", crate::ruang_tipe::bersih(pemilik), tname))),
                _ => self.field_dideklarasikan(&tname)
                    .map(|d| (d, format!("struct `{}`", crate::ruang_tipe::bersih(&tname)))),
            };
            if let Some((boleh, sebutan)) = sasaran {
                if let Some(f) = crate::interpreter::galat_index::field_sebar_liar(sf.keys(), &boleh) {
                    return runtime_err!("{}", crate::interpreter::galat_index::sebar_field_liar(&sebutan, f));
                }
            }
            fm.extend(sf);
        }
        for fi in fields {
            // Field yang tidak dideklarasikan ditolak, aturan yang sama dengan jalur
            // tulis.
            if let Some(None) = self.deklarasi_field(&tname, &fi.name) {
                if let Some(dideklarasikan) = self.field_dideklarasikan(&tname) {
                    let saran = crate::diag::saran_nama_terdekat(
                        &fi.name, dideklarasikan.iter().map(|x| x.as_str()));
                    return runtime_err!("{}",
                        crate::interpreter::galat_index::field_tak_dideklarasikan(
                            &tname, &fi.name, saran.as_deref()));
                }
            }
            fm.insert(fi.name.clone(), self.eval_expr(&fi.value, env)?);
        }
        // Default cuma untuk field yang TIDAK diisi literal maupun basisnya, sesudah keduanya,
        // akar dulu; deklarasi terdekat yang menentukan. Dulu tiap default dievaluasi lebih
        // dulu lalu ditimpa — efek sampingnya jalan di sini dan tidak di dua mesin lain.
        let silsilah = self.lineage_of(&tname);
        let mut isi_default: Vec<(String, Expr)> = Vec::new();
        for (i, t) in silsilah.iter().enumerate().rev() {
            let Some(info) = self.struct_defs.get(t) else { continue };
            for f in &info.fields {
                if fm.contains_key(&f.name) { continue; }
                // Tempatnya di deklarasi PERTAMA (akar), isinya dari deklarasi TERDEKAT —
                // urutan yang sama dengan `resolved_struct_fields` dan `field_rata`.
                let punya = |a: &String| self.struct_defs.get(a)
                    .and_then(|d| d.fields.iter().find(|g| g.name == f.name));
                if silsilah[i + 1..].iter().any(|a| punya(a).is_some()) { continue; }
                let terdekat = silsilah[..i].iter().find_map(punya).unwrap_or(f);
                if let Some(e) = &terdekat.default { isi_default.push((f.name.clone(), (**e).clone())); }
            }
        }
        for (nama, e) in isi_default {
            let v = self.eval_expr(&e, env)?;
            fm.insert(nama, v);
        }
        // Field wajib (tanpa default) yang tidak diisi: dulu nilainya lahir tanpa field itu.
        if let Some(f) = self.field_wajib_hilang(&tname, &fm, 0) {
            return runtime_err!("{}", crate::interpreter::galat_index::field_wajib(&tname, f));
        }
        if let (Some(milik), Some(pemilik)) = (self.varian_struct_field.get(&kunci_varian),
                                               self.struct_variant_owner.get(&kunci_varian)) {
            if let Some(f) = milik.iter().find(|f| !fm.contains_key(f.as_str())) {
                return runtime_err!("{}", crate::interpreter::galat_index::field_wajib_varian(
                    &format!("{}::{}", crate::ruang_tipe::bersih(pemilik), tname), f));
            }
        }
        let plain = Value::Struct { name: tname.clone(), fields: fm.into() };
        // If `tname` names a struct-variant of some enum (registered by
        // `register_enum` — see `Item::Enum` in `hoist_top_level`), wrap the plain …
        match self.struct_variant_owner.get(&kunci_varian).cloned() {
            Some(owner) => Ok(Value::Enum {
                type_name: owner,
                variant: tname,
                payload: Some(Box::new(plain)),
            }),
            None => Ok(plain),
        }
    }

    pub(crate) fn eval_expr_inner(&mut self, expr: &Expr, env: &mut Env) -> EvalResult {
        match expr {
            Expr::Literal(lit, _) => Ok(self.eval_lit(lit)),

            Expr::Ident(name, _) => match env.get(name) {
                Some(v) => Ok(v),
                None if self.belum_diisi.contains(name) => {
                    runtime_err!("{}", crate::interpreter::pesan_belum_diisi(name))
                }
                // Nama modul yang sudah di-import dulu di-bind ke `Value::Nil` supaya
                // menyebutnya tidak crash.
                None if self.non_value_names.contains_key(name) => {
                    runtime_err!("{}", self.non_value_names[name])
                }
                None if self.contoh_modul.contains_key(name) => {
                    let contoh = self.contoh_modul.get(name).map(|s| s.as_str()).unwrap_or("nama_fungsi");
                    runtime_err!("{}", crate::interpreter::pesan_nama_modul(name, contoh))
                }
                None => {
                    let sug = env.suggest_similar(name);
                    runtime_err!("{}", crate::interpreter::pesan_nama_tak_dikenal(
                        name, sug.as_deref()))
                }
            },

            Expr::PathExpr { segments, .. } => {
                // `::nama` — bawaan bahasa, tanpa peduli deklarasi program. Penurunan parser
                // (sebar, argumen bernama, literal dict) lewat sini, jadi `fn terap` milik
                // pemakai atau modul bernama `dict` tidak bisa membajaknya.
                let murni = segments.first().is_some_and(|s| s.is_empty());
                let segments: &[String] = if murni { &segments[1..] } else { &segments[..] };
                let path = segments.join("::");
                let ketemu = if murni { Env::bawaan_murni(&path) } else { env.get(&path) };
                // Check env first (covers qualified builtins + static method references)
                if let Some(v) = ketemu {
                    // Auto-call zero-arg native constants (math::PI, math::E, dst
                    // — biar bisa dipakai tanpa `()` seperti konstanta biasa)
                    if let Value::NativeFn(ref k) = v {
                        if matches!(k, NativeFnKind::MathPi | NativeFnKind::MathE
                            | NativeFnKind::MathTau | NativeFnKind::MathInf | NativeFnKind::MathNan) {
                            return self.call_native(k.clone(), vec![]);
                        }
                    }
                    return Ok(v);
                }
                if murni {
                    if let Some(sebab) = Self::jelaskan_namespace_terblokir(&path) {
                        return runtime_err!("{}", sebab);
                    }
                    if let Some(nama) = crate::interpreter::builtin::jembatan_stdlib::cari(&path) {
                        return Ok(Value::NativeFn(NativeFnKind::DariStdlibVm(nama)));
                    }
                    let daftar = crate::vm::daftar_builtin();
                    let sug = crate::diag::saran_nama_terdekat(
                        &path, daftar.iter().map(|s| s.as_str()));
                    return runtime_err!("{}", crate::interpreter::pesan_bukan_bawaan(&path, sug.as_deref()));
                }
                // `alias::Tipe::metode`: tipe milik modul bersifat global, jadi bentuk
                // ber-awalan modulnya merujuk method yang sama dengan `Tipe::metode`.
                if segments.len() == 3
                    && self.tipe_modul.contains(&format!("{}::{}", segments[0], segments[1]))
                {
                    if let Some(f) = self.methods.find_static(&segments[1], &segments[2]).cloned() {
                        return Ok(Value::Function(f.into()));
                    }
                    if let Some(mut f) = self.resolusi_metode(&segments[1], &segments[2]) {
                        std::sync::Arc::make_mut(&mut f).pemilik = Some(segments[1].clone());
                        return Ok(Value::Function(f.into()));
                    }
                }
                // Static method lookup in registry
                if segments.len() == 2 {
                    if let Some(f) = self.methods.find_static(&segments[0], &segments[1]).cloned() {
                        return Ok(Value::Function(f.into()));
                    }
                    // Method ber-`self` lewat bentuk path: `P::tambah(p, 4)`, dan — yang
                    // membuatnya berarti — `arr::map(xs, P::tambah)`.
                    if let Some(mut f) = self.resolusi_metode(&segments[0], &segments[1]) {
                        std::sync::Arc::make_mut(&mut f).pemilik = Some(segments[0].clone());
                        return Ok(Value::Function(f.into()));
                    }
                }
                // Nama yang dideklarasikan tapi bukan nilai — varian ber-field
                // (`E::V`), atau nama tipe lewat awalan modul (`bantu::P`).
                if let Some(alasan) = self.non_value_names.get(&path) {
                    return runtime_err!("{}", alasan);
                }
                // `var` dan nama ber-awalan `_` milik modul: dikenal, tapi tidak diekspor.
                if let Some(alasan) = self.alasan_jalur.get(&path) {
                    return runtime_err!("{}", alasan);
                }
                // Dulu di sini ada "last segment fallback": kalau `a::b` tidak ketemu, `b`
                // telanjang dicoba.
                if let Some(baru) = Self::nama_lama_memori(segments) {
                    return runtime_err!(
                        "`{}` sudah tidak ada — namanya sekarang `{}`. Semantiknya tidak berubah \
                         (alokasi eksplisit, deteksi use-after-free dan double-free), cuma namanya \
                         yang dibuat lebih JRG",
                        path, baru
                    );
                }
                // Namespace yang implementasinya ADA di stdlib VM tapi sengaja tidak
                // dijembatani ke sini.
                if let Some(sebab) = Self::jelaskan_namespace_terblokir(&path) {
                    return runtime_err!("{}", sebab);
                }
                // Stdlib yang ditulis untuk VM. Dicari di sini — pada jalur
                // GAGAL, yang memang jarang dilalui — supaya program yang
                // tidak memakainya tidak ikut membayar pemanenannya.
                if let Some(nama) = crate::interpreter::builtin::jembatan_stdlib::cari(&path) {
                    return Ok(Value::NativeFn(NativeFnKind::DariStdlibVm(nama)));
                }
                if let Some(alias) = self.nama_beralias.get(&segments[0]).filter(|_| segments.len() > 1) {
                    return runtime_err!("{}", crate::interpreter::pesan_modul_beralias(&path, &segments[0], alias));
                }
                // Modulnya dimuat, tapi oleh berkas LAIN: lingkungan ini tidak punya satu pun
                // `x::…`, jadi yang salah bukan ejaannya melainkan `import` yang belum ada.
                if segments.len() > 1 && self.alias_dikenal.contains(&segments[0])
                    && !env.ada_awalan(&format!("{}::", segments[0]))
                {
                    return runtime_err!("{}", crate::interpreter::pesan_modul_tak_diimpor(&path, &segments[0]));
                }
                let ekor = segments.last().map(|s| s.as_str()).unwrap_or("");
                let telanjang = segments.len() > 1 && env.get(ekor).is_some();
                // Calonnya dari `vm::daftar_builtin()`, BUKAN dari `env`: sebagian nama
                // (`str::len`, seluruh keluarga HOF) dicapai lewat jembatan nilai dan tidak …
                let daftar = crate::vm::daftar_builtin();
                let lain = crate::diag::nama_di_namespace_lain(
                    &path, daftar.iter().map(|s| s.as_str()));
                let sug = crate::diag::saran_nama_terdekat(
                    &path, daftar.iter().map(|s| s.as_str()))
                    .or_else(|| env.suggest_similar(&path));
                runtime_err!("{}", crate::interpreter::pesan_path_tak_dikenal_plus(
                    &path, sug.as_deref(), &lain, telanjang))
            }

            Expr::Unary { op, expr, .. } => {
                // `-170141183460469231731687303715884105728`: literalnya sendiri tidak muat
                // di 128 bit (parser cuma mengizinkannya di sini), jadi dibaca utuh sebagai MIN.
                if matches!(op, UnaryOp::Neg) {
                    if let Expr::Literal(LitKind::Int(n), _) = expr.as_ref() {
                        if *n == i128::MAX as u128 + 1 { return Ok(Value::Int(i128::MIN)); }
                    }
                }
                let v = self.eval_expr(expr, env)?;
                if let Some(h) = self.operator_unary_pengguna(op, &v)? {
                    return Ok(h);
                }
                self.eval_unary(op, v)
            }

            Expr::Binary { op, lhs, rhs, .. } => {
                // Short-circuit for && and ||
                match op {
                    BinaryOp::And => {
                        let lv = self.eval_expr(lhs, env)?;
                        if !self.truthy(&lv) { return Ok(Value::Bool(false)); }
                        let rv = self.eval_expr(rhs, env)?;
                        return Ok(Value::Bool(self.truthy(&rv)));
                    }
                    BinaryOp::Or => {
                        let lv = self.eval_expr(lhs, env)?;
                        if self.truthy(&lv) { return Ok(Value::Bool(true)); }
                        let rv = self.eval_expr(rhs, env)?;
                        return Ok(Value::Bool(self.truthy(&rv)));
                    }
                    _ => {}
                }
                let lv = self.eval_expr(lhs, env)?;
                let rv = self.eval_expr(rhs, env)?;
                // Operator yang didefinisikan pengguna didahulukan atas aturan bawaan, dan
                // diterapkan DI SINI karena `op_bin` bersifat `&self` sementara memanggil …
                if let Some(h) = self.operator_biner_pengguna(op, &lv, &rv)? {
                    return Ok(h);
                }
                let (lv, rv) = self.siapkan_gabung_string(op, lv, rv)?;
                self.op_bin(op.clone(), lv, rv)
            }

            Expr::Ternary { cond, then, else_, .. } => {
                let cv = self.eval_expr(cond, env)?;
                if self.truthy(&cv) { self.eval_expr(then, env) }
                else { self.eval_expr(else_, env) }
            }

            Expr::Pipe { lhs, rhs, .. } => {
                let lv = self.eval_expr(lhs, env)?;
                match rhs.as_ref() {
                    Expr::Call { func, args, .. } => {
                        let fv = self.eval_expr(func, env)?;
                        let mut av = vec![lv];
                        for a in args { av.push(self.eval_expr(a, env)?); }
                        self.call_fn(fv, av)
                    }
                    _ => { let fv = self.eval_expr(rhs, env)?; self.call_fn(fv, vec![lv]) }
                }
            }

            Expr::Cast { expr, ty, .. } => {
                let v = self.eval_expr(expr, env)?;
                self.eval_cast(v, ty)
            }

            Expr::Field { obj, field, .. } => {
                let ov = self.eval_expr(obj, env)?;
                self.check_field_access(&ov, field, env)?;
                self.eval_field(ov, field)
            }

            Expr::Method { obj, method, args, .. } => {
                let ov = self.eval_expr(obj, env)?;
                let mut av = Vec::with_capacity(args.len());
                for a in args { av.push(self.eval_expr(a, env)?); }
                self.eval_method(ov, method, av)
            }

            Expr::Index { obj, idx, .. } => {
                let ov = self.eval_expr(obj, env)?;
                let iv = self.eval_expr(idx, env)?;
                self.eval_index(ov, iv)
            }

            Expr::Slice { obj, lo, hi, .. } => {
                let ov = self.eval_expr(obj, env)?;
                let lv = lo.as_ref().map(|e| self.eval_expr(e, env)).transpose()?;
                let hv = hi.as_ref().map(|e| self.eval_expr(e, env)).transpose()?;
                self.eval_slice(ov, lv, hv)
            }

            Expr::Call { func, args, .. } => {
                let fv = self.eval_expr(func, env)?;
                let mut av = Vec::with_capacity(args.len());
                for a in args { av.push(self.eval_expr(a, env)?); }
                self.call_fn_dari(fv, av, Some(func))
            }

            Expr::Await { expr, .. } => self.eval_expr(expr, env),

            Expr::Propagate { expr, .. } => {
                let v = self.eval_expr(expr, env)?;
                match &v {
                    Value::Enum { variant, payload, .. } if variant == "Ok" || variant == "Some" => {
                        Ok(payload.as_deref().cloned().unwrap_or(Value::Unit))
                    }
                    // "Error" is accepted alongside "Err" for backward
                    // compatibility with any code already written
                    // against the older, narrower check this replaces.
                    Value::Enum { variant, .. } if variant == "Err" || variant == "None" || variant == "Error" => {
                        Err(Signal::Return(v.clone()))
                    }
                    other => runtime_err!(
                        "operator `?` hanya berlaku untuk Result/Option, dapat: {}", other
                    ),
                }
            }

            // Closure menangkap BINDING (sel bersama), bukan salinan nilai.
            Expr::Closure { params, body, is_move, .. } => {
                let pnames: Vec<String> = params.iter().map(|p| match &p.pattern {
                    Pattern::Ident { name, .. } => name.clone(),
                    _ => "_".to_string(),
                }).collect();
                let mut captures = Vec::new();
                let kunci = std::sync::Arc::as_ptr(body) as usize;
                let disebut = match self.nama_closure.get(&kunci) {
                    Some((_, n)) => std::sync::Arc::clone(n),
                    None => {
                        let mut n: Vec<String> =
                            crate::interpreter::capture::names_mentioned(body).into_iter().collect();
                        n.sort();
                        let n = std::sync::Arc::new(n);
                        self.nama_closure.insert(kunci, (std::sync::Arc::clone(body), std::sync::Arc::clone(&n)));
                        n
                    }
                };
                for name in disebut.iter().cloned() {
                    // Parameter closure ini sendiri tidak pernah ditangkap:
                    // nilainya baru ada saat dipanggil.
                    if pnames.contains(&name) { continue; }
                    // `move` menangkap SALINAN, closure biasa berbagi sel.
                    // Itu satu-satunya perbedaan keduanya, dan seluruh arti
                    // `move` turun dari situ.
                    let cell = if *is_move {
                        env.capture_salinan(&name)
                    } else {
                        env.capture_cell(&name)
                    };
                    if let Some(cell) = cell {
                        captures.push((name, cell));
                    }
                }
                Ok(Value::Closure {
                    params: pnames.into(),
                    body: std::sync::Arc::clone(body),
                    captures,
                    globals: env.globals.clone(),
                    ruang: self.ruang_kini.clone(),
                })
            }

            Expr::Block(b) => self.exec_block(b, env),

            Expr::Struct { ty, fields, spread, .. } => self.eval_literal_struct(ty, fields, spread, env),

            Expr::Array { elems, .. } => {
                let mut vs = Vec::with_capacity(elems.len());
                for e in elems { vs.push(self.eval_expr(e, env)?); }
                Ok(Value::Array(vs.into()))
            }

            Expr::ArrayRepeat { elem, count, .. } => {
                let ev = self.eval_expr(elem, env)?;
                let nv = self.eval_expr(count, env)?;
                let n  = crate::interpreter::jumlah_ulang(&crate::interpreter::arg_angka(&nv))
                    .map_err(|m| Signal::RuntimeError(RuntimeError::new(m)))?;
                Ok(Value::Array(vec![ev; n].into()))
            }

            // `()` yang DITULIS adalah unit — yang dijawab fungsi tanpa nilai dan yang
            // dicetak `()`. Dulu tuple kosong, jadi `f() == ()` menjawab `false`.
            Expr::Tuple { elems, .. } if elems.is_empty() => Ok(Value::Unit),
            Expr::Tuple { elems, .. } => {
                let mut vs = Vec::with_capacity(elems.len());
                for e in elems { vs.push(self.eval_expr(e, env)?); }
                Ok(Value::Tuple(vs.into()))
            }

            // Satu definisi dengan `range(...)` di dua mesin. Dulu salinan ketiga:
            // `hi + 1` meluap di `i128::MAX`, dan tanpa batas ukuran sama sekali.
            Expr::Range { lo, hi, inclusive, .. } => {
                let lo_v = match lo { Some(le) => self.eval_expr(le, env)?, None => Value::Int(0) };
                let hi_v = match hi {
                    Some(he) => self.eval_expr(he, env)?,
                    None => return runtime_err!("range tanpa batas atas tidak didukung"),
                };
                let nama = if *inclusive { "range_inclusive" } else { "range" };
                let g = |m: String| Signal::RuntimeError(RuntimeError::new(m));
                let (s, e) = crate::interpreter::range_batas(nama,
                    &crate::interpreter::arg_angka(&lo_v), &crate::interpreter::arg_angka(&hi_v)).map_err(g)?;
                let n = crate::interpreter::range_jumlah(nama, s, e, 1, *inclusive).map_err(g)?;
                Ok(Value::Array(crate::interpreter::range_isi(s, 1, n).map(Value::Int).collect::<Vec<_>>().into()))
            }

            // `make_chan(T)` / `make_chan(T, capacity)` — `cap` omitted means an
            // *unbuffered* channel (capacity 0): a send blocks until a receiver is ready …
            Expr::MakeChan { cap, .. } => {
                let capacity = match cap {
                    Some(e) => {
                        let cv = self.eval_expr(e, env)?;
                        self.to_usize(&cv)?
                    }
                    None => 0,
                };
                Ok(Value::Channel(channel_new(capacity)))
            }

            // `chan <- value` — a *blocking* send (used outside of `select`, where the
            // very same syntax instead goes through `exec_select`'s non-blocking …
            Expr::Send { chan, val, .. } => {
                let cv = self.eval_expr(chan, env)?;
                let vv = self.eval_expr(val, env)?;
                match cv {
                    Value::Channel(handle) => match channel_send(&handle, vv) {
                        Ok(())   => Ok(Value::Unit),
                        Err(msg) => runtime_err!("{}", msg),
                    },
                    other => runtime_err!("{}", crate::interpreter::pesan_bukan_channel("`c <- v` (kirim)", &other.type_name())),
                }
            }

            // `<-chan` — a *blocking* receive.
            Expr::Recv { chan, .. } => {
                let cv = self.eval_expr(chan, env)?;
                match cv {
                    Value::Channel(handle) => match channel_recv(&handle) {
                        Ok(v)    => Ok(v),
                        Err(msg) => runtime_err!("{}", msg),
                    },
                    other => runtime_err!("{}", crate::interpreter::pesan_bukan_channel("`<-c` (terima)", &other.type_name())),
                }
            }

            // Template literal `« ...
            Expr::Template { parts, .. } => {
                let mut out = String::new();
                for part in parts {
                    match part {
                        TemplatePart::Text(t) => out.push_str(t),
                        TemplatePart::Expr(e) => {
                            let v = self.eval_expr(e, env)?;
                            out.push_str(&self.tampil(&v)?);
                        }
                    }
                }
                Ok(Value::Str(out))
            }

            // `match scrutinee { ... }` used as an expression (see
            // `parse_prefix`'s `KwMatch` arm and the comment on
            // `eval_match_arms`, which this and `exec_match` both share).
            Expr::Match { scrutinee, arms, .. } => {
                let sv = self.eval_expr(scrutinee, env)?;
                self.eval_match_arms(sv, arms, env)
            }

            // `if` as an expression: the value is the taken branch's block
            // value. Same node the statement form uses, same evaluator —
            // statement position just discards the result.
            Expr::If(s) => self.exec_if(s, env),

            Expr::Error(span)                   => runtime_err!("AST error node, span: {:?}", span),
        }
    }


    // Function Call Dispatch
    // -----------------------------------------------------------------------


    pub fn truthy(&self, v: &Value) -> bool {
        match v {
            Value::Bool(b)  => *b,
            Value::Int(n)   => *n != 0,
            Value::Float(f) => crate::interpreter::float_truthy(*f),
            Value::Str(s)   => !s.is_empty(),
            Value::Nil      => false,
            Value::Unit     => false,
            Value::Array(a) => !a.is_empty(),
            // Array numerik kosong itu falsy, sama seperti array biasa kosong.
            Value::Nd(a) => a.size() != 0,
            Value::Tuple(t) => !t.is_empty(),
            // Dict kosong itu FALSY, sama seperti array kosong dan string kosong.
            Value::Struct { name, fields } if name == "dict" => !fields.is_empty(),
            _               => true,
        }
    }

    /// Convert Value to usize (for indexing/slicing).
    pub fn to_usize(&self, v: &Value) -> Result<usize, Signal> { nilai_ke_usize(v) }

    

    /// Convert Value to i128. Used for range bounds, math, etc.
    pub fn to_i128(&self, v: &Value) -> Result<i128, Signal> { nilai_ke_i128(v) }

    

    /// Convert Value to f64. Used for math functions.
    pub fn to_f64(&self, v: &Value) -> Result<f64, Signal> { nilai_ke_f64(v) }

    
}

/// Nama pendek sebuah sasaran cast, untuk pesan error `as`.
pub fn nama_tipe_sasaran(ty: &TypeExpr) -> String {
    // Satu definisi dengan parser: yang lama memakai `{:?}` dan menulis `string_`.
    crate::parser::nama_tipe(ty)
}


// ─── Konversi nilai → tipe Rust ──────────────────────────────────────────

pub fn nilai_ke_usize(v: &Value) -> Result<usize, Signal> {
    match v {
        Value::Int(n) => {
            if *n < 0 { return runtime_err!("index tidak boleh negatif: {}", n); }
            // MENJENUH, bukan memotong.
            Ok(usize::try_from(*n).unwrap_or(usize::MAX))
        }
        Value::Float(f) => runtime_err!(
            "index harus bilangan bulat, dapat pecahan `{}` — kalau memang mau dipotong \
             ke bawah, tulis eksplisit: `[{} as int]`",
            f, f
        ),
        _ => runtime_err!("index harus bilangan bulat, dapat `{}`", v.type_name()),
    }
}

/// Seperti [`nilai_ke_usize`], tapi kalimat tipe-salahnya menyebut FUNGSINYA —
/// kalimat yang sama dengan `bil()` di VM. Negatif tetap kalimat indeks acuan.
pub fn cacah_untuk(nama: &str, v: &Value) -> Result<usize, Signal> {
    match v {
        Value::Int(_) => nilai_ke_usize(v),
        lain => runtime_err!("{}", bukan_bulat(nama, lain)),
    }
}

/// Index argumen builtin (`remove`, `insert`, `swap`, ...) dengan aturan `a[i]`; di luar batas gagal.
pub fn indeks_untuk(nama: &str, v: &Value, panjang: usize, sisip: bool) -> Result<usize, Signal> {
    let i = match v { Value::Int(n) => *n, lain => return runtime_err!("{}", bukan_bulat(nama, lain)) };
    match crate::interpreter::value::normal_indeks(i, panjang, sisip) {
        Some(j) => Ok(j),
        None => runtime_err!("{}", crate::interpreter::pesan_index_luar_batas(nama, i, panjang)),
    }
}

/// Index karakter `str::char_at`: aturan `a[i]` atas karakter (bukan byte).
pub fn indeks_karakter(v: &Value, s: &str) -> Result<usize, Signal> {
    let Value::Int(i) = v else { return nilai_ke_usize(v) };
    let n = s.chars().count();
    match crate::interpreter::value::normal_indeks(*i, n, false) {
        Some(j) => Ok(j),
        None => runtime_err!("{}", crate::interpreter::pesan_index_luar_batas("str::char_at", *i, n)),
    }
}

/// Seperti [`indeks_untuk`], tapi di luar batas berarti `None` (`get`, `get_or`).
pub fn indeks_opsional(nama: &str, v: &Value, panjang: usize) -> Result<Option<usize>, Signal> {
    match v {
        Value::Int(n) => Ok(crate::interpreter::value::normal_indeks(*n, panjang, false)),
        lain => runtime_err!("{}", bukan_bulat(nama, lain)),
    }
}

/// Seperti [`nilai_ke_i128`], dengan nama fungsinya di kalimat galat.
pub fn bulat_untuk(nama: &str, v: &Value) -> Result<i128, Signal> {
    match v {
        Value::Int(n) => Ok(*n),
        lain => runtime_err!("{}", bukan_bulat(nama, lain)),
    }
}

fn bukan_bulat(nama: &str, v: &Value) -> String {
    let pecahan = matches!(v, Value::Float(_)).then(|| format!("{}", v));
    crate::interpreter::pesan_bukan_bulat(nama, &v.type_name(), pecahan.as_deref())
}

pub fn nilai_ke_i128(v: &Value) -> Result<i128, Signal> {
    match v {
        Value::Int(n)   => Ok(*n),
        // Pecahan DITOLAK, bukan dipotong — aturan yang sama dengan indeks
        // (`a[1.5]`) dan operator `"ab" * 2.5`. Dulu `*f as i128` membuat
        // `math::gcd(1.1, 7)` menjawab `1` dan `range(0, 2.5)` menjawab `[0, 1]`.
        Value::Float(f) => runtime_err!(
            "butuh bilangan bulat, dapat pecahan `{}` — kalau memang mau \
             dipotong, tulis eksplisit: `{} as int`",
            Value::Float(*f), Value::Float(*f)),
        // `bool` juga bukan angka (`true + 1` ditolak dua mesin); `true as int`
        // tetap sah, karena itu konversi yang DITULIS. Yang disebut TIPE-nya,
        // bukan isinya — satu definisi ini melayani `range`, `time::`, dan `arr::`.
        _ => runtime_err!("butuh bilangan bulat, dapat `{}`", v.type_name()),
    }
}

pub fn nilai_ke_f64(v: &Value) -> Result<f64, Signal> {
    match v {
        Value::Float(f) => Ok(*f),
        Value::Int(n)   => Ok(*n as f64),
        // Sama: dua puluh fungsi `math::` dulu menjawab `math::sqrt(true)`
        // dengan `1.0` di acuan dan ditolak `--vm`.
        _ => runtime_err!("butuh angka, dapat `{}`", v.type_name()),
    }
}

impl Interpreter {
    /// Method array yang tidak memanggil fungsi apa pun per elemen.
    pub fn metode_arr_lepas(a: &[Value], method: &str, args: &[Value]) -> EvalResult {
        match method {
            "dedup" => {
                let mut r: Vec<Value> = Vec::with_capacity(a.len());
                for v in a.iter() {
                    if let Some(last) = r.last() {
                        if Self::eq_lepas(last, v)? { continue; }
                    }
                    r.push(v.clone());
                }
                Ok(Value::Array(r.into()))
            }
            "drop" | "skip" => {
                if args.is_empty() {
                    return runtime_err!("{}", crate::interpreter::pesan_metode_argumen(
                        "arr", method, args.len()));
                }
                let nama = if method == "drop" { "arr::drop" } else { "arr::skip" };
                let n = args.first().map(|v| cacah_untuk(nama, v)).transpose()?.unwrap_or(0);
                Ok(Value::Array(a.iter().skip(n).cloned().collect()))
            }
            "rotate_left" | "rotate_right" => {
                let nama = if method == "rotate_left" { "arr::rotate_left" } else { "arr::rotate_right" };
                let n = args.first().map(|v| cacah_untuk(nama, v)).transpose()?.unwrap_or(1);
                let mut r: Vec<Value> = a.to_vec();
                if !r.is_empty() {
                    let n = n % r.len();
                    if method == "rotate_left" { r.rotate_left(n) } else { r.rotate_right(n) }
                }
                Ok(Value::Array(r.into()))
            }
            // `shuffle` di VM DULU mengembalikan masukannya apa adanya, dan `sample`
            // mengembalikan `Option::Some(elemen pertama)` — dua jawaban yang terlihat …
            "shuffle" => {
                let mut idxs: Vec<usize> = (0..a.len()).collect();
                fisher_yates_shuffle(&mut idxs);
                Ok(Value::Array(idxs.into_iter().map(|i| a[i].clone()).collect()))
            }
            "sample" => {
                // `n` boleh hilang, dan bawaannya 1.
                let n = args.first().map(|v| cacah_untuk("arr::sample", v)).transpose()?.unwrap_or(1).min(a.len());
                let mut idxs: Vec<usize> = (0..a.len()).collect();
                fisher_yates_shuffle(&mut idxs);
                Ok(Value::Array(idxs.into_iter().take(n).map(|i| a[i].clone()).collect()))
            }
            "binary_search" => {
                if args.is_empty() { return runtime_err!("{}", crate::interpreter::pesan_metode_argumen("arr", "binary_search", args.len())); }
                let target = &args[0];
                // Kalau ada elemen yang tidak sebanding dengan yang dicari, hasil pencarian
                // binernya tidak berarti apa-apa — itu dilaporkan, bukan dijawab "tidak …
                let mut gagal: Option<Value> = None;
                let idx = a.binary_search_by(|probe| match Self::cmp_values(probe, target) {
                    Some(o) => o,
                    None => {
                        if gagal.is_none() { gagal = Some(probe.clone()); }
                        std::cmp::Ordering::Equal
                    }
                });
                if let Some(x) = gagal {
                    return runtime_err!(
                        "binary_search: `{}` (tipe {}) tidak bisa dibandingkan dengan yang dicari \
                         `{}` (tipe {})",
                        x, x.type_name(), target, target.type_name()
                    );
                }
                match idx {
                    Ok(i) => Ok(crate::interpreter::some_value(Value::Int(i as i128))),
                    // `-1` adalah indeks yang SAH di JRG, jadi sentinel di sini
                    // berarti `a[a.binary_search(x)]` membaca elemen terakhir
                    // waktu `x` tidak ada.
                    Err(_) => Ok(crate::interpreter::none_value()),
                }
            }
            lain => runtime_err!("arr.{} tidak ada", lain),
        }
    }
}
