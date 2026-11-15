// src/llvm/codegen_keluarga.rs — pola varian enum GENERIK (`Some(v)`, `Ok(x)`, `E::A(t)`) atas nilai bertag.
// Isi kotak diubah saat jalan ke instans `F<any, ..>` lewat `@kel.F`; pengirimnya dirakit sesudah daftar tipe
// buram lengkap, jadi instans yang baru terlihat belakangan tidak pernah jatuh ke `_` diam-diam.

use crate::parser::*;

use super::codegen::Codegen;
use super::codegen_any::TAG_BURAM;
use super::types::{EnumPayload, JrgType};

/// Satu keluarga yang dicocokkan atas nilai bertag: pengubahnya dipancarkan per tipe buram.
#[derive(Clone, Debug)]
pub(crate) struct Keluarga {
    dasar: String,
    sasaran: String,
    terpancar: usize,
    ada: Vec<usize>,
}

fn simbol(dasar: &str) -> String {
    let aman: String = dasar.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    format!("kel.{}.{}", aman, dasar.len())
}

impl Codegen {
    /// Nama dasar keluarga generik sebuah pola varian, kalau ia generik (`Some` → `Option`).
    fn dasar_keluarga(&self, pty: &TypeExpr, varian: &str) -> Option<String> {
        let TypeExpr::Named(nt) = pty else { return None };
        let d = match nt.package.clone() {
            Some(p) => p,
            None => match varian {
                "Some" | "None" => "Option".to_string(),
                "Ok" | "Err" => "Result".to_string(),
                _ => {
                    let mut calon = self.tables.enums.iter()
                        .filter(|(_, e)| !e.generics.params.is_empty() && e.variants.iter().any(|v| v.name == varian))
                        .map(|(n, _)| n.clone());
                    let satu = calon.next()?;
                    if calon.next().is_some() { return None; }
                    satu
                }
            },
        };
        if matches!(d.as_str(), "Option" | "Result") { return Some(d); }
        [d.clone(), self.k(&d)].into_iter().find(|n| self.param_enum(n).is_some())
    }

    /// Instans `F<any, ..>` untuk pola berkeluarga generik, plus nama pengubahnya.
    pub(crate) fn keluarga_pola(&mut self, pty: &TypeExpr, varian: &str) -> Option<(JrgType, String)> {
        let d = self.dasar_keluarga(pty, varian)?;
        self.keluarga_dari_dasar(d)
    }

    /// Instans `F<any, ..>` untuk keluarga `d`, plus nama pengubahnya (didaftar sekali).
    pub(crate) fn keluarga_dari_dasar(&mut self, d: String) -> Option<(JrgType, String)> {
        let sasaran = match d.as_str() {
            "Option" => self.instantiate_option(&JrgType::Any),
            "Result" => self.instantiate_result(&JrgType::Any, &JrgType::Any),
            _ => {
                let n = self.param_enum(&d)?.len();
                self.instantiate_enum(&d, &vec![JrgType::Any; n]).ok()?
            }
        };
        if !self.keluarga_bertag.iter().any(|k| k.dasar == d) {
            self.keluarga_bertag.push(Keluarga { dasar: d.clone(), sasaran: sasaran.clone(), terpancar: 0, ada: Vec::new() });
        }
        Some((JrgType::Enum(sasaran), simbol(&d)))
    }

    pub(crate) fn ada_keluarga_tertunda(&self) -> bool {
        self.keluarga_bertag.iter().any(|k| k.terpancar < self.tipe_buram.len())
    }

    /// Satu langkah: pengubah satu tipe buram lagi untuk keluarga pertama yang belum menimbangnya.
    pub(crate) fn pancarkan_keluarga(&mut self) -> Result<(), String> {
        let Some(j) = self.keluarga_bertag.iter().position(|k| k.terpancar < self.tipe_buram.len()) else { return Ok(()) };
        let kel = self.keluarga_bertag[j].clone();
        let no = kel.terpancar;
        self.keluarga_bertag[j].terpancar += 1;
        let t = self.tipe_buram[no].clone();
        let JrgType::Enum(asal) = &t else { return Ok(()) };
        if super::generik::dasar(asal) != super::generik::dasar(&kel.dasar) { return Ok(()); }
        let asal_l = self.types.enums.get(asal).cloned().ok_or("enum asal hilang")?;
        let tuju_l = self.types.enums.get(&kel.sasaran).cloned().ok_or("enum sasaran hilang")?;
        let tuju_ty = JrgType::Enum(kel.sasaran.clone());
        let nol = crate::lexer::Span { lo: crate::lexer::BytePos(0), hi: crate::lexer::BytePos(0), file_id: 0 };
        self.mulai_fungsi_kosong(JrgType::Unit);
        let tag = self.get_tag("%p", &t);
        let label: Vec<String> = asal_l.variants.iter().map(|_| self.fresh_label("kel.v.")).collect();
        let kasus: Vec<String> = asal_l.variants.iter().zip(&label).map(|(v, l)| format!("i32 {}, label %{}", v.tag, l)).collect();
        let mustahil = self.fresh_label("kel.x.");
        self.terminate(format!("switch i32 {}, label %{} [ {} ]", tag, mustahil, kasus.join(" ")));
        for (va, l) in asal_l.variants.iter().zip(label) {
            self.start_block(l);
            let vt = tuju_l.variant(&va.name).cloned().ok_or("varian sasaran hilang")?;
            let baru = if matches!(va.payload, EnumPayload::Unit) {
                self.build_enum_unit_variant(&kel.sasaran, &va.name)?
            } else {
                let (dari, ke) = (va.payload.field_types(), vt.payload.field_types());
                let mut isi = Vec::with_capacity(dari.len());
                for (i, (fa, ft)) in dari.iter().zip(&ke).enumerate() {
                    let x = self.get_payload_field("%p", &t, &va.payload.as_llvm_struct(), i, fa);
                    let y = if *ft == JrgType::Any { self.kotak_any(&x, nol)? } else { self.coerce(&x, ft) };
                    if y.ty != *ft {
                        return Err(format!("pola `{}` atas nilai bertag berisi `{}` belum didukung backend `--emit-llvm` \
                            — ini celah backend, bukan salah programmu", va.name, super::modul::tampil(asal)));
                    }
                    isi.push(y);
                }
                self.build_enum_value(&kel.sasaran, &tuju_l, &vt, &isi)?
            };
            self.emit(format!("store {} {}, ptr %o", tuju_ty.llvm_ty(), baru.text));
            self.terminate("ret void".to_string());
        }
        self.start_block(mustahil);
        self.terminate("unreachable".to_string());
        let badan = self.take_function_body();
        self.tambah_definisi(format!("define internal void @{}.{}(ptr %p, ptr %o) {{\n{}}}\n", simbol(&kel.dasar), no + 1, badan));
        self.selesai_fungsi_kosong();
        self.keluarga_bertag[j].ada.push(no);
        Ok(())
    }

