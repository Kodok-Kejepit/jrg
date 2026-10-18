// src/llvm/codegen_jembatan.rs — builtin yang implementasinya milik `jrgc` (dok::, crypto::,
// re::, …), dipanggil dari biner LLVM lewat pustaka dinamis `libjrgjembatan`.

use crate::lexer::Span;
use crate::parser::*;

use super::codegen::{Codegen, Val};
use super::types::*;

/// Namespace yang dilayani jembatan. Yang butuh memanggil BALIK fungsi JRG (`py::`, `js::`,
/// `php::`, `lua::`) tidak di sini: nilai fungsi tidak bisa menyeberang.
/// Kunci penanda di dict yang membawa struct milik program menyeberang (tidak bisa ditulis di JRG).
pub const TANDA_STRUCT: &str = "\u{1}struct";

/// `fs` di sini cuma mengisi celah: penurun aslinya (lewat C) dicoba lebih dulu.
const NS_JEMBATAN: &[&str] = &["dok", "crypto", "re", "regex", "http", "ptr", "proses", "term", "wokwok", "suara", "jawa", "jawir",
    "canvas", "Canvas", "fs"];

/// Bentuk hasil sebuah builtin, dibaca dari tanda tangan `--doc`-nya (`… -> Result<string>`).
fn bentuk_hasil(nama: &str) -> String {
    crate::bantu::jelaskan(nama)
        .and_then(|t| t.lines().next().map(|b| b.to_string()))
        .and_then(|b| b.rsplit_once("->").map(|(_, r)| r.trim().to_string()))
        .unwrap_or_default()
}

impl Codegen {
    /// `ns::item(args)` lewat jembatan; `None` kalau namanya bukan builtin yang dilayani.
    pub(crate) fn coba_jembatan(&mut self, ns: &str, item: &str, args: &[Expr], span: Span) -> Option<Result<Val, String>> {
        let nama = format!("{}::{}", ns, item);
        if !NS_JEMBATAN.contains(&ns) || !crate::vm::ada_builtin(&nama) { return None; }
        Some(self.panggil_jembatan(&nama, None, args, span))
    }

    /// Panggilan `extern "C"`: lewat jembatan ke `ffi::native`, jadi aturan dan kalimatnya satu
    /// definisi dengan dua mesin lain. Hasilnya selalu `Result`.
    pub(crate) fn panggil_extern(&mut self, nama: &str, deskripsi: &str, args: &[Expr], span: Span) -> Result<Val, String> {
        self.panggil_jembatan(nama, Some(deskripsi), args, span)
    }

