// src/llvm/codegen_tunda.rs — llvm/codegen_tunda.rs — `defer` untuk backend LLVM.

use crate::parser::{Block, DeferStmt, Expr, ForStmt, IfStmt, MatchArm, MatchBody, Stmt};

use super::codegen::{Codegen, Val};
use super::types::JrgType;

/// Apakah badan fungsi ini memuat `defer` di mana pun, termasuk di dalam blok
/// bersarang?
pub fn blok_punya_defer(b: &Block) -> bool {
    b.stmts.iter().any(di_stmt) || b.tail.as_deref().map_or(false, di_ekspr)
}

fn di_if(i: &IfStmt) -> bool {
    i.init.as_deref().map_or(false, di_stmt)
        || di_ekspr(&i.cond)
        || blok_punya_defer(&i.then)
        || i.else_ifs.iter().any(|(s, c, b)| {
            s.as_deref().map_or(false, di_stmt) || di_ekspr(c) || blok_punya_defer(b)
        })
        || i.else_.as_ref().map_or(false, blok_punya_defer)
}

fn di_arm(a: &MatchArm) -> bool {
    a.guard.as_deref().map_or(false, di_ekspr)
        || match &a.body {
            MatchBody::Block(b) => blok_punya_defer(b),
            MatchBody::Expr(e) => di_ekspr(e),
        }
}

fn di_stmt(s: &Stmt) -> bool {
    match s {
        Stmt::Defer(_) => true,
        Stmt::Block(b) => blok_punya_defer(b),
        Stmt::If(i) => di_if(i),
        Stmt::While(w) => blok_punya_defer(&w.body) || di_ekspr(&w.cond),
        Stmt::For(ForStmt::Classic { init, cond, post, body, .. }) => {
            init.as_deref().map_or(false, di_stmt)
                || cond.as_deref().map_or(false, di_ekspr)
                || post.as_deref().map_or(false, di_stmt)
                || blok_punya_defer(body)
        }
        Stmt::For(ForStmt::Range { iter, body, .. }) => di_ekspr(iter) || blok_punya_defer(body),
        Stmt::Loop(l) => blok_punya_defer(&l.body),
        Stmt::Match(m) => di_ekspr(&m.scrutinee) || m.arms.iter().any(di_arm),
        Stmt::Expr(e) => di_ekspr(e),
        _ => false,
    }
}

/// `if` dan `match` di JRG adalah EKSPRESI, jadi sebuah `if` yang berdiri
/// sendiri sebagai statement sampai ke sini sebagai `Stmt::Expr(Expr::If)` — …
pub fn di_ekspr(e: &Expr) -> bool {
    match e {
        Expr::Block(b) => blok_punya_defer(b),
        Expr::If(i) => di_if(i),
        Expr::Match { scrutinee, arms, .. } => di_ekspr(scrutinee) || arms.iter().any(di_arm),
        _ => false,
    }
}

