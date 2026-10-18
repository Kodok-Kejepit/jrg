// src/llvm/codegen_fungsi.rs — penurunan fungsi dan monomorfisasi tipenya

#![allow(dead_code)]

use std::collections::HashMap;

use crate::parser::*;

use super::codegen::*;
use super::mangle::*;
use super::sema::*;
use super::types::*;

impl Codegen {
    // ── function codegen ────────────────────────────────────────────────

    pub(crate) fn codegen_function(&mut self, _base_name: &str, decl: &FnDecl, subst: &TypeSubst, mangled: &str, self_ty: Option<JrgType>, arity: usize) -> Result<(), String> {
        self.unit_kini = self.unit_simbol_dari(mangled);
        self.galat_duga_kini = self.galat_duga.get(mangled).cloned();
        self.locals.clear();
        self.push_scope();
        self.finished_blocks.clear();
        self.cur_lines.clear();
        self.entry_allocas.clear();
        self.cur_label = "entry".to_string();
        self.terminated = false;
        self.loop_stack.clear();
        self.cur_fn_generics = subst.clone();
        self.self_dipinjam = false;
        self.pinjaman.clear();
        self.kedalaman_match = 0;
        self.is_main_fn = mangled == "main";
        // Diisi di sini, bukan sesudah badannya diturunkan: lihat catatan di
        // deklarasi `cur_self_ty`.
        self.cur_self_ty = match &self_ty {
            Some(JrgType::Struct(n)) => Some(super::generik::dasar(n).to_string()),
            _ => None,
        };

        let ret_ty = self.tipe_return_duga(decl, subst, mangled, self_ty.as_ref(), arity)?;
        self.cur_ret_ty = ret_ty.clone();
        // Penyimpul MENYERAH dan `tipe_return` menebak `()`: nilai yang ternyata
        // dikembalikan badannya akan ditolak, bukan dibuang (`periksa_nilai_terbuang`).
        let ditebak = decl.ret_type.is_none() && !self.is_main_fn && ret_ty == JrgType::Unit
            && matches!(self.simpulkan_tipe_return(decl, subst), None | Some(JrgType::Never));
        let ditebak_lama = std::mem::replace(&mut self.ret_ditebak, ditebak);
        // Dihitung SEBELUM badannya diturunkan — `codegen_var` perlu tahu
        // sebuah lokal akan tumbuh pada saat ia dideklarasikan, bukan
        // sesudah penugasan yang menumbuhkannya terlihat.
        let tumbuh = self.lokal_yang_tumbuh(decl, subst);
        let tumbuh_lama = std::mem::replace(&mut self.lokal_tumbuh, tumbuh);
        // Dihitung SEBELUM badannya diturunkan, karena `alloca` sebuah lokal
        // dibuat jauh sebelum ekspresi closure yang menangkapnya dijumpai.
        let tangkap = match &*decl.body {
            FnBody::Block(b) => super::codegen_closure::nama_yang_naik_ke_heap(b),
            _ => std::collections::HashSet::new(),
        };
        let tangkap_lama = std::mem::replace(&mut self.tangkapan, tangkap);
        // Fungsi bersarang melihat saudara-saudaranya; fungsi tingkat atas
        // memulai dari kosong.
        let sarang = self.sarang_fn.get(mangled).cloned().unwrap_or_default();
        let sarang_lama = std::mem::replace(&mut self.fn_bersarang, sarang);
        let arg_kini = match &*decl.body {
            FnBody::Block(b) => {
                let mut lokal = super::infer::Lokal::new();
                for (i, p) in decl.params.iter().enumerate() {
                    if let Some(n) = pattern_simple_name(&p.pattern) {
                        if let Some(t) = self.tipe_dari_anotasi(&p.ty, subst)
                            .or_else(|| subst.get(&format!("$param{}", i)).cloned())
                        {
                            lokal.insert(n, t);
                        }
                    }
                }
                super::infer::tipe_argumen_dan_paruh(self, b, &lokal, subst)
            }
            _ => (HashMap::new(), HashMap::new()),
        };
        let (arg_kini, paruh_kini) = arg_kini;
        // Dict kosong yang diisi belakangan ikut ke tabel yang sama, dan begitu juga
        // lokal skalar yang tipenya berganti — ketiganya persoalan yang sama: tipe …
        if let FnBody::Block(b) = &*decl.body {
            for (n, t) in super::infer::dict_kosong_yang_diisi(self, b, subst) {
                self.lokal_tumbuh.entry(n).or_insert(t);
            }
            for n in super::infer::lokal_skalar_bentrok(self, b, subst) {
                self.lokal_tumbuh.insert(n, JrgType::Any);
            }
        }
        let arg_lama = std::mem::replace(&mut self.tipe_argumen, arg_kini);
        let paruh_lama = std::mem::replace(&mut self.paruh_result, paruh_kini);
        // `var rec = nil` yang kemudian diisi closure: tipenya diambil dari
        // penugasan itu, sebelum slotnya dibuat.
        let vfn = self.tipe_var_dari_closure(decl, subst, &self.tipe_argumen.clone());
        for (n, t) in vfn {
            self.lokal_tumbuh.entry(n).or_insert(t);
        }

        // Bingkai `defer` dibuka SEBELUM badannya diturunkan, karena `emit_return`
        // harus sudah tahu apakah ia boleh `ret` langsung atau harus …
        self.tunda = None;
        let punya_defer = match &*decl.body {
            FnBody::Block(b) => super::codegen_tunda::blok_punya_defer(b),
            _ => false,
        };

        // ---- parameter list ----
        let mut llparams: Vec<String> = Vec::new();
        let mut param_binds: Vec<(String, JrgType, String)> = Vec::new(); // (jrg name, type, incoming reg)

        for (i, p) in decl.params.iter().take(arity).enumerate() {
            let pname = pattern_simple_name(&p.pattern).unwrap_or_else(|| format!("_arg{}", i));
            let is_self = pname == "self";
            let pty = if is_self {
                self_ty.clone().ok_or_else(|| format!("`self` dipakai di luar konteks method (`{}`)", mangled))?
            } else if is_bare_array_type(&p.ty) {
                let key = format!("$arrparam{}", i);
                subst.get(&key).cloned().map(|elem| JrgType::Slice(Box::new(elem)))
                    .ok_or_else(|| format!("tidak bisa menyimpulkan tipe elemen `array` untuk parameter `{}`", pname))?
            } else {
                self.tipe_param(p, i, subst)?
            };
            let reg = format!("%p{}", i);
            let sig_ty = if is_self { "ptr".to_string() } else { pty.llvm_ty() };
            llparams.push(format!("{} {}", sig_ty, reg));
            param_binds.push((pname, pty, reg));
        }

        // Tangkapan closure yang sedang DIDUGA: cukup punya tempat dan tipe, isinya tidak dibaca.
        for (n, t) in std::mem::take(&mut self.benih_lokal) {
            if self.mode_duga { self.alloca_local(&n, t); }
        }
        // ---- entry block: alloca+store each param (self excepted: its
        // incoming register IS its address already, no extra indirection) ----
        let dibaca = self.nama_param_dibaca(decl);
        for (pname, pty, reg) in &param_binds {
            if pname == "self" {
                // Penerima adalah SALINAN: yang menulis ke `self`, atau menyebutnya di closure
                // dan `spawn` (yang bisa memegangnya lebih lama), mendapat salinannya sendiri.
                // Yang cuma membaca tidak membayar apa pun.
                let menulis = !crate::interpreter::capture::cuma_dibaca(&decl.body, &["self"])[0];
                // Varian milik: `self` adalah tempat variabel pemanggil yang sedang ditimpa hasilnya,
                // jadi ia ditulisi langsung — tanpa salinan masuk, tanpa salinan keluar.
                let milik = mangled.ends_with(super::codegen_tumbuh::AKHIRAN_MILIK);
                self.self_dipinjam = !menulis && !milik;
                if menulis && !milik {
                    let asal = self.load_place(reg, pty);
                    let salinan = self.salin_dalam(&asal);
                    let slot = self.alloca_local("self", pty.clone());
                    self.emit(format!("store {} {}, ptr {}", pty.llvm_ty(), salinan.text, slot));
                } else {
                    self.define_local("self", reg.clone(), pty.clone());
                }
                if self.self_dipinjam { self.pinjaman.insert("self".to_string()); }
            } else {
                // Lewat `alloca_local`, bukan `alloca_typed` langsung:
                // parameter juga bisa ditangkap closure, dan pintunya harus
                // satu.
                let slot = self.alloca_local(pname, pty.clone());
                self.emit(format!("store {} {}, ptr {}", pty.llvm_ty(), reg, slot));
                if dibaca.iter().any(|n| n == pname) && self.butuh_salin(pty) {
                    self.pinjaman.insert(pname.clone());
                }
            }
        }

        // Parameter yang situs pemanggilnya hilangkan dihitung DI SINI, setelah yang
        // datang sudah terikat — jadi sebuah default boleh menyebut parameter …
        for (i, p) in decl.params.iter().enumerate().skip(arity) {
            let pname = pattern_simple_name(&p.pattern).unwrap_or_else(|| format!("_arg{}", i));
            let kosong;
            let d = match p.default.as_ref() {
                Some(d) => d,
                // Pengumpul yang tidak dapat argumen berisi array kosong.
                None if p.is_variadic => {
                    kosong = Box::new(Expr::Array { elems: Vec::new(), span: p.span });
                    &kosong
                }
                None => return Err(format!(
                    "parameter `{}` tidak diberi nilai dan tidak punya default", pname)),
            };
            let petunjuk = self.tipe_param(p, i, subst).ok();
            let v = self.codegen_expr(d, petunjuk.as_ref())?;
            let pty = petunjuk.unwrap_or_else(|| v.ty.clone());
            let v = self.coerce(&v, &pty);
            let slot = self.alloca_typed("dflt.", &pty);
            if pty != JrgType::Unit {
                self.emit(format!("store {} {}, ptr {}", pty.llvm_ty(), v.text, slot));
            }
            self.define_local(&pname, slot, pty);
        }

        if punya_defer { self.buka_bingkai_tunda(&ret_ty); }

        // `var` tingkat modul diinisialisasi di awal `main`, dalam urutan
        // deklarasinya, sebelum satu baris pun badan `main` jalan.
        if self.is_main_fn {
            self.siapkan_var_global()?;
        }

        if self.is_main_fn && self.options.full_buffered_stdout {
            self.ensure_extern(
                "declare void @jrg_set_stdout_buffering(i32) nounwind",
                "jrg_set_stdout_buffering",
            );
            self.emit("call void @jrg_set_stdout_buffering(i32 1)".to_string());
        }

        match &*decl.body {
            FnBody::Block(block) => {
                self.codegen_fn_block(block, &ret_ty)?;
            }
            FnBody::Expr(e) => {
                let v = self.codegen_expr(e, Some(&ret_ty))?;
                let v = self.lepas_dari_self(e, v);
                self.emit_return(&v, &ret_ty);
            }
            // Sama seperti interpreter dan VM: tidak ada badan untuk
            // diterjemahkan. Menolak dengan sebab yang benar, bukan
            // memancarkan fungsi kosong yang diam-diam mengembalikan nol.
            FnBody::Abstrak => {
                return Err(format!(
                    "`{}` dideklarasikan tanpa badan di trait-nya, jadi tidak ada yang \
                     bisa diterjemahkan — `impl` yang dipakai belum menyediakannya",
                    decl.name
                ));
            }
        }

        if !self.terminated {
            if ret_ty == JrgType::Unit {
                self.emit_void_return();
            } else {
                return Err(format!(
                    "fungsi `{}`: ada jalur kode yang tidak mengembalikan nilai bertipe `{}`",
                    mangled, ret_ty.display_name()
                ));
            }
        }

        self.pancarkan_blok_tunda(&ret_ty)?;

        self.pop_scope();
        let body_text = self.take_function_body();
        let is_main = mangled == "main";
        let ret_spelling = if is_main { "i32".to_string() } else { ret_ty.llvm_ret_ty() };
        let linkage = if is_main { "dso_local" } else { "internal" };
        let def = format!(
            "define {} {} @{}({}) local_unnamed_addr nounwind {{\n{}}}\n",
            linkage,
            ret_spelling,
            llvm_quoted_ident(mangled),
            llparams.join(", "),
            body_text
        );
        self.fn_defs.push(def);
        self.is_main_fn = false;
        self.lokal_tumbuh = tumbuh_lama;
        self.tangkapan = tangkap_lama;
        self.fn_bersarang = sarang_lama;
        self.ret_ditebak = ditebak_lama;
        self.tipe_argumen = arg_lama;
        self.paruh_result = paruh_lama;
        Ok(())
    }

