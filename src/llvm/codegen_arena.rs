// src/llvm/codegen_arena.rs — arena yang membebaskan (antrean B butir 5, langkah 2): badan loop yang terbukti tidak
// meloloskan nilai berpenyangga memulihkan kursor arena di akhir tiap putaran (`jrg_arena_simpan`/`jrg_arena_pulih`).
// Analisisnya sengaja sempit; yang tidak dikenalnya menolak. Rancangan: dokumentasi/src/llvm/arena-rancangan.md.

use crate::parser::*;

use super::codegen::Codegen;
use super::types::JrgType;

/// Bawaan murni yang boleh dipanggil di badan yang dipulihkan: tidak menyimpan apa pun di luar nilainya sendiri.
const BAWAAN_MURNI: &[&str] = &["len", "str", "int", "float", "uint", "rune", "typeof", "wok", "wok_"];
const RUANG_MURNI: &[&str] = &["math", "str"];
/// Bawaan yang MENAMPILKAN argumennya (bisa memanggil `to_string` milik program).
const MERENDER: &[&str] = &["str", "wok", "wok_", "str::format", "str::pad_left", "str::pad_right", "str::center",
    "str::ljust", "str::rjust", "str::to_string"];
/// Method milik program yang dipanggil operator atau tampilan (`interpreter::operator`, hook `to_string`).
const HOOK: &[&str] = &["to_string", "add", "sub", "mul", "div", "rem", "pow", "bit_and", "bit_or", "bit_xor", "shl",
    "shr", "neg", "not", "bit_not", "eq", "cmp"];

impl Codegen {
    fn skalar(t: &JrgType) -> bool {
        t.is_numeric() || matches!(t, JrgType::Bool | JrgType::Rune | JrgType::Unit)
    }

    /// Nama yang terlihat DARI LUAR badan (dicek sebelum badannya diturunkan): lokal yang sudah ada, atau `var` modul.
    fn tipe_luar(&self, n: &str) -> Option<JrgType> {
        self.lookup_local(n).map(|(_, t)| t).or_else(|| self.var_global.get(&self.k(n)).map(|(_, t)| t.clone()))
    }

    /// Apakah badan loop ini boleh dipulihkan tiap putaran? Tulisan ke nama luar cuma skalar, panggilan cuma bawaan
    /// murni atau fungsi milik program yang lolos aturan yang sama, tanpa closure/goroutine/channel/`defer`.
    pub(crate) fn badan_bisa_dipulihkan(&mut self, b: &Block) -> bool {
        // IR membaca `jrg_arena` sebagai `[4 x i64]`: cuma di sasaran berpenunjuk 8 bita.
        if self.options.ptr32 || self.types.lebar_ptr != 8 || std::env::var_os("JRGC_TANPA_PULIH_ARENA").is_some() { return false; }
        // Badan yang tidak mungkin mengalokasi (hitungan angka, `char_at`) tidak dibayar dua panggilan per putaran.
        self.blok_mengalokasi(b) && self.blok_bersih(b, true)
    }

    /// Apakah badan ini MUNGKIN mengalokasi di arena? Yang pasti tidak: angka, perbandingan, baca indeks, `len`,
    /// `str::char_at`, `math::`; selain itu (dan yang tidak dikenal) dianggap mengalokasi.
    fn blok_mengalokasi(&self, b: &Block) -> bool {
        b.stmts.iter().any(|s| self.stmt_mengalokasi(s)) || b.tail.as_ref().is_some_and(|e| self.expr_mengalokasi(e))
    }

