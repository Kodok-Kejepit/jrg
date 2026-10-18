// src/llvm/codegen_any.rs — llvm/codegen_any.rs — nilai bertag (`%JrgAny`).

use crate::lexer::Span;
use super::codegen::{Codegen, Val};
use super::types::JrgType;

/// Golongan yang bisa masuk kotak. Angkanya ikut dipancarkan ke IR, jadi ia
/// bagian dari kontrak antara pemasang dan pembuka kotak.
pub const TAG_NIL: i32 = 0;
pub const TAG_BOOL: i32 = 1;
pub const TAG_INT: i32 = 2;
pub const TAG_FLOAT: i32 = 3;
pub const TAG_RUNE: i32 = 4;
pub const TAG_STR: i32 = 5;
pub const TAG_SLICE: i32 = 6;
pub const TAG_DICT: i32 = 7;
pub const TAG_UNIT: i32 = 8;

/// Golongan sebuah tipe, kalau ia bisa dikotak.
/// Tipe yang isinya tidak dikenal kotak (struct, tuple, enum, fungsi, channel): muatannya
/// penunjuk ke salinan nilainya, dan 24 bit atas kata-tagnya nomor tipe itu (`Codegen::tipe_buram`).
pub const TAG_BURAM: i32 = 10;

pub fn tag_untuk(t: &JrgType) -> Option<i32> {
    Some(match t {
        JrgType::Nil => TAG_NIL,
        JrgType::Unit => TAG_UNIT,
        JrgType::Bool => TAG_BOOL,
        JrgType::Rune => TAG_RUNE,
        JrgType::Str => TAG_STR,
        JrgType::Slice(_) | JrgType::Array(_, _) => TAG_SLICE,
        JrgType::Dict(_) => TAG_DICT,
        JrgType::Struct(_) | JrgType::Enum(_) | JrgType::Fn(..) | JrgType::Chan(_) | JrgType::Masa(..) | JrgType::Sinkron(_) => TAG_BURAM,
        JrgType::Tuple(ts) if !ts.is_empty() => TAG_BURAM,
        t if t.is_float() => TAG_FLOAT,
        t if t.is_int() => TAG_INT,
        _ => return None,
    })
}

/// Lukisan sebuah tipe untuk isi kotak, satu nibble per tingkat (kode sama dengan `primitif.c`).
fn lukis(t: &JrgType) -> Option<(u32, u32)> {
    let satu = |k: u32| Some((k, 4));
    match t {
        JrgType::Any => satu(1),
        JrgType::I64 => satu(2),
        JrgType::F64 => satu(3),
        JrgType::Bool => satu(4),
        JrgType::Rune => satu(5),
        JrgType::Str => satu(6),
        JrgType::Slice(e) => lukis(e).map(|(l, n)| (7 | l << 4, n + 4)),
        JrgType::Dict(v) => lukis(v).map(|(l, n)| (8 | l << 4, n + 4)),
        JrgType::DictCampur(_) => Some((8 | 1 << 4, 8)),
        _ => None,
    }
}

/// Bentuk sebuah agregat di dalam kotak: array berukuran tetap di tingkat mana pun jadi slice.
pub fn luruh(t: &JrgType) -> JrgType {
    match t {
        JrgType::Array(e, _) | JrgType::Slice(e) => JrgType::Slice(Box::new(luruh(e))),
        JrgType::Dict(v) => JrgType::Dict(Box::new(luruh(v))),
        lain => lain.clone(),
    }
}

/// Untuk array/dict yang isinya bertipe buram: bentuk ber-isi nilai bertag-nya.
pub fn isi_buram(t: &JrgType) -> Option<JrgType> {
    match t {
        JrgType::Slice(e) | JrgType::Array(e, _) if tag_untuk(e) == Some(TAG_BURAM) =>
            Some(JrgType::Slice(Box::new(JrgType::Any))),
        JrgType::Dict(e) if tag_untuk(e) == Some(TAG_BURAM) => Some(JrgType::Dict(Box::new(JrgType::Any))),
        _ => None,
    }
}

/// Nama sebuah tipe agregat seperti yang ditulis pesan `jrg_any_ubah`.
pub fn nama_isi(t: &JrgType) -> String {
    match t {
        JrgType::Slice(e) | JrgType::Array(e, _) => format!("array<{}>", nama_isi(e)),
        JrgType::Dict(v) => format!("dict<{}>", nama_isi(v)),
        JrgType::DictCampur(_) => "dict<campur>".to_string(),
        JrgType::Any => "campur".to_string(),
        lain => lain.nama_typeof(),
    }
}

impl Codegen {
    /// Kata-tag sebuah array/dict: tag di 8 bit bawah, lukisan ISINYA di 24 bit atas. Isi
    /// yang tak terlukis (struct, tuple, lebar lain) memakai nomor daftar: cuma sama-persis yang cocok.
    pub(crate) fn kata_kotak(&mut self, t: &JrgType) -> Option<i32> {
        let (tag, isi) = match t {
            JrgType::Slice(e) | JrgType::Array(e, _) => (TAG_SLICE, (**e).clone()),
            JrgType::Dict(v) => (TAG_DICT, (**v).clone()),
            JrgType::DictCampur(_) => (TAG_DICT, JrgType::Any),
            _ => return None,
        };
        let bit = match lukis(&isi) {
            Some((l, n)) if n <= 24 => l,
            _ => {
                let frag = isi.mangle_frag();
                let no = match self.jenis_buram.iter().position(|x| *x == frag) {
                    Some(i) => i,
                    None => { self.jenis_buram.push(frag); self.jenis_buram.len() - 1 }
                };
                15 | ((no as u32 + 1) & 0xF_FFFF) << 4
            }
        };
        Some((tag as u32 | bit << 8) as i32)
    }

