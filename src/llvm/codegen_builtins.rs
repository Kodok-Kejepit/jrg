// src/llvm/codegen_builtins.rs — jrgc LLVM backend — src/llvm/codegen_builtins.rs
// The builtin function dispatch table: wok/eprint/sprint/tok, len/typeof/

#![allow(dead_code)]

use crate::parser::*;
use crate::lexer::Span;

use super::codegen::{Codegen, Val};
use super::codegen_expr::fmt_hexfloat;
use super::types::*;

impl Codegen {
    /// Returns `None` when `name` isn't a recognized builtin at all (so the
    /// caller falls through to ordinary user-function lookup); `Some(..)`
    /// once it's committed to handling it (success or a specific error).
    pub fn try_builtin_call(&mut self, name: &str, args: &[Expr], expected: Option<&JrgType>, span: Span) -> Option<Result<Val, String>> {
        // `arr::…` punya berkasnya sendiri: tiap fungsinya mengembalikan
        // koleksi BARU, jadi semuanya berbagi perkakas penyalinan dan
        // penelusuran yang sama.
        if let Some(r) = self.coba_builtin_arr(name, args, span) { return Some(r); }
        if let Some(r) = self.coba_builtin_dict(name, args, expected, span) { return Some(r); }
        if let Some(r) = self.coba_builtin_jawa(name, args, span) { return Some(r); }

        Some(match name {
            "wok" => self.builtin_print(args, false, true),
            "wok_" | "fmt::wok_" => self.builtin_print(args, false, false),
            "fmt::wok" => self.builtin_print(args, false, true),
            "eprint" | "fmt::eprint" => self.builtin_print(args, true, true),
            "sprint" | "fmt::sprint" => self.builtin_sprint(args),
            "tok" | "fmt::tok" => self.builtin_tok(args),
            "len" => self.builtin_len(args, span),
            "num_cpus" if args.is_empty() => {
                self.ensure_extern("declare i64 @jrg_num_cpus()", "jrg_num_cpus");
                let r = self.fresh("cpu.");
                self.emit(format!("{} = call i64 @jrg_num_cpus()", r));
                Ok(Val { text: r, ty: JrgType::I64 })
            }
            "range" => self.builtin_range(args, span),
            // `range_inclusive(a, b)` adalah `range(a, b + 1)`; luapan di `b + 1` diperiksa seperti `+`.
            "range_inclusive" if args.len() == 2 => {
                let satu = Expr::Literal(crate::parser::LitKind::Int(1), span);
                let ujung = Expr::Binary { op: BinaryOp::Add, lhs: Box::new(args[1].clone()), rhs: Box::new(satu), span };
                self.builtin_range(&[args[0].clone(), ujung], span)
            }
            "enumerate" | "zip" if self.kunci_lapis("arr", name).is_some() => {
                let kunci = self.kunci_lapis("arr", name).unwrap_or_default();
                self.call_named_function(&kunci, args, expected, span)
            }
            // `list(x)`: karakter sebuah teks, atau arraynya sendiri.
            "list" if args.len() == 1 => (|| {
                let v = self.codegen_expr(&args[0], None)?;
                match &v.ty {
                    JrgType::Str => {
                        let slot = self.spill_to_place(&v);
                        let isi = self.lokal_sementara(slot, JrgType::Str, span);
                        self.codegen_path_call(&["str".to_string(), "chars".to_string()], &[isi], expected, span)
                    }
                    JrgType::Array(..) | JrgType::Slice(_) => Ok(v),
                    lain => Err(super::galat::tandai(span, format!(
                        "`list` butuh teks atau array, dapat `{}`", lain.nama_typeof()))),
                }
            })(),
            "typeof" => self.builtin_typeof(args),
            "assert" => self.builtin_assert(args, span),
            "panik" => self.builtin_panik(args, span),
            "assert_eq" => self.builtin_assert_eq(args, span),
            "exit" | "std::exit" => self.builtin_exit(args),
            // `int(x)` ADALAH `x as int` di acuan, jadi lewat pintu yang sama.
            "int" => self.builtin_konversi(args, crate::parser::PrimTy::Int, span),
            "uint" => self.builtin_konversi(args, crate::parser::PrimTy::Uint, span),
            "float" => self.builtin_konversi(args, crate::parser::PrimTy::F64, span),
            "rune" => self.builtin_konversi(args, crate::parser::PrimTy::Rune, span),
            "bool" => self.builtin_conv(args, JrgType::Bool, span),
            // `str::to_string(v)` memang mengubah nilai apa pun jadi teks — sama dengan `str(v)`.
            "str" | "str::to_string" => self.builtin_conv(args, JrgType::Str, span),
            "parse_int" | "str::to_int" => self.builtin_parse(args, true, span),
            "parse_float" | "str::to_float" => self.builtin_parse(args, false, span),
            "abs" | "math::abs" => self.builtin_abs(args, span),
            "sqrt" | "math::sqrt" => self.builtin_libm1(args, "sqrt", span),
            "floor" | "math::floor" => self.builtin_libm1(args, "floor", span),
            "ceil" | "math::ceil" => self.builtin_libm1(args, "ceil", span),
            "round" | "math::round" => self.builtin_libm1(args, "round", span),
            "sin" | "math::sin" => self.builtin_libm1(args, "sin", span),
            "cos" | "math::cos" => self.builtin_libm1(args, "cos", span),
            "tan" | "math::tan" => self.builtin_libm1(args, "tan", span),
            "math::asin" => self.builtin_libm1(args, "asin", span),
            "math::acos" => self.builtin_libm1(args, "acos", span),
            "math::atan" => self.builtin_libm1(args, "atan", span),
            "math::atan2" => self.builtin_libm2(args, "atan2", span),
            "math::hypot" => self.builtin_libm2(args, "hypot", span),
            "dict::is_dict" if args.len() == 1 => self.builtin_is_dict(&args[0]),
            "Chan::new" | "chan::make" if args.len() <= 1 => self.chan_bertag(args),
            "Atomic::new" | "Mutex::new" | "atomic::get" | "atomic::set" | "atomic::add" | "atomic::sub" | "atomic::cas"
            | "mutex::lock" | "mutex::unlock" | "mutex::with_lock" => self.panggil_sinkron(name, args, span),
            "Future::new" => self.masa_baru(args, false, span),
            "Task::spawn" => self.masa_baru(args, true, span),
            _ if (name.starts_with("future::") || name.starts_with("task::")) && self.nama_masa(name, args.len()) =>
                return self.coba_builtin_masa(name, args, span),
            _ if name.starts_with("chan::") && self.coba_nama_chan(&name[6..], args.len()) =>
                return self.coba_builtin_chan(&name[6..], args, span),
            "json::parse" if args.len() == 1 => self.builtin_json_parse(&args[0], span),
            "json::stringify" if args.len() == 1 => self.builtin_json_tulis(&args[0], None, span),
            "json::pretty" if args.len() == 1 || args.len() == 2 => self.builtin_json_tulis(&args[0], Some(args.get(1)), span),
            // Pintu sistem operasi (`jrg_os` di primitif.c) — cuma untuk `lapis.jrg`.
            "lapis::os" if args.len() == 4 && self.unit_lapis == Some(self.unit_kini) => self.builtin_lapis_os(args),
            // Sumber acak mentah (63 bit, tak negatif) — cuma untuk `lapis.jrg`.
            "lapis::acak" if args.is_empty() && self.unit_lapis == Some(self.unit_kini) => {
                self.ensure_extern("declare i64 @jrg_acak63()", "jrg_acak63");
                let r = self.fresh("acak.");
                self.emit(format!("{} = call i64 @jrg_acak63()", r));
                Ok(Val { text: r, ty: JrgType::I64 })
            }
            "math::copysign" => self.builtin_libm2(args, "copysign", span),
            "math::trunc" => self.builtin_libm1(args, "trunc", span),
            "ln" | "math::ln" => self.builtin_libm1(args, "log", span),
            "log10" | "math::log10" => self.builtin_libm1(args, "log10", span),
            "log2" | "math::log2" => self.builtin_libm1(args, "log2", span),
            "exp" | "math::exp" => self.builtin_libm1(args, "exp", span),
            "pow" | "math::pow" => self.builtin_math_pow(args, span),
            "math::min" => self.builtin_minmax(args, true, span),
            "math::max" => self.builtin_minmax(args, false, span),
            "clamp" | "math::clamp" => self.builtin_clamp(args, span),
            "math::is_nan" => self.builtin_is_nan(args, span),
            "math::is_inf" => self.builtin_is_inf(args, span),
            "math::sign" => self.builtin_sign(args, span),
            "math::factorial" => self.builtin_runtime_i64_1(args, "jrg_fact_i64", span),
            "math::gcd" => self.builtin_runtime_i64_2(args, "jrg_gcd_i64", span),
            "math::wrap_add" => self.builtin_bungkus(args, "add", name, span),
            "math::wrap_sub" => self.builtin_bungkus(args, "sub", name, span),
            "math::wrap_mul" => self.builtin_bungkus(args, "mul", name, span),
            "math::lcm" => self.builtin_runtime_i64_2(args, "jrg_lcm_i64", span),
            "math::PI" => Ok(Val { text: fmt_hexfloat(std::f64::consts::PI), ty: JrgType::F64 }),
            "math::E" => Ok(Val { text: fmt_hexfloat(std::f64::consts::E), ty: JrgType::F64 }),
            "str::format" => self.builtin_str_format(args, span),
            "str::upper" => self.builtin_str1(args, "jrg_str_to_upper", span),
            "str::lower" => self.builtin_str1(args, "jrg_str_to_lower", span),
            "str::trim" => self.builtin_str1(args, "jrg_str_trim", span),
            "str::reverse" => self.builtin_str1(args, "jrg_str_reverse", span),
            "str::capitalize" => self.builtin_str1(args, "jrg_str_capitalize", span),
            "str::title" => self.builtin_str1(args, "jrg_str_title", span),
            "str::snake_case" => self.builtin_str1(args, "jrg_str_snake_case", span),
            "str::camel_case" => self.builtin_str1(args, "jrg_str_camel_case", span),
            "str::swap_case" => self.builtin_str1(args, "jrg_str_swap_case", span),
            "str::trim_start" => self.builtin_str1(args, "jrg_str_trim_start", span),
            "str::trim_end" => self.builtin_str1(args, "jrg_str_trim_end", span),
            "str::is_alpha" => self.builtin_str_adalah(args, 0, name, span),
            "str::is_alphanumeric" => self.builtin_str_adalah(args, 1, name, span),
            "str::is_lower" => self.builtin_str_adalah(args, 2, name, span),
            "str::is_upper" => self.builtin_str_adalah(args, 3, name, span),
            "str::is_numeric" => self.builtin_str_adalah(args, 4, name, span),
            "str::words" => self.builtin_str_pecah(args, "jrg_str_words", JrgType::Str, name, span),
            "str::bytes" => self.builtin_str_pecah(args, "jrg_str_bytes", JrgType::I64, name, span),
            "str::byte_len" => self.builtin_str_byte_len(args, span),
            "str::starts_with" => self.builtin_str_bool2(args, "jrg_str_starts_with", span),
            "str::ends_with" => self.builtin_str_bool2(args, "jrg_str_ends_with", span),
            "str::contains" => self.builtin_str_bool2(args, "jrg_str_contains", span),
            "str::repeat" => self.builtin_str_repeat(args, span),
            "str::replace" => self.builtin_str_replace(args, span),
            // FALLBACK, dan urutannya bagian dari kebenarannya: nama `str::` yang sudah
            // punya builtin khusus di atas harus menang.
            _ => return self.coba_builtin_str(name, args, span),
        })
    }