    /// Codegen a function body block, honoring the tail expression as the
    /// implicit return (Rust-style "last expression is the value").
    pub(crate) fn codegen_fn_block(&mut self, block: &Block, ret_ty: &JrgType) -> Result<(), String> {
        self.hoist_fn_bersarang(block);
        for stmt in &block.stmts {
            self.codegen_stmt(stmt)?;
            if self.terminated { break; }
        }
        if !self.terminated {
            if let Some(tail) = &block.tail {
                // A `Unit`-returning function's tail is discarded exactly like any other
                // statement position — no result slot, no cross-branch type unification if …
                if *ret_ty == JrgType::Unit {
                    // `if … else` di ekor: lewat bentuk ekspresinya kalau tipenya cuma TEBAKAN,
                    // supaya cabang yang menghasilkan nilai terlihat penjaga — dulu `() ()`.
                    let t = match tail.as_ref() {
                        Expr::If(i) if self.ret_ditebak && i.else_.is_some() =>
                            Some(self.codegen_expr(tail, None)?.ty),
                        // `match` di ekor sama: lengan yang tipenya tidak terbaca sekilas
                        // (`Some(v) => [v]`) dulu membuat fungsinya menjawab `()` tanpa pesan.
                        Expr::Match { .. } if self.ret_ditebak => Some(self.codegen_expr(tail, None)?.ty),
                        _ => self.codegen_expr_discarding_tipe(tail)?,
                    };
                    self.periksa_nilai_terbuang(t.as_ref());
                    self.emit_void_return();
                } else {
                    let v = self.codegen_expr(tail, Some(ret_ty))?;
                    let v = self.lepas_dari_self(tail, v);
                    self.emit_return(&v, ret_ty);
                }
            }
        }
        self.fn_bersarang.pop();
        Ok(())
    }

