// src/interpreter/alur.rs — alur kendali:
// if/for/while/loop/match/select/defer

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

/// Apa yang diiterasi `for`: nilai biasa, atau rentang di kepalanya yang
/// tidak pernah dijadikan array.
pub(crate) enum SumberFor {
    Nilai(Value),
    Rentang { awal: i128, jumlah: u128 },
}

use crate::interpreter::{
    Value, Env, Signal, RuntimeError, EvalResult, Interpreter, JrgFunction, DeferredCall,
    ok_value, err_value, some_value, none_value,
    ChannelHandle, channel_send, channel_recv,
    channel_try_send, channel_try_recv,
};
use crate::runtime_err;


use crate::interpreter::eval_stmt::*;

impl Interpreter {
    pub(crate) fn exec_short_decl(&mut self, s: &ShortDeclStmt, env: &mut Env) -> EvalResult {
        let mut vals: Vec<Value> = Vec::with_capacity(s.values.len());
        for e in &s.values { vals.push(self.eval_expr(e, env)?); }
        if s.names.len() == vals.len() {
            for (name, val) in s.names.iter().zip(vals) {
                if name != "_" { env.define(name, val); }
            }
        } else if vals.len() == 1 {
            match vals.into_iter().next() {
                Some(Value::Tuple(tv)) => {
                    for (name, val) in s.names.iter().zip(tv) {
                        if name != "_" { env.define(name, val); }
                    }
                }
                Some(single) => {
                    if let Some(name) = s.names.first() {
                        if name != "_" { env.define(name, single); }
                    }
                }
                None => {}
            }
        } else {
            return runtime_err!("short decl: {} nama tapi {} nilai", s.names.len(), vals.len());
        }
        Ok(Value::Unit)
    }

    /// Shared by statement-position `if` and expression-position `if`
    /// (`Expr::If`), exactly as `eval_match_arms` is shared by `Stmt::Match` and …
    pub(crate) fn exec_if(&mut self, s: &IfStmt, env: &mut Env) -> EvalResult {
        env.push_scope();
        if let Some(init) = &s.init { self.exec_stmt(init, env)?; }
        let cv = self.eval_expr(&s.cond, env)?;
        let result = if self.truthy(&cv) {
            self.exec_block(&s.then, env)
        } else {
            let mut matched = false;
            let mut br = Ok(Value::Unit);
            for (init_opt, cond_e, blk) in &s.else_ifs {
                if let Some(si) = init_opt { self.exec_stmt(si, env)?; }
                let ec = self.eval_expr(cond_e, env)?;
                if self.truthy(&ec) { br = self.exec_block(blk, env); matched = true; break; }
            }
            if matched { br }
            else if let Some(eb) = &s.else_ { self.exec_block(eb, env) }
            else { Ok(Value::Unit) }
        };
        env.pop_scope();
        result
    }

    pub(crate) fn exec_for(&mut self, s: &ForStmt, env: &mut Env) -> EvalResult {
        match s {
            ForStmt::Classic { init, cond, post, body, label, .. } => {
                env.push_scope();
                if let Some(si) = init { self.exec_stmt(si, env)?; }
                loop {
                    if let Some(c) = cond {
                        let cv = self.eval_expr(c, env)?;
                        if !self.truthy(&cv) { break; }
                    }
                    match self.exec_block(body, env) {
                        Ok(_) => {}
                        // Sinyal tanpa label, atau labelnya cocok dengan
                        // label loop INI (lihat `signal_is_for_me`),
                        // berarti loop ini yang dituju.
                        Err(Signal::Break(l)) if Self::signal_is_for_me(&l, label) => break,
                        Err(Signal::Break(l))        => { env.pop_scope(); return Err(Signal::Break(l)); }
                        Err(Signal::Continue(l)) if Self::signal_is_for_me(&l, label) => {}
                        Err(Signal::Continue(l))     => { env.pop_scope(); return Err(Signal::Continue(l)); }
                        Err(e)                       => { env.pop_scope(); return Err(e); }
                    }
                    if let Some(p) = post { self.exec_stmt(p, env)?; }
                }
                env.pop_scope();
                Ok(Value::Unit)
            }
            ForStmt::Range { vars, iter, body, label, .. } => {
                // Rentang yang ditulis di kepala `for` tidak dijadikan array:
                // artinya sama persis, tanpa batas `MAKS_ELEMEN`.
                let sumber = match iter.as_ref() {
                    Expr::Range { lo, hi, inclusive, .. } =>
                        self.rentang_for(lo.as_deref(), hi.as_deref(), *inclusive, env)
                            .map_err(|s| match s {
                                Signal::RuntimeError(e) => Signal::RuntimeError(
                                    e.fill_span(iter.span()).isi_span_bingkai(iter.span())),
                                lain => lain,
                            })?,
                    _ => SumberFor::Nilai(self.eval_expr(iter, env)?),
                };
                env.push_scope();
                let result = self.exec_for_range(vars, sumber, body, label, env);
                env.pop_scope();
                result
            }
        }
    }

