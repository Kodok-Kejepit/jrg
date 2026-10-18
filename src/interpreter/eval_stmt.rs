// src/interpreter/eval_stmt.rs — interpreter/eval_stmt.rs
// Eksekusi statement: `exec_stmt` (dispatcher utama) + semua `exec_*`

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

/// Satu langkah di jalur target assignment: `[...]` atau `.nama`.
#[derive(Debug, Clone)]
/// Satu langkah turun di jalur target sebuah assignment (`grid[0].isi = v`).
pub(crate) enum Langkah {
    Index(Value, crate::lexer::Span),
    Field(String, crate::lexer::Span),
}

impl Langkah {
    fn span(&self) -> crate::lexer::Span {
        match self { Langkah::Index(_, s) | Langkah::Field(_, s) => *s }
    }
}

/// Isi lokasi sebuah galat dari langkah yang sedang dijalani.
pub(crate) fn beri_span(sig: Signal, l: &Langkah) -> Signal {
    match sig {
        Signal::RuntimeError(e) => Signal::RuntimeError(e.fill_span(l.span())),
        lain => lain,
    }
}

impl Interpreter {
    pub fn exec_stmt(&mut self, stmt: &Stmt, env: &mut Env) -> EvalResult {
        match stmt {
            Stmt::Expr(e)       => self.exec_expr_stmt(e, env),
            // Fungsi bernama di dalam badan fungsi lain.
            Stmt::Fn(_)         => Ok(Value::Unit),
            Stmt::Let(s)        => self.exec_let(s, env),
            Stmt::Var(s)        => self.exec_var(s, env),
            Stmt::Const(cd)     => {
                let v = self.eval_expr(&cd.value, env)?;
                env.define(&cd.name, v);
                Ok(Value::Unit)
            }
            Stmt::Assign(s)     => self.exec_assign(s, env),
            Stmt::ShortDecl(s)  => self.exec_short_decl(s, env),
            Stmt::If(s)         => self.exec_if(s, env),
            Stmt::For(s)        => self.exec_for(s, env),
            Stmt::While(s)      => self.exec_while(s, env),
            Stmt::Loop(s)       => self.exec_loop(s, env),
            Stmt::Match(s)      => self.exec_match(s, env),
            Stmt::Return(s)     => self.exec_return(s, env),
            Stmt::Break(s)      => Err(Signal::Break(s.label.clone())),
            Stmt::Continue(s)   => Err(Signal::Continue(s.label.clone())),
            Stmt::Block(b)      => self.exec_block(b, env),
            Stmt::Defer(d)      => self.exec_defer(d, env),
            Stmt::Spawn(sp)     => {
                self.spawn_goroutine(sp, env)?;
                Ok(Value::Unit)
            }
            Stmt::Select(sel)   => self.exec_select(sel, env),
            Stmt::Error(span)   => runtime_err!("AST error node, span: {:?}", span),
        }
    }

    /// Menjalankan statement berupa satu ekspresi saja, mis. `foo();`.
    /// Sebuah FUNGSI yang berdiri sendiri di sini ditolak: dulu yang nullary
    /// dipanggil (dengan peringatan) sementara VM dan LLVM diam saja.
    pub(crate) fn exec_expr_stmt(&mut self, e: &Expr, env: &mut Env) -> EvalResult {
        if matches!(e, Expr::Ident(..) | Expr::PathExpr { .. }) {
            let val = self.eval_expr(e, env)?;
            if val.bisa_dipanggil() {
                let nama = match e {
                    Expr::Ident(n, _) => n.clone(),
                    Expr::PathExpr { segments, .. } => segments.join("::"),
                    _ => format!("{}", val),
                };
                return Err(Signal::RuntimeError(
                    RuntimeError::new(crate::interpreter::galat_index::fungsi_sebagai_pernyataan(&nama))
                        .fill_span(e.span()),
                ));
            }
            return Ok(Value::Unit);
        }
        self.eval_expr(e, env)?;
        Ok(Value::Unit)
    }