    /// Emit `ret void` — except inside `main`, which the C ABI/OS entry point
    /// requires to actually be `i32 @main()`, so a bare `return;` / fall-through …
    pub(crate) fn siapkan_var_global(&mut self) -> Result<(), String> {
        let daftar = self.tables.vars.clone();
        for v in &daftar {
            // Nilai awal `var` milik modul diturunkan DI DALAM unitnya.
            self.unit_kini = super::modul::unit_dari(&v.name);
            let want = match &v.ty {
                Some(te) => Some(self.resolve_type_with_subst(te, &TypeSubst::new())
                    .map_err(|e| super::galat::tandai(v.span, e))?),
                None => None,
            };
            let nilai = match &v.value {
                Some(e) => {
                    let val = self.codegen_expr(e, want.as_ref())
                        .map_err(|e| super::galat::beri_span_bila_kosong(e, v.span))?;
                    let val = self.ambil_milik(e, val)?;
                    match &want {
                        Some(t) => self.coerce(&val, t),
                        None => val,
                    }
                }
                None => return Err(super::galat::tandai(v.span, format!(
                    "`var {}` tingkat modul tanpa nilai awal belum didukung backend LLVM v1\n\
                     bantuan: beri nilai awalnya, mis. `var {} = 0`",
                    super::modul::tampil(&v.name), super::modul::tampil(&v.name)))),
            };
            // Sebuah `var` tingkat modul BOLEH ditumbuhkan dari fungsi mana pun di
            // program, dan analisis "lokal yang tumbuh" cuma melihat satu fungsi — jadi …
            let ty = match want {
                Some(t) => t,
                None => match &nilai.ty {
                    // Array KOSONG tidak memberi tahu tipe elemennya. Yang
                    // memberi tahu adalah penugasan di fungsi mana pun di
                    // program ini — lihat `tipe_elemen_var_global`.
                    JrgType::Array(e, 0) => {
                        // Konflik dilaporkan DI SINI, di deklarasinya — tempat sebabnya berada.
                        let bentrok = self.tipe_elemen_var_global_konflik(&v.name);
                        if bentrok.len() > 1 {
                            JrgType::Slice(Box::new(JrgType::Any))
                        } else {
                        match self.tipe_elemen_var_global(&v.name) {
                            Some(t) => JrgType::Slice(Box::new(t)),
                            None => {
                                // Dicatat, BUKAN dilaporkan: bawaan `int` benar untuk program yang memang
                                // memakainya sebagai int, dan menolaknya di sini akan merusak yang sekarang …
                                self.var_kosong_tak_tersimpulkan.push(v.name.clone());
                                JrgType::Slice(Box::new((**e).clone()))
                            }
                        }
                        }
                    }
                    JrgType::Array(e, _) => JrgType::Slice(e.clone()),
                    // Skalar tingkat modul yang tipenya berganti.
                    lain if super::infer::skalar_global_bentrok(self, &v.name, lain) =>
                        JrgType::Any,
                    lain => lain.clone(),
                },
            };
            let nilai = self.coerce(&nilai, &ty);
            let nilai = if matches!(ty, JrgType::Slice(_)) { self.salin_slice(&nilai) } else { nilai };
            let sym = format!("@{}", super::mangle::llvm_quoted_ident(&format!("gv.{}", v.name)));
            self.var_global_baris.push(format!(
                "{} = internal global {} zeroinitializer", sym, ty.llvm_ty()));
            if ty != JrgType::Unit {
                self.emit(format!("store {} {}, ptr {}", ty.llvm_ty(), nilai.text, sym));
            }
            self.var_global.insert(v.name.clone(), (sym, ty));
        }
        // Dipanggil dari dalam penurunan `main`: unitnya dikembalikan ke berkas utama.
        self.unit_kini = 0;
        Ok(())
    }