    /// Masukkan sebuah nilai ke dalam kotak bertag.
    pub fn kotak_any(&mut self, v: &Val, span: Span) -> Result<Val, String> {
        if v.ty == JrgType::Any { return Ok(v.clone()); }
        // Dict campur sama penyangganya dengan `dict<campur>`; yang hilang cuma tabel per-kuncinya.
        let samakan = samakan_dict(v);
        let v = &samakan;
        // Array berukuran tetap yang BERSARANG diluruhkan dulu, supaya isinya terlukis.
        let luruhan = luruh(&v.ty);
        let bersarang = matches!(&v.ty, JrgType::Array(e, _) | JrgType::Slice(e) | JrgType::Dict(e) if luruh(e) != **e);
        let diluruhkan;
        let v = if bersarang {
            diluruhkan = self.coerce(v, &luruhan);
            if diluruhkan.ty == luruhan { &diluruhkan } else { v }
        } else { v };
        // Array/dict ber-isi tipe buram masuk kotak sebagai ber-isi nilai bertag: tiap isinya
        // dikotakkan sendiri, jadi kotaknya tetap bisa dicetak dan dibuka per elemen.
        let dibungkus;
        let v = match isi_buram(&v.ty) {
            Some(tujuan) => {
                dibungkus = self.coerce(v, &tujuan);
                if dibungkus.ty == tujuan { &dibungkus } else { v }
            }
            None => v,
        };
        let tag = tag_untuk(&v.ty).ok_or_else(|| super::galat::tandai(span, format!(
            "nilai bertipe `{}` tidak bisa disimpan di tempat yang isinya campur tipe\n\
             bantuan: pisahkan jadi nilai-nilai bertipe sama, atau pakai struct\n\
             catatan: yang bisa dikotak: nil, bool, int, float, rune, string, \
             array, dan dict",
            v.ty.display_name())))?;

        // Slot dipakai supaya muatannya bisa ditulis lewat penunjuk yang
        // di-cast — `insertvalue` ke `[2 x i64]` akan menuntut penyalinan
        // bita per bita untuk setiap bentuk muatan.
        let slot = self.alloca_typed("anybox.", &JrgType::Any);
        let tp = self.fresh("anyt.");
        self.emit(format!("{} = getelementptr %JrgAny, ptr {}, i32 0, i32 0", tp, slot));
        let kata = if tag == TAG_BURAM { self.kata_buram(&v.ty) } else { self.kata_kotak(&v.ty).unwrap_or(tag) };
        self.emit(format!("store i32 {}, ptr {}", kata, tp));

        let mp = self.fresh("anym.");
        self.emit(format!("{} = getelementptr %JrgAny, ptr {}, i32 0, i32 1", mp, slot));
        // Muatannya dinolkan lebih dulu.
        self.emit(format!("store [2 x i64] zeroinitializer, ptr {}", mp));

        if tag == TAG_BURAM {
            // Nilainya disalin ke heap; kotaknya memegang penunjuknya.
            let uk = self.types.sizeof(&v.ty).max(1);
            let al = self.types.alignof(&v.ty).max(1);
            self.ensure_extern("declare ptr @jrg_alloc(i64, i64)", "jrg_alloc");
            let buf = self.fresh("anybur.");
            self.emit(format!("{} = call ptr @jrg_alloc(i64 {}, i64 {})", buf, uk, al));
            self.emit(format!("store {} {}, ptr {}", v.ty.llvm_ty(), v.text, buf));
            self.emit(format!("store ptr {}, ptr {}", buf, mp));
            return Ok(self.load_place(&slot, &JrgType::Any));
        }
        let nilai = match tag {
            TAG_INT => self.coerce_longgar(v, &JrgType::I64),
            TAG_FLOAT => self.coerce_longgar(v, &JrgType::F64),
            TAG_NIL | TAG_UNIT => Val { text: "0".into(), ty: JrgType::I64 },
            _ => v.clone(),
        };
        if !matches!(tag, TAG_NIL | TAG_UNIT) {
            let ty_simpan = match tag {
                TAG_SLICE => JrgType::Slice(Box::new(JrgType::I64)),
                _ => nilai.ty.clone(),
            };
            // Array tetap diluruhkan jadi slice lebih dulu: `[2 x i64]` panjangnya
            // bagian dari TIPENYA, dan kotak tidak menyimpan panjang di tagnya.
            let nilai = match &v.ty {
                JrgType::Array(elem, n) => {
                    let uk = self.types.sizeof(elem).max(1) * *n as u64;
                    let al = self.types.alignof(elem).max(1);
                    self.ensure_extern("declare ptr @jrg_alloc(i64, i64)", "jrg_alloc");
                    let buf = self.fresh("anyarr.");
                    self.emit(format!("{} = call ptr @jrg_alloc(i64 {}, i64 {})", buf, uk, al));
                    let src = self.alloca_typed("anysrc.", &v.ty);
                    self.emit(format!("store {} {}, ptr {}", v.ty.llvm_ty(), v.text, src));
                    self.ensure_extern(
                        "declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)",
                        "llvm.memcpy.p0.p0.i64");
                    self.emit(format!(
                        "call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)",
                        buf, src, uk));
                    self.rakit_slice(&buf, &n.to_string(), elem)
                }
                _ => nilai,
            };
            self.emit(format!("store {} {}, ptr {}", ty_simpan.llvm_ty(), nilai.text, mp));
        }
        Ok(self.load_place(&slot, &JrgType::Any))
    }

    /// Keluarkan nilai dari kotak, ke tipe yang sudah diketahui.
    pub fn buka_any(&mut self, v: &Val, tujuan: &JrgType, span: Span) -> Result<Val, String> {
        if *tujuan == JrgType::Any { return Ok(v.clone()); }
        tag_untuk(tujuan).ok_or_else(|| super::galat::tandai(span, format!(
            "tidak bisa membaca nilai bertipe `{}` dari tempat yang isinya campur tipe",
            tujuan.display_name())))?;
        // Array berukuran tetap: dibuka sebagai slice, lalu panjangnya harus persis.
        if let JrgType::Array(e, n) = tujuan {
            let s = self.buka_any(v, &JrgType::Slice(e.clone()), span)?;
            let (p, l) = self.bagian_koleksi_pub(&s, e);
            let beda = self.fresh("anyal.");
            self.emit(format!("{} = icmp ne i64 {}, {}", beda, l, n));
            let (gagal, lanjut) = (self.fresh_label("anyal.gagal."), self.fresh_label("anyal.ok."));
            self.terminate(format!("br i1 {}, label %{}, label %{}, !prof !0", beda, gagal, lanjut));
            self.start_block(gagal);
            self.panik(&format!(
                "array di tempat campur tipe tidak berisi tepat {} elemen, jadi tidak bisa mengisi tempat bertipe `{}`",
                n, tujuan.display_name()));
            self.start_block(lanjut);
            return Ok(self.load_place(&p, tujuan));
        }
        // Array ber-isi tipe buram: dibuka sebagai array bertag, lalu tiap elemennya dibuka.
        if let (Some(antara), JrgType::Slice(_)) = (isi_buram(tujuan), tujuan) {
            let s = self.buka_any(v, &antara, span)?;
            let hasil = self.coerce(&s, tujuan);
            if hasil.ty == *tujuan { return Ok(hasil); }
        }
        if let Some(kata) = self.kata_kotak(tujuan) {
            return Ok(self.buka_agregat(v, tujuan, kata, None));
        }
        if tag_untuk(tujuan) == Some(TAG_BURAM) { return Ok(self.buka_buram(v, tujuan)); }
        let slot = self.alloca_typed("anyunbox.", &JrgType::Any);
        self.emit(format!("store %JrgAny {}, ptr {}", v.text, slot));
        let mp = self.fresh("anym.");
        self.emit(format!("{} = getelementptr %JrgAny, ptr {}, i32 0, i32 1", mp, slot));
        match tujuan {
            JrgType::Nil => Ok(Val { text: "zeroinitializer".into(), ty: JrgType::Nil }),
            JrgType::Unit => Ok(Val::unit()),
            _ => {
                let lebar = match tag_untuk(tujuan) {
                    Some(TAG_INT) => JrgType::I64,
                    Some(TAG_FLOAT) => JrgType::F64,
                    _ => tujuan.clone(),
                };
                let r = self.fresh("anyv.");
                self.emit(format!("{} = load {}, ptr {}", r, lebar.llvm_ty(), mp));
                let v = Val { text: r, ty: lebar };
                Ok(self.coerce(&v, tujuan))
            }
        }
    }

