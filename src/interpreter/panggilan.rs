// src/interpreter/panggilan.rs — blok, badan fungsi, dan pemanggilan

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


use crate::interpreter::eval_stmt::*;

impl Interpreter {
    pub fn exec_block(&mut self, block: &Block, env: &mut Env) -> EvalResult {
        env.push_scope();
        // `fn` bernama di dalam blok di-hoist duluan, persis seperti `fn` top-level.
        self.hoist_block_fns(block, env);
        let result = (|| -> EvalResult {
            for stmt in &block.stmts { self.exec_stmt(stmt, env)?; }
            if let Some(tail) = &block.tail { self.eval_expr(tail, env) }
            else { Ok(Value::Unit) }
        })();
        env.pop_scope();
        result
    }

    /// Ikat semua `fn` bernama di sebuah blok sebelum isinya dijalankan.
    pub(crate) fn hoist_block_fns(&mut self, block: &Block, env: &mut Env) {
        let decls: Vec<crate::parser::FnDecl> = block
            .stmts
            .iter()
            .filter_map(|st| match st {
                Stmt::Fn(fd) => Some(fd.clone()),
                _ => None,
            })
            .collect();
        if decls.is_empty() {
            return;
        }
        let group = std::sync::Arc::new(decls);
        for fd in group.iter() {
            let v = self.make_fn_val_in_group(fd, env, Some(std::sync::Arc::clone(&group)));
            env.define(&fd.name, v);
        }
    }

    pub fn exec_fn_body(&mut self, body: &FnBody, env: &mut Env) -> EvalResult {
        match body {
            FnBody::Block(b) => match self.exec_block(b, env) {
                Ok(v) | Err(Signal::Return(v)) => Ok(v),
                Err(e) => Err(e),
            },
            FnBody::Expr(e) => self.eval_expr(e, env),
            // Method abstrak tidak punya badan untuk dijalankan.
            FnBody::Abstrak => runtime_err!(
                "method ini dideklarasikan tanpa badan di trait-nya, jadi tidak ada \
                 yang bisa dijalankan — `impl` yang dipakai belum menyediakannya"
            ),
        }
    }

    // -----------------------------------------------------------------------
    // Expressions
    // -----------------------------------------------------------------------


    pub fn call_fn(&mut self, func: Value, args: Vec<Value>) -> EvalResult {
        self.call_fn_dari(func, args, None)
    }

    /// `callee`: ekspresi di situs panggilan LANGSUNG, supaya pesan jumlah argumen
    /// menyebut nama yang ditulis pemakai, bukan nama kanoniknya.
    pub fn call_fn_dari(&mut self, func: Value, args: Vec<Value>, callee: Option<&Expr>) -> EvalResult {
        match func {
            Value::Function(f) => self.call_jrg(f, args),
            Value::Closure { params, body, captures, globals, ruang } =>
                self.call_closure(params, body, captures, globals, ruang, args),
            Value::NativeFn(k) => {
                super::builtin::aritas::periksa(&k, args.len(), callee)
                    .map_err(|e| Signal::RuntimeError(RuntimeError::new(e)))?;
                self.call_native(k, args)
            }
            // Calling a tuple-variant constructor (`Ok(5)`, `Some(x)`, `Err("oops")`, or
            // any user enum's `Variant(payload)`) builds the corresponding …
            Value::EnumCtor { type_name, variant, arity } => {
                if args.len() != arity {
                    return runtime_err!("{}", crate::interpreter::pesan_konstruktor_argumen(
                        &type_name, &variant, arity, args.len()));
                }
                let payload = match args.len() {
                    0 => None,
                    1 => Some(Box::new(args.into_iter().next().unwrap())),
                    _ => Some(Box::new(Value::Tuple(args.into()))),
                };
                Ok(Value::Enum { type_name, variant, payload })
            }
            // Calling an extern "C" function — see `crate::ffi::native` for how
            // arguments are marshaled to the C ABI and the native call actually made.
            Value::ExternFn(info) => crate::ffi::native::call_extern_fn(&info, args),
            // TIPE-nya, bukan isinya: dulu nilainya dicetak dan pesannya tumbuh bersama data.
            other => runtime_err!("{}", crate::interpreter::pesan_bukan_fungsi(&other.type_name())),
        }
    }

