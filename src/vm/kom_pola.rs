// src/vm/kom_pola.rs — pencocokan pola

use super::opcode::OpCode;
use super::value::VmValue;
use crate::parser::*;
use super::compiler::*;

// ─── Pattern matching helper ──────────────────────────────────────────────

// PatResult::Never sengaja dipertahankan dan tetap ditangani di semua
// pemanggil (lihat compile_match) walau tidak ada varian Pattern saat ini …
#[allow(dead_code)]
pub(crate) enum PatResult {
    Always(usize),  // Unconditional jump index (wildcard/ident)
    Cond(usize),    // Conditional jump index (literal/range)
    Never,          // Pattern will never match
}

#[derive(Clone)]
pub(crate) enum PatternSource {
    Local(usize),
    /// Elemen ke-`i`. Boleh NEGATIF — dihitung dari belakang, sama seperti
    /// `a[-1]` di bahasa. Itu yang bikin pola `[a, ..., z]` bisa menguji
    /// elemen terakhir tanpa tahu panjangnya saat kompilasi.
    TupleElem(Box<PatternSource>, i128),
    Field(Box<PatternSource>, String),
    /// Payload sebuah varian enum — dibaca dari labelnya, bukan dari kunci peta.
    Payload(Box<PatternSource>),
    /// `...sisa` di pola array: potongan sesudah `depan` elemen, sebelum `belakang` terakhir.
    Sisa(Box<PatternSource>, i128, i128),
}

/// Apakah pola ini memuat `|` yang mengikat nama, di kedalaman mana pun?
pub(crate) fn ada_or_pengikat(p: &Pattern) -> bool {
    match p {
        Pattern::Or { pats, .. } => pats.iter().any(pattern_binds_name) || pats.iter().any(ada_or_pengikat),
        Pattern::Binding { inner, .. } | Pattern::Ref { inner, .. } => ada_or_pengikat(inner),
        Pattern::Tuple { elems, .. } => elems.iter().any(ada_or_pengikat),
        Pattern::Struct { fields, .. } => fields.iter().any(|f| ada_or_pengikat(&f.pat)),
        Pattern::Enum { inner: Some(i), .. } => ada_or_pengikat(i),
        Pattern::Array { before, after, .. } => before.iter().chain(after).any(ada_or_pengikat),
        _ => false,
    }
}

/// Uraikan `|` (di kedalaman mana pun) jadi alternatif tanpa `|`, urut sumber.
pub(crate) fn uraikan_or(p: &Pattern) -> Vec<Pattern> {
    fn hasil_kali(bagian: Vec<Vec<Pattern>>) -> Vec<Vec<Pattern>> {
        let mut acc: Vec<Vec<Pattern>> = vec![Vec::new()];
        for pilihan in bagian {
            let mut baru = Vec::new();
            for a in &acc {
                for x in &pilihan {
                    let mut v = a.clone();
                    v.push(x.clone());
                    baru.push(v);
                }
            }
            acc = baru;
        }
        acc
    }
    match p {
        Pattern::Or { pats, .. } => pats.iter().flat_map(uraikan_or).collect(),
        Pattern::Binding { name, inner, span } => uraikan_or(inner).into_iter()
            .map(|i| Pattern::Binding { name: name.clone(), inner: Box::new(i), span: *span }).collect(),
        Pattern::Ref { inner, is_mut, span } => uraikan_or(inner).into_iter()
            .map(|i| Pattern::Ref { inner: Box::new(i), is_mut: *is_mut, span: *span }).collect(),
        Pattern::Enum { ty, variant, inner: Some(i), span } => uraikan_or(i).into_iter()
            .map(|x| Pattern::Enum { ty: ty.clone(), variant: variant.clone(),
                                     inner: Some(Box::new(x)), span: *span }).collect(),
        Pattern::Tuple { elems, span } => hasil_kali(elems.iter().map(uraikan_or).collect())
            .into_iter().map(|e| Pattern::Tuple { elems: e, span: *span }).collect(),
        Pattern::Struct { ty, fields, span } => {
            let tiap = fields.iter().map(|f| uraikan_or(&f.pat)).collect();
            hasil_kali(tiap).into_iter().map(|ps| Pattern::Struct {
                ty: ty.clone(),
                fields: fields.iter().zip(ps).map(|(f, p)| FieldPat {
                    name: f.name.clone(), pat: p, span: f.span }).collect(),
                span: *span,
            }).collect()
        }
        Pattern::Array { before, rest, after, span } => {
            let n = before.len();
            let tiap = before.iter().chain(after).map(uraikan_or).collect();
            hasil_kali(tiap).into_iter().map(|mut v| {
                let blk = v.split_off(n);
                Pattern::Array { before: v, rest: rest.clone(), after: blk, span: *span }
            }).collect()
        }
        _ => vec![p.clone()],
    }
}

