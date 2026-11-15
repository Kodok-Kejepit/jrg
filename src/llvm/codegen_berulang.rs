// src/llvm/codegen_berulang.rs — tipe yang memuat dirinya lewat slice atau dict (`struct N { anak: []N }`).
// Tampil, `==` dan salin-dalamnya jadi satu fungsi per tipe yang memanggil dirinya saat jalan;
// penurunan sebaris atas tipe seperti itu tidak berujung dan menghabiskan stack `jrgc` sendiri.

use crate::lexer::Span;

use super::codegen::{BantuTipe, Codegen, KerjaFn, Val};
use super::types::JrgType;

impl Codegen {
    /// `Some(nama)` kalau `ty` struct/enum berulang yang harus lewat fungsi bantunya di sini.
    /// Di badan fungsi bantu itu sendiri tingkat pertamanya diturunkan sebaris, sekali.
    pub(crate) fn lewat_bantu(&mut self, ty: &JrgType) -> bool {
        let (JrgType::Struct(n) | JrgType::Enum(n)) = ty else { return false };
        if self.sebaris_sekali.as_deref() == Some(n.as_str()) {
            self.sebaris_sekali = None;
            return false;
        }
        self.types.berulang(n)
    }

    fn nama_bantu(jenis: BantuTipe, ty: &JrgType) -> String {
        let awal = match jenis { BantuTipe::Tampil => "tampil", BantuTipe::Sama => "sama", BantuTipe::Salin => "salin" };
        format!("{}.tipe.{}", awal, ty.mangle_frag())
    }

    fn simbol_bantu(&mut self, jenis: BantuTipe, ty: &JrgType) -> String {
        let nama = Self::nama_bantu(jenis, ty);
        if self.bantu_tipe_ada.insert(nama.clone()) {
            self.antre_fn(KerjaFn::BantuTipe { jenis, ty: ty.clone() });
        }
        super::mangle::llvm_quoted_ident(&nama)
    }

    /// Tampilan `v` lewat `@tampil.tipe.T(ptr)`.
    pub(crate) fn tampil_lewat_bantu(&mut self, v: &Val) -> Val {
        let f = self.simbol_bantu(BantuTipe::Tampil, &v.ty);
        let p = self.spill_to_place(v);
        let r = self.fresh("tbt.");
        self.emit(format!("{} = call %JrgStr @{}(ptr {})", r, f, p));
        Val { text: r, ty: JrgType::Str }
    }

    /// `l == r` lewat `@sama.tipe.T(ptr, ptr) -> i8`, di-AND-kan ke `slot`.
    pub(crate) fn sama_lewat_bantu(&mut self, slot: &str, ty: &JrgType, l: &str, r: &str) {
        let f = self.simbol_bantu(BantuTipe::Sama, ty);
        let lp = self.spill_to_place(&Val { text: l.to_string(), ty: ty.clone() });
        let rp = self.spill_to_place(&Val { text: r.to_string(), ty: ty.clone() });
        let h = self.fresh("sbt.");
        self.emit(format!("{} = call i8 @{}(ptr {}, ptr {})", h, f, lp, rp));
        self.dan_ke_slot(slot, &Val { text: h, ty: JrgType::Bool });
    }

    /// Salin-dalam `v` lewat `@salin.tipe.T(ptr)`, di tempat.
    pub(crate) fn salin_lewat_bantu(&mut self, v: &Val) -> Val {
        let f = self.simbol_bantu(BantuTipe::Salin, &v.ty);
        let p = self.spill_to_place(v);
        self.emit(format!("call void @{}(ptr {})", f, p));
        self.load_place(&p, &v.ty)
    }