    /// Kata-tag sebuah tipe buram: `TAG_BURAM` + nomor urutnya di `tipe_buram`.
    pub(crate) fn kata_buram(&mut self, t: &JrgType) -> i32 {
        let no = match self.tipe_buram.iter().position(|x| x == t) {
            Some(i) => i,
            None => { self.tipe_buram.push(t.clone()); self.tipe_buram.len() - 1 }
        };
        TAG_BURAM | ((no as i32 + 1) << 8)
    }

    /// Nilai bertipe buram keluar dari kotak: kata-tagnya harus PERSIS milik `tujuan`.
    fn buka_buram(&mut self, v: &Val, tujuan: &JrgType) -> Val {
        let kata = self.kata_buram(tujuan);
        let slot = self.alloca_typed("anybk.", &JrgType::Any);
        self.emit(format!("store %JrgAny {}, ptr {}", v.text, slot));
        let w = self.fresh("anyw.");
        self.emit(format!("{} = load i32, ptr {}", w, slot));
        let beda = self.fresh("anybb.");
        self.emit(format!("{} = icmp ne i32 {}, {}", beda, w, kata));
        let (gagal, lanjut) = (self.fresh_label("anybk.gagal."), self.fresh_label("anybk.ok."));
        self.terminate(format!("br i1 {}, label %{}, label %{}, !prof !0", beda, gagal, lanjut));
        self.start_block(gagal);
        let kotak = v.clone();
        let nama = tujuan.nama_typeof();
        self.panik_tipe_any(&kotak, |t| format!(
            "nilai bertipe `{}` tidak bisa mengisi tempat yang bertipe `{}` — di backend `--emit-llvm` \
             tipe sebuah tempat tetap (interpreter dan `--vm` menyimpan tipe bersama nilainya)", t, nama));
        self.start_block(lanjut);
        let mp = self.fresh("anym.");
        self.emit(format!("{} = getelementptr %JrgAny, ptr {}, i32 0, i32 1", mp, slot));
        let p = self.fresh("anybp.");
        self.emit(format!("{} = load ptr, ptr {}", p, mp));
        self.load_place(&p, tujuan)
    }

    /// Badan `@tampil.buram.N(ptr)`: tampilan nilai bertipe buram ke-N, lewat pencetak tipenya.
    pub(crate) fn pancarkan_tampil_buram(&mut self, no: usize) -> Result<(), String> {
        let t = self.tipe_buram[no].clone();
        self.mulai_fungsi_kosong(JrgType::Str);
        let v = self.load_place("%p", &t);
        let teks = self.build_display_str(&v)?;
        self.emit_return(&teks, &JrgType::Str);
        let badan = self.take_function_body();
        self.tambah_definisi(format!(
            "define internal %JrgStr @tampil.buram.{}(ptr %p) nounwind {{\n{}}}\n", no + 1, badan));
        self.selesai_fungsi_kosong();
        Ok(())
    }

    /// Badan `@sama.buram.N(ptr, ptr) -> i8`: `==` dua nilai bertipe buram ke-N. Tipe yang tidak
    /// punya kesamaan (fungsi, channel) gagal saat jalan, seperti di dua mesin lain.
    pub(crate) fn pancarkan_sama_buram(&mut self, no: usize) {
        let t = self.tipe_buram[no].clone();
        let nol = crate::lexer::Span { lo: crate::lexer::BytePos(0), hi: crate::lexer::BytePos(0), file_id: 0 };
        self.mulai_fungsi_kosong(JrgType::Bool);
        let (a, b) = (self.load_place("%a", &t), self.load_place("%b", &t));
        let slot = self.alloca_raw("sbur.", "i8", Some(1));
        self.emit(format!("store i8 1, ptr {}", slot));
        if self.kumpul_sama(&slot, &t, &a.text, &b.text, nol).is_err() {
            let _ = self.take_function_body();
            self.selesai_fungsi_kosong();
            self.mulai_fungsi_kosong(JrgType::Bool);
            let nama = t.nama_typeof();
            self.panik(&crate::interpreter::pesan_banding(&nama, &nama));
        } else {
            let r = self.fresh("sburr.");
            self.emit(format!("{} = load i8, ptr {}", r, slot));
            self.terminate(format!("ret i8 {}", r));
        }
        let badan = self.take_function_body();
        self.tambah_definisi(format!(
            "define internal i8 @sama.buram.{}(ptr %a, ptr %b) nounwind {{\n{}}}\n", no + 1, badan));
        self.selesai_fungsi_kosong();
    }