/// Pasangan (nama, sumber) yang diikat pola TANPA `|`, urut kemunculan.
pub(crate) fn ikatan_pola(p: &Pattern, src: &PatternSource, out: &mut Vec<(String, PatternSource)>) {
    let anak = |s: &PatternSource, i: i128| PatternSource::TupleElem(Box::new(s.clone()), i);
    match p {
        Pattern::Binding { name, inner, .. } => {
            if name != "_" { out.push((name.clone(), src.clone())); }
            ikatan_pola(inner, src, out);
        }
        Pattern::Ident { name, .. } if name == "_" || ident_varian_unit(name).is_some() => {}
        Pattern::Ident { name, .. } => out.push((name.clone(), src.clone())),
        Pattern::Tuple { elems, .. } => {
            for (i, e) in elems.iter().enumerate() { ikatan_pola(e, &anak(src, i as i128), out) }
        }
        Pattern::Enum { inner: Some(i), .. } =>
            ikatan_pola(i, &PatternSource::Payload(Box::new(src.clone())), out),
        Pattern::Struct { ty, fields, .. } => {
            let target = if matches!(ty, TypeExpr::Named(nt) if nt.package.is_some()) {
                PatternSource::Payload(Box::new(src.clone()))
            } else { src.clone() };
            for f in fields {
                ikatan_pola(&f.pat, &PatternSource::Field(Box::new(target.clone()), f.name.clone()), out);
            }
        }
        Pattern::Ref { inner, .. } => ikatan_pola(inner, src, out),
        Pattern::Array { before, rest, after, .. } => {
            for (i, e) in before.iter().enumerate() { ikatan_pola(e, &anak(src, i as i128), out) }
            if let Some(Some(n)) = rest {
                out.push((n.clone(), PatternSource::Sisa(Box::new(src.clone()),
                    before.len() as i128, after.len() as i128)));
            }
            for (j, e) in after.iter().enumerate() {
                ikatan_pola(e, &anak(src, -((after.len() - j) as i128)), out)
            }
        }
        _ => {}
    }
}

/// Nama bentuk sebuah pola, untuk pesan error `let` yang gagal cocok.
pub(crate) fn pattern_binds_name(pat: &Pattern) -> bool {
    match pat {
        // Selalu mengikat: namanya di kiri `@` itu sendiri.
        Pattern::Binding { .. } => true,
        Pattern::Ident { name, .. } if ident_varian_unit(name).is_some() => false,
        Pattern::Ident { name, .. } => name != "_",
        Pattern::Tuple { elems, .. } => elems.iter().any(pattern_binds_name),
        Pattern::Struct { fields, .. } => fields.iter().any(|field| pattern_binds_name(&field.pat)),
        Pattern::Enum { inner, .. } => inner.as_deref().map_or(false, pattern_binds_name),
        Pattern::Ref { inner, .. } => pattern_binds_name(inner),
        Pattern::Or { pats, .. } => pats.iter().any(pattern_binds_name),
        // Pola array belum didukung backend VM (lihat `bind_pattern` /
        // `compile_pattern_test` di bawah, yang menolaknya terang-terangan).
        Pattern::Array { before, rest, after, .. } =>
            before.iter().any(pattern_binds_name)
            || after.iter().any(pattern_binds_name)
            || matches!(rest, Some(Some(_))),
        Pattern::Literal { .. } | Pattern::Range { .. } | Pattern::Wildcard(_) => false,
    }
}

/// Identifier telanjang yang sebenarnya nama VARIAN ENUM TANPA PAYLOAD
/// (`None`, atau varian unit bikinan pemakai), bukan nama pengikat.
pub(crate) fn ident_varian_unit(name: &str) -> Option<String> {
    match enum_ctor_lookup(name) {
        Some((enum_name, 0)) => Some(enum_name),
        _ => None,
    }
}

pub(crate) fn pattern_is_irrefutable(pat: &Pattern) -> bool {
    match pat {
        // Setegas polanya: `n @ _` tidak pernah menolak, `n @ 1` menolak.
        Pattern::Binding { inner, .. } => pattern_is_irrefutable(inner),
        // Varian unit MENOLAK kalau nilainya bukan varian itu, jadi ia
        // tidak pernah irrefutable.
        Pattern::Ident { name, .. } if ident_varian_unit(name).is_some() => false,
        Pattern::Ident { .. } | Pattern::Wildcard(_) => true,
        Pattern::Ref { inner, .. } => pattern_is_irrefutable(inner),
        Pattern::Or { pats, .. } => pats.iter().any(pattern_is_irrefutable),
        _ => false,
    }
}