    /// Di ujung `main` program yang memakai konkurensi: goroutine yang belum selesai ditunggu.
    pub(crate) fn tunggu_goroutine(&mut self) {
        if !self.berkonkurensi { return; }
        self.ensure_extern("declare void @jrg_ko_tunggu_semua()", "jrg_ko_tunggu_semua");
        self.emit("call void @jrg_ko_tunggu_semua()".to_string());
    }

    /// Apakah fungsi yang sedang diturunkan itu `main`?
    pub fn sedang_main(&self) -> bool { self.is_main_fn }

    pub fn emit_void_return(&mut self) {
        if self.terminated { return; }
        if self.is_main_fn {
            if self.lompat_ke_tunda(Some("i32 0")) { return; }
            self.tunggu_goroutine();
            self.terminate("ret i32 0".to_string());
        } else {
            if self.lompat_ke_tunda(None) { return; }
            self.terminate("ret void".to_string());
        }
    }

    /// Buka bingkai `defer` fungsi yang sedang diturunkan: catat tinggi tumpukannya
    /// dan sediakan tempat untuk nilai pulangnya.
    pub(crate) fn buka_bingkai_tunda(&mut self, ret_ty: &JrgType) {
        self.ensure_extern("declare i64 @jrg_tunda_tinggi()", "jrg_tunda_tinggi");
        let dasar = self.fresh("tunda.dasar.");
        self.emit(format!("{} = call i64 @jrg_tunda_tinggi()", dasar));
        let slot_ret = if *ret_ty == JrgType::Unit && !self.is_main_fn {
            None
        } else if self.is_main_fn {
            Some(self.alloca_raw("tunda.ret.", "i32", Some(4)))
        } else {
            Some(self.alloca_typed("tunda.ret.", ret_ty))
        };
        let blok = self.fresh_label("tunda.jalankan.");
        self.tunda = Some(Tunda { dasar, blok, slot_ret, jumlah: 0 });
    }