    pub(crate) fn exec_let(&mut self, s: &LetStmt, env: &mut Env) -> EvalResult {
        let v = self.eval_expr(&s.init, env)?;
        // Dulu bagian pola yang MENGUJI (literal, rentang, varian unit) dilewati,
        // jadi `let (a, 5) = (1, 6)` diterima diam-diam.
        let pasti = match &s.pattern {
            crate::parser::Pattern::Wildcard(_) => true,
            crate::parser::Pattern::Ident { name, .. } => self.varian_unit(name).is_none(),
            _ => false,
        };
        if !pasti {
            if let Err(e) = self.cek_pola(&s.pattern, &v) {
                return runtime_err!("{}", e);
            }
            if !self.pat_matches(&s.pattern, &v) {
                return Err(Signal::RuntimeError(RuntimeError::new(
                    crate::interpreter::pesan_let_tak_cocok(crate::parser::asal_let(s), &v.type_name()))
                    .fill_span(s.span)));
            }
        }
        self.bind_pattern(&s.pattern, v, env)?;
        Ok(Value::Unit)
    }

    pub(crate) fn exec_var(&mut self, s: &VarStmt, env: &mut Env) -> EvalResult {
        let nol = || s.ty.as_ref().and_then(|t| crate::parser::nilai_nol(t, s.span));
        let v = if let Some(init) = &s.init { self.eval_expr(init, env)? }
                else if let Some(e) = nol() { self.eval_expr(&e, env)? }
                else { Value::Nil };
        env.define(&s.name, v);
        Ok(Value::Unit)
    }

    /// Jalur cepat untuk pola MENUMBUHKAN KOLEKSI: `a = a + x`, `a = a + x + y + …`, `a += x`.
    pub(crate) fn coba_tumbuh_di_tempat(
        &mut self,
        s: &crate::parser::AssignStmt,
        env: &mut Env,
    ) -> Result<Option<Value>, Signal> {
        if s.targets.len() != 1 || s.values.len() != 1 { return Ok(None); }
        let Expr::Ident(name, _) = &s.targets[0] else { return Ok(None) };
        let operan: Vec<&Expr> = match (&s.op, &s.values[0]) {
            (AssignOp::PlusEq, v) => vec![v],
            (AssignOp::Eq, e) => match crate::interpreter::capture::rantai_tambah(e, name) {
                Some(o) => o,
                None => return Ok(None),
            },
            _ => return Ok(None),
        };
        // Cuma array dan string yang tumbuh; sisanya (angka, struct ber-operator `add`)
        // lewat jalur biasa — diperiksa SEBELUM menelusuri AST, yang jauh lebih mahal.
        if !env.lokal_polos_koleksi(name) { return self.tumbuh_bukan_lokal(s, name, &operan, env); }
        if operan.iter().any(|e| crate::interpreter::capture::names_mentioned(e).contains(name.as_str())) {
            return Ok(None);
        }
        let Some(mut acc) = env.ambil_lokal_polos(name) else { return Ok(None) };
        let awal = panjang_koleksi(&acc);
        let mut cadangan = None;
        for e in operan {
            let langkah = match self.eval_expr(e, env) {
                Ok(kanan) => self.tambah_di_tempat(&mut acc, kanan, &mut cadangan),
                Err(err) => Err(err),
            };
            if let Err(err) = langkah {
                // Gagal di tengah rantai: binding kembali ke nilai SEMULA, seperti jalur biasa.
                env.set(name, pulihkan_koleksi(cadangan.unwrap_or(acc), awal));
                return Err(beri_lokasi_tumbuh(err, s));
            }
        }
        env.set(name, acc);
        Ok(Some(Value::Unit))
    }