    fn stmt_mengalokasi(&self, s: &Stmt) -> bool {
        match s {
            Stmt::Let(l) => self.expr_mengalokasi(&l.init),
            Stmt::Var(v) => v.init.as_ref().is_some_and(|e| self.expr_mengalokasi(e)),
            Stmt::ShortDecl(d) => d.values.iter().any(|e| self.expr_mengalokasi(e)),
            Stmt::Expr(e) => self.expr_mengalokasi(e),
            Stmt::Assign(a) => a.values.iter().chain(a.targets.iter()).any(|e| self.expr_mengalokasi(e))
                || !matches!(a.op, AssignOp::Eq | AssignOp::PlusEq | AssignOp::MinusEq | AssignOp::StarEq)
                || a.targets.iter().any(|t| !matches!(t, Expr::Ident(..)) && !self.tulis_elemen_di_tempat(t)),
            Stmt::If(i) => self.expr_mengalokasi(&i.cond) || self.blok_mengalokasi(&i.then)
                || i.init.is_some() || i.else_ifs.iter().any(|(init, c, b)| init.is_some() || self.expr_mengalokasi(c) || self.blok_mengalokasi(b))
                || i.else_.as_ref().is_some_and(|b| self.blok_mengalokasi(b)),
            Stmt::Break(_) | Stmt::Continue(_) => false,
            Stmt::Block(b) => self.blok_mengalokasi(b),
            Stmt::While(w) => self.expr_mengalokasi(&w.cond) || self.blok_mengalokasi(&w.body),
            Stmt::Loop(l) => self.blok_mengalokasi(&l.body),
            Stmt::For(ForStmt::Range { iter, body, .. }) =>
                !matches!(&**iter, Expr::Range { .. }) || self.expr_mengalokasi(iter) || self.blok_mengalokasi(body),
            Stmt::For(ForStmt::Classic { init, cond, post, body, .. }) =>
                init.as_ref().is_some_and(|s| self.stmt_mengalokasi(s)) || cond.as_ref().is_some_and(|e| self.expr_mengalokasi(e))
                    || post.as_ref().is_some_and(|s| self.stmt_mengalokasi(s)) || self.blok_mengalokasi(body),
            Stmt::Return(r) => r.values.iter().any(|e| self.expr_mengalokasi(e)),
            _ => true,
        }
    }

    /// `a[i] = skalar` atas larik/slice skalar yang terlihat dan tidak berbagi penyangga: ditulis di tempat.
    fn tulis_elemen_di_tempat(&self, t: &Expr) -> bool {
        let Expr::Index { obj, .. } = t else { return false };
        let Expr::Ident(n, _) = &**obj else { return false };
        match self.lookup_local(n) {
            Some((_, JrgType::Array(e, _))) => Self::skalar(&e),
            Some((slot, JrgType::Slice(e))) => Self::skalar(&e) && !self.milik_lokal.contains_key(&slot),
            _ => false,
        }
    }

    fn expr_mengalokasi(&self, e: &Expr) -> bool {
        // Konversi tertulis ke tipe dasar (`x as uint`) skalar walau penebak tipe tidak menjawabnya.
        let skalar = |x: &Expr| matches!(x, Expr::Literal(LitKind::Int(_) | LitKind::Float(_) | LitKind::Bool(_) | LitKind::Rune(_), _))
            || matches!(x, Expr::Cast { ty, .. } if self.tipe_dari_anotasi(ty, &Default::default()).is_some_and(|t| Self::skalar(&t)))
            || self.peek_expr_type(x).is_some_and(|t| Self::skalar(&t));
        match e {
            Expr::Literal(LitKind::Str(_) | LitKind::RawStr(_), _) => false,
            Expr::Literal(..) | Expr::Ident(..) => false,
            Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => self.expr_mengalokasi(expr),
            // Salah menjawab "tidak" cuma melewatkan pemulihan (aman), jadi operan yang tipenya belum diketahui (lokal
            // badan) dianggap angka; yang PASTI teks atau berpenyangga dianggap mengalokasi.
            Expr::Binary { lhs, rhs, .. } => {
                let berisi = |x: &Expr| matches!(x, Expr::Literal(LitKind::Str(_) | LitKind::RawStr(_), _))
                    || (!skalar(x) && self.peek_expr_type(x).is_some());
                berisi(lhs) || berisi(rhs) || self.expr_mengalokasi(lhs) || self.expr_mengalokasi(rhs)
            }
            Expr::Index { obj, idx, .. } => self.expr_mengalokasi(obj) || self.expr_mengalokasi(idx),
            Expr::Field { obj, .. } => self.expr_mengalokasi(obj),
            // `if`/`match`/blok di posisi pernyataan diurai sebagai ekspresi.
            Expr::If(i) => self.stmt_mengalokasi(&Stmt::If((**i).clone())),
            Expr::Block(b) => self.blok_mengalokasi(b),
            Expr::Match { scrutinee, arms, .. } => self.expr_mengalokasi(scrutinee) || arms.iter().any(|a|
                a.guard.as_ref().is_some_and(|g| self.expr_mengalokasi(g)) || match &a.body {
                    MatchBody::Expr(e) => self.expr_mengalokasi(e),
                    MatchBody::Block(b) => self.blok_mengalokasi(b),
                }),
            Expr::Call { func, args, .. } => {
                let tanpa = match &**func {
                    Expr::Ident(n, _) => matches!(n.as_str(), "len" | "int" | "float" | "uint" | "rune")
                        && self.lookup_local(n).is_none() && !self.tables.functions.contains_key(&self.k(n)),
                    Expr::PathExpr { segments, .. } => matches!(super::infer::tanpa_awalan_bawaan(segments),
                        [ns, it] if (ns == "math" || (ns == "str" && it == "char_at")) && self.unit_alias(ns).is_none()),
                    _ => false,
                };
                !tanpa || args.iter().any(|a| self.expr_mengalokasi(a))
            }
            _ => true,
        }
    }