    /// `@jrg_sama_buram(kata, ptr, ptr)`: pemilih pembanding per nomor tipe buram.
    pub(crate) fn definisi_sama_buram(&self) -> String {
        let mut kepala = String::from("define internal i8 @jrg_sama_buram(i32 %w, ptr %a, ptr %b) {\nentry:\n  switch i32 %w, label %lain [");
        let mut badan = String::new();
        for i in 0..self.sama_terpancar {
            kepala.push_str(&format!(" i32 {}, label %b{}", TAG_BURAM | ((i as i32 + 1) << 8), i + 1));
            badan.push_str(&format!("b{0}:\n  %r{0} = call i8 @sama.buram.{0}(ptr %a, ptr %b)\n  ret i8 %r{0}\n", i + 1));
        }
        // `@jrg_buram_aritas(kata)`: jumlah elemen kalau tipenya tuple, 0 kalau bukan.
        let mut aritas = String::from("define internal i32 @jrg_buram_aritas(i32 %w) {\nentry:\n  switch i32 %w, label %lain [");
        let mut ba = String::new();
        for (i, t) in self.tipe_buram.iter().enumerate() {
            if let JrgType::Tuple(ts) = t {
                aritas.push_str(&format!(" i32 {}, label %b{}", TAG_BURAM | ((i as i32 + 1) << 8), i + 1));
                ba.push_str(&format!("b{}:\n  ret i32 {}\n", i + 1, ts.len()));
            }
        }
        format!("{} ]\n{}lain:\n  ret i8 0\n}}\n{} ]\n{}lain:\n  ret i32 0\n}}\n", kepala, badan, aritas, ba)
    }

    /// `@jrg_tampil_buram(kata, ptr)` dan `@jrg_typeof_buram(kata)`: pemilih per nomor tipe.
    pub(crate) fn definisi_buram(&self) -> String {
        let mut konst = String::new();
        let mut tampil = String::from("define internal %JrgStr @jrg_tampil_buram(i32 %w, ptr %p) cold {\nentry:\n  switch i32 %w, label %lain [");
        let mut nama = String::from("define internal %JrgStr @jrg_typeof_buram(i32 %w, ptr %p) cold {\nentry:\n  switch i32 %w, label %lain [");
        let (mut bt, mut bn) = (String::new(), String::new());
        let teks = |i: usize, t: &str| format!(
            "@.tbur.{} = private unnamed_addr constant [{} x i8] c\"{}\", align 1\n", i, t.len(), super::mangle::llvm_escape(t.as_bytes()));
        for (i, t) in self.tipe_buram.iter().enumerate() {
            let kata = TAG_BURAM | ((i as i32 + 1) << 8);
            let ty = t.nama_typeof();
            konst.push_str(&teks(i + 1, &ty));
            tampil.push_str(&format!(" i32 {}, label %b{}", kata, i + 1));
            nama.push_str(&format!(" i32 {}, label %b{}", kata, i + 1));
            if i < self.buram_terpancar {
                bt.push_str(&format!("b{0}:\n  %r{0} = call %JrgStr @tampil.buram.{0}(ptr %p)\n  ret %JrgStr %r{0}\n", i + 1));
            } else {
                bt.push_str(&format!("b{}:\n  br label %lain\n", i + 1));
            }
            // Fungsi bernama dan closure satu tipe statis: lingkungannya (null = bernama) yang membedakan.
            if matches!(t, JrgType::Fn(..)) {
                bn.push_str(&format!(
                    "b{0}:\n  %e{0} = getelementptr %JrgFn, ptr %p, i32 0, i32 1\n  %v{0} = load ptr, ptr %e{0}\n  \
                     %n{0} = icmp eq ptr %v{0}, null\n  %s{0} = select i1 %n{0}, %JrgStr {{ ptr @.tbur.{0}, i64 {1} }}, \
                     %JrgStr {{ ptr @.tbur.c, i64 7 }}\n  ret %JrgStr %s{0}\n", i + 1, ty.len()));
                continue;
            }
            bn.push_str(&format!("b{}:\n  ret %JrgStr {{ ptr @.tbur.{}, i64 {} }}\n", i + 1, i + 1, ty.len()));
        }
        konst.push_str(&teks(0, "<nilai>"));
        konst.push_str("@.tbur.c = private unnamed_addr constant [7 x i8] c\"closure\", align 1\n");
        let lain = "lain:\n  ret %JrgStr { ptr @.tbur.0, i64 7 }\n}\n";
        format!("{}{} ]\n{}{}{} ]\n{}{}", konst, tampil, bt, lain, nama, bn, lain)
    }

    /// Array/dict keluar dari kotak sebagai `tujuan`: kata-tag yang sama dibaca langsung, yang
    /// lain diubah `jrg_any_ubah` (O(n)) atau gagal saat jalan dengan menyebut tipe aslinya.
    pub(crate) fn buka_agregat(&mut self, v: &Val, tujuan: &JrgType, kata: i32, pola: Option<&str>) -> Val {
        let slot = self.alloca_typed("anyag.", &JrgType::Any);
        self.emit(format!("store %JrgAny {}, ptr {}", v.text, slot));
        let tp = self.fresh("anyt.");
        self.emit(format!("{} = getelementptr %JrgAny, ptr {}, i32 0, i32 0", tp, slot));
        let w = self.fresh("anyw.");
        self.emit(format!("{} = load i32, ptr {}", w, tp));
        let cocok = self.fresh("anyc.");
        self.emit(format!("{} = icmp eq i32 {}, {}", cocok, w, kata));
        let lty = tujuan.llvm_ty();
        let hasil = self.alloca_raw("anyh.", &lty, Some(8));
        let (cepat, lambat, gab) = (self.fresh_label("anyag.cepat."), self.fresh_label("anyag.ubah."), self.fresh_label("anyag.gab."));
        self.terminate(format!("br i1 {}, label %{}, label %{}, !prof !1", cocok, cepat, lambat));

        self.start_block(cepat);
        let mp = self.fresh("anym.");
        self.emit(format!("{} = getelementptr %JrgAny, ptr {}, i32 0, i32 1", mp, slot));
        let r = self.fresh("anyv.");
        self.emit(format!("{} = load {}, ptr {}", r, lty, mp));
        self.emit(format!("store {} {}, ptr {}", lty, r, hasil));
        self.terminate(format!("br label %{}", gab));

        self.start_block(lambat);
        let bawaan = format!(
            "nilai bertipe `\u{1}` tidak bisa mengisi tempat yang bertipe `{}` — di backend `--emit-llvm` \
             tipe sebuah tempat tetap (interpreter dan `--vm` menyimpan tipe bersama nilainya)", nama_isi(tujuan));
        let teks = self.intern_string(pola.unwrap_or(&bawaan));
        let (pp, pl) = self.split_str(&teks);
        let keluar = self.alloca_typed("anyu.", &JrgType::Any);
        self.ensure_extern("declare void @jrg_any_ubah(ptr, i32, ptr, ptr, i64)", "jrg_any_ubah");
        self.emit(format!("call void @jrg_any_ubah(ptr {}, i32 {}, ptr {}, ptr {}, i64 {})", slot, kata, keluar, pp, pl));
        let mp2 = self.fresh("anym.");
        self.emit(format!("{} = getelementptr %JrgAny, ptr {}, i32 0, i32 1", mp2, keluar));
        let r2 = self.fresh("anyv.");
        self.emit(format!("{} = load {}, ptr {}", r2, lty, mp2));
        self.emit(format!("store {} {}, ptr {}", lty, r2, hasil));
        self.terminate(format!("br label %{}", gab));

        self.start_block(gab);
        let h = self.fresh("anyhv.");
        self.emit(format!("{} = load {}, ptr {}", h, lty, hasil));
        Val { text: h, ty: tujuan.clone() }
    }

