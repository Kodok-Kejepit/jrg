// src/interpreter/builtin/sysenv.rs — interpreter/builtin/sysenv.rs
// Dispatch untuk namespace `env::` — environment variable & argumen CLI

use crate::interpreter::{Value, Interpreter, EvalResult};
use crate::runtime_err;
use super::NativeFnKind;

impl Interpreter {
    pub(super) fn dispatch_sysenv(&mut self, kind: &NativeFnKind, args: &[Value]) -> Option<EvalResult> {
        use NativeFnKind::*;
        if !matches!(kind, EnvGet | EnvGetOr | EnvSet | EnvArgs | EnvExe) {
            return None;
        }
        Some(Self::dispatch_sysenv_inner(kind, args))
    }

    pub fn dispatch_sysenv_inner(kind: &NativeFnKind, args: &[Value]) -> EvalResult {
        use NativeFnKind::*;
        // Nama variabel adalah ISI: dulu `env::get(nil)` membaca variabel bernama
        // "nil". Nilai yang DITULIS `env::set` dirender, seperti isi berkas.
        fn nama<'a>(fungsi: &str, args: &'a [Value]) -> Result<&'a str, crate::interpreter::Signal> {
            match args.first() {
                Some(Value::Str(s)) => Ok(s.as_str()),
                lain => Err(crate::interpreter::Signal::RuntimeError(crate::interpreter::RuntimeError::new(
                    crate::interpreter::pesan_arg_harus_teks(fungsi, "nama variabelnya",
                        &lain.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".to_string()))))),
            }
        }
        match kind {
            EnvGet => {
                let name = nama("env::get", args)?;
                // `Option`, bukan `nil`: yang mungkin tidak ada menjawab `Option`.
                Ok(std::env::var(name).map(|v| crate::interpreter::some_value(Value::Str(v)))
                    .unwrap_or_else(|_| crate::interpreter::none_value()))
            }

            EnvGetOr => {
                if args.len() < 2 { return runtime_err!("env::get_or butuh (nama, default)"); }
                let name = nama("env::get_or", args)?;
                Ok(std::env::var(name).map(Value::Str).unwrap_or_else(|_| args[1].clone()))
            }

            EnvSet => {
                if args.len() < 2 { return runtime_err!("env::set butuh (nama, nilai)"); }
                let name = nama("env::set", args)?;
                let val = format!("{}", args[1]);
                // `set_var` MEMATIKAN prosesnya untuk nama kosong, nama ber-`=`, dan
                // NUL di nama atau nilai — jadi semuanya diperiksa lebih dulu.
                if name.is_empty() || name.contains('=') || name.contains('\0') {
                    return runtime_err!("env::set: nama variabel {:?} tidak sah — tidak boleh \
                                         kosong, dan tidak boleh berisi `=` atau karakter NUL", name);
                }
                if val.contains('\0') {
                    return runtime_err!("env::set: nilai untuk `{}` berisi karakter NUL, dan \
                                         variabel lingkungan tidak bisa menyimpannya", name);
                }
                // SAFETY: JRG selalu single-threaded lewat interpreter (goroutine JRG jalan
                // di worker thread milik JRG sendiri, bukan lewat env var), jadi tidak ada …
                unsafe { std::env::set_var(&name, &val); }
                Ok(Value::Unit)
            }

            EnvArgs => {
                // `[nama_skrip, arg...]`, bukan argumen jrgc — bentuk yang
                // sama dengan Python dan Node, dan satu-satunya yang berguna
                // buat program. Diisi `main` lewat `crate::argumen::pasang`.
                Ok(Value::Array(crate::argumen::ambil()
                    .into_iter().map(Value::Str).collect()))
            }

            EnvExe => {
                // Path biner yang SEDANG berjalan. `env::args()[0]` tidak bisa
                // dipakai untuk ini: ia nama skrip, bukan nama juru bahasanya.
                match std::env::current_exe() {
                    Ok(p) => Ok(Value::Str(p.display().to_string())),
                    Err(e) => runtime_err!("env::exe: tidak bisa dibaca: {}", e),
                }
            }

            _ => runtime_err!("dispatch_sysenv_inner: kind tidak dikenal"),
        }
    }
}
