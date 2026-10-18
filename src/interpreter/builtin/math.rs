// src/interpreter/builtin/math.rs — interpreter/builtin/math.rs
// Dispatch untuk namespace `math::` (factorial, trig, gcd/lcm, konstanta,

use crate::interpreter::{Value, Signal, RuntimeError, EvalResult, Interpreter};
use crate::runtime_err;
use super::NativeFnKind;

impl Interpreter {
    pub(super) fn dispatch_math(&mut self, kind: &NativeFnKind, args: &[Value]) -> Option<EvalResult> {
        use NativeFnKind::*;
        if !matches!(kind,
            MathAbs | MathSqrt | MathPow | MathFloor | MathCeil | MathRound |
            MathSin | MathCos | MathTan | MathAsin | MathAcos | MathAtan | MathAtan2 |
            MathLn | MathLog10 | MathLog2 | MathExp | MathMin | MathMax | MathClamp |
            MathIsNan | MathIsInf | MathSign | MathFact | MathGcd | MathLcm |
            MathPi | MathE |
            MathTau | MathInf | MathNan | MathHypot | MathLog | MathDegrees | MathRadians |
            MathTrunc | MathCopysign | MathWrapAdd | MathWrapSub | MathWrapMul
        ) {
            return None;
        }
        Some(Self::dispatch_math_lepas(kind, args))
    }

    /// `|n|` yang melaporkan kasus satu-satunya yang tidak punya jawaban:
    /// `i128::MIN`, yang lawan positifnya tidak muat di 128 bit.
    fn abs_checked(n: i128, konteks: &str) -> Result<i128, Signal> {
        n.checked_abs().ok_or_else(|| {
            Signal::RuntimeError(RuntimeError::new(format!(
                "{}: `{}` tidak punya lawan positif di integer 128 bit", konteks, n
            )))
        })
    }