    /// `A = A + [x]` atas `var` modul atau lokal yang ditangkap closure. Bindingnya tidak bisa
    /// dikosongkan (kanan boleh membacanya), jadi nilai kini dipegang berdua dan disambung di
    /// tempat hanya kalau penyimpanannya masih yang sama sesudah kanan dihitung.
    fn tumbuh_bukan_lokal(
        &mut self, s: &crate::parser::AssignStmt, name: &str, operan: &[&Expr], env: &mut Env,
    ) -> Result<Option<Value>, Signal> {
        let Some(kiri) = env.array_bukan_lokal_polos(name) else { return Ok(None) };
        if matches!(s.op, AssignOp::PlusEq) {
            // `+=` menghitung kanan DULU baru membaca sasarannya, jadi tidak ada yang perlu
            // dibuktikan: sambung langsung ke apa pun yang dipegang binding itu sekarang.
            drop(kiri);
            let mut kanan = Some(self.eval_expr(operan[0], env)?);
            if matches!(kanan, Some(Value::Array(_))) {
                env.with_binding_mut(name, |slot| {
                    let Value::Array(a) = slot else { return };
                    if let Some(Value::Array(b)) = kanan.take() { a.extend(b.ambil()); }
                });
            }
            if let Some(v) = kanan { self.assign_target(&s.targets[0], AssignOp::PlusEq, v, env)?; }
            return Ok(Some(Value::Unit));
        }
        let mut tambahan: Vec<Value> = Vec::new();
        for (i, e) in operan.iter().enumerate() {
            match self.eval_expr(e, env)? {
                Value::Array(b) => tambahan.extend(b.ambil()),
                lain => {
                    // Bukan penyambungan array: aturan `+` biasa untuk sisa rantainya.
                    let mut isi = kiri;
                    isi.extend(tambahan);
                    let (mut acc, mut cadangan) = (Value::Array(isi), None);
                    self.tambah_di_tempat(&mut acc, lain, &mut cadangan).map_err(|e| beri_lokasi_tumbuh(e, s))?;
                    for e in &operan[i + 1..] {
                        let kanan = self.eval_expr(e, env)?;
                        self.tambah_di_tempat(&mut acc, kanan, &mut cadangan).map_err(|e| beri_lokasi_tumbuh(e, s))?;
                    }
                    env.set(name, acc);
                    return Ok(Some(Value::Unit));
                }
            }
        }
        let mut paket = Some((kiri, tambahan));
        env.with_binding_mut(name, |slot| {
            let Value::Array(a) = slot else { return };
            if !paket.as_ref().is_some_and(|(k, _)| a.sama_penyimpanan(k)) { return; }
            if let Some((k, t)) = paket.take() { drop(k); a.extend(t); }
        });
        // Ada yang menulis binding itu selagi kanan dihitung: hasilnya tetap nilai LAMA + kanan.
        if let Some((mut k, t)) = paket { k.extend(t); env.set(name, Value::Array(k)); }
        Ok(Some(Value::Unit))
    }

    /// Satu langkah `acc + kanan` dengan aturan `Expr::Binary`, menyambung di tempat kalau
    /// bisa. Saat gagal `acc` tidak dihabiskan; kalau jenisnya berubah (array + teks),
    /// salinan O(1)-nya disimpan di `cadangan` supaya pemanggil bisa memulihkannya.
    pub(crate) fn tambah_di_tempat(
        &mut self, acc: &mut Value, kanan: Value, cadangan: &mut Option<Value>,
    ) -> Result<(), Signal> {
        if let Some(h) = self.operator_biner_pengguna(&BinaryOp::Add, acc, &kanan)? {
            *acc = h;
            return Ok(());
        }
        match (&mut *acc, kanan) {
            (Value::Str(s), kanan) => {
                use std::fmt::Write;
                if Self::perlu_tampil(&kanan) {
                    let t = self.tampil(&kanan)?;
                    s.push_str(&t);
                } else {
                    write!(s, "{}", kanan).expect("Display Value tidak boleh gagal");
                }
                Ok(())
            }
            (Value::Array(a), Value::Array(b)) => { a.extend(b); Ok(()) }
            (_, kanan) => {
                let lama = std::mem::replace(acc, Value::Nil);
                if cadangan.is_none() { *cadangan = Some(lama.clone()); }
                let (l, r) = self.siapkan_gabung_string(&BinaryOp::Add, lama, kanan)?;
                *acc = self.v_add(l, r)?;
                Ok(())
            }
        }
    }