impl Compiler {
    pub(crate) fn bind_pattern(&mut self, pat: &Pattern, sc_slot: usize, bindings: &mut Vec<String>) -> CR<()> {
        match pat {
            // Nilai utuhnya ke `name`, lalu pola di kanan `@` mengikat
            // apa pun yang ia ikat.
            Pattern::Binding { name, inner, .. } => {
                if name != "_" {
                    self.chunk.emit_arg(OpCode::LoadLocal, sc_slot as u32);
                    self.define_local(name);
                    bindings.push(name.clone());
                }
                self.bind_pattern(inner, sc_slot, bindings)
            }
            // Nama varian unit BUKAN pengikat.
            Pattern::Ident { name, .. } if ident_varian_unit(name).is_some() => Ok(()),
            Pattern::Ident { name, .. } if name != "_" => {
                self.chunk.emit_arg(OpCode::LoadLocal, sc_slot as u32);
                self.define_local(name);
                bindings.push(name.clone());
                Ok(())
            }
            Pattern::Ident { .. } => Ok(()),
            Pattern::Wildcard(_) => Ok(()),
            Pattern::Tuple { elems, .. } => {
                // Simplified: bind each element (assuming tuple is array)
                for (i, p) in elems.iter().enumerate() {
                    self.chunk.emit_arg(OpCode::LoadLocal, sc_slot as u32);
                    let ci = self.chunk.add_constant(VmValue::Int(i as i128));
                    self.chunk.emit_arg(OpCode::Const, ci);
                    self.chunk.emit(OpCode::GetIndex);
                    // scrutinee[i] sekarang di top-of-stack.
                    let elem_slot = self.slot_berikutnya();
                    self.define_local("__tuple_elem__");
                    bindings.push("__tuple_elem__".to_string());
                    self.bind_pattern(p, elem_slot, bindings)?;
                }
                Ok(())
            }
            Pattern::Enum { inner, .. } => {
                if let Some(inner_pat) = inner {
                    self.chunk.emit_arg(OpCode::LoadLocal, sc_slot as u32);
                    self.chunk.emit(OpCode::AmbilPayload);
                    // Sama seperti Tuple di atas: daftarkan payload sebagai local
                    // sementara, baru bind inner pattern dari situ.
                    let payload_slot = self.slot_berikutnya();
                    self.define_local("__enum_payload__");
                    bindings.push("__enum_payload__".to_string());
                    self.bind_pattern(inner_pat, payload_slot, bindings)?;
                }
                Ok(())
            }
            Pattern::Struct { ty, fields, .. } => {
                let target_slot = if matches!(ty, TypeExpr::Named(nt) if nt.package.is_some()) {
                    self.chunk.emit_arg(OpCode::LoadLocal, sc_slot as u32);
                    self.chunk.emit(OpCode::AmbilPayload);
                    let slot = self.slot_berikutnya();
                    self.define_local("__struct_payload__");
                    bindings.push("__struct_payload__".to_string());
                    slot
                } else {
                    sc_slot
                };
                for field in fields {
                    self.chunk.emit_arg(OpCode::LoadLocal, target_slot as u32);
                    let field_name = self.chunk.add_name(&field.name);
                    self.chunk.emit_arg(OpCode::GetField, field_name);
                    let field_slot = self.slot_berikutnya();
                    self.define_local("__struct_field__");
                    bindings.push("__struct_field__".to_string());
                    self.bind_pattern(&field.pat, field_slot, bindings)?;
                }
                Ok(())
            }
            Pattern::Ref { inner, .. } => self.bind_pattern(inner, sc_slot, bindings),
            Pattern::Or { pats, .. } => {
                if pats.iter().any(pattern_binds_name) {
                    return Err(CompileError::new(
                        "or-pattern yang mengikat variabel belum didukung Bytecode VM",
                    ));
                }
                Ok(())
            }
            Pattern::Literal { .. } | Pattern::Range { .. } => Ok(()),
            // Pola array.
            Pattern::Array { before, rest, after, .. } => {
                for (i, p) in before.iter().enumerate() {
                    self.chunk.emit_arg(OpCode::LoadLocal, sc_slot as u32);
                    let ci = self.chunk.add_constant(VmValue::Int(i as i128));
                    self.chunk.emit_arg(OpCode::Const, ci);
                    self.chunk.emit(OpCode::GetIndex);
                    let elem_slot = self.slot_berikutnya();
                    self.define_local("__arr_elem__");
                    bindings.push("__arr_elem__".to_string());
                    self.bind_pattern(p, elem_slot, bindings)?;
                }
                for (j, p) in after.iter().enumerate() {
                    self.chunk.emit_arg(OpCode::LoadLocal, sc_slot as u32);
                    let dari_belakang = -((after.len() - j) as i128);
                    let ci = self.chunk.add_constant(VmValue::Int(dari_belakang));
                    self.chunk.emit_arg(OpCode::Const, ci);
                    self.chunk.emit(OpCode::GetIndex);
                    let elem_slot = self.slot_berikutnya();
                    self.define_local("__arr_elem__");
                    bindings.push("__arr_elem__".to_string());
                    self.bind_pattern(p, elem_slot, bindings)?;
                }
                // `...nama` mengikat SISANYA sebagai array baru.
                if let Some(Some(nama)) = rest {
                    self.chunk.emit_arg(OpCode::LoadLocal, sc_slot as u32);
                    let lo = self.chunk.add_constant(VmValue::Int(before.len() as i128));
                    self.chunk.emit_arg(OpCode::Const, lo);
                    if after.is_empty() {
                        self.chunk.emit(OpCode::Nil);
                    } else {
                        let hi = self.chunk.add_constant(VmValue::Int(-(after.len() as i128)));
                        self.chunk.emit_arg(OpCode::Const, hi);
                    }
                    self.chunk.emit(OpCode::Slice);
                    self.define_local(nama);
                    bindings.push(nama.clone());
                }
                Ok(())
            }
        }
    }


    
    pub(crate) fn compile_match(&mut self, scrutinee: &Expr, arms: &[MatchArm]) -> CR<()> {
        // Evaluasi scrutinee
        self.compile_expr(scrutinee)?;
        let sc_slot = self.slot_berikutnya();
        self.define_local("__match_scrutinee__");

        let mut end_jumps: Vec<usize> = Vec::new();

        for arm in arms {
            let mut arm_hit_jumps: Vec<usize> = Vec::new();
            let mut always_match = false;

            // Alternatif yang MENGIKAT: tiap alternatif diuji lalu menaruh nilai nama-namanya
            // (urutan alternatif pertama) di stack; parser sudah menjamin namanya sama.
            let alternatif_pengikat = arm.patterns.iter().any(ada_or_pengikat)
                || (arm.patterns.len() > 1 && arm.patterns.iter().any(pattern_binds_name));
            let (skip_arm_j, bindings) = if alternatif_pengikat {
                let sumber = PatternSource::Local(sc_slot);
                let alts: Vec<Pattern> = arm.patterns.iter().flat_map(uraikan_or).collect();
                let mut awal = Vec::new();
                ikatan_pola(&alts[0], &sumber, &mut awal);
                let nama: Vec<String> = awal.into_iter().map(|(n, _)| n).collect();
                let mut ke_ikat = Vec::new();
                for alt in &alts {
                    let mut failures = Vec::new();
                    self.compile_pattern_conditions(alt, &sumber, &mut failures)?;
                    let mut ik = Vec::new();
                    ikatan_pola(alt, &sumber, &mut ik);
                    for n in &nama {
                        match ik.iter().find(|(m, _)| m == n) {
                            Some((_, s)) => self.emit_pattern_source(s),
                            None => { self.chunk.emit(OpCode::Nil); }
                        }
                    }
                    ke_ikat.push(self.chunk.emit_jump(OpCode::Jump));
                    for f in failures { self.chunk.patch_jump(f); }
                }
                let skip = self.chunk.emit_jump(OpCode::Jump);
                for j in ke_ikat { self.chunk.patch_jump(j); }
                for n in &nama { self.define_local(n); }
                (Some(skip), nama)
            } else {

            for pat in &arm.patterns {
                match self.compile_pat_check(pat, sc_slot)? {
                    PatResult::Always(j) => {
                        arm_hit_jumps.clear();
                        arm_hit_jumps.push(j);
                        always_match = true;
                        break;
                    }
                    PatResult::Cond(j) => {
                        arm_hit_jumps.push(j);
                    }
                    PatResult::Never => {}
                }
            }

            if arm_hit_jumps.is_empty() {
                // Semua pattern arm ini Never (tidak mungkin cocok) → jangan compile body sama sekali.
                // `continue` dari dalam blok `else` di atas: lengan ini dilewati utuh.
                continue;
            }

            // Kalau bukan always_match, arm_hit_jumps berisi JumpTrue kondisional: kalau
            // semua gagal (fall-through, tidak ada satupun pattern yang cocok), kita …
            let skip_arm_j = if !always_match {
                Some(self.chunk.emit_jump(OpCode::Jump))
            } else {
                None
            };

            for j in &arm_hit_jumps { self.chunk.patch_jump(*j); }

            // Binding — HARUS sebelum guard, karena guard expression umumnya
            // mereferensikan nama yang di-bind oleh pattern (mis. `n if n > 5`).
            let mut bindings: Vec<String> = Vec::new();
            for pat in &arm.patterns {
                self.bind_pattern(pat, sc_slot, &mut bindings)?;
            }
            (skip_arm_j, bindings)
            };

            // Guard
            let guard_fail_jump = if let Some(g) = &arm.guard {
                self.compile_expr(g)?;
                Some(self.chunk.emit_jump(OpCode::JumpFalse))
            } else { None };

            // Body — match dipakai sebagai expression, jadi SETIAP arm harus
            // meninggalkan tepat satu nilai di stack (baik Block maupun Expr).
            self.begin_scope();
            match &arm.body {
                MatchBody::Block(blk) => self.compile_block_as_value(blk)?,
                MatchBody::Expr(e) => {
                    self.compile_expr(e)?;
                }
            }
            self.end_scope_keep_top();

            // Unbind — hasil body ada di top-of-stack, DI ATAS binding-binding pattern.
            if !bindings.is_empty() {
                self.chunk.emit_arg(OpCode::SlideDown, bindings.len() as u32);
            }
            for _ in &bindings {
                self.locals.pop();
            }

            end_jumps.push(self.chunk.emit_jump(OpCode::Jump));

            if let Some(gj) = guard_fail_jump {
                self.chunk.patch_jump(gj);
                // Di jalur ini body BELUM dieksekusi (guard gagal sebelum body),
                // jadi tidak ada hasil di atas binding — Pop biasa aman di sini.
                for _ in &bindings {
                    self.chunk.emit(OpCode::Pop);
                }
            }

            if let Some(sj) = skip_arm_j {
                self.chunk.patch_jump(sj);
            }
        }

        // Tidak ada arm yang cocok: gagal sambil menyebut nilainya (scrutinee masih di puncak
        // stack) — atau, untuk hasil penurunan `??`, tipe sisi kirinya.
        let koalesensi = crate::parser::lengan_dari_koalesensi(arms);
        self.chunk.emit_arg(OpCode::GagalCocok, koalesensi as u32);

        for ej in end_jumps { self.chunk.patch_jump(ej); }

        // Buang scrutinee dari bawah top-of-stack, tapi JAGA hasil arm yang barusan
        // dieksekusi (ada di top-of-stack). Pop biasa di sini akan membuang hasil
        // arm dan menyisakan nilai scrutinee sebagai "hasil" — salah.
        self.chunk.emit_arg(OpCode::SlideDown, 1);
        self.locals.pop();

        Ok(())
    }


