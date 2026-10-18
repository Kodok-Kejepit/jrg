// src/interpreter/builtin/ffi_pintu.rs — pintu `py::`/`js::`/`php::`/`lua::`, bebas mesin.
// Tiap lengan memeriksa TIPE argumennya lalu menyerahkan ke `ffi::*`; satu-satunya
// yang dibutuhkan dari mesin pemanggil adalah `panggil` (callback ke fungsi JRG).

pub use crate::ffi::pekerja::Panggil;

/// Namespace yang dilayani pintu ini.
pub const NAMESPACE_FFI: &[&str] = &["py::", "js::", "php::", "lua::"];
use crate::interpreter::{EvalResult, RuntimeError, Signal, Value};
use crate::runtime_err;
use super::NativeFnKind;

fn jsobj_arg(args: &[Value], n: usize, fname: &str) -> Result<crate::ffi::javascript::JsRef, Signal> {
    match args.get(n) {
        Some(Value::JsObj(h)) => Ok(h.clone()),
        Some(lain) => Err(Signal::RuntimeError(RuntimeError::new(format!(
            "{}: argumen {} harus handle JavaScript (hasil `js::ref(...)`, atau nilai \
             balik `js::` yang memang tidak bisa dikonversi), dapat `{}`",
            fname, n + 1, lain.type_name())))),
        None => Err(Signal::RuntimeError(RuntimeError::new(format!(
            "{}: butuh handle JavaScript di argumen {}, tapi argumennya tidak ada",
            fname, n + 1)))),
    }
}

fn pyobj_arg(args: &[Value], n: usize, fname: &str) -> Result<crate::ffi::python::PyRef, Signal> {
    match args.get(n) {
        Some(Value::PyObj(h)) => Ok(h.clone()),
        Some(lain) => Err(Signal::RuntimeError(RuntimeError::new(format!(
            "{}: argumen {} harus handle Python (hasil `py::ref(...)`, atau nilai \
             balik `py::` yang memang tidak bisa dikonversi), dapat `{}`",
            fname, n + 1, lain.type_name())))),
        None => Err(Signal::RuntimeError(RuntimeError::new(format!(
            "{}: butuh handle Python di argumen {}, tapi argumennya tidak ada",
            fname, n + 1)))),
    }
}

/// Kegagalan callback seperti yang dilihat worker: kalimatnya saja, sama di dua mesin.
pub fn pesan_sinyal(s: Signal) -> String {
    match s {
        Signal::RuntimeError(r) => r.message,
        lain => format!("{}", lain),
    }
}

/// Method langsung di atas handle asing (`df.head(5)`, `tgl.format("Y-m-d")`);
/// `None` kalau penerimanya bukan handle asing.
pub fn metode_handle(obj: &Value, method: &str, args: Vec<Value>, panggil: Panggil) -> Option<Value> {
    Some(match obj {
        Value::PyObj(h) => crate::ffi::python::py_method(h, method, args, panggil),
        Value::JsObj(h) => crate::ffi::javascript::js_method(h, method, args, panggil),
        Value::PhpObj(h) => crate::ffi::php::php_method(h, method, &args, None, false, panggil),
        _ => return None,
    })
}

/// Layani satu builtin FFI; `None` kalau `kind` bukan milik keempat namespace ini.
pub fn pintu_ffi(kind: &NativeFnKind, args: &[Value], panggil: Panggil) -> Option<EvalResult> {
    if let Some(r) = super::php::dispatch_php(kind, args, &mut *panggil) { return Some(r); }
    if let Some(r) = super::lua::dispatch_lua(kind, args, &mut *panggil) { return Some(r); }
    py_js(kind, args, panggil).transpose()
}