    /// Program ini punya method yang dipanggil operator atau tampilan? Tanpa itu, operator dan tampilan tidak bisa
    /// menjalankan kode program apa pun tipe operannya.
    fn ada_hook(&self) -> bool {
        self.tables.impls.values().any(|m| m.keys().any(|k| HOOK.contains(&k.as_str())))
    }

    fn blok_bersih(&mut self, b: &Block, luar_terlihat: bool) -> bool {
        b.stmts.iter().all(|s| self.stmt_bersih(s, luar_terlihat))
            && b.tail.as_ref().map_or(true, |e| self.expr_bersih(e))
    }

    fn stmt_bersih(&mut self, s: &Stmt, luar: bool) -> bool {
        match s {
            Stmt::Let(l) => self.expr_bersih(&l.init),
            Stmt::Var(v) => v.init.as_ref().map_or(true, |e| self.expr_bersih(e)),
            Stmt::Const(c) => self.expr_bersih(&c.value),
            Stmt::ShortDecl(d) => d.values.iter().all(|e| self.expr_bersih(e)),
            Stmt::Expr(e) => self.expr_bersih(e),
            Stmt::Assign(a) => {
                a.targets.iter().all(|t| self.sasaran_bersih(t, luar))
                    && a.values.iter().all(|e| self.expr_bersih(e))
            }
            Stmt::If(i) => self.if_bersih(i, luar),
            Stmt::For(ForStmt::Classic { init, cond, post, body, .. }) => {
                init.as_ref().map_or(true, |s| self.stmt_bersih(s, luar))
                    && cond.as_ref().map_or(true, |e| self.expr_bersih(e))
                    && post.as_ref().map_or(true, |s| self.stmt_bersih(s, luar))
                    && self.blok_bersih(body, luar)
            }
            Stmt::For(ForStmt::Range { iter, body, .. }) => self.expr_bersih(iter) && self.blok_bersih(body, luar),
            Stmt::While(w) => self.expr_bersih(&w.cond) && self.blok_bersih(&w.body, luar),
            Stmt::Loop(l) => self.blok_bersih(&l.body, luar),
            Stmt::Match(m) => self.expr_bersih(&m.scrutinee) && m.arms.iter().all(|a| self.lengan_bersih(a, luar)),
            Stmt::Return(r) => r.values.iter().all(|e| self.expr_bersih(e)),
            Stmt::Break(_) | Stmt::Continue(_) => true,
            Stmt::Block(b) => self.blok_bersih(b, luar),
            Stmt::Spawn(_) | Stmt::Select(_) | Stmt::Defer(_) | Stmt::Fn(_) | Stmt::Error(_) => false,
        }
    }

    fn if_bersih(&mut self, i: &IfStmt, luar: bool) -> bool {
        i.init.as_ref().map_or(true, |s| self.stmt_bersih(s, luar))
            && self.expr_bersih(&i.cond) && self.blok_bersih(&i.then, luar)
            && i.else_ifs.iter().all(|(init, c, b)| init.as_ref().map_or(true, |s| self.stmt_bersih(s, luar))
                && self.expr_bersih(c) && self.blok_bersih(b, luar))
            && i.else_.as_ref().map_or(true, |b| self.blok_bersih(b, luar))
    }