    /// Kalau fungsi ini punya `defer`, sebuah `return` tidak boleh langsung
    /// `ret`: daftarnya harus dijalankan dulu.
    pub(crate) fn lompat_ke_tunda(&mut self, nilai: Option<&str>) -> bool {
        let Some(t) = &self.tunda else { return false };
        let blok = t.blok.clone();
        let slot = t.slot_ret.clone();
        if let (Some(slot), Some(v)) = (slot, nilai) {
            self.emit(format!("store {}, ptr {}", v, slot));
        }
        self.terminate(format!("br label %{}", blok));
        true
    }

    /// Fungsi tanpa anotasi yang tipe kembaliannya DITEBAK `()` ternyata
    /// mengembalikan nilai: tolak di sini, di mana tipenya sudah diketahui.
    /// Dulu nilainya dibuang (`ret void`), jadi `typeof(f(x))` menjawab `unit`.
    pub(crate) fn periksa_nilai_terbuang(&mut self, t: Option<&JrgType>) {
        let Some(t) = t else { return };
        if !self.ret_ditebak || matches!(t, JrgType::Unit | JrgType::Never) { return; }
        if self.mode_duga {
            self.hasil_duga = Some(match &self.hasil_duga {
                Some(lama) => Self::gabung_duga(lama, t),
                None => t.clone(),
            });
            return;
        }
        self.lapor(format!(
            "nilai bertipe `{}` ini dikembalikan fungsinya, tapi backend LLVM tidak bisa \
             menyimpulkan tipe kembalian fungsi itu dari badannya — tanpa anotasi, \
             nilainya akan DIBUANG\n\
             bantuan: tulis tipe kembaliannya di tanda tangan fungsinya: `-> {}`\n\
             catatan: interpreter dan VM mengembalikan nilai ini; menebak `()` \
             membuat `typeof` menjawab `unit` tanpa satu pun galat",
            t.nama_typeof(), t.display_name()));
    }

    /// Nilai kembalian yang berakar pada `self` PINJAMAN (`return self.isi`, `return self`)
    /// disalin: tanpa itu hasilnya berbagi penyangga dengan milik pemanggil, dan menulis ke
    /// hasil mengubah aslinya.
    pub(crate) fn lepas_dari_self(&mut self, e: &Expr, v: Val) -> Val {
        fn berakar_self(e: &Expr) -> bool {
            match e {
                Expr::Ident(n, _) => n == "self",
                Expr::Field { obj, .. } | Expr::Index { obj, .. } => berakar_self(obj),
                _ => false,
            }
        }
        fn akar(e: &Expr) -> Option<&str> {
            match e {
                Expr::Ident(n, _) => Some(n.as_str()),
                Expr::Field { obj, .. } | Expr::Index { obj, .. } => akar(obj),
                _ => None,
            }
        }
        // `var` modul sama saja: yang dikembalikan harus salinan, bukan penyangga globalnya.
        let global = akar(e).map_or(false, |n| self.lookup_local(n).is_none() && self.var_global.contains_key(&self.k(n)));
        if (self.self_dipinjam && berakar_self(e)) || global {
            if !self.butuh_salin(&v.ty) { return v; }
            return self.salin_dalam(&v);
        }
        // Bentuk lain yang menyebut sebuah pinjaman (`Some(self.xs)`, `(p, 1)`, ternary, `match`):
        // hasilnya bisa memuat penyangga pemanggil, jadi dijadikan milik sendiri.
        if self.pinjaman.is_empty() || !self.butuh_salin(&v.ty) { return v; }
        let disebut = crate::interpreter::capture::names_mentioned(e);
        if !self.pinjaman.iter().any(|n| disebut.contains(n)) { return v; }
        match self.ambil_milik(e, v.clone()) { Ok(baru) => baru, Err(_) => v }
    }

    /// Nama parameter `decl` yang cuma dibaca badannya (lihat `capture::cuma_dibaca`):
    /// pemanggil boleh menyerahkan penyangganya tanpa menyalin. Pengumpul dan pola tidak ikut.
    pub(crate) fn nama_param_dibaca(&self, decl: &FnDecl) -> Vec<String> {
        let nama: Vec<Option<String>> = decl.params.iter()
            .map(|p| if p.is_variadic { None } else { pattern_simple_name(&p.pattern) }).collect();
        let daftar: Vec<&str> = nama.iter().map(|n| n.as_deref().unwrap_or("")).collect();
        let hasil = crate::interpreter::capture::cuma_dibaca(&decl.body, &daftar);
        nama.into_iter().zip(hasil)
            .filter_map(|(n, b)| n.filter(|n| b && n != "self")).collect()
    }

