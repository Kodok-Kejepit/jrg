// src/interpreter/builtin/lua.rs — pintu `lua::` di interpreter. Tiap lengan memeriksa
// TIPE argumennya lalu menyerahkan ke `ffi::lua`; jumlahnya sudah diperiksa pintu native.

use crate::ffi::lua;
use crate::ffi::pekerja::Panggil;
use crate::interpreter::{pesan_arg_harus_teks, EvalResult, RuntimeError, Signal, Value};
use super::NativeFnKind;

fn teks(args: &[Value], i: usize, fname: &str, peran: &str) -> Result<String, Signal> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(s.clone()),
        lain => Err(Signal::RuntimeError(RuntimeError::new(pesan_arg_harus_teks(
            fname, &format!("argumen ke-{} ({})", i + 1, peran),
            &lain.map(|v| v.type_name()).unwrap_or_else(|| "tidak ada".into()))))),
    }
}

/// Pintu `lua::`, bebas mesin: `p` satu-satunya jalan worker Lua menjalankan kode JRG.
pub(super) fn dispatch_lua(kind: &NativeFnKind, args: &[Value], p: Panggil) -> Option<EvalResult> {
    {
        use NativeFnKind::*;
        if !matches!(kind, LuaEval | LuaExec | LuaCall | LuaRequire | LuaTersedia) { return None; }
        Some(Ok(match kind {
            LuaEval => lua::lua_eval(&match teks(args, 0, "lua::eval", "kode") { Ok(t) => t, Err(e) => return Some(Err(e)) }, p),
            LuaExec => lua::lua_exec(&match teks(args, 0, "lua::exec", "kode") { Ok(t) => t, Err(e) => return Some(Err(e)) }, p),
            LuaCall => {
                let f = match teks(args, 0, "lua::call", "nama fungsi") { Ok(t) => t, Err(e) => return Some(Err(e)) };
                lua::lua_call(&f, &args[1..], p)
            }
            LuaRequire => lua::lua_require(&match teks(args, 0, "lua::require", "nama modul") { Ok(t) => t, Err(e) => return Some(Err(e)) }, p),
            LuaTersedia => lua::lua_tersedia(p),
            _ => unreachable!("dispatch_lua menyaring jenisnya di atas"),
        }))
    }
}