    /// Apakah sebuah sinyal `break`/`continue` menyasar loop dengan label
    /// `label_loop`?
    pub(crate) fn signal_is_for_me(sig_label: &Option<String>, label_loop: &Option<String>) -> bool {
        match sig_label {
            None => true,
            Some(l) => label_loop.as_deref() == Some(l.as_str()),
        }
    }

    /// Batas `a..b`/`a..=b` di kepala `for`, dengan aturan dan kalimat yang
    /// sama dengan `Expr::Range` — cuma jumlahnya yang tidak dibatasi.
    fn rentang_for(
        &mut self, lo: Option<&Expr>, hi: Option<&Expr>, inklusif: bool, env: &mut Env,
    ) -> Result<SumberFor, Signal> {
        let lo_v = match lo { Some(le) => self.eval_expr(le, env)?, None => Value::Int(0) };
        let hi_v = match hi {
            Some(he) => self.eval_expr(he, env)?,
            None => return runtime_err!("range tanpa batas atas tidak didukung"),
        };
        let nama = if inklusif { "range_inclusive" } else { "range" };
        let g = |m: String| Signal::RuntimeError(RuntimeError::new(m));
        let (awal, akhir) = crate::interpreter::range_batas(nama,
            &crate::interpreter::arg_angka(&lo_v), &crate::interpreter::arg_angka(&hi_v)).map_err(g)?;
        let jumlah = crate::interpreter::range_hitung(nama, awal, akhir, 1, inklusif).map_err(g)?;
        Ok(SumberFor::Rentang { awal, jumlah })
    }

