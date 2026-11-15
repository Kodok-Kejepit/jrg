// src/vm/op.rs — badan handler opcode yang juga dijalankan langsung oleh jalur lambat JIT
// (`jit::lambat::satu`), tanpa masuk loop dispatch: satu definisi untuk keduanya.

use super::opcode::OpCode;
use super::peta::IsiPeta;
use super::value::{Label, Peta, VmValue};
use super::vm::*;

impl Vm {
    #[inline]
    pub(crate) fn op_get_method(&mut self, arg: u32, sp: Option<crate::lexer::Span>) -> VmResult<()> {
        let obj = self.pop(sp)?;
        let argc = (arg >> 24) as usize;
        let val = {
            let fr = self.frames.last()
                .ok_or_else(|| VmError::new("no active frame"))?;
            let name = fr.func.chunk.names.get((arg & 0x00FF_FFFF) as usize)
                .ok_or_else(|| VmError::new(format!("name idx {} OOB", arg)))?;
            // Jumlah argumen yang DITULIS diperiksa di sini karena sesudah `Call` ia
            // sudah bercampur dengan penerima — dan tanpa ini `(2.5).sqrt(1)` dijawab …
            let nanti = argc == crate::vm::opcode::ARGC_NANTI as usize;
            let v = match self.get_method(&obj, name, sp) {
                Ok(v) => {
                    if nanti {
                        let slot = self.stack.len().saturating_sub(1);
                        self.galat_metode.retain(|(i, _)| *i < slot);
                    }
                    v
                }
                // Acuan mengevaluasi argumennya dulu: galatnya menunggu `PeriksaArgMetode`
                // di slot fungsi (sesudah `Swap`, satu di bawah slot ini).
                Err(e) if nanti => {
                    let slot = self.stack.len().saturating_sub(1);
                    self.galat_metode.retain(|(i, _)| *i < slot);
                    self.galat_metode.push((slot, e));
                    VmValue::Nil
                }
                Err(e) => return Err(e),
            };
            if argc != crate::vm::opcode::ARGC_NANTI as usize && matches!(v, VmValue::NativeFn(_)) {
                if let Some(p) = Self::pesan_jumlah_arg(&obj, name, argc) {
                    return Err(VmError::at(p, sp));
                }
            }
            v
        };
        self.push(val);
        Ok(())
    }

    /// Stack: [fungsi, penerima, arg0..argN-1]; tidak ada yang diambil.
    #[inline]
    pub(crate) fn op_periksa_arg_metode(&mut self, arg: u32, sp: Option<crate::lexer::Span>) -> VmResult<()> {
        use crate::vm::opcode::TANDA_TUNDA;
        let argc = (arg >> 24) as usize;
        let n = self.stack.len();
        if n < argc + 2 { return Err(VmError::at("stack underflow PeriksaArgMetode", sp)); }
        let slot = n - argc - 2;
        self.galat_metode.retain(|(i, _)| *i <= slot);
        if self.galat_metode.last().is_some_and(|(i, _)| *i == slot) {
            let (_, e) = self.galat_metode.pop().unwrap();
            if arg & TANDA_TUNDA == 0 { return Err(e); }
            self.galat_tunda_metode = Some(e);
            return Ok(());
        }
        let pesan = match &self.stack[n - argc - 2] {
            VmValue::NativeFn(_) => {
                let fr = self.frames.last()
                    .ok_or_else(|| VmError::new("no active frame"))?;
                let name = fr.func.chunk.names.get((arg & (TANDA_TUNDA - 1)) as usize)
                    .ok_or_else(|| VmError::new(format!("name idx {} OOB", arg)))?;
                Self::pesan_jumlah_arg(&self.stack[n - argc - 1], name, argc)
            }
            _ => None,
        };
        if let Some(p) = pesan {
            if arg & TANDA_TUNDA == 0 { return Err(VmError::at(p, sp)); }
            self.galat_tunda_metode = Some(VmError::at(p, sp));
        }
        Ok(())
    }