    pub(crate) fn emit_pattern_source(&mut self, source: &PatternSource) {
        match source {
            PatternSource::Local(slot) => {
                self.chunk.emit_arg(OpCode::LoadLocal, *slot as u32);
            }
            PatternSource::TupleElem(parent, index) => {
                self.emit_pattern_source(parent);
                let ci = self.chunk.add_constant(VmValue::Int(*index));
                self.chunk.emit_arg(OpCode::Const, ci);
                self.chunk.emit(OpCode::GetIndex);
            }
            PatternSource::Field(parent, field) => {
                self.emit_pattern_source(parent);
                let ni = self.chunk.add_name(field);
                self.chunk.emit_arg(crate::vm::visibilitas::opcode_get(field), ni);
            }
            PatternSource::Payload(parent) => {
                self.emit_pattern_source(parent);
                self.chunk.emit(OpCode::AmbilPayload);
            }
            PatternSource::Sisa(parent, depan, belakang) => {
                self.emit_pattern_source(parent);
                let lo = self.chunk.add_constant(VmValue::Int(*depan));
                self.chunk.emit_arg(OpCode::Const, lo);
                if *belakang == 0 {
                    self.chunk.emit(OpCode::Nil);
                } else {
                    let hi = self.chunk.add_constant(VmValue::Int(-*belakang));
                    self.chunk.emit_arg(OpCode::Const, hi);
                }
                self.chunk.emit(OpCode::Slice);
            }
        }
    }