    pub(crate) fn exec_for_range(
        &mut self,
        vars: &[String],
        sumber: SumberFor,
        body: &Block,
        label: &Option<String>,
        env: &mut Env,
    ) -> EvalResult {
        macro_rules! bind_run {
            ($idx:expr, $val:expr) => {{
                match vars.len() {
                    0 => {}
                    1 => env.define_ulang(&vars[0], Value::Int($idx as i128)),
                    2 => { env.define_ulang(&vars[0], Value::Int($idx as i128)); env.define_ulang(&vars[1], $val); }
                    _ => return runtime_err!("for-range: max 2 variabel"),
                }
                match self.exec_block(body, env) {
                    Ok(_) => {}
                    Err(Signal::Break(l)) if Self::signal_is_for_me(&l, label) => return Ok(Value::Unit),
                    Err(Signal::Break(l))        => return Err(Signal::Break(l)),
                    Err(Signal::Continue(l)) if Self::signal_is_for_me(&l, label) => {}
                    Err(Signal::Continue(l))     => return Err(Signal::Continue(l)),
                    Err(e)                       => return Err(e),
                }
            }}
        }
        let iter_val = match sumber {
            SumberFor::Nilai(v) => v,
            // Elemen ke-k adalah `awal + k`; nama pertama tetap INDEKS-nya,
            // persis seperti kalau rentangnya sudah jadi array.
            SumberFor::Rentang { awal, jumlah } => {
                let mut k: u128 = 0;
                while k < jumlah {
                    bind_run!(k as i128, Value::Int(awal.wrapping_add(k as i128)));
                    k += 1;
                }
                return Ok(Value::Unit);
            }
        };
        match iter_val {
            Value::Array(elems) => {
                for (i, v) in elems.into_iter().enumerate() { bind_run!(i, v); }
            }
            Value::Str(s) => {
                for (i, c) in s.chars().enumerate() { bind_run!(i, Value::Rune(c)); }
            }
            Value::Int(n) => {
                for i in 0..n { bind_run!(i as usize, Value::Int(i)); }
            }
            // Dict ikut aturan yang sama dengan koleksi lain: yang didapat variabel
            // pertama adalah "alamat" elemennya — index untuk array/string, KEY untuk …
            Value::Struct { ref name, ref fields } if name == crate::interpreter::builtin::dict::DICT_TYPE_NAME => {
                let mut keys: Vec<String> = fields.keys().cloned().collect();
                keys.sort();
                for k in keys {
                    let v = fields[&k].clone();
                    match vars.len() {
                        0 => {}
                        1 => env.define(&vars[0], Value::Str(k.clone())),
                        2 => {
                            env.define(&vars[0], Value::Str(k.clone()));
                            env.define(&vars[1], v);
                        }
                        _ => return runtime_err!("for-in dict: paling banyak 2 variabel (`for key, nilai in d`)"),
                    }
                    match self.exec_block(body, env) {
                        Ok(_) => {}
                        Err(Signal::Break(l)) if Self::signal_is_for_me(&l, label) => return Ok(Value::Unit),
                        Err(Signal::Break(l))        => return Err(Signal::Break(l)),
                        Err(Signal::Continue(l)) if Self::signal_is_for_me(&l, label) => {}
                        Err(Signal::Continue(l))     => return Err(Signal::Continue(l)),
                        Err(e)                       => return Err(e),
                    }
                }
            }
            // Struct buatan pemakai sengaja TIDAK ikut diiterasi: fieldnya bagian dari
            // bentuknya, bukan koleksi data. Kalimatnya satu definisi dengan VM, dan
            // menyebut tipe — bukan isi — nilainya.
            other => return runtime_err!(
                "{}", crate::interpreter::pesan_tak_bisa_iterasi(&other.type_name())
            ),
        }
        Ok(Value::Unit)
    }

    pub(crate) fn exec_while(&mut self, s: &WhileStmt, env: &mut Env) -> EvalResult {
        loop {
            let cv = self.eval_expr(&s.cond, env)?;
            if !self.truthy(&cv) { break; }
            match self.exec_block(&s.body, env) {
                Ok(_) => {}
                Err(Signal::Break(l)) if Self::signal_is_for_me(&l, &s.label) => break,
                Err(Signal::Break(l))        => return Err(Signal::Break(l)),
                Err(Signal::Continue(l)) if Self::signal_is_for_me(&l, &s.label) => {}
                Err(Signal::Continue(l))     => return Err(Signal::Continue(l)),
                Err(e)                       => return Err(e),
            }
        }
        Ok(Value::Unit)
    }

    pub(crate) fn exec_loop(&mut self, s: &LoopStmt, env: &mut Env) -> EvalResult {
        loop {
            match self.exec_block(&s.body, env) {
                Ok(_) => {}
                Err(Signal::Break(l)) if Self::signal_is_for_me(&l, &s.label) => break,
                Err(Signal::Break(l))        => return Err(Signal::Break(l)),
                Err(Signal::Continue(l)) if Self::signal_is_for_me(&l, &s.label) => {}
                Err(Signal::Continue(l))     => return Err(Signal::Continue(l)),
                Err(e)                       => return Err(e),
            }
        }
        Ok(Value::Unit)
    }

    pub(crate) fn exec_match(&mut self, s: &MatchStmt, env: &mut Env) -> EvalResult {
        let scrutinee = self.eval_expr(&s.scrutinee, env)?;
        self.eval_match_arms(scrutinee, &s.arms, env)
    }