    #[inline]
    pub(crate) fn op_build_struct(&mut self, arg: u32, sp: Option<crate::lexer::Span>) -> VmResult<()> {
        let f = self.frames.last().ok_or_else(|| VmError::new("no active frame"))?;
        let Some(VmValue::Map(proto)) = f.chunk().constants.get(arg as usize) else {
            return Err(VmError::at("BuildStruct: prototipe bukan struct", sp));
        };
        let bentuk = proto.bentuk().ok_or_else(|| VmError::at("BuildStruct: prototipe tanpa bentuk", sp))?;
        let start = self.stack.len().checked_sub(bentuk.nama.len()).ok_or_else(|| VmError::at("BuildStruct: stack kurang", sp))?;
        let label = proto.label.clone();
        // Langsung dikembalikan (`Return` sesudahnya, bingkai tanpa `defer`): lokal sebentuk milik tunggal dipakai ulang.
        let ulang = f.chunk().code.get(f.ip).is_some_and(|i| i.op == OpCode::Return) && f.defers.is_empty();
        let basis = f.slot + 1;
        if let Some(p) = ulang.then(|| self.calon_pakai_ulang(basis, start - basis, &label, &bentuk)).flatten() {
            let mut lama = std::mem::replace(&mut self.stack[basis + p], VmValue::Nil);
            let VmValue::Map(m) = &mut lama else { unreachable!("diperiksa calon_pakai_ulang") };
            let Some(IsiPeta::Rapat { nilai, .. }) = Peta::isi_tunggal(m) else { unreachable!("diperiksa calon_pakai_ulang") };
            for (tempat, v) in nilai.iter_mut().zip(self.stack.drain(start..)) { *tempat = v; }
            self.stack.push(lama);
            return Ok(());
        }
        let nilai: Vec<VmValue> = self.stack.drain(start..).collect();
        self.stack.push(VmValue::Map(Peta::struktur_arc(label, bentuk, nilai)));
        Ok(())
    }

    /// Lokal bingkai (posisi `basis + p`, `p < batas`, delapan pertama) yang struct rapat berbentuk `bentuk`, berlabel
    /// `label`, dan milik tunggal: struct yang langsung dikembalikan boleh menimpanya — sesudah `Return` lokal itu
    /// tidak terlihat siapa pun. Satu definisi untuk VM (`op_build_struct`) dan JIT (`SitusBangun::StrukturUlang`).
    #[inline]
    pub(crate) fn calon_pakai_ulang(&mut self, basis: usize, batas: usize, label: &Label, bentuk: &std::sync::Arc<crate::vm::peta::Bentuk>) -> Option<usize> {
        let Label::Struct(nama) = label else { return None };
        (0..batas.min(8)).find(|&p| match self.stack.get_mut(basis + p) {
            // Label nilai dan situs biasanya berbagi `Arc` dari konstanta yang sama: alamatnya dulu.
            Some(VmValue::Map(m)) => matches!(&m.label, Label::Struct(n) if std::sync::Arc::ptr_eq(n, nama) || n == nama)
                && matches!(Peta::isi_tunggal(m), Some(IsiPeta::Rapat { bentuk: b, nilai })
                    if std::sync::Arc::ptr_eq(b, bentuk) && nilai.len() == bentuk.nama.len()),
            _ => false,
        })
    }

