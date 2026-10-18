// src/llvm/codegen_sinkron.rs — `Atomic` dan `Mutex` di backend LLVM: pegangan ke keadaan di
// heap, jadi salinannya menunjuk keadaan yang sama. Kuncinya di `korutin.c`.

use crate::lexer::Span;
use crate::parser::*;

use super::codegen::{Codegen, Val};
use super::types::*;

impl Codegen {
    /// Argumen yang harus bilangan bulat; yang lain ditolak dengan kalimat `sinkron::pesan_bukan_int`.
    fn sinkron_bulat(&mut self, e: &Expr, siapa: &str, span: Span) -> Result<Val, String> {
        let v = self.codegen_expr(e, Some(&JrgType::I64))?;
        if !v.ty.is_int() {
            return Err(super::galat::tandai(span, crate::sinkron::pesan_bukan_int(siapa, &v.ty.nama_typeof())));
        }
        Ok(self.coerce_longgar(&v, &JrgType::I64))
    }

    fn sinkron_pegangan(&mut self, e: &Expr, kunci: bool, siapa: &str, span: Span) -> Result<Val, String> {
        let v = self.codegen_expr(e, None)?;
        if v.ty != JrgType::Sinkron(kunci) {
            let apa = if kunci { "Mutex" } else { "Atomic" };
            return Err(super::galat::tandai(span, crate::sinkron::pesan_bukan(siapa, apa, &v.ty.nama_typeof())));
        }
        Ok(v)
    }

    fn sinkron_aritas(nama: &str, dapat: usize, min: usize, maks: usize, span: Span) -> Result<(), String> {
        if dapat >= min && dapat <= maks { return Ok(()); }
        let butuh = if min == maks { format!("{}", maks) } else { format!("{} sampai {}", min, maks) };
        Err(super::galat::tandai(span, format!("`{}` butuh {} argumen, dapat {}", nama, butuh, dapat)))
    }

    /// `Atomic::new(n?)`, `Mutex::new()`, `atomic::*`, `mutex::*`.
    pub(crate) fn panggil_sinkron(&mut self, nama: &str, args: &[Expr], span: Span) -> Result<Val, String> {
        let i64v = |text: String| Val { text, ty: JrgType::I64 };
        match nama {
            "Atomic::new" => {
                Self::sinkron_aritas(nama, args.len(), 0, 1, span)?;
                let awal = match args.first() {
                    Some(e) => self.sinkron_bulat(e, nama, span)?,
                    None => i64v("0".into()),
                };
                self.ensure_extern("declare ptr @jrg_atom_baru(i64)", "jrg_atom_baru");
                let r = self.fresh("atom.");
                self.emit(format!("{} = call ptr @jrg_atom_baru(i64 {})", r, awal.text));
                Ok(Val { text: r, ty: JrgType::Sinkron(false) })
            }
            "Mutex::new" => {
                Self::sinkron_aritas(nama, args.len(), 0, 0, span)?;
                self.ensure_extern("declare ptr @jrg_kunci_baru()", "jrg_kunci_baru");
                let r = self.fresh("kunci.");
                self.emit(format!("{} = call ptr @jrg_kunci_baru()", r));
                Ok(Val { text: r, ty: JrgType::Sinkron(true) })
            }
            "atomic::get" => {
                Self::sinkron_aritas(nama, args.len(), 1, 1, span)?;
                let a = self.sinkron_pegangan(&args[0], false, nama, span)?;
                let r = self.fresh("atom.");
                self.emit(format!("{} = load i64, ptr {}", r, a.text));
                Ok(i64v(r))
            }
            "atomic::set" | "atomic::add" | "atomic::sub" => {
                Self::sinkron_aritas(nama, args.len(), 2, 2, span)?;
                let a = self.sinkron_pegangan(&args[0], false, nama, span)?;
                let n = self.sinkron_bulat(&args[1], nama, span)?;
                if nama == "atomic::set" {
                    self.emit(format!("store i64 {}, ptr {}", n.text, a.text));
                    return Ok(Val::unit());
                }
                let kini = self.fresh("atom.");
                self.emit(format!("{} = load i64, ptr {}", kini, a.text));
                let op = if nama == "atomic::add" { BinaryOp::Add } else { BinaryOp::Sub };
                let baru = self.hitung_terperiksa(&op, &i64v(kini), &n);
                self.emit(format!("store i64 {}, ptr {}", baru.text, a.text));
                Ok(baru)
            }
            "atomic::cas" => {
                Self::sinkron_aritas(nama, args.len(), 3, 3, span)?;
                let a = self.sinkron_pegangan(&args[0], false, nama, span)?;
                let harap = self.sinkron_bulat(&args[1], nama, span)?;
                let baru = self.sinkron_bulat(&args[2], nama, span)?;
                let (kini, sama, isi, r) = (self.fresh("atom."), self.fresh("atom."), self.fresh("atom."), self.fresh("atom."));
                self.emit(format!("{} = load i64, ptr {}", kini, a.text));
                self.emit(format!("{} = icmp eq i64 {}, {}", sama, kini, harap.text));
                self.emit(format!("{} = select i1 {}, i64 {}, i64 {}", isi, sama, baru.text, kini));
                self.emit(format!("store i64 {}, ptr {}", isi, a.text));
                self.emit(format!("{} = zext i1 {} to i8", r, sama));
                Ok(Val { text: r, ty: JrgType::Bool })
            }
            "mutex::lock" | "mutex::unlock" => {
                Self::sinkron_aritas(nama, args.len(), 1, 1, span)?;
                let m = self.sinkron_pegangan(&args[0], true, nama, span)?;
                self.kunci_atau_lepas(&m, nama, nama == "mutex::lock", false);
                Ok(Val::unit())
            }
            _ => {
                Self::sinkron_aritas(nama, args.len(), 2, 2, span)?;
                let m = self.sinkron_pegangan(&args[0], true, nama, span)?;
                let f = self.codegen_expr(&args[1], None)?;
                if !matches!(f.ty, JrgType::Fn(..)) {
                    return Err(super::galat::tandai(span, format!(
                        "nilai bertipe `{}` bukan fungsi, jadi tidak bisa dipanggil — pastikan yang dipanggil \
                         atau diserahkan memang fungsi atau closure", f.ty.nama_typeof())));
                }
                self.kunci_atau_lepas(&m, nama, true, true);
                let r = self.panggil_val(&f, &[]).map_err(|e| super::galat::tandai(span, e))?;
                self.kunci_atau_lepas(&m, nama, false, true);
                Ok(r)
            }
        }
    }

    /// `dijaga`: dikunci `mutex::with_lock`, jadi runtime melepasnya juga kalau badannya gagal.
    fn kunci_atau_lepas(&mut self, m: &Val, nama: &str, kunci: bool, dijaga: bool) {
        let s = self.intern_string(nama);
        let (p, l) = self.split_str(&s);
        if kunci {
            self.ensure_extern("declare void @jrg_kunci_kunci(ptr, i32, ptr, i64)", "jrg_kunci_kunci");
            self.emit(format!("call void @jrg_kunci_kunci(ptr {}, i32 {}, ptr {}, i64 {})", m.text, dijaga as i32, p, l));
        } else {
            self.ensure_extern("declare void @jrg_kunci_lepas(ptr, ptr, i64)", "jrg_kunci_lepas");
            self.emit(format!("call void @jrg_kunci_lepas(ptr {}, ptr {}, i64 {})", m.text, p, l));
        }
    }
}