    /// Shared by `exec_match` (`Stmt::Match` — kept for AST/API compatibility,
    /// though the parser no longer produces it; see the comment on `KwMatch` in …
    pub(crate) fn eval_match_arms(&mut self, scrutinee: Value, arms: &[MatchArm], env: &mut Env) -> EvalResult {
        for arm in arms {
            // Diperiksa SEBELUM dicocokkan: pola yang salah bentuknya dulu
            // cuma dilewati diam-diam.
            for p in &arm.patterns {
                if let Err(e) = self.cek_pola(p, &scrutinee) {
                    return runtime_err!("{}", e);
                }
            }
            let matched_pat = arm.patterns.iter().find(|p| self.pat_matches(p, &scrutinee));
            let matched_pat = match matched_pat {
                Some(p) => p,
                None => continue,
            };
            env.push_scope();
            self.bind_pattern(matched_pat, scrutinee.clone(), env)?;
            if let Some(g) = &arm.guard {
                let gv = match self.eval_expr(g, env) {
                    Ok(v) => v,
                    Err(e) => { env.pop_scope(); return Err(e); }
                };
                if !self.truthy(&gv) {
                    env.pop_scope();
                    continue;
                }
            }
            let result = match &arm.body {
                MatchBody::Expr(e)  => self.eval_expr(e, env),
                MatchBody::Block(b) => self.exec_block(b, env),
            };
            env.pop_scope();
            return result;
        }
        if crate::parser::lengan_dari_koalesensi(arms) {
            return runtime_err!("{}", crate::interpreter::pesan_koalesensi_bukan_option(
                &scrutinee.type_name()));
        }
        runtime_err!("{}", crate::interpreter::pesan_match_tak_cocok(&format!("{}", scrutinee)))
    }

    pub(crate) fn exec_return(&mut self, s: &ReturnStmt, env: &mut Env) -> EvalResult {
        let v = match s.values.len() {
            0 => Value::Unit,
            1 => self.eval_expr(&s.values[0], env)?,
            _ => {
                let mut vs = Vec::with_capacity(s.values.len());
                for e in &s.values { vs.push(self.eval_expr(e, env)?); }
                Value::Tuple(vs.into())
            }
        };
        Err(Signal::Return(v))
    }

    /// Runs `spawn { ...
    pub(crate) fn spawn_goroutine(&mut self, sp: &SpawnStmt, env: &mut Env) -> Result<(), Signal> {
        // `thread::spawn`'s closure must be `'static` — Rust's type system simply
        // does not allow a borrowed reference into the spawning stack frame to cross …
        let mut thread_env = env.clone();
        thread_env.push_scope();
        // `spawn |x, ch| { ...
        if let Some(params) = &sp.params {
            for p in params {
                if let Pattern::Ident { name, .. } = &p.pattern {
                    let captured = env.get(name).unwrap_or(Value::Nil);
                    thread_env.define(name, captured);
                }
            }
        }
        let mut thread_interp = self.clone();
        // `defer` di dalam badan goroutine butuh frame yang sama seperti pemanggilan
        // fungsi biasa.
        thread_interp.defer_frames.push(Vec::new());
        let body = sp.body.clone();
        // Didaftarkan SEBELUM utasnya lahir: kalau sesudah, laporan bisa
        // terlanjur dicetak sebelum penghitungnya naik.
        crate::profiler::utas_baru();
        let spawn_result = std::thread::Builder::new()
            .name("jrg-goroutine".to_string())
            .spawn(move || {
                // Badan goroutine dilaporkan sebagai `<goroutine>`, nama yang SAMA dengan
                // yang dipakai VM.
                crate::profiler::enter("<goroutine>");
                let hasil = thread_interp.exec_block(&body, &mut thread_env);
                let hasil = thread_interp.run_defers(hasil);
                crate::profiler::exit();
                // Goroutine adalah thread biasa, jadi profilernya juga
                // thread-local — hasilnya disetor di sini supaya kerja di
                // dalam goroutine ikut terhitung, bukan hilang diam-diam.
                crate::profiler::flush();
                match hasil {
                    Ok(_) | Err(Signal::Return(_)) => {}
                    // A bare `break`/`continue` with nothing to break or continue out of at the
                    // goroutine's own top level is a user-code mistake — reported the same way …
                    Err(Signal::Break(l)) => crate::host::galat_baris(
                        &format!("goroutine error: stray signal: {}", Signal::Break(l))),
                    Err(Signal::Continue(l)) => crate::host::galat_baris(
                        &format!("goroutine error: stray signal: {}", Signal::Continue(l))),
                    Err(Signal::RuntimeError(e)) =>
                        crate::host::galat_baris(&format!("goroutine error: {}", e)),
                }
            });
        match spawn_result {
            Ok(handle) => {
                let mut guard = self.spawn_handles.lock().unwrap_or_else(|p| p.into_inner());
                guard.push(handle);
                Ok(())
            }
            // Dulu cuma dicetak lalu programnya jalan terus TANPA goroutine-nya.
            Err(e) => Err(Signal::RuntimeError(RuntimeError::new(
                crate::host::tanpa_thread("goroutine", &e)
                    .unwrap_or_else(|| format!("goroutine tidak bisa dibuat: {}", e)))
                .fill_span(sp.span))),
        }
    }

