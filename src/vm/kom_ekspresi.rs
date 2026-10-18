// src/vm/kom_ekspresi.rs — kompilasi ekspresi

use super::opcode::OpCode;
use super::value::{VmValue, UpvalSrc};
use crate::parser::*;
use super::value::VmFunction;
use super::compiler::*;

/// Jumlah payload varian `enum_name::variant`, dengan kunci yang sama dengan
/// `emit_enum_ctor` (Option/Result terdaftar telanjang).
fn arity_konstruktor(enum_name: &str, variant: &str) -> Option<usize> {
    let kunci = if matches!(enum_name, "Option" | "Result") {
        variant.to_string()
    } else {
        format!("{}::{}", enum_name, variant)
    };
    enum_ctor_lookup(&kunci).map(|(_, a)| a)
}

/// Ekspresi yang hasilnya PASTI `bool`: perbandingan, literal bool, dan `&&`/`||` lain.
/// Untuk yang begini `KeBool` tidak dipancarkan, jadi kondisi biasa tidak membayar apa pun.
fn pasti_bool(e: &Expr) -> bool {
    match e {
        Expr::Literal(LitKind::Bool(_), _) => true,
        Expr::Binary { op, .. } => matches!(op, BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt
            | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge | BinaryOp::And | BinaryOp::Or),
        _ => false,
    }
}

impl Compiler {
    // ── Expression ───────────────────────────────────────────────────────

    /// Emit konstruksi nilai enum buat satu variant enum/Option/Result, dengan
    /// payload dari `payload_exprs` (0, 1, atau N ekspresi — N>1 dibundel jadi …
    pub(crate) fn emit_enum_ctor(&mut self, enum_name: &str, variant: &str, payload_exprs: &[Expr]) -> CR<()> {
        let lookup_name = if matches!(enum_name, "Option" | "Result") {
            variant.to_string()
        } else {
            format!("{}::{}", enum_name, variant)
        };
        if let Some((_, arity)) = enum_ctor_lookup(&lookup_name) {
            if arity != payload_exprs.len() {
                return Err(CompileError::new(crate::interpreter::pesan_konstruktor_argumen(
                    enum_name, variant, arity, payload_exprs.len())));
            }
        }
        // Tipe dan varian datang dari PROTOTIPE di kolam konstanta, bukan dari kunci di
        // dalam peta — data pemakai tidak bisa memalsukannya.
        let proto = self.chunk.add_constant(VmValue::varian(enum_name, variant, None));
        if payload_exprs.is_empty() {
            // Varian unit tidak punya apa pun yang bisa ditulis, jadi satu nilai dipakai bersama.
            self.chunk.emit_arg(OpCode::Const, proto);
            return Ok(());
        }
        if payload_exprs.len() == 1 {
            self.compile_expr(&payload_exprs[0])?;
        } else {
            for (i, e) in payload_exprs.iter().enumerate() {
                self.dgn_temp(i, |c| c.compile_expr(e))?;
            }
            self.chunk.emit_arg(OpCode::BuildTuple, payload_exprs.len() as u32);
        }
        self.chunk.emit_arg(OpCode::BuildEnum, proto);
        Ok(())
    }

    /// Gagal saat JALAN dengan `pesan` kalau struct di puncak stack tidak punya `field`;
    /// tanpa basis sebar ketiadaannya sudah pasti, jadi tidak ada yang diperiksa.
    fn emit_panik_tanpa_field(&mut self, field: &str, periksa: bool, pesan: &str) {
        let lewati = periksa.then(|| {
            let ni = self.chunk.add_name(field);
            self.chunk.emit(OpCode::Dup);
            self.chunk.emit_arg(OpCode::HasField, ni);
            self.chunk.emit_jump(OpCode::JumpTrue)
        });
        let ci = self.chunk.add_constant(VmValue::str(pesan));
        self.chunk.emit_arg(OpCode::Const, ci);
        self.chunk.emit(OpCode::Panic);
        if let Some(l) = lewati { self.chunk.patch_jump(l); }
    }

    pub(crate) fn emit_struct_map(
        &mut self,
        type_name: &str,
        fields: &[crate::parser::FieldInit],
        spread: Option<&Expr>,
        varian: Option<(&str, &[String])>,
    ) -> CR<()> {
        let declared_fields = resolved_struct_fields(type_name);

        if let Some(base) = spread {
            // Basisnya diperiksa begitu selesai dievaluasi, seperti di interpreter.
            self.emit_bawaan("__periksa_sebar")?;
            self.dgn_temp(1, |c| c.compile_value_expr(base))?;
            let (sebutan, daftar) = match varian {
                Some((nama, milik)) => (nama.to_string(),
                    VmValue::array(milik.iter().map(|m| VmValue::str(m)).collect())),
                None => (type_name.to_string(), VmValue::Nil),
            };
            let ci = self.chunk.add_constant(VmValue::str(&sebutan));
            self.chunk.emit_arg(OpCode::Const, ci);
            let di = self.chunk.add_constant(daftar);
            self.chunk.emit_arg(OpCode::Const, di);
            self.chunk.emit_arg(OpCode::Call, 3);
        }
        // Prototipe membawa label struct-nya; yang di atasnya cuma pasangan field.
        let proto = self.chunk.add_constant(VmValue::struktur(type_name, Default::default()));
        self.chunk.emit_arg(OpCode::Const, proto);

        for (i, field) in fields.iter().enumerate() {
            // Field liar gagal saat JALAN, di gilirannya, seperti di interpreter.
            if !declared_fields.is_empty() && !declared_fields.iter().any(|d| d.name == field.name) {
                let saran = crate::diag::saran_nama_terdekat(
                    &field.name, declared_fields.iter().map(|d| d.name.as_str()));
                let pesan = self.chunk.add_constant(VmValue::str(
                    &crate::interpreter::galat_index::field_tak_dideklarasikan(
                        type_name, &field.name, saran.as_deref())));
                self.chunk.emit_arg(OpCode::Const, pesan);
                self.chunk.emit(OpCode::Panic);
            }
            let field_key = self.chunk.add_constant(VmValue::str(&field.name));
            self.chunk.emit_arg(OpCode::Const, field_key);
            // Nilai field juga penyerahan nilai: `P { isi: a }` tidak boleh membuat
            // struct yang field-nya beralias ke `a`.
            let tumpuk = spread.is_some() as usize + 1 + i * 2 + 1;
            self.dgn_temp(tumpuk, |c| c.compile_value_expr(&field.value))?;
        }
        // Default sesudah field yang ditulis, sama dengan dua mesin lain.
        let mut default_count = 0usize;
        if spread.is_none() {
            for declared in &declared_fields {
                if fields.iter().any(|field| field.name == declared.name) {
                    continue;
                }
                if let Some(default) = &declared.default {
                    let field_key = self.chunk.add_constant(VmValue::str(&declared.name));
                    self.chunk.emit_arg(OpCode::Const, field_key);
                    let tumpuk = 1 + (fields.len() + default_count) * 2 + 1;
                    self.dgn_temp(tumpuk, |c| c.compile_expr(default))?;
                    default_count += 1;
                }
            }
        }

        self.chunk.emit_arg(
            OpCode::BuildStruct,
            (default_count + fields.len()) as u32,
        );
        if spread.is_some() {
            self.chunk.emit(OpCode::MergeMap);
            // Default untuk field yang tidak dibawa basis maupun literalnya.
            for declared in &declared_fields {
                if fields.iter().any(|field| field.name == declared.name) {
                    continue;
                }
                let Some(default) = &declared.default else { continue };
                let ni = self.chunk.add_name(&declared.name);
                self.chunk.emit(OpCode::Dup);
                self.chunk.emit_arg(OpCode::HasField, ni);
                let sudah_ada = self.chunk.emit_jump(OpCode::JumpTrue);
                self.chunk.emit(OpCode::Dup);
                self.dgn_temp(2, |c| c.compile_expr(default))?;
                self.chunk.emit_arg(OpCode::SetField, ni);
                self.chunk.patch_jump(sudah_ada);
            }
        }
        // Field wajib yang tidak diisi gagal saat JALAN, sesudah semua nilainya dievaluasi.
        for declared in &declared_fields {
            if declared.default.is_some() || fields.iter().any(|f| f.name == declared.name) { continue; }
            let pesan = crate::interpreter::galat_index::field_wajib(type_name, &declared.name);
            self.emit_panik_tanpa_field(&declared.name, spread.is_some(), &pesan);
        }
        Ok(())
    }

    /// Catat nama callee kalau panggilan ini PASTI salah jumlah argumennya andai
    /// calleenya builtin bernama itu, supaya pesannya menyebut nama yang ditulis.
    fn catat_bila_salah_jumlah(&mut self, func: &Expr, argc: usize, pos: usize) {
        let nama = match func {
            Expr::Ident(n, _) => n.clone(),
            // `::len` adalah `len` yang sama; awalannya cuma menutup pembayangan.
            Expr::PathExpr { segments, .. } if segments.first().is_some_and(|s| s.is_empty()) =>
                segments[1..].join("::"),
            Expr::PathExpr { segments, .. } => segments.join("::"),
            _ => return,
        };
        use crate::interpreter::arity as ar;
        if ar::maks(&nama).is_some_and(|m| argc > m) || ar::minimal(&nama).is_some_and(|m| argc < m) {
            self.chunk.catat_nama_panggil(pos, &nama);
        }
    }