    /// Satu implementasi `math::` untuk DUA mesin: `--vm` mencapainya lewat
    /// jembatan nilai, jadi jawaban, penolakan, dan kalimatnya tidak bisa …
    pub(crate) fn dispatch_math_lepas(kind: &NativeFnKind, args: &[Value]) -> EvalResult {
        use NativeFnKind::*;
        let (nama, n, bentuk) = bentuk_math(kind);
        if args.len() != n {
            return runtime_err!("{}", pesan_math_argumen(nama, n, bentuk, args.len()));
        }
        let angka = |v: &Value| dengan_nama(nama, crate::interpreter::eval_expr::nilai_ke_f64(v));
        let bulat = |v: &Value| dengan_nama(nama, crate::interpreter::eval_expr::nilai_ke_i128(v));
        fn fst(args: &[Value]) -> Value { args.get(0).cloned().unwrap_or(Value::Nil) }
        match kind {
            // `checked_abs`: `i128::MIN` tidak punya lawan positif di 128
            // bit, jadi `.abs()` telanjang di situ bikin runtime Rust panik
            // dan seluruh interpreter mati dengan "panic internal".
            MathAbs => match fst(args) {
                Value::Int(n)   => n.checked_abs().map(Value::Int).ok_or_else(|| {
                    Signal::RuntimeError(RuntimeError::new(format!(
                        "{}", crate::interpreter::pesan_abs_min()
                    )))
                }),
                Value::Float(f) => Ok(Value::Float(f.abs())),
                other => runtime_err!("math::abs: butuh angka, dapat `{}`", other.type_name()),
            },
            MathSqrt  => { let f = angka(&fst(args))?; Ok(Value::Float(f.sqrt())) }
            MathPow   => {
                Ok(Value::Float(angka(&args[0])?.powf(angka(&args[1])?)))
            }
            MathFloor  => { let f = angka(&fst(args))?; Ok(Value::Float(f.floor())) }
            MathCeil   => { let f = angka(&fst(args))?; Ok(Value::Float(f.ceil()))  }
            MathRound  => { let f = angka(&fst(args))?; Ok(Value::Float(f.round())) }
            MathSin    => { let f = angka(&fst(args))?; Ok(Value::Float(f.sin()))   }
            MathCos    => { let f = angka(&fst(args))?; Ok(Value::Float(f.cos()))   }
            MathTan    => { let f = angka(&fst(args))?; Ok(Value::Float(f.tan()))   }
            MathAsin   => { let f = angka(&fst(args))?; Ok(Value::Float(f.asin()))  }
            MathAcos   => { let f = angka(&fst(args))?; Ok(Value::Float(f.acos()))  }
            MathAtan   => { let f = angka(&fst(args))?; Ok(Value::Float(f.atan()))  }
            MathAtan2  => {
                Ok(Value::Float(angka(&args[0])?.atan2(angka(&args[1])?)))
            }
            MathLn     => { let f = angka(&fst(args))?; Ok(Value::Float(f.ln()))    }
            MathLog10  => { let f = angka(&fst(args))?; Ok(Value::Float(f.log10())) }
            MathLog2   => { let f = angka(&fst(args))?; Ok(Value::Float(f.log2()))  }
            MathExp    => { let f = angka(&fst(args))?; Ok(Value::Float(f.exp()))   }
            MathMin => {
                match Self::banding_lepas(args[0].clone(), args[1].clone(), std::cmp::Ordering::Less, false)? {
                    Value::Bool(true) => Ok(args[0].clone()),
                    _                 => Ok(args[1].clone()),
                }
            }
            MathMax => {
                match Self::banding_lepas(args[0].clone(), args[1].clone(), std::cmp::Ordering::Greater, false)? {
                    Value::Bool(true) => Ok(args[0].clone()),
                    _                 => Ok(args[1].clone()),
                }
            }
            MathClamp => {
                let x = args[0].clone(); let lo = args[1].clone(); let hi = args[2].clone();
                // `lo` harus <= `hi`.
                if matches!(Self::banding_lepas(lo.clone(), hi.clone(), std::cmp::Ordering::Greater, false)?, Value::Bool(true)) {
                    return runtime_err!(
                        "math::clamp: batas terbalik — lo (`{}`) lebih besar dari hi (`{}`); cek lagi urutan argumennya, harusnya `clamp(x, lo, hi)` dengan lo <= hi",
                        lo, hi
                    );
                }
                if matches!(Self::banding_lepas(x.clone(), lo.clone(), std::cmp::Ordering::Less, false)?, Value::Bool(true)) {
                    return Ok(lo);
                }
                if matches!(Self::banding_lepas(x.clone(), hi.clone(), std::cmp::Ordering::Greater, false)?, Value::Bool(true)) {
                    return Ok(hi);
                }
                Ok(x)
            }
            MathIsNan  => { let f = angka(&fst(args))?; Ok(Value::Bool(f.is_nan()))      }
            MathIsInf  => { let f = angka(&fst(args))?; Ok(Value::Bool(f.is_infinite())) }
            MathSign   => match fst(args) {
                Value::Int(n)   => Ok(Value::Int(n.signum())),
                // `f64::signum` menjawab `1.0` untuk `0.0` dan `-1.0` untuk `-0.0` —
                // tanda NOL dijawab positif. Salinan VM yang lama sudah benar di sini.
                Value::Float(f) => Ok(Value::Float(if f == 0.0 { 0.0 } else { f.signum() })),
                other => runtime_err!("math::sign: butuh angka, dapat `{}`", other.type_name()),
            },
            MathFact => {
                let n = bulat(&fst(args))?;
                match Interpreter::faktorial_terperiksa(n) {
                    Ok(v) => Ok(Value::Int(v)),
                    Err(e) => runtime_err!("{}", e),
                }
            }
            MathGcd => {
                // `checked_abs` — lihat catatan di `MathAbs`.
                let mut a = Self::abs_checked(bulat(&args[0])?, "math::gcd")?;
                let mut b = Self::abs_checked(bulat(&args[1])?, "math::gcd")?;
                while b != 0 { let t = b; b = a % b; a = t; }
                Ok(Value::Int(a))
            }
            MathLcm => {
                let a = Self::abs_checked(bulat(&args[0])?, "math::lcm")?;
                let b = Self::abs_checked(bulat(&args[1])?, "math::lcm")?;
                if a == 0 || b == 0 { return Ok(Value::Int(0)); }
                let mut ga = a; let mut gb = b;
                while gb != 0 { let t = gb; gb = ga % gb; ga = t; }
                // Diperiksa: `a / ga * b` telanjang MEMBUNGKUS di build rilis, jadi
                // `math::lcm(i128::MAX, 2)` dulu menjawab `-2`.
                (a / ga).checked_mul(b).map(Value::Int).ok_or_else(|| Signal::RuntimeError(
                    RuntimeError::new("math::lcm: hasilnya kelewat besar buat integer 128 bit".to_string())))
            }
            MathPi => Ok(Value::Float(std::f64::consts::PI)),
            MathE  => Ok(Value::Float(std::f64::consts::E)),
            MathTau => Ok(Value::Float(std::f64::consts::TAU)),
            MathInf => Ok(Value::Float(f64::INFINITY)),
            MathNan => Ok(Value::Float(f64::NAN)),
            MathHypot => {
                Ok(Value::Float(angka(&args[0])?.hypot(angka(&args[1])?)))
            }
            MathLog => {
                Ok(Value::Float(angka(&args[0])?.log(angka(&args[1])?)))
            }
            MathDegrees => { let r = angka(&fst(args))?; Ok(Value::Float(r.to_degrees())) }
            MathRadians => { let d = angka(&fst(args))?; Ok(Value::Float(d.to_radians())) }
            MathTrunc => { let f = angka(&fst(args))?; Ok(Value::Float(f.trunc())) }
            // Aritmetika yang SENGAJA membungkus di 64 bit tak bertanda (hash, LCG): operator
            // biasa diperiksa di tiga mesin, jadi pembungkusan harus ditulis.
            MathWrapAdd | MathWrapSub | MathWrapMul => {
                let ke64 = |v: i128| v.rem_euclid(1i128 << 64) as u64;
                let (a, b) = (ke64(bulat(&args[0])?), ke64(bulat(&args[1])?));
                let h = match kind {
                    MathWrapAdd => a.wrapping_add(b),
                    MathWrapSub => a.wrapping_sub(b),
                    _ => a.wrapping_mul(b),
                };
                Ok(Value::Int(h as i128))
            }
            MathCopysign => {
                Ok(Value::Float(angka(&args[0])?.copysign(angka(&args[1])?)))
            }
            _ => unreachable!("kind {:?} lolos filter dispatch_math tapi tak ada arm-nya di dispatch_math_lepas", kind),
        }
    }

