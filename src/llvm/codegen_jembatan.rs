// src/llvm/codegen_jembatan.rs — builtin yang implementasinya milik `jrgc` (dok::, crypto::,
// re::, …), dipanggil dari biner LLVM lewat pustaka dinamis `libjrgjembatan`.

use crate::lexer::Span;
use crate::parser::*;

use super::codegen::{Codegen, Val};
use super::types::*;

/// Kunci penanda di dict yang membawa struct milik program menyeberang (tidak bisa ditulis di JRG).
pub const TANDA_STRUCT: &str = "\u{1}struct";
/// Penanda tuple milik program yang menyeberang sebagai dict berkunci `0`, `1`, ….
pub const TANDA_TUPLE: &str = "\u{1}tuple";
/// Penanda enum milik program: nama tipe, nama varian, dan muatannya (bentuk muatan VM).
pub const TANDA_ENUM: &str = "\u{1}enum";
pub const TANDA_VARIAN: &str = "\u{1}varian";
pub const TANDA_ISI: &str = "\u{1}isi";

/// Wadah yang isinya bisa berupa nilai buram (struct/tuple milik program) saat jalan.
pub(crate) fn mengandung_buram(t: &JrgType) -> bool {
    match t {
        JrgType::Array(e, _) | JrgType::Slice(e) | JrgType::Dict(e) => **e == JrgType::Any
            || super::codegen_any::tag_untuk(e) == Some(super::codegen_any::TAG_BURAM) || mengandung_buram(e),
        _ => false,
    }
}

/// Namespace yang dilayani jembatan. `fs` di sini cuma mengisi celah: penurun aslinya (lewat C)
/// dicoba lebih dulu. `jawa::`/`jawir::` TIDAK: registry-nya milik runtime biner ini, bukan `jrgc`.
const NS_JEMBATAN: &[&str] = &["dok", "crypto", "re", "regex", "http", "ptr", "proses", "term", "wokwok", "suara",
    "canvas", "Canvas", "fs", "py", "js", "php", "lua", "zip", "xml", "jwt", "gzip", "tar", "url", "toml", "semver"];