    /// (kata-tag penuh, penunjuk muatan) sebuah kotak bertipe buram.
    pub(crate) fn bagian_buram(&mut self, v: &Val) -> (String, String) {
        let slot = self.alloca_typed("anybg.", &JrgType::Any);
        self.emit(format!("store %JrgAny {}, ptr {}", v.text, slot));
        let w = self.fresh("anyw.");
        self.emit(format!("{} = load i32, ptr {}", w, slot));
        let mp = self.fresh("anym.");
        self.emit(format!("{} = getelementptr %JrgAny, ptr {}, i32 0, i32 1", mp, slot));
        let p = self.fresh("anybp.");
        self.emit(format!("{} = load ptr, ptr {}", p, mp));
        (w, p)
    }

    /// Tag sebuah kotak, sebagai `i32` — 8 bit bawah kata-tagnya.
    pub fn tag_any(&mut self, v: &Val) -> Val {
        let slot = self.alloca_typed("anytag.", &JrgType::Any);
        self.emit(format!("store %JrgAny {}, ptr {}", v.text, slot));
        let tp = self.fresh("anyt.");
        self.emit(format!("{} = getelementptr %JrgAny, ptr {}, i32 0, i32 0", tp, slot));
        let w = self.fresh("anytw.");
        self.emit(format!("{} = load i32, ptr {}", w, tp));
        let r = self.fresh("anytv.");
        self.emit(format!("{} = and i32 {}, 255", r, w));
        Val { text: r, ty: JrgType::I32 }
    }

    /// Badan `@jrg_tampil_kotak(ptr)`: array/dict di dalam kotak, dicetak lewat bentuk bertagnya.
    pub(crate) fn pancarkan_tampil_kotak(&mut self) -> Result<(), String> {
        self.mulai_fungsi_kosong(JrgType::Str);
        let nol = Span { lo: crate::lexer::BytePos(0), hi: crate::lexer::BytePos(0), file_id: 0 };
        let v = self.load_place("%k", &JrgType::Any);
        let tag = self.tag_any(&v);
        let d = self.fresh("tkd.");
        self.emit(format!("{} = icmp eq i32 {}, {}", d, tag.text, TAG_DICT));
        let (l_dict, l_arr) = (self.fresh_label("tk.dict."), self.fresh_label("tk.arr."));
        self.terminate(format!("br i1 {}, label %{}, label %{}", d, l_dict, l_arr));
        let pola = "mencetak array/dict berisi tipe ini (`\u{1}`) dari tempat campur tipe belum didukung backend --emit-llvm";
        for (label, tujuan) in [
            (l_arr, JrgType::Slice(Box::new(JrgType::Any))),
            (l_dict, JrgType::Dict(Box::new(JrgType::Any))),
        ] {
            self.start_block(label);
            let kata = self.kata_kotak(&tujuan).expect("array/dict bertag selalu punya kata-tag");
            let isi = self.buka_agregat(&v, &tujuan, kata, Some(pola));
            let teks = match &tujuan {
                JrgType::Dict(_) => self.tampilan_dict(&isi, &JrgType::Any)?,
                _ => self.build_display_str(&isi)?,
            };
            self.emit_return(&teks, &JrgType::Str);
        }
        let _ = nol;
        let badan = self.take_function_body();
        self.tambah_definisi(format!(
            "define internal %JrgStr @jrg_tampil_kotak(ptr %k) nounwind {{\n{}}}\n", badan));
        self.selesai_fungsi_kosong();
        Ok(())
    }
}

/// `DictCampur` dilihat sebagai dict biasa bernilai `Any`.
pub fn samakan_dict(v: &Val) -> Val {
    match &v.ty {
        JrgType::DictCampur(_) =>
            Val { text: v.text.clone(), ty: JrgType::Dict(Box::new(JrgType::Any)) },
        _ => v.clone(),
    }
}

/// Bentuk tipe yang sama, untuk tempat yang cuma memegang tipenya.
pub fn samakan_tipe(t: &JrgType) -> JrgType {
    match t {
        JrgType::DictCampur(_) => JrgType::Dict(Box::new(JrgType::Any)),
        _ => t.clone(),
    }
}