    /// Argumen `arg` ke parameter ke-`i` milik `decl` boleh menumpang: parameternya cuma dibaca,
    /// dan akarnya lokal polos — tidak ada pihak lain yang bisa menulisinya selama panggilan.
    pub(crate) fn boleh_pinjam(&self, decl: &FnDecl, i: usize, arg: &Expr, ty: &JrgType) -> bool {
        let Some(nama) = decl.params.get(i).and_then(|p| pattern_simple_name(&p.pattern)) else { return false };
        if !self.butuh_salin(ty) || !self.akar_lokal_polos(arg) { return false; }
        self.nama_param_dibaca(decl).contains(&nama)
    }

    /// Nama di pangkal `x`, `x.a`, `x[i]`.
    pub(crate) fn nama_akar(e: &Expr) -> Option<&str> {
        match e {
            Expr::Ident(n, _) => Some(n.as_str()),
            Expr::Field { obj, .. } | Expr::Index { obj, .. } => Self::nama_akar(obj),
            _ => None,
        }
    }

    /// `x`, `x.a`, `x[i]` yang berakar pada lokal milik fungsi ini saja: bukan `var` modul,
    /// bukan yang dipakai bersama closure, bukan `self` yang menunjuk milik pemanggil.
    pub(crate) fn akar_lokal_polos(&self, e: &Expr) -> bool {
        let Some(n) = Self::nama_akar(e) else { return false };
        self.lookup_local(n).is_some() && !self.tangkapan.contains(n) && !self.tangkap_masuk.contains(n)
            && !(n == "self" && self.self_dipinjam)
    }

    pub fn emit_return(&mut self, v: &Val, ret_ty: &JrgType) {
        if self.terminated { return; }
        if self.is_main_fn {
            if *ret_ty == JrgType::Unit {
                self.emit_void_return();
            } else {
                let exit_code = self.coerce_longgar(v, &JrgType::I32);
                let sm = format!("i32 {}", exit_code.text);
                if self.lompat_ke_tunda(Some(&sm)) { return; }
                self.tunggu_goroutine();
                self.terminate(format!("ret {}", sm));
            }
            return;
        }
        if *ret_ty == JrgType::Unit {
            self.periksa_nilai_terbuang(Some(&v.ty));
            self.emit_void_return();
        } else {
            let coerced = self.coerce(v, ret_ty);
            let sm = format!("{} {}", ret_ty.llvm_ty(), coerced.text);
            if self.lompat_ke_tunda(Some(&sm)) { return; }
            self.terminate(format!("ret {}", sm));
        }
    }

    /// Selesaikan tipe sebuah PARAMETER fungsi, dengan lokasi parameter itu
    /// sendiri kalau gagal.
    pub fn tipe_return(&mut self, decl: &FnDecl, subst: &TypeSubst) -> Result<JrgType, String> {
        if let Some(te) = &decl.ret_type {
            // `-> array` telanjang: tipe elemennya tidak ada di anotasinya, jadi ia
            // disimpulkan dari badan fungsinya — mekanisme yang sama yang sudah dipakai …
            if is_bare_array_type(te) {
                return match self.simpulkan_tipe_return(decl, subst) {
                    Some(JrgType::Array(e, _)) | Some(JrgType::Slice(e)) => {
                        Ok(JrgType::Slice(e))
                    }
                    // Tidak terbaca dari badannya: array bertag, seperti `array` di posisi lain.
                    None => Ok(JrgType::Slice(Box::new(JrgType::Any))),
                    _ => Err(format!(
                        "tipe return `array` pada `{}` tidak menyebut tipe elemennya, \
                         dan backend LLVM tidak bisa menyimpulkannya dari badan fungsi\n\
                         bantuan: tulis tipe elemennya, mis. `-> [3]int`, atau kembalikan \
                         nilai yang tipenya terbaca langsung",
                        decl.name
                    )),
                };
            }
            return self.resolve_type_with_subst(te, subst);
        }
        let simpul = self.simpulkan_tipe_return(decl, subst);
        self.wujudkan_opsi();
        match simpul {
            // `Never` di sini adalah penanda "sudah pasti tidak bisa
            // disimpulkan" dari `infer.rs`, bukan tipe yang sebenarnya.
            Some(crate::llvm::types::JrgType::Never) | None => Ok(JrgType::Unit),
            Some(t) => Ok(t),
        }
    }

