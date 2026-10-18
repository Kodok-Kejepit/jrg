// src/interpreter/builtin/rand.rs — interpreter/builtin/rand.rs
// Dispatch untuk namespace `rand::` — angka acak yang gampang dipakai

use crate::interpreter::{Value, Signal, RuntimeError, EvalResult, Interpreter};
use crate::runtime_err;
use super::NativeFnKind;
use super::arr::fisher_yates_shuffle;

fn random_u64() -> u64 {
    let bytes = crate::dok::crypto::secure_random_bytes(8);
    let mut b = [0u8; 8];
    b.copy_from_slice(&bytes[..8]);
    u64::from_le_bytes(b)
}

/// Float acak di [0, 1) — 53 bit presisi (sama seperti `f64` mantissa),
/// dibagi dari integer acak supaya distribusinya rata.
fn random_f64_unit() -> f64 {
    (random_u64() >> 11) as f64 / (1u64 << 53) as f64
}

impl Interpreter {
    pub(super) fn dispatch_rand(&mut self, kind: &NativeFnKind, args: &[Value]) -> Option<EvalResult> {
        use NativeFnKind::*;
        if !matches!(kind, RandInt | RandFloat | RandBool | RandChoice | RandShuffle | RandSample) {
            return None;
        }
        Some(Self::dispatch_rand_inner(kind, args))
    }

    pub fn dispatch_rand_inner(kind: &NativeFnKind, args: &[Value]) -> EvalResult {
        use NativeFnKind::*;
        fn fst(args: &[Value]) -> Value { args.get(0).cloned().unwrap_or(Value::Nil) }
        match kind {
            RandInt => {
                if args.len() < 2 { return runtime_err!("rand::int butuh (lo, hi) — inklusif dua sisi, mis. rand::int(1, 6)"); }
                let lo = crate::interpreter::eval_expr::nilai_ke_i128(&args[0])?;
                let hi = crate::interpreter::eval_expr::nilai_ke_i128(&args[1])?;
                if hi < lo { return runtime_err!("rand::int: hi ({}) harus >= lo ({})", hi, lo); }
                let acak = ((random_u64() as u128) << 64) | random_u64() as u128;
                Ok(Value::Int(crate::interpreter::acak_dalam_rentang(lo, hi, acak)))
            }

            RandFloat => Ok(Value::Float(random_f64_unit())),

            RandBool => Ok(Value::Bool(random_u64() % 2 == 0)),

            RandChoice => {
                let arr = match fst(args) {
                    Value::Array(a) => a,
                    o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("rand::choice", 1, &o.type_name())),
                };
                if arr.is_empty() { return runtime_err!("rand::choice: array kosong, tidak ada yang bisa dipilih"); }
                let i = (random_u64() as usize) % arr.len();
                Ok(arr[i].clone())
            }

            RandShuffle => {
                let arr = match fst(args) {
                    Value::Array(a) => a,
                    o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("rand::shuffle", 1, &o.type_name())),
                };
                let mut idxs: Vec<usize> = (0..arr.len()).collect();
                fisher_yates_shuffle(&mut idxs);
                Ok(Value::Array(idxs.into_iter().map(|i| arr[i].clone()).collect()))
            }

            RandSample => {
                if args.len() < 2 { return runtime_err!("rand::sample butuh (arr, n)"); }
                let arr = match args[0].clone() {
                    Value::Array(a) => a,
                    o => return runtime_err!("{}", crate::interpreter::pesan_arg_harus_array("rand::sample", 1, &o.type_name())),
                };
                let n = crate::interpreter::eval_expr::nilai_ke_usize(&args[1])?.min(arr.len());
                let mut idxs: Vec<usize> = (0..arr.len()).collect();
                fisher_yates_shuffle(&mut idxs);
                Ok(Value::Array(idxs.into_iter().take(n).map(|i| arr[i].clone()).collect()))
            }

            _ => unreachable!("dispatch_rand_inner dipanggil dengan kind di luar namespace rand::"),
        }
    }
}
