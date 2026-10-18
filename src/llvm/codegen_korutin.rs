// src/llvm/codegen_korutin.rs — konkurensi backend LLVM: `spawn`, channel, `select` di atas
// korutin satu utas (`korutin.c`).

use crate::lexer::Span;
use crate::parser::*;

use super::codegen::{Codegen, Val};
use super::types::*;

impl Codegen {
    /// Channel baru ber-elemen `ety`, kapasitas dari `cap` (tanpa `cap`: 0, serah-terima langsung).
    fn chan_baru(&mut self, ety: JrgType, cap: Option<&Expr>) -> Result<Val, String> {
        let cap_v = match cap {
            Some(e) => { let v = self.codegen_expr(e, Some(&JrgType::I64))?; self.coerce_longgar(&v, &JrgType::I64) }
            None => Val { text: "0".into(), ty: JrgType::I64 },
        };
        self.ensure_extern("declare ptr @jrg_chan_new(i64, i64)", "jrg_chan_new");
        let r = self.fresh("chan.");
        self.emit(format!("{} = call ptr @jrg_chan_new(i64 {}, i64 {})", r, cap_v.text, self.types.sizeof(&ety)));
        Ok(Val { text: r, ty: JrgType::Chan(Box::new(ety)) })
    }

    /// `make_chan(T, cap?)`.
    pub fn codegen_make_chan(&mut self, ty: &TypeExpr, cap: &Option<Box<Expr>>, _span: Span) -> Result<Val, String> {
        let ety = self.resolve_type_pub(ty)?;
        self.chan_baru(ety, cap.as_deref())
    }

    /// `Chan::new(cap?)` / `chan::make(cap?)`: channel tanpa tipe elemen, isinya nilai bertag.
    pub(crate) fn chan_bertag(&mut self, args: &[Expr]) -> Result<Val, String> {
        self.chan_baru(JrgType::Any, args.first())
    }

    fn chan_dari(&mut self, chan: &Expr, apa: &str, span: Span) -> Result<(Val, JrgType), String> {
        let cv = self.codegen_expr(chan, None)?;
        match &cv.ty {
            JrgType::Chan(e) => { let e = (**e).clone(); Ok((cv, e)) }
            other => Err(super::galat::tandai(span, format!("{} butuh channel, dapat `{}`", apa, other.nama_typeof()))),
        }
    }

    fn kirim_ke(&mut self, cv: &Val, ety: &JrgType, val: &Expr) -> Result<String, String> {
        let vv = self.codegen_expr(val, Some(ety))?;
        let vv = self.ambil_milik(val, vv)?;
        let vv = self.coerce(&vv, ety);
        if vv.ty != *ety {
            return Err(format!("nilai bertipe `{}` tidak bisa dikirim ke `{}`", vv.ty.display_name(), cv.ty.display_name()));
        }
        Ok(self.spill_to_place(&vv))
    }

    /// `c <- v`.
    pub fn codegen_send(&mut self, chan: &Expr, val: &Expr, span: Span) -> Result<Val, String> {
        let (cv, ety) = self.chan_dari(chan, "`<-` (kirim)", span)?;
        let vp = self.kirim_ke(&cv, &ety, val)?;
        self.ensure_extern("declare void @jrg_chan_send(ptr, ptr, i64)", "jrg_chan_send");
        self.emit(format!("call void @jrg_chan_send(ptr {}, ptr {}, i64 {})", cv.text, vp, self.types.sizeof(&ety)));
        Ok(Val::unit())
    }

    /// `<-c`.
    pub fn codegen_recv(&mut self, chan: &Expr, span: Span) -> Result<Val, String> {
        let (cv, ety) = self.chan_dari(chan, "`<-` (terima)", span)?;
        let slot = self.alloca_typed("recv.", &ety);
        self.ensure_extern("declare void @jrg_chan_recv(ptr, ptr, i64)", "jrg_chan_recv");
        self.emit(format!("call void @jrg_chan_recv(ptr {}, ptr {}, i64 {})", cv.text, slot, self.types.sizeof(&ety)));
        Ok(self.load_place(&slot, &ety))
    }