fn py_js(kind: &NativeFnKind, args: &[Value], panggil: Panggil) -> Result<Option<Value>, Signal> {
    use NativeFnKind::*;
    fn fst(args: &[Value]) -> Value { args.first().cloned().unwrap_or(Value::Nil) }
    let hasil: EvalResult = match kind {
        JsEval => {
            let code = match fst(&args) {
                Value::Str(s) => s,
                other => return runtime_err!("js::eval: argumen harus string, dapat `{}`", other.type_name()),
            };
            Ok(crate::ffi::javascript::js_eval(&code))
        }
        JsExec => {
            let code = match fst(&args) {
                Value::Str(s) => s,
                other => return runtime_err!("js::exec: argumen harus string, dapat `{}`", other.type_name()),
            };
            Ok(crate::ffi::javascript::js_exec(&code))
        }
        JsImport => {
            let code = match fst(&args) {
                Value::Str(s) => s,
                other => return runtime_err!("js::import: argumen harus string, dapat `{}`", other.type_name()),
            };
            Ok(crate::ffi::javascript::js_import(&code))
        }
        JsRefE => {
            let code = match fst(&args) {
                Value::Str(s) => s,
                other => return runtime_err!("js::ref: argumen harus string, dapat `{}`", other.type_name()),
            };
            Ok(crate::ffi::javascript::js_ref(&code))
        }
        JsCall => {
            let m = match fst(&args) {
                Value::Str(s) => s,
                other => return runtime_err!("js::call: argumen 1 (modul) harus string, dapat `{}`", other.type_name()),
            };
            let f = match args.get(1) {
                Some(Value::Str(s)) => s.clone(),
                other => return runtime_err!(
                    "js::call: argumen 2 (nama fungsi) harus string, dapat `{}`",
                    other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into())),
            };
            let sisa: Vec<Value> = args.iter().skip(2).cloned().collect();
            Ok(crate::ffi::javascript::js_call_opt(&m, &f, sisa, false, &mut *panggil))
        }
        JsRefCall => {
            let m = match fst(&args) {
                Value::Str(s) => s,
                other => return runtime_err!("js::ref_call: argumen 1 (modul) harus string, dapat `{}`", other.type_name()),
            };
            let f = match args.get(1) {
                Some(Value::Str(s)) => s.clone(),
                other => return runtime_err!(
                    "js::ref_call: argumen 2 (nama fungsi) harus string, dapat `{}`",
                    other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into())),
            };
            let sisa: Vec<Value> = args.iter().skip(2).cloned().collect();
            Ok(crate::ffi::javascript::js_call_opt(&m, &f, sisa, true, &mut *panggil))
        }
        JsToJrg => {
            let h = jsobj_arg(&args, 0, "js::to_jrg")?;
            Ok(crate::ffi::javascript::js_to_jrg(&h))
        }
        JsRepr => {
            let h = jsobj_arg(&args, 0, "js::repr")?;
            Ok(crate::ffi::javascript::js_repr(&h))
        }
        JsLen => {
            let h = jsobj_arg(&args, 0, "js::len")?;
            Ok(crate::ffi::javascript::js_len(&h))
        }
        JsDir => {
            let h = jsobj_arg(&args, 0, "js::dir")?;
            Ok(crate::ffi::javascript::js_dir(&h))
        }
        JsGet => {
            let h = jsobj_arg(&args, 0, "js::get")?;
            let n = match args.get(1) {
                Some(Value::Str(s)) => s.clone(),
                other => return runtime_err!(
                    "js::get: argumen 2 harus string, dapat `{}`",
                    other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into())),
            };
            Ok(crate::ffi::javascript::js_get(&h, &n))
        }
        JsRefGet => {
            let h = jsobj_arg(&args, 0, "js::ref_get")?;
            let n = match args.get(1) {
                Some(Value::Str(s)) => s.clone(),
                other => return runtime_err!(
                    "js::ref_get: argumen 2 harus string, dapat `{}`",
                    other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into())),
            };
            Ok(crate::ffi::javascript::js_ref_get(&h, &n))
        }
        JsMethod => {
            let h = jsobj_arg(&args, 0, "js::method")?;
            let n = match args.get(1) {
                Some(Value::Str(s)) => s.clone(),
                other => return runtime_err!(
                    "js::method: argumen 2 (nama method) harus string, dapat `{}`",
                    other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into())),
            };
            let sisa: Vec<Value> = args.iter().skip(2).cloned().collect();
            Ok(crate::ffi::javascript::js_method(&h, &n, sisa, &mut *panggil))
        }
        JsRefMethod => {
            let h = jsobj_arg(&args, 0, "js::ref_method")?;
            let n = match args.get(1) {
                Some(Value::Str(s)) => s.clone(),
                other => return runtime_err!(
                    "js::ref_method: argumen 2 (nama method) harus string, dapat `{}`",
                    other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into())),
            };
            let sisa: Vec<Value> = args.iter().skip(2).cloned().collect();
            Ok(crate::ffi::javascript::js_ref_method(&h, &n, sisa, &mut *panggil))
        }
        JsItem => {
            let h = jsobj_arg(&args, 0, "js::item")?;
            let k = args.get(1).cloned().unwrap_or(Value::Nil);
            Ok(crate::ffi::javascript::js_item(&h, &k))
        }
        JsRefItem => {
            let h = jsobj_arg(&args, 0, "js::ref_item")?;
            let k = args.get(1).cloned().unwrap_or(Value::Nil);
            Ok(crate::ffi::javascript::js_ref_item(&h, &k))
        }
        JsDelItem => {
            let h = jsobj_arg(&args, 0, "js::del_item")?;
            let k = args.get(1).cloned().unwrap_or(Value::Nil);
            Ok(crate::ffi::javascript::js_del_item(&h, &k))
        }
        JsHas => {
            let h = jsobj_arg(&args, 0, "js::has")?;
            let k = args.get(1).cloned().unwrap_or(Value::Nil);
            Ok(crate::ffi::javascript::js_has(&h, &k))
        }
        JsRefs => Ok(crate::ffi::javascript::js_refs()),
        JsSet => {
            let h = jsobj_arg(&args, 0, "js::set")?;
            let n = match args.get(1) {
                Some(Value::Str(s)) => s.clone(),
                other => return runtime_err!(
                    "js::set: argumen 2 harus string, dapat `{}`",
                    other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into())),
            };
            let v = args.get(2).cloned().unwrap_or(Value::Nil);
            Ok(crate::ffi::javascript::js_set(&h, &n, &v))
        }
        JsSetItem => {
            let h = jsobj_arg(&args, 0, "js::set_item")?;
            let k = args.get(1).cloned().unwrap_or(Value::Nil);
            let v = args.get(2).cloned().unwrap_or(Value::Nil);
            Ok(crate::ffi::javascript::js_set_item(&h, &k, &v))
        }

        PyEval => {
            let code = match fst(&args) {
                Value::Str(s) => s,
                other => return runtime_err!("py::eval: argumen harus string, dapat `{}`", other.type_name()),
            };
            Ok(crate::ffi::python::py_eval(&code, &mut *panggil))
        }

        // ── Operasi di atas HANDLE ───────────────────────────────

        // `py::ref(code)` — seperti `py::eval`, tapi hasilnya SELALU
        // tinggal di Python. Dipakai untuk data besar yang tidak
        // perlu menyeberang.
        PyRef => {
            let code = match fst(&args) {
                Value::Str(s) => s,
                other => return runtime_err!("py::ref: argumen harus string, dapat `{}`", other.type_name()),
            };
            Ok(crate::ffi::python::py_ref(&code, &mut *panggil))
        }
        // `py::get(h, "nama")` — baca atribut objeknya.
        PyGet => {
            let h = pyobj_arg(&args, 0, "py::get")?;
            let nama = match args.get(1) {
                Some(Value::Str(s)) => s.clone(),
                other => return runtime_err!(
                    "py::get: argumen 2 (nama atribut) harus string, dapat `{}`",
                    other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into())),
            };
            Ok(crate::ffi::python::py_getattr(&h, &nama, &mut *panggil))
        }
        // `py::method(h, "nama", ...args)` — panggil methodnya.
        PyMethod => {
            let h = pyobj_arg(&args, 0, "py::method")?;
            let nama = match args.get(1) {
                Some(Value::Str(s)) => s.clone(),
                other => return runtime_err!(
                    "py::method: argumen 2 (nama method) harus string, dapat `{}`",
                    other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into())),
            };
            Ok(crate::ffi::python::py_method_cb(&h, &nama, args[2..].to_vec(), Vec::new(), false, &mut *panggil))
        }

        // Keyword argument pada metode sebuah handle.
        PyMethodKw | PyRefMethodKw => {
            let tetap = matches!(kind, PyRefMethodKw);
            let siapa = if tetap { "py::ref_method_kw" } else { "py::method_kw" };
            let h = pyobj_arg(&args, 0, siapa)?;
            let nama = match args.get(1) {
                Some(Value::Str(s)) => s.clone(),
                other => return runtime_err!(
                    "{}: argumen 2 (nama method) harus string, dapat `{}`", siapa,
                    other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into())),
            };
            // Bentuknya `(h, nama, [args], [[k, v], ...])` — sama persis
            // dengan `py::call_kw`, supaya yang sudah hafal satu tidak
            // perlu menghafal yang lain.
            let posisi = match args.get(2) {
                Some(Value::Array(a)) => a.to_vec(),
                None => Vec::new(),
                other => return runtime_err!(
                    "{}: argumen 3 harus array argumen posisi, dapat `{}`", siapa,
                    other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into())),
            };
            let kw = match args.get(3) {
                Some(Value::Array(a)) => a.to_vec(),
                None => Vec::new(),
                other => return runtime_err!(
                    "{}: argumen 4 harus array pasangan [nama, nilai], dapat `{}`", siapa,
                    other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into())),
            };
            Ok(crate::ffi::python::py_method_cb(&h, &nama, posisi, kw, tetap, &mut *panggil))
        }
        PyLen => {
            let h = pyobj_arg(&args, 0, "py::len")?;
            Ok(crate::ffi::python::py_len(&h, &mut *panggil))
        }
        PyDir => {
            let h = pyobj_arg(&args, 0, "py::dir")?;
            Ok(crate::ffi::python::py_dir(&h, &mut *panggil))
        }
        PySet => {
            let h = pyobj_arg(&args, 0, "py::set")?;
            let nama = match args.get(1) {
                Some(Value::Str(s)) => s.clone(),
                other => return runtime_err!(
                    "py::set: argumen 2 (nama atribut) harus string, dapat `{}`",
                    other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into())),
            };
            let nilai = args.get(2).cloned().unwrap_or(Value::Nil);
            Ok(crate::ffi::python::py_setattr(&h, &nama, &nilai, &mut *panggil))
        }
        PySetItem => {
            let h = pyobj_arg(&args, 0, "py::set_item")?;
            let k = args.get(1).cloned().unwrap_or(Value::Nil);
            let v = args.get(2).cloned().unwrap_or(Value::Nil);
            Ok(crate::ffi::python::py_setitem(&h, &k, &v, &mut *panggil))
        }
        PyDelItem => {
            let h = pyobj_arg(&args, 0, "py::del_item")?;
            let k = args.get(1).cloned().unwrap_or(Value::Nil);
            Ok(crate::ffi::python::py_delitem(&h, &k, &mut *panggil))
        }
        PyHas => {
            let h = pyobj_arg(&args, 0, "py::has")?;
            let k = args.get(1).cloned().unwrap_or(Value::Nil);
            Ok(crate::ffi::python::py_contains(&h, &k, &mut *panggil))
        }

        // Varian "pertahankan handle" dari getattr/method/item/call.
        PyRefGet => {
            let h = pyobj_arg(&args, 0, "py::ref_get")?;
            let nama = match args.get(1) {
                Some(Value::Str(s)) => s.clone(),
                other => return runtime_err!(
                    "py::ref_get: argumen 2 (nama atribut) harus string, dapat `{}`",
                    other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into())),
            };
            Ok(crate::ffi::python::py_getattr_opt(&h, &nama, true, &mut *panggil))
        }
        PyRefMethod => {
            let h = pyobj_arg(&args, 0, "py::ref_method")?;
            let nama = match args.get(1) {
                Some(Value::Str(s)) => s.clone(),
                other => return runtime_err!(
                    "py::ref_method: argumen 2 (nama method) harus string, dapat `{}`",
                    other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into())),
            };
            Ok(crate::ffi::python::py_method_cb(&h, &nama, args[2..].to_vec(), Vec::new(), true, &mut *panggil))
        }
        PyRefItem => {
            let h = pyobj_arg(&args, 0, "py::ref_item")?;
            let kunci = args.get(1).cloned().unwrap_or(Value::Nil);
            Ok(crate::ffi::python::py_item_opt(&h, &kunci, true, &mut *panggil))
        }
        PyRefCall => {
            let modul = match fst(&args) {
                Value::Str(s) => s,
                other => return runtime_err!("py::ref_call: argumen 1 (modul) harus string, dapat `{}`", other.type_name()),
            };
            let f = match args.get(1) {
                Some(Value::Str(s)) => s.clone(),
                other => return runtime_err!(
                    "py::ref_call: argumen 2 (fungsi) harus string, dapat `{}`",
                    other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into())),
            };
            Ok(crate::ffi::python::py_call_cb(&modul, &f, args[2..].to_vec(), true, &mut *panggil))
        }
        // `py::item(h, kunci)` — `h[kunci]`.
        PyItem => {
            let h = pyobj_arg(&args, 0, "py::item")?;
            let kunci = args.get(1).cloned().unwrap_or(Value::Nil);
            Ok(crate::ffi::python::py_item(&h, &kunci, &mut *panggil))
        }
        // `py::to_jrg(h)` — paksa objeknya menyeberang jadi nilai JRG.
        PyToJrg => {
            let h = pyobj_arg(&args, 0, "py::to_jrg")?;
            Ok(crate::ffi::python::py_to_jrg(&h, &mut *panggil))
        }
        // `py::repr(h)` — bentuk `repr()` Python-nya sebagai string.
        PyRepr => {
            let h = pyobj_arg(&args, 0, "py::repr")?;
            Ok(crate::ffi::python::py_repr(&h, &mut *panggil))
        }
        // `py::refs()` — berapa handle yang masih hidup di worker.
        // Membuat janji pembebasan otomatis bisa DIAMATI, bukan cuma
        // dipercaya.
        PyRefs => {
            Ok(crate::ffi::python::py_refs(&mut *panggil))
        }
        // `py::exec(code)` — runs Python statement(s) for side
        // effects (assignment, import, def, loops, ...).
        PyExec => {
            let code = match fst(&args) {
                Value::Str(s) => s,
                other => return runtime_err!("py::exec: argumen harus string, dapat `{}`", other.type_name()),
            };
            Ok(crate::ffi::python::py_exec(&code, &mut *panggil))
        }
        // `py::import(module)` — pre-imports a module so a missing package fails
        // right here instead of inside some later, unrelated `py::call`.
        PyImport => {
            let module = match fst(&args) {
                Value::Str(s) => s,
                other => return runtime_err!("py::import: argumen harus string, dapat `{}`", other.type_name()),
            };
            Ok(crate::ffi::python::py_import(&module, &mut *panggil))
        }
        // `py::call(module, func_path, arg1, arg2, ...)` — imports `module`,
        // resolves `func_path` as a dot-separated attribute path off of it (so …
        PyCall => {
            if args.len() < 2 {
                return runtime_err!("py::call butuh minimal (modul, fungsi, ...args)");
            }
            let module = match &args[0] {
                Value::Str(s) => s.clone(),
                other => return runtime_err!("py::call: argumen 1 (modul) harus string, dapat `{}`", other.type_name()),
            };
            let func = match &args[1] {
                Value::Str(s) => s.clone(),
                other => return runtime_err!("py::call: argumen 2 (fungsi) harus string, dapat `{}`", other.type_name()),
            };
            let rest = args[2..].to_vec();
            // Kemampuan MEMANGGIL kode JRG diserahkan ke lapisan FFI di sini, karena
            // lapisan itu tidak punya akses ke interpreter.
            Ok(crate::ffi::python::py_call_cb(&module, &func, rest, false, &mut *panggil))
        }
        // `py::call_kw(module, func_path, args_array, kwargs_pairs)` — like
        // `py::call`, but for Python calls that need keyword arguments …
        PyCallKw => {
            if args.len() != 4 {
                return runtime_err!(
                    "py::call_kw butuh persis 4 argumen: (modul, fungsi, args_array, kwargs_pairs), dapat {}",
                    args.len()
                );
            }
            let module = match &args[0] {
                Value::Str(s) => s.clone(),
                other => return runtime_err!("py::call_kw: argumen 1 (modul) harus string, dapat `{}`", other.type_name()),
            };
            let func = match &args[1] {
                Value::Str(s) => s.clone(),
                other => return runtime_err!("py::call_kw: argumen 2 (fungsi) harus string, dapat `{}`", other.type_name()),
            };
            let pos_args = match &args[2] {
                Value::Array(a) | Value::Tuple(a) => a.to_vec(),
                other => return runtime_err!(
                    "py::call_kw: argumen 3 (args) harus array, dapat `{}`", other
                ),
            };
            let kwarg_pairs = match &args[3] {
                Value::Array(a) | Value::Tuple(a) => a.to_vec(),
                other => return runtime_err!(
                    "py::call_kw: argumen 4 (kwargs) harus array pasangan [nama, nilai], dapat `{}`", other
                ),
            };
            Ok(crate::ffi::python::py_call_kw_cb(&module, &func, pos_args, kwarg_pairs, false, &mut *panggil))
        }
        _ => return Ok(None),
    };
    hasil.map(Some)
}