    /// Uji label enum. `tipe` boleh tidak diketahui; payload bisa dituntut ada.
    pub(crate) fn emit_cek_enum(
        &mut self,
        source: &PatternSource,
        tipe: Option<&str>,
        varian: &str,
        wajib_payload: bool,
        subpola: Option<usize>,
        failures: &mut Vec<usize>,
    ) {
        self.emit_pattern_source(source);
        let desc = VmValue::tuple(vec![
            tipe.map_or(VmValue::Nil, |t| VmValue::str(t)),
            VmValue::str(varian),
            VmValue::Bool(wajib_payload),
            subpola.map_or(VmValue::Nil, |n| VmValue::Int(n as i128)),
        ]);
        let ci = self.chunk.add_constant(desc);
        self.chunk.emit_arg(OpCode::CekEnum, ci);
        failures.push(self.chunk.emit_jump(OpCode::JumpFalse));
    }

    /// Uji label struct `nama` (persis, tanpa warisan) plus daftar field yang
    /// disebut polanya. Field yang tidak ada diperiksa saat JALAN, supaya waktu
    /// gagalnya sama dengan interpreter.
    pub(crate) fn emit_cek_struct(&mut self, source: &PatternSource, nama: &str,
                                  field: &[String], failures: &mut Vec<usize>) {
        self.emit_pattern_source(source);
        let desc = VmValue::tuple(vec![
            VmValue::str(nama),
            VmValue::array(field.iter().map(|f| VmValue::str(f.as_str())).collect()),
        ]);
        let ci = self.chunk.add_constant(desc);
        self.chunk.emit_arg(OpCode::CekStruct, ci);
        failures.push(self.chunk.emit_jump(OpCode::JumpFalse));
    }

