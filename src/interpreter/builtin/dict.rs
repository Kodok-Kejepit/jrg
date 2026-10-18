// src/interpreter/builtin/dict.rs — interpreter/builtin/dict.rs
// Dispatch untuk namespace `dict::` — peta key/value dinamis ala `dict`

use std::collections::HashMap;
use crate::interpreter::{Value, Signal, RuntimeError, EvalResult, Interpreter};
use crate::interpreter::{
    cek_dict_kurang, pesan_arg_harus_array, pesan_arg_harus_dict, pesan_bukan_pasangan,
    pesan_butuh_nilai, pesan_pasangan_bukan_dua,
};
use crate::runtime_err;
use super::NativeFnKind;

/// Nama tipe internal buat dict — dipakai supaya `Display`/`typeof` bisa
/// bedain dict dari struct biasa (lihat `dict::is_dict` & pesan error).
pub(crate) const DICT_TYPE_NAME: &str = "dict";

/// Nama `dict::` yang benar-benar berarti sebagai METHOD atas sebuah dict.
pub const METODE_DICT: &[&str] = &[
    "len", "is_empty", "keys", "values", "items", "has",
    "get", "get_or", "set", "remove", "merge",
];

pub(crate) fn is_dict(v: &Value) -> bool {
    matches!(v, Value::Struct { name, .. } if name == DICT_TYPE_NAME)
}

fn galat(pesan: String) -> Signal {
    Signal::RuntimeError(RuntimeError::new(pesan))
}

/// Isi dict dari argumen ke-`ke` (mulai 1); kalimat galatnya satu definisi
/// dengan VM.
fn as_dict_fields(v: &Value, fname: &str, ke: usize) -> Result<HashMap<String, Value>, Signal> {
    match v {
        // `(**fields)` menembus pembungkus `Bersama` sampai ke `HashMap`-nya.
        Value::Struct { name, fields } if name == DICT_TYPE_NAME => Ok((**fields).clone()),
        other => Err(galat(pesan_arg_harus_dict(fname, ke, &other.type_name()))),
    }
}

/// Isi dict untuk DIBACA saja — tanpa menyalin petanya (`as_dict_fields` menyalin semuanya).
fn pinjam_dict<'a>(v: &'a Value, fname: &str, ke: usize) -> Result<&'a HashMap<String, Value>, Signal> {
    match v {
        Value::Struct { name, fields } if name == DICT_TYPE_NAME => Ok(&**fields),
        other => Err(galat(pesan_arg_harus_dict(fname, ke, &other.type_name()))),
    }
}

/// Argumen yang kurang ditolak lebih dulu, dari tabel yang sama dengan VM.
fn cukup(args: &[Value], nama: &str) -> Result<(), Signal> {
    cek_dict_kurang(nama, args.len()).map_err(galat)
}

fn dict_dari(fields: HashMap<String, Value>) -> Value {
    Value::Struct { name: DICT_TYPE_NAME.to_string(), fields: fields.into() }
}

fn sorted_keys(fields: &HashMap<String, Value>) -> Vec<String> {
    let mut ks: Vec<String> = fields.keys().cloned().collect();
    ks.sort();
    ks
}

impl Interpreter {
    /// `dict::set` / `dict::remove` dengan argumen yang DIMILIKI: kalau dict-nya tinggal
    /// dipegang panggilan ini (`d = dict::set(d, k, v)` sesudah bindingnya dilepas), isinya
    /// diubah di tempat — `ambil` gratis untuk pemegang terakhir. Bentuk lain dikembalikan utuh.
    pub(super) fn dict_konsumsi(kind: &NativeFnKind, mut args: Vec<Value>) -> Result<EvalResult, Vec<Value>> {
        let cocok = match kind {
            NativeFnKind::DictSet => args.len() == 3,
            NativeFnKind::DictRemove => args.len() == 2,
            _ => false,
        };
        if !cocok || !is_dict(&args[0]) { return Err(args); }
        let Value::Struct { fields, .. } = std::mem::replace(&mut args[0], Value::Nil) else { unreachable!("sudah dipastikan dict") };
        let mut isi = fields.ambil();
        let kunci = format!("{}", args[1]);
        if matches!(kind, NativeFnKind::DictSet) {
            isi.insert(kunci, std::mem::replace(&mut args[2], Value::Nil));
        } else {
            isi.remove(&kunci);
        }
        Ok(Ok(dict_dari(isi)))
    }

    pub(super) fn dispatch_dict(&mut self, kind: &NativeFnKind, args: &[Value]) -> Option<EvalResult> {
        use NativeFnKind::*;
        if !matches!(kind,
            DictNew | DictFromPairs | DictSet | DictGet | DictGetOr | DictHas |
            DictRemove | DictKeys | DictValues | DictItems | DictLen | DictIsEmpty |
            DictMerge | DictIsDict
        ) {
            return None;
        }
        Some(self.dispatch_dict_inner(kind, args))
    }

