// src/llvm/codegen_stmt.rs — penurunan pernyataan dan cast

#![allow(dead_code)]

use std::collections::HashMap;
use std::rc::Rc;

use crate::parser::*;

use super::codegen::*;
use super::types::*;

impl Codegen {
    // ── statements ───────────────────────────────────────────────────────

    /// Pembungkus statement — pasangan dari `codegen_expr`.
    pub fn codegen_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        let lama = self.masuk_span(stmt.span());
        let hasil = self.codegen_stmt_dalam(stmt)
            .map_err(|e| crate::llvm::galat::beri_span_bila_kosong(e, stmt.span()));
        self.pulih_span(lama);
        hasil
    }

    pub(crate) fn codegen_stmt_dalam(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::Let(l) => self.codegen_let(l),
            Stmt::Var(v) => self.codegen_var(v),
            Stmt::Const(_) => Ok(()), // module-level consts folded at use; local consts: treat as let
            // An `if` in statement position keeps the original statement lowering: no
            // result slot and no cross-branch type unification, so `if c { 1 } else { …
            Stmt::Expr(e) => self.codegen_expr_discarding(e),
            Stmt::Assign(a) => {
                let hasil = self.codegen_assign(a);
                // Penanda pemindahan argumen tidak boleh bertahan melewati pernyataannya.
                self.arg_pindah = None;
                hasil
            }
            Stmt::ShortDecl(s) => self.codegen_short_decl(s),
            Stmt::If(i) => self.codegen_if(i),
            Stmt::While(w) => self.codegen_while(w),
            Stmt::Loop(l) => self.codegen_loop(l),
            Stmt::For(f) => self.codegen_for(f),
            Stmt::Match(m) => { self.codegen_match_stmt(m)?; Ok(()) }
            Stmt::Return(r) => self.codegen_return(r),
            Stmt::Break(b) => self.codegen_break(b),
            Stmt::Continue(c) => self.codegen_continue(c),
            Stmt::Block(b) => { self.push_scope(); self.codegen_stmt_block(b)?; self.pop_scope(); Ok(()) }
            // Fungsi bersarang bernama: didukung interpreter, belum di backend LLVM.
            Stmt::Fn(_) => Ok(()),
            Stmt::Spawn(s) => self.codegen_spawn(s),
            Stmt::Select(s) => self.codegen_select(s),
            Stmt::Defer(d) => self.codegen_defer(d),
            Stmt::Error(_) => Err("statement tidak valid (parse error)".into()),
        }
    }

    pub fn codegen_stmt_block(&mut self, block: &Block) -> Result<(), String> {
        self.hoist_fn_bersarang(block);
        let r = (|cg: &mut Self| {
            for s in &block.stmts {
                cg.codegen_stmt(s)?;
                if cg.terminated { return Ok(()); }
            }
            if let Some(tail) = &block.tail {
                cg.codegen_expr_discarding(tail)?;
            }
            Ok(())
        })(self);
        self.fn_bersarang.pop();
        r
    }

    /// Daftarkan setiap `fn` di blok ini SEBELUM satu statement pun diturunkan.
    pub(crate) fn hoist_fn_bersarang(&mut self, block: &Block) {
        let mut lapis: HashMap<String, (String, Rc<FnDecl>)> = HashMap::new();
        for s in &block.stmts {
            if let Stmt::Fn(fd) = s {
                self.fn_bersarang_ke += 1;
                let unik = format!("{}$sarang{}", fd.name, self.fn_bersarang_ke);
                lapis.insert(fd.name.clone(), (unik, Rc::new(fd.clone())));
            }
        }
        self.fn_bersarang.push(lapis);
    }

    /// Cari sebuah fungsi bersarang yang terlihat dari sini.
    pub fn catat_sarang(&mut self, simbol: &str) {
        if self.sarang_fn.contains_key(simbol) { return; }
        let l = self.fn_bersarang.clone();
        self.sarang_fn.insert(simbol.to_string(), l);
    }

    pub fn cari_fn_bersarang(&self, nama: &str) -> Option<(String, Rc<FnDecl>)> {
        for lapis in self.fn_bersarang.iter().rev() {
            if let Some(v) = lapis.get(nama) { return Some(v.clone()); }
        }
        None
    }

    /// Lower an expression whose value is thrown away (an expression statement,
    /// or the tail of a block used purely for effect such as a loop or …
    pub(crate) fn codegen_expr_discarding(&mut self, e: &Expr) -> Result<(), String> {
        self.codegen_expr_discarding_tipe(e).map(|_| ())
    }

    /// Sama, dan menyebut tipe nilai yang dibuang — `None` untuk `if` di posisi
    /// pernyataan, yang memang tidak menyatukan tipe cabangnya.
    pub(crate) fn codegen_expr_discarding_tipe(&mut self, e: &Expr) -> Result<Option<JrgType>, String> {
        if let Expr::If(ifs) = e {
            return self.codegen_if(ifs).map(|_| None);
        }
        // Nama telanjang yang ternyata sebuah FUNGSI: tidak mengerjakan apa
        // pun. Dua mesin acuan menolaknya sejak Fase KURUNG; di sini tipenya
        // sudah diketahui saat kompilasi, jadi penolakannya di situ.
        let hasil = self.codegen_expr(e, None)?;
        if matches!(e, Expr::Ident(..) | Expr::PathExpr { .. })
            && matches!(hasil.ty, JrgType::Fn(..)) {
            let nama = match e {
                Expr::Ident(n, _) => n.clone(),
                Expr::PathExpr { segments, .. } => segments.join("::"),
                _ => String::new(),
            };
            return Err(crate::llvm::galat::tandai(e.span(),
                crate::interpreter::galat_index::fungsi_sebagai_pernyataan(&nama)));
        }
        Ok(Some(hasil.ty))
    }

    pub(crate) fn codegen_let(&mut self, l: &LetStmt) -> Result<(), String> {
        let expected = match &l.ty {
            Some(te) => Some(self.resolve_type_pub(te)?),
            // Tabel yang sama yang membawa tipe `var a = []` juga membawa
            // tipe `let d = dict::new()`. Sebuah `let` bisa jadi wadah kosong
            // yang diisi lewat `d["k"] = v` sesudahnya, persis seperti `var`.
            None => pattern_simple_name(&l.pattern)
                .and_then(|n| self.lokal_tumbuh.get(&n).cloned()),
        };
        // Nama yang sedang diikat dititipkan supaya closure di sisi kanan
        // tahu ia akan dipanggil dengan nama apa — itu yang membuat
        // `let byparam = |p| p` bisa membaca tipe `p` dari `byparam(5)`.
        let nama_lama = std::mem::replace(
            &mut self.nama_diikat, pattern_simple_name(&l.pattern));
        let v = self.codegen_expr(&l.init, expected.as_ref());
        self.nama_diikat = nama_lama;
        let v = v?;
        // Nama baru = pemilik baru. Lihat `ambil_milik`.
        let v = self.ambil_milik(&l.init, v)?;
        let final_ty = expected.unwrap_or(v.ty.clone());
        // Span dipersempit ke INISIALISASINYA sebelum koersi.
        let lama = self.masuk_span(l.init.span());
        let v = self.coerce(&v, &final_ty);
        self.pulih_span(lama);
        self.bind_pattern(&l.pattern, &v, &final_ty)
    }

    pub(crate) fn codegen_var(&mut self, v: &VarStmt) -> Result<(), String> {
        let expected = match &v.ty {
            Some(te) => Some(self.resolve_type_pub(te)?),
            // Lokal yang nanti ditumbuhkan diberi tipe `array<T>` sejak awal, supaya
            // panjangnya tidak ikut jadi bagian tipenya.
            None => self.lokal_tumbuh.get(&v.name).cloned(),
        };
        let val = match &v.init {
            Some(e) => {
                let nama_lama = std::mem::replace(&mut self.nama_diikat, Some(v.name.clone()));
                let hasil = self.codegen_expr(e, expected.as_ref());
                self.nama_diikat = nama_lama;
                let hasil = hasil?;
                self.ambil_milik(e, hasil)?
            }
            None => {
                // Tipe tanpa nilai nol yang jelas (struct, Option, enum, …) bernilai `nil` di
                // acuan; LLVM tidak punya `nil` untuknya dan dulu mengarang nilai.
                if let Some(te) = &v.ty {
                    if crate::parser::nilai_nol(te, v.span).is_none() {
                        return Err(super::galat::tandai(v.span, format!(
                            "`var {}` tanpa nilai awal bertipe yang tidak punya nilai nol — acuan \
                             mengisinya `nil`, yang belum bisa diwakili backend LLVM v1. Beri nilai \
                             awal, atau jalankan tanpa `--emit-llvm`", v.name)));
                    }
                }
                let t = expected.clone().unwrap_or(JrgType::I64);
                self.zero_value(&t)
            }
        };
        // `var x = nil` tanpa anotasi: yang nanti mengisinya bertipe lain, jadi tempatnya
        // nilai bertag (lapis B). Dulu tempatnya bertipe `nil` dan pengisian pertama ditolak.
        let nil_polos = v.ty.is_none() && expected.is_none() && val.ty == JrgType::Nil;
        let final_ty = if nil_polos { JrgType::Any } else { expected.unwrap_or(val.ty.clone()) };
        // Sama seperti `codegen_let`: tunjuk inisialisasinya, bukan seluruh
        // statement. `None` kalau tidak ada inisialisasi — di situ statement
        // memang satu-satunya tempat yang bisa ditunjuk.
        let lama = match &v.init {
            Some(e) => Some(self.masuk_span(e.span())),
            None => None,
        };
        let val = self.coerce(&val, &final_ty);
        if let Some(l) = lama { self.pulih_span(l); }
        let slot = self.alloca_local(&v.name, final_ty.clone());
        if final_ty != JrgType::Unit {
            self.emit(format!("store {} {}, ptr {}", final_ty.llvm_ty(), val.text, slot));
        }
        Ok(())
    }

    pub fn zero_value(&mut self, ty: &JrgType) -> Val {
        let text = match ty {
            JrgType::Bool => "0".to_string(),
            t if t.is_int() => "0".to_string(),
            JrgType::F32 | JrgType::F64 => "0x0000000000000000".to_string(),
            JrgType::Str => {
                return self.intern_string("");
            }
            _ => "zeroinitializer".to_string(),
        };
        Val { text, ty: ty.clone() }
    }

    pub fn bind_pattern(&mut self, pat: &Pattern, v: &Val, ty: &JrgType) -> Result<(), String> {
        match pat {
            // Nama berhuruf besar bisa varian unit (`let None = …`): ia MENGUJI, bukan mengikat.
            Pattern::Ident { name, .. } if name.starts_with(char::is_uppercase) =>
                self.ikat_lewat_pencocok(pat, v, ty),
            Pattern::Ident { name, .. } => {
                if name == "_" { return Ok(()); }
                let slot = self.alloca_local(name, ty.clone());
                if *ty != JrgType::Unit {
                    self.emit(format!("store {} {}, ptr {}", ty.llvm_ty(), v.text, slot));
                }
                Ok(())
            }
            Pattern::Tuple { elems, .. } => {
                let field_tys: Vec<JrgType> = match ty {
                    JrgType::Tuple(ts) => ts.clone(),
                    _ => return Err(format!("let: pola tuple tidak cocok dengan tipe `{}`", ty.display_name())),
                };
                if field_tys.len() != elems.len() {
                    return Err("let: jumlah elemen pola tuple tidak cocok".to_string());
                }
                for (i, (p, fty)) in elems.iter().zip(field_tys.iter()).enumerate() {
                    let reg = self.fresh("tupfld.");
                    self.emit(format!("{} = extractvalue {} {}, {}", reg, ty.llvm_ty(), v.text, i));
                    self.bind_pattern(p, &Val { text: reg, ty: fty.clone() }, fty)?;
                }
                Ok(())
            }
            Pattern::Wildcard(_) => Ok(()),
            Pattern::Ref { inner, .. } => self.bind_pattern(inner, v, ty),

            Pattern::Array { .. } | Pattern::Enum { .. } => self.ikat_lewat_pencocok(pat, v, ty),

            _ => Err("let: bentuk pola ini belum didukung backend LLVM (gunakan identifier atau tuple)".into()),
        }
    }

    /// Pola yang bisa TIDAK cocok di posisi `let` memakai PENCOCOK YANG SAMA dengan
    /// `match`, bukan penyalinan aturannya; gagal cocok = gagal saat jalan.
    fn ikat_lewat_pencocok(&mut self, pat: &Pattern, v: &Val, ty: &JrgType) -> Result<(), String> {
        let tempat = self.spill_to_place(v);
        let gagal = self.fresh_label("letpat.gagal.");
        let lanjut = self.fresh_label("letpat.ok.");
        self.test_pattern(pat, &tempat, ty, &gagal)?;
        self.terminate(format!("br label %{}", lanjut));

        self.start_block(gagal);
        self.ensure_extern(
            "declare void @jrg_panic(ptr, i64) cold nounwind noreturn",
            "jrg_panic");
        let teks = match pat {
            Pattern::Array { .. } =>
                "destructure array: nilainya tidak cocok dengan pola di sisi kiri `let`".to_string(),
            _ => crate::interpreter::pesan_let_tak_cocok(0, &ty.display_name()),
        };
        let pesan = self.intern_string(&teks);
        let (mp, ml) = self.split_str(&pesan);
        self.emit(format!("call void @jrg_panic(ptr {}, i64 {})", mp, ml));
        self.terminate("unreachable".to_string());

        self.start_block(lanjut);
        Ok(())
    }

    pub(crate) fn codegen_short_decl(&mut self, s: &ShortDeclStmt) -> Result<(), String> {
        if s.names.len() == s.values.len() {
            for (n, e) in s.names.iter().zip(s.values.iter()) {
                let v = self.codegen_expr(e, None)?;
                if n == "_" { continue; }
                let slot = self.alloca_local(n, v.ty.clone());
                if v.ty != JrgType::Unit {
                    self.emit(format!("store {} {}, ptr {}", v.ty.llvm_ty(), v.text, slot));
                }
            }
            Ok(())
        } else if s.values.len() == 1 {
            let v = self.codegen_expr(&s.values[0], None)?;
            let field_tys: Vec<JrgType> = match &v.ty {
                JrgType::Tuple(ts) => ts.clone(),
                other => return Err(format!("`:=` multi-nama butuh nilai tuple, dapat `{}`", other.display_name())),
            };
            if field_tys.len() != s.names.len() {
                return Err("`:=`: jumlah nama tidak cocok dengan jumlah nilai yang dikembalikan".into());
            }
            for (i, (n, fty)) in s.names.iter().zip(field_tys.iter()).enumerate() {
                if n == "_" { continue; }
                let reg = self.fresh("mret.");
                self.emit(format!("{} = extractvalue {} {}, {}", reg, v.ty.llvm_ty(), v.text, i));
                let slot = self.alloca_local(n, fty.clone());
                self.emit(format!("store {} {}, ptr {}", fty.llvm_ty(), reg, slot));
            }
            Ok(())
        } else {
            Err("`:=`: jumlah nama dan nilai tidak cocok".into())
        }
    }

    /// Catatan "variabel ini tidak bisa berganti tipe", untuk sebuah target
    /// assignment yang tipenya sudah pasti.
    pub(crate) fn catatan_ganti_tipe(target: &Expr, ty: Option<&JrgType>) -> Option<String> {
        let Expr::Ident(nama, _) = target else { return None };
        let ty = ty?;
        if matches!(ty, JrgType::Unit) { return None; }
        Some(format!(
            "catatan: `{}` sudah bertipe `{}` di sini, dan backend `--emit-llvm` \
             tidak bisa mengganti tipe sebuah variabel di tengah jalan\n\
             bantuan: pakai nama lain untuk nilai bertipe lain\n\
             catatan: interpreter dan VM membolehkannya karena keduanya menyimpan \
             tipe bersama nilainya saat jalan",
            nama,
            ty.display_name()
        ))
    }

    /// Tipe tempat sasaran `a[i]`/`p.x`/`d[k]`, tanpa memancarkan apa pun.
    fn tipe_tempat(&self, e: &Expr) -> Option<JrgType> {
        if let Expr::Index { obj, .. } = e {
            if let Some(JrgType::Dict(v)) = self.tipe_jalur(obj) { return Some(*v); }
        }
        self.tipe_jalur(e)
    }

    pub(crate) fn codegen_assign(&mut self, a: &AssignStmt) -> Result<(), String> {
        if a.targets.len() != a.values.len() {
            if a.targets.len() == 1 && a.values.len() > 1 {
                return Err("assignment: satu target tidak bisa menerima banyak nilai".into());
            }
        }
        // `a = a + [x]`, `a = arr::push(a, x)`, `a += [x]`: ditumbuhkan DI TEMPAT (kapasitas
        // per variabel) — dulu tiap putaran menyalin seluruh array ke alokasi baru.
        if a.targets.len() > 1 && a.values.len() == 1 && matches!(a.op, AssignOp::Eq) {
            return self.codegen_bongkar(a);
        }
        if self.coba_tumbuh_di_tempat(a)? { return Ok(()); }
        if self.coba_tumbuh_jalur(a)? { return Ok(()); }
        if self.coba_panggil_milik(a)? { return Ok(()); }
        self.arg_pindah = self.argumen_dipindahkan(a);
        // `d = dict::set(d, k, v)` adalah `d[k] = v`: disisipkan di tempat, tanpa menyalin dict.
        if let Some(langsung) = Self::pasang_di_tempat(a) { return self.codegen_assign(&langsung); }
        // Semua sisi kanan dulu, baru ditulis (`a, b = b, a`). Sasaran tunggal `a[i]`/`p.x` juga
        // mencari tempatnya SESUDAH sisi kanan, seperti dua mesin acuan (#750).
        let tunda = match (a.targets.as_slice(), a.values.len()) {
            ([t @ (Expr::Index { .. } | Expr::Field { .. })], 1) => self.tipe_tempat(t),
            _ => None,
        };
        let mut place_ptrs: Vec<Option<(String, JrgType)>> = Vec::with_capacity(a.targets.len());
        for t in &a.targets {
            // `_` membuang nilainya: tidak ada tempat untuk ditulis (membacanya ditolak parser).
            let buang = matches!(t, Expr::Ident(n, _) if n == "_");
            place_ptrs.push(if buang || tunda.is_some() { None } else { Some(self.get_place(t)?) });
        }
        let mut new_vals = Vec::with_capacity(a.values.len());
        for (i, val_expr) in a.values.iter().enumerate() {
            let tempat = &place_ptrs[i.min(place_ptrs.len() - 1)];
            let expected = tempat.as_ref().filter(|_| a.targets.len() == a.values.len())
                .map(|(_, ty)| ty.clone()).or_else(|| tunda.clone());
            // Nama targetnya dititipkan, supaya closure di sisi kanan tahu ia akan
            // dipanggil dengan nama apa — itu yang membuat `rec = |n| ...
            let nama_lama = std::mem::replace(
                &mut self.nama_diikat,
                match &a.targets[i.min(a.targets.len() - 1)] {
                    Expr::Ident(n, _) => Some(n.clone()),
                    _ => None,
                });
            // Catatan dipasang SEBELUM sisi kanan dievaluasi, bukan cuma di sekitar
            // koersi terakhir.
            let catatan_lama = std::mem::replace(
                &mut self.catatan_koersi,
                Self::catatan_ganti_tipe(
                    &a.targets[i.min(a.targets.len() - 1)],
                    expected.as_ref(),
                ),
            );
            let v = self.codegen_expr(val_expr, expected.as_ref());
            self.catatan_koersi = catatan_lama;
            self.nama_diikat = nama_lama;
            let v = v?;
            let v = self.ambil_milik(val_expr, v)?;
            new_vals.push(v);
        }
        if tunda.is_some() { place_ptrs[0] = Some(self.get_place(&a.targets[0])?); }
        for (i, (tempat, rhs)) in place_ptrs.into_iter().zip(new_vals.into_iter()).enumerate() {
            let Some((ptr, ty)) = tempat else { continue };
            let rhs_final = if matches!(a.op, AssignOp::Eq) {
                // JRG bertipe DINAMIS: `var x = 1; x = "teks"` sah, dan `typeof(x)` berubah
                // jadi `string`.
                let catatan_lama = std::mem::replace(
                    &mut self.catatan_koersi,
                    Self::catatan_ganti_tipe(
                        &a.targets[i.min(a.targets.len() - 1)],
                        Some(&ty),
                    ),
                );
                let hasil = self.coerce(&rhs, &ty);
                self.catatan_koersi = catatan_lama;
                hasil
            } else {
                let cur = self.load_place(&ptr, &ty);
                let op = compound_to_binop(&a.op);
                // Lewat `gabung_nilai`, BUKAN `arith_binop` langsung: itu yang membuat `a +=
                // b` benar-benar berarti `a = a + b`.
                // Operator milik pemakai didahulukan, urutan yang sama dengan `a + b`.
                let hasil = match self.operator_pengguna_biner(&op, &cur, &rhs, a.span)? {
                    Some(h) => h,
                    None => self.gabung_nilai(&op, cur, rhs, a.span)?,
                };
                // Dikoersi ke tipe target, sama seperti cabang `=` di atas.
                self.coerce(&hasil, &ty)
            };
            if ty != JrgType::Unit {
                self.emit(format!("store {} {}, ptr {}", ty.llvm_ty(), rhs_final.text, ptr));
            }
            let _ = i;
        }
        Ok(())
    }

    pub fn load_place(&mut self, ptr: &str, ty: &JrgType) -> Val {
        if *ty == JrgType::Unit {
            return Val::unit();
        }
        let reg = self.fresh("ld.");
        self.emit(format!("{} = load {}, ptr {}", reg, ty.llvm_ty(), ptr));
        Val { text: reg, ty: ty.clone() }
    }

    pub fn codegen_if(&mut self, ifs: &IfStmt) -> Result<(), String> {
        self.push_scope();
        if let Some(init) = &ifs.init { self.codegen_stmt(init)?; }
        let merge = self.fresh_label("if.end.");
        self.codegen_if_chain(&ifs.cond, &ifs.then, &ifs.else_ifs, 0, &ifs.else_, &merge)?;
        if !self.finished_blocks.iter().any(|_| false) { /* no-op, keeps borrow checker calm about unused merge in some paths */ }
        self.start_block(merge);
        self.pop_scope();
        Ok(())
    }

    /// `if` in expression position — the value is the taken branch's block value,
    /// stored into one stack slot that the merge block loads back.
    pub fn codegen_if_expr(&mut self, s: &IfStmt, expected: Option<&JrgType>) -> Result<Val, String> {
        if s.else_.is_none() {
            // No `else`: not value-producing. Lower exactly as the
            // statement form so behavior is unchanged from before `Expr::If`
            // existed.
            self.codegen_if(s)?;
            return Ok(Val::unit());
        }

        self.push_scope();
        if let Some(init) = &s.init {
            self.codegen_stmt(init)?;
        }
        let merge = self.fresh_label("ifx.end.");
        let mut acc = IfExprValue {
            slot: expected
                .filter(|t| **t != JrgType::Unit)
                .map(|t| (self.alloca_typed("ifx.slot.", t), t.clone())),
            saw_valueless_branch: false,
        };
        self.codegen_if_expr_chain(&s.cond, &s.then, &s.else_ifs, 0, &s.else_, &merge, &mut acc)?;
        self.start_block(merge);
        self.pop_scope();

        match acc.slot {
            Some((ptr, ty)) => {
                if acc.saw_valueless_branch {
                    // Some branch produced a value and another produced none.
                    return Err(format!(
                        "`if` dipakai sebagai nilai bertipe `{}`, tetapi ada cabang yang tidak menghasilkan nilai — \
                         setiap cabang (termasuk `else`) harus diakhiri sebuah ekspresi",
                        ty.display_name()
                    ));
                }
                Ok(self.load_place(&ptr, &ty))
            }
            None => Ok(Val::unit()),
        }
    }

    pub(crate) fn codegen_if_expr_chain(
        &mut self,
        cond: &Expr,
        then_blk: &Block,
        else_ifs: &[(Option<Box<Stmt>>, Box<Expr>, Block)],
        idx: usize,
        final_else: &Option<Block>,
        merge: &str,
        acc: &mut IfExprValue,
    ) -> Result<(), String> {
        let cv = self.codegen_expr(cond, Some(&JrgType::Bool))?;
        let cond_i1 = self.to_i1(&cv);
        let then_lbl = self.fresh_label("ifx.then.");
        let else_lbl = self.fresh_label("ifx.else.");
        self.terminate(format!("br i1 {}, label %{}, label %{}", cond_i1, then_lbl, else_lbl));

        self.start_block(then_lbl);
        self.push_scope();
        let want = acc.slot.as_ref().map(|(_, t)| t.clone());
        let v = self.codegen_block_expr(then_blk, want.as_ref())?;
        self.pop_scope();
        self.store_branch_value(v, acc);
        if !self.terminated { self.terminate(format!("br label %{}", merge)); }

        self.start_block(else_lbl);
        if idx < else_ifs.len() {
            let (init, econd, eblk) = &else_ifs[idx];
            self.push_scope();
            if let Some(i) = init { self.codegen_stmt(i)?; }
            self.codegen_if_expr_chain(econd, eblk, else_ifs, idx + 1, final_else, merge, acc)?;
            self.pop_scope();
        } else if let Some(eb) = final_else {
            self.push_scope();
            let want = acc.slot.as_ref().map(|(_, t)| t.clone());
            let v = self.codegen_block_expr(eb, want.as_ref())?;
            self.pop_scope();
            self.store_branch_value(v, acc);
            if !self.terminated { self.terminate(format!("br label %{}", merge)); }
        } else if !self.terminated {
            self.terminate(format!("br label %{}", merge));
        }
        Ok(())
    }

    /// Commit one branch's value into the shared result slot, allocating the slot
    /// on the first branch that actually produces a value (which is how an …
    pub(crate) fn store_branch_value(&mut self, v: Val, acc: &mut IfExprValue) {
        // A branch that already left the function (`return`, `break`, a
        // propagated `?`) has no value to contribute and no path to the
        // merge block — it is not a "valueless branch" in the error sense.
        if self.terminated {
            return;
        }
        if acc.slot.is_none() {
            if v.ty == JrgType::Unit {
                acc.saw_valueless_branch = true;
                return;
            }
            let ty = v.ty.clone();
            let ptr = self.alloca_typed("ifx.slot.", &ty);
            acc.slot = Some((ptr, ty));
        }
        let (ptr, ty) = acc.slot.clone().expect("slot just ensured");
        if v.ty == JrgType::Unit {
            acc.saw_valueless_branch = true;
            return;
        }
        let cv = self.coerce(&v, &ty);
        self.emit(format!("store {} {}, ptr {}", ty.llvm_ty(), cv.text, ptr));
    }

    pub(crate) fn codegen_if_chain(
        &mut self,
        cond: &Expr,
        then_blk: &Block,
        else_ifs: &[(Option<Box<Stmt>>, Box<Expr>, Block)],
        idx: usize,
        final_else: &Option<Block>,
        merge: &str,
    ) -> Result<(), String> {
        let cv = self.codegen_expr(cond, Some(&JrgType::Bool))?;
        let cond_i1 = self.to_i1(&cv);
        let then_lbl = self.fresh_label("if.then.");
        let else_lbl = self.fresh_label("if.else.");
        self.terminate(format!("br i1 {}, label %{}, label %{}", cond_i1, then_lbl, else_lbl));

        self.start_block(then_lbl);
        self.push_scope();
        self.codegen_stmt_block(then_blk)?;
        self.pop_scope();
        if !self.terminated { self.terminate(format!("br label %{}", merge)); }

        self.start_block(else_lbl);
        if idx < else_ifs.len() {
            let (init, econd, eblk) = &else_ifs[idx];
            self.push_scope();
            if let Some(i) = init { self.codegen_stmt(i)?; }
            self.codegen_if_chain(econd, eblk, else_ifs, idx + 1, final_else, merge)?;
            self.pop_scope();
        } else if let Some(eb) = final_else {
            self.push_scope();
            self.codegen_stmt_block(eb)?;
            self.pop_scope();
            if !self.terminated { self.terminate(format!("br label %{}", merge)); }
        } else if !self.terminated {
            self.terminate(format!("br label %{}", merge));
        }
        Ok(())
    }

    pub(crate) fn codegen_while(&mut self, w: &WhileStmt) -> Result<(), String> {
        let cond_lbl = self.fresh_label("while.cond.");
        let body_lbl = self.fresh_label("while.body.");
        let end_lbl = self.fresh_label("while.end.");
        self.terminate(format!("br label %{}", cond_lbl));

        self.start_block(cond_lbl.clone());
        let cv = self.codegen_expr(&w.cond, Some(&JrgType::Bool))?;
        let ci1 = self.to_i1(&cv);
        self.terminate(format!("br i1 {}, label %{}, label %{}", ci1, body_lbl, end_lbl));

        self.start_block(body_lbl);
        self.loop_stack.push(LoopCtx { label: w.label.clone(), continue_bb: cond_lbl.clone(), break_bb: end_lbl.clone() });
        self.push_scope();
        self.codegen_stmt_block(&w.body)?;
        self.pop_scope();
        self.loop_stack.pop();
        if !self.terminated { self.terminate(format!("br label %{}", cond_lbl)); }

        self.start_block(end_lbl);
        Ok(())
    }

    pub(crate) fn codegen_loop(&mut self, l: &LoopStmt) -> Result<(), String> {
        let body_lbl = self.fresh_label("loop.body.");
        let end_lbl = self.fresh_label("loop.end.");
        self.terminate(format!("br label %{}", body_lbl));

        self.start_block(body_lbl.clone());
        self.loop_stack.push(LoopCtx { label: l.label.clone(), continue_bb: body_lbl.clone(), break_bb: end_lbl.clone() });
        self.push_scope();
        self.codegen_stmt_block(&l.body)?;
        self.pop_scope();
        self.loop_stack.pop();
        if !self.terminated { self.terminate(format!("br label %{}", body_lbl)); }

        self.start_block(end_lbl);
        Ok(())
    }

    pub(crate) fn codegen_for(&mut self, f: &ForStmt) -> Result<(), String> {
        match f {
            ForStmt::Classic { init, cond, post, body, label, .. } => {
                self.push_scope();
                if let Some(i) = init { self.codegen_stmt(i)?; }
                let cond_lbl = self.fresh_label("for.cond.");
                let body_lbl = self.fresh_label("for.body.");
                let post_lbl = self.fresh_label("for.post.");
                let end_lbl = self.fresh_label("for.end.");
                self.terminate(format!("br label %{}", cond_lbl));

                self.start_block(cond_lbl.clone());
                if let Some(c) = cond {
                    let cv = self.codegen_expr(c, Some(&JrgType::Bool))?;
                    let ci1 = self.to_i1(&cv);
                    self.terminate(format!("br i1 {}, label %{}, label %{}", ci1, body_lbl, end_lbl));
                } else {
                    self.terminate(format!("br label %{}", body_lbl));
                }

                self.start_block(body_lbl);
                self.loop_stack.push(LoopCtx { label: label.clone(), continue_bb: post_lbl.clone(), break_bb: end_lbl.clone() });
                self.push_scope();
                self.codegen_stmt_block(body)?;
                self.pop_scope();
                self.loop_stack.pop();
                if !self.terminated { self.terminate(format!("br label %{}", post_lbl)); }

                self.start_block(post_lbl);
                if let Some(p) = post { self.codegen_stmt(p)?; }
                if !self.terminated { self.terminate(format!("br label %{}", cond_lbl)); }

                self.start_block(end_lbl);
                self.pop_scope();
                Ok(())
            }
            ForStmt::Range { vars, iter, body, label, .. } =>
                self.codegen_for_range(vars, iter, body, label),
        }
    }

    pub(crate) fn codegen_return(&mut self, r: &ReturnStmt) -> Result<(), String> {
        let ret_ty = self.cur_ret_ty.clone();
        if r.values.is_empty() {
            self.emit_void_return();
            return Ok(());
        }
        if r.values.len() == 1 {
            // Tipe kembalian TEBAKAN `()`: nilainya diturunkan tanpa harapan, supaya penjaga
            // melihat tipe aslinya — dengan harapan `()` ternary dan `match` sudah jadi `()`.
            let harapan = if self.ret_ditebak { None } else { Some(&ret_ty) };
            let v = self.codegen_expr(&r.values[0], harapan)?;
            let v = self.lepas_dari_self(&r.values[0], v);
            self.emit_return(&v, &ret_ty);
            return Ok(());
        }
        // multiple return values -> build the tuple aggregate then return it
        let field_tys: Vec<JrgType> = match &ret_ty {
            JrgType::Tuple(ts) => ts.clone(),
            _ => return Err("return: banyak nilai dikembalikan tapi tipe return fungsi bukan tuple".into()),
        };
        if field_tys.len() != r.values.len() {
            return Err("return: jumlah nilai tidak cocok dengan tipe return fungsi".into());
        }
        let mut agg = "undef".to_string();
        for (i, (e, fty)) in r.values.iter().zip(field_tys.iter()).enumerate() {
            let v = self.codegen_expr(e, Some(fty))?;
            let v = self.coerce(&v, fty);
            let v = self.lepas_dari_self(e, v);
            let reg = self.fresh("retagg.");
            self.emit(format!("{} = insertvalue {} {}, {} {}, {}", reg, ret_ty.llvm_ty(), agg, fty.llvm_ty(), v.text, i));
            agg = reg;
        }
        // Lewat `emit_return` juga, supaya `defer` tidak punya pengecualian
        // di satu bentuk `return` saja — pintu keluarnya harus satu.
        let v = Val { text: agg, ty: ret_ty.clone() };
        self.emit_return(&v, &ret_ty);
        Ok(())
    }

    pub(crate) fn codegen_break(&mut self, b: &BreakStmt) -> Result<(), String> {
        let target = self.find_loop(&b.label).map(|c| c.break_bb.clone());
        match target {
            Some(bb) => { self.terminate(format!("br label %{}", bb)); Ok(()) }
            None => Err("`break` di luar loop".into()),
        }
    }

    pub(crate) fn codegen_continue(&mut self, c: &ContinueStmt) -> Result<(), String> {
        let target = self.find_loop(&c.label).map(|ctx| ctx.continue_bb.clone());
        match target {
            Some(bb) => { self.terminate(format!("br label %{}", bb)); Ok(()) }
            None => Err("`continue` di luar loop".into()),
        }
    }

    pub(crate) fn find_loop(&self, label: &Option<String>) -> Option<&LoopCtx> {
        match label {
            None => self.loop_stack.last(),
            Some(l) => self.loop_stack.iter().rev().find(|c| c.label.as_deref() == Some(l.as_str())),
        }
    }


    // ── casts / coercion ─────────────────────────────────────────────────

    /// Turunkan sebuah nilai jadi `i1` untuk dipakai sebagai kondisi cabang.
    pub fn to_i1(&mut self, v: &Val) -> String {
        let b = if v.ty == JrgType::Bool { v.clone() } else { self.coerce_longgar(v, &JrgType::Bool) };
        let reg = self.fresh("i1.");
        self.emit(format!("{} = trunc i8 {} to i1", reg, b.text));
        reg
    }

    pub fn from_i1(&mut self, reg: &str) -> Val {
        let out = self.fresh("b.");
        self.emit(format!("{} = zext i1 {} to i8", out, reg));
        Val { text: out, ty: JrgType::Bool }
    }

    pub fn cur_ret_ty(&self) -> JrgType {
        self.cur_ret_ty.clone()
    }

    pub fn push_loop(&mut self, ctx: LoopCtx) {
        self.loop_stack.push(ctx);
    }

    pub fn pop_loop(&mut self) {
        self.loop_stack.pop();
    }
}