    pub(crate) fn emit_pattern_type_check(
        &mut self,
        source: &PatternSource,
        expected: &str,
        failures: &mut Vec<usize>,
    ) {
        self.emit_pattern_source(source);
        self.chunk.emit(OpCode::Typeof);
        let ci = self.chunk.add_constant(VmValue::str(expected));
        self.chunk.emit_arg(OpCode::Const, ci);
        self.chunk.emit(OpCode::Eq);
        failures.push(self.chunk.emit_jump(OpCode::JumpFalse));
    }

    pub(crate) fn emit_pattern_field_check(
        &mut self,
        source: &PatternSource,
        field: &str,
        failures: &mut Vec<usize>,
    ) {
        self.emit_pattern_source(source);
        let ni = self.chunk.add_name(field);
        self.chunk.emit_arg(OpCode::HasField, ni);
        failures.push(self.chunk.emit_jump(OpCode::JumpFalse));
    }

    pub(crate) fn compile_pattern_lit(&mut self, lit: &LitKind) -> CR<()> {
        match lit {
            // Jangkauannya sudah dijaga parser (literal positif sampai `i128::MAX`, yang
            // negatif disimpan sebagai pola bit-nya), jadi seluruh 128 bit sah di sini.
            // Dulu dibatasi 64 bit — sisa VM lama — dan `--vm` menolak pola yang jalan di acuan.
            LitKind::Int(n) => {
                let ci = self.chunk.add_constant(VmValue::Int(*n as i128));
                self.chunk.emit_arg(OpCode::Const, ci);
                Ok(())
            }
            _ => self.compile_lit(lit),
        }
    }