    /// `spawn { .. }`: badannya jadi closure `move` tanpa hasil, lalu diserahkan ke penjadwal.
    pub fn codegen_spawn(&mut self, s: &SpawnStmt) -> Result<(), String> {
        // `spawn |i, ch| { .. }`: daftar tangkapan yang ditulis — tiap namanya harus sudah ada di sini.
        for p in s.params.iter().flatten() {
            let nama = super::codegen_access::pattern_simple_name_pub(&p.pattern).unwrap_or_default();
            if self.lookup_local(&nama).is_none() {
                return Err(super::galat::tandai(p.span, format!(
                    "`spawn |{}|`: `{}` belum ada di sini — daftar di antara `|..|` menyebut nilai yang DITANGKAP", nama, nama)));
            }
        }
        let mut badan = s.body.clone();
        // Ekor blok dibuang: goroutine tidak punya hasil.
        if let Some(ekor) = badan.tail.take() { badan.stmts.push(Stmt::Expr(ekor)); }
        let f = self.closure_tanpa_hasil(&Expr::Block(badan), s.span)?;
        let kode = self.fresh("spk.");
        self.emit(format!("{} = extractvalue %JrgFn {}, 0", kode, f.text));
        let env = self.fresh("spe.");
        self.emit(format!("{} = extractvalue %JrgFn {}, 1", env, f.text));
        self.ensure_extern("declare void @jrg_ko_spawn(ptr, ptr)", "jrg_ko_spawn");
        self.emit(format!("call void @jrg_ko_spawn(ptr {}, ptr {})", kode, env));
        Ok(())
    }

    /// `Future::new(f, args..)` / `Task::spawn(f, args..)`: `f(args..)` jalan sebagai goroutine
    /// dan hasilnya menunggu di channel sekali-serah.
    pub(crate) fn masa_baru(&mut self, args: &[Expr], tugas: bool, span: Span) -> Result<Val, String> {
        let nama = if tugas { "Task::spawn" } else { "Future::new" };
        let Some((f, sisa)) = args.split_first() else {
            return Err(super::galat::tandai(span, format!("`{}` butuh fungsi yang dijalankan", nama)));
        };
        let panggil = Expr::Call { func: Box::new(f.clone()), args: sisa.to_vec(), span };
        let clo = self.closure_dgn_params(&[], &panggil, &[], true, span)?;
        let JrgType::Fn(_, ret) = clo.ty.clone() else { return Err("closure tugas bukan fungsi".to_string()); };
        let ret = *ret;
        let kode = self.fresh("msk.");
        self.emit(format!("{} = extractvalue %JrgFn {}, 0", kode, clo.text));
        let env = self.fresh("mse.");
        self.emit(format!("{} = extractvalue %JrgFn {}, 1", env, clo.text));
        // Pembungkus per tugas: panggil closure-nya, kirim hasilnya ke channel milik tugas itu.
        self.penerima_lapis += 1;
        let bungkus = format!("masa.{}", self.penerima_lapis);
        let kosong = ret == JrgType::Unit || ret == JrgType::Never;
        let lty = ret.llvm_ty();
        let badan = if kosong {
            "  call void %k(ptr %e)\n  call void @jrg_chan_send(ptr %c, ptr %p, i64 0)\n".to_string()
        } else {
            format!("  %r = call {t} %k(ptr %e)\n  %s = alloca {t}\n  store {t} %r, ptr %s\n  \
                     call void @jrg_chan_send(ptr %c, ptr %s, i64 {n})\n", t = lty, n = self.types.sizeof(&ret))
        };
        self.ensure_extern("declare void @jrg_chan_send(ptr, ptr, i64)", "jrg_chan_send");
        self.tambah_definisi(format!(
            "define internal void @{b}(ptr %p) nounwind {{\nentry:\n  %e = load ptr, ptr %p\n  \
             %pk = getelementptr ptr, ptr %p, i64 1\n  %c = load ptr, ptr %pk\n  \
             %pf = getelementptr ptr, ptr %p, i64 2\n  %k = load ptr, ptr %pf\n{badan}  ret void\n}}\n",
            b = bungkus, badan = badan));
        self.ensure_extern("declare ptr @jrg_masa_baru(ptr, ptr, ptr, i64, i32)", "jrg_masa_baru");
        let r = self.fresh("masa.");
        self.emit(format!("{} = call ptr @jrg_masa_baru(ptr @{}, ptr {}, ptr {}, i64 {}, i32 {})",
            r, bungkus, env, kode, self.types.sizeof(&ret), tugas as i32));
        Ok(Val { text: r, ty: JrgType::Masa(Box::new(ret), tugas) })
    }

