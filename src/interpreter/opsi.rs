// src/interpreter/opsi.rs — method `Option`/`Result` dari prelude JRG (`src/opsi.jrg`) di interpreter.

use super::{Interpreter, EvalResult, Signal};
use super::value::Value;

impl Interpreter {
    /// `o.m(args)` lewat prelude kalau `o` sebuah `Option`/`Result` dan prelude punya `m`; `None` kalau tidak.
    pub(crate) fn coba_opsi(&mut self, obj: &Value, method: &str, args: &[Value]) -> Option<EvalResult> {
        let Value::Enum { type_name, .. } = obj else { return None };
        let nama = crate::opsi::nama_fungsi(type_name, method)?;
        let kunci = format!("{}::{}", crate::opsi::ALIAS, nama);
        if self.env.get(&kunci).is_none() {
            if let Err(e) = self.load_module(crate::opsi::NAMA, Some(crate::opsi::ALIAS)) {
                return Some(Err(Signal::RuntimeError(e)));
            }
        }
        let f = self.env.get(&kunci)?;
        if let Value::Function(jf) = &f {
            let butuh = jf.params.len().saturating_sub(1);
            if args.len() != butuh {
                return Some(Err(Signal::RuntimeError(super::RuntimeError::new(
                    crate::opsi::pesan_argumen(method, &jf.params[1..], args.len())))));
            }
        }
        let mut semua = Vec::with_capacity(args.len() + 1);
        semua.push(obj.clone());
        semua.extend_from_slice(args);
        Some(self.call_fn(f, semua).map_err(|s| match s {
            // Galat yang LAHIR di prelude menunjuk situs method-nya, bukan baris di `<opsi>`.
            Signal::RuntimeError(mut e) if e.span.is_some_and(di_opsi) => {
                e.span = None;
                e.jejak.retain(|b| !b.span.is_some_and(di_opsi));
                Signal::RuntimeError(e)
            }
            lain => lain,
        }))
    }
}

fn di_opsi(sp: crate::lexer::Span) -> bool {
    sp.file_id != 0 && crate::diag::sumber_modul(sp.file_id).is_some_and(|f| f.name == crate::opsi::NAMA)
}