    /// Runs `select { case ...: ...
    pub(crate) fn exec_select(&mut self, sel: &SelectStmt, env: &mut Env) -> EvalResult {
        enum ResolvedCase<'a> {
            Send { chan: ChannelHandle, val: Value, body: &'a Block },
            Recv { chan: ChannelHandle, name: Option<&'a str>, is_decl: bool, body: &'a Block },
            LetRecv { chan: ChannelHandle, pattern: &'a Pattern, body: &'a Block },
        }

        fn as_channel(v: Value) -> Result<ChannelHandle, Signal> {
            match v {
                Value::Channel(c) => Ok(c),
                other => runtime_err!("{}", crate::interpreter::pesan_bukan_channel("`case` di `select`", &other.type_name())),
            }
        }

        let mut resolved: Vec<ResolvedCase> = Vec::with_capacity(sel.cases.len());
        for case in &sel.cases {
            match case {
                SelectCase::Send { chan, val, body, .. } => {
                    let chan_v = self.eval_expr(chan, env)?;
                    let val_v  = self.eval_expr(val, env)?;
                    let handle = as_channel(chan_v)?;
                    resolved.push(ResolvedCase::Send { chan: handle, val: val_v, body });
                }
                SelectCase::Recv { names, is_decl, chan, body, .. } => {
                    let chan_v = self.eval_expr(chan, env)?;
                    let handle = as_channel(chan_v)?;
                    resolved.push(ResolvedCase::Recv {
                        chan: handle, name: names.first().map(|s| s.as_str()), is_decl: *is_decl, body,
                    });
                }
                SelectCase::Let { stmt, body, .. } => {
                    let chan_expr = match stmt.init.as_ref() {
                        Expr::Recv { chan, .. } => chan.as_ref(),
                        other => return runtime_err!(
                            "select `case let ... = ...:` harus berupa channel receive (<-chan), dapat ekspresi lain: {:?}",
                            other
                        ),
                    };
                    let chan_v = self.eval_expr(chan_expr, env)?;
                    let handle = as_channel(chan_v)?;
                    resolved.push(ResolvedCase::LetRecv { chan: handle, pattern: &stmt.pattern, body });
                }
            }
        }

        let n = resolved.len();
        let poll_start_offset = || -> usize {
            if n == 0 { return 0; }
            crate::host::sejak_epoch()
                .map(|d| d.subsec_nanos() as usize)
                .unwrap_or(0) % n
        };

        loop {
            // Channel tertutup tidak pernah "siap": ditolak di tiap putaran, SEBELUM
            // memilih cabang, supaya jawabannya tidak bergantung urutan acaknya.
            for c in &resolved {
                match c {
                    ResolvedCase::Send { chan, .. } if chan.tertutup() =>
                        return runtime_err!("{}", crate::antrean::PESAN_KIRIM_TERTUTUP),
                    ResolvedCase::Recv { chan, .. } | ResolvedCase::LetRecv { chan, .. }
                        if chan.tertutup() && chan.panjang() == 0 =>
                        return runtime_err!("{}", crate::antrean::PESAN_TERIMA_TERTUTUP),
                    _ => {}
                }
            }
            let start = poll_start_offset();
            for i in 0..n {
                let idx = (start + i) % n;
                match &resolved[idx] {
                    ResolvedCase::Send { chan, val, body } => {
                        if channel_try_send(chan, val.clone()).is_ok() {
                            env.push_scope();
                            let r = self.exec_block(body, env);
                            env.pop_scope();
                            return r;
                        }
                    }
                    ResolvedCase::Recv { chan, name, is_decl, body } => {
                        if let Ok(v) = channel_try_recv(chan) {
                            env.push_scope();
                            if let Some(nm) = name {
                                if *is_decl {
                                    env.define(nm, v);
                                } else if !env.set_variabel(nm, v) {
                                    let pesan = env.pesan_sasaran_tugas(nm);
                                    env.pop_scope();
                                    return runtime_err!("{}", pesan);
                                }
                            }
                            let r = self.exec_block(body, env);
                            env.pop_scope();
                            return r;
                        }
                    }
                    ResolvedCase::LetRecv { chan, pattern, body } => {
                        if let Ok(v) = channel_try_recv(chan) {
                            env.push_scope();
                            let bind_result = self.bind_pattern(pattern, v, env);
                            let r = bind_result.and_then(|_| self.exec_block(body, env));
                            env.pop_scope();
                            return r;
                        }
                    }
                }
            }
            if let Some(default_block) = &sel.default {
                return self.exec_block(default_block, env);
            }
            // No case ready and no default: this is a blocking select.
            crate::host::tidur(std::time::Duration::from_micros(if n == 0 { 50_000 } else { 200 }));
        }
    }