    /// `tipe_return`, plus penduga untuk fungsi tanpa anotasi yang penyimpulnya menyerah:
    /// badannya diturunkan sekali di SALINAN terisolasi, dan tipe nilai yang pertama kali
    /// dikembalikan dibaca dari sana.
    pub(crate) fn tipe_return_duga(&mut self, decl: &FnDecl, subst: &TypeSubst, mangled: &str,
                                   self_ty: Option<&JrgType>, arity: usize) -> Result<JrgType, String> {
        let t = self.tipe_return(decl, subst)?;
        let ditebak = decl.ret_type.is_none() && mangled != "main" && t == JrgType::Unit
            && matches!(&*decl.body, FnBody::Block(_))
            && matches!(self.simpulkan_tipe_return(decl, subst), None | Some(JrgType::Never));
        if !ditebak { return Ok(t); }
        let mut isi: Vec<String> = subst.iter().map(|(k, v)| format!("{}={}", k, v.mangle_frag())).collect();
        isi.sort();
        let kunci = format!("{}|{}|{}", mangled, self_ty.map(|x| x.mangle_frag()).unwrap_or_default(), isi.join(","));
        if let Some(h) = self.cache_duga.get(&kunci) { return Ok(h.clone().unwrap_or(JrgType::Unit)); }
        // Sedang diduga (rekursi) atau terlalu dalam: tebakan lama, yang gagal KERAS kalau salah.
        if self.sedang_duga.contains(&kunci) || self.sedang_duga.len() >= 24 { return Ok(JrgType::Unit); }
        let mut salinan = self.clone();
        salinan.mode_duga = true;
        salinan.hasil_duga = None;
        salinan.separuh_duga = (None, None);
        salinan.sedang_duga.insert(kunci.clone());
        let sebelum = salinan.errors.len();
        let jalan = salinan.codegen_function(&decl.name, decl, subst, mangled, self_ty.cloned(), arity);
        let galat = jalan.err().or_else(|| salinan.errors.get(sebelum).cloned());
        let mut hasil = salinan.hasil_duga.take().filter(|t| !Self::memuat_never(t));
        // `return Err(..)` di satu cabang dan `return Ok(..)` di cabang lain: dua separuh itu
        // bersama-sama menyebut `Result<T, E>` yang tidak ditulis satu pun dari keduanya.
        if hasil.is_none() {
            if let (Some(t), Some(e)) = salinan.separuh_duga.clone() {
                hasil = Some(JrgType::Enum(salinan.instantiate_result(&t, &e)));
            }
        }
        // Instansiasi `Option<T>`/`Result<T, E>` yang lahir di salinan ikut dibawa pulang: tipe
        // hasilnya bisa menyebut salah satunya, dan tata letaknya harus dikenal di sini juga.
        for (k, v) in &salinan.types.enums { self.types.enums.entry(k.clone()).or_insert_with(|| v.clone()); }
        // Instans struct generik yang lahir di salinan ikut pulang, dengan alasan yang sama.
        for (k, v) in &salinan.types.structs { self.types.structs.entry(k.clone()).or_insert_with(|| v.clone()); }
        for (k, v) in &salinan.struct_instans { self.struct_instans.entry(k.clone()).or_insert_with(|| v.clone()); }
        for n in &salinan.types.emit_order {
            if !self.types.emit_order.contains(n) { self.types.emit_order.push(n.clone()); }
        }
        for (k, v) in &salinan.option_args { self.option_args.entry(k.clone()).or_insert_with(|| v.clone()); }
        for (k, v) in &salinan.result_args { self.result_args.entry(k.clone()).or_insert_with(|| v.clone()); }
        for (k, v) in salinan.cache_duga { if v.is_some() { self.cache_duga.entry(k).or_insert(v); } }
        for (k, v) in salinan.galat_duga { self.galat_duga.entry(k).or_insert(v); }
        match galat {
            Some(g) if hasil.is_none() && !g.contains("butuh anotasi") => { self.galat_duga.insert(mangled.to_string(), g); }
            _ => {}
        }
        // `None` cuma dicatat di puncak: di dalam dugaan lain ia bisa lahir dari tebakan sementara.
        if hasil.is_some() || self.sedang_duga.is_empty() { self.cache_duga.insert(kunci, hasil.clone()); }
        Ok(hasil.unwrap_or(JrgType::Unit))
    }

    pub fn tipe_param(&mut self, p: &FnParam, idx: usize, subst: &TypeSubst) -> Result<JrgType, String> {
        let nama = pattern_simple_name(&p.pattern).unwrap_or_else(|| "_".to_string());

        // Parameter tanpa anotasi: tipenya disimpulkan dari situs PEMANGGILAN dan
        // dititipkan di substitusi.
        if matches!(p.ty, TypeExpr::Inferred) {
            if let Some(t) = subst.get(&format!("$param{}", idx)) {
                return Ok(t.clone());
            }
        }
        self.resolve_type_with_subst(&p.ty, subst).map_err(|e| {
            // Baris bantuan hanya ditambahkan kalau memang soal anotasi.
            // Kegagalan lain (mis. `chan`, `map`) sudah menjelaskan dirinya,
            // dan menyarankan anotasi di situ akan jadi saran yang salah.
            let pesan = if matches!(p.ty, TypeExpr::Inferred) {
                format!(
                    "parameter `{}` tidak punya anotasi tipe, dan backend LLVM tidak \
                     menyimpulkannya dari pemanggil\n\
                     bantuan: tulis tipenya, mis. `{}: int` atau `{}: []int`\n\
                     catatan: interpreter dan VM menerima parameter tanpa anotasi; \
                     hanya backend --emit-llvm yang memerlukannya",
                    nama, nama, nama
                )
            } else {
                e
            };
            crate::llvm::galat::beri_span_bila_kosong(pesan, p.span)
        })
    }