    fn lengan_bersih(&mut self, a: &MatchArm, luar: bool) -> bool {
        a.guard.as_ref().map_or(true, |g| self.expr_bersih(g)) && match &a.body {
            MatchBody::Expr(e) => self.expr_bersih(e),
            MatchBody::Block(b) => self.blok_bersih(b, luar),
        }
    }

    /// Tulisan ke nama yang terlihat dari luar: cuma `x = ..`/`x += ..` atas skalar. Nama dalam badan bebas ditulis.
    fn sasaran_bersih(&mut self, t: &Expr, luar: bool) -> bool {
        let akar = |e: &Expr| -> Option<String> {
            let mut e = e;
            loop {
                match e {
                    Expr::Ident(n, _) => return Some(n.clone()),
                    Expr::Field { obj, .. } | Expr::Index { obj, .. } => e = obj,
                    _ => return None,
                }
            }
        };
        let Some(n) = akar(t) else { return false };
        if n == "_" { return true; }
        let indeks_bersih = match t {
            Expr::Index { idx, .. } => self.expr_bersih(idx),
            _ => true,
        };
        let tipe = if luar { self.tipe_luar(&n) } else { self.var_global.get(&self.k(&n)).map(|(_, t)| t.clone()) };
        match tipe {
            None => indeks_bersih,
            Some(ty) if matches!(t, Expr::Ident(..)) => Self::skalar(&ty),
            // `luar[i] = skalar` atas larik skalar menulis di tempat tanpa alokasi. Slice yang masih berbagi
            // penyangga (bendera CoW) disalin saat ditulis — salinannya lahir sesudah titik simpan, jadi tidak.
            Some(ty) => match (t, &ty) {
                (Expr::Index { obj, .. }, JrgType::Array(e, _)) if matches!(**obj, Expr::Ident(..)) => Self::skalar(e) && indeks_bersih,
                (Expr::Index { obj, .. }, JrgType::Slice(e)) if matches!(**obj, Expr::Ident(..)) =>
                    Self::skalar(e) && indeks_bersih
                        && self.lookup_local(&n).is_some_and(|(slot, _)| !self.milik_lokal.contains_key(&slot)),
                _ => false,
            },
        }
    }

    fn expr_bersih(&mut self, e: &Expr) -> bool {
        match e {
            Expr::Literal(..) | Expr::Ident(..) | Expr::Range { .. } => true,
            Expr::Unary { expr, .. } | Expr::Cast { expr, .. } | Expr::Field { obj: expr, .. } => self.expr_bersih(expr),
            Expr::Binary { lhs, rhs, .. } => self.expr_bersih(lhs) && self.expr_bersih(rhs)
                && (!self.ada_hook() || (self.tampil_polos(lhs) && self.tampil_polos(rhs))),
            Expr::Ternary { cond, then, else_, .. } => self.expr_bersih(cond) && self.expr_bersih(then) && self.expr_bersih(else_),
            Expr::Index { obj, idx, .. } => self.expr_bersih(obj) && self.expr_bersih(idx),
            Expr::Slice { obj, lo, hi, .. } => self.expr_bersih(obj)
                && lo.as_ref().map_or(true, |x| self.expr_bersih(x)) && hi.as_ref().map_or(true, |x| self.expr_bersih(x)),
            Expr::Array { elems, .. } | Expr::Tuple { elems, .. } => elems.iter().all(|x| self.expr_bersih(x)),
            Expr::Block(b) => self.blok_bersih(b, true),
            Expr::If(i) => self.if_bersih(i, true),
            Expr::Match { scrutinee, arms, .. } => self.expr_bersih(scrutinee) && arms.iter().all(|a| self.lengan_bersih(a, true)),
            // Templat dan `str`/`wok` memanggil hook tampilan milik tipe: cuma skalar dan teks.
            Expr::Template { parts, .. } => parts.iter().all(|p| match p {
                TemplatePart::Text(_) => true,
                TemplatePart::Expr(x) => self.expr_bersih(x) && self.tampil_polos(x),
            }),
            Expr::Method { obj, method, args, .. } => {
                let tipe = self.peek_expr_type(obj);
                // Method milik program atas struct: diperiksa seperti fungsi milik program (`self` nama dalam).
                if let Some(JrgType::Struct(nama)) = &tipe {
                    let f = self.tables.impls.get(nama).and_then(|m| m.get(method)).cloned();
                    return match f {
                        Some(f) => self.expr_bersih(obj) && args.iter().all(|x| self.expr_bersih(x))
                            && self.fungsi_bersih(&format!("{}::{}", nama, method), &f),
                        None => false,
                    };
                }
                if tipe.is_none() {
                    // Penerima lokal badan (tipenya belum diketahui): tiap method program bernama sama harus bersih;
                    // tanpa itu ia method bawaan, murni kecuali yang merender di program yang punya hook.
                    let calon: Vec<(String, std::rc::Rc<FnDecl>)> = self.tables.impls.iter()
                        .filter_map(|(t, m)| m.get(method).map(|f| (format!("{}::{}", t, method), f.clone()))).collect();
                    let dasar = self.expr_bersih(obj) && args.iter().all(|x| self.expr_bersih(x) && self.tampil_polos(x));
                    if calon.is_empty() {
                        return dasar && (!self.ada_hook() || !matches!(method.as_str(), "to_string" | "format" | "pad_left" | "pad_right" | "center"));
                    }
                    return dasar && calon.iter().all(|(n, f)| self.fungsi_bersih(n, f));
                }
                let polos = tipe.is_some_and(|t| Self::skalar(&t) || t == JrgType::Str);
                polos && self.expr_bersih(obj) && args.iter().all(|x| self.expr_bersih(x) && self.tampil_polos(x))
            }
            Expr::Struct { fields, spread: None, .. } => fields.iter().all(|f| self.expr_bersih(&f.value)),
            Expr::Call { func, args, .. } => self.panggilan_bersih(func, args),
            _ => false,
        }
    }