    fn dispatch_dict_inner(&mut self, kind: &NativeFnKind, args: &[Value]) -> EvalResult {
        use NativeFnKind::*;
        match kind {
            DictNew => Ok(dict_dari(HashMap::new())),

            DictFromPairs => {
                cukup(args, "dict::from_pairs")?;
                let Value::Array(arr) = &args[0] else {
                    return runtime_err!("{}", pesan_arg_harus_array(
                        "dict::from_pairs", 1, &args[0].type_name()));
                };
                let mut fields = HashMap::new();
                for (i, pair) in arr.iter().enumerate() {
                    let (k, v) = match pair {
                        Value::Array(p) | Value::Tuple(p) if p.len() == 2 => (p[0].clone(), p[1].clone()),
                        Value::Array(p) | Value::Tuple(p) => return runtime_err!(
                            "{}", pesan_pasangan_bukan_dua("dict::from_pairs", i, p.len())),
                        o => return runtime_err!(
                            "{}", pesan_bukan_pasangan("dict::from_pairs", i, &o.type_name())),
                    };
                    fields.insert(format!("{}", k), v);
                }
                Ok(dict_dari(fields))
            }

            DictSet => {
                cukup(args, "dict::set")?;
                let mut fields = as_dict_fields(&args[0], "dict::set", 1)?;
                fields.insert(format!("{}", args[1]), args[2].clone());
                Ok(dict_dari(fields))
            }

            // Mengembalikan `Option`: `Some(nilai)` kalau key-nya ada, `None` kalau
            // tidak.
            DictGet => {
                cukup(args, "dict::get")?;
                let fields = pinjam_dict(&args[0], "dict::get", 1)?;
                Ok(match fields.get(&format!("{}", args[1])) {
                    Some(v) => crate::interpreter::some_value(v.clone()),
                    None    => crate::interpreter::none_value(),
                })
            }

            DictGetOr => {
                cukup(args, "dict::get_or")?;
                let fields = pinjam_dict(&args[0], "dict::get_or", 1)?;
                Ok(fields.get(&format!("{}", args[1])).cloned().unwrap_or_else(|| args[2].clone()))
            }

            DictHas => {
                cukup(args, "dict::has")?;
                let fields = pinjam_dict(&args[0], "dict::has", 1)?;
                Ok(Value::Bool(fields.contains_key(&format!("{}", args[1]))))
            }

            DictRemove => {
                cukup(args, "dict::remove")?;
                let mut fields = as_dict_fields(&args[0], "dict::remove", 1)?;
                fields.remove(&format!("{}", args[1]));
                Ok(dict_dari(fields))
            }

            DictKeys => {
                cukup(args, "dict::keys")?;
                let fields = pinjam_dict(&args[0], "dict::keys", 1)?;
                Ok(Value::Array(sorted_keys(&fields).into_iter().map(Value::Str).collect()))
            }

            DictValues => {
                cukup(args, "dict::values")?;
                let fields = pinjam_dict(&args[0], "dict::values", 1)?;
                Ok(Value::Array(sorted_keys(&fields).into_iter()
                    .map(|k| fields.get(&k).cloned().unwrap_or(Value::Nil)).collect()))
            }

            DictItems => {
                cukup(args, "dict::items")?;
                let fields = pinjam_dict(&args[0], "dict::items", 1)?;
                Ok(Value::Array(sorted_keys(&fields).into_iter()
                    .map(|k| {
                        let v = fields.get(&k).cloned().unwrap_or(Value::Nil);
                        Value::Tuple(vec![Value::Str(k), v].into())
                    }).collect()))
            }

            DictLen => {
                cukup(args, "dict::len")?;
                let fields = pinjam_dict(&args[0], "dict::len", 1)?;
                Ok(Value::Int(fields.len() as i128))
            }

            DictIsEmpty => {
                cukup(args, "dict::is_empty")?;
                let fields = pinjam_dict(&args[0], "dict::is_empty", 1)?;
                Ok(Value::Bool(fields.is_empty()))
            }

            // Predikat tanpa nilai ditolak, sama seperti `Option::is_some()`.
            DictIsDict => match args.first() {
                Some(v) => Ok(Value::Bool(is_dict(v))),
                None => runtime_err!("{}", pesan_butuh_nilai("dict::is_dict")),
            },

            DictMerge => {
                cukup(args, "dict::merge")?;
                let mut fields = as_dict_fields(&args[0], "dict::merge", 1)?;
                // Posisinya ikut disebut: dulu argumen KEDUA dituduh "argumen pertama".
                let other = as_dict_fields(&args[1], "dict::merge", 2)?;
                for (k, v) in other { fields.insert(k, v); }
                Ok(dict_dari(fields))
            }

            _ => unreachable!("dispatch_dict_inner dipanggil dengan kind di luar namespace dict::"),
        }
    }
}