impl Codegen {
    /// Cetak isi sebuah kotak, dengan golongannya dibaca saat jalan.
    pub fn tampilan_any(&mut self, v: &Val, span: Span) -> Result<Val, String> {
        let tag = self.tag_any(v);
        let slot = self.alloca_typed("anystr.", &JrgType::Str);

        let l_nil = self.fresh_label("anyd.nil.");
        let l_bool = self.fresh_label("anyd.bool.");
        let l_int = self.fresh_label("anyd.int.");
        let l_flt = self.fresh_label("anyd.flt.");
        let l_rune = self.fresh_label("anyd.rune.");
        let l_str = self.fresh_label("anyd.str.");
        let l_tolak = self.fresh_label("anyd.tolak.");
        let l_unit = self.fresh_label("anyd.unit.");
        let l_agr = self.fresh_label("anyd.agr.");
        let l_bur = self.fresh_label("anyd.bur.");
        let l_gab = self.fresh_label("anyd.gab.");
        self.terminate(format!(
            "switch i32 {}, label %{} [ i32 {}, label %{} i32 {}, label %{} \
             i32 {}, label %{} i32 {}, label %{} i32 {}, label %{} i32 {}, label %{} \
             i32 {}, label %{} i32 {}, label %{} i32 {}, label %{} i32 {}, label %{} ]",
            tag.text, l_tolak,
            TAG_NIL, l_nil, TAG_BOOL, l_bool, TAG_INT, l_int,
            TAG_FLOAT, l_flt, TAG_RUNE, l_rune, TAG_STR, l_str,
            TAG_UNIT, l_unit, TAG_SLICE, l_agr, TAG_DICT, l_agr, TAG_BURAM, l_bur));

        for (lbl, ty) in [
            (l_nil, JrgType::Nil), (l_bool, JrgType::Bool), (l_int, JrgType::I64),
            (l_flt, JrgType::F64), (l_rune, JrgType::Rune), (l_str, JrgType::Str),
        ] {
            self.start_block(lbl);
            let isi = self.buka_any(v, &ty, span)?;
            let teks = self.build_display_str(&isi)?;
            self.emit(format!("store %JrgStr {}, ptr {}", teks.text, slot));
            self.terminate(format!("br label %{}", l_gab));
        }

        self.start_block(l_unit);
        let unit = self.intern_string("()");
        self.emit(format!("store %JrgStr {}, ptr {}", unit.text, slot));
        self.terminate(format!("br label %{}", l_gab));

        // Array dan dict dicetak di luar jalur: isinya bisa bersarang sedalam apa pun.
        self.start_block(l_agr);
        if !self.is_fn_emitted_or_queued("jrg_tampil_kotak") {
            self.tandai_fn_terpakai("jrg_tampil_kotak");
            self.antre_fn(super::codegen::KerjaFn::TampilKotak);
        }
        let tempat = self.alloca_typed("anyagp.", &JrgType::Any);
        self.emit(format!("store %JrgAny {}, ptr {}", v.text, tempat));
        let teks = self.fresh("anyagt.");
        self.emit(format!("{} = call %JrgStr @jrg_tampil_kotak(ptr {})", teks, tempat));
        self.emit(format!("store %JrgStr {}, ptr {}", teks, slot));
        self.terminate(format!("br label %{}", l_gab));

        // Tipe buram: pencetak per tipenya dipilih dari kata-tag (`@jrg_tampil_buram`).
        self.start_block(l_bur);
        let (w, p) = self.bagian_buram(v);
        self.pakai_buram = true;
        let tb = self.fresh("anybt.");
        self.emit(format!("{} = call %JrgStr @jrg_tampil_buram(i32 {}, ptr {})", tb, w, p));
        self.emit(format!("store %JrgStr {}, ptr {}", tb, slot));
        self.terminate(format!("br label %{}", l_gab));

        // Selain itu: nilai titipan milik `jrgc` (handle, struct) — tampilannya ditanyakan ke jembatan.
        self.start_block(l_tolak);
        let t = self.tanya_titipan(v, "::tampil");
        self.emit(format!("store %JrgStr {}, ptr {}", t.text, slot));
        self.terminate(format!("br label %{}", l_gab));

        self.start_block(l_gab);
        Ok(self.load_place(&slot, &JrgType::Str))
    }
}

impl Codegen {
    /// Isi sebuah kotak sebagai `f64`, dengan golongannya dibaca saat jalan.
    fn ke_f64_sesuai_tag(&mut self, v: &Val, tag: &Val, span: Span)
        -> Result<Val, String>
    {
        let adalah_int = self.fresh("f64ti.");
        self.emit(format!("{} = icmp eq i32 {}, {}", adalah_int, tag.text, TAG_INT));
        let slot = self.alloca_typed("f64s.", &JrgType::F64);
        let b_int = self.fresh_label("f64.int.");
        let b_flt = self.fresh_label("f64.flt.");
        let gab = self.fresh_label("f64.gab.");
        self.terminate(format!("br i1 {}, label %{}, label %{}", adalah_int, b_int, b_flt));

        self.start_block(b_int);
        let iv = self.buka_any(v, &JrgType::I64, span)?;
        let fv = self.coerce_longgar(&iv, &JrgType::F64);
        self.emit(format!("store double {}, ptr {}", fv.text, slot));
        self.terminate(format!("br label %{}", gab));

        self.start_block(b_flt);
        let fv2 = self.buka_any(v, &JrgType::F64, span)?;
        self.emit(format!("store double {}, ptr {}", fv2.text, slot));
        self.terminate(format!("br label %{}", gab));

        self.start_block(gab);
        Ok(self.load_place(&slot, &JrgType::F64))
    }

    /// `typeof` sebuah kotak — dibaca dari tagnya saat jalan.
    pub fn typeof_any(&mut self, v: &Val) -> Result<Val, String> {
        let tag = self.tag_any(v);
        let slot = self.alloca_typed("anyty.", &JrgType::Str);
        let gab = self.fresh_label("anyty.gab.");
        let bawaan = self.fresh_label("anyty.lain.");

        let golongan: Vec<(i32, JrgType)> = vec![
            (TAG_NIL, JrgType::Nil), (TAG_UNIT, JrgType::Unit),
            (TAG_BOOL, JrgType::Bool), (TAG_INT, JrgType::I64),
            (TAG_FLOAT, JrgType::F64), (TAG_RUNE, JrgType::Rune),
            (TAG_STR, JrgType::Str),
            (TAG_SLICE, JrgType::Slice(Box::new(JrgType::I64))),
            (TAG_DICT, JrgType::Dict(Box::new(JrgType::I64))),
        ];
        let label: Vec<String> = golongan.iter()
            .map(|(t, _)| self.fresh_label(&format!("anyty.{}.", t)))
            .collect();
        let kasus: Vec<String> = golongan.iter().zip(&label)
            .map(|((t, _), l)| format!("i32 {}, label %{}", t, l))
            .collect();
        self.terminate(format!(
            "switch i32 {}, label %{} [ {} ]", tag.text, bawaan, kasus.join(" ")));

        for ((_, ty), l) in golongan.iter().zip(&label) {
            self.start_block(l.clone());
            let nama = self.intern_string(&ty.nama_typeof());
            self.emit(format!("store %JrgStr {}, ptr {}", nama.text, slot));
            self.terminate(format!("br label %{}", gab));
        }

        // Tag di luar daftar hanya mungkin kalau kotaknya dipasang jalur yang
        // belum ada di sini — sebuah bug di kompilator ini, bukan di program
        // pemakainya. Digagalkan keras, bukan dijawab dengan nama yang salah.
        self.start_block(bawaan);
        // Tipe buram dijawab dari kata-tagnya; sisanya (titipan `jrgc`) ditanyakan ke jembatan.
        let adl = self.fresh("anytb.");
        self.emit(format!("{} = icmp eq i32 {}, {}", adl, tag.text, TAG_BURAM));
        let (l_b, l_t) = (self.fresh_label("anyty.bur."), self.fresh_label("anyty.titip."));
        self.terminate(format!("br i1 {}, label %{}, label %{}", adl, l_b, l_t));
        self.start_block(l_b);
        let (w, p) = self.bagian_buram(v);
        self.pakai_buram = true;
        let nb = self.fresh("anytn.");
        self.emit(format!("{} = call %JrgStr @jrg_typeof_buram(i32 {}, ptr {})", nb, w, p));
        self.emit(format!("store %JrgStr {}, ptr {}", nb, slot));
        self.terminate(format!("br label %{}", gab));
        self.start_block(l_t);
        let t = self.tanya_titipan(v, "::typeof");
        self.emit(format!("store %JrgStr {}, ptr {}", t.text, slot));
        self.terminate(format!("br label %{}", gab));

        self.start_block(gab);
        Ok(self.load_place(&slot, &JrgType::Str))
    }
}

