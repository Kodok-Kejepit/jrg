// src/llvm/codegen_forrange.rs — `for` atas rentang, elemen, dan karakter

#![allow(dead_code)]

use crate::parser::*;

use super::codegen::{Codegen, Val, LoopCtx};
use super::types::*;

impl Codegen {
    // ── for-range ───────────────────────────────────────────────────────

    pub fn codegen_for_range(&mut self, vars: &[Ident], iter: &Expr, body: &Block,
                             label: &Option<String>) -> Result<(), String> {
        if vars.is_empty() || vars.len() > 2 {
            return Err("`for ... in`: butuh 1 atau 2 nama variabel".into());
        }
        if let Expr::Range { lo, hi, inclusive, .. } = iter {
            let lo_v = match lo {
                Some(e) => self.codegen_expr(e, Some(&JrgType::I64))?,
                None => Val { text: "0".into(), ty: JrgType::I64 },
            };
            let hi_v = match hi {
                Some(e) => self.codegen_expr(e, Some(&lo_v.ty))?,
                None => return Err("range tanpa batas atas tidak didukung".into()),
            };
            let hi_v = self.coerce(&hi_v, &lo_v.ty);
            return self.codegen_counting_loop(vars, lo_v, hi_v, *inclusive, body, label);
        }
        self.slot_bulat_gagal = None;
        let iv = self.codegen_expr(iter, None)?;
        // `for i in range(2.5)`: rentangnya sudah gagal saat jalan, badannya tak tercapai.
        if iv.ty == JrgType::Unit && self.slot_bulat_gagal.take().is_some_and(|s| Some(s) == iter_span(iter)) {
            return Ok(());
        }
        let menumpang = vars.len() == 2 && self.akar_lokal_polos(iter)
            && crate::interpreter::capture::blok_cuma_membaca(body, &vars[1])
            && !crate::interpreter::capture::blok_menulis(body, Self::nama_akar(iter).unwrap_or(""));
        if iv.ty == JrgType::Any { return self.for_bertag(vars, &iv, body, label); }
        self.for_atas(vars, iv, body, label, menumpang)
    }