    /// `defer f(...)` — catat pemanggilannya untuk dijalankan nanti, saat fungsi
    /// yang memuatnya selesai.
    pub(crate) fn exec_defer(&mut self, d: &DeferStmt, env: &mut Env) -> EvalResult {
        let beku = match d.expr.as_ref() {
            Expr::Call { func, args, .. } => {
                let callee = self.eval_expr(func.as_ref(), env)?;
                let mut vals = Vec::with_capacity(args.len());
                for a in args { vals.push(self.eval_expr(a, env)?); }
                DeferredCall::Fn { callee, args: vals }
            }
            Expr::Method { obj, method, args, .. } => {
                let ov = self.eval_expr(obj.as_ref(), env)?;
                let mut vals = Vec::with_capacity(args.len());
                for a in args { vals.push(self.eval_expr(a, env)?); }
                DeferredCall::Method { obj: ov, method: method.clone(), args: vals }
            }
            // `defer x + 1` tidak punya arti: tidak ada yang bisa ditunda
            // dari sebuah perhitungan yang hasilnya langsung dibuang.
            // Ditolak, bukan diam-diam dianggap tidak ada.
            _ => return runtime_err!(
                "`defer` harus diikuti pemanggilan fungsi atau method, mis. \
                 `defer tutup(f)` atau `defer f.close()`"
            ),
        };
        match self.defer_frames.last_mut() {
            Some(frame) => { frame.push(beku); Ok(Value::Unit) }
            // Di luar fungsi tidak ada "nanti" yang jelas untuk dijalankan.
            None => runtime_err!(
                "`defer` cuma bisa dipakai di dalam fungsi — di luar itu tidak ada \
                 titik selesai yang jelas buat menjalankannya"
            ),
        }
    }

    /// Jalankan semua `defer` milik frame teratas, dari yang paling akhir
    /// didaftarkan (LIFO, seperti Go), lalu buang frame-nya.
    pub(crate) fn run_defers(&mut self, hasil: EvalResult) -> EvalResult {
        let Some(frame) = self.defer_frames.pop() else { return hasil };
        let mut out = hasil;
        for beku in frame.into_iter().rev() {
            let r = match beku {
                DeferredCall::Fn { callee, args } => self.call_fn(callee, args),
                DeferredCall::Method { obj, method, args } => self.eval_method(obj, &method, args),
            };
            if let Err(Signal::RuntimeError(e)) = r {
                out = match out {
                    Ok(_) | Err(Signal::Return(_)) => Err(Signal::RuntimeError(e)),
                    // Kalimatnya ditambah di TEMPAT: galat baru membuang jejak yang sudah
                    // terkumpul dari bingkai-bingkai di dalamnya.
                    Err(Signal::RuntimeError(mut asli)) => {
                        asli.message = crate::interpreter::pesan_defer_ikut_gagal(
                            &asli.message, &e.message);
                        Err(Signal::RuntimeError(asli))
                    }
                    lain => lain,
                };
            }
        }
        out
    }

}