    /// Nilai yang ditampilkan atau dioperasikan tanpa hook milik program: program tanpa hook, atau skalar/teks.
    fn tampil_polos(&self, e: &Expr) -> bool {
        !self.ada_hook() || matches!(e, Expr::Literal(..))
            || self.peek_expr_type(e).is_some_and(|t| Self::skalar(&t) || t == JrgType::Str)
    }

    fn panggilan_bersih(&mut self, func: &Expr, args: &[Expr]) -> bool {
        if !args.iter().all(|a| self.expr_bersih(a)) { return false; }
        match func {
            Expr::Ident(n, _) => {
                if self.lookup_local(n).is_some() { return false; }
                if let Some(f) = self.tables.functions.get(&self.k(n)).cloned() { return self.fungsi_bersih(n, &f); }
                BAWAAN_MURNI.contains(&n.as_str())
                    && (!MERENDER.contains(&n.as_str()) || args.iter().all(|a| self.tampil_polos(a)))
            }
            Expr::PathExpr { segments, .. } => {
                let s = super::infer::tanpa_awalan_bawaan(segments);
                let [ns, item] = s else { return false };
                let nama = format!("{}::{}", ns, item);
                RUANG_MURNI.contains(&ns.as_str()) && self.unit_alias(ns).is_none()
                    && (!MERENDER.contains(&nama.as_str()) || args.iter().all(|a| self.tampil_polos(a)))
            }
            _ => false,
        }
    }

    /// Fungsi milik program yang dipanggil dari badan yang dipulihkan: badannya lolos aturan yang sama, dengan semua
    /// parameternya nama dalam (salinan atau pinjaman, tidak pernah ditulis ke luar). Rekursi dianggap bersih.
    fn fungsi_bersih(&mut self, n: &str, f: &FnDecl) -> bool {
        let kunci = self.k(n);
        if let Some(&b) = self.fungsi_bersih_memo.get(&kunci) { return b; }
        self.fungsi_bersih_memo.insert(kunci.clone(), true);
        let ok = match &*f.body {
            FnBody::Block(b) => self.blok_bersih(b, false),
            FnBody::Expr(e) => self.expr_bersih(e),
            FnBody::Abstrak => false,
        };
        self.fungsi_bersih_memo.insert(kunci, ok);
        ok
    }