impl Codegen {
    pub fn codegen_defer(&mut self, d: &DeferStmt) -> Result<(), String> {
        if self.tunda.is_none() {
            // Tidak bisa terjadi lewat jalur biasa — `blok_punya_defer`
            // sudah membuka bingkainya. Kalau toh terjadi, ia harus
            // menyebutkan apa yang sebenarnya salah, bukan diam.
            return Err(super::galat::tandai(d.span,
                "`defer` di tempat yang tidak punya bingkai fungsi".to_string()));
        }

        // Hanya panggilan yang bisa ditunda — sama seperti acuannya.
        let (arg_exprs, penerima) = match &*d.expr {
            Expr::Call { args, .. } => (args.clone(), None),
            Expr::Method { obj, args, .. } => (args.clone(), Some((**obj).clone())),
            lain => return Err(super::galat::tandai(d.span, format!(
                "`defer` butuh sebuah pemanggilan, diberi `{}`\n\
                 bantuan: tulis `defer f(x)` atau `defer obj.m(x)`",
                sebut_bentuk(lain)))),
        };

        // Penerima method ikut dibekukan: `defer p.tandai()` harus menandai
        // `p` yang ada SAAT baris itu dibaca.
        let mut beku: Vec<(String, JrgType, Val)> = Vec::new();
        let id = self.tunda.as_ref().unwrap().jumlah;
        if let Some(o) = &penerima {
            let v = self.codegen_expr(o, None)?;
            // Salin kalau ia agregat milik orang lain: nilai bekunya harus
            // benar-benar terlepas dari variabel asalnya.
            let v = self.ambil_milik(o, v)?;
            beku.push((format!("$tunda{}.diri", id), v.ty.clone(), v));
        }
        // Yang dipanggil ikut dibekukan kalau ia NILAI (lokal atau hasil ekspresi):
        // situsnya jadi fungsi tersendiri dan tidak melihat lokal fungsi ini.
        let mut fungsi_beku = None;
        if let Expr::Call { func, .. } = &*d.expr {
            let nilai = match &**func {
                Expr::Ident(n, _) => self.lookup_local(n).is_some(),
                Expr::PathExpr { .. } => false,
                _ => true,
            };
            if nilai {
                let v = self.codegen_expr(func, None)?;
                let nama = format!("$tunda{}.fn", id);
                beku.push((nama.clone(), v.ty.clone(), v));
                fungsi_beku = Some(nama);
            }
        }
        for (i, a) in arg_exprs.iter().enumerate() {
            let v = self.codegen_expr(a, None)?;
            let v = self.ambil_milik(a, v)?;
            beku.push((format!("$tunda{}.{}", id, i), v.ty.clone(), v));
        }

        // Satu blok heap per pendaftaran: sebuah `defer` di dalam loop mendaftar
        // sekali per putaran, dan tiap putaran punya nilai bekunya sendiri.
        let blok_ty = JrgType::Tuple(beku.iter().map(|(_, t, _)| t.clone()).collect());
        let tipe_blok = blok_ty.llvm_ty();
        let blok = if beku.is_empty() {
            "null".to_string()
        } else {
            let uk = self.types.sizeof(&blok_ty);
            self.ensure_extern("declare ptr @jrg_alloc(i64, i64)", "jrg_alloc");
            let p = self.fresh("tundaarg.");
            self.emit(format!("{} = call ptr @jrg_alloc(i64 {}, i64 8)", p, uk));
            for (i, (_, t, v)) in beku.iter().enumerate() {
                if *t == JrgType::Unit { continue; }
                let f = self.fresh("tundaf.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", f, tipe_blok, p, i));
                self.emit(format!("store {} {}, ptr {}", t.llvm_ty(), v.text, f));
            }
            p
        };

        // Panggilan aslinya disimpan dengan tiap argumen (dan penerimanya) diganti
        // nama sintetis, yang diikat sebagai lokal di fungsi situsnya.
        let sp = d.span;
        let ganti = |nama: &str| Expr::Ident(nama.to_string(), sp);
        let panggilan = match &*d.expr {
            Expr::Call { func, span, .. } => Expr::Call {
                func: match &fungsi_beku { Some(n) => Box::new(ganti(n)), None => func.clone() },
                args: (0..arg_exprs.len()).map(|i| ganti(&format!("$tunda{}.{}", id, i))).collect(),
                span: *span,
            },
            Expr::Method { method, span, .. } => Expr::Method {
                obj: Box::new(ganti(&format!("$tunda{}.diri", id))),
                method: method.clone(),
                args: (0..arg_exprs.len()).map(|i| ganti(&format!("$tunda{}.{}", id, i))).collect(),
                span: *span,
            },
            _ => unreachable!("bentuk lain sudah ditolak di atas"),
        };