    pub(crate) fn pancarkan_bantu_tipe(&mut self, jenis: BantuTipe, ty: &JrgType) -> Result<(), String> {
        let (JrgType::Struct(n) | JrgType::Enum(n)) = ty else { return Ok(()) };
        let nama = super::mangle::llvm_quoted_ident(&Self::nama_bantu(jenis, ty));
        let nol = Span::dummy();
        let lama = self.sebaris_sekali.replace(n.clone());
        let def = match jenis {
            BantuTipe::Tampil => {
                self.mulai_fungsi_kosong(JrgType::Str);
                let v = self.load_place("%p", ty);
                let s = self.build_display_str(&v)?;
                self.emit_return(&s, &JrgType::Str);
                format!("define internal %JrgStr @{}(ptr %p) {{\n{}}}\n", nama, self.take_function_body())
            }
            BantuTipe::Sama => {
                self.mulai_fungsi_kosong(JrgType::I8);
                let a = self.load_place("%a", ty);
                let b = self.load_place("%b", ty);
                let slot = self.alloca_raw("sbt.", "i8", Some(1));
                self.emit(format!("store i8 1, ptr {}", slot));
                self.kumpul_sama(&slot, ty, &a.text, &b.text, nol)?;
                let r = self.fresh("sbtr.");
                self.emit(format!("{} = load i8, ptr {}", r, slot));
                self.terminate(format!("ret i8 {}", r));
                format!("define internal i8 @{}(ptr %a, ptr %b) {{\n{}}}\n", nama, self.take_function_body())
            }
            BantuTipe::Salin => {
                self.mulai_fungsi_kosong(JrgType::Unit);
                let v = self.load_place("%p", ty);
                let baru = self.salin_dalam(&v);
                self.emit(format!("store {} {}, ptr %p", ty.llvm_ty(), baru.text));
                self.terminate("ret void".to_string());
                format!("define internal void @{}(ptr %p) {{\n{}}}\n", nama, self.take_function_body())
            }
        };
        self.sebaris_sekali = lama;
        self.tambah_definisi(def);
        self.selesai_fungsi_kosong();
        Ok(())
    }
}

// ─── Kotak: field yang menutup siklus sebaris, disimpan di heap ──────────

impl Codegen {
    /// Penunjuk isi kotak yang tersimpan di `slot`.
    pub(crate) fn isi_kotak(&mut self, slot: &str) -> String {
        let p = self.fresh("ktk.");
        self.emit(format!("{} = load ptr, ptr {}", p, slot));
        p
    }

    /// Simpan `v` ke tempat bertipe `ty`. Tempat `Kotak` mendapat kotak BARU berisi salinan
    /// dalam `v`, jadi tidak ada dua nilai yang berbagi kotak.
    pub(crate) fn simpan_slot(&mut self, dst: &str, ty: &JrgType, v: &Val) {
        if let JrgType::Kotak(t) = ty {
            let v = self.salin_dalam(v);
            return self.simpan_kotak_baru(dst, t, &v);
        }
        self.simpan_slot_mentah(dst, ty, v)
    }

    /// Seperti `simpan_slot`, tanpa menyalin `v` (yang sudah disalin pemanggilnya).
    pub(crate) fn simpan_slot_mentah(&mut self, dst: &str, ty: &JrgType, v: &Val) {
        match ty {
            JrgType::Unit => {}
            JrgType::Kotak(t) => { let t = (**t).clone(); self.simpan_kotak_baru(dst, &t, v) }
            _ => self.emit(format!("store {} {}, ptr {}", ty.llvm_ty(), v.text, dst)),
        }
    }

    /// `v` sebagai nilai `Kotak(t)`: penunjuk ke kotak baru berisi salinan dalamnya.
    pub(crate) fn kotakkan_nilai(&mut self, t: &JrgType, v: &Val) -> Val {
        let slot = self.alloca_raw("ktkn.", "ptr", Some(8));
        let ty = JrgType::Kotak(Box::new(t.clone()));
        self.simpan_slot(&slot, &ty, v);
        let r = self.fresh("ktkv.");
        self.emit(format!("{} = load ptr, ptr {}", r, slot));
        Val { text: r, ty }
    }

    fn simpan_kotak_baru(&mut self, dst: &str, t: &JrgType, v: &Val) {
        self.ensure_extern("declare ptr @jrg_alloc(i64, i64)", "jrg_alloc");
        let (n, a) = (self.types.sizeof(t).max(1), self.types.alignof(t).max(1));
        let p = self.fresh("ktkb.");
        self.emit(format!("{} = call ptr @jrg_alloc(i64 {}, i64 {})", p, n, a));
        self.simpan_slot_mentah(&p, t, v);
        self.emit(format!("store ptr {}, ptr {}", p, dst));
    }
}
