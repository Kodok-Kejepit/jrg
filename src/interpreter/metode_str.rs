// src/interpreter/metode_str.rs — method bawaan untuk string

use std::collections::HashMap;

use crate::parser::{
    AssignOp, BinaryOp, Block, BreakStmt, ContinueStmt, EnumDecl, EnumVariantKind,
    Expr, ExternBlock, ExternItem, ExternParam, FnBody,
    ForStmt, IfStmt, ImplItem, Item, LetStmt, LitKind, LoopStmt,
    MatchArm, MatchBody, MatchStmt, NamedType, OwnershipQual, ParsedFile, Pattern, PrimTy,
    ReturnStmt, SelectCase, SelectStmt, ShortDeclStmt, SpawnStmt, Stmt, TemplatePart, TypeExpr,
    UnaryOp, UsePath, VarStmt, WhileStmt,
    StructDecl, StructField, TraitDecl, TraitItem, Visibility,
};

use crate::interpreter::{
    Value, Env, Signal, RuntimeError, EvalResult, Interpreter, JrgFunction,
    ok_value, err_value, some_value, none_value,
    ChannelHandle, channel_new, channel_send, channel_recv,
    channel_try_send, channel_try_recv,
    enum_payload, enum_parts,
};
use crate::interpreter::builtin::{NativeFnKind, fisher_yates_shuffle};
use crate::runtime_err;
use crate::interpreter::eval_expr::{nama_tipe_sasaran, nilai_ke_usize, nilai_ke_i128, nilai_ke_f64};