    pub(crate) fn compile_pattern_conditions(
        &mut self,
        pat: &Pattern,
        source: &PatternSource,
        failures: &mut Vec<usize>,
    ) -> CR<()> {
        match pat {
            // Pengikatnya tidak menguji apa pun; yang menguji adalah pola
            // di kanan `@`.
            Pattern::Binding { inner, .. } => self.compile_pattern_conditions(inner, source, failures),
            Pattern::Ident { name, .. } if ident_varian_unit(name).is_some() => {
                let enum_name = ident_varian_unit(name).unwrap();
                self.emit_cek_enum(source, Some(&enum_name), name, false, Some(0), failures);
                Ok(())
            }
            // Varian milik modul yang ditulis telanjang: tanpa penolakan ini
            // ia MENGIKAT dan lengan pertama menelan seluruh `match`.
            Pattern::Ident { name, .. }
                if name != "_" && crate::vm::compiler::enum_ctor_unit_akhiran(name).is_some() =>
            {
                // Gagalnya SAAT JALAN, bukan saat kompilasi: kalau tidak,
                // keluaran sebelum `match` hilang di satu mesin saja.
                let (tipe, _) = crate::vm::compiler::enum_ctor_unit_akhiran(name).unwrap();
                let pesan = crate::vm::compiler::bukan_nilai_lookup(name).map_or_else(
                    || crate::interpreter::pesan_varian_modul_pola(name, &tipe, "yang diimpor"),
                    |dasar| format!("{} (di posisi pola nama telanjang malah MENGIKAT dan cocok \
                                     dengan apa pun, jadi lengan ini akan menelan semuanya)", dasar),
                );
                let ci = self.chunk.add_constant(VmValue::str(pesan));
                self.chunk.emit_arg(OpCode::Const, ci);
                self.chunk.emit(OpCode::Panic);
                Ok(())
            }
            Pattern::Wildcard(_) | Pattern::Ident { .. } => Ok(()),
            Pattern::Literal { lit, .. } => {
                self.emit_pattern_source(source);
                self.compile_pattern_lit(lit)?;
                self.chunk.emit(OpCode::Eq);
                failures.push(self.chunk.emit_jump(OpCode::JumpFalse));
                Ok(())
            }
            Pattern::Range { lo, hi, inclusive, .. } => {
                // Jenisnya diuji DULU: nilai yang tidak sebanding dengan batasnya tidak cocok,
                // seperti acuan — dulu `Ge` atasnya menggagalkan seluruh program.
                if crate::interpreter::eval_stmt::pola_rentang_rune(lo, hi) {
                    self.emit_pattern_type_check(source, "rune", failures);
                } else {
                    self.emit_pattern_source(source);
                    self.chunk.emit(OpCode::Typeof);
                    let ci = self.chunk.add_constant(VmValue::str("int"));
                    self.chunk.emit_arg(OpCode::Const, ci);
                    self.chunk.emit(OpCode::Eq);
                    let bukan_int = self.chunk.emit_jump(OpCode::JumpFalse);
                    let angka = self.chunk.emit_jump(OpCode::Jump);
                    self.chunk.patch_jump(bukan_int);
                    self.emit_pattern_type_check(source, "float", failures);
                    self.chunk.patch_jump(angka);
                }
                if let Some(lo_e) = lo.as_deref() {
                    let Pattern::Literal { lit, .. } = lo_e else {
                        return Err(CompileError::new("batas bawah range pattern harus literal"));
                    };
                    self.emit_pattern_source(source);
                    self.compile_pattern_lit(lit)?;
                    self.chunk.emit(OpCode::Ge);
                    failures.push(self.chunk.emit_jump(OpCode::JumpFalse));
                }
                if let Some(hi_e) = hi.as_deref() {
                    let Pattern::Literal { lit, .. } = hi_e else {
                        return Err(CompileError::new("batas atas range pattern harus literal"));
                    };
                    self.emit_pattern_source(source);
                    self.compile_pattern_lit(lit)?;
                    self.chunk.emit(if *inclusive { OpCode::Le } else { OpCode::Lt });
                    failures.push(self.chunk.emit_jump(OpCode::JumpFalse));
                }
                Ok(())
            }
            Pattern::Tuple { elems: pats, .. } => {
                self.emit_pattern_type_check(source, "tuple", failures);
                self.emit_pattern_source(source);
                self.chunk.emit(OpCode::Len);
                let n_ci = self.chunk.add_constant(VmValue::Int(pats.len() as i128));
                self.chunk.emit_arg(OpCode::Const, n_ci);
                self.chunk.emit(OpCode::Eq);
                failures.push(self.chunk.emit_jump(OpCode::JumpFalse));
                for (index, child) in pats.iter().enumerate() {
                    let child_source = PatternSource::TupleElem(Box::new(source.clone()), index as i128);
                    self.compile_pattern_conditions(child, &child_source, failures)?;
                }
                Ok(())
            }
            Pattern::Enum { ty, variant, inner, .. } => {
                let enum_name = match ty {
                    TypeExpr::Named(nt) => nt.package.clone().or_else(|| {
                        enum_ctor_lookup(variant).map(|(name, _)| name)
                    }),
                    _ => None,
                };
                // Jumlah sub-pola DIBAWA ke runtime: interpreter baru bisa
                // memeriksanya saat lengannya dicocokkan, dan satu kesalahan
                // tidak boleh gagal di dua WAKTU yang berbeda.
                let dapat = match inner {
                    None => 0,
                    Some(p) => match &**p {
                        Pattern::Tuple { elems, .. } => elems.len(),
                        _ => 1,
                    },
                };
                self.emit_cek_enum(source, enum_name.as_deref(), variant, inner.is_some(),
                                   Some(dapat), failures);
                if let Some(inner) = inner {
                    let payload = PatternSource::Payload(Box::new(source.clone()));
                    self.compile_pattern_conditions(inner, &payload, failures)?;
                }
                Ok(())
            }
            Pattern::Struct { ty, fields, .. } => {
                let (target, expected_name) = match ty {
                    // `E::V { .. }`: payload varian enum yang berupa struct bernama `V`.
                    TypeExpr::Named(nt) if nt.package.is_some() => {
                        let enum_name = nt.package.as_deref().unwrap_or_default();
                        self.emit_cek_enum(source, Some(enum_name), &nt.name, true, None, failures);
                        (PatternSource::Payload(Box::new(source.clone())), nt.name.as_str())
                    }
                    TypeExpr::Named(nt) => (source.clone(), nt.name.as_str()),
                    _ => {
                        return Err(CompileError::new(
                            "struct pattern membutuhkan nama tipe",
                        ));
                    }
                };
                let nama_field: Vec<String> = fields.iter().map(|f| f.name.clone()).collect();
                self.emit_cek_struct(&target, expected_name, &nama_field, failures);
                for field in fields {
                    self.emit_pattern_field_check(&target, &field.name, failures);
                    let field_source = PatternSource::Field(
                        Box::new(target.clone()),
                        field.name.clone(),
                    );
                    self.compile_pattern_conditions(&field.pat, &field_source, failures)?;
                }
                Ok(())
            }
            Pattern::Ref { inner, .. } => {
                self.compile_pattern_conditions(inner, source, failures)
            }
            Pattern::Or { pats, .. } => {
                if pats.is_empty() {
                    failures.push(self.chunk.emit_jump(OpCode::Jump));
                    return Ok(());
                }
                let mut success_jumps = Vec::with_capacity(pats.len());
                for alternative in pats {
                    let mut alternative_failures = Vec::new();
                    self.compile_pattern_conditions(
                        alternative,
                        source,
                        &mut alternative_failures,
                    )?;
                    success_jumps.push(self.chunk.emit_jump(OpCode::Jump));
                    for failure in alternative_failures {
                        self.chunk.patch_jump(failure);
                    }
                }
                failures.push(self.chunk.emit_jump(OpCode::Jump));
                for success in success_jumps {
                    self.chunk.patch_jump(success);
                }
                Ok(())
            }
            // Pola array: `[a, b]`, `[kepala, ...ekor]`, `[a, ..., z]`, `[]`.
            Pattern::Array { before, rest, after, .. } => {
                self.emit_pattern_type_check(source, "array", failures);

                self.emit_pattern_source(source);
                self.chunk.emit(OpCode::Len);
                let minimal = (before.len() + after.len()) as i128;
                let ci = self.chunk.add_constant(VmValue::Int(minimal));
                self.chunk.emit_arg(OpCode::Const, ci);
                // Tanpa `...` panjangnya harus PERSIS; dengan `...` cukup
                // cukup panjang untuk menampung bagian depan dan belakang.
                if rest.is_some() {
                    self.chunk.emit(OpCode::Ge);
                } else {
                    self.chunk.emit(OpCode::Eq);
                }
                failures.push(self.chunk.emit_jump(OpCode::JumpFalse));

                for (i, p) in before.iter().enumerate() {
                    let elem = PatternSource::TupleElem(Box::new(source.clone()), i as i128);
                    self.compile_pattern_conditions(p, &elem, failures)?;
                }
                for (j, p) in after.iter().enumerate() {
                    let dari_belakang = -((after.len() - j) as i128);
                    let elem = PatternSource::TupleElem(Box::new(source.clone()), dari_belakang);
                    self.compile_pattern_conditions(p, &elem, failures)?;
                }
                Ok(())
            }
            // Ditolak terang-terangan (lihat catatan di `bind_pattern`).
            Pattern::Array { .. } => Err(CompileError::new(
                "pola array (`[a, b]`, `[kepala, ...ekor]`) belum didukung backend --vm — \
                 jalankan tanpa --vm (interpreter) untuk memakainya",
            )),
        }
    }

    /// Emit satu pattern check. Jalur sukses melompat ke body arm, sedangkan
    /// semua kondisi gagal jatuh ke pattern/arm berikutnya dengan stack seimbang.
    pub(crate) fn compile_pat_check(&mut self, pat: &Pattern, sc_slot: usize) -> CR<PatResult> {
        let source = PatternSource::Local(sc_slot);
        let mut failures = Vec::new();
        self.compile_pattern_conditions(pat, &source, &mut failures)?;
        let hit = self.chunk.emit_jump(OpCode::Jump);
        for failure in failures {
            self.chunk.patch_jump(failure);
        }
        if pattern_is_irrefutable(pat) {
            Ok(PatResult::Always(hit))
        } else {
            Ok(PatResult::Cond(hit))
        }
    }
}

