// src/vm/opsi.rs — method `Option`/`Result` dari prelude JRG (`src/opsi.jrg`) di VM: dimuat sekali per VM saat
// method pertama yang bukan bawaan diminta, lewat pengompilasi modul yang sama dengan `import`.

use super::value::{Label, VmValue};
use super::vm::*;

impl Vm {
    /// Fungsi prelude untuk `obj.name(..)` kalau `obj` sebuah `Option`/`Result` dan prelude punya `name`; jumlah
    /// argumen yang `ditulis` (kalau diketahui) diperiksa dengan kalimat method (`opsi::pesan_argumen`).
    pub(crate) fn fungsi_opsi(&mut self, obj: &VmValue, name: &str, ditulis: Option<usize>,
        sp: Option<crate::lexer::Span>) -> Option<VmResult<VmValue>> {
        let VmValue::Map(m) = obj else { return None };
        let Label::Enum { nama, .. } = &m.label else { return None };
        let fungsi = crate::opsi::nama_fungsi(&nama.tipe, name)?;
        let kunci = format!("{}::{}", crate::opsi::ALIAS, fungsi);
        if !self.globals.contains_key(&kunci) {
            if let Err(e) = self.muat_opsi() { return Some(Err(e)); }
        }
        let f = self.globals.get(&kunci).cloned()?;
        if let (VmValue::Function(jf), Some(n)) = (&f, ditulis) {
            if jf.params.len() != n + 1 {
                return Some(Err(VmError::at(crate::opsi::pesan_argumen(name, &jf.params[1..], n), sp)));
            }
        }
        Some(Ok(f))
    }

    /// Prelude dikompilasi (sekali per thread) lalu dijalankan sebagai fungsi di VM ini: globalnya terpasang.
    fn muat_opsi(&mut self) -> VmResult<()> {
        thread_local! { static CHUNK: std::cell::RefCell<Option<super::chunk::Chunk>> = const { std::cell::RefCell::new(None) }; }
        let chunk = match CHUNK.with(|c| c.borrow().clone()) {
            Some(c) => c,
            None => {
                let c = kompilasi_opsi()?;
                CHUNK.with(|x| *x.borrow_mut() = Some(c.clone()));
                c
            }
        };
        let f = std::sync::Arc::new(super::value::VmFunction::new("<opsi>", 0, chunk));
        self.call_value_immediate(VmValue::Function(f), Vec::new()).map(|_| ())
    }
}

fn kompilasi_opsi() -> VmResult<super::chunk::Chunk> {
    use crate::lexer::{lex, SourceFile};
    let id = crate::diag::register_module_source(crate::opsi::NAMA.to_string(), crate::opsi::SUMBER.to_string());
    let src = SourceFile { id, name: crate::opsi::NAMA.to_string(), src: crate::opsi::SUMBER.to_string() };
    let tokens = lex(&src).map_err(|e| VmError::new(format!("prelude `<opsi>`: {}", e.msg)))?;
    let (module, galat) = crate::parser::parse(tokens, false);
    if let Some(g) = galat.first() { return Err(VmError::new(format!("prelude `<opsi>`: {}", g.msg))); }
    let (mut chunk, _) = super::compiler::compile_module(&module, crate::opsi::ALIAS, &super::compiler::PetaImpor::new())
        .map_err(|e| VmError::new(format!("prelude `<opsi>`: {}", e)))?;
    // Unit diakhiri `Halt` (seluruh VM berhenti); dijalankan sebagai fungsi, ia harus kembali.
    if let Some(i) = chunk.code.last_mut().filter(|i| i.op == super::opcode::OpCode::Halt) { i.op = super::opcode::OpCode::ReturnNil; }
    Ok(chunk)
}