    pub(crate) fn call_jrg(&mut self, func: impl Into<std::sync::Arc<JrgFunction>>, args: Vec<Value>) -> EvalResult {
        let func: std::sync::Arc<JrgFunction> = func.into();
        // `pemilik` hanya terisi kalau method ini dicapai lewat bentuk path
        // (`P::tambah`). Pemanggilan method biasa membiarkannya `None`, jadi
        // jalur terpanas tidak membayar apa pun di sini.
        if let Some(pemilik) = &func.pemilik {
            let tipe = match args.first() {
                Some(Value::Struct { name, .. }) => Some(name.clone()),
                Some(Value::Enum { type_name, .. }) => Some(type_name.clone()),
                _ => None,
            };
            let cocok = tipe.as_deref()
                .map(|t| self.lineage_of(t).iter().any(|n| n == pemilik))
                .unwrap_or(false);
            if !cocok {
                let dapat = args.first()
                    .map(|v| v.type_name())
                    .unwrap_or_else(|| "tidak ada argumen".to_string());
                return runtime_err!("{}", crate::interpreter::pesan_penerima_salah(
                    &func.name, pemilik, &dapat));
            }
        }
        // Parameter pengumpul menerima berapa pun sisanya, jadi batas atas
        // arity tidak berlaku untuknya.
        if !func.variadic && args.len() > func.params.len() {
            return runtime_err!("{}", crate::interpreter::pesan_fungsi_kebanyakan(
                &func.name, func.params.len(), args.len(), &func.params));
        }
        // Yang KURANG juga diperiksa sebelum bingkainya dibuka: galatnya milik situs
        // panggil, jadi jejaknya sama dengan `--vm` (dulu satu bingkai lebih).
        let tetap = func.params.len().saturating_sub(usize::from(func.variadic));
        if args.len() < tetap && func.defaults.get(args.len()).and_then(|d| d.as_ref()).is_none() {
            return runtime_err!("{}", crate::interpreter::pesan_parameter_wajib(
                &func.name, &func.params[args.len()], args.len() + 1));
        }
        // ── Batas kedalaman rekursi ───────────────────────────────────────
        // Interpreter ini tree-walking: satu level rekursi JRG memakai beberapa …
        self.call_depth += 1;
        if self.call_depth > Self::MAX_CALL_DEPTH {
            self.call_depth -= 1;
            return runtime_err!("{}", crate::interpreter::pesan_rekursi_dalam(
                &func.name, Self::MAX_CALL_DEPTH));
        }

        let ruang_lama = std::mem::replace(&mut self.ruang_kini, func.ruang.clone());
        // ── Profiler: catat masuk ─────────────────────────────────────────
        crate::profiler::enter(&func.name);
        // Berbagi `globals` (Arc::clone, O(1)) alih-alih deep-clone seluruh scope
        // global tiap panggilan — lihat dokumentasi di struct `Env` buat kenapa ini …
        let mut ce = Env::with_globals(func.env.globals.clone());
        ce.push_scope();

        // Ikat nama fungsi ke dirinya sendiri supaya badannya bisa memanggil dirinya
        // (rekursi).
        if let Some(group) = &func.group {
            for fd in group.iter() {
                let v = self.make_fn_val_in_group(fd, &ce, Some(std::sync::Arc::clone(group)));
                ce.define(&fd.name, v);
            }
        }
        // Cuma FUNGSI yang melihat namanya sendiri. Method (punya `ruang`) tidak: di VM
        // namanya tidak ada di scope, dan dulu default trait `sapa` membayangi fungsi global
        // `sapa`, sementara `P::f` di dalam method lolos tanpa pemeriksaan penerima.
        if !func.name.is_empty() && func.ruang.is_none() {
            ce.define(&func.name, Value::Function(func.clone()));
        }

        // Isi parameter satu-satu, BERURUTAN (bukan zip sekaligus) — penting supaya
        // default seperti `fn f(a: int, b: int = a + 1)` bisa merujuk `a` yang sudah …
        let mut args = args;
        // Pengumpulnya diikat TERPISAH dari argumen tetap: dulu array kosongnya didorong
        // ke ujung argumen, jadi `fn g(a, b = 2, sisa...)` lalu `g(1)` mengisi `b` dengan `[]`.
        let n = func.params.len();
        let mut pengumpul = if func.variadic {
            let kumpul = if args.len() >= n { args.split_off(n - 1) } else { Vec::new() };
            Some(Value::Array(kumpul.into()))
        } else { None };
        let mut args_it = args.into_iter();
        let mut bind_err: Option<Signal> = None;
        for (i, name) in func.params.iter().enumerate() {
            let val = if func.variadic && i + 1 == n {
                pengumpul.take().unwrap_or_else(|| Value::Array(Vec::new().into()))
            } else if let Some(v) = args_it.next() {
                v
            } else {
                match func.defaults.get(i).and_then(|d| d.as_ref()) {
                    Some(expr) => match self.eval_expr(expr, &mut ce) {
                        Ok(v)  => v,
                        Err(e) => { bind_err = Some(e); break; }
                    },
                    None => {
                        bind_err = Some(Signal::RuntimeError(RuntimeError::new(
                            crate::interpreter::pesan_parameter_wajib(&func.name, name, i + 1))));
                        break;
                    }
                }
            };
            ce.define(name, val);
        }

        // Frame `defer` dibuka SETELAH parameter terikat (sebuah default parameter
        // tidak bisa memuat `defer`) dan ditutup di jalur keluar yang sama untuk …
        self.defer_frames.push(Vec::new());
        let r = match bind_err {
            Some(e) => Err(e),
            None    => self.exec_fn_body(&func.body, &mut ce),
        };
        let r = self.run_defers(r);
        ce.pop_scope();
        // ── Profiler: catat keluar (selalu dipanggil, termasuk saat error) ─
        crate::profiler::exit();
        self.ruang_kini = ruang_lama;
        // Kedalaman dikurangi di SATU tempat ini, pada jalur keluar yang
        // sama untuk sukses maupun error — jadi hitungannya tidak pernah
        // bocor waktu sebuah panggilan gagal di tengah jalan.
        self.call_depth -= 1;
        // Bingkai jejak didorong di SINI — satu-satunya tempat badan sebuah fungsi
        // JRG benar-benar dijalankan — jadi builtin tidak pernah menyumbang bingkai …
        let r = match r {
            Err(Signal::RuntimeError(e)) => {
                let nama = if func.name.is_empty() { "<anonim>" } else { &func.name };
                Err(Signal::RuntimeError(e.tambah_bingkai(nama)))
            }
            lain => lain,
        };
        r
    }