    fn panggil_jembatan(&mut self, nama: &str, awalan: Option<&str>, args: &[Expr], span: Span) -> Result<Val, String> {
        super::PAKAI_JEMBATAN.store(true, std::sync::atomic::Ordering::Relaxed);
        let geser = usize::from(awalan.is_some());
        let n = args.len() + geser;
        let larik = self.alloca_raw("jbarg.", &format!("[{} x %JrgAny]", n.max(1)), Some(8));
        if let Some(teks) = awalan {
            let t = self.intern_string(teks);
            let k = self.kotak_any(&t, span)?;
            self.emit(format!("store %JrgAny {}, ptr {}", k.text, larik));
        }
        for (i, a) in args.iter().enumerate() {
            let v = self.codegen_expr(a, None)?;
            // Struct milik program menyeberang sebagai dict bertanda nama tipenya; `jrgc` di sisi
            // sana menyusunnya kembali jadi struct (`Warna` ke `canvas::fill_rect`).
            // Cuma struct yang DITULIS program; `Ptr` milik `jawa::` LLVM punya registry sendiri.
            let v = if matches!(&v.ty, JrgType::Struct(n) if self.tables.structs.contains_key(n)
                || self.tables.structs.contains_key(super::generik::dasar(n))) {
                let tempat = self.spill_to_place(&v);
                let e = self.lokal_sementara(tempat, v.ty.clone(), span);
                let ex = self.struct_ke_dict(&e, &v.ty, span);
                self.codegen_expr(&ex, None)?
            } else { v };
            let v = super::codegen_any::samakan_dict(&v);
            let buram = super::codegen_any::tag_untuk(&v.ty) == Some(super::codegen_any::TAG_BURAM);
            let ada_kotak = v.ty == JrgType::Any;
            let k = if buram { Err(String::new()) } else { self.kotak_any(&v, span) };
            let k = k.map_err(|_| super::galat::tandai(span, format!(
                "argumen ke-{} bertipe `{}` tidak bisa diserahkan ke `{}` dari backend `--emit-llvm` — yang bisa \
                 menyeberang: nil, bool, angka, teks, array, dict, dan nilai yang dijawab builtin lain", i + 1, v.ty.display_name(), nama)))?;
            // Kotak yang ternyata berisi struct milik program: diubah jadi dict bertanda saat jalan.
            let k = if ada_kotak {
                self.pakai_jemb_buram = true;
                self.pakai_buram = true;
                let r = self.fresh("jbk.");
                self.emit(format!("{} = call %JrgAny @jrg_buram_ke_jembatan(%JrgAny {})", r, k.text));
                Val { text: r, ty: JrgType::Any }
            } else { k };
            let p = self.fresh("jba.");
            self.emit(format!("{} = getelementptr %JrgAny, ptr {}, i64 {}", p, larik, i + geser));
            self.emit(format!("store %JrgAny {}, ptr {}", k.text, p));
        }
        let keluar = self.alloca_typed("jbo.", &JrgType::Any);
        let teks = self.intern_string(if awalan.is_some() { "::extern" } else { nama });
        let (np, nl) = self.split_str(&teks);
        self.ensure_extern("declare i32 @jrg_jembatan(ptr, i64, ptr, i64, ptr)", "jrg_jembatan");
        let r = self.fresh("jbr.");
        self.emit(format!("{} = call i32 @jrg_jembatan(ptr {}, i64 {}, ptr {}, i64 {}, ptr {})", r, np, nl, larik, n, keluar));

        let bentuk = if awalan.is_some() { "Result".to_string() } else { bentuk_hasil(nama) };
        let isi = self.lokal_sementara(keluar.clone(), JrgType::Any, span);
        if bentuk.starts_with("Result") || bentuk.starts_with("Option") {
            let hasil = bentuk.starts_with("Result");
            let harap = JrgType::Enum(if hasil { self.instantiate_result(&JrgType::Any, &JrgType::Any) }
                                      else { self.instantiate_option(&JrgType::Any) });
            let ada = self.construct_option_result(if hasil { "Ok" } else { "Some" }, &[isi.clone()], Some(&harap), span)?;
            let tak = if hasil { self.construct_option_result("Err", &[isi], Some(&harap), span)? }
                      else { self.construct_option_result("None", &[], Some(&harap), span)? };
            let c = self.fresh("jbc.");
            self.emit(format!("{} = icmp sge i32 {}, 2", c, r));
            let p = self.fresh("jbp.");
            self.emit(format!("{} = select i1 {}, {} {}, {} {}", p, c, harap.llvm_ty(), tak.text, harap.llvm_ty(), ada.text));
            return Ok(Val { text: p, ty: harap });
        }
        let v = self.load_place(&keluar, &JrgType::Any);
        let tujuan = match bentuk.as_str() {
            "bool" => JrgType::Bool,
            "int" => JrgType::I64,
            "float" => JrgType::F64,
            "string" => JrgType::Str,
            "array" => JrgType::Slice(Box::new(JrgType::Any)),
            "dict" => JrgType::Dict(Box::new(JrgType::Any)),
            "unit" => return Ok(Val::unit()),
            // Struct milik `jrgc` yang NAMANYA sama dengan struct milik program (`Warna` dari
            // `canvas::get_pixel`): disusun jadi struct program itu, seperti di dua mesin lain.
            t => {
                // Cuma struct yang DITULIS program (bukan tata letak internal seperti `Ptr` milik `jawa::`).
                let cocok: Vec<String> = self.tables.structs.keys()
                    .filter(|k| super::modul::tampil(k) == t && self.param_struct(k).is_none()
                        && self.types.structs.contains_key(*k)).cloned().collect();
                if let [kunci] = cocok.as_slice() {
                    let layout = self.types.structs.get(kunci).cloned().ok_or("struct hilang")?;
                    let sty = JrgType::Struct(kunci.clone());
                    let mut kini = "undef".to_string();
                    for (i, (f, ft)) in layout.fields.iter().enumerate() {
                        let x = self.field_titipan(&v, f, span)?;
                        let x = self.coerce(&x, ft);
                        let r = self.fresh("jbs.");
                        self.emit(format!("{} = insertvalue {} {}, {} {}, {}", r, sty.llvm_ty(), kini, ft.llvm_ty(), x.text, i));
                        kini = r;
                    }
                    return Ok(Val { text: kini, ty: sty });
                }
                return Ok(v);
            }
        };
        self.buka_terperiksa(&v, &tujuan).ok_or_else(|| "hasil jembatan tidak bisa dibuka".to_string())
    }

    /// `::dict::from_pairs([["\u{1}struct", "Nama"], ["f", x.f], ..])`, bersarang untuk field struct.
    fn struct_ke_dict(&self, e: &Expr, ty: &JrgType, span: Span) -> Expr {
        let JrgType::Struct(nama) = ty else { return e.clone() };
        let Some(layout) = self.types.structs.get(nama) else { return e.clone() };
        let teks = |t: &str| Expr::Literal(LitKind::Str(t.to_string()), span);
        let pasang = |k: Expr, v: Expr| Expr::Array { elems: vec![k, v], span };
        let mut isi = vec![pasang(teks(TANDA_STRUCT), teks(super::generik::dasar(nama)))];
        for (f, ft) in &layout.fields {
            let akses = Expr::Field { obj: Box::new(e.clone()), field: f.clone(), span };
            isi.push(pasang(teks(f), self.struct_ke_dict(&akses, ft, span)));
        }
        Expr::Call { func: Box::new(Expr::PathExpr { segments: vec![String::new(), "dict".into(), "from_pairs".into()], span }),
                     args: vec![Expr::Array { elems: isi, span }], span }
    }