    fn masa_dari(&mut self, e: &Expr, nama: &str, span: Span) -> Result<(Val, JrgType), String> {
        let v = self.codegen_expr(e, None)?;
        match &v.ty {
            JrgType::Masa(t, _) => { let t = (**t).clone(); Ok((v, t)) }
            other => Err(super::galat::tandai(span, format!(
                "{}: harus `Future`/`Task`, dapat `{}`", nama, other.nama_typeof()))),
        }
    }

    /// `future::`/`task::` atas sebuah Future/Task; `None` kalau `nama` bukan salah satunya.
    pub(crate) fn coba_builtin_masa(&mut self, nama: &str, args: &[Expr], span: Span) -> Option<Result<Val, String>> {
        // Cuma nama yang ada di acuan (`task::await`, `future::join` tidak) — dulu dua awalan untuk semua (#760).
        if !crate::vm::ada_builtin(nama) { return None; }
        let item = nama.strip_prefix("future::").or_else(|| nama.strip_prefix("task::"))?;
        let aritas = match item { "await" | "join" | "is_done" | "try_get" | "all" | "race" => 1, "timeout" => 2, _ => return None };
        if args.len() != aritas { return None; }
        Some((|| {
            let teks = self.intern_string(nama);
            let (np, nl) = self.split_str(&teks);
            if item == "all" || item == "race" {
                return self.masa_banyak(&args[0], item == "race", nama, &np, &nl, span);
            }
            let (m, t) = self.masa_dari(&args[0], nama, span)?;
            let slot = self.alloca_typed("msh.", &t);
            if t != JrgType::Unit { self.emit(format!("store {} zeroinitializer, ptr {}", t.llvm_ty(), slot)); }
            match item {
                "await" | "join" => {
                    self.ensure_extern("declare void @jrg_masa_ambil(ptr, ptr, ptr, i64)", "jrg_masa_ambil");
                    self.emit(format!("call void @jrg_masa_ambil(ptr {}, ptr {}, ptr {}, i64 {})", m.text, slot, np, nl));
                    Ok(self.load_place(&slot, &t))
                }
                "is_done" => {
                    self.ensure_extern("declare i32 @jrg_masa_selesai(ptr)", "jrg_masa_selesai");
                    let r = self.fresh("msd.");
                    self.emit(format!("{} = call i32 @jrg_masa_selesai(ptr {})", r, m.text));
                    let b = self.fresh("msdb.");
                    self.emit(format!("{} = icmp ne i32 {}, 0", b, r));
                    Ok(self.from_i1(&b))
                }
                _ => {
                    // `try_get` (tanpa menunggu) dan `timeout(f, ms)`: `Some(hasil)` atau `None`.
                    let mikro = if item == "timeout" {
                        let ms = self.codegen_expr(&args[1], Some(&JrgType::I64))?;
                        let ms = self.bulat_arg(&ms, "future::timeout", span)?;
                        let u = self.fresh("msu.");
                        self.emit(format!("{} = mul i64 {}, 1000", u, ms.text));
                        u
                    } else { "0".to_string() };
                    self.ensure_extern("declare i32 @jrg_masa_coba(ptr, ptr, i64, ptr, i64)", "jrg_masa_coba");
                    let r = self.fresh("msc.");
                    self.emit(format!("{} = call i32 @jrg_masa_coba(ptr {}, ptr {}, i64 {}, ptr {}, i64 {})", r, m.text, slot, mikro, np, nl));
                    let ada = self.fresh("msa.");
                    self.emit(format!("{} = icmp ne i32 {}, 0", ada, r));
                    let isi = self.lokal_sementara(slot, t.clone(), span);
                    let harap = JrgType::Enum(self.instantiate_option(&t));
                    let some = self.construct_option_result("Some", &[isi], Some(&harap), span)?;
                    let none = self.construct_option_result("None", &[], Some(&harap), span)?;
                    let p = self.fresh("msp.");
                    self.emit(format!("{} = select i1 {}, {} {}, {} {}", p, ada, harap.llvm_ty(), some.text, harap.llvm_ty(), none.text));
                    Ok(Val { text: p, ty: harap })
                }
            }
        })())
    }