    /// `jalur = jalur + x` dan `jalur = jalur.push(x)` atas field/elemen dikerjakan sebagai
    /// `jalur += x`, supaya koleksinya tumbuh di tempat. Syaratnya tempat itu masih memegang
    /// nilai yang tadi dibaca: terbukti dari namanya (lokal polos) atau dari penyimpanannya.
    pub(crate) fn coba_tumbuh_jalur(
        &mut self,
        s: &crate::parser::AssignStmt,
        env: &mut Env,
    ) -> Result<Option<Value>, Signal> {
        if s.targets.len() != 1 || s.values.len() != 1 || !matches!(s.op, AssignOp::Eq) { return Ok(None); }
        let tgt = &s.targets[0];
        if matches!(tgt, Expr::Ident(..)) { return Ok(None); }
        let (lhs, rhs, dorong) = match &s.values[0] {
            Expr::Binary { op: BinaryOp::Add, lhs, rhs, .. } => (&**lhs, &**rhs, false),
            Expr::Method { obj, method, args, .. } if method == "push" && args.len() == 1 => (&**obj, &args[0], true),
            _ => return Ok(None),
        };
        let mut nama: Vec<&str> = Vec::new();
        if !jalur_murni(tgt, &mut nama) || !jalur_sama(tgt, lhs) { return Ok(None); }
        if nama.is_empty() { return Ok(None); }
        // Lokal polos yang tidak disebut kanan (akar maupun nama index jalurnya) tidak bisa
        // berubah di antara pembacaan dan penulisan; yang ditangkap closure atau milik modul bisa.
        let polos = nama.iter().all(|n| env.lokal_polos(n)) && {
            let disebut = crate::interpreter::capture::names_mentioned(rhs);
            !nama.iter().any(|n| disebut.contains(*n))
        };

        // Nilai kini dibaca DULU seperti jalur biasa, jadi galat bacanya mendahului kanan.
        let kini = self.eval_expr(lhs, env)?;
        match &kini {
            Value::Array(_) => {}
            Value::Str(_) if polos && !dorong => {}
            _ => return Ok(None),
        }
        let kanan = self.eval_expr(rhs, env)?;
        let kanan = if dorong { Value::Array(vec![kanan].into()) } else { kanan };
        let tempat_sama = match (&kini, &kanan) {
            (Value::Str(_), Value::Str(_)) => true,
            (Value::Array(a), Value::Array(_)) => polos
                || matches!(self.eval_expr(lhs, env), Ok(Value::Array(b)) if a.sama_penyimpanan(&b)),
            _ => false,
        };
        if tempat_sama {
            drop(kini);
            self.assign_target(tgt, AssignOp::PlusEq, kanan, env)?;
            return Ok(Some(Value::Unit));
        }
        // Bukan penyambungan murni, atau tempatnya sudah ditulis: aturan `+` biasa atas nilai
        // yang tadi dibaca, dengan lokasi ekspresinya.
        let sp = s.values[0].span();
        let hasil = self.apply_assign_op(AssignOp::PlusEq, kini, kanan).map_err(|e| match e {
            Signal::RuntimeError(r) => Signal::RuntimeError(r.fill_span(sp).isi_span_bingkai(sp)),
            lain => lain,
        })?;
        self.assign_target(tgt, AssignOp::Eq, hasil, env)?;
        Ok(Some(Value::Unit))
    }

    /// `x = x.m(..)` (dan rantainya) atas struct milik lokal polos: nilai `x` DIPINDAHKAN ke
    /// method sebagai `self`, jadi tulisan ke `self` di sana tidak menyalin seluruh isinya.
    pub(crate) fn coba_konsumsi_penerima(
        &mut self,
        s: &crate::parser::AssignStmt,
        env: &mut Env,
    ) -> Result<Option<Value>, Signal> {
        if s.targets.len() != 1 || s.values.len() != 1 || !matches!(s.op, AssignOp::Eq) { return Ok(None); }
        let Expr::Ident(name, _) = &s.targets[0] else { return Ok(None) };
        match self.nilai_konsumsi(name, &s.values[0], env)? {
            Some(v) => { env.set(name, v); Ok(Some(Value::Unit)) }
            None => Ok(None),
        }
    }