    /// Kalau `func` (posisi callee sebuah `Expr::Call`) merujuk ke constructor
    /// enum/Option/Result yang terdaftar (bare `Some`/`Ok`/ `Err`, atau path …
    pub(crate) fn resolve_enum_ctor(&self, func: &Expr) -> Option<(String, String)> {
        match func {
            Expr::Ident(name, _) if self.resolve_local(name).is_none()
                && self.resolve_upval(name).is_none()
                && super::compiler::milik_unit(name).is_none() =>
            {
                enum_ctor_lookup(name).map(|(enum_name, _)| (enum_name, name.clone()))
            }
            Expr::PathExpr { segments, .. } => {
                let full = segments.join("::");
                let variant = segments.last().cloned().unwrap_or_default();
                enum_ctor_lookup(&full).map(|(enum_name, _)| (enum_name, variant))
            }
            _ => None,
        }
    }

    /// Kompilasi sebuah ekspresi, sambil MENANDAI lokasinya.
    pub(crate) fn compile_expr(&mut self, expr: &Expr) -> CR<()> {
        let lama = self.chunk.masuk_span(expr.span());
        let hasil = self.compile_expr_dalam(expr);
        // Dipulihkan JUGA saat gagal: `?` di pemanggil akan membatalkan seluruh
        // kompilasi, tapi kalau suatu saat ada jalur yang memulihkan diri dari error …
        self.chunk.pulih_span(lama);
        hasil
    }

