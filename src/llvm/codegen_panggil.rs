// src/llvm/codegen_panggil.rs — pemanggilan fungsi dan metode

#![allow(dead_code)]

use crate::parser::*;
use crate::lexer::Span;

use super::codegen::{Codegen, Val};
use super::mangle::*;
use super::sema::*;
use super::types::*;
use super::codegen_access::*;

impl Codegen {
    // ── calls ────────────────────────────────────────────────────────────

    /// Tipe tiap argumen, dibaca TANPA menurunkan satu pun dari mereka.
    pub(crate) fn tipe_argumen_di_tempat(&self, args: &[Expr]) -> Option<Vec<JrgType>> {
        args.iter()
            .map(|a| self.peek_expr_type(a)
                .or_else(|| super::codegen_expr::tipe_bawaan_ekspresi(a)))
            .collect()
    }

    pub fn codegen_call(&mut self, func: &Expr, args: &[Expr], expected: Option<&JrgType>, span: Span) -> Result<Val, String> {
        match func {
            Expr::Ident(name, _) => {
                // Sebuah VARIABEL yang memegang fungsi menang atas fungsi
                // bernama yang kebetulan sama namanya — bayangannya sama
                // dengan variabel lain, dan acuannya juga begitu.
                if let Some((ptr, ty)) = self.lookup_local(name) {
                    if matches!(ty, JrgType::Fn(..)) {
                        let f = self.load_place(&ptr, &ty);
                        return self.panggil_nilai_fungsi(&f, args, span);
                    }
                }
                // Fungsi milik PROGRAM menang atas bawaan bernama sama, seperti di dua mesin
                // acuan — dulu `fn len(a, b, c)` dijawab "`len` butuh 1 argumen", dan `fn abs`
                // milik pemakai tidak pernah terpanggil.
                if self.cari_fn_bersarang(name).is_some() || self.tables.functions.contains_key(&self.k(name))
                    || self.tables.extern_desk.contains_key(name) {
                    return self.call_named_function(name, args, expected, span);
                }
                if let Some(g) = Self::aritas_bawaan(name, args.len(), span) { return Err(g); }
                if let Some(r) = self.try_builtin_call(name, args, expected, span) { return r; }
                if matches!(name.as_str(), "Some" | "None" | "Ok" | "Err") {
                    return self.construct_option_result(name, args, expected, span);
                }
                // `A(3)` — varian BERPAYLOAD yang disebut tanpa kualifikasi.
                // Interpreter dan VM sama-sama menerimanya.
                if !self.tables.functions.contains_key(&self.k(name)) {
                    let cocok = self.enum_punya_varian(name, false);
                    if cocok.len() == 1 {
                        return self.construct_user_enum_variant(&cocok[0], name, args, span);
                    }
                    if cocok.len() > 1 {
                        return Err(super::galat::tandai(span, format!(
                            "`{}` ambigu — varian dengan nama itu ada di {}\n\
                             bantuan: tulis lengkap, mis. `{}::{}`",
                            name, cocok.join(" dan "), cocok[0], name)));
                    }
                }
                if !self.tables.functions.contains_key(&self.k(name)) {
                    if let Some((sym, ty)) = self.var_global.get(&self.k(name)).cloned() {
                        if matches!(ty, JrgType::Fn(..)) {
                            let f = self.load_place(&sym, &ty);
                            return self.panggil_nilai_fungsi(&f, args, span);
                        }
                    }
                }
                self.call_named_function(name, args, expected, span)
            }
            // `::nama(...)`: bawaan bahasa, tanpa peduli fungsi milik program.
            Expr::PathExpr { segments, span: pspan } if segments.first().is_some_and(|s| s.is_empty()) => {
                if segments.len() > 2 {
                    return self.codegen_path_call(&segments[1..], args, expected, *pspan);
                }
                let name = segments.get(1).map(|s| s.as_str()).unwrap_or("");
                if let Some(galat) = Self::celah_turunan(name, span) { return Err(galat); }
                if let Some(g) = Self::aritas_bawaan(name, args.len(), span) { return Err(g); }
                if let Some(r) = self.try_builtin_call(name, args, expected, span) { return r; }
                if matches!(name, "Some" | "None" | "Ok" | "Err") {
                    return self.construct_option_result(name, args, expected, span);
                }
                Err(crate::llvm::galat::tandai(*pspan, crate::interpreter::pesan_bukan_bawaan(name, None)))
            }
            Expr::PathExpr { segments, span: pspan } => self.codegen_path_call(segments, args, expected, *pspan),
            // Closure yang LANGSUNG dipanggil di tempat: `(|x| x * 2)(4)`.
            Expr::Closure { params, body, is_move, span: cspan, .. }
                if params.len() == args.len()
                    && self.tipe_argumen_di_tempat(args).is_some() =>
            {
                let ptys = self.tipe_argumen_di_tempat(args).expect("baru saja diperiksa");
                let f = self.closure_dgn_params(params, body, &ptys, *is_move, *cspan)?;
                self.panggil_nilai_fungsi(&f, args, span)
            }
            lain => {
                // Ekspresi apa pun yang menghasilkan nilai fungsi bisa
                // dipanggil: `(bikin())(3)`, `daftar[0](3)`, `t.0(3)`.
                let f = self.codegen_expr(lain, None)?;
                if matches!(f.ty, JrgType::Fn(..)) {
                    return self.panggil_nilai_fungsi(&f, args, span);
                }
                Err(super::galat::tandai(span, format!(
                    "`{}` bukan sesuatu yang bisa dipanggil", f.ty.display_name())))
            }
        }
    }

    /// Jumlah argumen builtin diperiksa dengan batas dan kalimat dua mesin acuan
    /// (`interpreter::arity`); nama tanpa entri di sana dibiarkan ke penurunnya.
    fn aritas_bawaan(nama: &str, n: usize, span: Span) -> Option<String> {
        use crate::interpreter::arity;
        let maks = arity::maks(nama);
        if let Some(m) = maks.filter(|m| n > *m) {
            return Some(super::galat::tandai(span, arity::pesan_kebanyakan(nama, m, n)));
        }
        let min = arity::minimal(nama).filter(|m| n < *m)?;
        Some(super::galat::tandai(span, arity::pesan_kurang(nama, min, maks, n, arity::tanda_tangan(nama))))
    }

    /// Nomor unit di balik alias `ns`, dilihat dari unit yang sedang diturunkan.
    /// Nilai yang SUDAH dievaluasi, sebagai ekspresi: lokal bernama mustahil (`$` tak bisa ditulis).
    pub(crate) fn lokal_sementara(&mut self, ptr: String, ty: JrgType, span: Span) -> Expr {
        self.penerima_lapis += 1;
        let nama = format!("$penerima{}", self.penerima_lapis);
        self.define_local(&nama, ptr, ty);
        Expr::Ident(nama, span)
    }