impl Interpreter {
    /// Method bawaan untuk string, satu-satunya definisinya.
    pub fn metode_str(s: &str, method: &str, args: &[Value]) -> Option<EvalResult> {
        // `?` tidak bisa dipakai di sini: fungsinya mengembalikan `Option` sementara
        // galatnya `Result`.
        macro_rules! bawa {
            ($e:expr) => { match $e { Ok(v) => v, Err(e) => return Some(Err(e)) } };
        }
        let s = &s.to_string();
        Some(match (s, method) {
            // -----------------------------------------------------------
            // String built-in methods
            // -----------------------------------------------------------
            (s, "len")          => Ok(Value::Int(s.chars().count() as i128)),
            (s, "byte_len")     => Ok(Value::Int(s.len() as i128)),
            (s, "is_empty")     => Ok(Value::Bool(s.is_empty())),
            (s, "to_uppercase") => Ok(Value::Str(s.to_uppercase())),
            (s, "to_lowercase") => Ok(Value::Str(s.to_lowercase())),
            // `upper`/`lower` adalah ejaan pendek yang SUDAH ada sebagai
            // `str::upper`/`str::lower`, dan sudah ada sebagai method di VM — yang tidak …
            (s, "upper")        => Ok(Value::Str(s.to_uppercase())),
            (s, "lower")        => Ok(Value::Str(s.to_lowercase())),
            // Enam nama yang punya bentuk namespace di KEDUA backend dan bentuk method
            // HANYA di VM — jadi `"x".capitalize()` jalan lewat `--vm` dan ditolak …
            (s, "capitalize") | (s, "swap_case") | (s, "is_upper")
            | (s, "is_lower") | (s, "slice") | (s, "format")
            // Tujuh lagi dengan bentuk yang sama persis: `str::X(s, ..)` jalan
            // di dua mesin, `"s".X(..)` cuma di VM.
            | (s, "ljust") | (s, "rjust") | (s, "strip_prefix")
            | (s, "strip_suffix") | (s, "substr") => {
                use crate::interpreter::builtin::NativeFnKind as K;
                let kind = match method {
                    "capitalize" => K::StrCapitalize,
                    "swap_case"  => K::StrSwapCase,
                    "is_upper"   => K::StrIsUpper,
                    "is_lower"   => K::StrIsLower,
                    "slice"      => K::StrSlice,
                    // `ljust` Python memadatkan di KANAN, `rjust` di kiri —
                    // penamaannya soal ke mana teksnya rata, bukan ke mana
                    // isian ditaruh. Pemetaan yang sama dengan `str::ljust`.
                    "ljust"        => K::StrPadRight,
                    "rjust"        => K::StrPadLeft,
                    "strip_prefix" => K::StrStripPrefix,
                    "strip_suffix" => K::StrStripSuffix,
                    "substr"       => K::StrSubstr,
                    _            => K::StrFormat,
                };
                let mut v = vec![Value::Str(s.clone())];
                v.extend(args.iter().cloned());
                Self::dispatch_str_inner(&kind, &v)
            }
            (s, "trim")         => Ok(Value::Str(s.trim().to_string())),
            (s, "trim_start")   => Ok(Value::Str(s.trim_start().to_string())),
            (s, "trim_end")     => Ok(Value::Str(s.trim_end().to_string())),
            (s, "to_string")    => Ok(Value::Str(s.clone())),
            (s, "chars")        => Ok(Value::Array(s.chars().map(Value::Rune).collect())),
            (s, "bytes")        => Ok(Value::Array(s.bytes().map(|b| Value::Int(b as i128)).collect())),
            (s, "words")        => Ok(Value::Array(s.split_whitespace().map(|w| Value::Str(w.to_string())).collect())),
            (s, "lines")        => Ok(Value::Array(s.lines().map(|l| Value::Str(l.to_string())).collect())),
            (s, "reverse")      => Ok(Value::Str(s.chars().rev().collect())),
            // Tujuh method ini SATU implementasi dengan bentuk `str::`-nya, yang
            // juga dipakai VM. Salinan di sini dulu men-Display POLA-nya, jadi
            // `"x1.5y".contains(1.5)` menjawab `true` — kelas #313 di pintu method.
            (s, "split" | "starts_with" | "ends_with" | "contains" | "replace"
                | "find" | "index_of" | "count") => {
                use crate::interpreter::builtin::NativeFnKind::*;
                let kind = match method {
                    "split" => StrSplit, "starts_with" => StrStartsWith,
                    "ends_with" => StrEndsWith, "contains" => StrContains,
                    "replace" => StrReplace, "count" => StrCount, _ => StrFind,
                };
                let mut semua = Vec::with_capacity(args.len() + 1);
                semua.push(Value::Str(s.clone()));
                semua.extend(args.iter().cloned());
                Self::dispatch_str_inner(&kind, &semua)
            }
            (s, "repeat") => {
                // Argumen yang hilang DITOLAK, tidak diberi nilai bawaan.
                if args.is_empty() {
                    return Some(runtime_err!(
                        "{}", crate::interpreter::pesan_metode_argumen("str", "repeat", args.len())));
                }
                // Batas alokasi yang sama dengan operator `"ab" * n` —
                // dulu `s.repeat(n)` di sini dipanggil langsung tanpa
                // batas, jadi `n` yang kebesaran bisa OOM-kill proses.
                let n = bawa!(args.first().map(|v| nilai_ke_i128(v)).transpose()).unwrap_or(1);
                Self::v_repeat_str(&s, n)
            }
            (s, "char_at") => {
                // Argumen yang hilang DITOLAK, tidak diberi nilai bawaan.
                if args.is_empty() {
                    return Some(runtime_err!(
                        "{}", crate::interpreter::pesan_metode_argumen("str", "char_at", args.len())));
                }
                let i = bawa!(crate::interpreter::eval_expr::indeks_karakter(&args[0], s));
                Ok(Value::Rune(s.chars().nth(i).unwrap_or_default()))
            }
            // Lewat dispatcher yang sama dengan bentuk `str::to_int`, bukan
            // salinan kedua: dua kalimat untuk satu kegagalan parse adalah
            // persis yang dulu terjadi di sini.
            (s, "to_int") | (s, "parse_int") =>
                Self::parse_int_lepas(&Value::Str(s.clone())),
            (s, "to_float") | (s, "parse_float") =>
                Self::parse_float_lepas(&Value::Str(s.clone())),
            (s, "pad_left") => {
                // Argumen yang hilang DITOLAK, tidak diberi nilai bawaan.
                if args.is_empty() {
                    return Some(runtime_err!(
                        "{}", crate::interpreter::pesan_metode_argumen("str", "pad_left", args.len())));
                }
                // Satu definisi dengan bentuk namespace: salinan kedua di sini dulu tanpa
                // batas lebar, jadi `"a".pad_left(99999999999)` mematikan prosesnya.
                let mut semua = vec![Value::Str(s.clone())];
                semua.extend(args.iter().cloned());
                Self::dispatch_str_inner(&crate::interpreter::builtin::NativeFnKind::StrPadLeft, &semua)
            }
            (s, "pad_right") => {
                // Argumen yang hilang DITOLAK, tidak diberi nilai bawaan.
                if args.is_empty() {
                    return Some(runtime_err!(
                        "{}", crate::interpreter::pesan_metode_argumen("str", "pad_right", args.len())));
                }
                // Satu definisi dengan bentuk namespace: salinan kedua di sini dulu tanpa
                // batas lebar, jadi `"a".pad_right(99999999999)` mematikan prosesnya.
                let mut semua = vec![Value::Str(s.clone())];
                semua.extend(args.iter().cloned());
                Self::dispatch_str_inner(&crate::interpreter::builtin::NativeFnKind::StrPadRight, &semua)
            }
            (s, "center") => {
                // Argumen yang hilang DITOLAK, tidak diberi nilai bawaan.
                if args.is_empty() {
                    return Some(runtime_err!(
                        "{}", crate::interpreter::pesan_metode_argumen("str", "center", args.len())));
                }
                // Satu definisi dengan bentuk namespace: salinan kedua di sini dulu tanpa
                // batas lebar, jadi `"a".center(99999999999)` mematikan prosesnya.
                let mut semua = vec![Value::Str(s.clone())];
                semua.extend(args.iter().cloned());
                Self::dispatch_str_inner(&crate::interpreter::builtin::NativeFnKind::StrCenter, &semua)
            }
            (s, "title") => {
                let r: String = s.split_whitespace().map(|w| {
                    let mut ch = w.chars();
                    match ch.next() {
                        None    => String::new(),
                        Some(c) => c.to_uppercase().to_string() + ch.as_str(),
                    }
                }).collect::<Vec<_>>().join(" ");
                Ok(Value::Str(r))
            }
            // Sama seperti `str::slice`: dilempar ke `eval_slice` yang sama dengan
            // `s[awal:akhir]`, bukan diimplementasikan ulang dengan …
            (s, "substring") | (s, "sub") => {
                let lo = args.first().cloned();
                let hi = args.get(1).cloned();
                Self::potong_str(s, lo, hi)
            }
            (s, "is_numeric") =>
                Ok(Value::Bool(!s.is_empty() && s.trim().chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-'))),
            (s, "is_alpha") =>
                Ok(Value::Bool(!s.is_empty() && s.chars().all(|c| c.is_alphabetic()))),
            (s, "is_alphanumeric") =>
                Ok(Value::Bool(!s.is_empty() && s.chars().all(|c| c.is_alphanumeric()))),
            (s, "snake_case") => {
                let mut r = String::new();
                for (i, c) in s.chars().enumerate() {
                    if c.is_uppercase() && i > 0 { r.push('_'); }
                    r.push(c.to_lowercase().next().unwrap_or(c));
                }
                Ok(Value::Str(r.replace(' ', "_")))
            }
            (s, "camel_case") => {
                let words: Vec<&str> = s.split(|c: char| c == '_' || c == ' ').collect();
                let r: String = words.iter().enumerate().map(|(i, w)| {
                    if i == 0 { w.to_lowercase() } else {
                        let mut ch = w.chars();
                        match ch.next() {
                            None    => String::new(),
                            Some(c) => c.to_uppercase().to_string() + &ch.as_str().to_lowercase(),
                        }
                    }
                }).collect();
                Ok(Value::Str(r))
            }
            _ => return None,
        })
    }

    /// Cabang `Str` dari `eval_slice`, diangkat karena ia bebas `self`
    /// sementara `eval_slice` secara keseluruhan tidak (cabang struct
    /// memanggil method pengguna).
    pub fn potong_str(s: &str, lo: Option<Value>, hi: Option<Value>) -> EvalResult {
        let ch: Vec<char> = s.chars().collect();
        let n = ch.len();
        let si = lo.as_ref().map(|v| Self::slice_bound(v, n, "awal")).transpose()?.unwrap_or(0);
        let ei = hi.as_ref().map(|v| Self::slice_bound(v, n, "akhir")).transpose()?.unwrap_or(n);
        let (si, ei) = Self::slice_bounds(si, ei)?;
        Ok(Value::Str(ch[si..ei].iter().collect()))
    }

    /// `"x" + p` adalah jalur tampilan, jadi ia harus memakai hook juga.
    pub(crate) fn siapkan_gabung_string(
        &mut self,
        op: &BinaryOp,
        a: Value,
        b: Value,
    ) -> Result<(Value, Value), Signal> {
        if !matches!(op, BinaryOp::Add) {
            return Ok((a, b));
        }
        match (&a, &b) {
            (Value::Str(_), _) if Self::perlu_tampil(&b) => {
                let s = self.tampil(&b)?;
                Ok((a, Value::Str(s)))
            }
            (_, Value::Str(_)) if Self::perlu_tampil(&a) => {
                let s = self.tampil(&a)?;
                Ok((Value::Str(s), b))
            }
            _ => Ok((a, b)),
        }
    }

    /// Nilai yang boleh berubah bentuk lewat hook tampilan.
    pub(crate) fn perlu_tampil(v: &Value) -> bool {
        matches!(
            v,
            Value::Struct { .. } | Value::Enum { .. } | Value::Array(_) | Value::Tuple(_)
        )
    }

}