    // ── printing ─────────────────────────────────────────────────────────

    /// Pembungkus publik supaya lengan `Nil` bisa mendelegasikan ke string.
    pub fn emit_print_val_pub(&mut self, v: &Val, to_stderr: bool, span: Span) -> Result<(), String> {
        self.emit_print_val(v, to_stderr, span)
    }

    fn emit_print_val(&mut self, v: &Val, to_stderr: bool, span: Span) -> Result<(), String> {
        let se = if to_stderr { 1 } else { 0 };
        match &v.ty {
            JrgType::I8 | JrgType::I16 | JrgType::I32 | JrgType::I64 => {
                self.ensure_extern("declare void @jrg_print_i64(i64, i32)", "jrg_print_i64");
                let v64 = self.coerce_longgar(v, &JrgType::I64);
                self.emit(format!("call void @jrg_print_i64(i64 {}, i32 {})", v64.text, se));
            }
            JrgType::U8 | JrgType::U16 | JrgType::U32 | JrgType::U64 => {
                self.ensure_extern("declare void @jrg_print_u64(i64, i32)", "jrg_print_u64");
                let v64 = self.coerce_longgar(v, &JrgType::U64);
                self.emit(format!("call void @jrg_print_u64(i64 {}, i32 {})", v64.text, se));
            }
            JrgType::I128 => {
                self.ensure_extern("declare void @jrg_print_i128(i64, i64, i32)", "jrg_print_i128");
                let (lo, hi) = self.split_i128(v);
                self.emit(format!("call void @jrg_print_i128(i64 {}, i64 {}, i32 {})", lo, hi, se));
            }
            JrgType::U128 => {
                self.ensure_extern("declare void @jrg_print_u128(i64, i64, i32)", "jrg_print_u128");
                let (lo, hi) = self.split_i128(v);
                self.emit(format!("call void @jrg_print_u128(i64 {}, i64 {}, i32 {})", lo, hi, se));
            }
            JrgType::F32 => {
                self.ensure_extern("declare void @jrg_print_f32(float, i32)", "jrg_print_f32");
                self.emit(format!("call void @jrg_print_f32(float {}, i32 {})", v.text, se));
            }
            JrgType::F64 => {
                self.ensure_extern("declare void @jrg_print_f64(double, i32)", "jrg_print_f64");
                self.emit(format!("call void @jrg_print_f64(double {}, i32 {})", v.text, se));
            }
            // `rune` mencetak KARAKTERNYA, bukan angkanya.
            JrgType::Nil => {
                let t = self.intern_string("nil");
                self.emit_print_val_pub(&t, to_stderr, span)?;
            }
            JrgType::Unit => {
                let t = self.intern_string("()");
                self.emit_print_val_pub(&t, to_stderr, span)?;
            }
            JrgType::Rune => {
                self.ensure_extern("declare void @jrg_print_char(i32, i32)", "jrg_print_char");
                self.emit(format!("call void @jrg_print_char(i32 {}, i32 {})", v.text, se));
            }
            JrgType::Bool => {
                self.ensure_extern("declare void @jrg_print_bool(i32, i32)", "jrg_print_bool");
                let v32 = self.fresh("pb.");
                self.emit(format!("{} = zext i8 {} to i32", v32, v.text));
                self.emit(format!("call void @jrg_print_bool(i32 {}, i32 {})", v32, se));
            }
            JrgType::Str => {
                self.ensure_extern("declare void @jrg_print_str(ptr, i64, i32)", "jrg_print_str");
                let (p, l) = self.split_str(v);
                self.emit(format!("call void @jrg_print_str(ptr {}, i64 {}, i32 {})", p, l, se));
            }
            JrgType::Array(_, _) | JrgType::Slice(_) | JrgType::Tuple(_) | JrgType::Struct(_) | JrgType::Enum(_) | JrgType::Dict(_)
            | JrgType::Any | JrgType::DictCampur(_) | JrgType::Fn(..) | JrgType::Chan(_) | JrgType::Masa(..) | JrgType::Sinkron(_) => {
                let s = self.build_display_str(v)?;
                self.ensure_extern("declare void @jrg_print_str(ptr, i64, i32)", "jrg_print_str");
                let (p, l) = self.split_str(&s);
                self.emit(format!("call void @jrg_print_str(ptr {}, i64 {}, i32 {})", p, l, se));
            }
            other => return Err(format!("[{}:{}] `wok`/`eprint`: mencetak tipe `{}` belum didukung backend LLVM v1", span.lo.0, span.hi.0, other.display_name())),
        }
        Ok(())
    }

    /// Recursively render any value as its `%JrgStr` Display form, matching the
    /// tree-walking interpreter's `impl Display for Value` byte for byte: arrays …
    pub fn build_display_str(&mut self, v: &Val) -> Result<Val, String> {
        // Hook tampilan buatan pengguna didahulukan atas format bawaan.
        if self.punya_hook_tampil(&v.ty) {
            return self.panggil_hook_tampil(v);
        }
        match &v.ty {
            JrgType::Str => Ok(v.clone()),
            JrgType::Fn(..) => Ok(self.tampilan_fungsi(v)),
            JrgType::Sinkron(k) => Ok(self.intern_string(if *k { "<mutex>" } else { "<atomic>" })),
            JrgType::Masa(_, tugas) => Ok(self.intern_string(&format!(
                "{} {{_ch: <handle>, _serah: <handle>}}", if *tugas { "Task" } else { "Future" }))),
            JrgType::Chan(_) => {
                self.ensure_extern("declare i64 @jrg_chan_info(ptr, i32)", "jrg_chan_info");
                let kap = self.fresh("chk.");
                self.emit(format!("{} = call i64 @jrg_chan_info(ptr {}, i32 1)", kap, v.text));
                let angka = self.val_to_str(&Val { text: kap, ty: JrgType::I64 })?;
                let awal = self.intern_string("<chan cap=");
                let s = self.str_concat(&awal, &angka)?;
                let akhir = self.intern_string(">");
                self.str_concat(&s, &akhir)
            }
            JrgType::Bool => self.val_to_str(v),
            JrgType::Unit => Ok(self.intern_string("()")),
            JrgType::Array(elem, n) => {
                let place = self.spill_to_place(v);
                let mut result = self.intern_string("[");
                for i in 0..*n {
                    if i > 0 {
                        let comma = self.intern_string(", ");
                        result = self.str_concat(&result, &comma)?;
                    }
                    let ep = self.fresh("dae.");
                    self.emit(format!("{} = getelementptr {}, ptr {}, i64 0, i64 {}", ep, v.ty.llvm_ty(), place, i));
                    let ev = self.load_place(&ep, elem);
                    let es = self.build_display_str(&ev)?;
                    result = self.str_concat(&result, &es)?;
                }
                let close = self.intern_string("]");
                self.str_concat(&result, &close)
            }
            JrgType::Slice(elem) => self.build_slice_display(v, elem),
            JrgType::Dict(vt) => { let vt = (**vt).clone(); self.tampilan_dict(v, &vt) }
            JrgType::DictCampur(_) => {
                let d = super::codegen_any::samakan_dict(v);
                self.tampilan_dict(&d, &JrgType::Any)
            }
            // Kotak bertag: golongannya dibaca saat jalan, lalu dicetak lewat jalur yang
            // sama dengan nilai telanjang — jadi `1` di dalam kotak tercetak persis …
            JrgType::Any => self.tampilan_any(v, crate::lexer::Span { lo: crate::lexer::BytePos(0), hi: crate::lexer::BytePos(0), file_id: 0 }),
            JrgType::Tuple(ts) => {
                let mut result = self.intern_string("(");
                for (i, t) in ts.iter().enumerate() {
                    if i > 0 {
                        let comma = self.intern_string(", ");
                        result = self.str_concat(&result, &comma)?;
                    }
                    let fv = self.fresh("dtf.");
                    self.emit(format!("{} = extractvalue {} {}, {}", fv, v.ty.llvm_ty(), v.text, i));
                    let ev = Val { text: fv, ty: t.clone() };
                    let es = self.build_display_str(&ev)?;
                    result = self.str_concat(&result, &es)?;
                }
                let close = self.intern_string(")");
                self.str_concat(&result, &close)
            }
            JrgType::Struct(name) => {
                let layout = self.types.structs.get(name).cloned().ok_or_else(|| format!("struct `{}` tidak dikenal", name))?;
                let mut sorted: Vec<(usize, String, JrgType)> = layout.fields.iter().enumerate()
                    .map(|(i, (n, t))| (i, n.clone(), t.clone())).collect();
                sorted.sort_by(|a, b| a.1.cmp(&b.1));
                let place = self.spill_to_place(v);
                let mut result = self.intern_string(&format!("{} {{", super::generik::dasar(name)));
                for (pos, (idx, fname, fty)) in sorted.iter().enumerate() {
                    if pos > 0 {
                        let comma = self.intern_string(", ");
                        result = self.str_concat(&result, &comma)?;
                    }
                    let prefix = self.intern_string(&format!("{}: ", fname));
                    result = self.str_concat(&result, &prefix)?;
                    let fp = self.fresh("dsf.");
                    self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", fp, v.ty.llvm_ty(), place, idx));
                    let fv = self.load_place(&fp, fty);
                    let fs = self.build_display_str(&fv)?;
                    result = self.str_concat(&result, &fs)?;
                }
                let close = self.intern_string("}");
                self.str_concat(&result, &close)
            }
            JrgType::Enum(name) => self.build_enum_display(v, name),
            JrgType::Never => Ok(self.intern_string("<!>")),
            _ => self.val_to_str(v),
        }
    }

    fn build_slice_display(&mut self, v: &Val, elem: &JrgType) -> Result<Val, String> {
        let place = self.spill_to_place(v);
        let pf = self.fresh("dslp.");
        self.emit(format!("{} = getelementptr %JrgSlice, ptr {}, i32 0, i32 0", pf, place));
        let dataptr = self.fresh("dsld.");
        self.emit(format!("{} = load ptr, ptr {}", dataptr, pf));
        let lf = self.fresh("dsll.");
        self.emit(format!("{} = getelementptr %JrgSlice, ptr {}, i32 0, i32 1", lf, place));
        let lenv = self.fresh("dsllv.");
        self.emit(format!("{} = load i64, ptr {}", lenv, lf));

        let acc_slot = self.alloca_raw("dsacc.", "%JrgStr", Some(8));
        let open = self.intern_string("[");
        self.emit(format!("store %JrgStr {}, ptr {}", open.text, acc_slot));

        // Element 0, if any, with no leading separator.
        let has_any = self.fresh("dshas.");
        self.emit(format!("{} = icmp sgt i64 {}, 0", has_any, lenv));
        let first_lbl = self.fresh_label("dsfirst.");
        let skip_first_lbl = self.fresh_label("dsskipfirst.");
        self.terminate(format!("br i1 {}, label %{}, label %{}", has_any, first_lbl, skip_first_lbl));

        self.start_block(first_lbl);
        let e0p = self.fresh("dse0.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 0", e0p, elem.llvm_ty(), dataptr));
        let e0v = self.load_place(&e0p, elem);
        let e0s = self.build_display_str(&e0v)?;
        let acc0 = self.load_place(&acc_slot, &JrgType::Str);
        let acc0b = self.str_concat(&acc0, &e0s)?;
        self.emit(format!("store %JrgStr {}, ptr {}", acc0b.text, acc_slot));
        if !self.is_terminated() { self.terminate(format!("br label %{}", skip_first_lbl)); }