    pub(crate) fn compile_expr_dalam(&mut self, expr: &Expr) -> CR<()> {
        match expr {

            // Literal(LitKind, Span)
            Expr::Literal(lit, _) => self.compile_lit(lit)?,

            // Ident(String, Span)
            Expr::Ident(name, _) => {
                if let Some(slot) = self.resolve_local(name) {
                    // Penerima `x = x.m(..)`: nilainya dipindahkan, bukan dibaca — lihat
                    // `coba_konsumsi_penerima`.
                    if self.konsumsi_penerima.as_deref() == Some(name.as_str()) {
                        self.konsumsi_penerima = None;
                        self.chunk.emit_arg(OpCode::AmbilLokal, slot as u32);
                        return Ok(());
                    }
                    self.chunk.emit_arg(OpCode::LoadLocal, slot as u32);
                } else if let Some(ui) = self.resolve_upval(name) {
                    // Binding dari scope pembuat closure. Dicek SETELAH
                    // lokal supaya `let` di dalam badan closure tetap
                    // membayangi tangkapan dengan nama sama.
                    self.chunk.emit_arg(OpCode::LoadUpval, ui as u32);
                } else if let Some(g) = self.resolve_saudara(name) {
                    // Fungsi bersarang: nama globalnya sudah di-mangle.
                    let ni = self.chunk.add_name(&g);
                    self.chunk.emit_arg(OpCode::LoadGlobal, ni);
                } else if let Some((global, jenis)) = super::compiler::milik_unit(name) {
                    // Nama tingkat atas milik UNIT INI (fungsi, `const`/`let`, `var`). Didahulukan
                    // atas konstruktor dan bawaan: `fn Some` atau `fn len` milik pemakai menang,
                    // dan cuma di berkas yang mendeklarasikannya.
                    if jenis != Milik::Fungsi { self.catat_tingkat_atas(name); }
                    let ni = self.chunk.add_name(&global);
                    self.chunk.emit_arg(OpCode::LoadGlobal, ni);
                } else if let Some((enum_name, arity)) = enum_ctor_lookup(name) {
                    if arity == 0 {
                        // None, atau (kalau ada) variant unit lain yang
                        // terdaftar bare — konstruksi langsung, bukan cari
                        // global yang tidak pernah ada.
                        self.emit_enum_ctor(&enum_name, name, &[])?;
                    } else {
                        // Konstruktor sebagai NILAI (`arr::map(xs, Some)`): dulu dicari sebagai
                        // global yang tidak pernah ada, lalu dituduh salah ketik.
                        let ci = self.chunk.add_constant(VmValue::ktor(&enum_name, name, arity));
                        self.chunk.emit_arg(OpCode::Const, ci);
                    }
                } else if let Some(v) = crate::vm::bawaan::bawaan(name) {
                    // Bawaan dirujuk lewat NILAINYA: tidak ada nama global untuk ditimpa unit lain.
                    let ci = self.chunk.add_constant(v);
                    self.chunk.emit_arg(OpCode::Const, ci);
                } else {
                    // Tidak dikenal unit ini: gagal saat JALAN. Saran dan penjelasannya dihitung
                    // SEKARANG — saat jalan lokal sudah jadi slot dan nama modul sudah ber-awalan.
                    self.catat_saran_nama(name);
                    self.catat_bukan_nilai(name);
                    let ni = self.chunk.add_name(name);
                    self.chunk.emit_arg(OpCode::LoadGlobal, ni);
                }
            }
            Expr::Binary { op, lhs, rhs, .. } => {
                // Short-circuit AND / OR
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    self.compile_expr(lhs)?;
                    let lompat = if *op == BinaryOp::And { OpCode::JumpFalseKeep } else { OpCode::JumpTrueKeep };
                    let skip = self.chunk.emit_jump(lompat);
                    self.chunk.emit(OpCode::Pop);
                    self.compile_expr(rhs)?;
                    self.chunk.patch_jump(skip);
                    // Hasilnya `bool`, seperti acuan — dulu operand-nya sendiri, jadi
                    // `1 && 2` menjawab `2` di sini dan `true` di interpreter.
                    if !(pasti_bool(lhs) && pasti_bool(rhs)) { self.chunk.emit(OpCode::KeBool); }
                } else {
                    self.compile_expr(lhs)?;
                    self.dgn_temp(1, |c| c.compile_expr(rhs))?;
                    let oc = match op {
                        BinaryOp::Add  => OpCode::Add,
                        BinaryOp::Sub  => OpCode::Sub,
                        BinaryOp::Mul  => OpCode::Mul,
                        BinaryOp::Div  => OpCode::Div,
                        BinaryOp::Rem  => OpCode::Mod,
                        BinaryOp::Pow  => OpCode::Pow,
                        BinaryOp::Eq   => OpCode::Eq,
                        BinaryOp::Ne   => OpCode::Ne,
                        BinaryOp::Lt   => OpCode::Lt,
                        BinaryOp::Le   => OpCode::Le,
                        BinaryOp::Gt   => OpCode::Gt,
                        BinaryOp::Ge   => OpCode::Ge,
                        BinaryOp::BitAnd => OpCode::BitAnd,
                        BinaryOp::BitOr  => OpCode::BitOr,
                        BinaryOp::BitXor => OpCode::BitXor,
                        BinaryOp::Shl    => OpCode::Shl,
                        BinaryOp::Shr    => OpCode::Shr,
                        BinaryOp::AndNot => OpCode::AndNot,
                        BinaryOp::Chan => {
                            return Err(CompileError::new(
                                "operator channel infix tidak valid; gunakan `ch <- nilai` atau `<-ch`",
                            ));
                        }
                        BinaryOp::And | BinaryOp::Or => unreachable!("short-circuit handled above"),
                    };
                    self.chunk.emit(oc);
                }
            }

            // Unary { op, expr, span }
            Expr::Unary { op, expr, .. } => {
                // Nilai paling negatif tidak punya lawan positif, jadi
                // `-170141183460469231731687303715884105728` mustahil ditulis sebagai negasi …
                if matches!(op, UnaryOp::Neg) {
                    if let Expr::Literal(LitKind::Int(n), _) = expr.as_ref() {
                        if *n == i128::MAX as u128 + 1 {
                            let ci = self.chunk.add_constant(VmValue::Int(i128::MIN));
                            self.chunk.emit_arg(OpCode::Const, ci);
                            return Ok(());
                        }
                    }
                }
                self.compile_expr(expr)?;
                match op {
                    UnaryOp::Neg    => { self.chunk.emit(OpCode::Neg); }
                    UnaryOp::Not    => { self.chunk.emit(OpCode::Not); }
                    UnaryOp::BitNot => { self.chunk.emit(OpCode::BitNot); }
                    UnaryOp::Ref | UnaryOp::RefMut | UnaryOp::Move | UnaryOp::Deref => {}
                }
            }

            // Ternary { cond, then, else_, span }
            Expr::Ternary { cond, then, else_, .. } => {
                self.compile_expr(cond)?;
                let jf = self.chunk.emit_jump(OpCode::JumpFalse);
                self.compile_expr(then)?;
                let je = self.chunk.emit_jump(OpCode::Jump);
                self.chunk.patch_jump(jf);
                self.compile_expr(else_)?;
                self.chunk.patch_jump(je);
            }

            // Call { func, args, span }
            Expr::Call { func, args, .. } => {
                // Jalur cepat hanya kalau jumlah payload-nya COCOK; yang salah jadi panggilan
                // biasa atas nilai konstruktornya, supaya gagal saat JALAN seperti acuannya.
                if let Some((enum_name, variant)) = self.resolve_enum_ctor(func)
                    .filter(|(e, v)| arity_konstruktor(e, v) == Some(args.len()))
                {
                    self.emit_enum_ctor(&enum_name, &variant, args)?;
                } else if let Expr::PathExpr { segments, .. } = func.as_ref() {
                    // `::arr::map(...)` adalah HOF bawaan yang sama; `arr::map` milik modul
                    // pemakai bernama `arr` adalah fungsi biasa.
                    let murni = segments.first().is_some_and(|s| s.is_empty());
                    let full = if murni { segments[1..].join("::") } else { segments.join("::") };
                    let milik_modul = !murni
                        && matches!(super::compiler::resolusi_jalur(segments), Jalur::Global(_));
                    let full = if milik_modul { String::new() } else { full };
                    // Jumlah argumennya diperiksa DI SINI, sebelum `HofCall` dipancarkan.
                    let arity_hof = if matches!(full.as_str(), "arr::reduce" | "arr::zip_with" | "arr::fold") { 3 } else { 2 };
                    if is_hof_name(&full) && args.len() != arity_hof {
                        // Kalimat yang SAMA dengan pintu native interpreter, dengan
                        // nama yang ditulis — di sini namanya pasti diketahui.
                        use crate::interpreter::arity as ar;
                        let pesan = if args.len() > arity_hof {
                            ar::pesan_kebanyakan(&full, arity_hof, args.len())
                        } else {
                            ar::pesan_kurang(&full, arity_hof, Some(arity_hof), args.len(), ar::tanda_tangan(&full))
                        };
                        let ci = self.chunk.add_constant(VmValue::str(&pesan));
                        self.chunk.emit_arg(OpCode::Const, ci);
                        self.chunk.emit(OpCode::Panic);
                    } else if is_hof_name(&full) {
                        for (i, a) in args.iter().enumerate() {
                            self.dgn_temp(i, |c| c.compile_expr(a))?;
                        }
                        // Alias (`arr::count`) dijalankan lengan yang melayaninya.
                        let ni = self.chunk.add_name(super::hof::layanan_hof(&full).unwrap_or(full.as_str()));
                        self.chunk.emit_arg(OpCode::HofCall, ni);
                    } else {
                        self.compile_expr(func)?;
                        let argc = args.len() as u32;
                        for (i, a) in args.iter().enumerate() {
                            self.dgn_temp(1 + i, |c| c.compile_value_expr(a))?;
                        }
                        let pos = self.chunk.emit_arg(OpCode::Call, argc);
                        self.catat_bila_salah_jumlah(func, args.len(), pos);
                    }
                } else {
                    self.compile_expr(func)?;
                    let argc = args.len() as u32;
                    // Argumen adalah penyerahan nilai: callee tidak boleh
                    // bisa memutasi array/struct milik pemanggil.
                    for (i, a) in args.iter().enumerate() {
                        self.dgn_temp(1 + i, |c| c.compile_value_expr(a))?;
                    }
                    let pos = self.chunk.emit_arg(OpCode::Call, argc);
                    self.catat_bila_salah_jumlah(func, args.len(), pos);
                }
            }

            // Method { obj, method, args, span }
            Expr::Method { obj, method, args, .. } => {
                if is_hof_method(method) {
                    // Jumlah argumennya diputuskan SAAT JALAN: penerimanya bisa array, Option, atau
                    // tipe pemakai yang punya method bernama sama dengan aritas lain.
                    let ni = self.chunk.add_name(method);
                    if ni >= (1 << 24) || args.len() > 255 {
                        return Err(CompileError::new(format!(
                            "method `{}`: nama atau argumennya terlalu banyak untuk satu instruksi", method)));
                    }
                    self.compile_expr(obj)?;
                    for (i, arg) in args.iter().enumerate() {
                        self.dgn_temp(1 + i, |c| c.compile_value_expr(arg))?;
                    }
                    self.chunk.emit_arg(OpCode::HofMethod, ni | ((args.len() as u32) << 24));
                } else {
                    // Load fungsi method via GetMethod (dengan global fallback).
                    self.compile_expr(obj)?;
                    self.chunk.emit(OpCode::Dup);
                    let ni = self.chunk.add_name(method);
                    // Argc ikut di 8 bit atas, seperti `HofMethod`: tanpa itu pintu method tidak
                    // tahu berapa yang DITULIS pemakai, jadi ia cuma bisa memakai kalimat bentuk …
                    self.chunk.emit_arg(OpCode::GetMethod, ni | ((args.len() as u32) << 24));
                    // `[penerima, fungsi]` → `[fungsi, penerima]`
                    self.chunk.emit(OpCode::Swap);

                    // `self` DISALIN, sama seperti argumen lain.
                    let mut argc = 1u32;
                    for (i, a) in args.iter().enumerate() {
                        self.dgn_temp(2 + i, |c| c.compile_value_expr(a))?;
                        argc += 1;
                    }
                    self.chunk.emit_arg(OpCode::Call, argc);
                }
            }

            // Field { obj, field, span }
            Expr::Field { obj, field, .. } => {
                self.compile_expr(obj)?;
                let ni = self.chunk.add_name(field);
                self.chunk.emit_arg(crate::vm::visibilitas::opcode_get(field), ni);
            }

            // Index { obj, idx, span }
            Expr::Index { obj, idx, .. } => {
                if let Some(slot) = self.lokal_berindeks_polos(obj, idx) {
                    self.compile_expr(idx)?;
                    self.chunk.emit_arg(OpCode::IndeksLokal, slot as u32);
                    return Ok(());
                }
                self.compile_expr(obj)?;
                self.dgn_temp(1, |c| c.compile_expr(idx))?;
                self.chunk.emit(OpCode::GetIndex);
            }

            Expr::Slice { obj, lo, hi, .. } => {
                self.compile_expr(obj)?;
                if let Some(lo) = lo {
                    self.dgn_temp(1, |c| c.compile_expr(lo))?;
                } else {
                    self.chunk.emit(OpCode::Nil);
                }
                if let Some(hi) = hi {
                    self.dgn_temp(2, |c| c.compile_expr(hi))?;
                } else {
                    self.chunk.emit(OpCode::Nil);
                }
                self.chunk.emit(OpCode::Slice);
            }

            // Array { elems, span }
            Expr::Array { elems, .. } => {
                // Elemen literal juga penyerahan nilai: `[a, b]` tidak
                // boleh membuat array baru yang isinya beralias ke `a`.
                for (i, e) in elems.iter().enumerate() {
                    self.dgn_temp(i, |c| c.compile_value_expr(e))?;
                }
                self.chunk.emit_arg(OpCode::BuildArray, elems.len() as u32);
            }

            // Tuple { elems, span }
            // `()` yang ditulis adalah unit, seperti acuannya.
            Expr::Tuple { elems, .. } if elems.is_empty() => { self.chunk.emit(OpCode::Unit); }
            Expr::Tuple { elems, .. } => {
                for (i, e) in elems.iter().enumerate() {
                    self.dgn_temp(i, |c| c.compile_value_expr(e))?;
                }
                self.chunk.emit_arg(OpCode::BuildTuple, elems.len() as u32);
            }

            // Block(Block)
            Expr::Block(blk) => {
                self.compile_block_as_value(blk)?;
            }

            // Closure { params, body, span, ..
            Expr::Closure { params, body, is_move, .. } => {
                let pnames: Vec<String> = params.iter().map(param_name).collect();
                let ups = self.rencana_tangkapan(&pnames, body);
                for u in &ups {
                    if let UpvalSrc::ParentLocal(slot) = u.src { self.slot_tertangkap.insert(slot as usize); }
                }

                let arity = params.len() as u8;
                let mut c = Compiler::new("<closure>", arity);
                // Closure di dalam sebuah method masih "di dalam tipe itu": ia bisa
                // menangkap `self` dan interpreter memang mengizinkannya menyentuh field …
                c.chunk.tipe_self = self.chunk.tipe_self.clone();
                c.scope_depth = 1;
                for p in params {
                    let slot = c.locals.len();
                    c.locals.push(Local { name: param_name(p), depth: 1, slot });
                }
                // Badan closure diresolusi terhadap: lokalnya sendiri dulu
                // (parameter + `let` di dalam, jadi bayangan tetap menang),
                // baru daftar tangkapan ini, baru global.
                c.upvals = ups.clone();
                c.compile_expr(body)?;
                c.chunk.emit(OpCode::Return);

                let srcs: Vec<UpvalSrc> = ups.iter().map(|u| u.src).collect();
                let kosong = srcs.is_empty();
                let mut fn_val = VmFunction::closure("<closure>", arity, c.chunk, srcs, *is_move);
                // Nama parameternya ikut disimpan.
                fn_val.params = pnames.clone();
                fn_val.arity_min = params.iter().filter(|p| p.default.is_none()).count() as u8;
                fn_val.variadic = params.last().map_or(false, |p| p.is_variadic);
                let ci = self.chunk.add_constant(VmValue::Function(std::sync::Arc::new(fn_val)));
                if kosong {
                    // Tanpa tangkapan tidak ada yang perlu dirangkai: nilai
                    // fungsinya konstan, jadi biayanya persis sama dengan
                    // sebelum penangkapan ada — satu `Const`, nol alokasi.
                    self.chunk.emit_arg(OpCode::Const, ci);
                } else {
                    self.chunk.emit_arg(OpCode::MakeClosure, ci);
                }
            }

            // PathExpr { segments, span }
            Expr::PathExpr { segments, .. } => {
                let full = segments.join("::");
                let variant = segments.last().cloned().unwrap_or_default();
                match enum_ctor_lookup(&full) {
                    // Variant unit user-enum direferensi bare tanpa `(...)`
                    // (mis. `Shape::Empty`, bukan `Shape::Circle(r)`).
                    Some((enum_name, 0)) => self.emit_enum_ctor(&enum_name, &variant, &[])?,
                    Some((enum_name, arity)) => {
                        let ci = self.chunk.add_constant(VmValue::ktor(&enum_name, &variant, arity));
                        self.chunk.emit_arg(OpCode::Const, ci);
                    }
                    None => match super::compiler::resolusi_jalur(segments) {
                        Jalur::Global(g) => {
                            let ni = self.chunk.add_name(&g);
                            self.chunk.emit_arg(OpCode::LoadGlobal, ni);
                        }
                        Jalur::Bawaan(v) => {
                            let ci = self.chunk.add_constant(v);
                            self.chunk.emit_arg(OpCode::Const, ci);
                        }
                        Jalur::Tolak(pesan) => self.emit_tolak(&pesan),
                        Jalur::Biasa => {
                            self.catat_bukan_nilai(&full);
                            let ni = self.chunk.add_name(&full);
                            self.chunk.emit_arg(OpCode::LoadGlobal, ni);
                        }
                    },
                }
            }

            // Struct { ty, fields, spread, span } → struct ber-label
            Expr::Struct { ty, fields, spread, .. } => {
                let qualified = qualified_type_expr_name(ty);
                if let Some((enum_name, declared_fields)) = enum_struct_ctor_lookup(&qualified) {
                    for field in fields {
                        if !declared_fields.iter().any(|name| name == &field.name) {
                            return Err(CompileError::new(
                                crate::interpreter::pesan_field_varian(&field.name, &qualified)));
                        }
                    }
                    let variant = qualified.rsplit("::").next().unwrap_or(&qualified);
                    // Payload-nya struct bernama varian, dibangun tanpa apa pun di bawahnya di
                    // stack — dulu lima konstanta di bawahnya tidak ikut terhitung sementara.
                    let sebutan = format!("{}::{}", crate::ruang_tipe::bersih(&enum_name), variant);
                    self.emit_struct_map(variant, fields, spread.as_deref(), Some((&sebutan, &declared_fields)))?;
                    for wajib in declared_fields.iter().filter(|d| !fields.iter().any(|f| &f.name == *d)) {
                        let pesan = crate::interpreter::galat_index::field_wajib_varian(
                            &format!("{}::{}", crate::ruang_tipe::bersih(&enum_name), variant), wajib);
                        self.emit_panik_tanpa_field(wajib, spread.is_some(), &pesan);
                    }
                    let proto = self.chunk.add_constant(VmValue::varian(&enum_name, variant, None));
                    self.chunk.emit_arg(OpCode::BuildEnum, proto);
                } else {
                    let mut nama = type_expr_name(ty);
                    // `Self { .. }` di dalam method berarti tipe pemilik method-nya.
                    if nama == "Self" {
                        if let Some(t) = &self.chunk.tipe_self { nama = t.clone(); }
                    }
                    // Belum dikenal saat kompilasi: diputuskan saat JALAN, seperti acuan.
                    if nama != "dict" && struct_def_lookup(&nama).is_none()
                        && !crate::interpreter::galat_index::STRUCT_DATA_PUSTAKA.contains(&nama.as_str())
                    {
                        self.emit_bawaan("__periksa_struct")?;
                        let ci = self.chunk.add_constant(VmValue::str(&nama));
                        self.chunk.emit_arg(OpCode::Const, ci);
                        self.chunk.emit_arg(OpCode::Call, 1);
                        self.chunk.emit(OpCode::Pop);
                    }
                    self.emit_struct_map(&nama, fields, spread.as_deref(), None)?;
                }
            }

            // Range { lo, hi, inclusive, span } → compile sebagai array slice
            Expr::Range { lo, hi, inclusive, .. } => {
                let Some(hi_e) = hi else {
                    return Err(CompileError::new("range tanpa batas atas tidak didukung Bytecode VM"));
                };
                let fn_name = if *inclusive { "range_inclusive" } else { "range" };
                self.emit_bawaan(fn_name)?;
                if let Some(lo_e) = lo {
                    self.dgn_temp(1, |c| c.compile_expr(lo_e))?;
                } else {
                    self.chunk.emit_arg(OpCode::IntImm, 0);
                }
                self.dgn_temp(2, |c| c.compile_expr(hi_e))?;
                self.chunk.emit_arg(OpCode::Call, 2);
            }

            // Pipe { lhs, rhs, span } → rhs(lhs)
            Expr::Pipe { lhs, rhs, .. } => {
                self.compile_expr(rhs)?;
                self.dgn_temp(1, |c| c.compile_expr(lhs))?;
                self.chunk.emit_arg(OpCode::Call, 1);
            }

            // MakeChan { ty, cap, span } → chan::new(cap)
            Expr::MakeChan { cap, .. } => {
                if let Some(cap_expr) = cap {
                    self.compile_expr(cap_expr)?;
                } else {
                    self.chunk.emit_arg(OpCode::IntImm, 0); // unbuffered
                }
                self.chunk.emit(OpCode::ChanMake);
            }

            // Send { chan, val, span } → chan <- val
            Expr::Send { chan, val, .. } => {
                self.compile_expr(chan)?;
                self.dgn_temp(1, |c| c.compile_expr(val))?;
                self.chunk.emit(OpCode::ChanSend);
                self.chunk.emit(OpCode::Nil); // send returns nil
            }

            // Recv { chan, span } → <-chan
            Expr::Recv { chan, .. } => {
                self.compile_expr(chan)?;
                self.chunk.emit(OpCode::ChanRecv);
            }

            Expr::Cast { expr, ty, .. } => {
                // Parser sudah menolak sasaran yang tidak bermakna, dengan kalimat
                // yang sama untuk tiga mesin; ini cuma jaring pengaman.
                let sasaran = sasaran_cast(ty).ok_or_else(|| CompileError::new(
                    crate::interpreter::konversi::pesan_sasaran_cast_tak_sah(&type_expr_name(ty))))?;
                let (builtin, lebar) = match sasaran {
                    SasaranCast::Builtin(b) => (b, None),
                    SasaranCast::Lebar(nomor) => ("__ubah_bulat", Some(nomor)),
                    SasaranCast::Pointer => ("__jadi_pointer", None),
                };
                self.emit_bawaan(builtin)?;
                self.dgn_temp(1, |c| c.compile_expr(expr))?;
                match lebar {
                    Some(nomor) => {
                        let ci = self.chunk.add_constant(VmValue::Int(nomor as i128));
                        self.chunk.emit_arg(OpCode::Const, ci);
                        self.chunk.emit_arg(OpCode::Call, 2);
                    }
                    None => { self.chunk.emit_arg(OpCode::Call, 1); }
                }
            }
            Expr::Match { scrutinee, arms, .. } => {
                self.compile_match(scrutinee, arms)?;
            }

            // `if` as an expression — every path must leave exactly one
            // value on the stack (see `compile_if_expr`).
            Expr::If(is) => {
                self.compile_if_expr(is)?;
            }

            // ArrayRepeat { elem, count } → [elem; count]
            Expr::ArrayRepeat { elem, count, .. } => {
                // Compile elem dan count, lalu emit BuildRepeat
                // VM akan membuat array berisi elem sebanyak count kali
                self.compile_expr(elem)?;
                self.dgn_temp(1, |c| c.compile_expr(count))?;
                self.chunk.emit(OpCode::BuildRepeat);
            }

            // Semua ekspresi lain → emit Nil (placeholder)
            Expr::Propagate { expr, .. } => {
                self.compile_expr(expr)?;
                self.chunk.emit(OpCode::Propagate);
            }
            Expr::Await { expr, .. } => {
                self.compile_expr(expr)?;
            }
            // Template literal `«...»` belum ada di VM.
            Expr::Template { parts, .. } => {
                let kosong = self.chunk.add_constant(VmValue::str(""));
                self.chunk.emit_arg(OpCode::Const, kosong);
                for part in parts {
                    match part {
                        TemplatePart::Text(t) => {
                            let ci = self.chunk.add_constant(VmValue::str(t.as_str()));
                            self.chunk.emit_arg(OpCode::Const, ci);
                        }
                        // Akumulator sudah menumpuk di stack saat ekspresi
                        // ini dikompilasi.
                        TemplatePart::Expr(e) => self.dgn_temp(1, |c| c.compile_expr(e))?,
                    }
                    self.chunk.emit(OpCode::Concat);
                }
            }
            Expr::Error(_) => return Err(CompileError::new("expression AST tidak valid")),
        }
        Ok(())
    }

    pub(crate) fn compile_lit(&mut self, lit: &LitKind) -> CR<()> {
        match lit {
            LitKind::Nil        => { self.chunk.emit(OpCode::Nil); }
            LitKind::Bool(b)    => { self.chunk.emit(if *b { OpCode::True } else { OpCode::False }); }
            LitKind::Int(n) if *n <= 32767 => {
                self.chunk.emit_arg(OpCode::IntImm, *n as u32);
            }
            // Integer VM sekarang 128 bit — sama lebarnya dengan
            // interpreter — jadi literal yang sah di satu backend sah juga
            // di backend lain. Yang tersisa cuma batas nyata tipe itu.
            LitKind::Int(n) if *n <= i128::MAX as u128 => {
                let ci = self.chunk.add_constant(VmValue::Int(*n as i128));
                self.chunk.emit_arg(OpCode::Const, ci);
            }
            LitKind::Int(n) => {
                return Err(CompileError::new(format!(
                    "literal integer {} di luar jangkauan integer JRG (128 bit)",
                    n
                )));
            }
            LitKind::Float(f) | LitKind::Imag(f) => {
                let ci = self.chunk.add_constant(VmValue::Float(*f));
                self.chunk.emit_arg(OpCode::Const, ci);
            }
            LitKind::Str(s) | LitKind::RawStr(s) => {
                let ci = self.chunk.add_constant(VmValue::str(s.as_str()));
                self.chunk.emit_arg(OpCode::Const, ci);
            }
            LitKind::Rune(r) => {
                // Rune adalah tipe tersendiri, bukan kode angkanya.
                let ci = self.chunk.add_constant(VmValue::Rune(*r));
                self.chunk.emit_arg(OpCode::Const, ci);
            }
        }
        Ok(())
    }

    /// Jalur cepat untuk pola MENUMBUHKAN KOLEKSI: `a = a + <x>` dan `a += <x>`.
    pub(crate) fn coba_tumbuh_di_tempat(&mut self, s: &crate::parser::AssignStmt) -> CR<bool> {
        if s.targets.len() != 1 || s.values.len() != 1 { return Ok(false); }
        let Expr::Ident(name, _) = &s.targets[0] else { return Ok(false) };

        // `a = a + x + y + …` juga: satu definisi rantai dengan interpreter.
        let operan: Vec<&Expr> = match (&s.op, &s.values[0]) {
            (AssignOp::PlusEq, v) => vec![v],
            (AssignOp::Eq, e) => match crate::interpreter::capture::rantai_tambah(e, name) {
                Some(o) => o,
                None => return Ok(false),
            },
            _ => return Ok(false),
        };
        let Some(slot) = self.resolve_local(name) else {
            // Tangkapan closure atau `var` modul: `A = A + x` dengan satu operan.
            let (Expr::Binary { lhs, .. }, AssignOp::Eq, [kanan]) = (&s.values[0], &s.op, operan.as_slice()) else {
                return Ok(false);
            };
            let Some((coba, simpan, idx)) = self.sasaran_sel(name) else { return Ok(false) };
            self.compile_expr(lhs)?;
            self.dgn_temp(1, |c| c.compile_expr(kanan))?;
            let sp = s.values[0].span();
            self.emit_tumbuh_sel(coba, idx, simpan, idx, |c| {
                let lama = c.chunk.masuk_span(sp);
                c.chunk.emit(OpCode::Add);
                c.chunk.pulih_span(lama);
            });
            return Ok(true);
        };
        if operan.iter().any(|e| crate::interpreter::capture::names_mentioned(e).contains(name.as_str())) {
            return Ok(false);
        }

        self.chunk.emit_arg(OpCode::AmbilLokal, slot as u32);
        // Lokal yang sudah ditangkap closure tinggal di sel: `AmbilLokal` cuma membacanya, jadi
        // operan terakhir mencoba menyambung ke selnya dulu.
        let terakhir = if self.slot_tertangkap.contains(&slot) { operan.len() - 1 } else { usize::MAX };
        // Galat `+`-nya menunjuk tempat yang sama dengan jalur biasa (dan dengan interpreter).
        let sp = if matches!(s.op, AssignOp::PlusEq) { s.targets[0].span() } else { s.values[0].span() };
        for (i, e) in operan.into_iter().enumerate() {
            self.dgn_temp(1, |c| c.compile_expr(e))?;
            let lama = self.chunk.masuk_span(sp);
            if i == terakhir {
                self.emit_tumbuh_sel(OpCode::CobaTumbuhLokal, slot as u32, OpCode::StoreLocal, slot as u32,
                    |c| { c.chunk.emit(OpCode::Add); });
                self.chunk.pulih_span(lama);
                return Ok(true);
            }
            self.chunk.emit(OpCode::Add);
            self.chunk.pulih_span(lama);
        }
        self.chunk.emit_arg(OpCode::SimpanBuang, slot as u32);
        Ok(true)
    }

    /// Binding bersel yang boleh ditugasi dan bukan lokal: opcode coba-tumbuh, opcode simpan,
    /// dan indeksnya (nomor tangkapan, atau nomor nama `var` modul).
    fn sasaran_sel(&mut self, name: &str) -> Option<(OpCode, OpCode, u32)> {
        if self.resolve_local(name).is_some() { return None; }
        if let Some(ui) = self.resolve_upval(name) {
            return Some((OpCode::CobaTumbuhUpval, OpCode::StoreUpval, ui as u32));
        }
        match super::compiler::milik_unit(name) {
            Some((g, Milik::Var)) => Some((OpCode::CobaTumbuhGlobal, OpCode::StoreGlobal, self.chunk.add_name(&g))),
            _ => None,
        }
    }

    /// `[kini, kanan]` di stack: sambung di tempat lewat `coba`, atau kerjakan `biasa` (yang
    /// meninggalkan satu nilai) lalu simpan. Dua cabangnya meninggalkan stack yang sama.
    fn emit_tumbuh_sel(&mut self, coba: OpCode, arg: u32, simpan: OpCode, idx: u32, biasa: impl FnOnce(&mut Self)) {
        self.chunk.emit_arg(coba, arg);
        let lambat = self.chunk.emit_jump(OpCode::JumpFalse);
        let selesai = self.chunk.emit_jump(OpCode::Jump);
        self.chunk.patch_jump(lambat);
        biasa(self);
        self.chunk.emit_arg(simpan, idx);
        self.chunk.emit(OpCode::Pop);
        self.chunk.patch_jump(selesai);
    }

    /// `jalur = jalur + x` dan `jalur = jalur.push(x)` atas field/elemen: kalau tempat itu
    /// masih memegang nilai yang tadi dibaca (`SamaTempat`), dikerjakan sebagai `jalur += x`.
    pub(crate) fn coba_tumbuh_jalur(&mut self, s: &crate::parser::AssignStmt) -> CR<bool> {
        use crate::interpreter::eval_stmt::{jalur_murni, jalur_sama};
        if s.targets.len() != 1 || s.values.len() != 1 || !matches!(s.op, AssignOp::Eq) { return Ok(false); }
        let tgt = &s.targets[0];
        if matches!(tgt, Expr::Ident(..)) { return Ok(false); }
        let (lhs, rhs, dorong) = match &s.values[0] {
            Expr::Binary { op: crate::parser::BinaryOp::Add, lhs, rhs, .. } => (&**lhs, &**rhs, false),
            Expr::Method { obj, method, args, .. } if method == "push" && args.len() == 1 => (&**obj, &args[0], true),
            _ => return Ok(false),
        };
        if !jalur_murni(tgt, &mut Vec::new()) || !jalur_sama(tgt, lhs) { return Ok(false); }
        let sp = s.values[0].span();

        // `a[i] = a[i] + x` atas lokal: `kini` dan `kanan` tinggal di puncak stack, jadi kalau
        // bukan penyambungan array, `Add` langsung memakainya — tanpa lokal sementara.
        if let (false, Expr::Index { obj, idx, .. }) = (dorong, tgt) {
            if let Some(slot) = self.lokal_berindeks_polos(obj, idx) {
                self.compile_expr(lhs)?;
                self.dgn_temp(1, |c| c.compile_value_expr(rhs))?;
                self.dgn_temp(2, |c| c.compile_expr(idx))?;
                let di = self.chunk.code.len();
                self.chunk.emit_arg(OpCode::TumbuhIndexLokal, slot as u32);
                let lama = self.chunk.masuk_span(sp);
                self.chunk.emit(OpCode::Add);
                self.chunk.pulih_span(lama);
                let lama = self.chunk.masuk_span(tgt.span());
                let hasil = self.dgn_temp(1, |c| c.compile_expr(idx));
                self.chunk.emit_arg(OpCode::TulisIndexLokal, slot as u32);
                self.chunk.pulih_span(lama);
                hasil?;
                let lompat = self.chunk.code.len() - di - 1;
                if lompat > 255 || slot >= 1 << 24 {
                    return Err(CompileError::new("jalur tumbuh: lompatan atau slot di luar jangkauan"));
                }
                self.chunk.code[di].arg |= (lompat as u32) << 24;
                return Ok(true);
            }
        }

        self.begin_scope();
        self.compile_expr(lhs)?;
        let kini = self.slot_berikutnya();
        self.define_local("__tumbuh_kini__");
        self.compile_value_expr(rhs)?;
        let kanan = self.slot_berikutnya();
        self.define_local("__tumbuh_kanan__");
        if kanan != kini + 1 {
            return Err(CompileError::new("jalur tumbuh: slot nilai kini dan kanan tidak berurutan"));
        }
        // Bacaan ulangnya memakai ekspresi SASARAN, supaya galatnya menunjuk sasaran.
        self.compile_expr(tgt)?;
        self.chunk.emit_arg(OpCode::SamaTempat, kini as u32 | if dorong { 1 << 31 } else { 0 });
        let lambat = self.chunk.emit_jump(OpCode::JumpFalse);
        self.begin_scope();
        self.compile_assign_target(tgt, &AssignOp::PlusEq, kanan)?;
        self.end_scope();
        let selesai = self.chunk.emit_jump(OpCode::Jump);
        self.chunk.patch_jump(lambat);

        // Tempatnya sudah ditulis, atau ini bukan penyambungan array: nilai yang tadi dibaca
        // dipakai apa adanya, dengan aturan `+` (atau pemanggilan `push`) yang biasa.
        let lama = self.chunk.masuk_span(sp);
        self.chunk.emit_arg(OpCode::LoadLocal, kini as u32);
        if dorong {
            let ni = self.chunk.add_name("push");
            self.chunk.emit(OpCode::Dup);
            self.chunk.emit_arg(OpCode::GetMethod, ni | (1 << 24));
            self.chunk.emit(OpCode::Swap);
            self.chunk.emit_arg(OpCode::LoadLocal, kanan as u32);
            self.chunk.emit_arg(OpCode::Call, 2);
        } else {
            self.chunk.emit_arg(OpCode::LoadLocal, kanan as u32);
            self.chunk.emit(OpCode::Add);
        }
        self.chunk.pulih_span(lama);
        self.chunk.emit_arg(OpCode::SimpanBuang, kanan as u32);
        self.begin_scope();
        self.compile_assign_target(tgt, &AssignOp::Eq, kanan)?;
        self.end_scope();
        self.chunk.patch_jump(selesai);
        self.end_scope();
        Ok(true)
    }

    /// `x = x.m(..)` dan rantainya: nilai `x` DIPINDAHKAN ke method sebagai penerima. Selama
    /// method jalan pemanggil tidak memegangnya, jadi tulisan ke `self` di dalam method
    /// mengenai penyimpanan yang tinggal satu pemegangnya — tanpa salinan seluruh isi.
    pub(crate) fn coba_konsumsi_penerima(&mut self, s: &crate::parser::AssignStmt) -> CR<bool> {
        if s.targets.len() != 1 || s.values.len() != 1 || !matches!(s.op, AssignOp::Eq) { return Ok(false); }
        let Expr::Ident(name, _) = &s.targets[0] else { return Ok(false) };
        if !Self::bentuk_konsumsi(name, &s.values[0]) { return Ok(false); }
        let Some(slot) = self.resolve_local(name) else { return Ok(false) };
        self.konsumsi_penerima = Some(name.clone());
        let hasil = self.compile_expr(&s.values[0]);
        self.konsumsi_penerima = None;
        hasil?;
        self.chunk.emit_arg(OpCode::SimpanBuang, slot as u32);
        Ok(true)
    }

    /// `a, b = e`: nilainya dibongkar lewat `BongkarTuple`, lalu tiap sasaran ditugasi seperti
    /// penugasan berganda biasa. Sasaran lokal yang juga penerima `e` dipindahkan (`q, v = q.pop()`).
    pub(crate) fn compile_bongkar(&mut self, s: &crate::parser::AssignStmt) -> CR<()> {
        self.begin_scope();
        let pindah = s.targets.iter().find_map(|t| match t {
            Expr::Ident(n, _) if Self::bentuk_konsumsi(n, &s.values[0]) && self.resolve_local(n).is_some() => Some(n.clone()),
            _ => None,
        });
        self.konsumsi_penerima = pindah;
        let hasil = self.compile_expr(&s.values[0]);
        self.konsumsi_penerima = None;
        hasil?;
        let n = s.targets.len();
        let lama = self.chunk.masuk_span(s.values[0].span());
        self.chunk.emit_arg(OpCode::BongkarTuple, n as u32);
        self.chunk.pulih_span(lama);
        let mut slots = Vec::with_capacity(n);
        for i in 0..n {
            slots.push(self.slot_berikutnya());
            self.define_local(&format!("__assign_rhs_{}__", i));
        }
        for (target, slot) in s.targets.iter().zip(slots) {
            self.compile_assign_target(target, &s.op, slot)?;
        }
        self.end_scope();
        Ok(())
    }

    /// Apakah `nilai` berbentuk rantai method berakar pada `name`, atau panggilan yang argumen
    /// pertamanya `name` — tanpa `name` disebut di tempat lain?
    fn bentuk_konsumsi(name: &str, nilai: &Expr) -> bool {
        let name = &name.to_string();
        let mut kini = nilai;
        let mut dalam = 0;
        // `x = f(x, ..)`: argumen pertama dipindahkan (`d = dict::set(d, k, v)`).
        if let Expr::Call { func, args, .. } = kini {
            let menyebut = |e: &Expr| crate::interpreter::capture::names_mentioned(e).contains(name.as_str());
            let cocok = matches!(&**func, Expr::Ident(..) | Expr::PathExpr { .. })
                && matches!(args.first(), Some(Expr::Ident(n, _)) if n == name)
                && !menyebut(func) && !args[1..].iter().any(menyebut);
            if !cocok { return false; }
            dalam = 1;
            kini = &args[0];
        }
        loop {
            match kini {
                Expr::Method { obj, args, .. } => {
                    // Argumen dihitung selagi bindingnya kosong, jadi tidak boleh menyebutnya.
                    if args.iter().any(|a| crate::interpreter::capture::names_mentioned(a).contains(name.as_str())) {
                        return false;
                    }
                    dalam += 1;
                    kini = obj;
                }
                Expr::Ident(n, _) if n == name && dalam > 0 => break,
                _ => return false,
            }
        }
        true
    }

    /// Jalur cepat `a = arr::push(a, x)` / `a = a.push(x)` / `a = a.pop()[0]`.
    pub(crate) fn coba_ubah_di_tempat(&mut self, s: &crate::parser::AssignStmt) -> CR<bool> {
        if s.targets.len() != 1 || s.values.len() != 1 { return Ok(false); }
        if !matches!(s.op, AssignOp::Eq) { return Ok(false); }
        let Expr::Ident(name, _) = &s.targets[0] else { return Ok(false) };

        // `pop` menjawab `(array_baru, nilai)`, jadi bentuk yang benar-benar
        // ditulis orang adalah `a = arr::pop(a)[0]`. Indeksnya dipisah di
        // sini dan dipasang lagi lewat opcode `Index` yang biasa.
        let (inti, indeks): (&Expr, Option<&Expr>) = match &s.values[0] {
            Expr::Index { obj, idx, .. } => (&**obj, Some(&**idx)),
            lain => (lain, None),
        };

        // 0 push, 1 pop, 2 reverse, 3 concat (+4 untuk bentuk method) — dibaca `ArrUbahMilik`.
        let (op, arg_expr): (u32, Option<&Expr>) = match inti {
            Expr::Call { func, args, .. } if !args.is_empty() => {
                if !BOLEH_JALUR_ARR.with(|f| f.get()) { return Ok(false); }
                let Expr::Ident(n0, _) = &args[0] else { return Ok(false) };
                if n0 != name { return Ok(false); }
                let nama_fn = match &**func {
                    Expr::Ident(n, _) => n.clone(),
                    Expr::PathExpr { segments, .. } => segments.join("::"),
                    _ => return Ok(false),
                };
                // Kalau namanya ternyata sebuah lokal/upvalue, ia jelas
                // bukan builtin — jangan disambar.
                if self.resolve_local(&nama_fn).is_some() || self.resolve_upval(&nama_fn).is_some() {
                    return Ok(false);
                }
                match (nama_fn.as_str(), args.len()) {
                    ("arr::push", 2)    => (0, Some(&args[1])),
                    ("arr::pop", 1)     => (1, None),
                    ("arr::reverse", 1) => (2, None),
                    ("arr::concat", 2)  => (3, Some(&args[1])),
                    _ => return Ok(false),
                }
            }
            Expr::Method { obj, method, args, .. } => {
                let Expr::Ident(n0, _) = &**obj else { return Ok(false) };
                if n0 != name { return Ok(false); }
                // Sebuah tipe pemakai boleh punya `push`/`pop`/`reverse`/ `concat` sendiri,
                // dan namanya di sini tidak menyebutkan tipe apa pun — jadi begitu nama itu …
                if METODE_PEMAKAI.with(|m| m.borrow().contains(method.as_str())) {
                    return Ok(false);
                }
                // Bit 4 menandai bentuk METHOD, supaya penerima yang bukan array
                // dijawab kalimat pintu method (seperti acuan), bukan kalimat `arr::`.
                match (method.as_str(), args.len()) {
                    ("push", 1)    => (4, Some(&args[0])),
                    ("pop", 0)     => (5, None),
                    ("reverse", 0) => (6, None),
                    ("concat", 1)  => (7, Some(&args[0])),
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        };

        // Argumen tambahan dan ekspresi indeks tidak boleh menyebut `a`:
        // keduanya dihitung setelah bindingnya dikosongkan.
        for e in [arg_expr, indeks].into_iter().flatten() {
            if crate::interpreter::capture::names_mentioned(e).contains(name.as_str()) {
                return Ok(false);
            }
        }

        let Some(slot) = self.resolve_local(name) else {
            // Tangkapan closure atau `var` modul: `A = A.push(x)` dan `A = A.concat(b)`.
            let (Some(e), None, 0 | 3) = (arg_expr, indeks, op & 3) else { return Ok(false) };
            let Some((coba, simpan, idx)) = self.sasaran_sel(name) else { return Ok(false) };
            self.compile_expr(&s.targets[0])?;
            self.dgn_temp(1, |c| c.compile_expr(e))?;
            let sp = inti.span();
            self.emit_tumbuh_sel(coba, idx | if op & 3 == 0 { 1 << 31 } else { 0 }, simpan, idx, |c| {
                let lama = c.chunk.masuk_span(sp);
                c.chunk.emit_arg(OpCode::ArrUbahMilik, op);
                c.chunk.pulih_span(lama);
            });
            return Ok(true);
        };

        self.chunk.emit_arg(OpCode::AmbilLokal, slot as u32);
        if let Some(e) = arg_expr {
            self.dgn_temp(1, |c| c.compile_expr(e))?;
        }
        if let (true, Some(_), None, 0 | 3) = (self.slot_tertangkap.contains(&slot), arg_expr, indeks, op & 3) {
            // Lokal bersel (ditangkap closure): `push`/`concat` mencoba menyambung ke selnya dulu.
            let sp = inti.span();
            let arg = slot as u32 | if op & 3 == 0 { 1 << 31 } else { 0 };
            self.emit_tumbuh_sel(OpCode::CobaTumbuhLokal, arg, OpCode::StoreLocal, slot as u32, |c| {
                let lama = c.chunk.masuk_span(sp);
                c.chunk.emit_arg(OpCode::ArrUbahMilik, op);
                c.chunk.pulih_span(lama);
            });
            return Ok(true);
        }
        // Galatnya (sasaran bukan array) menunjuk pemanggilannya, seperti interpreter.
        let lama = self.chunk.masuk_span(inti.span());
        self.chunk.emit_arg(OpCode::ArrUbahMilik, op);
        self.chunk.pulih_span(lama);
        if let Some(e) = indeks {
            self.dgn_temp(1, |c| c.compile_expr(e))?;
            self.chunk.emit(OpCode::GetIndex);
        }
        self.chunk.emit_arg(OpCode::SimpanBuang, slot as u32);
        Ok(true)
    }

    /// Slot `obj` kalau `obj[idx]` adalah lokal yang diindeks nama atau literal: menghitung
    /// index semacam itu tidak menjalankan kode apa pun, jadi wadahnya boleh dibaca SESUDAHNYA.
    fn lokal_berindeks_polos(&self, obj: &Expr, idx: &Expr) -> Option<usize> {
        let Expr::Ident(nama, _) = obj else { return None };
        if self.konsumsi_penerima.as_deref() == Some(nama.as_str()) { return None; }
        if !matches!(idx, Expr::Ident(..) | Expr::Literal(crate::parser::LitKind::Int(_) | crate::parser::LitKind::Str(_), _)) {
            return None;
        }
        self.resolve_local(nama)
    }

    pub(crate) fn compile_assignment(&mut self, assignment: &crate::parser::AssignStmt) -> CR<()> {
        if self.coba_tumbuh_di_tempat(assignment)? { return Ok(()); }
        if self.coba_ubah_di_tempat(assignment)?   { return Ok(()); }
        if self.coba_tumbuh_jalur(assignment)?     { return Ok(()); }
        if self.coba_konsumsi_penerima(assignment)? { return Ok(()); }
        if assignment.targets.len() > 1 && assignment.values.len() == 1 && matches!(assignment.op, AssignOp::Eq) {
            return self.compile_bongkar(assignment);
        }
        // `x = e` atas lokal: nilainya langsung pindah ke slotnya, tanpa lokal sementara.
        if let ([sasaran @ Expr::Ident(nama, _)], [nilai], AssignOp::Eq) =
            (assignment.targets.as_slice(), assignment.values.as_slice(), &assignment.op)
        {
            if let (true, Some(slot)) = (nama != "_", self.resolve_local(nama)) {
                self.compile_value_expr(nilai)?;
                let lama = self.chunk.masuk_span(sasaran.span());
                self.chunk.emit_arg(OpCode::SimpanBuang, slot as u32);
                self.chunk.pulih_span(lama);
                return Ok(());
            }
        }

        // `a[i] = e` atas lokal dengan index nama/literal: tanpa salinan wadah dan lokal sementara.
        if let ([sasaran @ Expr::Index { obj, idx, .. }], [nilai], AssignOp::Eq) =
            (assignment.targets.as_slice(), assignment.values.as_slice(), &assignment.op)
        {
            if let Some(slot) = self.lokal_berindeks_polos(obj, idx) {
                self.compile_value_expr(nilai)?;
                let lama = self.chunk.masuk_span(sasaran.span());
                let hasil = self.dgn_temp(1, |c| c.compile_expr(idx));
                self.chunk.emit_arg(OpCode::TulisIndexLokal, slot as u32);
                self.chunk.pulih_span(lama);
                return hasil;
            }
        }

        self.begin_scope();

        let mut rhs_slots = Vec::with_capacity(assignment.targets.len());
        for (i, value) in assignment.values.iter().enumerate() {
            // Sisi kanan adalah penyerahan nilai, jadi disalin — `b = a`
            // memberi `b` nilai terpisah. (Sisi KIRI di
            // `compile_assign_target` sengaja tidak disalin.)
            self.compile_value_expr(value)?;
            let slot = self.slot_berikutnya();
            self.define_local(&format!("__assign_rhs_{}__", i));
            rhs_slots.push(slot);
        }
        while rhs_slots.len() < assignment.targets.len() {
            self.chunk.emit(OpCode::Nil);
            let slot = self.slot_berikutnya();
            self.define_local(&format!("__assign_rhs_{}__", rhs_slots.len()));
            rhs_slots.push(slot);
        }

        for (target, rhs_slot) in assignment.targets.iter().zip(rhs_slots) {
            self.compile_assign_target(target, &assignment.op, rhs_slot)?;
        }

        self.end_scope();
        Ok(())
    }

    /// Kompilasi ekspresi sebagai JALUR TARGET penulisan, bukan sebagai nilai
    /// yang dibaca.
    pub(crate) fn compile_jalur_target(&mut self, e: &Expr) -> CR<()> {
        // Disiplin span yang sama dengan `compile_expr`, dan ketiadaannya di sini
        // adalah sisa yang terlewat di fase SPAN-VM: galat di JALUR TARGET sebuah …
        let lama = self.chunk.masuk_span(e.span());
        let hasil = self.compile_jalur_target_dalam(e);
        self.chunk.pulih_span(lama);
        hasil
    }

    pub(crate) fn compile_jalur_target_dalam(&mut self, e: &Expr) -> CR<()> {
        match e {
            Expr::Ident(name, _) => {
                if let Some(slot) = self.resolve_local(name) {
                    self.chunk.emit_arg(OpCode::PinjamLokal, slot as u32);
                } else if let Some(ui) = self.resolve_upval(name) {
                    self.chunk.emit_arg(OpCode::PinjamUpval, ui as u32);
                } else if let Some((global, jenis)) = super::compiler::milik_unit(name) {
                    if jenis != Milik::Fungsi { self.catat_tingkat_atas(name); }
                    let ni = self.chunk.add_name(&global);
                    self.chunk.emit_arg(OpCode::PinjamGlobal, ni);
                } else if let Some(v) = crate::vm::bawaan::bawaan(name) {
                    // `len[0] = 1`: akarnya ada tapi bukan wadah; langkah berikutnya yang
                    // menolak, dengan kalimat tentang tipenya — sama dengan acuan.
                    let ci = self.chunk.add_constant(v);
                    self.chunk.emit_arg(OpCode::Const, ci);
                } else {
                    let pesan = self.pesan_sasaran_tugas(name);
                    self.emit_tolak_sasaran(&pesan);
                }
                Ok(())
            }
            Expr::Index { obj, idx, .. } => {
                self.compile_jalur_target(obj)?;
                self.dgn_temp(1, |c| c.compile_expr(idx))?;
                self.chunk.emit(OpCode::PinjamIndex);
                Ok(())
            }
            Expr::Field { obj, field, .. } => {
                self.compile_jalur_target(obj)?;
                let ni = self.chunk.add_name(field);
                self.chunk.emit_arg(OpCode::PinjamField, ni);
                Ok(())
            }
            // Jalur yang TIDAK berakar pada sebuah binding — `f(a)[0] = 1`, `f(a).x =
            // 1`, `(a + b).x = 1`.
            lain => Err(CompileError::new(crate::interpreter::pesan_bukan_sasaran(lain))),
        }
    }

    pub(crate) fn compile_assign_target(&mut self, target: &Expr, op: &AssignOp, rhs_slot: usize) -> CR<()> {
        // Span dipasang untuk SELURUH penulisan, bukan cuma penelusurannya.
        let lama = self.chunk.masuk_span(target.span());
        let hasil = self.compile_assign_target_dalam(target, op, rhs_slot);
        self.chunk.pulih_span(lama);
        hasil
    }

    pub(crate) fn compile_assign_target_dalam(&mut self, target: &Expr, op: &AssignOp, rhs_slot: usize) -> CR<()> {
        match target {
            Expr::Ident(name, _) => {
                if name == "_" {
                    return Ok(());
                }
                // Yang boleh ditugasi: lokal, tangkapan, dan `var` tingkat atas unit INI. Sisanya
                // ditolak saat jalan — dulu `StoreGlobal` membuat global baru untuk nama apa pun,
                // jadi salah ketik satu huruf lolos dan `len = 5` menimpa builtin-nya.
                let var_global = match super::compiler::milik_unit(name) {
                    Some((g, Milik::Var)) => Some(g),
                    _ => None,
                };
                if self.resolve_local(name).is_none() && self.resolve_upval(name).is_none()
                    && var_global.is_none()
                {
                    let pesan = self.pesan_sasaran_tugas(name);
                    self.emit_tolak_sasaran(&pesan);
                    return Ok(());
                }
                if matches!(op, AssignOp::Eq) {
                    self.chunk.emit_arg(OpCode::LoadLocal, rhs_slot as u32);
                } else if let (AssignOp::PlusEq, Some((coba, simpan, idx))) = (op, self.sasaran_sel(name)) {
                    // `A += x` atas tangkapan atau `var` modul: sambung di tempat kalau bisa.
                    self.compile_expr(target)?;
                    self.chunk.emit_arg(OpCode::LoadLocal, rhs_slot as u32);
                    self.emit_tumbuh_sel(coba, idx, simpan, idx, |c| { c.chunk.emit(OpCode::Add); });
                    return Ok(());
                } else {
                    self.compile_expr(target)?;
                    self.chunk.emit_arg(OpCode::LoadLocal, rhs_slot as u32);
                    self.emit_assign_op(op)?;
                }
                if let Some(slot) = self.resolve_local(name) {
                    self.chunk.emit_arg(OpCode::StoreLocal, slot as u32);
                } else if let Some(ui) = self.resolve_upval(name) {
                    // Menulis ke binding tangkapan menembus ke selnya, jadi
                    // perubahan dari dalam closure terlihat di scope asal —
                    // itu inti "menangkap binding, bukan nilai".
                    self.chunk.emit_arg(OpCode::StoreUpval, ui as u32);
                } else if let Some(g) = var_global {
                    let ni = self.chunk.add_name(&g);
                    self.chunk.emit_arg(OpCode::StoreGlobal, ni);
                }
                self.chunk.emit(OpCode::Pop);
            }
            Expr::Field { obj, field, .. } => {
                self.compile_jalur_target(obj)?;
                let obj_slot = self.slot_berikutnya();
                self.define_local("__assign_obj__");

                let mut lewat = None;
                if matches!(op, AssignOp::Eq) {
                    self.chunk.emit_arg(OpCode::LoadLocal, obj_slot as u32);
                    self.chunk.emit_arg(OpCode::LoadLocal, rhs_slot as u32);
                } else {
                    // `+=`: coba sambung di tempat dulu; slot nilainya diisi Nil supaya stack
                    // kedua cabang sama.
                    lewat = if matches!(op, AssignOp::PlusEq) {
                        self.chunk.emit_arg(OpCode::LoadLocal, obj_slot as u32);
                        self.chunk.emit_arg(OpCode::LoadLocal, rhs_slot as u32);
                        let ni = self.chunk.add_name(field);
                        self.chunk.emit_arg(OpCode::CobaTambahField, ni);
                        let biasa = self.chunk.emit_jump(OpCode::JumpFalse);
                        self.chunk.emit(OpCode::Nil);
                        let selesai = self.chunk.emit_jump(OpCode::Jump);
                        self.chunk.patch_jump(biasa);
                        Some(selesai)
                    } else { None };
                    self.chunk.emit_arg(OpCode::LoadLocal, obj_slot as u32);
                    let ni = self.chunk.add_name(field);
                    self.chunk.emit_arg(crate::vm::visibilitas::opcode_get(field), ni);
                    self.chunk.emit_arg(OpCode::LoadLocal, rhs_slot as u32);
                    self.emit_assign_op(op)?;
                    let value_slot = self.slot_berikutnya();
                    self.define_local("__assign_value__");
                    self.chunk.emit_arg(OpCode::LoadLocal, obj_slot as u32);
                    self.chunk.emit_arg(OpCode::LoadLocal, value_slot as u32);
                }
                let ni = self.chunk.add_name(field);
                self.chunk.emit_arg(crate::vm::visibilitas::opcode_set(field), ni);
                if let Some(j) = lewat { self.chunk.patch_jump(j); }
            }
            Expr::Index { obj, idx, .. } => {
                self.compile_jalur_target(obj)?;
                let obj_slot = self.slot_berikutnya();
                self.define_local("__assign_obj__");
                self.compile_expr(idx)?;
                let idx_slot = self.slot_berikutnya();
                self.define_local("__assign_idx__");

                let mut lewat = None;
                if matches!(op, AssignOp::Eq) {
                    self.chunk.emit_arg(OpCode::LoadLocal, obj_slot as u32);
                    self.chunk.emit_arg(OpCode::LoadLocal, idx_slot as u32);
                    self.chunk.emit_arg(OpCode::LoadLocal, rhs_slot as u32);
                } else {
                    if matches!(op, AssignOp::PlusEq) {
                        self.chunk.emit_arg(OpCode::LoadLocal, obj_slot as u32);
                        self.chunk.emit_arg(OpCode::LoadLocal, idx_slot as u32);
                        self.chunk.emit_arg(OpCode::LoadLocal, rhs_slot as u32);
                        self.chunk.emit(OpCode::CobaTambahIndex);
                        let biasa = self.chunk.emit_jump(OpCode::JumpFalse);
                        self.chunk.emit(OpCode::Nil);
                        lewat = Some(self.chunk.emit_jump(OpCode::Jump));
                        self.chunk.patch_jump(biasa);
                    }
                    self.chunk.emit_arg(OpCode::LoadLocal, obj_slot as u32);
                    self.chunk.emit_arg(OpCode::LoadLocal, idx_slot as u32);
                    self.chunk.emit(OpCode::GetIndex);
                    self.chunk.emit_arg(OpCode::LoadLocal, rhs_slot as u32);
                    self.emit_assign_op(op)?;
                    let value_slot = self.slot_berikutnya();
                    self.define_local("__assign_value__");
                    self.chunk.emit_arg(OpCode::LoadLocal, obj_slot as u32);
                    self.chunk.emit_arg(OpCode::LoadLocal, idx_slot as u32);
                    self.chunk.emit_arg(OpCode::LoadLocal, value_slot as u32);
                }
                self.chunk.emit(OpCode::SetIndex);
                if let Some(j) = lewat { self.chunk.patch_jump(j); }
            }
            lain => return Err(CompileError::new(crate::interpreter::pesan_bukan_sasaran(lain))),
        }
        Ok(())
    }

    pub(crate) fn emit_assign_op(&mut self, op: &AssignOp) -> CR<()> {
        let opcode = match op {
            AssignOp::Eq => return Ok(()),
            AssignOp::PlusEq => OpCode::Add,
            AssignOp::PowEq  => OpCode::Pow,
            AssignOp::MinusEq => OpCode::Sub,
            AssignOp::StarEq => OpCode::Mul,
            AssignOp::SlashEq => OpCode::Div,
            AssignOp::PercentEq => OpCode::Mod,
            AssignOp::AndEq => OpCode::BitAnd,
            AssignOp::OrEq => OpCode::BitOr,
            AssignOp::CaretEq => OpCode::BitXor,
            AssignOp::ShlEq => OpCode::Shl,
            AssignOp::ShrEq => OpCode::Shr,
        };
        self.chunk.emit(opcode);
        Ok(())
    }

}