    /// `{ let isi = <lapis>(s); let b = jawa::buffer(len(isi)); for i, x in isi { jawa::buffer_set(b, i, x) }; b }`
    /// dengan nama yang tidak bisa ditulis pemakai (`$`).
    fn buffer_dari_hex(&self, teks: &Expr, sp: Span) -> Option<Expr> {
        let isi_fn = super::modul::kunci(self.unit_lapis?, "_isi_buffer_hex");
        if !self.tables.functions.contains_key(&isi_fn) { return None; }
        let id = |n: &str| Expr::Ident(n.to_string(), sp);
        let jawa = |item: &str, args: Vec<Expr>| Expr::Call {
            func: Box::new(Expr::PathExpr { segments: vec!["jawa".into(), item.into()], span: sp }), args, span: sp };
        let ikat = |nama: &str, init: Expr| Stmt::Let(LetStmt {
            is_mut: false, ownership: None, pattern: Pattern::Ident { name: nama.into(), is_mut: false, span: sp },
            ty: None, init: Box::new(init), lifetime: None, span: sp });
        let isi = Expr::Call { func: Box::new(id(&isi_fn)), args: vec![teks.clone()], span: sp };
        let panjang = Expr::Call { func: Box::new(id("len")), args: vec![id("$isi_hex")], span: sp };
        let isi_ulang = Stmt::For(ForStmt::Range {
            vars: vec!["$i_hex".into(), "$x_hex".into()], is_decl: true, iter: Box::new(id("$isi_hex")),
            body: Block { stmts: vec![Stmt::Expr(Box::new(jawa("buffer_set", vec![id("$buf_hex"), id("$i_hex"), id("$x_hex")])))],
                          tail: None, span: sp },
            label: None, span: sp });
        Some(Expr::Block(Block {
            stmts: vec![ikat("$isi_hex", isi), ikat("$buf_hex", jawa("buffer", vec![panjang])), isi_ulang],
            tail: Some(Box::new(id("$buf_hex"))), span: sp }))
    }

    /// Kunci fungsi lapis ketiga yang melayani `ns::item`, kalau ada.
    pub(crate) fn kunci_lapis(&self, ns: &str, item: &str) -> Option<String> {
        let k = super::modul::kunci(self.unit_lapis?, &format!("{}_{}", ns, item));
        if self.tables.functions.contains_key(&k) { Some(k) } else { None }
    }

    pub(crate) fn unit_alias(&self, ns: &str) -> Option<usize> {
        self.tables.alias_unit.get(self.unit_kini).and_then(|m| m.get(ns)).copied()
    }

    /// Kunci tabel `ns::item` milik unit `u`, sesudah aturan ekspor: nama ber-`_` privat dan
    /// `var` tidak diekspor — kalimatnya milik dua mesin lain.
    pub(crate) fn kunci_anggota_modul(&self, u: usize, ns: &str, item: &str, span: Span) -> Result<String, String> {
        let kunci = super::modul::kunci(u, item);
        if item.starts_with('_') {
            return Err(super::galat::tandai(span, crate::interpreter::pesan_privat_modul(item, ns)));
        }
        if self.tables.vars.iter().any(|v| v.name == kunci) {
            return Err(super::galat::tandai(span, crate::interpreter::pesan_var_modul(item, ns)));
        }
        Ok(kunci)
    }

    /// `ns::item` sebagai NILAI: `const`/`let` milik modul, diturunkan di dalam unitnya.
    pub(crate) fn nilai_anggota_modul(&mut self, u: usize, ns: &str, item: &str,
                                      expected: Option<&JrgType>, span: Span) -> Result<Val, String> {
        let kunci = self.kunci_anggota_modul(u, ns, item, span)?;
        if let Some(c) = self.tables.consts.get(&kunci).cloned() {
            let lama = std::mem::replace(&mut self.unit_kini, u);
            let hasil = (|| {
                let want = match &c.ty {
                    Some(te) => Some(self.resolve_type_pub(te)?),
                    None => expected.cloned(),
                };
                self.codegen_expr(&c.value, want.as_ref())
            })();
            self.unit_kini = lama;
            return hasil;
        }
        if self.tables.functions.contains_key(&kunci) {
            if let Some(r) = self.nilai_fungsi_bernama(&kunci, expected, span) { return r; }
        }
        Err(super::galat::tandai(span, crate::interpreter::pesan_path_tak_dikenal(&format!("{}::{}", ns, item), None)))
    }

    pub(crate) fn codegen_path_call(&mut self, segments: &[String], args: &[Expr], expected: Option<&JrgType>, span: Span) -> Result<Val, String> {
        if segments.len() == 2 {
            let (ns, item) = (segments[0].as_str(), segments[1].as_str());
            // Alias yang di-`import` UNIT INI menang atas namespace bawaan bernama sama.
            if let Some(u) = self.unit_alias(ns) {
                let kunci = self.kunci_anggota_modul(u, ns, item, span)?;
                if self.tables.functions.contains_key(&kunci) {
                    return self.call_named_function(&kunci, args, expected, span);
                }
                // `const`/`let` modul yang memegang fungsi.
                let f = self.nilai_anggota_modul(u, ns, item, None, span)?;
                return self.panggil_nilai_fungsi(&f, args, span);
            }
            let qualified = format!("{}::{}", ns, item);
            if let Some(g) = Self::aritas_bawaan(&qualified, args.len(), span) { return Err(g); }
            if let Some(r) = self.try_builtin_call(&qualified, args, expected, span) { return r; }

            if matches!(item, "Some" | "None" | "Ok" | "Err") {
                return self.construct_option_result(item, args, expected, span);
            }
            // `Option::unwrap(o)` adalah `o.unwrap()`: satu badan, dua pintu.
            if matches!(ns, "Option" | "Result") && !args.is_empty() && self.unit_alias(ns).is_none() {
                return self.codegen_method(&args[0], item, &args[1..], expected, span);
            }
            if let Some(en) = self.tables.enums.get(ns).cloned() {
                if en.variants.iter().any(|v| v.name == item) {
                    return self.construct_user_enum_variant(ns, item, args, span);
                }
            }
            if let Some(decl) = self.tables.find_method(ns, item).cloned() {
                let takes_self = decl.params.first().map(|p| pattern_simple_name_pub(&p.pattern).as_deref() == Some("self")).unwrap_or(false);
                if takes_self {
                    let Some(penerima) = args.first() else {
                        return Err(crate::llvm::galat::tandai(span, format!(
                            concat!("`{}::{}` adalah method ber-`self`, jadi ia butuh ",
                                    "penerimanya sebagai argumen pertama: `{}::{}(x, …)` ",
                                    "— atau tulis bentuk method-nya, `x.{}( … )`"),
                            ns, item, ns, item, item)));
                    };
                    let (optr, oty) = self.get_place(penerima)?;
                    let cocok = matches!(&oty,
                        JrgType::Struct(n) | JrgType::Enum(n) if n.as_str() == ns);
                    if !cocok {
                        return Err(crate::llvm::galat::tandai(span,
                            crate::interpreter::pesan_penerima_salah(
                                &qualified, ns, &oty.display_name())));
                    }
                    return self.call_method_impl(ns, &decl, &optr, &oty, &args[1..], span);
                }
                return self.call_static_method(ns, &decl, args, span);
            }
            // `fs::join(a, b, c, ..)` adalah `fs::join(fs::join(a, b), c)`: lapis cuma punya bentuk dua.
            if ns == "fs" && item == "join" && args.len() > 2 {
                let (awal, akhir) = args.split_at(args.len() - 1);
                let dalam = Expr::Call {
                    func: Box::new(Expr::PathExpr { segments: segments.to_vec(), span }),
                    args: awal.to_vec(), span,
                };
                return self.codegen_path_call(segments, &[dalam, akhir[0].clone()], expected, span);
            }
            // `jawa::buffer_from_hex(s)`: buffer milik runtime, isinya dari lapis (`_isi_buffer_hex`).
            if ns == "jawa" && item == "buffer_from_hex" && args.len() == 1 {
                if let Some(blok) = self.buffer_dari_hex(&args[0], span) { return self.codegen_expr(&blok, expected); }
            }
            // Lapis ketiga: builtin yang ditulis dalam JRG, dicari sesudah semua penurun asli.
            if let Some(kunci) = self.kunci_lapis(ns, item) {
                return self.call_named_function(&kunci, args, expected, span);
            }
            // Builtin yang implementasinya milik `jrgc`: lewat jembatan pustaka dinamis.
            if let Some(r) = self.coba_jembatan(ns, item, args, span) { return r; }
            let di_import = self.tables.imports.iter().any(|n| n == ns);
            return Err(crate::llvm::galat::tandai(
                span,
                crate::llvm::galat::jelaskan_path_hilang(ns, item, di_import),
            ));
        }
        // `modul::Enum::Varian(..)`: tipe satu ruang sesudah `import`, jadi segmen modulnya dibuang.
        if segments.len() == 3 && self.unit_alias(&segments[0]).is_some() {
            return self.codegen_path_call(&segments[1..], args, expected, span);
        }
        Err(format!("[{}:{}] path pemanggilan fungsi dengan lebih dari 2 segmen belum didukung", span.lo.0, span.hi.0))
    }