    pub(crate) fn call_closure(
        &mut self,
        params: std::sync::Arc<Vec<String>>,
        body: std::sync::Arc<Expr>,
        captures: Vec<(String, std::sync::Arc<std::sync::Mutex<Value>>)>,
        globals: crate::interpreter::Globals,
        ruang: Option<std::sync::Arc<String>>,
        args: Vec<Value>,
    ) -> EvalResult {
        if params.len() != args.len() {
            // Diperiksa SEBELUM `enter`, jadi jalur gagal ini tidak pernah
            // meninggalkan frame yang tidak pernah ditutup.
            return runtime_err!("{}", match params.get(args.len()) {
                Some(p) => crate::interpreter::pesan_parameter_wajib("<closure>", p, args.len() + 1),
                None => crate::interpreter::pesan_fungsi_kebanyakan(
                    "<closure>", params.len(), args.len(), &params),
            });
        }
        // Closure ikut dihitung: rekursi tak berujung lewat closure dulu meluapkan stack
        // interpreter (proses mati), sementara `--vm` menolaknya dengan kalimat ini.
        self.call_depth += 1;
        if self.call_depth > Self::MAX_CALL_DEPTH {
            self.call_depth -= 1;
            return runtime_err!("{}", crate::interpreter::pesan_rekursi_dalam(
                "<closure>", Self::MAX_CALL_DEPTH));
        }
        // Closure juga diprofilkan, dengan nama yang SAMA dengan yang dipakai VM
        // (`<closure>`).
        crate::profiler::enter("<closure>");
        let ruang_lama = std::mem::replace(&mut self.ruang_kini, ruang);
        // Env pemanggilan dibangun dari nol: globals milik closure (bukan milik
        // pemanggil — aturan lexical yang sama dengan `call_jrg`), lalu satu scope …
        let mut cap = Env::with_globals(globals);
        cap.push_scope();
        for (name, cell) in captures { cap.define_shared(&name, cell); }
        // Parameter di scope TERPISAH di atas tangkapan, supaya parameter
        // yang namanya kebetulan sama dengan variabel tangkapan tetap
        // membayangi tangkapannya.
        cap.push_scope();
        for (name, val) in params.iter().zip(args) { cap.define(name, val); }
        // `defer` di dalam badan closure ikut aturan yang sama dengan
        // fungsi biasa: dijalankan waktu closure-nya selesai.
        self.defer_frames.push(Vec::new());
        let r = self.eval_expr(&body, &mut cap);
        let r = self.run_defers(r);
        cap.pop_scope();
        cap.pop_scope();
        // Dipasangkan dengan `enter` di atas, di jalur keluar yang SAMA untuk sukses
        // maupun error — persis seperti `call_jrg`.
        crate::profiler::exit();
        self.ruang_kini = ruang_lama;
        self.call_depth -= 1;
        // `return` di dalam closure berhenti di SINI — closure itu sendiri yang
        // selesai, bukan fungsi yang membungkusnya.
        match r {
            Err(Signal::Return(v)) => Ok(v),
            // Nama bingkainya sama dengan yang dipakai VM dan profiler (`<closure>`) —
            // closure tidak punya nama, dan mengarang satu dari variabel yang kebetulan …
            Err(Signal::RuntimeError(e)) => Err(Signal::RuntimeError(e.tambah_bingkai("<closure>"))),
            lain => lain,
        }
    }


    // -----------------------------------------------------------------------
    // Native Function Implementations
    // -----------------------------------------------------------------------


}