    /// `@kel.F(kotak, out) -> i1`: `true` dan `out` terisi kalau isi kotaknya anggota keluarga `F`.
    pub(crate) fn definisi_keluarga(&self) -> String {
        let mut out = String::new();
        for k in &self.keluarga_bertag {
            let s = simbol(&k.dasar);
            let mut kepala = format!("define internal i1 @{}(%JrgAny %k, ptr %o) {{\nentry:\n  %w = extractvalue %JrgAny %k, 0\n  \
                %p0 = extractvalue %JrgAny %k, 1, 0\n  %p = inttoptr i64 %p0 to ptr\n  switch i32 %w, label %tidak [", s);
            let mut badan = String::new();
            for no in &k.ada {
                let kata = TAG_BURAM | ((*no as i32 + 1) << 8);
                kepala.push_str(&format!(" i32 {}, label %b{}", kata, no + 1));
                badan.push_str(&format!("b{0}:\n  call void @{1}.{0}(ptr %p, ptr %o)\n  ret i1 true\n", no + 1, s));
            }
            out.push_str(&format!("{} ]\n{}tidak:\n  ret i1 false\n}}\n", kepala, badan));
        }
        out
    }
}

impl Codegen {
    /// `x.m(..)` atas nilai bertag yang mungkin `Option`/`Result`: isinya diubah ke `F<any, ..>` lewat
    /// `@kel.F` dan method-nya diturunkan seperti biasa; isi lain lewat `lain` (jalur bertag yang lama).
    pub(crate) fn metode_keluarga_bertag(
        &mut self, v: &super::codegen::Val, nama: &str, args: &[Expr], span: crate::lexer::Span,
        lain: impl FnOnce(&mut Self) -> Result<super::codegen::Val, String>,
    ) -> Result<super::codegen::Val, String> {
        let keluarga: Vec<&str> = ["Option", "Result"].into_iter()
            .filter(|f| crate::vm::ada_builtin(&format!("{}::{}", f, nama))).collect();
        if keluarga.is_empty() { return lain(self); }
        let slot = self.alloca_typed("mkb.", &JrgType::Any);
        let akhir = self.fresh_label("mkb.end.");
        for f in keluarga {
            let Some((ty, sym)) = self.keluarga_dari_dasar(f.to_string()) else { continue };
            let o = self.alloca_typed("mkb.", &ty);
            let c = self.fresh("mkb.");
            self.emit(format!("{} = call i1 @{}(%JrgAny {}, ptr {})", c, sym, v.text, o));
            let (ya, terus) = (self.fresh_label("mkb.ya."), self.fresh_label("mkb.b."));
            self.terminate(format!("br i1 {}, label %{}, label %{}", c, ya, terus));
            self.start_block(ya);
            let penerima = self.lokal_sementara(o, ty.clone(), span);
            let r = self.codegen_method(&penerima, nama, args, None, span)?;
            if !self.is_terminated() {
                let r = if matches!(r.ty, JrgType::Unit | JrgType::Never) { super::codegen::Val::unit() } else { r };
                let k = self.coerce(&r, &JrgType::Any);
                self.emit(format!("store %JrgAny {}, ptr {}", k.text, slot));
                self.terminate(format!("br label %{}", akhir));
            }
            self.start_block(terus);
        }
        let r = lain(self)?;
        if !self.is_terminated() {
            let k = self.coerce(&r, &JrgType::Any);
            self.emit(format!("store %JrgAny {}, ptr {}", k.text, slot));
            self.terminate(format!("br label %{}", akhir));
        }
        self.start_block(akhir);
        Ok(self.load_place(&slot, &JrgType::Any))
    }
}