        self.start_block(skip_first_lbl);
        let idx_slot = self.alloca_raw("dsidx.", "i64", Some(8));
        self.emit(format!("store i64 1, ptr {}", idx_slot));

        let cond_lbl = self.fresh_label("dscond.");
        let body_lbl = self.fresh_label("dsbody.");
        let end_lbl = self.fresh_label("dsend.");
        self.terminate(format!("br label %{}", cond_lbl));

        self.start_block(cond_lbl.clone());
        let iv = self.load_place(&idx_slot, &JrgType::I64);
        let cmp = self.fresh("dscmp.");
        self.emit(format!("{} = icmp slt i64 {}, {}", cmp, iv.text, lenv));
        self.terminate(format!("br i1 {}, label %{}, label %{}", cmp, body_lbl, end_lbl));

        self.start_block(body_lbl);
        let iv2 = self.load_place(&idx_slot, &JrgType::I64);
        let ep = self.fresh("dse.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", ep, elem.llvm_ty(), dataptr, iv2.text));
        let ev = self.load_place(&ep, elem);
        let es = self.build_display_str(&ev)?;
        let accv = self.load_place(&acc_slot, &JrgType::Str);
        let comma = self.intern_string(", ");
        let accsep = self.str_concat(&accv, &comma)?;
        let accfinal = self.str_concat(&accsep, &es)?;
        self.emit(format!("store %JrgStr {}, ptr {}", accfinal.text, acc_slot));
        let iv3 = self.load_place(&idx_slot, &JrgType::I64);
        let nextv = self.fresh("dsn.");
        self.emit(format!("{} = add i64 {}, 1", nextv, iv3.text));
        self.emit(format!("store i64 {}, ptr {}", nextv, idx_slot));
        if !self.is_terminated() { self.terminate(format!("br label %{}", cond_lbl)); }

        self.start_block(end_lbl);
        let accfin = self.load_place(&acc_slot, &JrgType::Str);
        let close = self.intern_string("]");
        self.str_concat(&accfin, &close)
    }

    fn build_enum_display(&mut self, v: &Val, ename: &str) -> Result<Val, String> {
        let layout = self.types.enums.get(ename).cloned().ok_or_else(|| format!("enum `{}` tidak dikenal", ename))?;
        let place = self.spill_to_place(v);
        let tagp = self.fresh("edtp.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 0", tagp, v.ty.llvm_ty(), place));
        let tagv = self.fresh("edtv.");
        self.emit(format!("{} = load i32, ptr {}", tagv, tagp));

        let result_slot = self.alloca_raw("eds.", "%JrgStr", Some(8));

        let merge_lbl = self.fresh_label("edmerge.");
        let n = layout.variants.len();
        for (i, variant) in layout.variants.iter().enumerate() {
            let next_lbl = self.fresh_label("ednext.");
            let cmp = self.fresh("edc.");
            self.emit(format!("{} = icmp eq i32 {}, {}", cmp, tagv, variant.tag));
            let body_lbl = self.fresh_label("edbody.");
            self.terminate(format!("br i1 {}, label %{}, label %{}", cmp, body_lbl, next_lbl));

            self.start_block(body_lbl);
            // Nama yang DICETAK adalah nama sumbernya, bukan nama termangel.
            let terlihat = ename.split('$').next().unwrap_or(ename);
            let mut s = self.intern_string(&format!("{}::{}", terlihat, variant.name));
            match &variant.payload {
                EnumPayload::Unit => {}
                EnumPayload::Tuple(fields) if fields.len() == 1 => {
                    let payload_struct = variant.payload.as_llvm_struct();
                    let blob = self.fresh("edb.");
                    self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", blob, v.ty.llvm_ty(), place));
                    let fp = self.fresh("edf.");
                    self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 0", fp, payload_struct, blob));
                    let fv = self.load_place(&fp, &fields[0]);
                    let fs = self.build_display_str(&fv)?;
                    let open = self.intern_string("(");
                    let close = self.intern_string(")");
                    s = self.str_concat(&s, &open)?;
                    s = self.str_concat(&s, &fs)?;
                    s = self.str_concat(&s, &close)?;
                }
                EnumPayload::Tuple(fields) => {
                    let payload_struct = variant.payload.as_llvm_struct();
                    let blob = self.fresh("edb.");
                    self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", blob, v.ty.llvm_ty(), place));
                    let mut inner = self.intern_string("(");
                    for (fi, fty) in fields.iter().enumerate() {
                        if fi > 0 { let c = self.intern_string(", "); inner = self.str_concat(&inner, &c)?; }
                        let fp = self.fresh("edfm.");
                        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", fp, payload_struct, blob, fi));
                        let fv = self.load_place(&fp, fty);
                        let fs = self.build_display_str(&fv)?;
                        inner = self.str_concat(&inner, &fs)?;
                    }
                    let iclose = self.intern_string(")");
                    inner = self.str_concat(&inner, &iclose)?;
                    let open = self.intern_string("(");
                    let close = self.intern_string(")");
                    s = self.str_concat(&s, &open)?;
                    s = self.str_concat(&s, &inner)?;
                    s = self.str_concat(&s, &close)?;
                }
                EnumPayload::Struct(fields) => {
                    let payload_struct = variant.payload.as_llvm_struct();
                    let blob = self.fresh("edbs.");
                    self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", blob, v.ty.llvm_ty(), place));
                    // Tampilannya sama dengan dua mesin lain: `E::V(V {a: 1, b: 2})`.
                    let open = self.intern_string(&format!("({} {{", variant.name));
                    s = self.str_concat(&s, &open)?;
                    let mut sorted: Vec<(usize, String, JrgType)> = fields.iter().enumerate()
                        .map(|(i, (n, t))| (i, n.clone(), t.clone())).collect();
                    sorted.sort_by(|a, b| a.1.cmp(&b.1));
                    for (pos, (idx, fname, fty)) in sorted.iter().enumerate() {
                        if pos > 0 { let c = self.intern_string(", "); s = self.str_concat(&s, &c)?; }
                        let prefix = self.intern_string(&format!("{}: ", fname));
                        s = self.str_concat(&s, &prefix)?;
                        let fp = self.fresh("edfs.");
                        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", fp, payload_struct, blob, idx));
                        let fv = self.load_place(&fp, fty);
                        let fs = self.build_display_str(&fv)?;
                        s = self.str_concat(&s, &fs)?;
                    }
                    let close = self.intern_string("})");
                    s = self.str_concat(&s, &close)?;
                }
            }
            self.emit(format!("store %JrgStr {}, ptr {}", s.text, result_slot));
            if !self.is_terminated() { self.terminate(format!("br label %{}", merge_lbl)); }

            self.start_block(next_lbl);
            if i + 1 == n {
                let fallback = self.intern_string("<?>");
                self.emit(format!("store %JrgStr {}, ptr {}", fallback.text, result_slot));
                self.terminate(format!("br label %{}", merge_lbl));
            }
        }
        self.start_block(merge_lbl);
        Ok(self.load_place(&result_slot, &JrgType::Str))
    }

    fn builtin_print(&mut self, args: &[Expr], to_stderr: bool, newline: bool) -> Result<Val, String> {
        self.ensure_extern("declare void @jrg_print_space(i32)", "jrg_print_space");
        self.ensure_extern("declare void @jrg_print_newline(i32)", "jrg_print_newline");
        let se = if to_stderr { 1 } else { 0 };
        // SEMUA argumen dievaluasi dulu, baru dicetak — seperti acuan: `wok(f(1), f(2))` yang
        // f-nya mencetak dulu menulis `1 ` di antara dua keluaran f.
        let mut nilai = Vec::with_capacity(args.len());
        for a in args { nilai.push(self.codegen_expr(a, None)?); }
        for (i, (v, a)) in nilai.iter().zip(args).enumerate() {
            if i > 0 { self.emit(format!("call void @jrg_print_space(i32 {})", se)); }
            // Span ARGUMENNYA: span kosong dulu terbaca sebagai baris 1 kolom 1.
            self.emit_print_val(v, to_stderr, a.span())?;
        }
        if newline { self.emit(format!("call void @jrg_print_newline(i32 {})", se)); }
        Ok(Val::unit())
    }

    /// Apakah tipe ini punya `to_string` buatan pengguna.
    pub fn punya_hook_tampil(&self, ty: &JrgType) -> bool {
        let nama = match ty {
            JrgType::Struct(n) | JrgType::Enum(n) => n,
            _ => return false,
        };
        self.tables
            .find_method(nama, crate::interpreter::tampil::NAMA_HOOK)
            .is_some()
    }

    /// Pancarkan pemanggilan hook tampilan, dan periksa bentuknya.
    fn panggil_hook_tampil(&mut self, v: &Val) -> Result<Val, String> {
        let nama = match &v.ty {
            JrgType::Struct(n) | JrgType::Enum(n) => n.clone(),
            _ => unreachable!("dipanggil hanya lewat punya_hook_tampil"),
        };
        let hook = crate::interpreter::tampil::NAMA_HOOK;
        let decl = self.tables.find_method(&nama, hook).cloned()
            .expect("sudah diperiksa punya_hook_tampil");

        if decl.params.len() != 1 {
            return Err(format!(
                "`{}::{}` dipakai sebagai hook tampilan, jadi ia harus menerima \
                 `self` saja — yang ini minta {} parameter. Ganti namanya kalau \
                 memang bukan hook tampilan",
                nama, hook, decl.params.len()
            ));
        }

        let place = self.spill_to_place(v);
        let span = Span { lo: crate::lexer::BytePos(0), hi: crate::lexer::BytePos(0), file_id: 0 };
        let hasil = self.call_method_impl_pub(&nama, &decl, &place, &v.ty, &[], span)?;
        if hasil.ty != JrgType::Str {
            return Err(format!(
                "`{}::{}` dipakai sebagai hook tampilan, jadi ia harus \
                 mengembalikan string — yang ini mengembalikan `{}`",
                nama, hook, hasil.ty.display_name()
            ));
        }
        Ok(hasil)
    }

    pub fn val_to_str(&mut self, v: &Val) -> Result<Val, String> {
        match &v.ty {
            // `rune` ikut di sini, bukan lewat `is_numeric`: ia memang bukan bilangan
            // (`'a' + 1` ditolak), tapi ia PUNYA bentuk string — satu karakter.
            JrgType::Nil => Ok(self.intern_string("nil")),
            JrgType::Bool | JrgType::Rune => self.do_cast(v, &JrgType::Str, Span { lo: crate::lexer::BytePos(0), hi: crate::lexer::BytePos(0), file_id: 0 }),
            t if t.is_numeric() => self.do_cast(v, &JrgType::Str, Span { lo: crate::lexer::BytePos(0), hi: crate::lexer::BytePos(0), file_id: 0 }),
            JrgType::Str => Ok(v.clone()),
            JrgType::Fn(..) => Ok(self.tampilan_fungsi(v)),
            JrgType::Array(_, _) | JrgType::Slice(_) | JrgType::Tuple(_) | JrgType::Struct(_) | JrgType::Enum(_) | JrgType::Unit | JrgType::Dict(_)
            | JrgType::Any | JrgType::DictCampur(_) | JrgType::Sinkron(_) | JrgType::Chan(_) | JrgType::Masa(..) => self.build_display_str(v),
            other => Err(format!("konversi `{}` ke string belum didukung", other.display_name())),
        }
    }

    fn builtin_sprint(&mut self, args: &[Expr]) -> Result<Val, String> {
        if args.is_empty() { return Ok(self.intern_string("")); }
        let mut acc: Option<Val> = None;
        for a in args {
            let v = self.codegen_expr(a, None)?;
            let sv = if v.ty == JrgType::Str { v } else { self.val_to_str(&v)? };
            acc = Some(match acc {
                None => sv,
                Some(prev) => {
                    let sp = self.intern_string(" ");
                    let with_space = self.str_concat(&prev, &sp)?;
                    self.str_concat(&with_space, &sv)?
                }
            });
        }
        Ok(acc.unwrap())
    }

    fn builtin_tok(&mut self, args: &[Expr]) -> Result<Val, String> {
        if !args.is_empty() {
            self.builtin_print(args, false, false)?;
            self.ensure_extern("declare void @jrg_flush(i32)", "jrg_flush");
            self.emit("call void @jrg_flush(i32 0)".to_string());
        }
        self.ensure_extern("declare i32 @jrg_read_line(ptr, ptr)", "jrg_read_line");
        let outp = self.alloca_raw("tokp.", "ptr", None);
        let outl = self.alloca_raw("tokl.", "i64", None);
        self.emit(format!("store ptr null, ptr {}", outp));
        self.emit(format!("store i64 0, ptr {}", outl));
        let okr = self.fresh("tokok.");
        self.emit(format!("{} = call i32 @jrg_read_line(ptr {}, ptr {})", okr, outp, outl));
        let _ = okr;
        self.join_str(&outp, &outl)
    }

    fn builtin_len(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        if args.len() != 1 { return Err(format!("[{}:{}] `len` butuh 1 argumen", span.lo.0, span.hi.0)); }
        let v = self.codegen_expr(&args[0], None)?;
        self.panjang_koleksi(&v, span)
    }

    /// Panjang sebuah koleksi yang SUDAH diturunkan.
    pub fn panjang_koleksi(&mut self, v: &Val, span: Span) -> Result<Val, String> {
        match &v.ty {
            JrgType::Str => {
                self.ensure_extern("declare i64 @jrg_str_char_count(ptr, i64)", "jrg_str_char_count");
                let (p, l) = self.split_str(v);
                let r = self.fresh("slen.");
                self.emit(format!("{} = call i64 @jrg_str_char_count(ptr {}, i64 {})", r, p, l));
                Ok(Val { text: r, ty: JrgType::I64 })
            }
            JrgType::Array(_, n) => Ok(Val { text: n.to_string(), ty: JrgType::I64 }),
            JrgType::Slice(_) => {
                let lp = self.spill_to_place(v);
                let lf = self.fresh("sllen.");
                self.emit(format!("{} = getelementptr %JrgSlice, ptr {}, i32 0, i32 1", lf, lp));
                Ok(self.load_place(&lf, &JrgType::I64))
            }
            JrgType::Tuple(ts) => Ok(Val { text: ts.len().to_string(), ty: JrgType::I64 }),
            JrgType::Dict(_) => { let (_, n) = self.bagian_dict_mentah(v); Ok(Val { text: n, ty: JrgType::I64 }) }
            // Nilai bertag: panjangnya dibaca menurut tagnya; yang tak berpanjang gagal saat jalan.
            JrgType::Any => {
                use super::codegen_any::{TAG_DICT, TAG_SLICE, TAG_STR};
                let tag = self.tag_any(v);
                let hasil = self.alloca_raw("lenany.", "i64", Some(8));
                let (l_str, l_agr, l_tolak, l_gab) = (self.fresh_label("lenany.str."),
                    self.fresh_label("lenany.agr."), self.fresh_label("lenany.tolak."), self.fresh_label("lenany.gab."));
                self.terminate(format!(
                    "switch i32 {}, label %{} [ i32 {}, label %{} i32 {}, label %{} i32 {}, label %{} ]",
                    tag.text, l_tolak, TAG_STR, l_str, TAG_SLICE, l_agr, TAG_DICT, l_agr));
                self.start_block(l_str);
                let s = self.buka_any(v, &JrgType::Str, span)?;
                let n = self.panjang_koleksi(&s, span)?;
                self.emit(format!("store i64 {}, ptr {}", n.text, hasil));
                self.terminate(format!("br label %{}", l_gab));
                // Array dan dict sama-sama `{ptr, len}` di muatan kotaknya.
                self.start_block(l_agr);
                let tempat = self.spill_to_place(v);
                let lp = self.fresh("lenanyp.");
                self.emit(format!("{} = getelementptr %JrgAny, ptr {}, i32 0, i32 1, i64 1", lp, tempat));
                let n = self.fresh("lenanyn.");
                self.emit(format!("{} = load i64, ptr {}", n, lp));
                self.emit(format!("store i64 {}, ptr {}", n, hasil));
                self.terminate(format!("br label %{}", l_gab));
                self.start_block(l_tolak);
                self.panik_tipe_any(v, |t| crate::interpreter::pesan_len_tak_bisa(t));
                self.start_block(l_gab);
                Ok(self.load_place(&hasil, &JrgType::I64))
            }
            other => Err(format!("[{}:{}] `len`: tipe `{}` tidak punya panjang", span.lo.0, span.hi.0, other.display_name())),
        }
    }

    /// `range(n)`, `range(a, b)`, `range(a, b, langkah)`.
    fn builtin_range(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        if args.is_empty() || args.len() > 3 {
            return Err(super::galat::tandai(span, format!(
                "`range` butuh 1..3 argumen, diberi {}", args.len())));
        }
        let nilai = |cg: &mut Self, e: &Expr| -> Result<String, String> {
            let v = cg.codegen_expr(e, Some(&JrgType::I64))?;
            Ok(cg.coerce(&v, &JrgType::I64).text)
        };
        let (awal, akhir, langkah) = match args.len() {
            1 => ("0".to_string(), nilai(self, &args[0])?, "1".to_string()),
            2 => (nilai(self, &args[0])?, nilai(self, &args[1])?, "1".to_string()),
            _ => (nilai(self, &args[0])?, nilai(self, &args[1])?, nilai(self, &args[2])?),
        };

        // Langkah nol akan membuat panjangnya tak berhingga; acuannya
        // menolaknya, jadi di sini juga — bukan diam-diam menghasilkan
        // koleksi kosong, yang terlihat seperti jawaban.
        let nol = self.fresh("rgz.");
        self.emit(format!("{} = icmp eq i64 {}, 0", nol, langkah));
        let mati = self.fresh_label("rg.mati.");
        let lanjut = self.fresh_label("rg.lanjut.");
        self.terminate(format!("br i1 {}, label %{}, label %{}", nol, mati, lanjut));
        self.start_block(mati);
        self.panik("range: langkah tidak boleh 0");
        self.start_block(lanjut);

        // panjang = maks(0, ceil((akhir - awal) / langkah))
        let beda = self.fresh("rgd.");
        self.emit(format!("{} = sub i64 {}, {}", beda, akhir, awal));
        let sesuai = self.fresh("rgs.");
        self.emit(format!("{} = sdiv i64 {}, {}", sesuai, beda, langkah));
        let sisa = self.fresh("rgm.");
        self.emit(format!("{} = srem i64 {}, {}", sisa, beda, langkah));
        let ada_sisa = self.fresh("rgas.");
        self.emit(format!("{} = icmp ne i64 {}, 0", ada_sisa, sisa));
        let tambah = self.fresh("rgt.");
        self.emit(format!("{} = zext i1 {} to i64", tambah, ada_sisa));
        let kasar = self.fresh("rgk.");
        self.emit(format!("{} = add i64 {}, {}", kasar, sesuai, tambah));
        let positif = self.fresh("rgp.");
        self.emit(format!("{} = icmp sgt i64 {}, 0", positif, kasar));
        let n = self.fresh("rgn.");
        self.emit(format!("{} = select i1 {}, i64 {}, i64 0", n, positif, kasar));

        let buf = self.buffer_baru(&n, &JrgType::I64);
        let i = self.alloca_raw("rgi.", "i64", Some(8));
        self.emit(format!("store i64 0, ptr {}", i));
        let (cek, badan, usai) = (self.fresh_label("rg.cek."), self.fresh_label("rg.badan."),
                                  self.fresh_label("rg.usai."));
        self.terminate(format!("br label %{}", cek));

        self.start_block(cek.clone());
        let iv = self.load_place(&i, &JrgType::I64);
        let c = self.fresh("rgc.");
        self.emit(format!("{} = icmp slt i64 {}, {}", c, iv.text, n));
        self.terminate(format!("br i1 {}, label %{}, label %{}", c, badan, usai));

        self.start_block(badan);
        let ofs = self.fresh("rgo.");
        self.emit(format!("{} = mul i64 {}, {}", ofs, iv.text, langkah));
        let nilai_i = self.fresh("rgv.");
        self.emit(format!("{} = add i64 {}, {}", nilai_i, awal, ofs));
        let p = self.fresh("rgpp.");
        self.emit(format!("{} = getelementptr i64, ptr {}, i64 {}", p, buf, iv.text));
        self.emit(format!("store i64 {}, ptr {}", nilai_i, p));
        let inc = self.fresh("rginc.");
        self.emit(format!("{} = add i64 {}, 1", inc, iv.text));
        self.emit(format!("store i64 {}, ptr {}", inc, i));
        if !self.is_terminated() { self.terminate(format!("br label %{}", cek)); }

        self.start_block(usai);
        Ok(self.rakit_slice(&buf, &n, &JrgType::I64))
    }

    fn builtin_typeof(&mut self, args: &[Expr]) -> Result<Val, String> {
        if args.len() != 1 { return Err("`typeof` butuh 1 argumen".into()); }
        let v = self.codegen_expr(&args[0], None)?;
        // Sebuah nilai bertag tidak punya jawaban `typeof` yang statis — jawabannya
        // justru tag yang dibawanya saat jalan.
        if v.ty == JrgType::Any { return self.typeof_any(&v); }
        // Fungsi bernama dan closure satu tipe statis; yang membedakan lingkungannya (null =
        // fungsi bernama), dibaca saat jalan. Dulu tiap nilai fungsi dijawab `function`.
        if matches!(v.ty, JrgType::Fn(..)) {
            // Tanpa lingkungan: `function`, atau `enum_ctor:E::A` untuk konstruktor.
            self.pakai_nama_fn = true;
            let c = self.intern_string("closure");
            let (kode, f) = (self.fresh("tfk."), self.fresh("tff."));
            let (env, nol, hasil) = (self.fresh("tfe."), self.fresh("tfn."), self.fresh("tfh."));
            self.emit(format!("{} = extractvalue %JrgFn {}, 0", kode, v.text));
            self.emit(format!("{} = call %JrgStr @jrg_jenis_fn(ptr {})", f, kode));
            self.emit(format!("{} = extractvalue %JrgFn {}, 1", env, v.text));
            self.emit(format!("{} = icmp eq ptr {}, null", nol, env));
            self.emit(format!("{} = select i1 {}, {} {}, {} {}", hasil, nol,
                c.ty.llvm_ty(), f, c.ty.llvm_ty(), c.text));
            return Ok(Val { text: hasil, ty: c.ty.clone() });
        }
        Ok(self.intern_string(&v.ty.nama_typeof()))
    }

    /// `panik(pesan)`: hentikan program dengan pesan itu — satu-satunya cara kode JRG (dan
    /// modul `std/`) melempar galat. Keluaran sebelumnya dikuras dulu oleh `jrg_panic`.
    fn builtin_panik(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        if args.len() != 1 {
            return Err(super::galat::tandai(span, format!("`panik` butuh 1 argumen, dapat {}", args.len())));
        }
        let m = self.codegen_expr(&args[0], Some(&JrgType::Str))?;
        let m = if m.ty != JrgType::Str { self.val_to_str(&m)? } else { m };
        self.ensure_extern("declare void @jrg_panic(ptr, i64) cold nounwind noreturn", "jrg_panic");
        let (mp, ml) = self.split_str(&m);
        self.emit(format!("call void @jrg_panic(ptr {}, i64 {})", mp, ml));
        self.terminate("unreachable".to_string());
        let lanjut = self.fresh_label("sesudah_panik.");
        self.start_block(lanjut);
        // Tidak pernah menghasilkan nilai: sebagai ekor fungsi ia cocok dengan tipe hasil apa pun.
        Ok(Val { text: "undef".to_string(), ty: JrgType::Never })
    }

    fn builtin_assert(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        if args.is_empty() {
            return Err(format!("[{}:{}] {}", span.lo.0, span.hi.0,
                crate::interpreter::pesan_assert_kurang("assert", 1, 0)));
        }
        let cv = self.codegen_expr(&args[0], Some(&JrgType::Bool))?;
        let cv = self.coerce_longgar(&cv, &JrgType::Bool);
        let ci1 = self.to_i1(&cv);
        let msg = if args.len() >= 2 {
            let m = self.codegen_expr(&args[1], Some(&JrgType::Str))?;
            if m.ty != JrgType::Str { self.val_to_str(&m)? } else { m }
        } else {
            self.intern_string("assertion failed")
        };
        let fail_lbl = self.fresh_label("assertfail.");
        let ok_lbl = self.fresh_label("assertok.");
        self.terminate_unlikely_false(&ci1, &ok_lbl, &fail_lbl);
        self.start_block(fail_lbl);
        self.ensure_extern(
            "declare void @jrg_assert_fail(ptr, i64) cold nounwind noreturn",
            "jrg_assert_fail",
        );
        let (mp, ml) = self.split_str(&msg);
        self.emit(format!("call void @jrg_assert_fail(ptr {}, i64 {})", mp, ml));
        self.terminate("unreachable".to_string());
        self.start_block(ok_lbl);
        Ok(Val::unit())
    }

    fn builtin_assert_eq(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        if args.len() < 2 {
            return Err(format!("[{}:{}] {}", span.lo.0, span.hi.0,
                crate::interpreter::pesan_assert_kurang("assert_eq", 2, args.len())));
        }
        let lv = self.codegen_expr(&args[0], None)?;
        let rv0 = self.codegen_expr(&args[1], Some(&lv.ty))?;
        let rv = self.coerce(&rv0, &lv.ty);
        let eqv = self.cmp_binop_pub_entry(&BinaryOp::Eq, &lv, &rv, span)?;
        let i1 = self.to_i1(&eqv);
        let fail_lbl = self.fresh_label("aefail.");
        let ok_lbl = self.fresh_label("aeok.");
        self.terminate_unlikely_false(&i1, &ok_lbl, &fail_lbl);
        self.start_block(fail_lbl);
        let ls = self.val_to_str(&lv)?;
        let rs = self.val_to_str(&rv)?;
        let m1 = self.intern_string("assertion failed: `left == right`\n  left: ");
        let m2 = self.intern_string("\n right: ");
        let p1 = self.str_concat(&m1, &ls)?;
        let p2 = self.str_concat(&p1, &m2)?;
        let full = self.str_concat(&p2, &rs)?;
        self.ensure_extern(
            "declare void @jrg_assert_fail(ptr, i64) cold nounwind noreturn",
            "jrg_assert_fail",
        );
        let (mp, ml) = self.split_str(&full);
        self.emit(format!("call void @jrg_assert_fail(ptr {}, i64 {})", mp, ml));
        self.terminate("unreachable".to_string());
        self.start_block(ok_lbl);
        Ok(Val::unit())
    }

    fn builtin_exit(&mut self, args: &[Expr]) -> Result<Val, String> {
        if args.len() > 1 {
            return Err(crate::interpreter::arity::pesan_kebanyakan("exit", 1, args.len()));
        }
        let code = if args.is_empty() {
            Val { text: "0".into(), ty: JrgType::I32 }
        } else {
            // Tanpa petunjuk tipe: `exit(256)` yang dipaksa ke `i32` lalu dipotong sistem
            // operasi jadi 0 adalah program gagal yang terbaca sukses.
            let v = self.codegen_expr(&args[0], None)?;
            if !v.ty.is_int() {
                return Err(crate::host::pesan_kode_keluar(&v.ty.nama_typeof()));
            }
            self.kode_keluar_terperiksa(&v)
        };
        self.ensure_extern(
            "declare void @jrg_exit(i32) cold nounwind noreturn",
            "jrg_exit",
        );
        self.emit(format!("call void @jrg_exit(i32 {})", code.text));
        self.terminate("unreachable".to_string());
        Ok(Val::unit())
    }

    fn builtin_conv(&mut self, args: &[Expr], target: JrgType, span: Span) -> Result<Val, String> {
        if args.len() != 1 { return Err(format!("[{}:{}] konversi tipe butuh 1 argumen", span.lo.0, span.hi.0)); }
        let v = self.codegen_expr(&args[0], None)?;
        self.do_cast(&v, &target, span)
    }

    fn builtin_konversi(&mut self, args: &[Expr], p: crate::parser::PrimTy, span: Span)
        -> Result<Val, String>
    {
        if args.len() != 1 { return Err(format!("[{}:{}] konversi tipe butuh 1 argumen", span.lo.0, span.hi.0)); }
        let s = super::codegen_konversi::Sasaran::dari_prim(&p)
            .expect("int/uint/float/rune punya sasaran konversi");
        let v = self.codegen_expr(&args[0], None)?;
        self.cast_eksplisit(&v, &s, span)
    }

    /// `str::to_int`/`str::to_float` dan aliasnya — kalimatnya menyebut nama
    /// kanoniknya, sama dengan acuan.
    fn builtin_parse(&mut self, args: &[Expr], bulat: bool, span: Span) -> Result<Val, String> {
        let pintu = if bulat { "str::to_int" } else { "str::to_float" };
        if args.len() != 1 {
            return Err(format!("[{}:{}] `{}` butuh 1 argumen", span.lo.0, span.hi.0, pintu));
        }
        let v = self.codegen_expr(&args[0], None)?;
        if bulat { self.pintu_parse_bulat(&v, pintu, span) }
        else { self.pintu_parse_pecahan(&v, pintu, span) }
    }

    // ── math ─────────────────────────────────────────────────────────────

    fn emit_float_intrinsic1(&mut self, stem: &str, v: &Val) -> Val {
        debug_assert!(v.ty.is_float());
        let suffix = if v.ty == JrgType::F32 { "f32" } else { "f64" };
        let symbol = format!("llvm.{}.{}", stem, suffix);
        let llvm_ty = v.ty.llvm_ty();
        self.ensure_extern(
            &format!("declare {} @{}({})", llvm_ty, symbol, llvm_ty),
            &symbol,
        );
        let result = self.fresh("fintr.");
        self.emit(format!(
            "{} = call{} {} @{}({} {})",
            result,
            self.fast_math_flag(),
            llvm_ty,
            symbol,
            llvm_ty,
            v.text
        ));
        Val { text: result, ty: v.ty.clone() }
    }

    fn emit_float_intrinsic2(&mut self, stem: &str, a: &Val, b: &Val) -> Val {
        debug_assert!(a.ty.is_float() && a.ty == b.ty);
        let suffix = if a.ty == JrgType::F32 { "f32" } else { "f64" };
        let symbol = format!("llvm.{}.{}", stem, suffix);
        let llvm_ty = a.ty.llvm_ty();
        self.ensure_extern(
            &format!("declare {} @{}({}, {})", llvm_ty, symbol, llvm_ty, llvm_ty),
            &symbol,
        );
        let result = self.fresh("fintr.");
        self.emit(format!(
            "{} = call{} {} @{}({} {}, {} {})",
            result,
            self.fast_math_flag(),
            llvm_ty,
            symbol,
            llvm_ty,
            a.text,
            llvm_ty,
            b.text
        ));
        Val { text: result, ty: a.ty.clone() }
    }

    fn builtin_abs(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        if args.len() != 1 { return Err(format!("[{}:{}] `abs` butuh 1 argumen", span.lo.0, span.hi.0)); }
        // Yang bukan angka dulu dijawab apa adanya: `math::abs(true)` = `true`.
        let v = self.angka_math(&args[0], "math::abs", span)?;
        if v.ty.is_float() {
            Ok(self.emit_float_intrinsic1("fabs", &v))
        } else if v.ty.is_signed_int() {
            // `0 - x` diperiksa: `math::abs` atas nilai paling negatif dulu menjawab dirinya sendiri.
            // Cabangnya cuma dijalani kalau `x` negatif, jadi nilai positif tidak ikut diperiksa.
            let nol = Val { text: "0".into(), ty: v.ty.clone() };
            let cmp0 = self.fresh("absc0.");
            self.emit(format!("{} = icmp eq {} {}, {}", cmp0, v.ty.llvm_ty(), v.text,
                super::codegen_konversi::jangkauan(&v.ty).0));
            let aman = self.fresh("absx.");
            self.emit(format!("{} = select i1 {}, {} {}, {} 0", aman, cmp0, v.ty.llvm_ty(), v.text, v.ty.llvm_ty()));
            let _ = self.hitung_terperiksa(&BinaryOp::Sub, &nol, &Val { text: aman, ty: v.ty.clone() });
            let neg = self.fresh("absneg.");
            self.emit(format!("{} = sub {} 0, {}", neg, v.ty.llvm_ty(), v.text));
            let cmp = self.fresh("absc.");
            self.emit(format!("{} = icmp slt {} {}, 0", cmp, v.ty.llvm_ty(), v.text));
            let r = self.fresh("abs.");
            self.emit(format!("{} = select i1 {}, {} {}, {} {}", r, cmp, v.ty.llvm_ty(), neg, v.ty.llvm_ty(), v.text));
            Ok(Val { text: r, ty: v.ty })
        } else {
            Ok(v)
        }
    }

    fn builtin_libm1(&mut self, args: &[Expr], fname: &str, span: Span) -> Result<Val, String> {
        if args.len() != 1 { return Err(format!("[{}:{}] `{}` butuh 1 argumen", span.lo.0, span.hi.0, fname)); }
        let v0 = self.codegen_expr(&args[0], Some(&JrgType::F64))?;
        let v = self.coerce_longgar(&v0, &JrgType::F64);
        let intrinsic = match fname {
            "sqrt" => Some("sqrt"),
            "floor" => Some("floor"),
            "ceil" => Some("ceil"),
            "round" => Some("round"),
            "trunc" => Some("trunc"),
            "sin" => Some("sin"),
            "cos" => Some("cos"),
            "log" => Some("log"),
            "log10" => Some("log10"),
            "log2" => Some("log2"),
            "exp" => Some("exp"),
            _ => None,
        };
        if let Some(stem) = intrinsic {
            return Ok(self.emit_float_intrinsic1(stem, &v));
        }
        self.ensure_extern(&format!("declare double @{}(double)", fname), fname);
        let r = self.fresh("libm.");
        self.emit(format!(
            "{} = call{} double @{}(double {})",
            r,
            self.fast_math_flag(),
            fname,
            v.text
        ));
        Ok(Val { text: r, ty: JrgType::F64 })
    }

    fn builtin_libm2(&mut self, args: &[Expr], fname: &str, span: Span) -> Result<Val, String> {
        if args.len() != 2 { return Err(format!("[{}:{}] `{}` butuh 2 argumen", span.lo.0, span.hi.0, fname)); }
        let a0 = self.codegen_expr(&args[0], Some(&JrgType::F64))?;
        let a = self.coerce_longgar(&a0, &JrgType::F64);
        let b0 = self.codegen_expr(&args[1], Some(&JrgType::F64))?;
        let b = self.coerce_longgar(&b0, &JrgType::F64);
        self.ensure_extern(&format!("declare double @{}(double, double)", fname), fname);
        let r = self.fresh("libm2.");
        self.emit(format!(
            "{} = call{} double @{}(double {}, double {})",
            r,
            self.fast_math_flag(),
            fname,
            a.text,
            b.text
        ));
        Ok(Val { text: r, ty: JrgType::F64 })
    }

    /// `math::pow` selalu pecahan, seperti acuan (`math::pow(2, 10)` = `1024.0`); `**` antar
    /// bilangan bulat yang tetap bulat. Dulu argumen kedua dikoersi ke tipe argumen pertama.
    fn builtin_math_pow(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        if args.len() != 2 { return Err(format!("[{}:{}] `pow` butuh 2 argumen", span.lo.0, span.hi.0)); }
        let a = self.angka_math(&args[0], "math::pow", span)?;
        let b = self.angka_math(&args[1], "math::pow", span)?;
        let a = self.coerce_longgar(&a, &JrgType::F64);
        let b = self.coerce_longgar(&b, &JrgType::F64);
        self.pow_binop(&a, &b, span)
    }

    /// Argumen angka sebuah `math::`, dengan kalimat acuan untuk yang bukan angka.
    fn angka_math(&mut self, e: &Expr, nama: &str, span: Span) -> Result<Val, String> {
        let v = self.codegen_expr(e, None)?;
        if !v.ty.is_numeric() && v.ty != JrgType::Never {
            return Err(super::galat::tandai(span, format!(
                "{}: butuh angka, dapat `{}`", nama, v.ty.nama_typeof())));
        }
        Ok(v)
    }

    /// Rumus acuan, `a < b $ a : b` (dan `>` untuk max): jawabannya SALAH SATU argumen apa
    /// adanya, jadi `math::min(1.0, NaN)` = NaN. Dulu `minnum` membuang NaN, dan argumen
    /// kedua dikoersi ke tipe yang pertama (`math::max(1, 2.5)` = 2).
    fn builtin_minmax(&mut self, args: &[Expr], want_min: bool, span: Span) -> Result<Val, String> {
        let nama = if want_min { "math::min" } else { "math::max" };
        if args.len() != 2 { return Err(format!("[{}:{}] `{}` butuh 2 argumen", span.lo.0, span.hi.0, nama)); }
        let a = self.angka_math(&args[0], nama, span)?;
        let b = self.angka_math(&args[1], nama, span)?;
        // Bulat lawan pecahan: acuan menjawab argumen ASLINYA, jadi tipe hasilnya baru
        // diketahui saat jalan — backend bertipe statis tidak punya satu tipe untuknya.
        if a.ty.is_float() != b.ty.is_float() {
            return Err(super::galat::tandai(span, format!(
                "`{}` atas bilangan bulat dan pecahan sekaligus belum didukung backend LLVM: \
                 jawaban acuannya argumen aslinya, jadi tipenya (`int` atau `float`) baru \
                 diketahui saat jalan\n\
                 bantuan: samakan tipenya dulu, mis. `{}(x as float, 0.0)`", nama, nama)));
        }
        let b = self.coerce(&b, &a.ty);
        if a.ty.is_float() {
            let pred = if want_min { "olt" } else { "ogt" };
            let cmp = self.fresh("fmmc.");
            self.emit(format!("{} = fcmp {} {} {}, {}", cmp, pred, a.ty.llvm_ty(), a.text, b.text));
            let r = self.fresh("fmm.");
            self.emit(format!("{} = select i1 {}, {} {}, {} {}", r, cmp, a.ty.llvm_ty(), a.text, a.ty.llvm_ty(), b.text));
            Ok(Val { text: r, ty: a.ty })
        } else if a.ty.is_int() {
            let pred = if a.ty.is_signed_int() {
                if want_min { "slt" } else { "sgt" }
            } else if want_min { "ult" } else { "ugt" };
            let cmp = self.fresh("immc.");
            self.emit(format!("{} = icmp {} {} {}, {}", cmp, pred, a.ty.llvm_ty(), a.text, b.text));
            let r = self.fresh("imm.");
            self.emit(format!("{} = select i1 {}, {} {}, {} {}", r, cmp, a.ty.llvm_ty(), a.text, a.ty.llvm_ty(), b.text));
            Ok(Val { text: r, ty: a.ty })
        } else {
            Err(format!("[{}:{}] `math::min`/`max` butuh tipe numerik", span.lo.0, span.hi.0))
        }
    }

    fn builtin_clamp(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        if args.len() != 3 { return Err(format!("[{}:{}] `clamp` butuh 3 argumen (nilai, min, max)", span.lo.0, span.hi.0)); }
        let v = self.codegen_expr(&args[0], None)?;
        let lo0 = self.codegen_expr(&args[1], Some(&v.ty))?;
        let lo = self.coerce(&lo0, &v.ty);
        let hi0 = self.codegen_expr(&args[2], Some(&v.ty))?;
        let hi = self.coerce(&hi0, &v.ty);
        let lowered = self.builtin_minmax_vals(&v, &lo, false)?; // max(v, lo)
        self.builtin_minmax_vals(&lowered, &hi, true) // min(that, hi)
    }

    fn builtin_minmax_vals(&mut self, a: &Val, b: &Val, want_min: bool) -> Result<Val, String> {
        if a.ty.is_float() {
            Ok(self.emit_float_intrinsic2(
                if want_min { "minnum" } else { "maxnum" },
                a,
                b,
            ))
        } else if a.ty.is_int() {
            let pred = if a.ty.is_signed_int() {
                if want_min { "slt" } else { "sgt" }
            } else if want_min { "ult" } else { "ugt" };
            let cmp = self.fresh("immc.");
            self.emit(format!("{} = icmp {} {} {}, {}", cmp, pred, a.ty.llvm_ty(), a.text, b.text));
            let r = self.fresh("imm.");
            self.emit(format!("{} = select i1 {}, {} {}, {} {}", r, cmp, a.ty.llvm_ty(), a.text, a.ty.llvm_ty(), b.text));
            Ok(Val { text: r, ty: a.ty.clone() })
        } else {
            Err("`clamp` butuh tipe numerik".into())
        }
    }

    fn builtin_is_nan(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        if args.len() != 1 { return Err(format!("[{}:{}] `math::is_nan` butuh 1 argumen", span.lo.0, span.hi.0)); }
        let v0 = self.codegen_expr(&args[0], Some(&JrgType::F64))?;
        let v = self.coerce_longgar(&v0, &JrgType::F64);
        let r = self.fresh("isnan.");
        self.emit(format!(
            "{} = fcmp uno double {}, {}",
            r,
            v.text,
            v.text
        ));
        Ok(self.from_i1(&r))
    }

    fn builtin_is_inf(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        if args.len() != 1 { return Err(format!("[{}:{}] `math::is_inf` butuh 1 argumen", span.lo.0, span.hi.0)); }
        let v0 = self.codegen_expr(&args[0], Some(&JrgType::F64))?;
        let v = self.coerce_longgar(&v0, &JrgType::F64);
        self.ensure_extern(
            "declare double @llvm.fabs.f64(double)",
            "llvm.fabs.f64",
        );
        let abs = self.fresh("isinfab.");
        self.emit(format!(
            "{} = call double @llvm.fabs.f64(double {})",
            abs,
            v.text
        ));
        let r = self.fresh("isinf.");
        self.emit(format!(
            "{} = fcmp oeq double {}, 0x7FF0000000000000",
            r,
            abs
        ));
        Ok(self.from_i1(&r))
    }

    fn builtin_sign(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        if args.len() != 1 { return Err(format!("[{}:{}] `math::sign` butuh 1 argumen", span.lo.0, span.hi.0)); }
        let v = self.codegen_expr(&args[0], None)?;
        if v.ty.is_float() {
            let v64 = self.coerce_longgar(&v, &JrgType::F64);
            self.ensure_extern("declare double @jrg_sign_f64(double)", "jrg_sign_f64");
            let r = self.fresh("sign.");
            // Tanpa bendera fast-math: NaN harus sampai ke runtime apa adanya.
            self.emit(format!("{} = call double @jrg_sign_f64(double {})", r, v64.text));
            Ok(Val { text: r, ty: JrgType::F64 })
        } else {
            let v64 = self.coerce_longgar(&v, &JrgType::I64);
            self.ensure_extern("declare i64 @jrg_sign_i64(i64)", "jrg_sign_i64");
            let r = self.fresh("signi.");
            self.emit(format!("{} = call i64 @jrg_sign_i64(i64 {})", r, v64.text));
            Ok(self.coerce(&Val { text: r, ty: JrgType::I64 }, &v.ty))
        }
    }

    fn builtin_runtime_i64_1(&mut self, args: &[Expr], fname: &str, span: Span) -> Result<Val, String> {
        if args.len() != 1 { return Err(format!("[{}:{}] `{}` butuh 1 argumen", span.lo.0, span.hi.0, fname)); }
        let v0 = self.codegen_expr(&args[0], Some(&JrgType::I64))?;
        let v = self.bulat_arg(&v0, "", span)?;
        self.ensure_extern(&format!("declare i64 @{}(i64)", fname), fname);
        let r = self.fresh("rti1.");
        self.emit(format!("{} = call i64 @{}(i64 {})", r, fname, v.text));
        Ok(Val { text: r, ty: JrgType::I64 })
    }

    fn builtin_runtime_i64_2(&mut self, args: &[Expr], fname: &str, span: Span) -> Result<Val, String> {
        if args.len() != 2 { return Err(format!("[{}:{}] `{}` butuh 2 argumen", span.lo.0, span.hi.0, fname)); }
        let a0 = self.codegen_expr(&args[0], Some(&JrgType::I64))?;
        let a = self.bulat_arg(&a0, "", span)?;
        let b0 = self.codegen_expr(&args[1], Some(&JrgType::I64))?;
        let b = self.bulat_arg(&b0, "", span)?;
        self.ensure_extern(&format!("declare i64 @{}(i64, i64)", fname), fname);
        let r = self.fresh("rti2.");
        self.emit(format!("{} = call i64 @{}(i64 {}, i64 {})", r, fname, a.text, b.text));
        Ok(Val { text: r, ty: JrgType::I64 })
    }

    /// `math::wrap_*`: satu instruksi tanpa pemeriksaan, hasilnya `u64`. Operand bertipe
    /// lain dibawa ke 64 bit dulu (bit-nya, bukan nilainya — itulah arti modulo 2^64).
    fn builtin_bungkus(&mut self, args: &[Expr], instr: &str, nama: &str, span: Span) -> Result<Val, String> {
        if args.len() != 2 {
            return Err(crate::llvm::galat::tandai(span, format!("`{}` butuh 2 argumen, dapat {}", nama, args.len())));
        }
        let mut ops = Vec::new();
        for a in args {
            // `-1` ditulis sebagai bilangan bertanda; bit-nya yang dipakai (modulo 2^64).
            let harap = if matches!(a, Expr::Unary { op: UnaryOp::Neg, .. }) { JrgType::I64 } else { JrgType::U64 };
            let v = self.codegen_expr(a, Some(&harap))?;
            if !v.ty.is_int() {
                return Err(crate::llvm::galat::tandai(span, format!(
                    "{}: butuh bilangan bulat, dapat `{}`", nama, v.ty.nama_typeof())));
            }
            let bit = v.ty.int_bits().unwrap_or(64);
            let teks = if bit == 64 { v.text.clone() } else {
                let r = self.fresh("w64.");
                let cara = if bit > 64 { "trunc" } else if v.ty.is_signed_int() { "sext" } else { "zext" };
                self.emit(format!("{} = {} {} {} to i64", r, cara, v.ty.llvm_ty(), v.text));
                r
            };
            ops.push(teks);
        }
        let r = self.fresh("bungkus.");
        self.emit(format!("{} = {} i64 {}, {}", r, instr, ops[0], ops[1]));
        Ok(Val { text: r, ty: JrgType::U64 })
    }

    // ── str:: helpers ────────────────────────────────────────────────────

    pub fn builtin_str1_val(&mut self, v: &Val, fname: &str) -> Result<Val, String> {
        self.ensure_extern(&format!("declare void @{}(ptr, i64, ptr, ptr)", fname), fname);
        let (p, l) = self.split_str(v);
        let outp = self.alloca_raw("s1p.", "ptr", None);
        let outl = self.alloca_raw("s1l.", "i64", None);
        self.emit(format!("call void @{}(ptr {}, i64 {}, ptr {}, ptr {})", fname, p, l, outp, outl));
        self.join_str(&outp, &outl)
    }

    /// `lapis::os(op, a, b, i) -> (int, string)`: hasil (negatif = -errno) dan teksnya.
    fn builtin_lapis_os(&mut self, args: &[Expr]) -> Result<Val, String> {
        let op = self.codegen_expr(&args[0], Some(&JrgType::I64))?;
        let a = self.codegen_expr(&args[1], Some(&JrgType::Str))?;
        let b = self.codegen_expr(&args[2], Some(&JrgType::Str))?;
        let i = self.codegen_expr(&args[3], Some(&JrgType::I64))?;
        if op.ty != JrgType::I64 || a.ty != JrgType::Str || b.ty != JrgType::Str || i.ty != JrgType::I64 {
            return Err("`lapis::os` butuh (int, string, string, int)".to_string());
        }
        self.ensure_extern("declare i64 @jrg_os(i32, ptr, i64, ptr, i64, i64, ptr)", "jrg_os");
        let (ap, al) = self.split_str(&a);
        let (bp, bl) = self.split_str(&b);
        let keluar = self.alloca_typed("osk.", &JrgType::Str);
        let op32 = self.fresh("osop.");
        self.emit(format!("{} = trunc i64 {} to i32", op32, op.text));
        let r = self.fresh("osr.");
        self.emit(format!("{} = call i64 @jrg_os(i32 {}, ptr {}, i64 {}, ptr {}, i64 {}, i64 {}, ptr {})",
            r, op32, ap, al, bp, bl, i.text, keluar));
        let teks = self.load_place(&keluar, &JrgType::Str);
        let ty = JrgType::Tuple(vec![JrgType::I64, JrgType::Str]);
        let t0 = self.fresh("ost.");
        self.emit(format!("{} = insertvalue {} undef, i64 {}, 0", t0, ty.llvm_ty(), r));
        let t1 = self.fresh("ost.");
        self.emit(format!("{} = insertvalue {} {}, %JrgStr {}, 1", t1, ty.llvm_ty(), t0, teks.text));
        Ok(Val { text: t1, ty })
    }

    /// `json::parse(s)`: nilai bertag, diurai runtime dengan pengurai yang sama dengan dua mesin lain.
    fn builtin_json_parse(&mut self, e: &Expr, span: Span) -> Result<Val, String> {
        let s = self.arg_teks(std::slice::from_ref(e), "json::parse", span)?;
        self.ensure_extern("declare void @jrg_json_parse(ptr, i64, ptr)", "jrg_json_parse");
        let (p, l) = self.split_str(&s);
        let keluar = self.alloca_typed("jsp.", &JrgType::Any);
        self.emit(format!("call void @jrg_json_parse(ptr {}, i64 {}, ptr {})", p, l, keluar));
        Ok(self.load_place(&keluar, &JrgType::Any))
    }

    /// `json::stringify(v)` (`indent` `None`) dan `json::pretty(v, lebar?)`.
    fn builtin_json_tulis(&mut self, e: &Expr, indent: Option<Option<&Expr>>, span: Span) -> Result<Val, String> {
        let v = self.codegen_expr(e, None)?;
        let v = super::codegen_any::samakan_dict(&v);
        let buram = super::codegen_any::tag_untuk(&v.ty) == Some(super::codegen_any::TAG_BURAM);
        let kotak = if buram { Err(String::new()) } else { self.kotak_any(&v, span) };
        // Yang memang TIDAK punya padanan JSON dijawab kalimat dua mesin lain; sisanya celah backend.
        let tak_ada = match &v.ty {
            JrgType::Sinkron(k) => Some((if *k { "mutex" } else { "atomic" }, crate::json_inti::JALAN_PEGANGAN)),
            JrgType::Chan(_) => Some(("channel", crate::json_inti::JALAN_CHANNEL)),
            JrgType::Fn(..) => Some(("fungsi", crate::json_inti::JALAN_FUNGSI)),
            _ => None,
        };
        if let Some((apa, jalan)) = tak_ada {
            return Err(super::galat::tandai(span, crate::json_inti::pesan_tak_bisa(apa, jalan)));
        }
        let kotak = kotak.map_err(|_| super::galat::tandai(span, format!(
            "nilai bertipe `{}` belum bisa disandikan jadi JSON di backend `--emit-llvm` — baru nil, bool, \
             angka, teks, array dan dict", v.ty.display_name())))?;
        let rapi = if indent.is_some() { 1 } else { 0 };
        let lebar = match indent {
            None | Some(None) => "2".to_string(),
            Some(Some(x)) => {
                let n = self.codegen_expr(x, Some(&JrgType::I64))?;
                if n.ty != JrgType::I64 {
                    return Err(super::galat::tandai(span, format!(
                        "json::pretty: argumen ke-2 (lebar indentasi) harus bilangan bulat, dapat `{}`", n.ty.nama_typeof())));
                }
                n.text
            }
        };
        self.ensure_extern("declare void @jrg_json_tulis(ptr, i32, i64, ptr, ptr)", "jrg_json_tulis");
        let tempat = self.spill_to_place(&kotak);
        let outp = self.alloca_raw("jtp.", "ptr", None);
        let outl = self.alloca_raw("jtl.", "i64", None);
        self.emit(format!("call void @jrg_json_tulis(ptr {}, i32 {}, i64 {}, ptr {}, ptr {})", tempat, rapi, lebar, outp, outl));
        self.join_str(&outp, &outl)
    }

    fn nama_masa(&self, nama: &str, n: usize) -> bool {
        let item = nama.rsplit("::").next().unwrap_or("");
        matches!((item, n), ("await" | "join" | "is_done" | "try_get" | "all" | "race", 1) | ("timeout", 2))
    }

    fn coba_nama_chan(&self, nama: &str, n: usize) -> bool {
        matches!((nama, n), ("send" | "try_send", 2)
            | ("recv" | "try_recv" | "close" | "len" | "cap" | "is_closed" | "has_value", 1))
    }

    /// `dict::is_dict(v)`: dari tipenya, atau dari tag kalau nilainya bertag.
    fn builtin_is_dict(&mut self, e: &Expr) -> Result<Val, String> {
        let v = self.codegen_expr(e, None)?;
        match &v.ty {
            JrgType::Dict(_) | JrgType::DictCampur(_) => Ok(Val { text: "1".into(), ty: JrgType::Bool }),
            JrgType::Any => {
                let tag = self.tag_any(&v);
                let c = self.fresh("isd.");
                self.emit(format!("{} = icmp eq i32 {}, {}", c, tag.text, super::codegen_any::TAG_DICT));
                Ok(self.from_i1(&c))
            }
            _ => Ok(Val { text: "0".into(), ty: JrgType::Bool }),
        }
    }

    /// Satu argumen `str::` yang HARUS teks; tipe lain ditolak dengan kalimat acuan.
    fn arg_teks(&mut self, args: &[Expr], nama: &str, span: Span) -> Result<Val, String> {
        if args.len() != 1 { return Err(format!("[{}:{}] `{}` butuh 1 argumen string", span.lo.0, span.hi.0, nama)); }
        let v = self.codegen_expr(&args[0], Some(&JrgType::Str))?;
        if v.ty != JrgType::Str {
            return Err(super::galat::tandai(span,
                crate::interpreter::pesan_arg_harus_teks(nama, "argumen pertama", &v.ty.nama_typeof())));
        }
        Ok(v)
    }

    /// Predikat `str::is_*`, satu pintu runtime (`jrg_str_adalah`).
    fn builtin_str_adalah(&mut self, args: &[Expr], kode: i32, nama: &str, span: Span) -> Result<Val, String> {
        let v = self.arg_teks(args, nama, span)?;
        self.ensure_extern("declare i32 @jrg_str_adalah(i32, ptr, i64)", "jrg_str_adalah");
        let (p, l) = self.split_str(&v);
        let r = self.fresh("sad.");
        self.emit(format!("{} = call i32 @jrg_str_adalah(i32 {}, ptr {}, i64 {})", r, kode, p, l));
        let i1 = self.fresh("sadi1.");
        self.emit(format!("{} = icmp ne i32 {}, 0", i1, r));
        Ok(self.from_i1(&i1))
    }

    /// `str::words`/`str::bytes`: teks jadi array ber-elemen `elem`.
    fn builtin_str_pecah(&mut self, args: &[Expr], fname: &str, elem: JrgType, nama: &str, span: Span) -> Result<Val, String> {
        let v = self.arg_teks(args, nama, span)?;
        self.ensure_extern(&format!("declare void @{}(ptr, i64, ptr, ptr)", fname), fname);
        let (p, l) = self.split_str(&v);
        let op = self.alloca_raw("spp.", "ptr", Some(8));
        let ol = self.alloca_raw("spl.", "i64", Some(8));
        self.emit(format!("call void @{}(ptr {}, i64 {}, ptr {}, ptr {})", fname, p, l, op, ol));
        let d = self.fresh("spd.");
        self.emit(format!("{} = load ptr, ptr {}", d, op));
        let n = self.fresh("spn.");
        self.emit(format!("{} = load i64, ptr {}", n, ol));
        Ok(self.rakit_slice(&d, &n, &elem))
    }

    fn builtin_str_byte_len(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        let v = self.arg_teks(args, "str::byte_len", span)?;
        let (_, l) = self.split_str(&v);
        Ok(Val { text: l, ty: JrgType::I64 })
    }

    fn builtin_str1(&mut self, args: &[Expr], fname: &str, span: Span) -> Result<Val, String> {
        if args.len() != 1 { return Err(format!("[{}:{}] `str::*` butuh 1 argumen string", span.lo.0, span.hi.0)); }
        let v0 = self.codegen_expr(&args[0], Some(&JrgType::Str))?;
        let v = self.coerce(&v0, &JrgType::Str);
        self.builtin_str1_val(&v, fname)
    }

    fn builtin_str_bool2(&mut self, args: &[Expr], fname: &str, span: Span) -> Result<Val, String> {
        if args.len() != 2 { return Err(format!("[{}:{}] `str::*` butuh 2 argumen string", span.lo.0, span.hi.0)); }
        let a0 = self.codegen_expr(&args[0], Some(&JrgType::Str))?;
        let a = self.coerce(&a0, &JrgType::Str);
        let b0 = self.codegen_expr(&args[1], Some(&JrgType::Str))?;
        let b = self.coerce(&b0, &JrgType::Str);
        self.ensure_extern(&format!("declare i32 @{}(ptr, i64, ptr, i64)", fname), fname);
        let (ap, al) = self.split_str(&a);
        let (bp, bl) = self.split_str(&b);
        let r = self.fresh("sb2.");
        self.emit(format!("{} = call i32 @{}(ptr {}, i64 {}, ptr {}, i64 {})", r, fname, ap, al, bp, bl));
        let i1 = self.fresh("sb2i1.");
        self.emit(format!("{} = icmp ne i32 {}, 0", i1, r));
        Ok(self.from_i1(&i1))
    }

    fn builtin_str_repeat(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        if args.len() != 2 { return Err(format!("[{}:{}] `str::repeat` butuh 2 argumen (string, n)", span.lo.0, span.hi.0)); }
        let s0 = self.codegen_expr(&args[0], Some(&JrgType::Str))?;
        let s = self.coerce(&s0, &JrgType::Str);
        let n0 = self.codegen_expr(&args[1], Some(&JrgType::I64))?;
        let n = self.bulat_arg(&n0, "str::repeat", span)?;
        self.ensure_extern("declare void @jrg_str_repeat(ptr, i64, i64, ptr, ptr)", "jrg_str_repeat");
        let (p, l) = self.split_str(&s);
        let outp = self.alloca_raw("srp.", "ptr", None);
        let outl = self.alloca_raw("srl.", "i64", None);
        self.emit(format!("call void @jrg_str_repeat(ptr {}, i64 {}, i64 {}, ptr {}, ptr {})", p, l, n.text, outp, outl));
        self.join_str(&outp, &outl)
    }

    fn builtin_str_replace(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        if args.len() != 3 { return Err(format!("[{}:{}] `str::replace` butuh 3 argumen (string, dari, ke)", span.lo.0, span.hi.0)); }
        let s0 = self.codegen_expr(&args[0], Some(&JrgType::Str))?;
        let s = self.coerce(&s0, &JrgType::Str);
        let f0 = self.codegen_expr(&args[1], Some(&JrgType::Str))?;
        let f = self.coerce(&f0, &JrgType::Str);
        let t0 = self.codegen_expr(&args[2], Some(&JrgType::Str))?;
        let t = self.coerce(&t0, &JrgType::Str);
        self.ensure_extern("declare void @jrg_str_replace(ptr, i64, ptr, i64, ptr, i64, ptr, ptr)", "jrg_str_replace");
        let (sp, sl) = self.split_str(&s);
        let (fp, fl) = self.split_str(&f);
        let (tp, tl) = self.split_str(&t);
        let outp = self.alloca_raw("srep.", "ptr", None);
        let outl = self.alloca_raw("srel.", "i64", None);
        self.emit(format!("call void @jrg_str_replace(ptr {}, i64 {}, ptr {}, i64 {}, ptr {}, i64 {}, ptr {}, ptr {})",
            sp, sl, fp, fl, tp, tl, outp, outl));
        self.join_str(&outp, &outl)
    }

    /// `str::format` — memakai pemindai template yang SAMA dengan interpreter.
    fn builtin_str_format(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        use crate::interpreter::SegmenFormat;
        if args.is_empty() { return Err(format!("[{}:{}] `str::format` butuh minimal 1 argumen (format string)", span.lo.0, span.hi.0)); }
        let fmt_str = match &args[0] {
            Expr::Literal(LitKind::Str(s), _) | Expr::Literal(LitKind::RawStr(s), _) => s.clone(),
            _ => return Err(format!("[{}:{}] `str::format`: argumen pertama harus literal string (backend LLVM v1 mem-parsing format string saat kompilasi)", span.lo.0, span.hi.0)),
        };
        let segmen = SegmenFormat::pindai(&fmt_str)
            .map_err(|e| format!("[{}:{}] {}", span.lo.0, span.hi.0, e))?;
        let rest = &args[1..];
        let jml = segmen.iter().filter(|s| matches!(s, SegmenFormat::Nilai { .. })).count();
        if jml != rest.len() {
            return Err(format!("[{}:{}] {}", span.lo.0, span.hi.0,
                SegmenFormat::pesan_jumlah(jml, rest.len(), &fmt_str)));
        }

        let mut parts: Vec<Val> = Vec::new();
        let mut arg_i = 0usize;
        for seg in &segmen {
            match seg {
                SegmenFormat::Teks(t) => parts.push(self.intern_string(t)),
                SegmenFormat::Nilai { desimal } => {
                    let v = self.codegen_expr(&rest[arg_i], None)?;
                    arg_i += 1;
                    let sv = match desimal {
                        None => self.val_to_str(&v)?,
                        Some(n) => self.str_desimal(&v, *n, span)?,
                    };
                    parts.push(sv);
                }
            }
        }
        if parts.is_empty() { return Ok(self.intern_string("")); }
        let mut acc = parts[0].clone();
        for p in &parts[1..] {
            acc = self.str_concat(&acc, p)?;
        }
        Ok(acc)
    }
}