    /// Aritmetika dan banding atas operand yang bukan dua bulat: operator milik pemakai dulu, lalu
    /// aturan bawaan (`+` dengan teks lewat hook tampilan dan menyambung di tempat bila milik tunggal).
    #[inline(always)]
    pub(crate) fn op_biner(&mut self, op: OpCode, a: VmValue, b: VmValue, sp: Option<crate::lexer::Span>) -> VmResult<VmValue> {
        use crate::parser::BinaryOp as B;
        let bop = match op {
            OpCode::Add => B::Add, OpCode::Sub => B::Sub, OpCode::Mul => B::Mul, OpCode::Div => B::Div,
            OpCode::Mod => B::Rem, OpCode::Pow => B::Pow, OpCode::Eq => B::Eq, OpCode::Ne => B::Ne,
            OpCode::Lt => B::Lt, OpCode::Le => B::Le, OpCode::Gt => B::Gt, OpCode::Ge => B::Ge,
            OpCode::BitAnd => B::BitAnd, OpCode::BitOr => B::BitOr, OpCode::BitXor => B::BitXor,
            OpCode::Shl => B::Shl, OpCode::Shr => B::Shr,
            // `&^` tidak punya operator milik pemakai.
            OpCode::AndNot => return a.and_not(&b).map_err(|e| VmError::at(e, sp)),
            _ => return Err(VmError::at(format!("bug di jrgc: op_biner atas {op:?}"), sp)),
        };
        // Teks + teks: tidak ada operator milik pemakai (cuma struct/enum yang punya) dan tidak ada hook tampilan
        // (yang cuma mengubah sisi bukan-teks), jadi langsung disambung.
        if let (OpCode::Add, VmValue::Str(_), VmValue::Str(_)) = (op, &a, &b) {
            return a.tambah_milik(&b).map_err(|e| VmError::at(e, sp));
        }
        match self.operator_biner_pengguna(&bop, &a, &b) {
            Err(e) => return Err(if e.span.is_none() { VmError::at(e.msg, sp) } else { e }),
            Ok(Some(h)) => return Ok(h),
            Ok(None) => {}
        }
        let galat = |e| VmError::at(e, sp);
        match op {
            OpCode::Add => {
                // `"x" + nilai` adalah jalur TAMPILAN, jadi ia lewat hook seperti `wok`.
                let (a, b) = self.siapkan_gabung_string(a, b)
                    .map_err(|e| if e.span.is_none() { VmError::at(e.msg, sp) } else { e })?;
                a.tambah_milik(&b).map_err(galat)
            }
            OpCode::Sub => a.sub(&b).map_err(galat),
            OpCode::Mul => a.mul(&b).map_err(galat),
            OpCode::Div => a.div(&b).map_err(galat),
            OpCode::Mod => a.modulo(&b).map_err(galat),
            OpCode::Pow => a.pow(&b).map_err(galat),
            OpCode::Eq => a.vm_eq(&b).map(VmValue::Bool).map_err(galat),
            OpCode::Ne => a.vm_eq(&b).map(|x| VmValue::Bool(!x)).map_err(galat),
            OpCode::Lt => a.lt(&b).map(VmValue::Bool).map_err(galat),
            OpCode::Le => a.le(&b).map(VmValue::Bool).map_err(galat),
            OpCode::Gt => a.gt(&b).map(VmValue::Bool).map_err(galat),
            OpCode::Ge => a.ge(&b).map(VmValue::Bool).map_err(galat),
            OpCode::BitAnd => a.bit_and(&b).map_err(galat),
            OpCode::BitOr => a.bit_or(&b).map_err(galat),
            OpCode::BitXor => a.bit_xor(&b).map_err(galat),
            OpCode::Shl => a.shl(&b).map_err(galat),
            _ => a.shr(&b).map_err(galat),
        }
    }

    #[inline(always)]
    pub(crate) fn op_copy_val(&mut self, sp: Option<crate::lexer::Span>) -> VmResult<()> {
        // Lihat catatan pada `OpCode::CopyVal` di opcode.rs.
        let v = self.pop(sp)?;
        self.push(if v.bisa_beralias() { v.deep_copy() } else { v });
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn op_set_index(&mut self, sp: Option<crate::lexer::Span>) -> VmResult<()> {
        let val = self.pop(sp)?;
        let (obj, idx) = self.pop2(sp)?;
        self.set_index(&obj, &idx, val, sp)
    }

    #[inline(always)]
    pub(crate) fn op_typeof(&mut self, sp: Option<crate::lexer::Span>) -> VmResult<()> {
        let v = self.pop(sp)?;
        self.push(VmValue::str(v.type_name()));
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn op_len(&mut self, sp: Option<crate::lexer::Span>) -> VmResult<()> {
        let v = self.pop(sp)?;
        let n = self.vm_len(&v, sp)?;
        self.push(VmValue::Int(n as i128));
        Ok(())
    }

    /// `BuildArray` (`tuple` = `BuildTuple`): `n` nilai teratas jadi satu larik/tuple.
    #[inline(always)]
    pub(crate) fn op_build_larik(&mut self, n: u32, tuple: bool) {
        let start = self.stack.len().saturating_sub(n as usize);
        let items: Vec<VmValue> = self.stack.drain(start..).collect();
        self.push(if tuple { VmValue::tuple(items) } else { VmValue::array(items) });
    }
}