/// Apakah builtin namespace `ns` dilayani jembatan (hasilnya nilai bertag)?
pub(crate) fn ns_jembatan(ns: &str) -> bool { NS_JEMBATAN.contains(&ns) }

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
        // Hitungan handle hidup tidak bisa dijawab jujur: biner LLVM tidak pernah melepasnya.
        if item == "refs" && matches!(ns, "py" | "js" | "php") {
            return Some(Err(super::galat::tandai(span, format!(
                "`{}` tidak bisa dijawab jujur di backend `--emit-llvm`: handle di sana tidak pernah dilepas \
                 (biner itu tidak punya destruktor), jadi hitungannya cuma naik — ini celah backend, bukan salah \
                 programmu\nbantuan: jalankan tanpa `--emit-llvm` kalau jumlah handle hidup yang mau diperiksa", nama))));
        }
        Some(self.panggil_jembatan(&nama, None, None, args, span))
    }

    /// Panggilan `extern "C"`: lewat jembatan ke `ffi::native`, jadi aturan dan kalimatnya satu
    /// definisi dengan dua mesin lain. Hasilnya selalu `Result`.
    pub(crate) fn panggil_extern(&mut self, nama: &str, deskripsi: &str, args: &[Expr], span: Span) -> Result<Val, String> {
        self.panggil_jembatan(nama, Some(("::extern", deskripsi)), Some("Result".into()), args, span)
    }

    /// Pintu jembatan bernama khusus (`::jawa_stats`) dengan bentuk hasil yang sudah diketahui.
    pub(crate) fn panggil_jembatan_pintu(&mut self, pintu: &str, bentuk: &str, args: &[Expr], span: Span) -> Result<Val, String> {
        self.panggil_jembatan(pintu, None, Some(bentuk.to_string()), args, span)
    }

    /// `x.m(args)` atas nilai bertag: lewat jembatan ke `vm::panggil_metode`. Bentuk hasilnya
    /// milik `canvas::m` kalau ada, selain itu `Result` (method handle asing); diperiksa saat jalan.
    pub(crate) fn metode_titipan(&mut self, v: &Val, metode: &str, args: &[Expr], span: Span) -> Result<Val, String> {
        let tempat = self.spill_to_place(v);
        let mut semua = vec![self.lokal_sementara(tempat, JrgType::Any, span)];
        semua.extend(args.iter().cloned());
        let kanvas = format!("canvas::{}", metode);
        let bentuk = if crate::vm::ada_builtin(&kanvas) { bentuk_hasil(&kanvas) } else { "Result".to_string() };
        self.panggil_jembatan(&format!(".{}()", metode), Some(("::metode", metode)), Some(bentuk), &semua, span)
    }

    pub(crate) fn panggil_jembatan(&mut self, nama: &str, awalan: Option<(&str, &str)>, bentuk: Option<String>,
                        args: &[Expr], span: Span) -> Result<Val, String> {
        super::PAKAI_JEMBATAN.store(true, std::sync::atomic::Ordering::Relaxed);
        let geser = usize::from(awalan.is_some());
        let n = args.len() + geser;
        let larik = self.alloca_raw("jbarg.", &format!("[{} x %JrgAny]", n.max(1)), Some(8));
        if let Some((_, teks)) = awalan {
            let t = self.intern_string(teks);
            let k = self.kotak_any(&t, span)?;
            self.emit(format!("store %JrgAny {}, ptr {}", k.text, larik));
        }
        for (i, a) in args.iter().enumerate() {
            // Closure tanpa anotasi: argumennya datang bertag dari `jrgc`, hasilnya pulang bertag.
            let harap = match a {
                Expr::Closure { params, ret: None, .. } if params.iter().all(|p| matches!(p.ty, TypeExpr::Inferred)) => Some(JrgType::Fn(vec![JrgType::Any; params.len()], Box::new(JrgType::Any))),
                _ => None,
            };
            let v = self.codegen_expr(a, harap.as_ref())?;
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
            // Nilai fungsi: menyeberang sebagai kotak fungsi (handler `dok::serve` dititipkan jembatan ke thread biner).
            if matches!(v.ty, JrgType::Fn(..)) {
                let k = self.kotak_fungsi_jembatan(&v);
                let p = self.fresh("jba.");
                self.emit(format!("{} = getelementptr %JrgAny, ptr {}, i64 {}", p, larik, i + geser));
                self.emit(format!("store %JrgAny {}, ptr {}", k.text, p));
                continue;
            }
            // `[nil, nil]` dan sejenisnya: elemennya tak terlukis, jadi diserahkan sebagai array bertag.
            let v = match &v.ty {
                JrgType::Array(e, _) | JrgType::Slice(e) if **e != JrgType::Any
                    && !super::codegen_any::terlukis(&super::codegen_any::luruh(e)) =>
                    self.coerce(&v, &JrgType::Slice(Box::new(JrgType::Any))),
                _ => v,
            };
            // Isi buram di dalam wadah diterjemahkan jembatan lewat kait `jrg_buram_jembatan_isi`.
            if mengandung_buram(&v.ty) { self.pakai_jemb_buram = true; self.pakai_buram = true; }
            // Struct, tuple dan enum milik program menyeberang sebagai dict bertanda (`jemb.buram.N`).
            let terjemah = matches!(v.ty, JrgType::Struct(_) | JrgType::Enum(_) | JrgType::Tuple(_));
            let buram = super::codegen_any::tag_untuk(&v.ty) == Some(super::codegen_any::TAG_BURAM) && !terjemah;
            let ada_kotak = v.ty == JrgType::Any || terjemah;
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
        let teks = self.intern_string(awalan.map_or(nama, |(pintu, _)| pintu));
        let (np, nl) = self.split_str(&teks);
        self.ensure_extern("declare i32 @jrg_jembatan(ptr, i64, ptr, i64, ptr)", "jrg_jembatan");
        let r = self.fresh("jbr.");
        self.emit(format!("{} = call i32 @jrg_jembatan(ptr {}, i64 {}, ptr {}, i64 {}, ptr {})", r, np, nl, larik, n, keluar));

        let bentuk = bentuk.unwrap_or_else(|| bentuk_hasil(nama));
        // Method atas nilai bertag: bentuknya ditebak dari namanya, jadi jawaban yang lain bentuk gagal keras.
        if matches!(awalan, Some(("::metode", _))) {
            let sah = self.fresh("jbv.");
            match bentuk.split('<').next().unwrap_or("") {
                "Result" => self.emit(format!("{} = icmp eq i32 {}, 1\n  {}.b = icmp eq i32 {}, 3\n  {}.c = or i1 {}, {}.b",
                    sah, r, sah, r, sah, sah, sah)),
                "Option" => self.emit(format!("{} = icmp eq i32 {}, 1\n  {}.b = icmp eq i32 {}, 2\n  {}.c = or i1 {}, {}.b",
                    sah, r, sah, r, sah, sah, sah)),
                _ => self.emit(format!("{} = icmp eq i32 {}, 0\n  {}.c = or i1 {}, {}", sah, r, sah, sah, sah)),
            }
            let (lanjut, gagal) = (self.fresh_label("jbv.ok."), self.fresh_label("jbv.gagal."));
            self.terminate(format!("br i1 {}.c, label %{}, label %{}", sah, lanjut, gagal));
            self.start_block(gagal);
            self.panik(&format!("method `{}` atas nilai bertag menjawab bentuk yang tidak bisa dibaca backend \
                `--emit-llvm` di tempat ini — ini celah backend, bukan salah programmu", nama));
            self.start_block(lanjut);
        }
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
                // Cuma struct data (`Warna` dll.), bukan handle pustaka seperti `Ptr` atau `DokRouter`.
                if !crate::interpreter::galat_index::HANDLE_PUSTAKA.contains(&t) {
                    let cocok: Vec<String> = self.tables.structs.keys()
                        .filter(|k| super::modul::tampil(k) == t && self.param_struct(k).is_none()
                            && self.types.structs.contains_key(*k)).cloned().collect();
                    if let [kunci] = cocok.as_slice() {
                        let layout = self.types.structs.get(kunci).cloned().ok_or("struct hilang")?;
                        let sty = JrgType::Struct(kunci.clone());
                        let mut kini = "undef".to_string();
                        for (i, (f, ft)) in layout.fields.iter().enumerate() {
                            let x = self.field_titipan(&v, f, span)?;
                            let x = self.coerce(&x, &ft.tanpa_kotak());
                            let x = if let JrgType::Kotak(t) = ft { self.kotakkan_nilai(t, &x) } else { x };
                            let r = self.fresh("jbs.");
                            self.emit(format!("{} = insertvalue {} {}, {} {}, {}", r, sty.llvm_ty(), kini, ft.llvm_ty(), x.text, i));
                            kini = r;
                        }
                        return Ok(Val { text: kini, ty: sty });
                    }
                }
                return Ok(v);
            }
        };
        self.buka_terperiksa(&v, &tujuan).ok_or_else(|| "hasil jembatan tidak bisa dibuka".to_string())
    }

    /// Kotak fungsi untuk jembatan (tag 11): penunjuk ke `{ %JrgFn, adaptor }`; adaptornya satu per tipe.
    fn kotak_fungsi_jembatan(&mut self, f: &Val) -> Val {
        let kunci = format!("jembatan:{:?}", f.ty);
        let nama = match self.adaptor_ada.get(&kunci) {
            Some(n) => n.clone(),
            None => {
                let n = format!("jemb.fn.{}", self.adaptor_ada.len());
                self.adaptor_ada.insert(kunci, n.clone());
                self.antre_fn(super::codegen::KerjaFn::Jembatan { nama: n.clone(), ty: f.ty.clone() });
                n
            }
        };
        self.ensure_extern("declare ptr @jrg_alloc(i64, i64)", "jrg_alloc");
        let kotak = self.fresh("jbf.");
        self.emit(format!("{} = call ptr @jrg_alloc(i64 24, i64 8)", kotak));
        self.emit(format!("store %JrgFn {}, ptr {}", f.text, kotak));
        let pa = self.fresh("jbf.");
        self.emit(format!("{} = getelementptr i8, ptr {}, i64 16", pa, kotak));
        self.emit(format!("store ptr @{}, ptr {}", super::mangle::llvm_quoted_ident(&nama), pa));
        let pi = self.fresh("jbf.");
        self.emit(format!("{} = ptrtoint ptr {} to i64", pi, kotak));
        let (a, b) = (self.fresh("jbf."), self.fresh("jbf."));
        self.emit(format!("{} = insertvalue %JrgAny {{ i32 11, [2 x i64] zeroinitializer }}, i64 {}, 1, 0", a, pi));
        self.emit(format!("{} = insertvalue %JrgAny {}, i32 11, 0", b, a));
        Val { text: b, ty: JrgType::Any }
    }

    /// Badan adaptor `@jemb.fn.N(ptr fungsi, ptr argumen, i64 n, ptr keluar) -> i32`: 0 berhasil, -1
    /// gagal dengan kalimatnya di `keluar`. Argumen bertag dibuka ke tipe parameternya (diperiksa).
    pub(crate) fn pancarkan_adaptor_jembatan(&mut self, nama: &str, ty: &JrgType) -> Result<(), String> {
        let JrgType::Fn(params, _) = ty else { return Ok(()) };
        self.mulai_fungsi_kosong(JrgType::Unit);
        let (ok, salah) = (self.fresh_label("jbad.ok."), self.fresh_label("jbad.salah."));
        let c = self.fresh("jbad.");
        self.emit(format!("{} = icmp eq i64 %n, {}", c, params.len()));
        self.terminate(format!("br i1 {}, label %{}, label %{}", c, ok, salah));
        self.start_block(salah);
        let pesan = self.intern_string(&format!(
            "fungsi milik program ini butuh {} argumen, tapi dipanggil balik dengan jumlah lain", params.len()));
        let k = self.kotak_any(&pesan, Span { lo: crate::lexer::BytePos(0), hi: crate::lexer::BytePos(0), file_id: 0 })?;
        self.emit(format!("store %JrgAny {}, ptr %out", k.text));
        self.terminate("ret i32 -1".to_string());
        self.start_block(ok);
        // Badan tugas: konteks `{ f, a, out }` di paket[0]; hasilnya ditulis ke `out`, lalu selesai.
        let ctx = self.fresh("jbad.");
        self.emit(format!("{} = alloca {{ ptr, ptr, ptr }}", ctx));
        for (i, x) in ["%f", "%a", "%out"].iter().enumerate() {
            let p = self.fresh("jbad.");
            self.emit(format!("{} = getelementptr {{ ptr, ptr, ptr }}, ptr {}, i32 0, i32 {}", p, ctx, i));
            self.emit(format!("store ptr {}, ptr {}", x, p));
        }
        let badan_nama = format!("{}.badan", nama);
        let (pp, pn) = (self.fresh("jbad."), self.fresh("jbad."));
        self.emit(format!("{} = alloca ptr", pp));
        self.emit(format!("{} = alloca i64", pn));
        self.ensure_extern("declare i32 @jrg_masa_lindung(ptr, ptr, ptr, ptr)", "jrg_masa_lindung");
        let rc = self.fresh("jbad.");
        self.emit(format!("{} = call i32 @jrg_masa_lindung(ptr @{}, ptr {}, ptr {}, ptr {})",
            rc, super::mangle::llvm_quoted_ident(&badan_nama), ctx, pp, pn));
        let (lulus, jatuh) = (self.fresh_label("jbad.lulus."), self.fresh_label("jbad.jatuh."));
        let g = self.fresh("jbad.");
        self.emit(format!("{} = icmp eq i32 {}, 0", g, rc));
        self.terminate(format!("br i1 {}, label %{}, label %{}", g, lulus, jatuh));
        self.start_block(jatuh);
        let (tp, tn) = (self.fresh("jbad."), self.fresh("jbad."));
        self.emit(format!("{} = load ptr, ptr {}", tp, pp));
        self.emit(format!("{} = load i64, ptr {}", tn, pn));
        let s1 = self.fresh("jbad.");
        self.emit(format!("{} = insertvalue %JrgStr undef, ptr {}, 0", s1, tp));
        let s2 = self.fresh("jbad.");
        self.emit(format!("{} = insertvalue %JrgStr {}, i64 {}, 1", s2, s1, tn));
        let nol = Span { lo: crate::lexer::BytePos(0), hi: crate::lexer::BytePos(0), file_id: 0 };
        let k = self.kotak_any(&Val { text: s2, ty: JrgType::Str }, nol)?;
        self.emit(format!("store %JrgAny {}, ptr %out", k.text));
        self.terminate("ret i32 -1".to_string());
        self.start_block(lulus);
        self.terminate("ret i32 0".to_string());
        let badan = self.take_function_body();
        self.tambah_definisi(format!("define internal i32 @{}(ptr %f, ptr %a, i64 %n, ptr %out) {{\n{}}}\n",
            super::mangle::llvm_quoted_ident(nama), badan));
        self.selesai_fungsi_kosong();

        // `@<adaptor>.badan(ptr %paket)`: panggil fungsinya, tulis hasilnya, kirim tanda selesai.
        self.mulai_fungsi_kosong(JrgType::Unit);
        self.emit("%ctx = load ptr, ptr %paket".to_string());
        self.emit("%pc = getelementptr ptr, ptr %paket, i64 1".to_string());
        self.emit("%c = load ptr, ptr %pc".to_string());
        let muat = |me: &mut Self, i: usize| {
            let p = me.fresh("jbb.");
            me.emit(format!("{} = getelementptr {{ ptr, ptr, ptr }}, ptr %ctx, i32 0, i32 {}", p, i));
            let v = me.fresh("jbb.");
            me.emit(format!("{} = load ptr, ptr {}", v, p));
            v
        };
        let (fp, ap, op) = (muat(self, 0), muat(self, 1), muat(self, 2));
        let f = self.load_place(&fp, ty);
        let mut args = Vec::with_capacity(params.len());
        for i in 0..params.len() {
            let p = self.fresh("jbb.");
            self.emit(format!("{} = getelementptr %JrgAny, ptr {}, i64 {}", p, ap, i));
            args.push(self.load_place(&p, &JrgType::Any));
        }
        let r = self.panggil_val(&f, &args)?;
        let r = if matches!(r.ty, JrgType::Unit | JrgType::Never) { Val::unit() } else { r };
        let k = self.coerce(&r, &JrgType::Any);
        self.emit(format!("store %JrgAny {}, ptr {}", k.text, op));
        self.ensure_extern("declare void @jrg_chan_send(ptr, ptr, i64)", "jrg_chan_send");
        self.emit("call void @jrg_chan_send(ptr %c, ptr null, i64 0)".to_string());
        self.terminate("ret void".to_string());
        let badan = self.take_function_body();
        self.tambah_definisi(format!("define internal void @{}(ptr %paket) {{\n{}}}\n",
            super::mangle::llvm_quoted_ident(&badan_nama), badan));
        self.selesai_fungsi_kosong();
        Ok(())
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
        let k = if matches!(t, JrgType::Struct(_) | JrgType::Tuple(_)) {
            let v = self.load_place("%p", &t);
            let tempat = self.spill_to_place(&v);
            let e = self.lokal_sementara(tempat, t.clone(), nol);
            let ex = match &t {
                JrgType::Tuple(ts) => {
                    let teks = |s: &str| Expr::Literal(LitKind::Str(s.to_string()), nol);
                    let pasang = |k: Expr, v: Expr| Expr::Array { elems: vec![k, v], span: nol };
                    let mut isi = vec![pasang(teks(TANDA_TUPLE), teks(""))];
                    for i in 0..ts.len() {
                        let f = Expr::Field { obj: Box::new(e.clone()), field: i.to_string(), span: nol };
                        isi.push(pasang(teks(&i.to_string()), f));
                    }
                    Expr::Call { func: Box::new(Expr::PathExpr { segments: vec![String::new(), "dict".into(), "from_pairs".into()], span: nol }),
                                 args: vec![Expr::Array { elems: isi, span: nol }], span: nol }
                }
                _ => self.struct_ke_dict(&e, &t, nol),
            };
            let d = self.codegen_expr(&ex, None)?;
            let d = super::codegen_any::samakan_dict(&d);
            self.kotak_any(&d, nol)?
        } else if let JrgType::Enum(en) = &t {
            self.enum_ke_jembatan(en, &t, nol)?
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

    /// Enum di `%p` sebagai dict bertanda: dulu dikembalikan apa adanya, jadi jembatan menolak struct apa
    /// pun yang memuat enum (`Option<int>`). Muatannya berbentuk VM: satu nilai, tuple, atau struct varian.
    fn enum_ke_jembatan(&mut self, en: &str, t: &JrgType, nol: Span) -> Result<Val, String> {
        use super::types::EnumPayload;
        let layout = self.types.enums.get(en).cloned().ok_or("enum hilang")?;
        let hasil = self.alloca_typed("jbe.", &JrgType::Any);
        let tag = self.get_tag("%p", t);
        let usai = self.fresh_label("jbe.usai.");
        let label: Vec<String> = layout.variants.iter().map(|_| self.fresh_label("jbe.v.")).collect();
        let kasus: Vec<String> = layout.variants.iter().zip(&label).map(|(v, l)| format!("i32 {}, label %{}", v.tag, l)).collect();
        self.terminate(format!("switch i32 {}, label %{} [ {} ]", tag, usai, kasus.join(" ")));
        let teks = |s: &str| Expr::Literal(LitKind::Str(s.to_string()), nol);
        let pasang = |k: Expr, v: Expr| Expr::Array { elems: vec![k, v], span: nol };
        let dari_pasangan = |isi: Vec<Expr>| Expr::Call { func: Box::new(Expr::PathExpr {
            segments: vec![String::new(), "dict".into(), "from_pairs".into()], span: nol }),
            args: vec![Expr::Array { elems: isi, span: nol }], span: nol };
        let terlihat = super::generik::dasar(en).split('$').next().unwrap_or(en).to_string();
        for (v, l) in layout.variants.iter().zip(&label) {
            self.start_block(l.clone());
            let ps = v.payload.as_llvm_struct();
            let mut nilai = Vec::new();
            for (i, ft) in v.payload.field_types().iter().enumerate() {
                let x = self.get_payload_field("%p", t, &ps, i, ft);
                let tempat = self.spill_to_place(&x);
                nilai.push(self.lokal_sementara(tempat, x.ty.clone(), nol));
            }
            let mut isi = vec![pasang(teks(TANDA_ENUM), teks(&terlihat)), pasang(teks(TANDA_VARIAN), teks(&v.name))];
            let muatan = match &v.payload {
                EnumPayload::Unit => None,
                EnumPayload::Tuple(_) if nilai.len() == 1 => nilai.pop(),
                EnumPayload::Tuple(_) => {
                    let mut d = vec![pasang(teks(TANDA_TUPLE), teks(""))];
                    for (i, x) in nilai.into_iter().enumerate() { d.push(pasang(teks(&i.to_string()), x)); }
                    Some(dari_pasangan(d))
                }
                EnumPayload::Struct(fs) => {
                    let mut d = vec![pasang(teks(TANDA_STRUCT), teks(&v.name))];
                    for ((f, _), x) in fs.iter().zip(nilai) { d.push(pasang(teks(f), x)); }
                    Some(dari_pasangan(d))
                }
            };
            if let Some(m) = muatan { isi.push(pasang(teks(TANDA_ISI), m)); }
            let d = self.codegen_expr(&dari_pasangan(isi), None)?;
            let d = super::codegen_any::samakan_dict(&d);
            let k = self.kotak_any(&d, nol)?;
            self.emit(format!("store %JrgAny {}, ptr {}", k.text, hasil));
            if !self.is_terminated() { self.terminate(format!("br label %{}", usai)); }
        }
        self.start_block(usai);
        Ok(self.load_place(&hasil, &JrgType::Any))
    }

    /// `@jrg_buram_ke_jembatan(kotak)`: kotak bertipe buram lewat penyeberangnya, sisanya apa adanya.
    pub(crate) fn definisi_buram_jembatan(&self) -> String {
        let mut s = String::from("define internal %JrgAny @jrg_buram_ke_jembatan(%JrgAny %a) {\nentry:\n  %w = extractvalue %JrgAny %a, 0\n  %p0 = extractvalue %JrgAny %a, 1, 0\n  %p = inttoptr i64 %p0 to ptr\n  switch i32 %w, label %lain [");
        let mut badan = String::new();
        for i in 0..self.jemb_terpancar {
            s.push_str(&format!(" i32 {}, label %b{}", super::codegen_any::TAG_BURAM | ((i as i32 + 1) << 8), i + 1));
            badan.push_str(&format!("b{0}:\n  %r{0} = call %JrgAny @jemb.buram.{0}(ptr %p)\n  ret %JrgAny %r{0}\n", i + 1));
        }
        // Kait untuk jembatan: isi buram di dalam array/dict diterjemahkan saat dibaca di sana.
        format!("{} ]\n{}lain:\n  ret %JrgAny %a\n}}\n\
                 define void @jrg_buram_jembatan_isi(ptr %a, ptr %out) {{\nentry:\n  %v = load %JrgAny, ptr %a\n  \
                 %r = call %JrgAny @jrg_buram_ke_jembatan(%JrgAny %v)\n  store %JrgAny %r, ptr %out\n  ret void\n}}\n", s, badan)
    }

    /// `x.f` atas nilai bertag: dibaca lewat jembatan (titipan milik `jrgc`, mis. `MousePos`).
    pub(crate) fn field_titipan(&mut self, v: &Val, field: &str, span: Span) -> Result<Val, String> {
        let t = self.intern_string(field);
        let tempat = self.spill_to_place(v);
        let a = self.lokal_sementara(tempat, JrgType::Any, span);
        let pt = self.spill_to_place(&t);
        let b = self.lokal_sementara(pt, JrgType::Str, span);
        self.panggil_jembatan("::field", None, None, &[a, b], span)
    }

    /// `x.f = v` atas nilai bertag: aturan `set_field` milik VM atas SALINANnya, lalu ditulis balik.
    pub(crate) fn tulis_field_titipan(&mut self, obj: &Expr, field: &str, nilai: &Expr, span: Span) -> Result<(), String> {
        let v = self.codegen_expr(nilai, None)?;
        let v = self.ambil_milik(nilai, v)?;
        let ty = v.ty.clone();
        let pv = self.spill_to_place(&v);
        let c = self.lokal_sementara(pv, ty, span);
        let (ptr, _) = self.get_place(obj)?;
        let o = self.load_place(&ptr, &JrgType::Any);
        let po = self.spill_to_place(&o);
        let a = self.lokal_sementara(po, JrgType::Any, span);
        let t = self.intern_string(field);
        let pt = self.spill_to_place(&t);
        let b = self.lokal_sementara(pt, JrgType::Str, span);
        let baru = self.panggil_jembatan("::field_tulis", None, None, &[a, b, c], span)?;
        let baru = self.coerce(&baru, &JrgType::Any);
        self.emit(format!("store %JrgAny {}, ptr {}", baru.text, ptr));
        Ok(())
    }

    /// `x["k"]` atas nilai titipan (struct milik `jrgc`, mis. `JsonObject`): lewat `::field`. Tidak
    /// menandai jembatan terpakai, alasannya sama dengan `tanya_titipan`.
    pub(crate) fn indeks_titipan(&mut self, v: &Val, k: &Val, span: Span) -> Result<Val, String> {
        let kk = self.kotak_any(k, span)?;
        let larik = self.fresh("jbi.");
        self.emit(format!("{} = alloca [2 x %JrgAny]", larik));
        for (i, x) in [v.text.clone(), kk.text].iter().enumerate() {
            let p = self.fresh("jbi.");
            self.emit(format!("{} = getelementptr %JrgAny, ptr {}, i64 {}", p, larik, i));
            self.emit(format!("store %JrgAny {}, ptr {}", x, p));
        }
        let keluar = self.alloca_typed("jbi.", &JrgType::Any);
        let teks = self.intern_string("::field");
        let (np, nl) = self.split_str(&teks);
        self.ensure_extern("declare i32 @jrg_jembatan(ptr, i64, ptr, i64, ptr)", "jrg_jembatan");
        let r = self.fresh("jbr.");
        self.emit(format!("{} = call i32 @jrg_jembatan(ptr {}, i64 {}, ptr {}, i64 2, ptr {})", r, np, nl, larik, keluar));
        Ok(self.load_place(&keluar, &JrgType::Any))
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
