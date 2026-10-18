// src/interpreter/builtin/php.rs — pintu `php::` di interpreter.
// Tiap lengan memeriksa TIPE argumennya lalu menyerahkan ke `ffi::php`;
// jumlahnya sudah diperiksa pintu native (`interpreter::arity`).

use crate::ffi::php::{self, PhpRef};
use crate::interpreter::{
    pesan_arg_harus_array, pesan_arg_harus_dict, pesan_arg_harus_teks,
    EvalResult, RuntimeError, Signal, Value,
};
use crate::ffi::pekerja::Panggil;
use super::NativeFnKind;

fn galat(pesan: String) -> Signal { Signal::RuntimeError(RuntimeError::new(pesan)) }

fn teks(args: &[Value], i: usize, fname: &str, peran: &str) -> Result<String, Signal> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(s.clone()),
        lain => Err(galat(pesan_arg_harus_teks(
            fname, &format!("argumen ke-{} ({})", i + 1, peran),
            &lain.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into())))),
    }
}

fn handle(args: &[Value], i: usize, fname: &str) -> Result<PhpRef, Signal> {
    match args.get(i) {
        Some(Value::PhpObj(h)) => Ok(h.clone()),
        Some(lain) => Err(galat(php::pesan_bukan_handle(fname, i + 1, lain))),
        None => Err(galat(format!("{}: butuh handle PHP di argumen {}", fname, i + 1))),
    }
}

fn daftar(args: &[Value], i: usize, fname: &str) -> Result<Vec<Value>, Signal> {
    match args.get(i) {
        Some(Value::Array(a)) => Ok(a.iter().cloned().collect()),
        lain => Err(galat(pesan_arg_harus_array(
            fname, i + 1, &lain.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into())))),
    }
}

/// Dict — atau `None` kalau argumennya memang tidak ditulis.
fn dict_opsional(args: &[Value], i: usize, fname: &str) -> Result<Option<Value>, Signal> {
    match args.get(i) {
        None => Ok(None),
        Some(v @ Value::Struct { name, .. }) if name == "dict" => Ok(Some(v.clone())),
        Some(lain) => Err(galat(pesan_arg_harus_dict(fname, i + 1, &lain.type_name()))),
    }
}

/// Fungsi yang dipanggil `php::call`: nama, atau handle ke sesuatu yang bisa dipanggil.
fn callable(args: &[Value], fname: &str) -> Result<Value, Signal> {
    match args.first() {
        Some(v @ (Value::Str(_) | Value::PhpObj(_))) => Ok(v.clone()),
        lain => Err(galat(format!(
            "{}: argumen ke-1 harus nama fungsi PHP (string, mis. \"strlen\" atau \
             \"Kelas::statik\") atau handle PHP yang bisa dipanggil, dapat `{}`",
            fname, lain.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into())))),
    }
}

/// Pintu `php::`, bebas mesin: `p` satu-satunya jalan worker PHP menjalankan kode JRG.
pub(super) fn dispatch_php(kind: &NativeFnKind, args: &[Value], p: Panggil) -> Option<EvalResult> {
    {
        use NativeFnKind::*;
        if !matches!(kind,
            PhpEval | PhpExec | PhpRef | PhpCall | PhpRefCall | PhpCallKw | PhpNew | PhpNewKw |
            PhpMethod | PhpRefMethod | PhpMethodKw | PhpGet | PhpRefGet | PhpSet |
            PhpItem | PhpRefItem | PhpSetItem | PhpDelItem | PhpHas | PhpLen | PhpDir |
            PhpRepr | PhpToJrg | PhpRefs | PhpRequire | PhpRender | PhpRenderStr | PhpTersedia
        ) {
            return None;
        }
        Some(dispatch_php_inner(kind, args, p))
    }
}