impl Codegen {
    /// Operator aritmetika ketika salah satu sisinya sebuah kotak.
    pub fn arith_any(
        &mut self, op: &crate::parser::BinaryOp, lv: &Val, rv: &Val, span: Span,
    ) -> Result<Val, String> {
        // Sisi yang belum berupa kotak dikotak dulu, supaya cuma ada satu
        // bentuk yang perlu ditangani di bawah.
        let l = self.kotak_any(lv, span)?;
        let r = self.kotak_any(rv, span)?;
        let lt = self.tag_any(&l);
        let rt = self.tag_any(&r);

        let slot = self.alloca_typed("aany.", &JrgType::Any);
        let gab = self.fresh_label("aany.gab.");
        let l_str = self.fresh_label("aany.str.");
        let l_flt = self.fresh_label("aany.flt.");
        let l_int = self.fresh_label("aany.int.");
        let l_tolak = self.fresh_label("aany.tolak.");

        // String menang lebih dulu, di sisi mana pun ia berada: `"n=" + 5`
        // dan `5 + "=n"` dua-duanya penggabungan — diukur di acuannya.
        let ls = self.fresh("aanyls.");
        self.emit(format!("{} = icmp eq i32 {}, {}", ls, lt.text, TAG_STR));
        let rs = self.fresh("aanyrs.");
        self.emit(format!("{} = icmp eq i32 {}, {}", rs, rt.text, TAG_STR));
        let ada_str = self.fresh("aanyas.");
        self.emit(format!("{} = or i1 {}, {}", ada_str, ls, rs));

        // Float menang atas int, juga di sisi mana pun: `1 + 2.5` pecahan.
        let lf = self.fresh("aanylf.");
        self.emit(format!("{} = icmp eq i32 {}, {}", lf, lt.text, TAG_FLOAT));
        let rf = self.fresh("aanyrf.");
        self.emit(format!("{} = icmp eq i32 {}, {}", rf, rt.text, TAG_FLOAT));
        let ada_flt = self.fresh("aanyaf.");
        self.emit(format!("{} = or i1 {}, {}", ada_flt, lf, rf));

        // Bulat hanya kalau KEDUANYA bulat (bool dan rune bukan bilangan di
        // JRG — `'a' + 1` ditolak acuannya, jadi ia harus ditolak di sini
        // juga, bukan diam-diam dihitung sebagai angka).
        let li = self.fresh("aanyli.");
        self.emit(format!("{} = icmp eq i32 {}, {}", li, lt.text, TAG_INT));
        let ri = self.fresh("aanyri.");
        self.emit(format!("{} = icmp eq i32 {}, {}", ri, rt.text, TAG_INT));
        let dua_int = self.fresh("aanydi.");
        self.emit(format!("{} = and i1 {}, {}", dua_int, li, ri));

        let cek_flt = self.fresh_label("aany.cekflt.");
        let cek_int = self.fresh_label("aany.cekint.");
        self.terminate(format!("br i1 {}, label %{}, label %{}", ada_str, l_str, cek_flt));
        self.start_block(cek_flt);
        self.terminate(format!("br i1 {}, label %{}, label %{}", ada_flt, l_flt, cek_int));
        self.start_block(cek_int);
        self.terminate(format!("br i1 {}, label %{}, label %{}", dua_int, l_int, l_tolak));

        use crate::parser::BinaryOp;

        // ── Cabang STRING ──
        self.start_block(l_str);
        match op {
            BinaryOp::Add => {
                let lb = self.tampilan_any(&l, span)?;
                let rb = self.tampilan_any(&r, span)?;
                let h = self.str_concat(&lb, &rb)?;
                let k = self.kotak_any(&h, span)?;
                self.emit(format!("store %JrgAny {}, ptr {}", k.text, slot));
                self.terminate(format!("br label %{}", gab));
            }
            BinaryOp::Mul => {
                // `str * n` — sisi mana pun yang string. Yang bukan string
                // harus bulat; kalau bukan, itu kesalahan saat jalan.
                let kiri_str = self.fresh("aanyks.");
                self.emit(format!("{} = icmp eq i32 {}, {}", kiri_str, lt.text, TAG_STR));
                let bs = self.fresh_label("aany.mulok.");
                let bn = self.fresh_label("aany.multidak.");
                let ambil_kiri = self.fresh_label("aany.mulkiri.");
                let ambil_kanan = self.fresh_label("aany.mulkanan.");
                let slot_s = self.alloca_typed("aanyms.", &JrgType::Str);
                let slot_n = self.alloca_typed("aanymn.", &JrgType::I64);
                self.terminate(format!(
                    "br i1 {}, label %{}, label %{}", kiri_str, ambil_kiri, ambil_kanan));

                self.start_block(ambil_kiri);
                let sv = self.buka_any(&l, &JrgType::Str, span)?;
                let nt = self.fresh("aanymt.");
                self.emit(format!("{} = icmp eq i32 {}, {}", nt, rt.text, TAG_INT));
                self.emit(format!("store %JrgStr {}, ptr {}", sv.text, slot_s));
                let nv = self.buka_any(&r, &JrgType::I64, span)?;
                self.emit(format!("store i64 {}, ptr {}", nv.text, slot_n));
                self.terminate(format!("br i1 {}, label %{}, label %{}", nt, bs, bn));

                self.start_block(ambil_kanan);
                let sv2 = self.buka_any(&r, &JrgType::Str, span)?;
                let nt2 = self.fresh("aanymt.");
                self.emit(format!("{} = icmp eq i32 {}, {}", nt2, lt.text, TAG_INT));
                self.emit(format!("store %JrgStr {}, ptr {}", sv2.text, slot_s));
                let nv2 = self.buka_any(&l, &JrgType::I64, span)?;
                self.emit(format!("store i64 {}, ptr {}", nv2.text, slot_n));
                self.terminate(format!("br i1 {}, label %{}, label %{}", nt2, bs, bn));

                self.start_block(bs);
                let sv3 = self.load_place(&slot_s, &JrgType::Str);
                let nv3 = self.load_place(&slot_n, &JrgType::I64);
                let h = self.ulang_string(&sv3, &nv3)?;
                let k = self.kotak_any(&h, span)?;
                self.emit(format!("store %JrgAny {}, ptr {}", k.text, slot));
                self.terminate(format!("br label %{}", gab));

                self.start_block(bn);
                self.panik("`str * n` butuh n berupa bilangan bulat");
            }
            _ => self.panik(
                "operator ini tidak berarti apa-apa untuk string — yang ada \
                 cuma `+` (gabung) dan `*` (ulang)"),
        }

        // ── Cabang PECAHAN ──
        self.start_block(l_flt);
        let lb = self.ke_f64_sesuai_tag(&l, &lt, span)?;
        let rb = self.ke_f64_sesuai_tag(&r, &rt, span)?;
        let hasil = self.arith_binop(op, &lb, &rb, span)?;
        let kotak = self.kotak_any(&hasil, span)?;
        self.emit(format!("store %JrgAny {}, ptr {}", kotak.text, slot));
        self.terminate(format!("br label %{}", gab));

        // ── Cabang BULAT ──
        //
        // Dimasuki hanya kalau KEDUA tagnya bulat, jadi tidak ada konversi.
        self.start_block(l_int);
        let lb = self.buka_any(&l, &JrgType::I64, span)?;
        let rb = self.buka_any(&r, &JrgType::I64, span)?;
        let hasil = self.arith_binop(op, &lb, &rb, span)?;
        let kotak = self.kotak_any(&hasil, span)?;
        self.emit(format!("store %JrgAny {}, ptr {}", kotak.text, slot));
        self.terminate(format!("br label %{}", gab));

        self.start_block(l_tolak);
        self.panik(
            "operator aritmetika di sini dipakai pada nilai yang golongannya \
             tidak punya arti aritmetika (nil, unit, array, atau dict)");

        self.start_block(gab);
        Ok(self.load_place(&slot, &JrgType::Any))
    }


}

