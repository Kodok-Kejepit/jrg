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
        let iv = self.codegen_expr(iter, None)?;
        let menumpang = vars.len() == 2 && self.akar_lokal_polos(iter)
            && crate::interpreter::capture::blok_cuma_membaca(body, &vars[1])
            && !crate::interpreter::capture::blok_menulis(body, Self::nama_akar(iter).unwrap_or(""));
        // Nilai bertag diiterasi sebagai array bertag; isi yang bukan array gagal saat jalan.
        let iv = match &iv.ty {
            JrgType::Any => self.buka_terperiksa(&iv, &JrgType::Slice(Box::new(JrgType::Any))).unwrap_or(iv),
            _ => iv,
        };
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
            other => Err(format!("`for ... in`: tipe `{}` tidak bisa diiterasi", other.display_name())),
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

        self.start_block(cond_lbl.clone());
        let offv = self.load_place(&off_slot, &JrgType::I64);
        let cpslot = self.alloca_raw("fscp.", "i32", Some(4));
        let noffslot = self.alloca_raw("fsno.", "i64", Some(8));
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