        let nama = format!("{}.tunda", self.nama_closure_baru());
        self.ensure_extern("declare void @jrg_tunda_push(ptr, ptr)", "jrg_tunda_push");
        self.emit(format!("call void @jrg_tunda_push(ptr @{}, ptr {})",
            super::mangle::llvm_quoted_ident(&nama), blok));
        self.tunda.as_mut().unwrap().jumlah += 1;
        let generik = self.cur_fn_generics.clone();
        if let Some(t) = self.cur_self_ty.clone() { self.ruang_simbol.insert(nama.clone(), t); }
        self.antre_fn(super::codegen::KerjaFn::Tunda {
            nama, panggilan, generik,
            beku: beku.into_iter().map(|(n, t, _)| (n, t)).collect(),
        });
        Ok(())
    }

    /// Satu situs `defer` sebagai fungsi: buka nilai bekunya, lalu panggil.
    pub fn pancarkan_situs_tunda(
        &mut self, nama: &str, panggilan: &Expr, beku: &[(String, JrgType)],
        generik: super::sema::TypeSubst,
    ) -> Result<(), String> {
        self.mulai_fungsi_kosong(JrgType::Unit);
        self.cur_self_ty = self.ruang_simbol.get(nama).cloned();
        self.cur_fn_generics = generik;
        let ditebak_lama = std::mem::replace(&mut self.ret_ditebak, false);
        let nama_lama = std::mem::replace(&mut self.nama_diikat, None);

        let tipe_blok = JrgType::Tuple(beku.iter().map(|(_, t)| t.clone()).collect()).llvm_ty();
        for (j, (n, ty)) in beku.iter().enumerate() {
            let slot = self.alloca_typed("tundal.", ty);
            if *ty != JrgType::Unit {
                let f = self.fresh("tundab.");
                self.emit(format!("{} = getelementptr {}, ptr %arg, i32 0, i32 {}", f, tipe_blok, j));
                let v = self.load_place(&f, ty);
                self.emit(format!("store {} {}, ptr {}", ty.llvm_ty(), v.text, slot));
            }
            self.define_local(n, slot, ty.clone());
        }
        // Nilainya dibuang: `defer` tidak punya tempat untuk hasil.
        let hasil = self.codegen_expr(panggilan, None);
        if hasil.is_ok() && !self.is_terminated() { self.terminate("ret void".to_string()); }

        let badan = self.take_function_body();
        self.selesai_fungsi_kosong();
        self.ret_ditebak = ditebak_lama;
        self.nama_diikat = nama_lama;
        hasil?;
        self.tambah_definisi(format!(
            "define internal void @{}(ptr %arg) local_unnamed_addr nounwind {{\n{}}}\n",
            super::mangle::llvm_quoted_ident(nama), badan));
        Ok(())
    }

    /// Blok pulang fungsi ber-`defer`: jalankan yang terdaftar sejak ia masuk.
    pub fn pancarkan_blok_tunda(&mut self, ret_ty: &JrgType) -> Result<(), String> {
        let Some(t) = self.tunda.take() else { return Ok(()) };
        self.start_block(t.blok.clone());
        self.ensure_extern("declare void @jrg_tunda_jalankan(i64)", "jrg_tunda_jalankan");
        self.emit(format!("call void @jrg_tunda_jalankan(i64 {})", t.dasar));
        // `main` ber-`defer`: goroutine ditunggu SESUDAH defer-nya, seperti dua mesin acuan (#755).
        if self.sedang_main() { self.tunggu_goroutine(); }
        self.pancarkan_ret(&t, ret_ty);
        Ok(())
    }

    fn pancarkan_ret(&mut self, t: &super::codegen::Tunda, ret_ty: &JrgType) {
        match &t.slot_ret {
            None => self.terminate("ret void".to_string()),
            Some(slot) => {
                let ty = if self.sedang_main() { JrgType::I32 } else { ret_ty.clone() };
                let v = self.load_place(slot, &ty);
                self.terminate(format!("ret {} {}", ty.llvm_ty(), v.text));
            }
        }
    }
}

fn sebut_bentuk(e: &Expr) -> &'static str {
    match e {
        Expr::Ident(..) => "sebuah nama",
        Expr::Binary { .. } => "sebuah operasi",
        Expr::Literal(..) => "sebuah nilai tetap",
        Expr::Index { .. } => "sebuah pengindeksan",
        Expr::Field { .. } => "sebuah akses field",
        _ => "bukan pemanggilan",
    }
}