    /// `n!` dengan overflow yang DIPERIKSA.
    pub(crate) fn faktorial_terperiksa(n: i128) -> Result<i128, String> {
        if n < 0 {
            return Err("math::factorial: n harus >= 0".to_string());
        }
        let mut r: i128 = 1;
        for i in 2..=n {
            r = match r.checked_mul(i) {
                Some(v) => v,
                None => return Err(format!(
                    "math::factorial({}) kelewat besar buat integer 128 bit — \
                     mulai kelewat di sekitar {}!. Pakai float kalau cuma butuh \
                     perkiraan besarannya",
                    n, i
                )),
            };
        }
        Ok(r)
    }
}

/// Nama, jumlah argumen, dan nama parameternya — satu tabel untuk nama di
/// pesan DAN pemeriksaan aritas, jadi keduanya tidak bisa menyebut hal yang …
fn bentuk_math(kind: &NativeFnKind) -> (&'static str, usize, &'static str) {
    use NativeFnKind::*;
    match kind {
        MathAbs => ("math::abs", 1, "(x)"),     MathSqrt => ("math::sqrt", 1, "(x)"),
        MathFloor => ("math::floor", 1, "(x)"), MathCeil => ("math::ceil", 1, "(x)"),
        MathRound => ("math::round", 1, "(x)"), MathTrunc => ("math::trunc", 1, "(x)"),
        MathSin => ("math::sin", 1, "(x)"),     MathCos => ("math::cos", 1, "(x)"),
        MathTan => ("math::tan", 1, "(x)"),     MathAsin => ("math::asin", 1, "(x)"),
        MathAcos => ("math::acos", 1, "(x)"),   MathAtan => ("math::atan", 1, "(x)"),
        MathLn => ("math::ln", 1, "(x)"),       MathLog10 => ("math::log10", 1, "(x)"),
        MathLog2 => ("math::log2", 1, "(x)"),   MathExp => ("math::exp", 1, "(x)"),
        MathIsNan => ("math::is_nan", 1, "(x)"), MathIsInf => ("math::is_inf", 1, "(x)"),
        MathSign => ("math::sign", 1, "(x)"),   MathFact => ("math::factorial", 1, "(n)"),
        MathDegrees => ("math::degrees", 1, "(radian)"),
        MathRadians => ("math::radians", 1, "(derajat)"),
        MathPow => ("math::pow", 2, "(x, y)"),  MathAtan2 => ("math::atan2", 2, "(y, x)"),
        MathMin => ("math::min", 2, "(a, b)"),  MathMax => ("math::max", 2, "(a, b)"),
        MathGcd => ("math::gcd", 2, "(a, b)"),  MathLcm => ("math::lcm", 2, "(a, b)"),
        MathHypot => ("math::hypot", 2, "(x, y)"), MathLog => ("math::log", 2, "(x, basis)"),
        MathCopysign => ("math::copysign", 2, "(besar, tanda)"),
        MathWrapAdd => ("math::wrap_add", 2, "(a, b)"), MathWrapSub => ("math::wrap_sub", 2, "(a, b)"),
        MathWrapMul => ("math::wrap_mul", 2, "(a, b)"),
        MathClamp => ("math::clamp", 3, "(x, lo, hi)"),
        MathPi => ("math::PI", 0, "()"),        MathE => ("math::E", 0, "()"),
        MathTau => ("math::TAU", 0, "()"),      MathInf => ("math::INF", 0, "()"),
        MathNan => ("math::NAN", 0, "()"),
        _ => unreachable!("kind {:?} bukan anggota math::", kind),
    }
}

