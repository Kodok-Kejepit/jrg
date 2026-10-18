// src/dok/json.rs — dok::json — JSON encode/decode untuk `interpreter::Value`

use std::collections::HashMap;

use crate::interpreter::{err_value, none_value, ok_value, some_value, Value};

pub const OBJ_NAME: &str = "JsonObject";

// ─── Encode ─────────────────────────────────────────────────────────────

pub fn json_encode(args: &[Value]) -> Result<Value, String> {
    let v = args.get(0).unwrap_or(&Value::Nil);
    Ok(Value::Str(encode(v)?))
}

/// Satu penyandi JSON untuk bahasa ini. Dulu ada salinan kedua di sini, dan
/// keduanya menjawab berbeda untuk enum, `Option`/`Result`, NaN/inf dan
/// fungsi — teks yang menetap di data program lain (Fase JSON-SATU).
pub fn encode(v: &Value) -> Result<String, String> {
    // Jembatan nilai menolak beberapa bentuk dengan alasan STRUKTURALNYA
    // ("tidak punya padanan di VM"). Yang ditanyakan pemakainya JSON, jadi
    // kalimatnya tetap kalimat JSON.
    let vm = crate::vm::ffi_jembatan::nilai_ke_vm(v).map_err(|_| {
        // Kata yang sama dengan penyandinya: di sana semua yang bisa dipanggil
        // disebut `fungsi`, jadi di sini tidak boleh jadi `closure`.
        let v = penyebab(v);
        let apa = if v.bisa_dipanggil() { "fungsi".to_string() } else { v.type_name() };
        crate::interpreter::galat_index::tidak_bisa_jadi_json(
            &apa, "kirim nilai biasa — angka, teks, array, dict, struct atau enum")
    })?;
    crate::stdlib::json::stringify_pub(&vm)
}

/// Isi terdalam yang tidak bisa menyeberang: yang disebut kalimatnya, bukan wadahnya.
fn penyebab(v: &Value) -> &Value {
    let gagal = |x: &&Value| crate::vm::ffi_jembatan::nilai_ke_vm(x).is_err();
    let anak = match v {
        Value::Array(a) | Value::Tuple(a) => a.iter().find(gagal),
        Value::Struct { fields, .. } => fields.values().find(gagal),
        Value::Enum { payload: Some(p), .. } => Some(&**p).filter(gagal),
        _ => None,
    };
    anak.map(penyebab).unwrap_or(v)
}

fn escape(s: &str) -> String { crate::json_inti::sandi_teks(s) }

pub fn json_decode(args: &[Value]) -> Result<Value, String> {
    let s = match args.get(0) {
        Some(Value::Str(s)) => s.as_str(),
        other => return Err(format!("dok::json_decode: butuh string, dapat `{}`",
            other.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada argumen".to_string()))),
    };
    match decode(s) {
        Ok(v) => Ok(ok_value(v)),
        Err(e) => Ok(err_value(format!("dok::json_decode: {}", e))),
    }
}

/// Urai lewat pengurai bersama (`json_inti`) — yang sama dengan `json::parse`.
pub fn decode(s: &str) -> Result<Value, String> {
    fn ke_nilai(j: crate::json_inti::Json) -> Value {
        use crate::json_inti::Json;
        match j {
            Json::Null => Value::Nil,
            Json::Bool(b) => Value::Bool(b),
            Json::Int(n) => Value::Int(n),
            Json::Float(f) => Value::Float(f),
            Json::Str(t) => Value::Str(t),
            Json::Arr(a) => Value::Array(a.into_iter().map(ke_nilai).collect::<Vec<_>>().into()),
            Json::Obj(o) => {
                let mut fields = HashMap::new();
                for (k, v) in o { fields.insert(k, ke_nilai(v)); }
                Value::Struct { name: OBJ_NAME.to_string(), fields: fields.into() }
            }
        }
    }
    crate::json_inti::urai(s).map(ke_nilai)
}

// ─── Accessor utilities ───────────────────────────────────────────────

pub fn json_get(args: &[Value]) -> Result<Value, String> {
    match (args.get(0), args.get(1)) {
        (Some(Value::Struct { fields, .. }), Some(Value::Str(k))) =>
            Ok(fields.get(k.as_str()).cloned().map(some_value).unwrap_or_else(none_value)),
        _ => Err("dok::json_get(obj, key)".to_string()),
    }
}

pub fn json_set(args: &[Value]) -> Result<Value, String> {
    match (args.get(0), args.get(1), args.get(2)) {
        // Handle pustaka bukan objek JSON: menambah field ke sana dulu diterima diam-diam.
        (Some(Value::Struct { name, .. }), Some(Value::Str(k)), Some(_))
            if crate::interpreter::galat_index::HANDLE_PUSTAKA.contains(&name.as_str()) =>
            Err(crate::interpreter::galat_index::tulis_handle(name, k)),
        (Some(Value::Struct { name, fields }), Some(Value::Str(k)), Some(v)) => {
            let mut new_fields = fields.clone();
            new_fields.insert(k.clone(), v.clone());
            Ok(Value::Struct { name: name.clone(), fields: new_fields })
        }
        _ => Err("dok::json_set(obj, key, val)".to_string()),
    }
}

pub fn json_keys(args: &[Value]) -> Result<Value, String> {
    match args.get(0) {
        Some(Value::Struct { fields, .. }) => {
            let mut ks: Vec<String> = fields.keys().cloned().collect();
            ks.sort();
            Ok(Value::Array(ks.into_iter().map(Value::Str).collect()))
        }
        _ => Err("dok::json_keys(obj)".to_string()),
    }
}