    pub fn resolve_type_with_subst(&mut self, te: &TypeExpr, subst: &TypeSubst) -> Result<JrgType, String> {
        // Option/Result need lazy instantiation as a side effect.
        match te {
            TypeExpr::Named(nt) if nt.name == "Option" => {
                if nt.args.len() != 1 {
                    return Err("Option<T> butuh tepat satu parameter tipe".into());
                }
                let t = self.resolve_type_with_subst(&nt.args[0], subst)?;
                Ok(JrgType::Enum(self.instantiate_option(&t)))
            }
            TypeExpr::Named(nt) if nt.name == "Result" => {
                if nt.args.len() != 2 {
                    return Err("Result<T, E> butuh tepat dua parameter tipe".into());
                }
                let t = self.resolve_type_with_subst(&nt.args[0], subst)?;
                let e = self.resolve_type_with_subst(&nt.args[1], subst)?;
                Ok(JrgType::Enum(self.instantiate_result(&t, &e)))
            }
            TypeExpr::Tuple(elems) => {
                let mut out = Vec::with_capacity(elems.len());
                for elem in elems {
                    out.push(self.resolve_type_with_subst(elem, subst)?);
                }
                Ok(JrgType::Tuple(out))
            }
            TypeExpr::Array(array) => {
                let elem = self.resolve_type_with_subst(&array.elem, subst)?;
                let len = match array.len.as_ref() {
                    Expr::Literal(LitKind::Int(value), _)
                        if *value <= i64::MAX as u128 =>
                    {
                        *value as u64
                    }
                    Expr::Literal(LitKind::Int(_), _) => {
                        return Err("panjang array terlalu besar untuk indeks i64".into());
                    }
                    _ => {
                        return Err("panjang array harus berupa literal integer konstan".into());
                    }
                };
                Ok(JrgType::Array(Box::new(elem), len))
            }
            TypeExpr::Slice(slice) => {
                let elem = self.resolve_type_with_subst(&slice.elem, subst)?;
                Ok(JrgType::Slice(Box::new(elem)))
            }
            TypeExpr::Pointer(pointer) => self.resolve_type_with_subst(&pointer.inner, subst),
            TypeExpr::Ownership(owned) => self.resolve_type_with_subst(&owned.inner, subst),
            // Struct generik: tata letaknya didaftarkan di sini, sekali per susunan argumen.
            TypeExpr::Named(nt) if !nt.args.is_empty() && !subst.contains_key(&nt.name)
                && self.param_struct(&nt.name).is_some() =>
            {
                let mut args = Vec::with_capacity(nt.args.len());
                for a in &nt.args { args.push(self.resolve_type_with_subst(a, subst)?); }
                Ok(JrgType::Struct(self.instantiate_struct(&nt.name, &args)?))
            }
            // Ditulis tanpa parameter (`[]Adegan`): instans bertag, didaftarkan juga di sini.
            TypeExpr::Named(nt) if nt.args.is_empty() && !subst.contains_key(&nt.name)
                && self.param_struct(&nt.name).is_some() =>
            {
                let n = self.param_struct(&nt.name).map(|p| p.len()).unwrap_or(0);
                Ok(JrgType::Struct(self.instantiate_struct(&nt.name, &vec![JrgType::Any; n])?))
            }
            TypeExpr::Fn(f) if f.generics.params.is_empty() => {
                // Lewat pintu ini juga, supaya `fn(Kotak<T>) -> T` mendaftarkan instansnya.
                let mut ps = Vec::with_capacity(f.params.len());
                for p in &f.params { ps.push(self.resolve_type_with_subst(p, subst)?); }
                let ret = match &f.ret {
                    Some(r) => self.resolve_type_with_subst(r, subst)?,
                    None => JrgType::Unit,
                };
                Ok(JrgType::Fn(ps, Box::new(ret)))
            }
            _ => {
                let ctx = TypeResolveCtx { tables: &self.tables, subst };
                resolve_type_expr(te, &ctx)
            }
        }
    }

    pub fn resolve_type_pub(&mut self, te: &TypeExpr) -> Result<JrgType, String> {
        let subst = self.cur_fn_generics.clone();
        self.resolve_type_with_subst(te, &subst)
    }

}