    /// Awal badan loop yang bersih: kursor dan chunk arena (`jrg_arena` field 0 dan 3) dibaca langsung ke dua slot.
    pub(crate) fn simpan_putaran(&mut self) -> String {
        if self.declared_externs.insert("jrg_arena".to_string()) {
            self.extern_decls.push("@jrg_arena = external global [4 x i64]".to_string());
        }
        let t = self.alloca_raw("titik.", "[2 x i64]", Some(8));
        for (dari, ke) in [(0, 0), (3, 1)] {
            let (p, v, q) = (self.fresh("arp."), self.fresh("arv."), self.fresh("arq."));
            self.emit(format!("{} = getelementptr [4 x i64], ptr @jrg_arena, i64 0, i64 {}", p, dari));
            self.emit(format!("{} = load i64, ptr {}", v, p));
            self.emit(format!("{} = getelementptr [2 x i64], ptr {}, i64 0, i64 {}", q, t, ke));
            self.emit(format!("store i64 {}, ptr {}", v, q));
        }
        t
    }

    /// Ujung putaran: kalau kursor atau chunk bergerak (ada yang dialokasi), kursor kembali ke titik simpan dan kapasitas
    /// lokal yang lahir di badan ini dilupakan (alamatnya bisa jadi milik penyangga lain sesudah pemulihan).
    pub(crate) fn pulihkan_putaran(&mut self, titik: &str, kap_badan: &[String]) {
        self.ensure_extern("declare void @jrg_arena_pulih_ke(i64, i64) nounwind", "jrg_arena_pulih_ke");
        let mut nilai = Vec::new();
        for (dari, ke) in [(0, 0), (3, 1)] {
            let (p, v, q, s) = (self.fresh("arp."), self.fresh("arv."), self.fresh("arq."), self.fresh("ars."));
            self.emit(format!("{} = getelementptr [4 x i64], ptr @jrg_arena, i64 0, i64 {}", p, dari));
            self.emit(format!("{} = load i64, ptr {}", v, p));
            self.emit(format!("{} = getelementptr [2 x i64], ptr {}, i64 0, i64 {}", q, titik, ke));
            self.emit(format!("{} = load i64, ptr {}", s, q));
            nilai.push((v, s));
        }
        let (b1, b2, b) = (self.fresh("arb."), self.fresh("arb."), self.fresh("arb."));
        self.emit(format!("{} = icmp ne i64 {}, {}", b1, nilai[0].0, nilai[0].1));
        self.emit(format!("{} = icmp ne i64 {}, {}", b2, nilai[1].0, nilai[1].1));
        self.emit(format!("{} = or i1 {}, {}", b, b1, b2));
        let (lambat, usai) = (self.fresh_label("arena.lambat."), self.fresh_label("arena.usai."));
        self.terminate(format!("br i1 {}, label %{}, label %{}, !prof !0", b, lambat, usai));
        self.start_block(lambat);
        self.emit(format!("call void @jrg_arena_pulih_ke(i64 {}, i64 {})", nilai[0].1, nilai[1].1));
        for k in kap_badan { self.emit(format!("store {{ ptr, i64 }} zeroinitializer, ptr {}", k)); }
        self.terminate(format!("br label %{}", usai));
        self.start_block(usai);
    }

    /// Badan loop, dengan pemulihan arena kalau `badan_bisa_dipulihkan`. Dipanggil sesudah `LoopCtx`-nya didorong:
    /// ujung badan dan `continue` lewat satu blok pemulih, yang ditulis terakhir supaya daftar kapasitasnya lengkap.
    pub(crate) fn badan_loop(&mut self, body: &Block) -> Result<(), String> {
        if !self.badan_bisa_dipulihkan(body) { return self.codegen_stmt_block(body); }
        let titik = self.simpan_putaran();
        let pulih = self.fresh_label("arena.pulih.");
        let kap_sebelum: std::collections::HashSet<String> = self.kap_tumbuh.values().cloned().collect();
        let lanjut = match self.loop_stack.last_mut() {
            Some(c) => std::mem::replace(&mut c.continue_bb, pulih.clone()),
            None => return Err("badan_loop tanpa LoopCtx".to_string()),
        };
        let r = self.codegen_stmt_block(body);
        if let Some(c) = self.loop_stack.last_mut() { c.continue_bb = lanjut.clone(); }
        r?;
        if !self.is_terminated() { self.terminate(format!("br label %{}", pulih)); }
        let kap: Vec<String> = self.kap_tumbuh.values().filter(|k| !k.starts_with('@') && !kap_sebelum.contains(*k)).cloned().collect();
        self.start_block(pulih);
        self.pulihkan_putaran(&titik, &kap);
        self.terminate(format!("br label %{}", lanjut));
        Ok(())
    }
}