    /// Nilai `ekspr` dengan lokal `name` DIPINDAHKAN ke dalamnya (penerima rantai method, atau
    /// argumen pertama sebuah panggilan). `None`: bentuknya bukan itu, dan tidak ada yang disentuh.
    pub(crate) fn nilai_konsumsi(&mut self, name: &str, ekspr: &Expr, env: &mut Env) -> Result<Option<Value>, Signal> {
        if let Expr::Call { .. } = ekspr { return self.coba_konsumsi_argumen(name, ekspr, env); }
        let mut rantai: Vec<&Expr> = Vec::new();
        let mut kini = ekspr;
        loop {
            match kini {
                Expr::Method { obj, .. } => { rantai.push(kini); kini = obj; }
                Expr::Ident(n, _) if n == name && !rantai.is_empty() => break,
                _ => return Ok(None),
            }
        }
        rantai.reverse();
        // Hanya struct ber-method pemakai; array dan teks punya jalurnya sendiri di atas.
        let Expr::Method { method: m0, args: a0, .. } = rantai[0] else { return Ok(None) };
        match env.lokal_polos_struct(name) {
            Some(t) if self.resolusi_metode(t, m0).is_some() => {}
            _ => return Ok(None),
        }
        // Argumen tidak boleh menyebut `x`: mereka dihitung selagi bindingnya kosong.
        for e in &rantai {
            let Expr::Method { args, .. } = e else { continue };
            if args.iter().any(|a| crate::interpreter::capture::names_mentioned(a).contains(name)) {
                return Ok(None);
            }
        }
        // Argumen panggilan PERTAMA dihitung sebelum binding dilepas: kalau ia gagal,
        // binding belum tersentuh.
        let mut av = Vec::with_capacity(a0.len());
        for a in a0 { av.push(self.eval_expr(a, env)?); }
        let Some(mut nilai) = env.ambil_lokal_polos(name) else { return Ok(None) };
        for (i, e) in rantai.iter().enumerate() {
            let Expr::Method { method, args, .. } = e else { unreachable!("rantai cuma berisi method") };
            let argumen = if i == 0 { std::mem::take(&mut av) } else {
                let mut v = Vec::with_capacity(args.len());
                for a in args { v.push(self.eval_expr(a, env)?); }
                v
            };
            nilai = match self.eval_method(nilai, method, argumen) {
                Ok(v) => v,
                Err(Signal::RuntimeError(g)) => return Err(Signal::RuntimeError(
                    g.fill_span(e.span()).isi_span_bingkai(e.span()))),
                Err(lain) => return Err(lain),
            };
        }
        Ok(Some(nilai))
    }

    /// `a, b = e`: `e` harus tuple berisi persis sebanyak sasarannya. Kalau salah satu sasaran
    /// adalah lokal yang juga penerima/argumen pertama `e` (`q, v = q.pop()`), ia dipindahkan.
    pub(crate) fn exec_bongkar(&mut self, s: &crate::parser::AssignStmt, env: &mut Env) -> EvalResult {
        let ekspr = &s.values[0];
        let mut nilai = None;
        for t in &s.targets {
            let Expr::Ident(n, _) = t else { continue };
            if let Some(v) = self.nilai_konsumsi(n, ekspr, env)? { nilai = Some(v); break; }
        }
        let nilai = match nilai { Some(v) => v, None => self.eval_expr(ekspr, env)? };
        let isi = match nilai {
            Value::Tuple(isi) if isi.len() == s.targets.len() => isi.ambil(),
            Value::Tuple(isi) => return Err(Signal::RuntimeError(RuntimeError::new(
                crate::interpreter::pesan_bongkar_jumlah(s.targets.len(), isi.len())).fill_span(ekspr.span()))),
            lain => return Err(Signal::RuntimeError(RuntimeError::new(
                crate::interpreter::pesan_bongkar_bukan_tuple(s.targets.len(), &lain.type_name())).fill_span(ekspr.span()))),
        };
        for (tgt, v) in s.targets.iter().zip(isi) {
            self.assign_target(tgt, AssignOp::Eq, v, env)?;
        }
        Ok(Value::Unit)
    }

