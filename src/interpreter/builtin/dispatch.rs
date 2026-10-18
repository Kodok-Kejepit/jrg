// src/interpreter/builtin/dispatch.rs — dispatcher pusat fungsi native

use std::collections::HashMap;

use crate::interpreter::{
    Value, Env, Signal, RuntimeError, EvalResult, Interpreter,
    ok_value, err_value, some_value, none_value,
};
use crate::runtime_err;
use super::*;

impl Interpreter {
    /// `str::to_int`, aliasnya `parse_int`, dan `.to_int()` — satu pintu, jadi
    /// pesannya menyebut nama kanoniknya seperti alias lain (`sin` → `math::sin`).
    pub fn parse_int_lepas(v: &Value) -> EvalResult {
        crate::interpreter::konversi::parse_bulat("str::to_int", v)
            .map(Value::Int).map_err(|e| Signal::RuntimeError(RuntimeError::new(e)))
    }

    /// `str::to_float`, aliasnya `parse_float`, dan `.to_float()`.
    pub fn parse_float_lepas(v: &Value) -> EvalResult {
        crate::interpreter::konversi::parse_float("str::to_float", v)
            .map(Value::Float).map_err(|e| Signal::RuntimeError(RuntimeError::new(e)))
    }

    /// `int(x)` dan saudaranya adalah `x as T` yang ditulis sebagai panggilan.
    fn ubah_lewat_cast(&self, args: &[Value], pintu: &str, sasaran: crate::parser::PrimTy) -> EvalResult {
        match args.first() {
            None => runtime_err!("{}", crate::interpreter::pesan_butuh_nilai(pintu)),
            Some(v) => self.eval_cast(v.clone(), &crate::parser::TypeExpr::Primitive(sasaran)),
        }
    }