    /// `future::all(fs)` (semua hasil, urut) dan `future::race(fs)` (hasil yang pertama siap).
    fn masa_banyak(&mut self, e: &Expr, balap: bool, nama: &str, np: &str, nl: &str, span: Span) -> Result<Val, String> {
        let a = self.codegen_expr(e, None)?;
        let (t, n) = match &a.ty {
            JrgType::Array(el, n) => match &**el { JrgType::Masa(t, _) => ((**t).clone(), Some(*n)), _ if *n == 0 => (JrgType::I64, Some(0)), _ => (JrgType::Never, None) },
            JrgType::Slice(el) => match &**el { JrgType::Masa(t, _) => ((**t).clone(), None), _ => (JrgType::Never, None) },
            _ => (JrgType::Never, None),
        };
        if t == JrgType::Never {
            return Err(super::galat::tandai(span, format!("{}: butuh array berisi `Future`/`Task`, dapat `{}`", nama, a.ty.display_name())));
        }
        if n == Some(0) && !balap {
            return Ok(Val { text: "zeroinitializer".into(), ty: JrgType::Array(Box::new(JrgType::I64), 0) });
        }
        let elem = JrgType::Masa(Box::new(t.clone()), false);
        let (ptr, len) = self.bagian_koleksi_pub(&a, &elem);
        self.ensure_extern("declare void @jrg_masa_ambil(ptr, ptr, ptr, i64)", "jrg_masa_ambil");
        if balap {
            self.ensure_extern("declare i64 @jrg_masa_balap(ptr, i64)", "jrg_masa_balap");
            let i = self.fresh("msb.");
            self.emit(format!("{} = call i64 @jrg_masa_balap(ptr {}, i64 {})", i, ptr, len));
            let ep = self.fresh("msbp.");
            self.emit(format!("{} = getelementptr ptr, ptr {}, i64 {}", ep, ptr, i));
            let m = self.fresh("msbm.");
            self.emit(format!("{} = load ptr, ptr {}", m, ep));
            let slot = self.alloca_typed("msbh.", &t);
            self.emit(format!("call void @jrg_masa_ambil(ptr {}, ptr {}, ptr {}, i64 {})", m, slot, np, nl));
            return Ok(self.load_place(&slot, &t));
        }
        let buf = self.buffer_baru(&len, &t);
        let idx = self.alloca_raw("msai.", "i64", Some(8));
        self.emit(format!("store i64 0, ptr {}", idx));
        let (cek, badan, usai) = (self.fresh_label("msa.cek."), self.fresh_label("msa.badan."), self.fresh_label("msa.usai."));
        self.terminate(format!("br label %{}", cek));
        self.start_block(cek.clone());
        let i = self.fresh("msai.");
        self.emit(format!("{} = load i64, ptr {}", i, idx));
        let c = self.fresh("msac.");
        self.emit(format!("{} = icmp slt i64 {}, {}", c, i, len));
        self.terminate(format!("br i1 {}, label %{}, label %{}", c, badan, usai));
        self.start_block(badan);
        let ep = self.fresh("msap.");
        self.emit(format!("{} = getelementptr ptr, ptr {}, i64 {}", ep, ptr, i));
        let m = self.fresh("msam.");
        self.emit(format!("{} = load ptr, ptr {}", m, ep));
        let dst = self.fresh("msad.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", dst, t.llvm_ty(), buf, i));
        self.emit(format!("call void @jrg_masa_ambil(ptr {}, ptr {}, ptr {}, i64 {})", m, dst, np, nl));
        let n1 = self.fresh("msan.");
        self.emit(format!("{} = add i64 {}, 1", n1, i));
        self.emit(format!("store i64 {}, ptr {}", n1, idx));
        self.terminate(format!("br label %{}", cek));
        self.start_block(usai);
        Ok(self.rakit_slice(&buf, &len, &t))
    }

    /// Angka tentang sebuah channel: 0 panjang, 1 kapasitas, 2 tertutup, 3 ada nilai.
    fn chan_info(&mut self, cv: &Val, apa: i32) -> String {
        self.ensure_extern("declare i64 @jrg_chan_info(ptr, i32)", "jrg_chan_info");
        let r = self.fresh("chi.");
        self.emit(format!("{} = call i64 @jrg_chan_info(ptr {}, i32 {})", r, cv.text, apa));
        r
    }

    /// `chan::nama(c, ..)` dan `c.nama(..)`; `None` kalau `nama` bukan operasi channel.
    pub(crate) fn coba_builtin_chan(&mut self, nama: &str, args: &[Expr], span: Span) -> Option<Result<Val, String>> {
        let aritas = match nama {
            "send" | "try_send" => 2,
            "recv" | "try_recv" | "close" | "len" | "cap" | "is_closed" | "has_value" => 1,
            _ => return None,
        };
        if args.len() != aritas { return None; }
        Some((|| {
            let (cv, ety) = self.chan_dari(&args[0], &format!("`chan::{}`", nama), span)?;
            let esize = self.types.sizeof(&ety);
            let bool_dari = |cg: &mut Self, r: String, lebar: &str| {
                let b = cg.fresh("chb.");
                cg.emit(format!("{} = icmp ne {} {}, 0", b, lebar, r));
                cg.from_i1(&b)
            };
            match nama {
                "send" => self.codegen_send(&args[0], &args[1], span),
                "recv" => self.codegen_recv(&args[0], span),
                "close" => {
                    self.ensure_extern("declare void @jrg_chan_close(ptr)", "jrg_chan_close");
                    self.emit(format!("call void @jrg_chan_close(ptr {})", cv.text));
                    Ok(Val::unit())
                }
                "len" => { let r = self.chan_info(&cv, 0); Ok(Val { text: r, ty: JrgType::I64 }) }
                "cap" => { let r = self.chan_info(&cv, 1); Ok(Val { text: r, ty: JrgType::I64 }) }
                "is_closed" => { let r = self.chan_info(&cv, 2); Ok(bool_dari(self, r, "i64")) }
                "has_value" => { let r = self.chan_info(&cv, 3); Ok(bool_dari(self, r, "i64")) }
                "try_send" => {
                    let vp = self.kirim_ke(&cv, &ety, &args[1])?;
                    self.ensure_extern("declare i32 @jrg_chan_try_send(ptr, ptr, i64)", "jrg_chan_try_send");
                    let r = self.fresh("cts.");
                    self.emit(format!("{} = call i32 @jrg_chan_try_send(ptr {}, ptr {}, i64 {})", r, cv.text, vp, esize));
                    let ok = Expr::Tuple { elems: Vec::new(), span };
                    self.hasil_coba(&r, &JrgType::Unit, ok,
                        "channel penuh, kirim akan block (would block)", "channel sudah ditutup: tidak ada penerima", span)
                }
                _ => {
                    let slot = self.alloca_typed("ctr.", &ety);
                    self.emit(format!("store {} zeroinitializer, ptr {}", ety.llvm_ty(), slot));
                    self.ensure_extern("declare i32 @jrg_chan_try_recv(ptr, ptr, i64)", "jrg_chan_try_recv");
                    let r = self.fresh("ctr.");
                    self.emit(format!("{} = call i32 @jrg_chan_try_recv(ptr {}, ptr {}, i64 {})", r, cv.text, slot, esize));
                    let isi = self.lokal_sementara(slot, ety.clone(), span);
                    self.hasil_coba(&r, &ety, isi,
                        "channel kosong, terima akan block (would block)", "channel sudah ditutup: tidak ada pengirim", span)
                }
            }
        })())
    }

    /// Di `select`, channel yang sudah ditutup tidak pernah "siap": ditolak, seperti dua mesin lain.
    fn tolak_tertutup(&mut self, hasil: &str, pesan: &str) {
        let c = self.fresh("selt.");
        self.emit(format!("{} = icmp eq i32 {}, 2", c, hasil));
        let (gagal, lanjut) = (self.fresh_label("sel.tutup."), self.fresh_label("sel.buka."));
        self.terminate(format!("br i1 {}, label %{}, label %{}, !prof !0", c, gagal, lanjut));
        self.start_block(gagal);
        self.panik(pesan);
        self.start_block(lanjut);
    }

    /// Hasil `try_send`/`try_recv` sebagai `Result`: 1 → `Ok(nilai)`, 0 → `Err(menunggu)`,
    /// 2 → `Err(tertutup)` — ketiganya dibangun, lalu satu dipilih.
    fn hasil_coba(&mut self, r: &str, ety: &JrgType, nilai: Expr, menunggu: &str, tertutup: &str, span: Span)
        -> Result<Val, String>
    {
        let harap = JrgType::Enum(self.instantiate_result(ety, &JrgType::Str));
        let teks = |t: &str| Expr::Literal(LitKind::Str(t.to_string()), span);
        let ok = self.construct_option_result("Ok", &[nilai], Some(&harap), span)?;
        let e0 = self.construct_option_result("Err", &[teks(menunggu)], Some(&harap), span)?;
        let e2 = self.construct_option_result("Err", &[teks(tertutup)], Some(&harap), span)?;
        let lty = harap.llvm_ty();
        let nol = self.fresh("hc0.");
        self.emit(format!("{} = icmp eq i32 {}, 0", nol, r));
        let gagal = self.fresh("hcg.");
        self.emit(format!("{} = select i1 {}, {} {}, {} {}", gagal, nol, lty, e0.text, lty, e2.text));
        let satu = self.fresh("hc1.");
        self.emit(format!("{} = icmp eq i32 {}, 1", satu, r));
        let p = self.fresh("hcp.");
        self.emit(format!("{} = select i1 {}, {} {}, {} {}", p, satu, lty, ok.text, lty, gagal));
        Ok(Val { text: p, ty: harap })
    }

    /// `select`: tiap case dicoba tanpa menunggu, berurutan; kalau belum ada yang siap dan
    /// tidak ada `default`, giliran diserahkan dulu ke goroutine lain lalu dicoba lagi.
    pub fn codegen_select(&mut self, sel: &SelectStmt) -> Result<(), String> {
        if sel.cases.is_empty() && sel.default.is_none() {
            return Err(super::galat::tandai(sel.span, "`select` butuh minimal satu case"));
        }
        self.ensure_extern("declare i32 @jrg_chan_try_recv(ptr, ptr, i64)", "jrg_chan_try_recv");
        self.ensure_extern("declare i32 @jrg_chan_try_send(ptr, ptr, i64)", "jrg_chan_try_send");
        let poll_lbl = self.fresh_label("sel.poll.");
        let end_lbl = self.fresh_label("sel.end.");
        self.terminate(format!("br label %{}", poll_lbl));
        self.start_block(poll_lbl.clone());

        for case in &sel.cases {
            let next_lbl = self.fresh_label("sel.next.");
            let body_lbl = self.fresh_label("sel.body.");
            match case {
                SelectCase::Recv { names, chan, body, span, .. } => {
                    let (cv, ety) = self.chan_dari(chan, "`select`", *span)?;
                    let slot = self.alloca_typed("selrv.", &ety);
                    let ok = self.fresh("selok.");
                    self.emit(format!("{} = call i32 @jrg_chan_try_recv(ptr {}, ptr {}, i64 {})", ok, cv.text, slot, self.types.sizeof(&ety)));
                    self.tolak_tertutup(&ok, crate::antrean::PESAN_TERIMA_TERTUTUP);
                    let okb = self.fresh("selokb.");
                    self.emit(format!("{} = icmp eq i32 {}, 1", okb, ok));
                    self.terminate(format!("br i1 {}, label %{}, label %{}", okb, body_lbl, next_lbl));
                    self.start_block(body_lbl);
                    self.push_scope();
                    if let Some(n) = names.first() {
                        if n != "_" {
                            let v = self.load_place(&slot, &ety);
                            let s = self.alloca_local(n, ety.clone());
                            self.emit(format!("store {} {}, ptr {}", ety.llvm_ty(), v.text, s));
                        }
                    }
                    self.codegen_stmt_block(body)?;
                    self.pop_scope();
                }
                SelectCase::Send { chan, val, body, span } => {
                    let (cv, ety) = self.chan_dari(chan, "`select`", *span)?;
                    let vp = self.kirim_ke(&cv, &ety, val)?;
                    let ok = self.fresh("selsok.");
                    self.emit(format!("{} = call i32 @jrg_chan_try_send(ptr {}, ptr {}, i64 {})", ok, cv.text, vp, self.types.sizeof(&ety)));
                    self.tolak_tertutup(&ok, crate::antrean::PESAN_KIRIM_TERTUTUP);
                    let okb = self.fresh("selsokb.");
                    self.emit(format!("{} = icmp eq i32 {}, 1", okb, ok));
                    self.terminate(format!("br i1 {}, label %{}, label %{}", okb, body_lbl, next_lbl));
                    self.start_block(body_lbl);
                    self.push_scope();
                    self.codegen_stmt_block(body)?;
                    self.pop_scope();
                }
                SelectCase::Let { span, .. } => {
                    return Err(super::galat::tandai(*span, "`select` dengan case `let` belum didukung backend `--emit-llvm`"));
                }
            }
            if !self.is_terminated() { self.terminate(format!("br label %{}", end_lbl)); }
            self.start_block(next_lbl);
        }

        if let Some(default_body) = &sel.default {
            self.push_scope();
            self.codegen_stmt_block(default_body)?;
            self.pop_scope();
            if !self.is_terminated() { self.terminate(format!("br label %{}", end_lbl)); }
        } else {
            self.ensure_extern("declare void @jrg_ko_tidur(i64)", "jrg_ko_tidur");
            self.emit("call void @jrg_ko_tidur(i64 200)".to_string());
            self.terminate(format!("br label %{}", poll_lbl));
        }
        self.start_block(end_lbl);
        Ok(())
    }
}