    /// `x = f(x, ..)`: argumen pertama DIPINDAHKAN ke pemanggilan — `d = dict::set(d, k, v)`
    /// menyisip di tempat, dan fungsi pemakai menerima nilai yang tinggal ia pegang sendiri.
    fn coba_konsumsi_argumen(&mut self, name: &str, panggil: &Expr, env: &mut Env) -> Result<Option<Value>, Signal> {
        let Expr::Call { func, args, .. } = panggil else { return Ok(None) };
        if !matches!(&**func, Expr::Ident(..) | Expr::PathExpr { .. }) { return Ok(None); }
        if !matches!(args.first(), Some(Expr::Ident(n, _)) if n == name) { return Ok(None); }
        let menyebut = |e: &Expr| crate::interpreter::capture::names_mentioned(e).contains(name);
        if menyebut(func) || args[1..].iter().any(menyebut) { return Ok(None); }
        if !env.lokal_polos(name) { return Ok(None); }
        // Cuma fungsi pemakai dan dua pengubah dict; sisanya punya jalurnya sendiri di atas.
        let fv = self.eval_expr(func, env)?;
        let layak = match &fv {
            Value::Function(_) => true,
            Value::NativeFn(k) => matches!(k,
                crate::interpreter::builtin::NativeFnKind::DictSet | crate::interpreter::builtin::NativeFnKind::DictRemove),
            _ => false,
        };
        if !layak { return Ok(None); }
        let mut av = Vec::with_capacity(args.len());
        av.push(Value::Nil);
        for a in &args[1..] { av.push(self.eval_expr(a, env)?); }
        let Some(nilai) = env.ambil_lokal_polos(name) else { return Ok(None) };
        av[0] = nilai;
        let hasil = match self.call_fn_dari(fv, av, Some(func)) {
            Ok(v) => v,
            Err(Signal::RuntimeError(g)) => return Err(Signal::RuntimeError(
                g.fill_span(panggil.span()).isi_span_bingkai(panggil.span()))),
            Err(lain) => return Err(lain),
        };
        Ok(Some(hasil))
    }