/// Method angka → fungsi `math::` yang melayaninya, tanpa mencari `env` di tiap
/// panggilan. `is_finite` hidup di stdlib VM, jadi ia lewat jembatan seperti
/// bentuk namespace-nya.
pub(crate) fn jenis_metode_math(method: &str) -> Option<NativeFnKind> {
    use NativeFnKind::*;
    Some(match method {
        "abs" => MathAbs,     "ceil" => MathCeil,   "cos" => MathCos,     "exp" => MathExp,
        "floor" => MathFloor, "is_inf" => MathIsInf, "is_nan" => MathIsNan, "ln" => MathLn,
        "log10" => MathLog10, "pow" => MathPow,     "round" => MathRound, "sin" => MathSin,
        "sqrt" => MathSqrt,   "tan" => MathTan,
        "is_finite" => DariStdlibVm("math::is_finite"),
        _ => return None,
    })
}

/// Kurang ATAU lebih argumen — satu kalimat, bentuknya sama dengan kalimat
/// method (`method `chunks` butuh 1 argumen (ukuran), dapat 0`).
pub fn pesan_math_argumen(nama: &str, n: usize, bentuk: &str, diberi: usize) -> String {
    crate::interpreter::pesan_butuh_argumen(nama, n, bentuk, diberi)
}

/// Galat konversi dari pintu bersama (`nilai_ke_f64`/`nilai_ke_i128`) tidak
/// tahu fungsi mana yang memanggilnya; di sini namanya dibubuhkan.
fn dengan_nama<T>(nama: &str, r: Result<T, Signal>) -> Result<T, Signal> {
    r.map_err(|s| match s {
        Signal::RuntimeError(mut e) => {
            e.message = format!("{}: {}", nama, e.message);
            Signal::RuntimeError(e)
        }
        lain => lain,
    })
}