impl Codegen {
    /// Layani `str::…` yang MEMANG belum ada, dan tidak lebih dari itu.
    pub fn coba_builtin_str(&mut self, nama: &str, args: &[Expr], span: Span)
        -> Option<Result<Val, String>>
    {
        let item = nama.strip_prefix("str::")?;
        // Yang punya bentuk method dan TIDAK punya builtin `str::` sendiri.
        const LEWAT_METHOD: &[(&str, usize)] = &[
            ("split", 2), ("find", 2), ("index_of", 2),
            ("to_uppercase", 1), ("to_lowercase", 1), ("len", 1),
        ];
        if let Some((_, n)) = LEWAT_METHOD.iter().find(|(m, _)| *m == item) {
            if args.len() != *n { return None; }
            return Some(self.codegen_method(&args[0], item, &args[1..], None, span));
        }
        if matches!(item, "sub" | "substring" | "slice") && matches!(args.len(), 1 | 2 | 3) {
            return Some((|| {
                let v = self.codegen_expr(&args[0], Some(&JrgType::Str))?;
                if v.ty != JrgType::Str {
                    return Err(super::galat::tandai(span, crate::interpreter::pesan_arg_harus_teks(
                        nama, "argumen pertama", &v.ty.nama_typeof())));
                }
                if args.len() == 1 { return Ok(v); }
                self.potong_string(&v, &Some(Box::new(args[1].clone())), &args.get(2).map(|e| Box::new(e.clone())))
            })());
        }
        if item == "join" && args.len() == 2 {
            return Some((|| {
                let harap = JrgType::Slice(Box::new(JrgType::Str));
                let a = self.codegen_expr(&args[0], Some(&harap))?;
                let elem = match &a.ty {
                    JrgType::Array(e, _) | JrgType::Slice(e) => (**e).clone(),
                    lain => return Err(super::galat::tandai(span,
                        crate::interpreter::pesan_arg_harus_array("str::join", 1, &lain.display_name()))),
                };
                if elem != JrgType::Str {
                    // Elemen bukan teks: lapis ketiga yang menampilkannya satu per satu.
                    if let Some(kunci) = self.kunci_lapis("arr", "join") {
                        let tempat = self.spill_to_place(&a);
                        let pertama = self.lokal_sementara(tempat, a.ty.clone(), span);
                        return self.call_named_function(&kunci, &[pertama, args[1].clone()], None, span);
                    }
                    return Err(super::galat::tandai(span, format!(
                        "`str::join` atas array berelemen `{}` belum didukung backend `--emit-llvm` — \
                         baru array teks\n  bantuan: ubah elemennya dulu, mis. `arr::map(a, |x| str(x))`",
                        elem.display_name())));
                }
                let (ptr, len) = self.bagian_koleksi_pub(&a, &elem);
                let p = self.codegen_expr(&args[1], Some(&JrgType::Str))?;
                if p.ty != JrgType::Str {
                    return Err(super::galat::tandai(span,
                        crate::interpreter::pesan_arg_harus_teks("str::join", "pemisahnya", &p.ty.display_name())));
                }
                let (sp, sl) = self.split_str(&p);
                self.ensure_extern("declare void @jrg_str_join(ptr, i64, ptr, i64, ptr, ptr)", "jrg_str_join");
                let op = self.alloca_raw("joinp.", "ptr", Some(8));
                let ol = self.alloca_raw("joinl.", "i64", Some(8));
                self.emit(format!("call void @jrg_str_join(ptr {}, i64 {}, ptr {}, i64 {}, ptr {}, ptr {})",
                                  ptr, len, sp, sl, op, ol));
                let d = self.fresh("joind.");
                self.emit(format!("{} = load ptr, ptr {}", d, op));
                let n = self.fresh("joinn.");
                self.emit(format!("{} = load i64, ptr {}", n, ol));
                let x = self.fresh("joins.");
                self.emit(format!("{} = insertvalue %JrgStr undef, ptr {}, 0", x, d));
                let y = self.fresh("joins.");
                self.emit(format!("{} = insertvalue %JrgStr {}, i64 {}, 1", y, x, n));
                Ok(Val { text: y, ty: JrgType::Str })
            })());
        }
        if item == "is_empty" && args.len() == 1 {
            return Some((|| {
                let v = self.codegen_expr(&args[0], Some(&JrgType::Str))?;
                let n = self.panjang_koleksi(&v, span)?;
                let r = self.fresh("sempty.");
                self.emit(format!("{} = icmp eq i64 {}, 0", r, n.text));
                Ok(Val { text: self.from_i1(&r).text, ty: JrgType::Bool })
            })());
        }
        None
    }
}