    /// Jalur cepat untuk `a = <transformer>(a, ...)` dan `a = a.<m>(...)`.
    pub(crate) fn coba_ubah_di_tempat(
        &mut self,
        s: &crate::parser::AssignStmt,
        env: &mut Env,
    ) -> Result<Option<Value>, Signal> {
        use crate::interpreter::builtin::{NativeFnKind, Pasti};

        if s.targets.len() != 1 || s.values.len() != 1 { return Ok(None); }
        if !matches!(s.op, AssignOp::Eq) { return Ok(None); }
        let Expr::Ident(name, _) = &s.targets[0] else { return Ok(None) };

        // `pop` mengembalikan `(array_baru, nilai)`, jadi satu-satunya cara wajar
        // memakainya adalah `a = arr::pop(a)[0]`.
        let (inti, indeks): (&Expr, Option<&Expr>) = match &s.values[0] {
            Expr::Index { obj, idx, .. } => (&**obj, Some(&**idx)),
            lain => (lain, None),
        };

        // Bentuk dinormalkan jadi (operasi, ekspresi argumen tambahan).
        let (op, arg_expr): (Pasti, Option<&Expr>) = match inti {
            Expr::Call { func, args, .. } if !args.is_empty() => {
                let Expr::Ident(n0, _) = &args[0] else { return Ok(None) };
                if n0 != name { return Ok(None); }
                // Identitas fungsi diambil dari NILAI callee, bukan teks namanya:
                // `arr::push` yang dibayangi definisi pemakai tidak boleh ikut tersambar …
                let nama_fn = match &**func {
                    Expr::Ident(n, _) => n.clone(),
                    Expr::PathExpr { segments, .. } => segments.join("::"),
                    _ => return Ok(None),
                };
                let Some(Value::NativeFn(kind)) = env.get(&nama_fn) else { return Ok(None) };
                match (kind, args.len()) {
                    (NativeFnKind::ArrPush,    2) => (Pasti::Push,    Some(&args[1])),
                    (NativeFnKind::ArrConcat,  2) => (Pasti::Concat,  Some(&args[1])),
                    (NativeFnKind::ArrPop,     1) => (Pasti::Pop,     None),
                    (NativeFnKind::ArrReverse, 1) => (Pasti::Reverse, None),
                    _ => return Ok(None),
                }
            }
            Expr::Method { obj, method, args, .. } => {
                let Expr::Ident(n0, _) = &**obj else { return Ok(None) };
                if n0 != name { return Ok(None); }
                match (method.as_str(), args.len()) {
                    ("push",    1) => (Pasti::Push,    Some(&args[0])),
                    ("concat",  1) => (Pasti::Concat,  Some(&args[0])),
                    ("pop",     0) => (Pasti::Pop,     None),
                    ("reverse", 0) => (Pasti::Reverse, None),
                    _ => return Ok(None),
                }
            }
            _ => return Ok(None),
        };

        // Argumen tambahan dan ekspresi indeks tidak boleh menyebut `a`:
        // keduanya ikut dihitung di sekitar saat bindingnya dikosongkan.
        for e in [arg_expr, indeks].into_iter().flatten() {
            if crate::interpreter::capture::names_mentioned(e).contains(name.as_str()) {
                return Ok(None);
            }
        }

        // Hanya untuk array; sisanya dijawab jalur biasa. Pemegang kedua milik lokal polos
        // dilepas DI SINI (ia dikosongkan nanti); binding lain dipegang berdua dan diubah di
        // tempat hanya kalau penyimpanannya masih yang sama.
        let kini = match env.get(name) {
            Some(Value::Array(k)) => if env.lokal_polos(name) { None } else { Some(k) },
            _ => return Ok(None),
        };

        // Argumen dievaluasi SEBELUM binding dilepas. Urutannya penting:
        // kalau ia gagal, binding belum tersentuh sama sekali.
        let arg_nilai = match arg_expr {
            Some(e) => Some(self.eval_expr(e, env)?),
            None => None,
        };
        let indeks_nilai = match indeks {
            Some(e) => Some(self.eval_expr(e, env)?),
            None => None,
        };
        // `concat` cuma tanpa-gagal kalau argumennya memang array.
        if matches!(op, Pasti::Concat) && !matches!(arg_nilai, Some(Value::Array(_))) {
            return Ok(None);
        }

        // Operasinya sendiri tidak bisa gagal, jadi tidak ada cadangan yang
        // perlu disiapkan — dan justru ketiadaan cadangan itulah yang
        // membuat pemegangnya tinggal satu.
        let hasil = match kini {
            None => {
                let Some(Value::Array(isi)) = env.ambil_lokal_polos(name) else {
                    unreachable!("lokal polos berisi array sudah dipastikan di atas")
                };
                Self::arr_pasti(op, isi, arg_nilai)
            }
            Some(kini) => {
                let mut paket = Some((kini, arg_nilai));
                let di_tempat = env.with_binding_mut(name, |slot| {
                    let sama = matches!((&*slot, &paket), (Value::Array(a), Some((k, _))) if a.sama_penyimpanan(k));
                    if !sama { return None; }
                    let (k, arg) = paket.take()?;
                    drop(k);
                    let Value::Array(isi) = std::mem::replace(slot, Value::Nil) else { unreachable!("baru saja dicocokkan") };
                    let h = Self::arr_pasti(op, isi, arg);
                    *slot = h.clone();
                    Some(h)
                }).flatten();
                match (di_tempat, paket) {
                    (Some(h), _) => h,
                    (None, Some((k, arg))) => Self::arr_pasti(op, k, arg),
                    (None, None) => unreachable!("paket cuma diambil di jalur di-tempat"),
                }
            }
        };

        // Pengindeksan BISA gagal (mis. `[9]` pada tuple 2-elemen).
        match indeks_nilai {
            None => { env.set(name, hasil); }
            Some(i) => match self.eval_index(hasil.clone(), i) {
                Ok(v)  => { env.set(name, v); }
                Err(e) => { env.set(name, hasil); return Err(e); }
            },
        }
        Ok(Some(Value::Unit))
    }

}


/// Galat `+` dari jalur tumbuh menunjuk tempat yang sama dengan jalur biasa: ekspresinya untuk
/// `a = a + x`, sasarannya untuk `a += x`.
fn beri_lokasi_tumbuh(e: Signal, s: &crate::parser::AssignStmt) -> Signal {
    let sp = if matches!(s.op, AssignOp::PlusEq) { s.targets[0].span() } else { s.values[0].span() };
    match e {
        Signal::RuntimeError(r) => Signal::RuntimeError(r.fill_span(sp).isi_span_bingkai(sp)),
        lain => lain,
    }
}