    /// `for` atas nilai bertag: wadahnya dipilih dari tag SEKALI sebelum loop (array, dict, teks, bulat),
    /// badannya diturunkan sekali dengan `k`/`v` bertag yang diisi per putaran menurut tag yang sama.
    fn for_bertag(&mut self, vars: &[Ident], x: &Val, body: &Block, label: &Option<String>) -> Result<(), String> {
        use super::codegen_any::{TAG_DICT, TAG_INT, TAG_SLICE, TAG_STR};
        let nol = crate::lexer::Span { lo: crate::lexer::BytePos(0), hi: crate::lexer::BytePos(0), file_id: 0 };
        let (t_arr, t_dict) = (JrgType::Slice(Box::new(JrgType::Any)), JrgType::Dict(Box::new(JrgType::Any)));
        let tag = self.tag_any(x);
        let n_slot = self.alloca_raw("fbn.", "i64", Some(8));
        let (a_slot, d_slot, s_slot) = (self.alloca_typed("fba.", &t_arr), self.alloca_typed("fbd.", &t_dict),
            self.alloca_typed("fbs.", &JrgType::Str));
        let off_slot = self.alloca_raw("fbo.", "i64", Some(8));
        self.emit(format!("store i64 0, ptr {}", off_slot));
        let golongan = [TAG_SLICE, TAG_DICT, TAG_STR, TAG_INT];
        let siap = self.fresh_label("fb.siap.");
        let lain = self.fresh_label("fb.lain.");
        let label_buka: Vec<String> = golongan.iter().map(|_| self.fresh_label("fb.buka.")).collect();
        let kasus: Vec<String> = golongan.iter().zip(&label_buka).map(|(t, l)| format!("i32 {}, label %{}", t, l)).collect();
        self.terminate(format!("switch i32 {}, label %{} [ {} ]", tag.text, lain, kasus.join(" ")));
        for (t, l) in golongan.iter().zip(label_buka) {
            self.start_block(l);
            // Yang diiterasi SALINAN: badan boleh menulis ke nilai aslinya, seperti acuan.
            let n = match *t {
                TAG_SLICE => {
                    let a = self.buka_any(x, &t_arr, nol)?;
                    let a = self.salin_slice(&a);
                    self.emit(format!("store %JrgSlice {}, ptr {}", a.text, a_slot));
                    let n = self.fresh("fbn.");
                    self.emit(format!("{} = extractvalue %JrgSlice {}, 1", n, a.text));
                    n
                }
                TAG_DICT => {
                    let d = self.buka_any(x, &t_dict, nol)?;
                    let d = self.salin_dict(&d);
                    self.emit(format!("store %JrgDict {}, ptr {}", d.text, d_slot));
                    self.bagian_dict(&d).1
                }
                TAG_STR => {
                    let sv = self.buka_any(x, &JrgType::Str, nol)?;
                    self.emit(format!("store %JrgStr {}, ptr {}", sv.text, s_slot));
                    let (p, l) = self.split_str(&sv);
                    self.jumlah_karakter(&p, &l)
                }
                _ => self.buka_any(x, &JrgType::I64, nol)?.text,
            };
            self.emit(format!("store i64 {}, ptr {}", n, n_slot));
            self.terminate(format!("br label %{}", siap));
        }
        self.start_block(lain);
        let lama = std::mem::replace(&mut self.tanpa_situs, true);
        self.panik_tipe_any_struct(x, crate::interpreter::pesan_tak_bisa_iterasi);
        self.tanpa_situs = lama;

        self.start_block(siap);
        let i_slot = self.alloca_raw("fbi.", "i64", Some(8));
        self.emit(format!("store i64 0, ptr {}", i_slot));
        let (cek, badan, pasca, usai) = (self.fresh_label("fb.cek."), self.fresh_label("fb.badan."),
            self.fresh_label("fb.pasca."), self.fresh_label("fb.usai."));
        self.terminate(format!("br label %{}", cek));
        self.start_block(cek.clone());
        let (iv, nv, c) = (self.load_place(&i_slot, &JrgType::I64), self.load_place(&n_slot, &JrgType::I64), self.fresh("fbc."));
        self.emit(format!("{} = icmp slt i64 {}, {}", c, iv.text, nv.text));
        self.terminate(format!("br i1 {}, label %{}, label %{}", c, badan, usai));

        self.start_block(badan);
        let (k_slot, v_slot) = (self.alloca_typed("fbk.", &JrgType::Any), self.alloca_typed("fbv.", &JrgType::Any));
        let i = self.load_place(&i_slot, &JrgType::I64);
        let isi = self.fresh_label("fb.isi.");
        let label_ambil: Vec<String> = golongan.iter().map(|_| self.fresh_label("fb.ambil.")).collect();
        let kasus: Vec<String> = golongan.iter().zip(&label_ambil).map(|(t, l)| format!("i32 {}, label %{}", t, l)).collect();
        let mustahil = self.fresh_label("fb.x.");
        let ik = self.kotak_any(&i, nol)?;
        self.terminate(format!("switch i32 {}, label %{} [ {} ]", tag.text, mustahil, kasus.join(" ")));
        for (t, l) in golongan.iter().zip(label_ambil) {
            self.start_block(l);
            let (k, v) = match *t {
                TAG_SLICE => {
                    let a = self.load_place(&a_slot, &t_arr);
                    let (dp, ep) = (self.fresh("fbdp."), self.fresh("fbep."));
                    self.emit(format!("{} = extractvalue %JrgSlice {}, 0", dp, a.text));
                    self.emit(format!("{} = getelementptr %JrgAny, ptr {}, i64 {}", ep, dp, i.text));
                    (ik.clone(), self.load_place(&ep, &JrgType::Any))
                }
                TAG_DICT => {
                    let d = self.load_place(&d_slot, &t_dict);
                    let (data, _) = self.bagian_dict(&d);
                    let pty = self.tipe_pasangan(&JrgType::Any);
                    let (kp, vp) = (self.fresh("fbkp."), self.fresh("fbvp."));
                    self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}, i32 0", kp, pty, data, i.text));
                    self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}, i32 1", vp, pty, data, i.text));
                    let kunci = self.load_place(&kp, &JrgType::Str);
                    (self.kotak_any(&kunci, nol)?, self.load_place(&vp, &JrgType::Any))
                }
                TAG_STR => {
                    self.ensure_extern("declare i32 @jrg_str_next_char(ptr, i64, i64, ptr, ptr)", "jrg_str_next_char");
                    let sv = self.load_place(&s_slot, &JrgType::Str);
                    let (p, l) = self.split_str(&sv);
                    let off = self.load_place(&off_slot, &JrgType::I64);
                    let (cp_slot, no_slot) = (self.alloca_raw("fbcp.", "i32", Some(4)), self.alloca_raw("fbno.", "i64", Some(8)));
                    let r = self.fresh("fbr.");
                    self.emit(format!("{} = call i32 @jrg_str_next_char(ptr {}, i64 {}, i64 {}, ptr {}, ptr {})",
                        r, p, l, off.text, cp_slot, no_slot));
                    let no = self.load_place(&no_slot, &JrgType::I64);
                    self.emit(format!("store i64 {}, ptr {}", no.text, off_slot));
                    let cp = self.load_place(&cp_slot, &JrgType::Rune);
                    (ik.clone(), self.kotak_any(&cp, nol)?)
                }
                _ => (ik.clone(), ik.clone()),
            };
            let v = if self.butuh_salin(&JrgType::Any) { self.salin_dalam(&v) } else { v };
            self.emit(format!("store %JrgAny {}, ptr {}", k.text, k_slot));
            self.emit(format!("store %JrgAny {}, ptr {}", v.text, v_slot));
            self.terminate(format!("br label %{}", isi));
        }
        self.start_block(mustahil);
        self.terminate("unreachable".to_string());

        self.start_block(isi);
        self.push_scope();
        for (nama, slot) in vars.iter().zip([&k_slot, &v_slot]) {
            if nama == "_" { continue; }
            let v = self.load_place(slot, &JrgType::Any);
            let s = self.alloca_local(nama, JrgType::Any);
            self.emit(format!("store %JrgAny {}, ptr {}", v.text, s));
        }
        self.push_loop(LoopCtx { label: label.clone(), continue_bb: pasca.clone(), break_bb: usai.clone() });
        self.codegen_stmt_block(body)?;
        self.pop_loop();
        self.pop_scope();
        if !self.is_terminated() { self.terminate(format!("br label %{}", pasca)); }

        self.start_block(pasca);
        let iv3 = self.load_place(&i_slot, &JrgType::I64);
        let inc = self.fresh("fbinc.");
        self.emit(format!("{} = add i64 {}, 1", inc, iv3.text));
        self.emit(format!("store i64 {}, ptr {}", inc, i_slot));
        self.terminate(format!("br label %{}", cek));
        self.start_block(usai);
        Ok(())
    }

    /// `for` atas nilai yang tipenya sudah pasti.
    fn for_atas(&mut self, vars: &[Ident], iv: Val, body: &Block, label: &Option<String>,
                menumpang: bool) -> Result<(), String> {
        match iv.ty.clone() {
            JrgType::I8 | JrgType::I16 | JrgType::I32 | JrgType::I64 | JrgType::I128
            | JrgType::U8 | JrgType::U16 | JrgType::U32 | JrgType::U64 | JrgType::U128 => {
                let lo_v = Val { text: "0".into(), ty: iv.ty.clone() };
                self.codegen_counting_loop(vars, lo_v, iv, false, body, label)
            }
            JrgType::Array(elem, n) => {
                let base = self.spill_to_place(&iv);
                let bp = self.fresh("frab.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i64 0, i64 0", bp, iv.ty.llvm_ty(), base));
                self.codegen_for_elems(vars, &bp, &elem, Val { text: n.to_string(), ty: JrgType::I64 }, body, label, menumpang)
            }
            JrgType::Slice(elem) => {
                let base = self.spill_to_place(&iv);
                let pfield = self.fresh("frsp.");
                self.emit(format!("{} = getelementptr %JrgSlice, ptr {}, i32 0, i32 0", pfield, base));
                let dataptr = self.fresh("frsd.");
                self.emit(format!("{} = load ptr, ptr {}", dataptr, pfield));
                let lfield = self.fresh("frsl.");
                self.emit(format!("{} = getelementptr %JrgSlice, ptr {}, i32 0, i32 1", lfield, base));
                let lenv = self.fresh("frslv.");
                self.emit(format!("{} = load i64, ptr {}", lenv, lfield));
                self.codegen_for_elems(vars, &dataptr, &elem, Val { text: lenv, ty: JrgType::I64 }, body, label, menumpang)
            }
            JrgType::Str => self.codegen_for_str_chars(vars, &iv, body, label),
            JrgType::Dict(vty) => self.codegen_for_dict(vars, &iv, &vty, body, label),
            // Dict yang nilainya berlainan jenis: penyangganya dict bernilai bertag, cuma tabel per-kuncinya yang hilang.
            JrgType::DictCampur(_) => {
                let d = self.coerce(&iv, &JrgType::Dict(Box::new(JrgType::Any)));
                self.codegen_for_dict(vars, &d, &JrgType::Any, body, label)
            }
            // Kalimat acuan saat jalan, sesudah iterable-nya dievaluasi; acuan tidak berlokasi.
            other => {
                let pesan = crate::interpreter::pesan_tak_bisa_iterasi(&self.nama_acuan(&other));
                let situs_lama = std::mem::replace(&mut self.tanpa_situs, true);
                let h = self.gagal_sesudah_argumen(&pesan, &[]);
                self.tanpa_situs = situs_lama;
                h.map(|_| ())
            }
        }
    }

    pub(crate) fn codegen_counting_loop(&mut self, vars: &[Ident], lo: Val, hi: Val, inclusive: bool,
                             body: &Block, label: &Option<String>) -> Result<(), String> {
        let ity = lo.ty.clone();
        let idx_slot = self.alloca_typed("loopidx.", &ity);
        self.emit(format!("store {} {}, ptr {}", ity.llvm_ty(), lo.text, idx_slot));

        let cond_lbl = self.fresh_label("forr.cond.");
        let body_lbl = self.fresh_label("forr.body.");
        let post_lbl = self.fresh_label("forr.post.");
        let end_lbl = self.fresh_label("forr.end.");
        self.terminate(format!("br label %{}", cond_lbl));

        self.start_block(cond_lbl.clone());
        let curv = self.load_place(&idx_slot, &ity);
        let pred = if inclusive {
            if ity.is_signed_int() { "sle" } else { "ule" }
        } else if ity.is_signed_int() { "slt" } else { "ult" };
        let cmp = self.fresh("frc.");
        self.emit(format!("{} = icmp {} {} {}, {}", cmp, pred, ity.llvm_ty(), curv.text, hi.text));
        self.terminate(format!("br i1 {}, label %{}, label %{}", cmp, body_lbl, end_lbl));

        self.start_block(body_lbl);
        self.push_scope();
        let curv2 = self.load_place(&idx_slot, &ity);
        // Nama pertama adalah INDEKS (`cur - lo`), nama kedua nilainya — aturan acuan:
        // `for i in 5..8` → 0 1 2. Untuk `lo` literal 0 keduanya sama, jadi IR-nya tetap.
        if !vars.is_empty() && vars[0] != "_" {
            let idx = if lo.text == "0" { curv2.text.clone() } else {
                let d = self.fresh("fri.");
                self.emit(format!("{} = sub {} {}, {}", d, ity.llvm_ty(), curv2.text, lo.text));
                d
            };
            let s = self.alloca_local(&vars[0], ity.clone());
            self.emit(format!("store {} {}, ptr {}", ity.llvm_ty(), idx, s));
        }
        if vars.len() == 2 && vars[1] != "_" {
            let s = self.alloca_local(&vars[1], ity.clone());
            self.emit(format!("store {} {}, ptr {}", ity.llvm_ty(), curv2.text, s));
        }
        self.push_loop(LoopCtx { label: label.clone(), continue_bb: post_lbl.clone(), break_bb: end_lbl.clone() });
        self.codegen_stmt_block(body)?;
        self.pop_loop();
        self.pop_scope();
        if !self.is_terminated() { self.terminate(format!("br label %{}", post_lbl)); }

        self.start_block(post_lbl);
        let curv3 = self.load_place(&idx_slot, &ity);
        let nextv = self.fresh("frn.");
        self.emit(format!("{} = add {} {}, 1", nextv, ity.llvm_ty(), curv3.text));
        self.emit(format!("store {} {}, ptr {}", ity.llvm_ty(), nextv, idx_slot));
        if !self.is_terminated() { self.terminate(format!("br label %{}", cond_lbl)); }

        self.start_block(end_lbl);
        Ok(())
    }

    pub(crate) fn codegen_for_elems(&mut self, vars: &[Ident], base_ptr: &str, elem: &JrgType, len: Val,
                         body: &Block, label: &Option<String>, menumpang: bool) -> Result<(), String> {
        let idx_slot = self.alloca_raw("feidx.", "i64", Some(8));
        self.emit(format!("store i64 0, ptr {}", idx_slot));

        let cond_lbl = self.fresh_label("fore.cond.");
        let body_lbl = self.fresh_label("fore.body.");
        let post_lbl = self.fresh_label("fore.post.");
        let end_lbl = self.fresh_label("fore.end.");
        self.terminate(format!("br label %{}", cond_lbl));

        self.start_block(cond_lbl.clone());
        let curv = self.load_place(&idx_slot, &JrgType::I64);
        let cmp = self.fresh("fec.");
        self.emit(format!("{} = icmp slt i64 {}, {}", cmp, curv.text, len.text));
        self.terminate(format!("br i1 {}, label %{}, label %{}", cmp, body_lbl, end_lbl));

        self.start_block(body_lbl);
        self.push_scope();
        let curv2 = self.load_place(&idx_slot, &JrgType::I64);
        let ep = self.fresh("fee.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", ep, elem.llvm_ty(), base_ptr, curv2.text));
        let ev = self.load_place(&ep, elem);
        if vars.len() == 1 {
            if vars[0] != "_" {
                let s = self.alloca_local(&vars[0], JrgType::I64);
                self.emit(format!("store i64 {}, ptr {}", curv2.text, s));
            }
        } else {
            if vars[0] != "_" {
                let s = self.alloca_local(&vars[0], JrgType::I64);
                self.emit(format!("store i64 {}, ptr {}", curv2.text, s));
            }
            if vars[1] != "_" {
                // Elemen yang memuat penyangga adalah nilai baru bagi badannya, kecuali
                // badannya cuma membaca dan sumbernya tidak bisa berubah selama itu.
                let ev = if !self.butuh_salin(elem) { ev }
                    else if menumpang { self.pinjaman.insert(vars[1].clone()); ev }
                    else { self.salin_dalam(&ev) };
                let s = self.alloca_local(&vars[1], elem.clone());
                if *elem != JrgType::Unit { self.emit(format!("store {} {}, ptr {}", elem.llvm_ty(), ev.text, s)); }
            }
        }
        self.push_loop(LoopCtx { label: label.clone(), continue_bb: post_lbl.clone(), break_bb: end_lbl.clone() });
        self.codegen_stmt_block(body)?;
        self.pop_loop();
        self.pop_scope();
        if !self.is_terminated() { self.terminate(format!("br label %{}", post_lbl)); }

        self.start_block(post_lbl);
        let curv3 = self.load_place(&idx_slot, &JrgType::I64);
        let nextv = self.fresh("fen.");
        self.emit(format!("{} = add i64 {}, 1", nextv, curv3.text));
        self.emit(format!("store i64 {}, ptr {}", nextv, idx_slot));
        if !self.is_terminated() { self.terminate(format!("br label %{}", cond_lbl)); }

        self.start_block(end_lbl);
        Ok(())
    }

    pub(crate) fn codegen_for_str_chars(&mut self, vars: &[Ident], sv: &Val, body: &Block,
                             label: &Option<String>) -> Result<(), String> {
        self.ensure_extern("declare i32 @jrg_str_next_char(ptr, i64, i64, ptr, ptr)", "jrg_str_next_char");
        let (sp, sl) = self.split_str(sv);
        let off_slot = self.alloca_raw("fsoff.", "i64", Some(8));
        self.emit(format!("store i64 0, ptr {}", off_slot));
        let idx_slot = self.alloca_raw("fsidx.", "i64", Some(8));
        self.emit(format!("store i64 0, ptr {}", idx_slot));

        let cond_lbl = self.fresh_label("fors.cond.");
        let body_lbl = self.fresh_label("fors.body.");
        let post_lbl = self.fresh_label("fors.post.");
        let end_lbl = self.fresh_label("fors.end.");
        self.terminate(format!("br label %{}", cond_lbl));

        // Bita ASCII didekode langsung di sini; selain itu lewat runtime. Dulu tiap karakter satu panggilan.
        self.start_block(cond_lbl.clone());
        let offv = self.load_place(&off_slot, &JrgType::I64);
        let cpslot = self.alloca_raw("fscp.", "i32", Some(4));
        let noffslot = self.alloca_raw("fsno.", "i64", Some(8));
        let (ada, l_cek, l_ascii, l_lambat) = (self.fresh("fsada."), self.fresh_label("fs.cek."),
            self.fresh_label("fs.ascii."), self.fresh_label("fs.lambat."));
        self.emit(format!("{} = icmp slt i64 {}, {}", ada, offv.text, sl));
        self.terminate(format!("br i1 {}, label %{}, label %{}", ada, l_cek, end_lbl));
        self.start_block(l_cek);
        let (bp, bv, asc) = (self.fresh("fsbp."), self.fresh("fsbv."), self.fresh("fsasc."));
        self.emit(format!("{} = getelementptr i8, ptr {}, i64 {}", bp, sp, offv.text));
        self.emit(format!("{} = load i8, ptr {}", bv, bp));
        self.emit(format!("{} = icmp sge i8 {}, 0", asc, bv));
        self.terminate(format!("br i1 {}, label %{}, label %{}", asc, l_ascii, l_lambat));
        self.start_block(l_ascii);
        let (cpz, no) = (self.fresh("fscpz."), self.fresh("fsno1."));
        self.emit(format!("{} = zext i8 {} to i32", cpz, bv));
        self.emit(format!("store i32 {}, ptr {}", cpz, cpslot));
        self.emit(format!("{} = add i64 {}, 1", no, offv.text));
        self.emit(format!("store i64 {}, ptr {}", no, noffslot));
        self.terminate(format!("br label %{}", body_lbl));
        self.start_block(l_lambat);
        let hasmore = self.fresh("fshm.");
        self.emit(format!("{} = call i32 @jrg_str_next_char(ptr {}, i64 {}, i64 {}, ptr {}, ptr {})",
            hasmore, sp, sl, offv.text, cpslot, noffslot));
        let cmp = self.fresh("fscmp.");
        self.emit(format!("{} = icmp ne i32 {}, 0", cmp, hasmore));
        self.terminate(format!("br i1 {}, label %{}, label %{}", cmp, body_lbl, end_lbl));

        self.start_block(body_lbl);
        self.push_scope();
        let cpv = self.load_place(&cpslot, &JrgType::I32);
        let idxv = self.load_place(&idx_slot, &JrgType::I64);
        if vars.len() == 1 {
            if vars[0] != "_" {
                let s = self.alloca_local(&vars[0], JrgType::I64);
                self.emit(format!("store i64 {}, ptr {}", idxv.text, s));
            }
        } else {
            if vars[0] != "_" {
                let s = self.alloca_local(&vars[0], JrgType::I64);
                self.emit(format!("store i64 {}, ptr {}", idxv.text, s));
            }
            if vars[1] != "_" {
                let s = self.alloca_local(&vars[1], JrgType::Rune);
                self.emit(format!("store i32 {}, ptr {}", cpv.text, s));
            }
        }
        self.push_loop(LoopCtx { label: label.clone(), continue_bb: post_lbl.clone(), break_bb: end_lbl.clone() });
        self.codegen_stmt_block(body)?;
        self.pop_loop();
        self.pop_scope();
        if !self.is_terminated() { self.terminate(format!("br label %{}", post_lbl)); }

        self.start_block(post_lbl);
        let nov = self.load_place(&noffslot, &JrgType::I64);
        self.emit(format!("store i64 {}, ptr {}", nov.text, off_slot));
        let idxv2 = self.load_place(&idx_slot, &JrgType::I64);
        let nidx = self.fresh("fsni.");
        self.emit(format!("{} = add i64 {}, 1", nidx, idxv2.text));
        self.emit(format!("store i64 {}, ptr {}", nidx, idx_slot));
        if !self.is_terminated() { self.terminate(format!("br label %{}", cond_lbl)); }

        self.start_block(end_lbl);
        Ok(())
    }
}

fn iter_span(e: &Expr) -> Option<crate::lexer::Span> {
    match e { Expr::Call { span, .. } => Some(*span), _ => None }
}