    pub fn call_native(&mut self, kind: NativeFnKind, args: Vec<Value>) -> EvalResult {
        // Transformer array dilayani DULU, selagi argumennya masih dimiliki.
        let args = match self.arr_konsumsi(&kind, args) {
            Ok(r) => return r,
            Err(kembali) => kembali,
        };

        let args = match Self::dict_konsumsi(&kind, args) {
            Ok(r) => return r,
            Err(kembali) => kembali,
        };

        if let Some(r) = self.dispatch_math(&kind, &args)     { return r; }
        if let Some(r) = self.dispatch_arr(&kind, &args)      { return r; }
        if let Some(r) = self.dispatch_str(&kind, &args)      { return r; }
        if let Some(r) = self.dispatch_dok(&kind, &args)      { return r; }
        if let Some(r) = heap::dispatch(&kind, &args)         { return r; }
        if let Some(r) = self.dispatch_dict(&kind, &args)     { return r; }
        if let Some(r) = self.dispatch_rand(&kind, &args)     { return r; }
        if let Some(r) = self.dispatch_time_ext(&kind, &args) { return r; }
        if let Some(r) = self.dispatch_file(&kind, &args)     { return r; }
        if let Some(r) = self.dispatch_sysenv(&kind, &args)   { return r; }
        {
            // Satu-satunya jalan worker FFI menjalankan kode JRG (callback).
            let mut panggil = |f: &Value, a: Vec<Value>| -> Result<Value, String> {
                match self.call_fn(f.clone(), a) {
                    Ok(v) | Err(Signal::Return(v)) => Ok(v),
                    Err(e) => Err(ffi_pintu::pesan_sinyal(e)),
                }
            };
            if let Some(r) = ffi_pintu::pintu_ffi(&kind, &args, &mut panggil) { return r; }
        }

        use NativeFnKind::*;

        // Diteruskan ke stdlib VM lewat jembatan nilai. Satu arm untuk
        // semuanya — lihat catatan di `jembatan_stdlib.rs` soal kenapa
        // yang diseberangkan nilainya, bukan kodenya.
        if let DariStdlibVm(nama) = kind {
            // Membaca SATU elemen tidak menyeberangkan seluruh array (jembatan menyalin tiap
            // elemennya): bentuk yang sah dijawab di sini, sisanya — dan tiap galat — oleh VM.
            match (nama, args.as_slice()) {
                ("arr::get", [Value::Array(a), Value::Int(i)]) if *i >= 0 => {
                    return Ok(match usize::try_from(*i).ok().and_then(|i| a.get(i)) {
                        Some(v) => crate::interpreter::some_value(v.clone()),
                        None => crate::interpreter::none_value(),
                    });
                }
                ("arr::get_or", [Value::Array(a), Value::Int(i), cadangan]) if *i >= 0 => {
                    return Ok(usize::try_from(*i).ok().and_then(|i| a.get(i)).cloned()
                        .unwrap_or_else(|| cadangan.clone()));
                }
                _ => {}
            }
            return jembatan_stdlib::panggil(nama, &args);
        }
        // `chan::m(c, ...)` IS `c.m(...)`: satu implementasi untuk dua bentuk.
        if let ChanNs(m) = kind {
            let mut it = args.into_iter();
            return match it.next() {
                Some(c @ Value::Channel(_)) => self.builtin_method(c, m, it.collect()),
                lain => runtime_err!("{}", crate::interpreter::pesan_arg_harus_channel(
                    &format!("chan::{}", m),
                    &lain.map(|v| v.type_name()).unwrap_or_else(|| "nil".into()))),
            };
        }

        let fst = |args: &Vec<Value>| -> Value { args.first().cloned().unwrap_or(Value::Nil) };

        match kind {
            // ---------------------------------------------------------------
            // IO
            // ---------------------------------------------------------------
            FmtWok => {
                if args.is_empty() { crate::host::tulis_baris(""); }
                else {
                    // `tampil_gabung`, bukan `format!("{}")` — di situlah hook
                    // `to_string` milik pengguna dipanggil. Lihat
                    // `interpreter/tampil.rs`.
                    let s = self.tampil_gabung(&args)?;
                    crate::host::tulis_baris(&s);
                }
                crate::host::siram();
                Ok(Value::Unit)
            }
            FmtWokNoNl => {
                let s = self.tampil_gabung(&args)?;
                crate::host::tulis(&s);
                crate::host::siram();
                Ok(Value::Unit)
            }
            FmtTok => {
                if !args.is_empty() {
                    let s = self.tampil_gabung(&args)?;
                    crate::host::tulis(&s);
                    crate::host::siram();
                }
                let mut line = String::new();
                match crate::host::baca_baris(&mut line) {
                    Ok(_) => {
                        if line.ends_with('\n') { line.pop(); }
                        if line.ends_with('\r') { line.pop(); }
                        Ok(Value::Str(line))
                    }
                    Err(e) => runtime_err!("tok: {}", e),
                }
            }
            FmtEprint => {
                let s = self.tampil_gabung(&args)?;
                crate::host::galat_baris(&s);
                Ok(Value::Unit)
            }
            FmtSprint => {
                let s = self.tampil_gabung(&args)?;
                Ok(Value::Str(s))
            }

            // ---------------------------------------------------------------
            // Type conversion
            // ---------------------------------------------------------------
            ToInt   => self.ubah_lewat_cast(&args, "int()", crate::parser::PrimTy::Int),
            ToFloat => self.ubah_lewat_cast(&args, "float()", crate::parser::PrimTy::F64),
            ToBool  => {
                if args.is_empty() {
                    return runtime_err!("{}", crate::interpreter::pesan_butuh_nilai("bool()"));
                }
                Ok(Value::Bool(self.truthy(&fst(&args))))
            }
            // Tanpa argumen DITOLAK, seperti `int()` — dulu `str()` menjawab
            // teks "nil" di sini dan "" di `--vm`.
            ToStr if args.is_empty() => runtime_err!("{}", crate::interpreter::pesan_butuh_nilai("str()")),
            ToStr   => Ok(Value::Str(format!("{}", fst(&args)))),
            ToList if args.is_empty() => runtime_err!("{}", crate::interpreter::pesan_butuh_nilai("list()")),
            ToList  => match fst(&args) {
                Value::Array(a)  => Ok(Value::Array(a)),
                Value::Str(s)    => Ok(Value::Array(s.chars().map(Value::Rune).collect())),
                Value::Tuple(t)  => Ok(Value::Array(t)),
                other => runtime_err!("list(): tidak bisa dari `{}`", other.type_name()),
            },

            // ---------------------------------------------------------------
            // Built-ins
            // ---------------------------------------------------------------

            // Tanpa argumen dulu terbaca `len(nil)` — menuduh `nil` yang tak ditulis.
            Len if args.is_empty() => runtime_err!("{}", crate::interpreter::pesan_butuh_nilai("len()")),
            Len => match fst(&args) {
                Value::Str(s)   => Ok(Value::Int(s.chars().count() as i128)),
                // Sama dengan `c.len()`: yang sudah terkirim dan belum diambil.
                Value::Channel(c) => Ok(Value::Int(c.panjang() as i128)),
                Value::Array(a) => Ok(Value::Int(a.len() as i128)),
                Value::Tuple(t) => Ok(Value::Int(t.len() as i128)),
                // Dict itu koleksi berukuran, sama seperti array dan string, dan `d.len()`
                // maupun `dict::len(d)` sudah menjawabnya.
                Value::Struct { name, fields } if name == "dict" =>
                    Ok(Value::Int(fields.len() as i128)),
                other => runtime_err!("{}", crate::interpreter::pesan_len_tak_bisa(
                    &other.type_name())),
            },
            ParseInt   => Self::parse_int_lepas(&fst(&args)),
            ParseFloat => Self::parse_float_lepas(&fst(&args)),
            Typeof if args.is_empty() => runtime_err!("{}", crate::interpreter::pesan_butuh_nilai("typeof()")),
            Typeof => Ok(Value::Str(crate::ruang_tipe::bersihkan(fst(&args).type_name()))),

            // `terap(f, args)` — mesin di balik gula `f(...v)`.
            Terap => {
                let f = match args.first() {
                    Some(v) if v.bisa_dipanggil() => v.clone(),
                    Some(lain) => return Err(Signal::RuntimeError(RuntimeError::new(crate::interpreter::pesan_bukan_fungsi(&lain.type_name())))),
                    None => return Err(Signal::RuntimeError(RuntimeError::new(
                        "terap dipanggil tanpa argumen — ia butuh fungsinya dan array \
                         argumennya: `terap(f, [1, 2])`".to_string()))),
                };
                let daftar = match args.get(1) {
                    Some(Value::Array(a)) => a.to_vec(),
                    Some(lain) => return Err(Signal::RuntimeError(RuntimeError::new(format!(
                        "terap: argumen 2 harus array, dapat `{}`", lain.type_name())))),
                    None => Vec::new(),
                };
                self.call_fn(f, daftar)
            }

            // `terap_bernama(f, posisional, nama, nilai)` — mesin di balik gula `f(a, b:
            // 2)`.
            TerapBernama => {
                let f = match args.first() {
                    Some(v @ (Value::Function(_) | Value::Closure { .. })) => v.clone(),
                    Some(v) if v.bisa_dipanggil() => return Err(Signal::RuntimeError(
                        RuntimeError::new(crate::interpreter::pesan_bernama_tanpa_param(&v.type_name())))),
                    Some(lain) => return Err(Signal::RuntimeError(RuntimeError::new(crate::interpreter::pesan_bukan_fungsi(&lain.type_name())))),
                    None => return Err(Signal::RuntimeError(RuntimeError::new(
                        "terap_bernama dipanggil tanpa argumen".to_string()))),
                };
                let galat = |m: String| Signal::RuntimeError(RuntimeError::new(
                    crate::interpreter::pesan_terap_bernama(&m)));
                let ambil_arr = |i: usize| -> Result<Vec<Value>, Signal> {
                    match args.get(i) {
                        Some(Value::Array(a)) => Ok(a.to_vec()),
                        None => Ok(Vec::new()),
                        Some(lain) => Err(galat(format!("argumen ke-{} harus array, dapat `{}`",
                                                        i + 1, lain.type_name()))),
                    }
                };
                let (posisional, nilai) = (ambil_arr(1)?, ambil_arr(3)?);
                let nama: Vec<String> = ambil_arr(2)?.into_iter()
                    .map(|v| match v {
                        Value::Str(s) => Ok(s.to_string()),
                        lain => Err(galat(format!("nama parameternya harus string, dapat `{}`", lain.type_name()))),
                    })
                    .collect::<Result<_, _>>()?;
                if nama.len() != nilai.len() {
                    return Err(galat(format!("{} nama tapi {} nilai", nama.len(), nilai.len())));
                }
                let param = match &f {
                    Value::Function(jf) => (*jf.params).clone(),
                    Value::Closure { params, .. } => (**params).clone(),
                    _ => Vec::new(),
                };
                let (posisi, panjang) =
                    crate::interpreter::susun_argumen_bernama(
                        &param, posisional.len(), &nama)
                    .map_err(|e| Signal::RuntimeError(RuntimeError::new(e)))?;
                let mut akhir: Vec<Value> = vec![Value::Nil; panjang];
                for (i, v) in posisional.into_iter().enumerate() { akhir[i] = v; }
                for (k, idx) in posisi.into_iter().enumerate() {
                    akhir[idx] = nilai.get(k).cloned().unwrap_or(Value::Nil);
                }
                self.call_fn(f, akhir)
            }
            // Satu-satunya cara kode JRG melempar galat sendiri.
            Panik => {
                let pesan = match args.first() {
                    None => "panik() tanpa alasan — sebutkan apa yang salah".to_string(),
                    Some(v) => format!("{}", v),
                };
                runtime_err!("{}", pesan)
            }
            Assert => {
                if args.is_empty() {
                    return runtime_err!("{}", crate::interpreter::pesan_assert_kurang(
                        "assert", 1, 0));
                }
                if !self.truthy(&fst(&args)) {
                    let msg = args.get(1).map(|v| format!("{}", v))
                        .unwrap_or_else(|| "assertion failed".to_string());
                    return runtime_err!("{}", crate::interpreter::pesan_assert(
                        args.get(1).map(|_| msg.as_str())));
                }
                Ok(Value::Unit)
            }
            AssertEq => {
                if args.len() < 2 {
                    return runtime_err!("{}", crate::interpreter::pesan_assert_kurang(
                        "assert_eq", 2, args.len()));
                }
                let a = fst(&args);
                let b = args.get(1).cloned().unwrap_or(Value::Nil);
                if !self.val_eq(&a, &b)? {
                    let tambahan = args.get(2).map(|v| format!("{}", v));
                    return runtime_err!("{}", crate::interpreter::pesan_assert_eq(
                        &a.to_string(), &a.type_name(),
                        &b.to_string(), &b.type_name(),
                        tambahan.as_deref()));
                }
                Ok(Value::Unit)
            }
            NumCpus => Ok(Value::Int(
                std::thread::available_parallelism()
                    .map(|count| count.get() as i128)
                    .unwrap_or(1),
            )),
            // ── Empat nama yang dulu cuma hidup di `--vm` ───────────────
            ToUint => self.ubah_lewat_cast(&args, "uint()", crate::parser::PrimTy::Uint),
            ToRune => self.ubah_lewat_cast(&args, "rune()", crate::parser::PrimTy::Rune),
            // Satu definisi dengan `range` dan literal `a..=b` — `interpreter::range_jumlah`.
            RangeInclusive => {
                if args.len() != 2 {
                    return runtime_err!("`range_inclusive` butuh 2 argumen (awal, akhir), dapat {}", args.len());
                }
                let g = |m: String| Signal::RuntimeError(RuntimeError::new(m));
                let (s, e) = crate::interpreter::range_batas("range_inclusive",
                    &crate::interpreter::arg_angka(&args[0]), &crate::interpreter::arg_angka(&args[1])).map_err(g)?;
                let n = crate::interpreter::range_jumlah("range_inclusive", s, e, 1, true).map_err(g)?;
                Ok(Value::Array(crate::interpreter::range_isi(s, 1, n).map(Value::Int).collect::<Vec<_>>().into()))
            },
            Range => {
                let g = |m: String| Signal::RuntimeError(RuntimeError::new(m));
                let aa: Vec<_> = args.iter().map(crate::interpreter::arg_angka).collect();
                let (s, e, st) = crate::interpreter::range_argumen(&aa).map_err(g)?;
                let n = crate::interpreter::range_jumlah("range", s, e, st, false).map_err(g)?;
                Ok(Value::Array(crate::interpreter::range_isi(s, st, n).map(Value::Int).collect::<Vec<_>>().into()))
            }
            Exit => {
                let kode = match args.first() {
                    None => 0,
                    Some(Value::Int(n)) => match crate::host::kode_keluar(*n) {
                        Ok(k) => k,
                        Err(e) => return runtime_err!("{}", e),
                    },
                    Some(l) => return runtime_err!("{}", crate::host::pesan_kode_keluar(&l.type_name())),
                };
                crate::host::keluar(kode);
            }


            CanvasNew => match crate::canvas::canvas_baru(&args) {
                Ok(h) => Ok(Value::Canvas(h)),
                Err(e) => runtime_err!("{}", e),
            },

            // --------------------------------------------------------------- time::
            // --------------------------------------------------------------- …

            // `time::now_ms()` returns milliseconds since the Unix epoch — coarse,
            // monotonic-enough wall-clock timing for measuring elapsed time between two …
            TimeNowMs => {
                let ms = crate::host::sejak_epoch()
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                Ok(Value::Int(ms as i128))
            }

            // `Future::`/`Task::` — thread OS nyata, sama seperti goroutine
            // dan sama seperti implementasi VM-nya.
            FutureNew  => self.mulai_tugas("Future", args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            TaskSpawn  => self.mulai_tugas("Task", args).map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            FutureAwait => self.tunggu_tugas(&args, "future::await").map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            TaskJoin    => self.tunggu_tugas(&args, "task::join").map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            FutureIsDone => self.tugas_selesai(&args, "future::is_done").map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            FutureAll     => self.tugas_semua(&args, "future::all").map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            FutureRace    => self.tugas_balapan(&args, "future::race").map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            FutureTimeout => self.tugas_batas_waktu(&args, "future::timeout").map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),
            FutureTryGet  => self.tugas_coba_ambil(&args, "future::try_get").map_err(|e| Signal::RuntimeError(RuntimeError::new(e))),

            // ── dok:: ─────────────────────────────────────────────────── Semua fungsi
            // murni (tak butuh callback ke JRG) di-delegasikan langsung, error …
            _ => unreachable!(
                "NativeFnKind {:?} seharusnya sudah ditangani salah satu \
                 dispatcher namespace (math/arr/str/dok/heap) di atas — \
                 kalau ini kena, ada varian baru yang belum dipetakan \
                 ke salah satu dispatcher.", kind
            ),
        }
    }
}
