// src/vm/kom_stmt.rs — kompilasi pernyataan

use super::opcode::OpCode;
use super::value::VmValue;
use crate::parser::*;
use super::value::VmFunction;
use super::kom_pola::*;
use super::compiler::*;

impl Compiler {
    // ── Statement ─────────────────────────────────────────────────────────

    pub(crate) fn compile_stmt(&mut self, stmt: &Stmt) -> CR<()> {
        match stmt {

            // let pat = expr
            Stmt::Let(ls) => {
                match &ls.pattern {
                    // Bentuk biasa: satu nama.
                    Pattern::Ident { name, .. } if ident_varian_unit(name).is_none() => {
                        self.compile_value_expr(&ls.init)?;
                        let name = pat_name(&ls.pattern);
                        self.define_local(&name);
                    }
                    Pattern::Wildcard(_) => {
                        self.compile_value_expr(&ls.init)?;
                        let name = pat_name(&ls.pattern);
                        self.define_local(&name);
                    }
                    // Destructuring: `let (a, b) = ...`, `let [x, ...sisa] = ...`, `let Titik {
                    // x, y } = ...`.
                    pat => {
                        self.compile_value_expr(&ls.init)?;
                        let slot = self.slot_berikutnya();
                        self.define_local("__destructure__");

                        let mut gagal = Vec::new();
                        self.compile_pattern_conditions(
                            pat,
                            &PatternSource::Local(slot),
                            &mut gagal,
                        )?;
                        let cocok = self.chunk.emit_jump(OpCode::Jump);
                        for j in gagal {
                            self.chunk.patch_jump(j);
                        }
                        // Jalur tidak cocok: nilainya masih di puncak stack.
                        let asal = crate::parser::asal_let(ls) as u32;
                        let lama = self.chunk.masuk_span(ls.span);
                        self.chunk.emit_arg(OpCode::GagalCocok, 2 + asal);
                        self.chunk.pulih_span(lama);
                        self.chunk.patch_jump(cocok);

                        let mut bindings = Vec::new();
                        self.bind_pattern(pat, slot, &mut bindings)?;
                    }
                }
            }

            // var name = expr?
            Stmt::Var(vs) => {
                let nol = vs.ty.as_ref().and_then(|t| crate::parser::nilai_nol(t, vs.span));
                if let Some(init) = vs.init.as_deref().or(nol.as_ref()) { self.compile_value_expr(init)?; }
                else { self.chunk.emit(OpCode::Nil); }
                self.define_local(&vs.name);
            }

            // const (inner scope)
            Stmt::Const(cd) => {
                self.compile_value_expr(&cd.value)?;
                self.define_local(&cd.name);
            }

            // name, name2 := expr, expr2
            Stmt::ShortDecl(sd) => {
                for v in &sd.values { self.compile_value_expr(v)?; }
                // Jika lebih banyak names dari values, pad dengan Nil
                for _ in sd.values.len()..sd.names.len() {
                    self.chunk.emit(OpCode::Nil);
                }
                for name in &sd.names { self.define_local(name); }
            }

            // targets = values  (or +=, -=, dll)
            Stmt::Assign(as_) => self.compile_assignment(as_)?,

            // expr; (statement context)
            Stmt::Expr(e) => {
                // `if` in statement position emits the original value-less
                // lowering rather than pushing a branch value just to pop
                // it, so existing bytecode for plain `if` is unchanged.
                if let Expr::If(is) = e.as_ref() {
                    self.compile_if_stmt(is)?;
                } else {
                    self.compile_expr(e)?;
                    // Nama telanjang: diperiksa saat JALAN, bukan saat kompilasi —
                    // keluaran sebelumnya harus tetap tercetak, seperti Fase POLA-VARIAN.
                    let nama = match e.as_ref() {
                        Expr::Ident(n, _) => Some(n.clone()),
                        Expr::PathExpr { segments, .. } => Some(segments.join("::")),
                        _ => None,
                    };
                    match nama {
                        Some(n) => {
                            let i = self.chunk.add_name(&n);
                            // Span ekspresinya, bukan span statement yang
                            // berlaku sesudah `compile_expr` memulihkannya —
                            // tanpa ini galatnya lahir tanpa lokasi (Fase SPAN-VM).
                            let lama = self.chunk.masuk_span(e.span());
                            self.chunk.emit_arg(OpCode::PopPeriksa, i);
                            self.chunk.pulih_span(lama);
                        }
                        None => { self.chunk.emit(OpCode::Pop); }
                    }
                }
            }

            // return [values]
            Stmt::Return(rs) => {
                if rs.values.is_empty() {
                    self.chunk.emit(OpCode::ReturnNil);
                } else if rs.values.len() == 1 {
                    self.compile_value_expr(&rs.values[0])?;
                    self.chunk.emit(OpCode::Return);
                } else {
                    // Tiap nilai menumpuk di stack sampai `BuildTuple`,
                    // jadi nilai ke-i dikompilasi dengan i temp di bawahnya.
                    for (i, value) in rs.values.iter().enumerate() {
                        self.dgn_temp(i, |c| c.compile_value_expr(value))?;
                    }
                    self.chunk.emit_arg(OpCode::BuildTuple, rs.values.len() as u32);
                    self.chunk.emit(OpCode::Return);
                }
            }

            // { stmts }
            Stmt::Block(blk) => {
                self.begin_scope();
                self.compile_block_stmts(blk)?;
                self.end_scope();
            }

            // Fungsi bersarang sudah dipasang lebih dulu oleh
            // `hoist_fn_bersarang` di awal blok, jadi di sini tidak ada lagi
            // yang perlu dikerjakan.
            Stmt::Fn(_) => {}

            // if cond { } [else if ...] [else { }]
            Stmt::If(is) => self.compile_if_stmt(is)?,

            // while cond { }
            Stmt::While(ws) => {
                let loop_start = self.chunk.code.len();
                let base = self.locals.len();
                self.loops.push(LoopCtx {
                    label: ws.label.clone(),
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                    break_base: base,
                    continue_base: base,
                });

                self.compile_expr(&ws.cond)?;
                let exit = self.chunk.emit_jump(OpCode::JumpFalse);

                self.begin_scope();
                self.compile_block_stmts(&ws.body)?;
                self.end_scope();

                let continues = self.loops.last().unwrap().continue_patches.clone();
                for patch in continues {
                    self.chunk.code[patch].arg = loop_start as u32;
                }
                self.chunk.emit_loop(loop_start);
                self.chunk.patch_jump(exit);

                let ctx = self.loops.pop().unwrap();
                for bp in ctx.break_patches { self.chunk.patch_jump(bp); }
            }

            // loop { }
            Stmt::Loop(ls) => {
                let loop_start = self.chunk.code.len();
                let base = self.locals.len();
                self.loops.push(LoopCtx {
                    label: ls.label.clone(),
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                    break_base: base,
                    continue_base: base,
                });

                self.begin_scope();
                self.compile_block_stmts(&ls.body)?;
                self.end_scope();

                let continues = self.loops.last().unwrap().continue_patches.clone();
                for patch in continues {
                    self.chunk.code[patch].arg = loop_start as u32;
                }
                self.chunk.emit_loop(loop_start);

                let ctx = self.loops.pop().unwrap();
                for bp in ctx.break_patches { self.chunk.patch_jump(bp); }
            }

            // for vars in iter { } / for init; cond; post { }
            Stmt::For(fs) => {
                match fs {
                    ForStmt::Range { vars, iter, body, label, .. } => {
                        if vars.len() > 2 {
                            return Err(CompileError::new("for-range mendukung maksimal 2 variabel"));
                        }
                        let break_base = self.locals.len();
                        // Rentang di kepala `for` tidak dijadikan array — satu
                        // aturan dengan interpreter (`Interpreter::rentang_for`).
                        if let Expr::Range { lo, hi, inclusive, .. } = iter.as_ref() {
                            let Some(hi_e) = hi else {
                                return Err(CompileError::new("range tanpa batas atas tidak didukung Bytecode VM"));
                            };
                            match lo {
                                Some(lo_e) => self.compile_expr(lo_e)?,
                                None => { self.chunk.emit_arg(OpCode::IntImm, 0); }
                            }
                            self.dgn_temp(1, |c| c.compile_expr(hi_e))?;
                            let lama = self.chunk.masuk_span(iter.span());
                            self.chunk.emit_arg(OpCode::MakeIterRentang, *inclusive as u32);
                            self.chunk.pulih_span(lama);
                        } else {
                            self.compile_expr(iter)?;
                            self.chunk.emit(OpCode::MakeIter);
                        }
                        // MakeIter meninggalkan [items, penanda_pasangan, idx].
                        let iter_slot = self.slot_berikutnya();
                        self.define_local("__iter_items__");
                        self.slot_berikutnya();
                        self.define_local("__iter_pasangan__");
                        let idx_slot = self.slot_berikutnya();
                        self.define_local("__iter_idx__");

                        let loop_start = self.chunk.code.len();
                        self.loops.push(LoopCtx {
                            label: label.clone(),
                            break_patches: Vec::new(),
                            continue_patches: Vec::new(),
                            break_base,
                            continue_base: self.locals.len(),
                        });

                        self.chunk.emit_arg(OpCode::IterAda, iter_slot as u32);
                        let exit = self.chunk.emit_jump(OpCode::JumpFalse);

                        // Satu variabel menerima index. Dua variabel menerima
                        // (index, value), sama seperti interpreter.
                        if let Some(index_name) = vars.first() {
                            self.chunk.emit_arg(OpCode::IterVar, (iter_slot * 2) as u32);
                            self.define_local(index_name);
                        }
                        if let Some(value_name) = vars.get(1) {
                            self.chunk.emit_arg(OpCode::IterVar, (iter_slot * 2 + 1) as u32);
                            self.define_local(value_name);
                        }

                        self.begin_scope();
                        self.compile_block_stmts(body)?;
                        self.end_scope();

                        for _ in 0..vars.len() {
                            self.chunk.emit(OpCode::Pop);
                            self.locals.pop();
                        }

                        // idx++
                        let increment_target = self.chunk.code.len();
                        let continues = self.loops.last().unwrap().continue_patches.clone();
                        for patch in continues {
                            self.chunk.code[patch].arg = increment_target as u32;
                        }
                        self.chunk.emit_arg(OpCode::IterMaju, idx_slot as u32);

                        self.chunk.emit_loop(loop_start);
                        self.chunk.patch_jump(exit);

                        // Natural exit membersihkan iterator. Jalur break sudah
                        // membersihkan slotnya sendiri sebelum melompat ke akhir.
                        self.chunk.emit(OpCode::Pop); // idx
                        self.chunk.emit(OpCode::Pop); // penanda pasangan
                        self.chunk.emit(OpCode::Pop); // items
                        self.locals.pop(); // idx
                        self.locals.pop(); // penanda
                        self.locals.pop(); // items

                        let ctx = self.loops.pop().unwrap();
                        for bp in ctx.break_patches { self.chunk.patch_jump(bp); }
                    }
                    ForStmt::Classic { init, cond, post, body, label, .. } => {
                        let break_base = self.locals.len();
                        self.begin_scope();
                        if let Some(init_s) = init { self.compile_stmt(init_s)?; }

                        let loop_start = self.chunk.code.len();
                        self.loops.push(LoopCtx {
                            label: label.clone(),
                            break_patches: Vec::new(),
                            continue_patches: Vec::new(),
                            break_base,
                            continue_base: self.locals.len(),
                        });

                        let exit = if let Some(c) = cond {
                            self.compile_expr(c)?;
                            Some(self.chunk.emit_jump(OpCode::JumpFalse))
                        } else { None };

                        self.begin_scope();
                        self.compile_block_stmts(body)?;
                        self.end_scope();
                        let post_target = self.chunk.code.len();
                        let continues = self.loops.last().unwrap().continue_patches.clone();
                        for patch in continues {
                            self.chunk.code[patch].arg = post_target as u32;
                        }
                        if let Some(post_s) = post { self.compile_stmt(post_s)?; }
                        self.chunk.emit_loop(loop_start);

                        if let Some(e) = exit { self.chunk.patch_jump(e); }
                        self.end_scope();

                        let ctx = self.loops.pop().unwrap();
                        for bp in ctx.break_patches { self.chunk.patch_jump(bp); }
                    }
                }
            }

            // break [label]
            Stmt::Break(stmt) => {
                let i = self.loop_tujuan(stmt.label.as_deref(), "break")?;
                // Keluar dari loop BERLABEL berarti melewati beberapa loop sekaligus, jadi
                // pembersihan stack dihitung dari basis loop tujuan — bukan dari yang …
                let base = self.loops[i].break_base;
                self.emit_runtime_cleanup(base);
                let bp = self.chunk.emit_jump(OpCode::Jump);
                self.loops[i].break_patches.push(bp);
            }

            // continue [label]
            Stmt::Continue(stmt) => {
                let i = self.loop_tujuan(stmt.label.as_deref(), "continue")?;
                let base = self.loops[i].continue_base;
                self.emit_runtime_cleanup(base);
                let patch = self.chunk.emit_jump(OpCode::Jump);
                self.loops[i].continue_patches.push(patch);
            }

            // defer expr — compile sebagai closure, push ke defer stack frame `defer
            // f(a, b)` — fungsi DAN argumennya dievaluasi SEKARANG, yang ditunda cuma …
            Stmt::Defer(ds) => {
                match ds.expr.as_ref() {
                    Expr::Call { func, args, .. } => {
                        self.compile_expr(func)?;
                        for (i, a) in args.iter().enumerate() {
                            self.dgn_temp(1 + i, |c| c.compile_value_expr(a))?;
                        }
                        self.chunk.emit_arg(OpCode::DeferPush, args.len() as u32);
                    }
                    Expr::Method { obj, method, args, .. } => {
                        // Penerima ikut dibekukan: `defer f.tutup()` memakai
                        // `f` yang berlaku di baris defer.
                        self.compile_expr(obj)?;
                        self.chunk.emit(OpCode::Dup);
                        let ni = self.chunk.add_name(method);
                        // Argc ikut, sama seperti situs method biasa: tanpa ini
                        // `arg >> 24` di sana terbaca 0 dan `defer f.tulis(x)`
                        // dituduh kurang argumen.
                        self.chunk.emit_arg(OpCode::GetMethod, ni | ((args.len() as u32) << 24));
                        self.chunk.emit(OpCode::Swap);
                        // Penerima defer dulu disalin di sini supaya "beku".
                        for (i, a) in args.iter().enumerate() {
                            self.dgn_temp(2 + i, |c| c.compile_value_expr(a))?;
                        }
                        self.chunk.emit_arg(OpCode::DeferPush, args.len() as u32 + 1);
                    }
                    lain => {
                        // Interpreter pun cuma menerima bentuk pemanggilan.
                        let _ = lain;
                        return Err(CompileError::new(
                            "`defer` harus diikuti pemanggilan fungsi atau method, \
                             mis. `defer tutup(f)` atau `defer f.close()`",
                        ));
                    }
                }
            }

            // match scrutinee { pat => body, ... }
            // Full implementation: jump table untuk semua arms
            Stmt::Match(ms) => {
                self.compile_match(&ms.scrutinee, &ms.arms)?;
                self.chunk.emit(OpCode::Pop);
            }

            // spawn { body } — jalankan block sebagai goroutine OS thread
            // ── Spawn goroutine ──────────────────────────────────────
            Stmt::Spawn(ss) => {
                let captures: Vec<(String, usize)> = if let Some(params) = &ss.params {
                    let mut captures = Vec::with_capacity(params.len());
                    for param in params {
                        let name = param_name(param);
                        if name == "_" {
                            return Err(CompileError::new(
                                "parameter capture spawn harus identifier",
                            ));
                        }
                        let slot = self.resolve_local(&name).ok_or_else(|| {
                            CompileError::new(format!(
                                "spawn tidak bisa menangkap local `{}` yang belum didefinisikan",
                                name,
                            ))
                        })?;
                        captures.push((name, slot));
                    }
                    captures
                } else {
                    self.locals
                        .iter()
                        .enumerate()
                        .filter(|(slot, local)| self.resolve_local(&local.name) == Some(*slot))
                        .map(|(slot, local)| (local.name.clone(), slot))
                        .collect()
                };
                let arity = u8::try_from(captures.len()).map_err(|_| {
                    CompileError::new("spawn menangkap lebih dari 255 local")
                })?;
                let mut c = Compiler::new("<goroutine>", arity);
                c.scope_depth = 1;
                for (name, _) in &captures {
                    let slot = c.locals.len();
                    c.locals.push(Local { name: name.clone(), depth: 1, slot });
                }
                c.compile_block_stmts(&ss.body)?;
                c.chunk.emit(OpCode::ReturnNil);
                c.chunk.local_count = c.locals.len();
                let fn_val = VmFunction::new("<goroutine>", arity, c.chunk);
                let ci = self.chunk.add_constant(VmValue::Function(std::sync::Arc::new(fn_val)));
                self.chunk.emit_arg(OpCode::Const, ci);
                for (_, slot) in &captures {
                    self.chunk.emit_arg(OpCode::LoadLocal, *slot as u32);
                }
                self.chunk.emit_arg(OpCode::Spawn, captures.len() as u32);
                self.chunk.emit(OpCode::Pop);
            }

            // ── Select ────────────────────────────────────────────────────
            Stmt::Select(ss) => {
                use crate::parser::SelectCase;
                enum CapturedSelectCase<'a> {
                    Recv {
                        names: &'a [String],
                        is_decl: bool,
                        chan_slot: usize,
                        body: &'a Block,
                    },
                    Send {
                        chan_slot: usize,
                        val_slot: usize,
                        body: &'a Block,
                    },
                    Let {
                        pattern: &'a Pattern,
                        chan_slot: usize,
                        body: &'a Block,
                    },
                }

                self.begin_scope();
                let mut captured = Vec::with_capacity(ss.cases.len());
                for (index, case) in ss.cases.iter().enumerate() {
                    match case {
                        SelectCase::Recv { names, is_decl, chan, body, .. } => {
                            if names.len() > 1 {
                                return Err(CompileError::new(
                                    "select receive Bytecode VM mendukung maksimal satu binding",
                                ));
                            }
                            self.compile_expr(chan)?;
                            self.chunk.emit(OpCode::CekChannelSelect);
                            let chan_slot = self.slot_berikutnya();
                            self.define_local(&format!("__select_chan_{}__", index));
                            captured.push(CapturedSelectCase::Recv {
                                names,
                                is_decl: *is_decl,
                                chan_slot,
                                body,
                            });
                        }
                        SelectCase::Send { chan, val, body, .. } => {
                            self.compile_expr(chan)?;
                            self.chunk.emit(OpCode::CekChannelSelect);
                            let chan_slot = self.slot_berikutnya();
                            self.define_local(&format!("__select_chan_{}__", index));
                            self.compile_expr(val)?;
                            let val_slot = self.slot_berikutnya();
                            self.define_local(&format!("__select_val_{}__", index));
                            captured.push(CapturedSelectCase::Send {
                                chan_slot,
                                val_slot,
                                body,
                            });
                        }
                        SelectCase::Let { stmt, body, .. } => {
                            let Expr::Recv { chan, .. } = stmt.init.as_ref() else {
                                return Err(CompileError::new(
                                    "select `case let` harus menerima dari channel",
                                ));
                            };
                            self.compile_expr(chan)?;
                            self.chunk.emit(OpCode::CekChannelSelect);
                            let chan_slot = self.slot_berikutnya();
                            self.define_local(&format!("__select_chan_{}__", index));
                            captured.push(CapturedSelectCase::Let {
                                pattern: &stmt.pattern,
                                chan_slot,
                                body,
                            });
                        }
                    }
                }

                let n = ss.cases.len() + if ss.default.is_some() { 1 } else { 0 };
                let poll_start = self.chunk.code.len();
                self.chunk.emit_arg(OpCode::SelectBegin, n as u32);
                for case in &captured {
                    let (slot, jenis) = match case {
                        CapturedSelectCase::Recv { chan_slot, .. } | CapturedSelectCase::Let { chan_slot, .. } => (*chan_slot, 0),
                        CapturedSelectCase::Send { chan_slot, .. } => (*chan_slot, 1),
                    };
                    self.chunk.emit_arg(OpCode::LoadLocal, slot as u32);
                    self.chunk.emit_arg(OpCode::CekTutupSelect, jenis);
                }
                let mut end_jumps: Vec<usize> = Vec::new();

                for case in captured {
                    match case {
                        CapturedSelectCase::Recv {
                            names,
                            is_decl,
                            chan_slot,
                            body,
                        } => {
                            self.chunk.emit_arg(OpCode::LoadLocal, chan_slot as u32);
                            let skip = self.chunk.emit_jump(OpCode::SelectRecvArm);
                            if let Some(name) = names.first() {
                                if is_decl {
                                    self.define_local(name);
                                } else if let Some(slot) = self.resolve_local(name) {
                                    self.chunk.emit_arg(OpCode::SimpanBuang, slot as u32);
                                } else if let Some(ui) = self.resolve_upval(name) {
                                    self.chunk.emit_arg(OpCode::StoreUpval, ui as u32);
                                    self.chunk.emit(OpCode::Pop);
                                } else if let Some((g, Milik::Var)) = super::compiler::milik_unit(name) {
                                    let ni = self.chunk.add_name(&g);
                                    self.chunk.emit_arg(OpCode::StoreGlobal, ni);
                                    self.chunk.emit(OpCode::Pop);
                                } else {
                                    // Sama dengan `x = v`: sasaran yang bukan variabel ditolak saat
                                    // JALAN — dulu galat kompilasi tanpa lokasi, dan tangkapan
                                    // closure maupun `var` modul ikut ditolak.
                                    self.chunk.emit(OpCode::Pop);
                                    let pesan = self.pesan_sasaran_tugas(name);
                                    self.emit_tolak_sasaran(&pesan);
                                }
                            } else {
                                self.chunk.emit(OpCode::Pop);
                            }
                            self.begin_scope();
                            self.compile_block_stmts(body)?;
                            self.end_scope();
                            if names.first().is_some() && is_decl {
                                self.locals.pop();
                                self.chunk.emit(OpCode::Pop);
                            }
                            end_jumps.push(self.chunk.emit_jump(OpCode::Jump));
                            self.chunk.patch_jump(skip);
                        }
                        CapturedSelectCase::Send { chan_slot, val_slot, body } => {
                            self.chunk.emit_arg(OpCode::LoadLocal, chan_slot as u32);
                            self.chunk.emit_arg(OpCode::LoadLocal, val_slot as u32);
                            let skip = self.chunk.emit_jump(OpCode::SelectSendArm);
                            self.begin_scope();
                            self.compile_block_stmts(body)?;
                            self.end_scope();
                            end_jumps.push(self.chunk.emit_jump(OpCode::Jump));
                            self.chunk.patch_jump(skip);
                        }
                        CapturedSelectCase::Let { pattern, chan_slot, body } => {
                            self.chunk.emit_arg(OpCode::LoadLocal, chan_slot as u32);
                            let skip = self.chunk.emit_jump(OpCode::SelectRecvArm);
                            let value_slot = self.slot_berikutnya();
                            self.define_local("__select_recv_value__");
                            let mut bindings = Vec::new();
                            self.bind_pattern(pattern, value_slot, &mut bindings)?;
                            self.begin_scope();
                            self.compile_block_stmts(body)?;
                            self.end_scope();
                            for _ in &bindings {
                                self.locals.pop();
                                self.chunk.emit(OpCode::Pop);
                            }
                            self.locals.pop();
                            self.chunk.emit(OpCode::Pop);
                            end_jumps.push(self.chunk.emit_jump(OpCode::Jump));
                            self.chunk.patch_jump(skip);
                        }
                    }
                }
                if let Some(default_body) = &ss.default {
                    self.chunk.emit(OpCode::SelectDefault);
                    self.begin_scope();
                    self.compile_block_stmts(default_body)?;
                    self.end_scope();
                    end_jumps.push(self.chunk.emit_jump(OpCode::Jump));
                }
                let retry_target = if ss.default.is_some() {
                    u32::MAX
                } else {
                    poll_start as u32
                };
                self.chunk.emit_arg(OpCode::SelectEnd, retry_target);
                for ej in end_jumps { self.chunk.patch_jump(ej); }
                self.end_scope();
            }

            Stmt::Error(_) => return Err(CompileError::new("statement AST tidak valid")),
        }
        Ok(())
    }

}