impl Codegen {
    /// `<` `<=` `>` `>=` ketika salah satu sisinya sebuah kotak.
    pub fn urut_any(
        &mut self, op: &crate::parser::BinaryOp, lv: &Val, rv: &Val, span: Span,
    ) -> Result<Val, String> {
        let l = self.kotak_any(lv, span)?;
        let r = self.kotak_any(rv, span)?;
        let lt = self.tag_any(&l);
        let rt = self.tag_any(&r);

        let slot = self.alloca_typed("uany.", &JrgType::Bool);
        let gab = self.fresh_label("uany.gab.");
        let b_str = self.fresh_label("uany.str.");
        let b_flt = self.fresh_label("uany.flt.");
        let b_int = self.fresh_label("uany.int.");
        let b_tolak = self.fresh_label("uany.tolak.");

        let ls = self.fresh("uanyls.");
        self.emit(format!("{} = icmp eq i32 {}, {}", ls, lt.text, TAG_STR));
        let rs = self.fresh("uanyrs.");
        self.emit(format!("{} = icmp eq i32 {}, {}", rs, rt.text, TAG_STR));
        let dua_str = self.fresh("uanyds.");
        self.emit(format!("{} = and i1 {}, {}", dua_str, ls, rs));

        let lf = self.fresh("uanylf.");
        self.emit(format!("{} = icmp eq i32 {}, {}", lf, lt.text, TAG_FLOAT));
        let rf = self.fresh("uanyrf.");
        self.emit(format!("{} = icmp eq i32 {}, {}", rf, rt.text, TAG_FLOAT));
        let ada_flt = self.fresh("uanyaf.");
        self.emit(format!("{} = or i1 {}, {}", ada_flt, lf, rf));
        let li = self.fresh("uanyli.");
        self.emit(format!("{} = icmp eq i32 {}, {}", li, lt.text, TAG_INT));
        let ri = self.fresh("uanyri.");
        self.emit(format!("{} = icmp eq i32 {}, {}", ri, rt.text, TAG_INT));
        let lnum = self.fresh("uanyln.");
        self.emit(format!("{} = or i1 {}, {}", lnum, lf, li));
        let rnum = self.fresh("uanyrn.");
        self.emit(format!("{} = or i1 {}, {}", rnum, rf, ri));
        let dua_num = self.fresh("uanydn.");
        self.emit(format!("{} = and i1 {}, {}", dua_num, lnum, rnum));
        let dua_int = self.fresh("uanydi.");
        self.emit(format!("{} = and i1 {}, {}", dua_int, li, ri));
        let pakai_flt = self.fresh("uanypf.");
        self.emit(format!("{} = and i1 {}, {}", pakai_flt, dua_num, ada_flt));

        let cek_num = self.fresh_label("uany.ceknum.");
        let cek_int = self.fresh_label("uany.cekint.");
        self.terminate(format!("br i1 {}, label %{}, label %{}", dua_str, b_str, cek_num));
        self.start_block(cek_num);
        self.terminate(format!("br i1 {}, label %{}, label %{}", pakai_flt, b_flt, cek_int));
        self.start_block(cek_int);
        self.terminate(format!("br i1 {}, label %{}, label %{}", dua_int, b_int, b_tolak));

        self.start_block(b_str);
        let lsv = self.buka_any(&l, &JrgType::Str, span)?;
        let rsv = self.buka_any(&r, &JrgType::Str, span)?;
        let h = self.cmp_binop_pub_entry(op, &lsv, &rsv, span)?;
        self.emit(format!("store i8 {}, ptr {}", h.text, slot));
        self.terminate(format!("br label %{}", gab));

        self.start_block(b_flt);
        let lfv = self.ke_f64_sesuai_tag(&l, &lt, span)?;
        let rfv = self.ke_f64_sesuai_tag(&r, &rt, span)?;
        let h = self.cmp_binop_pub_entry(op, &lfv, &rfv, span)?;
        self.emit(format!("store i8 {}, ptr {}", h.text, slot));
        self.terminate(format!("br label %{}", gab));

        self.start_block(b_int);
        let liv = self.buka_any(&l, &JrgType::I64, span)?;
        let riv = self.buka_any(&r, &JrgType::I64, span)?;
        let h = self.cmp_binop_pub_entry(op, &liv, &riv, span)?;
        self.emit(format!("store i8 {}, ptr {}", h.text, slot));
        self.terminate(format!("br label %{}", gab));

        self.start_block(b_tolak);
        self.panik(
            "nilai-nilai ini tidak sebanding, jadi tidak bisa diurutkan — \
             yang punya urutan cuma angka dengan angka, dan string dengan string");

        self.start_block(gab);
        Ok(self.load_place(&slot, &JrgType::Bool))
    }
}