    /// Tipe `Result`/`Option` lengkap untuk sebuah argumen konstruktor, kalau
    /// kedua paruhnya terbaca dari situs-situs panggilan fungsi ini.
    pub(crate) fn petunjuk_paruh(&mut self, name: &str, i: usize, arg: &Expr) -> Option<JrgType> {
        let konstruktor = match arg {
            Expr::Call { func, args, .. } if args.len() == 1 => match &**func {
                Expr::Ident(n, _) if matches!(n.as_str(), "Ok" | "Err" | "Some") => n.clone(),
                _ => return None,
            },
            _ => return None,
        };
        let (t, e) = self.paruh_result.get(&(name.to_string(), i)).cloned()?;
        match konstruktor.as_str() {
            "Some" => Some(JrgType::Enum(self.instantiate_option(&t?))),
            _ => Some(JrgType::Enum(self.instantiate_result(&t?, &e?))),
        }
    }

    /// `terap`/`terap_bernama` adalah nama TURUNAN: muncul karena gula `f(...v)` dan
    /// `f(a: 1)`, dan pemakainya tidak pernah menulisnya — jadi yang disebut gulanya.
    pub(crate) fn celah_turunan(name: &str, span: Span) -> Option<String> {
        match name {
            "terap" => Some(format!(
                "[{}:{}] spread di pemanggilan (`f(...v)`) belum didukung backend \
                 LLVM: jumlah argumennya baru diketahui saat program jalan, \
                 sementara backend ini menentukannya saat kompilasi. Interpreter \
                 dan `--vm` mendukungnya",
                span.lo.0, span.hi.0)),
            "terap_bernama" => Some(format!(
                "[{}:{}] argumen bernama (`f(a: 1)`) belum didukung backend LLVM: \
                 urutannya ditentukan dari nama parameter fungsinya SAAT JALAN, \
                 sementara backend ini menyusun daftar argumen saat kompilasi. \
                 Interpreter dan `--vm` mendukungnya",
                span.lo.0, span.hi.0)),
            _ => None,
        }
    }

    /// Tipe argumen yang diberikan badan `decl` kepada parameter ke-`i`-nya (sebuah fungsi),
    /// dengan tipe parameter lain diambil dari argumen situs panggilan ini — tanpa menurunkan
    /// apa pun. `None` kalau badannya tidak memanggil parameter itu dengan tipe yang terbaca.
    fn tipe_param_closure_dari_callee(&mut self, simbol: &str, decl: &FnDecl, args: &[Expr],
                                      i: usize, jumlah: usize, diketahui: &[Option<JrgType>]) -> Option<Vec<JrgType>> {
        // Lokal PEMANGGIL, untuk membaca tipe argumen lain tanpa menurunkannya.
        let mut di_sini = super::infer::Lokal::new();
        for lapis in &self.locals {
            for (n, (_, t)) in lapis { di_sini.insert(n.clone(), t.clone()); }
        }
        let generik = self.cur_fn_generics.clone();
        let tipe: Vec<Option<JrgType>> = args.iter().enumerate().map(|(j, a)| {
            // Argumen yang SUDAH diturunkan memberi tipe aslinya (`Ok(4)` tanpa anotasi).
            if j == i { None } else {
                diketahui.get(j).cloned().flatten().or_else(|| self.tipe_ekspresi(a, &di_sini, &generik, ""))
            }
        }).collect();
        self.tipe_param_fn_dari_badan(simbol, decl, &tipe, i, jumlah, 0)
    }

    /// Inti `tipe_param_closure_dari_callee`, atas TIPE argumen. Kalau badannya tidak memanggil
    /// parameter itu sendiri melainkan MENERUSKANNYA (`fold` ke `reduce`), yang dibaca badan
    /// fungsi yang menerimanya — paling dalam empat lapis.
    fn tipe_param_fn_dari_badan(&mut self, simbol: &str, decl: &FnDecl, tipe: &[Option<JrgType>],
                                i: usize, jumlah: usize, dalam: u8) -> Option<Vec<JrgType>> {
        let FnBody::Block(b) = &*decl.body else { return None };
        let mut lokal = super::infer::Lokal::new();
        let mut subst = TypeSubst::new();
        for (j, p) in decl.params.iter().enumerate() {
            if j == i { continue; }
            let Some(n) = pattern_simple_name_pub(&p.pattern) else { continue };
            let t = if matches!(p.ty, TypeExpr::Inferred) || is_bare_array_type(&p.ty) {
                tipe.get(j).cloned().flatten()
                    .map(|t| match t { JrgType::Array(e, _) => JrgType::Slice(e), lain => lain })
            } else {
                self.resolve_type_with_subst(&p.ty, &subst).ok()
            };
            if let Some(t) = t {
                if matches!(p.ty, TypeExpr::Inferred) { subst.insert(format!("$param{}", j), t.clone()); }
                lokal.insert(n, t);
            }
        }
        let nama = pattern_simple_name_pub(&decl.params.get(i)?.pattern)?;
        // Badannya milik unit fungsi itu: nama tingkat atas di dalamnya dicari di sana.
        let unit = self.unit_simbol_dari(simbol);
        let lama = std::mem::replace(&mut self.unit_kini, unit);
        let (peta, _) = super::infer::tipe_argumen_dan_paruh(self, b, &lokal, &subst);
        let mut hasil = peta.get(&nama).filter(|ts| ts.len() == jumlah).cloned();
        if hasil.is_none() && dalam < 4 {
            let mut terusan = Vec::new();
            super::infer::cari_penerusan(b, &nama, &mut terusan);
            for (g, args_g, k) in terusan {
                let kunci = self.k(&g);
                let Some(dg) = self.tables.functions.get(&kunci).cloned() else { continue };
                let tipe_g: Vec<Option<JrgType>> = args_g.iter().enumerate().map(|(j, a)| {
                    if j == k { None } else { self.tipe_ekspresi(a, &lokal, &subst, "") }
                }).collect();
                hasil = self.tipe_param_fn_dari_badan(&kunci, &dg, &tipe_g, k, jumlah, dalam + 1);
                if hasil.is_some() { break; }
            }
        }
        self.unit_kini = lama;
        hasil
    }