/// Jalur tanpa efek samping: nama, `.field`, dan `[literal]`/`[nama]`. Nama yang dipakai
/// (akar lebih dulu) dikumpulkan ke `nama`.
pub(crate) fn jalur_murni<'a>(e: &'a Expr, nama: &mut Vec<&'a str>) -> bool {
    match e {
        Expr::Ident(n, _) => { nama.insert(0, n); true }
        Expr::Field { obj, .. } => jalur_murni(obj, nama),
        Expr::Index { obj, idx, .. } => match &**idx {
            Expr::Literal(LitKind::Int(_) | LitKind::Str(_), _) => jalur_murni(obj, nama),
            Expr::Ident(n, _) => { nama.push(n); jalur_murni(obj, nama) }
            _ => false,
        },
        _ => false,
    }
}

/// Dua jalur murni menunjuk tempat yang sama.
pub(crate) fn jalur_sama(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Ident(x, _), Expr::Ident(y, _)) => x == y,
        (Expr::Field { obj: p, field: f, .. }, Expr::Field { obj: q, field: g, .. }) => f == g && jalur_sama(p, q),
        (Expr::Index { obj: p, idx: i, .. }, Expr::Index { obj: q, idx: j, .. }) => {
            let sama = match (&**i, &**j) {
                (Expr::Literal(LitKind::Int(x), _), Expr::Literal(LitKind::Int(y), _)) => x == y,
                (Expr::Literal(LitKind::Str(x), _), Expr::Literal(LitKind::Str(y), _)) => x == y,
                (Expr::Ident(x, _), Expr::Ident(y, _)) => x == y,
                _ => false,
            };
            sama && jalur_sama(p, q)
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Free helpers for range-pattern bounds (`0..10`, `'a'..='z'`, `0.0..1.0`)
// ---------------------------------------------------------------------------

/// Apakah salah satu batas pola rentang ini literal rune?
pub(crate) fn pola_rentang_rune(lo: &Option<Box<Pattern>>, hi: &Option<Box<Pattern>>) -> bool {
    let rune = |p: &Option<Box<Pattern>>| matches!(p.as_deref(),
        Some(Pattern::Literal { lit: LitKind::Rune(_), .. }));
    rune(lo) || rune(hi)
}

/// Bilangan bulat lawan batas angka (bulat atau pecahan), seperti `<` atas `5` dan `9.0`.
pub(crate) fn banding_batas(n: i128, p: &Pattern) -> Option<std::cmp::Ordering> {
    match p {
        Pattern::Literal { lit: LitKind::Int(a), .. } => Some(n.cmp(&(*a as i128))),
        Pattern::Literal { lit: LitKind::Float(a), .. } => (n as f64).partial_cmp(a),
        _ => None,
    }
}

pub(crate) fn pattern_bound_i128(p: &Pattern) -> Option<i128> {
    match p {
        Pattern::Literal { lit: LitKind::Int(a), .. }  => Some(*a as i128),
        Pattern::Literal { lit: LitKind::Rune(c), .. } => Some(*c as i128),
        _ => None,
    }
}

/// Reads a literal range-pattern bound as `f64`, accepting float literals
/// directly and widening integer literals (so `0..1.5` works the same as
/// `0.0..1.5`). Returns `None` for any other literal kind.
pub(crate) fn pattern_bound_f64(p: &Pattern) -> Option<f64> {
    match p {
        Pattern::Literal { lit: LitKind::Float(a), .. } => Some(*a),
        Pattern::Literal { lit: LitKind::Int(a), .. }    => Some(*a as i128 as f64),
        _ => None,
    }
}

/// Panjang array/teks yang sedang ditumbuhkan, untuk memulihkannya kalau gagal.
pub(crate) fn panjang_koleksi(v: &Value) -> usize {
    match v { Value::Str(s) => s.len(), Value::Array(a) => a.len(), _ => 0 }
}

/// Potong kembali ke panjang awal: penyambungan di tempat cuma MENAMBAH di ujung.
pub(crate) fn pulihkan_koleksi(v: Value, awal: usize) -> Value {
    match v {
        Value::Str(mut s) => { s.truncate(awal); Value::Str(s) }
        Value::Array(mut a) => { a.truncate(awal); Value::Array(a) }
        lain => lain,
    }
}