fn dispatch_php_inner(kind: &NativeFnKind, args: &[Value], p: Panggil) -> EvalResult {
    {
        use NativeFnKind::*;
        let sisa = |dari: usize| args.iter().skip(dari).cloned().collect::<Vec<_>>();
        Ok(match kind {
            PhpEval => php::php_eval(&teks(args, 0, "php::eval", "kode")?, false, p),
            PhpRef => php::php_eval(&teks(args, 0, "php::ref", "kode")?, true, p),
            PhpExec => php::php_exec(&teks(args, 0, "php::exec", "kode")?, p),
            PhpCall => php::php_call(&callable(args, "php::call")?, &sisa(1), None, false, p),
            PhpRefCall => php::php_call(&callable(args, "php::ref_call")?, &sisa(1), None, true, p),
            PhpCallKw => {
                let f = callable(args, "php::call_kw")?;
                let a = daftar(args, 1, "php::call_kw")?;
                let kw = dict_opsional(args, 2, "php::call_kw")?;
                php::php_call(&f, &a, kw.as_ref(), false, p)
            }
            PhpNew => php::php_new(&teks(args, 0, "php::new", "nama kelas")?, &sisa(1), None, p),
            PhpNewKw => {
                let k = teks(args, 0, "php::new_kw", "nama kelas")?;
                let a = daftar(args, 1, "php::new_kw")?;
                let kw = dict_opsional(args, 2, "php::new_kw")?;
                php::php_new(&k, &a, kw.as_ref(), p)
            }
            PhpMethod | PhpRefMethod => {
                let fname = if *kind == PhpMethod { "php::method" } else { "php::ref_method" };
                let h = handle(args, 0, fname)?;
                let n = teks(args, 1, fname, "nama method")?;
                php::php_method(&h, &n, &sisa(2), None, *kind == PhpRefMethod, p)
            }
            PhpMethodKw => {
                let h = handle(args, 0, "php::method_kw")?;
                let n = teks(args, 1, "php::method_kw", "nama method")?;
                let a = daftar(args, 2, "php::method_kw")?;
                let kw = dict_opsional(args, 3, "php::method_kw")?;
                php::php_method(&h, &n, &a, kw.as_ref(), false, p)
            }
            PhpGet | PhpRefGet => {
                let fname = if *kind == PhpGet { "php::get" } else { "php::ref_get" };
                let h = handle(args, 0, fname)?;
                php::php_get(&h, &teks(args, 1, fname, "nama properti")?, *kind == PhpRefGet, p)
            }
            PhpSet => {
                let h = handle(args, 0, "php::set")?;
                let n = teks(args, 1, "php::set", "nama properti")?;
                php::php_set(&h, &n, &args[2], p)
            }
            PhpItem | PhpRefItem => {
                let fname = if *kind == PhpItem { "php::item" } else { "php::ref_item" };
                let h = handle(args, 0, fname)?;
                php::php_kunci(&h, "item", fname, &args[1], None, *kind == PhpRefItem, p)
            }
            PhpSetItem => {
                let h = handle(args, 0, "php::set_item")?;
                php::php_kunci(&h, "setitem", "php::set_item", &args[1], Some(&args[2]), false, p)
            }
            PhpDelItem => {
                let h = handle(args, 0, "php::del_item")?;
                php::php_kunci(&h, "delitem", "php::del_item", &args[1], None, false, p)
            }
            PhpHas => {
                let h = handle(args, 0, "php::has")?;
                php::php_kunci(&h, "contains", "php::has", &args[1], None, false, p)
            }
            PhpLen => php::php_op_handle(&handle(args, 0, "php::len")?, "len", "php::len", p),
            PhpDir => php::php_op_handle(&handle(args, 0, "php::dir")?, "dir", "php::dir", p),
            PhpRepr => php::php_op_handle(&handle(args, 0, "php::repr")?, "repr", "php::repr", p),
            PhpToJrg => php::php_op_handle(&handle(args, 0, "php::to_jrg")?, "to_jrg", "php::to_jrg", p),
            PhpRefs => php::php_refs(p),
            PhpRequire => php::php_require(&teks(args, 0, "php::require", "jalur berkas")?, p),
            PhpRender => {
                let jalur = teks(args, 0, "php::render", "jalur berkas")?;
                let data = dict_opsional(args, 1, "php::render")?;
                php::php_render(&jalur, true, data.as_ref(), p)
            }
            PhpRenderStr => {
                let tpl = teks(args, 0, "php::render_str", "templat")?;
                let data = dict_opsional(args, 1, "php::render_str")?;
                php::php_render(&tpl, false, data.as_ref(), p)
            }
            PhpTersedia => php::php_tersedia(p),
            _ => unreachable!("dispatch_php menyaring jenisnya di atas"),
        })
    }
}