    /// Badan `@jemb.buram.N(ptr) -> %JrgAny`: struct buram ke-N sebagai dict bertanda.
    pub(crate) fn pancarkan_buram_jembatan(&mut self, no: usize) -> Result<(), String> {
        let t = self.tipe_buram[no].clone();
        let nol = Span { lo: crate::lexer::BytePos(0), hi: crate::lexer::BytePos(0), file_id: 0 };
        self.mulai_fungsi_kosong(JrgType::Any);
        let k = if matches!(t, JrgType::Struct(_)) {
            let v = self.load_place("%p", &t);
            let tempat = self.spill_to_place(&v);
            let e = self.lokal_sementara(tempat, t.clone(), nol);
            let ex = self.struct_ke_dict(&e, &t, nol);
            let d = self.codegen_expr(&ex, None)?;
            let d = super::codegen_any::samakan_dict(&d);
            self.kotak_any(&d, nol)?
        } else {
            // Selain struct: kotaknya dikembalikan apa adanya, jembatan yang menolaknya.
            let kata = super::codegen_any::TAG_BURAM | ((no as i32 + 1) << 8);
            let (a, b) = (self.fresh("jbq."), self.fresh("jbq."));
            self.emit(format!("{} = insertvalue %JrgAny undef, i32 {}, 0", a, kata));
            let pi = self.fresh("jbq.");
            self.emit(format!("{} = ptrtoint ptr %p to i64", pi));
            self.emit(format!("{} = insertvalue %JrgAny {}, [2 x i64] [i64 0, i64 0], 1", b, a));
            let c = self.fresh("jbq.");
            self.emit(format!("{} = insertvalue %JrgAny {}, i64 {}, 1, 0", c, b, pi));
            Val { text: c, ty: JrgType::Any }
        };
        self.terminate(format!("ret %JrgAny {}", k.text));
        let badan = self.take_function_body();
        self.tambah_definisi(format!("define internal %JrgAny @jemb.buram.{}(ptr %p) {{\n{}}}\n", no + 1, badan));
        self.selesai_fungsi_kosong();
        Ok(())
    }

    /// `@jrg_buram_ke_jembatan(kotak)`: kotak bertipe buram lewat penyeberangnya, sisanya apa adanya.
    pub(crate) fn definisi_buram_jembatan(&self) -> String {
        let mut s = String::from("define internal %JrgAny @jrg_buram_ke_jembatan(%JrgAny %a) {\nentry:\n  %w = extractvalue %JrgAny %a, 0\n  %p0 = extractvalue %JrgAny %a, 1, 0\n  %p = inttoptr i64 %p0 to ptr\n  switch i32 %w, label %lain [");
        let mut badan = String::new();
        for i in 0..self.jemb_terpancar {
            s.push_str(&format!(" i32 {}, label %b{}", super::codegen_any::TAG_BURAM | ((i as i32 + 1) << 8), i + 1));
            badan.push_str(&format!("b{0}:\n  %r{0} = call %JrgAny @jemb.buram.{0}(ptr %p)\n  ret %JrgAny %r{0}\n", i + 1));
        }
        format!("{} ]\n{}lain:\n  ret %JrgAny %a\n}}\n", s, badan)
    }

    /// `x.f` atas nilai bertag: dibaca lewat jembatan (titipan milik `jrgc`, mis. `MousePos`).
    pub(crate) fn field_titipan(&mut self, v: &Val, field: &str, span: Span) -> Result<Val, String> {
        let t = self.intern_string(field);
        let tempat = self.spill_to_place(v);
        let a = self.lokal_sementara(tempat, JrgType::Any, span);
        let pt = self.spill_to_place(&t);
        let b = self.lokal_sementara(pt, JrgType::Str, span);
        self.panggil_jembatan("::field", None, &[a, b], span)
    }

    /// Tampilan (`::tampil`) atau nama tipe (`::typeof`) sebuah nilai titipan milik `jrgc`.
    pub(crate) fn tanya_titipan(&mut self, v: &Val, apa: &str) -> Val {
        // TIDAK menandai jembatan terpakai: nilai titipan cuma bisa lahir dari panggilan jembatan,
        // dan yang itu sudah menandainya. Program lain yang mencetak nilai bertag tidak ikut menautkannya.
        let tempat = self.spill_to_place(v);
        let keluar = self.alloca_typed("jbt.", &JrgType::Any);
        let teks = self.intern_string(apa);
        let (np, nl) = self.split_str(&teks);
        self.ensure_extern("declare i32 @jrg_jembatan(ptr, i64, ptr, i64, ptr)", "jrg_jembatan");
        let r = self.fresh("jbr.");
        self.emit(format!("{} = call i32 @jrg_jembatan(ptr {}, i64 {}, ptr {}, i64 1, ptr {})", r, np, nl, tempat, keluar));
        let mp = self.fresh("jbm.");
        self.emit(format!("{} = getelementptr %JrgAny, ptr {}, i32 0, i32 1", mp, keluar));
        self.load_place(&mp, &JrgType::Str)
    }
}