    pub(crate) fn call_named_function(&mut self, name: &str, args: &[Expr], harap: Option<&JrgType>, span: Span) -> Result<Val, String> {
        // Fungsi BERSARANG menang atas yang tingkat atas dengan nama sama —
        // aturan bayangan yang sama dengan variabel. Simbolnya berbeda,
        // jadi nama itu ikut dibawa ke pemanggilan dan pemancarannya.
        if self.cari_fn_bersarang(name).is_none() && !self.tables.functions.contains_key(&self.k(name)) {
            // `extern fn`: lewat jembatan ke `ffi::native`.
            if let Some(d) = self.tables.extern_desk.get(name).cloned() {
                return self.panggil_extern(name, &d, args, span);
            }
        }
        let (nama_simbol, decl, bersarang) = match self.cari_fn_bersarang(name) {
            Some((unik, d)) => (unik, d, true),
            None => (self.k(name), self.tables.functions.get(&self.k(name)).cloned()
                .ok_or_else(|| {
                    if let Some(galat) = Self::celah_turunan(name, span) { return galat; }
                    // Sebuah nama yang dideklarasikan di blok `extern` BUKAN nama yang tidak
                    // ada: backend ini tidak memancarkan `extern "C"` sama sekali.
                    if self.tables.extern_fns.iter().any(|n| n == name) {
                        return crate::llvm::galat::tandai(span, format!(
                            "`{}` dideklarasikan di blok `extern`, dan backend `--emit-llvm` \
                             tidak memancarkan `extern \"C\"` sama sekali — jadi ia tidak bisa \
                             dipanggil dari sini\nbantuan: jalankan tanpa `--emit-llvm` \
                             (interpreter) atau dengan `--vm`; keduanya mendukung FFI penuh",
                            name));
                    }
                    // `println("x")` bukan fungsi yang salah ketik melainkan kata dari
                    // bahasa lain; lihat catatan di `codegen_expr`.
                    if crate::interpreter::padanan_bahasa_lain(name).is_some() {
                        return crate::llvm::galat::tandai(
                            span, crate::interpreter::pesan_nama_tak_dikenal(name, None));
                    }
                    format!("[{}:{}] fungsi `{}` tidak ditemukan", span.lo.0, span.hi.0, name)
                })?,
                false),
        };
        let name = nama_simbol.as_str();
        // Parameter yang dihilangkan boleh, asal SEMUA yang dihilangkan punya
        // default.
        let variadic = decl.params.last().map_or(false, |p| p.is_variadic);
        let args_kemas: Vec<Expr>;
        // Argumen TETAP yang belum lengkap tidak dikemas: sisanya dihitung di dalam
        // fungsinya (default, dan array kosong untuk pengumpulnya) seperti acuan.
        let args: &[Expr] = if variadic && args.len() >= decl.params.len() - 1 {
            let tetap = decl.params.len() - 1;
            let mut v: Vec<Expr> = args[..tetap].to_vec();
            v.push(Expr::Array { elems: args[tetap..].to_vec(), span });
            args_kemas = v;
            &args_kemas
        } else {
            args
        };

        if args.len() > decl.params.len()
            || decl.params[args.len()..].iter().any(|p| p.default.is_none() && !p.is_variadic)
        {
            let wajib = decl.params.iter().filter(|p| p.default.is_none()).count();
            let pesan = if wajib == decl.params.len() {
                format!("`{}` butuh {} argumen, diberi {}", name, decl.params.len(), args.len())
            } else {
                format!("`{}` butuh {}..{} argumen, diberi {}",
                    name, wajib, decl.params.len(), args.len())
            };
            return Err(super::galat::tandai(span, pesan));
        }
        let terpakai = args.len();
        // Diambil SEKARANG: panggilan lain di dalam argumen tidak boleh ikut memakainya.
        let pindah = self.arg_pindah.take()
            .filter(|n| matches!(args.first(), Some(Expr::Ident(m, _)) if m == n));
        let mut subst = TypeSubst::new();
        let generik: Vec<String> = decl.generics.params.iter().map(|p| p.name.clone()).collect();
        let mut concrete_tys: Vec<Option<JrgType>> = vec![None; args.len()];
        for (i, p) in decl.params.iter().take(terpakai).enumerate() {
            // Dua bentuk yang tipenya baru diketahui dari argumennya:
            // `array` telanjang, dan parameter tanpa anotasi sama sekali.
            if is_bare_array_type(&p.ty) || matches!(p.ty, TypeExpr::Inferred) { continue; }
            // Parameter tipe yang belum terikat bukan tipe konkret — juga kalau program punya
            // struct bernama sama (`struct T` lawan `fn f<T>`): parameter tipenya yang menang.
            let bebas: Vec<String> = generik.iter().filter(|g| !subst.contains_key(*g)).cloned().collect();
            if Self::menyebut_param(&p.ty, &bebas) { continue; }
            if let Ok(t) = self.resolve_type_with_subst(&p.ty, &subst) {
                concrete_tys[i] = Some(t);
            }
        }
        let mut arg_vals: Vec<Option<Val>> = vec![None; args.len()];
        for (i, p) in decl.params.iter().take(terpakai).enumerate() {
            if concrete_tys[i].is_none() {
                // `Ok(21)` sendirian cuma memberi `T`; `E` ada di situs panggilan LAIN atas
                // fungsi yang sama.
                let petunjuk = self.petunjuk_paruh(name, i, &args[i]);
                // Closure tanpa anotasi di parameter tanpa anotasi: tipenya dibaca dari cara
                // BADAN fungsi yang dipanggil memakai parameter itu (`f(x)` di `iterator::map`).
                let dari_callee = match &args[i] {
                    Expr::Closure { params, .. } if matches!(p.ty, TypeExpr::Inferred)
                        && params.iter().any(|q| matches!(q.ty, TypeExpr::Inferred)) =>
                    {
                        let diketahui: Vec<Option<JrgType>> = (0..args.len()).map(|j|
                            arg_vals[j].as_ref().map(|v| v.ty.clone()).or_else(|| concrete_tys[j].clone())).collect();
                        self.tipe_param_closure_dari_callee(name, &decl, args, i, params.len(), &diketahui)
                    }
                    // Parameter bertipe `fn(T) -> K` yang `K`-nya belum terikat: parameter closure-nya
                    // sudah bisa diketik dari ikatan sejauh ini, dan hasilnya lalu mengikat `K`.
                    Expr::Closure { params, .. } if params.iter().any(|q| matches!(q.ty, TypeExpr::Inferred)) => {
                        match &p.ty {
                            TypeExpr::Fn(f) if f.params.len() == params.len() => {
                                let mut ts = Vec::with_capacity(f.params.len());
                                for t in &f.params {
                                    match self.resolve_type_with_subst(t, &subst) { Ok(x) => ts.push(x), Err(_) => break }
                                }
                                if ts.len() == f.params.len() { Some(ts) } else { None }
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };
                let hasil_slice = matches!(&p.ty, TypeExpr::Fn(f)
                    if matches!(f.ret.as_deref(), Some(TypeExpr::Slice(_))));
                let v = match (&args[i], dari_callee) {
                    (Expr::Closure { params, body, is_move, span: sc, .. }, Some(ts)) if hasil_slice =>
                        self.closure_dgn_params_luruh(params, body, &ts, *is_move, *sc)?,
                    (Expr::Closure { params, body, is_move, span: sc, .. }, Some(ts)) =>
                        self.closure_dgn_params(params, body, &ts, *is_move, *sc)?,
                    // Fungsi BERNAMA di parameter `fn(K, ..)` yang parameternya sudah terikat: tipe
                    // parameternya jadi tempat yang dituju, seperti closure di atas.
                    (Expr::Ident(..), _) if petunjuk.is_none() => {
                        let harap_fn = match &p.ty {
                            TypeExpr::Fn(f) => {
                                let ts: Vec<JrgType> = f.params.iter()
                                    .filter_map(|t| self.resolve_type_with_subst(t, &subst).ok()).collect();
                                (ts.len() == f.params.len()).then(|| JrgType::Fn(ts, Box::new(JrgType::Any)))
                            }
                            _ => None,
                        };
                        self.codegen_expr(&args[i], harap_fn.as_ref())?
                    }
                    _ => self.codegen_expr(&args[i], petunjuk.as_ref())?,
                };
                if is_bare_array_type(&p.ty) {
                    let elem = match &v.ty {
                        JrgType::Array(e, _) => (**e).clone(),
                        JrgType::Slice(e) => (**e).clone(),
                        other => return Err(format!("[{}:{}] parameter `array` butuh nilai array/slice, dapat `{}`", span.lo.0, span.hi.0, other.display_name())),
                    };
                    subst.insert(format!("$arrparam{}", i), elem);
                } else if matches!(p.ty, TypeExpr::Inferred) {
                    // Tipe argumen APA ADANYA jadi tipe parameternya.
                    subst.insert(format!("$param{}", i), v.ty.clone());
                } else if let TypeExpr::Named(nt) = &p.ty {
                    if nt.args.is_empty() && !matches!(nt.name.as_str(), "Option" | "Result") {
                        subst.insert(nt.name.clone(), v.ty.clone());
                    } else {
                        self.satukan(&p.ty, &v.ty, &generik, &mut subst);
                    }
                } else {
                    self.satukan(&p.ty, &v.ty, &generik, &mut subst);
                }
                arg_vals[i] = Some(v);
            }
        }
        // Parameter tipe yang cuma muncul di tipe KEMBALIAN (`fn baru<T>() -> Kotak<T>`) diikat
        // dari tipe yang diharapkan situs pemanggilnya.
        if let (Some(rt), Some(h)) = (&decl.ret_type, harap) {
            if generik.iter().any(|g| !subst.contains_key(g)) { self.satukan(rt, h, &generik, &mut subst); }
        }
        // Yang tetap tidak terlihat dari mana pun jadi NILAI BERTAG (lapis B): programnya jalan
        // dengan isi apa saja, dan anotasi (`let q: Queue<int> = …`) memberi tipe yang cepat.
        for g in &generik {
            subst.entry(g.clone()).or_insert(JrgType::Any);
        }
        let mut final_args = Vec::with_capacity(args.len());
        for (i, p) in decl.params.iter().take(terpakai).enumerate() {
            let pty = if is_bare_array_type(&p.ty) {
                let elem = subst.get(&format!("$arrparam{}", i)).cloned()
                    .ok_or_else(|| format!("[{}:{}] tidak bisa menyimpulkan tipe elemen `array` untuk parameter ke-{}", span.lo.0, span.hi.0, i))?;
                JrgType::Slice(Box::new(elem))
            } else {
                self.tipe_param(p, i, &subst)?
            };
            let v = match arg_vals[i].take() {
                Some(v) => self.coerce(&v, &pty),
                None => { let v = self.codegen_expr(&args[i], Some(&pty))?; self.coerce(&v, &pty) }
            };
            // Parameter callee adalah pengikatan baru, jadi ia harus punya
            // salinannya sendiri — tanpa ini `f(s)` yang menulis `s[0]`
            // merusak milik pemanggil.
            let v = if (i == 0 && pindah.is_some()) || self.boleh_pinjam(&decl, i, &args[i], &v.ty) { v }
                    else { self.ambil_milik(&args[i], v)? };
            final_args.push((pty, v));
        }
        // Arity yang berbeda berarti FUNGSI yang berbeda: `a(5)` menghitung `y` dari
        // defaultnya, `a(5, 1)` tidak.
        let is_generic = !subst.is_empty();
        let sebagian = terpakai != decl.params.len();
        let mangled = if is_generic || sebagian {
            let type_args: Vec<JrgType> = final_args.iter().map(|(t, _)| t.clone()).collect();
            let mut dasar = if sebagian { format!("{}$ar{}", name, terpakai) } else { name.to_string() };
            // Ikatan parameter tipe ikut di nama: `baru<int>("a")` dan `baru<string>("a")`
            // berargumen sama tapi dua fungsi berbeda.
            for g in &generik {
                if let Some(t) = subst.get(g) { dasar.push_str(&format!("$g{}", t.mangle_frag())); }
            }
            mangle_fn(&dasar, &type_args)
        } else {
            name.to_string()
        };
        // Dicatat pada nama TERMANGEL, bukan nama dasarnya: sebuah `fn`
        // bersarang berparameter tanpa anotasi dimonomorfisasi per tipe, dan
        // yang dicari `codegen_function` adalah nama akhirnya.
        if bersarang { self.catat_sarang(&mangled); }
        let ret_ty = self.tipe_return_duga(&decl, &subst, &mangled, None, terpakai)?;
        self.enqueue_fn_arity(name, &decl, subst, mangled.clone(), None, terpakai);
        self.emit_call(&mangled, &final_args, &ret_ty, None)
    }

    pub(crate) fn call_static_method(&mut self, self_ty_name: &str, decl: &FnDecl, args: &[Expr], span: Span) -> Result<Val, String> {
        if decl.params.len() != args.len() {
            return Err(format!("[{}:{}] `{}::{}` butuh {} argumen, diberi {}", span.lo.0, span.hi.0, self_ty_name, decl.name, decl.params.len(), args.len()));
        }
        let mut subst = TypeSubst::new();
        self.ikat_diri(self_ty_name, &mut subst);
        for (i, (p, a)) in decl.params.iter().zip(args.iter()).enumerate() {
            if matches!(p.ty, TypeExpr::Inferred) {
                let v = self.codegen_expr(a, None)?;
                subst.insert(format!("$param{}", i), v.ty.clone());
            }
        }
        let mut final_args = Vec::with_capacity(args.len());
        for (i, (p, a)) in decl.params.iter().zip(args.iter()).enumerate() {
            let pty = self.tipe_param(p, i, &subst)?;
            let v = self.codegen_expr(a, Some(&pty))?;
            let v = self.coerce(&v, &pty);
            let v = if self.boleh_pinjam(decl, i, a, &v.ty) { v } else { self.ambil_milik(a, v)? };
            final_args.push((pty.clone(), v));
        }
        let mangled = if subst.is_empty() {
            mangle_method(self_ty_name, &decl.name)
        } else {
            let tys: Vec<JrgType> = final_args.iter().map(|(t, _)| t.clone()).collect();
            mangle_fn(&mangle_method(self_ty_name, &decl.name), &tys)
        };
        let u = self.tables.unit_tipe.get(super::generik::dasar(self_ty_name)).copied().unwrap_or(0);
        self.unit_simbol.entry(mangled.clone()).or_insert(u);
        let ret_ty = self.tipe_return_duga(decl, &subst, &mangled, None, decl.params.len())?;
        self.enqueue_fn(&decl.name, decl, subst, mangled.clone(), None);
        self.emit_call(&mangled, &final_args, &ret_ty, None)
    }

    pub fn codegen_method(&mut self, obj: &Expr, method: &str, args: &[Expr], _expected: Option<&JrgType>, span: Span) -> Result<Val, String> {
        let (optr, oty) = self.get_place(obj)?;
        // Penerima milik `var` modul dibaca lewat penunjuk: kalau method-nya atau argumennya bisa
        // menulis `var` modul, `self` disalin dulu — dulu tulisan itu ikut terlihat dari `self`.
        let (optr, oty) = if self.akar_var_modul(obj)
            && (self.metode_menulis_modul(method) || args.iter().any(|a| self.bisa_menulis_modul(a))) {
            let v = self.fresh("pen.");
            self.emit(format!("{} = load {}, ptr {}", v, oty.llvm_ty(), optr));
            let salinan = self.salin_dalam(&Val { text: v, ty: oty.clone() });
            let slot = self.alloca_typed("pen.", &oty);
            self.emit(format!("store {} {}, ptr {}", oty.llvm_ty(), salinan.text, slot));
            (slot, oty)
        } else { (optr, oty) };
        // Channel: method-nya adalah `chan::nama` dengan penerimanya di depan.
        if let JrgType::Chan(_) = &oty {
            let mut semua = Vec::with_capacity(args.len() + 1);
            semua.push(self.lokal_sementara(optr.clone(), oty.clone(), span));
            semua.extend(args.iter().cloned());
            if let Some(r) = self.coba_builtin_chan(method, &semua, span) { return r; }
        }
        // `Atomic`/`Mutex`: sama, lewat `atomic::nama`/`mutex::nama`.
        if let JrgType::Sinkron(kunci) = &oty {
            let ns = if *kunci { "mutex" } else { "atomic" };
            if let Some(p) = crate::interpreter::pesan_jumlah_arg_metode(ns, method, args.len()) {
                return Err(crate::llvm::galat::tandai(span, p));
            }
            if method == "to_string" && args.is_empty() {
                let penerima = self.lokal_sementara(optr.clone(), oty.clone(), span);
                if let Some(r) = self.try_builtin_call("str", &[penerima], _expected, span) { return r; }
            }
            if crate::sinkron::metode_ada(ns, method) {
                let mut semua = Vec::with_capacity(args.len() + 1);
                semua.push(self.lokal_sementara(optr.clone(), oty.clone(), span));
                semua.extend(args.iter().cloned());
                return self.panggil_sinkron(&format!("{}::{}", ns, method), &semua, span);
            }
        }

        if let JrgType::Enum(ename) = &oty {
            if let Some(t) = self.option_args.get(ename).cloned() {
                return self.codegen_option_method(&optr, ename, &t, method, args, span);
            }
            if let Some((t, e)) = self.result_args.get(ename).cloned() {
                return self.codegen_result_method(&optr, ename, &t, &e, method, args, span);
            }
        }
        if oty == JrgType::Str {
            if args.is_empty() && (method == "to_int" || method == "to_float") {
                let sv = self.load_place(&optr, &JrgType::Str);
                return if method == "to_int" { self.pintu_parse_bulat(&sv, "str::to_int", span) }
                       else { self.pintu_parse_pecahan(&sv, "str::to_float", span) };
            }
            if let Some(r) = self.metode_str_berargumen(&optr, method, args, span) { return r; }
            if let Some(r) = self.try_str_method(&optr, method, span) { return r; }
        }
        // Method pada koleksi.
        if matches!(oty, JrgType::Array(_, _) | JrgType::Slice(_)) {
            if method == "len" && args.is_empty() {
                let v = self.load_place(&optr, &oty);
                return self.panjang_koleksi(&v, span);
            }
            let v = self.load_place(&optr, &oty);
            let elem = match &oty {
                JrgType::Array(e, _) | JrgType::Slice(e) => (**e).clone(),
                _ => unreachable!("sudah disaring `matches!` di atas"),
            };
            if let Some(r) = self.metode_opsi_koleksi(&v, &elem, method, args, span) { return r; }
            if method == "pop" && args.is_empty() { return self.metode_pop(&v, &elem, span); }
            // Bentuk method dialihkan ke `arr::` yang sudah ada — `a.min()`
            // dan `arr::min(a)` adalah satu hal yang sama di acuannya, jadi
            // yang ditambahkan pengalihan, bukan implementasi kedua.
            if method == "join" && args.len() == 1 {
                let semua = [obj.clone(), args[0].clone()];
                if let Some(r) = self.coba_builtin_str("str::join", &semua, span) { return r; }
            }
            const METODE_ARR: &[&str] =
                &["push", "contains", "sum", "reverse", "sort", "min", "max"];
            if METODE_ARR.contains(&method) {
                let mut semua = Vec::with_capacity(args.len() + 1);
                semua.push(obj.clone());
                semua.extend(args.iter().cloned());
                if let Some(r) = self.coba_builtin_arr(
                    &format!("arr::{}", method), &semua, span)
                {
                    return r;
                }
            }
            if let Some(r) = self.coba_hof_koleksi(&v, method, args, span) { return r; }
        }
        // `d.len()` sama dengan `dict::len(d)`, dan seterusnya untuk ketujuh nama di
        // bawah — diukur di interpreter dan VM, yang memang menjawab sama untuk …
        if matches!(oty, JrgType::Dict(_) | JrgType::DictCampur(_)) {
            const METODE_DICT: &[&str] =
                &["len", "is_empty", "has", "get", "keys", "values", "remove", "merge"];
            if METODE_DICT.contains(&method) {
                let mut semua = Vec::with_capacity(args.len() + 1);
                semua.push(obj.clone());
                semua.extend(args.iter().cloned());
                if let Some(r) = self.coba_builtin_dict(
                    &format!("dict::{}", method), &semua, None, span)
                {
                    return r;
                }
            }
        }

        if oty == JrgType::Str && matches!(method, "sub" | "substring" | "slice") && matches!(args.len(), 1 | 2) {
            let v = self.load_place(&optr, &oty);
            return self.potong_string(&v, &Some(Box::new(args[0].clone())), &args.get(1).map(|e| Box::new(e.clone())));
        }
        let self_ty_name = match &oty {
            JrgType::Struct(n) | JrgType::Enum(n) => n.clone(),
            _ => {
                // Method yang ADA di mesin acuan untuk tipe ini disebut celah backend, bukan dituduh
                // tidak ada; nama tipenya nama bahasa (`array`), bukan `[3]i64`.
                let ns = match &oty {
                    JrgType::Array(_, _) | JrgType::Slice(_) => Some("arr"),
                    JrgType::Str => Some("str"),
                    JrgType::Dict(_) | JrgType::DictCampur(_) => Some("dict"),
                    _ => None,
                };
                let ada = ns.map_or(false, |n| crate::vm::ada_builtin(&format!("{}::{}", n, method)));
                // Nama `str::` yang penurunnya cuma ada di pintu namespace: satu badan, dua pintu.
                const STR_LEWAT_NAMESPACE: &[&str] = &["capitalize", "title", "snake_case", "camel_case",
                    "swap_case", "trim_start", "trim_end", "is_alpha", "is_alphanumeric", "is_lower",
                    "is_upper", "is_numeric", "words", "bytes", "byte_len"];
                let lewat = (oty == JrgType::Str && STR_LEWAT_NAMESPACE.contains(&method))
                    || (ns == Some("arr") && ["product", "concat", "drop", "get", "get_or", "skip", "swap", "take"].contains(&method))
                    || (oty == JrgType::Str && ["format", "to_string"].contains(&method))
                    || (ns == Some("dict") && method == "get_or");
                if let (true, Some(n)) = (lewat, ns) {
                    let mut semua = Vec::with_capacity(args.len() + 1);
                    // Literal teks diserahkan APA ADANYA: `"{} {}".format(..)` diurai saat kompilasi.
                    semua.push(match obj {
                        Expr::Literal(LitKind::Str(_) | LitKind::RawStr(_), _) => obj.clone(),
                        _ => self.lokal_sementara(optr.clone(), oty.clone(), span),
                    });
                    semua.extend(args.iter().cloned());
                    if let Some(r) = self.try_builtin_call(&format!("{}::{}", n, method), &semua, _expected, span) { return r; }
                }
                // `.to_string()` atas array dan dict: tampilannya, sama dengan `str(x)`.
                if method == "to_string" && args.is_empty() && matches!(ns, Some("arr") | Some("dict")) {
                    let penerima = self.lokal_sementara(optr.clone(), oty.clone(), span);
                    if let Some(r) = self.try_builtin_call("str", &[penerima], _expected, span) { return r; }
                }
                // Lapis ketiga melayani bentuk method juga; penerimanya SUDAH dievaluasi, jadi
                // ia diserahkan lewat lokal bernama mustahil, bukan dievaluasi sekali lagi.
                if let Some(kunci) = ns.filter(|_| ada).and_then(|n| self.kunci_lapis(n, method)) {
                    let mut semua = Vec::with_capacity(args.len() + 1);
                    semua.push(self.lokal_sementara(optr.clone(), oty.clone(), span));
                    semua.extend(args.iter().cloned());
                    return self.call_named_function(&kunci, &semua, _expected, span);
                }
                let pesan = if ada {
                    format!("method `.{}` untuk `{}` ada di pustaka standar, tapi backend `--emit-llvm` \
                             belum mendukungnya\nbantuan: jalankan tanpa `--emit-llvm` (interpreter) \
                             atau dengan `--vm`", method, oty.nama_typeof())
                } else {
                    format!("tipe `{}` tidak punya method `{}`", oty.nama_typeof(), method)
                };
                return Err(crate::llvm::galat::tandai(span, pesan));
            }
        };
        let decl = self.tables.find_method(&self_ty_name, method).cloned()
            .ok_or_else(|| format!("[{}:{}] `{}` tidak punya method `{}`", span.lo.0, span.hi.0, self_ty_name, method))?;
        let takes_self = decl.params.first().map(|p| pattern_simple_name_pub(&p.pattern).as_deref() == Some("self")).unwrap_or(false);
        if !takes_self {
            return Err(format!("[{}:{}] `{}` bukan instance method (tidak punya parameter `self`)", span.lo.0, span.hi.0, method));
        }
        self.call_method_impl(&self_ty_name, &decl, &optr, &oty, args, span)
    }

    /// Pembungkus publik supaya `val_to_str` bisa memanggil hook tampilan
    /// lewat jalur pemanggilan metode yang SAMA — termasuk penyimpulan tipe
    /// parameter dan monomorfisasinya.
    pub(crate) fn call_method_impl_pub(&mut self, self_ty_name: &str, decl: &FnDecl, self_ptr: &str, self_ty: &JrgType, args: &[Expr], span: Span) -> Result<Val, String> {
        self.call_method_impl(self_ty_name, decl, self_ptr, self_ty, args, span)
    }

    pub(crate) fn call_method_impl(&mut self, self_ty_name: &str, decl: &FnDecl, self_ptr: &str, self_ty: &JrgType, args: &[Expr], span: Span) -> Result<Val, String> {
        self.call_method_impl_dgn(self_ty_name, decl, self_ptr, self_ty, args, false, span)
    }

    /// `milik`: penerimanya DIPINDAHKAN (`x = x.m(..)`), jadi method berjalan langsung di atas
    /// tempat `x` tanpa menyalinnya.
    pub(crate) fn call_method_impl_dgn(&mut self, self_ty_name: &str, decl: &FnDecl, self_ptr: &str, self_ty: &JrgType, args: &[Expr], milik: bool, span: Span) -> Result<Val, String> {
        let rest_params = &decl.params[1..];
        if rest_params.len() != args.len() {
            return Err(format!("[{}:{}] method `{}.{}` butuh {} argumen, diberi {}", span.lo.0, span.hi.0, self_ty_name, decl.name, rest_params.len(), args.len()));
        }
        // Parameter method tanpa anotasi disimpulkan dari argumennya, sama seperti
        // fungsi bebas.
        let mut subst = TypeSubst::new();
        self.ikat_generik_penerima(self_ty_name, &decl.name, &mut subst);
        // Nilai yang sudah diturunkan di sini DIPAKAI LAGI di bawah: menurunkannya dua kali
        // menjalankan efek samping argumennya dua kali.
        let mut siap: Vec<Option<Val>> = vec![None; args.len()];
        for (i, (p, a)) in rest_params.iter().zip(args.iter()).enumerate() {
            if matches!(p.ty, TypeExpr::Inferred) {
                // Closure tanpa anotasi: tipe parameternya dari cara badan METHOD memakainya.
                let dari_badan = match a {
                    Expr::Closure { params, .. } if params.iter().any(|q| matches!(q.ty, TypeExpr::Inferred)) => {
                        let mut di_sini = super::infer::Lokal::new();
                        for lapis in &self.locals {
                            for (n, (_, t)) in lapis { di_sini.insert(n.clone(), t.clone()); }
                        }
                        let generik = self.cur_fn_generics.clone();
                        let mut tipe: Vec<Option<JrgType>> = vec![Some(self_ty.clone())];
                        for (j, b) in args.iter().enumerate() {
                            tipe.push(if j == i { None } else { self.tipe_ekspresi(b, &di_sini, &generik, "") });
                        }
                        let simbol = mangle_method(self_ty_name, &decl.name);
                        let u = self.tables.unit_tipe.get(super::generik::dasar(self_ty_name)).copied().unwrap_or(0);
                        self.unit_simbol.entry(simbol.clone()).or_insert(u);
                        self.tipe_param_fn_dari_badan(&simbol, decl, &tipe, i + 1, params.len(), 0)
                    }
                    _ => None,
                };
                let v = match (a, dari_badan) {
                    (Expr::Closure { params, body, is_move, span: sc, .. }, Some(ts)) =>
                        self.closure_dgn_params(params, body, &ts, *is_move, *sc)?,
                    _ => self.codegen_expr(a, None)?,
                };
                // `i + 1`, BUKAN `i`: `rest_params` sudah memotong `self`, sementara
                // `codegen_function` menelusuri `decl.params` yang lengkap.
                subst.insert(format!("$param{}", i + 1), v.ty.clone());
                siap[i] = Some(v);
            }
        }
        let mut final_args = Vec::new();
        for (i, (p, a)) in rest_params.iter().zip(args.iter()).enumerate() {
            let pty = self.tipe_param(p, i + 1, &subst)?;
            let v = match siap[i].take() {
                Some(v) => v,
                None => self.codegen_expr(a, Some(&pty))?,
            };
            let v = self.coerce(&v, &pty);
            let v = if self.boleh_pinjam(decl, i + 1, a, &v.ty) { v } else { self.ambil_milik(a, v)? };
            final_args.push((pty.clone(), v));
        }
        self.milik_berikutnya = milik;
        self.pancarkan_panggilan_metode(self_ty_name, decl, self_ptr, self_ty, final_args, subst)
    }

    /// Ekor bersama tiap pemanggilan metode: mangling, antrean monomorfisasi, dan
    /// pemancarannya.
    pub(crate) fn pancarkan_panggilan_metode(
        &mut self,
        self_ty_name: &str,
        decl: &FnDecl,
        self_ptr: &str,
        self_ty: &JrgType,
        final_args: Vec<(JrgType, Val)>,
        mut subst: TypeSubst,
    ) -> Result<Val, String> {
        self.ikat_generik_penerima(self_ty_name, &decl.name, &mut subst);
        // Nama termangling ikut membawa tipe argumen kalau ada yang
        // disimpulkan — tanpa itu dua pemanggilan dengan tipe berbeda akan
        // memakai satu badan fungsi yang sama dan salah satunya salah.
        let milik = std::mem::take(&mut self.milik_berikutnya);
        let mangled = if subst.is_empty() {
            mangle_method(self_ty_name, &decl.name)
        } else {
            let tys: Vec<JrgType> = final_args.iter().map(|(t, _)| t.clone()).collect();
            mangle_fn(&mangle_method(self_ty_name, &decl.name), &tys)
        };
        // Varian ber-`self` MILIK punya simbolnya sendiri: badannya tidak menyalin penerima.
        let mangled = if milik { format!("{}{}", mangled, super::codegen_tumbuh::AKHIRAN_MILIK) } else { mangled };
        let u = self.tables.unit_tipe.get(super::generik::dasar(self_ty_name)).copied().unwrap_or(0);
        self.unit_simbol.entry(mangled.clone()).or_insert(u);
        let ret_ty = self.tipe_return_duga(decl, &subst, &mangled, Some(self_ty), decl.params.len())?;
        self.enqueue_fn(&decl.name, decl, subst, mangled.clone(), Some(self_ty.clone()));
        self.emit_call(&mangled, &final_args, &ret_ty, Some(self_ptr.to_string()))
    }

    /// Panggil metode operator dengan operand yang sudah diturunkan.
    pub(crate) fn panggil_operator(
        &mut self,
        tipe: &str,
        decl: &FnDecl,
        self_ptr: &str,
        self_ty: &JrgType,
        arg: Option<Val>,
        span: Span,
    ) -> Result<Val, String> {
        let rest = &decl.params[1..];
        let mut subst = TypeSubst::new();
        self.ikat_generik_penerima(tipe, &decl.name, &mut subst);
        let mut final_args = Vec::new();
        if let Some(v) = arg {
            if rest.len() != 1 {
                return Err(format!(
                    "[{}:{}] {}",
                    span.lo.0, span.hi.0,
                    crate::interpreter::operator::pesan_arity(tipe, &decl.name, decl.params.len())
                ));
            }
            if matches!(rest[0].ty, TypeExpr::Inferred) {
                subst.insert("$param1".to_string(), v.ty.clone());
            }
            let pty = self.tipe_param(&rest[0], 1, &subst)?;
            let v = self.coerce(&v, &pty);
            final_args.push((pty, v));
        } else if !rest.is_empty() {
            return Err(format!(
                "[{}:{}] {}",
                span.lo.0, span.hi.0,
                crate::interpreter::operator::pesan_arity_unary(tipe, &decl.name, decl.params.len())
            ));
        }
        self.pancarkan_panggilan_metode(tipe, decl, self_ptr, self_ty, final_args, subst)
    }

    pub(crate) fn emit_call(&mut self, mangled: &str, args: &[(JrgType, Val)], ret_ty: &JrgType, self_ptr: Option<String>) -> Result<Val, String> {
        let mut argstr = Vec::new();
        if let Some(sp) = self_ptr { argstr.push(format!("ptr {}", sp)); }
        for (t, v) in args {
            if *t == JrgType::Unit { continue; }
            argstr.push(format!("{} {}", t.llvm_ty(), v.text));
        }
        let qname = llvm_quoted_ident(mangled);
        if *ret_ty == JrgType::Unit {
            self.emit(format!("call void @{}({})", qname, argstr.join(", ")));
            Ok(Val::unit())
        } else {
            let r = self.fresh("call.");
            self.emit(format!("{} = call {} @{}({})", r, ret_ty.llvm_ty(), qname, argstr.join(", ")));
            Ok(Val { text: r, ty: ret_ty.clone() })
        }
    }

}
