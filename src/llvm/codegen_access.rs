// src/llvm/codegen_access.rs — jrgc LLVM backend — src/llvm/codegen_access.rs
// Places (lvalues), aggregate construction, calls, and Option/Result +

#![allow(dead_code)]

use crate::parser::*;
use crate::lexer::Span;

use super::codegen::{Codegen, Val};
use super::types::*;

pub fn pattern_simple_name_pub(p: &Pattern) -> Option<String> {
    match p {
        Pattern::Ident { name, .. } => Some(name.clone()),
        Pattern::Ref { inner, .. } => pattern_simple_name_pub(inner),
        _ => None,
    }
}

/// Nilai yang jenisnya lain dari tempat yang ditujunya. Dua mesin acuan membiarkan nilainya
/// apa adanya; LLVM butuh satu jenis per tempat, jadi ia menolak alih-alih mengubahnya.
pub fn pesan_ganti_jenis(dari: &str, ke: &str) -> String {
    format!("nilai `{dari}` masuk ke tempat yang bertipe `{ke}` — backend LLVM butuh satu jenis \
             per tempat dan tidak mengubah jenis nilai diam-diam\n\
             bantuan: tulis konversinya (`x as {ke}`; literal bulat di tempat pecahan ditulis `7.0`), \
             samakan jenis kedua sisinya, atau jalankan tanpa `--emit-llvm`")
}

impl Codegen {
    // ── coercion ─────────────────────────────────────────────────────────

    /// `coerce` yang boleh mengganti jenis: promosi bulat ke pecahan, rune lawan angka, dan
    /// truthiness. Untuk situs yang MEMANG meminta itu (kondisi, argumen `math::`, `as`).
    pub fn coerce_longgar(&mut self, v: &Val, target: &JrgType) -> Val {
        let lama = std::mem::replace(&mut self.koersi_longgar, true);
        let r = self.coerce(v, target);
        self.koersi_longgar = lama;
        r
    }

    /// Argumen yang HARUS bilangan bulat (jumlah, indeks, milidetik): `rune` ditolak dengan
    /// kalimat acuan, tidak dibaca sebagai kodenya. Jenis lain sudah ditolak `coerce`.
    pub fn bulat_arg(&mut self, v: &Val, siapa: &str, span: crate::lexer::Span) -> Result<Val, String> {
        if v.ty == JrgType::Rune {
            let awalan = if siapa.is_empty() { String::new() } else { format!("{}: ", siapa) };
            return Err(super::galat::tandai(span, format!(
                "{}butuh bilangan bulat, dapat `{}`", awalan, v.ty.nama_typeof())));
        }
        Ok(self.coerce_longgar(v, &JrgType::I64))
    }

    pub fn coerce(&mut self, v: &Val, target: &JrgType) -> Val {
        if v.ty == *target { return v.clone(); }
        if *target == JrgType::Unit { return Val::unit(); }
        // Masuk-kotak adalah sebuah KOERSI, bukan operasi yang harus ditulis di tiap
        // situs: sebuah `i64` yang menuju tempat bertipe campur memang berarti …
        if *target == JrgType::Any {
            let sp = crate::lexer::Span { lo: crate::lexer::BytePos(0), hi: crate::lexer::BytePos(0), file_id: 0 };
            if let Ok(k) = self.kotak_any(v, sp) { return k; }
        }
        // Dict campur ke `dict` bernilai bertag: penyangganya sama, yang hilang cuma tabel per-kunci.
        if let (JrgType::DictCampur(_), JrgType::Dict(e)) = (&v.ty, target) {
            if **e == JrgType::Any { return Val { text: v.text.clone(), ty: target.clone() }; }
        }
        // Instans struct generik ke instans lain dari struct yang sama (`Adegan<Menu>` ke
        // `Adegan<campur>`): field demi field.
        if let (JrgType::Struct(a), JrgType::Struct(b)) = (&v.ty, target) {
            if a != b && super::generik::dasar(a) == super::generik::dasar(b) {
                let (la, lb) = (self.types.structs.get(a).cloned(), self.types.structs.get(b).cloned());
                if let (Some(la), Some(lb)) = (la, lb) {
                    if la.fields.len() == lb.fields.len() {
                        let mut kini = "undef".to_string();
                        for (i, ((_, ta), (_, tb))) in la.fields.iter().zip(lb.fields.iter()).enumerate() {
                            let e = self.fresh("sgi.");
                            self.emit(format!("{} = extractvalue {} {}, {}", e, v.ty.llvm_ty(), v.text, i));
                            let c = self.coerce(&Val { text: e, ty: ta.clone() }, tb);
                            let r = self.fresh("sgo.");
                            self.emit(format!("{} = insertvalue {} {}, {} {}, {}", r, target.llvm_ty(), kini, tb.llvm_ty(), c.text, i));
                            kini = r;
                        }
                        return Val { text: kini, ty: target.clone() };
                    }
                }
            }
        }
        // Fungsi ke fungsi beda tipe yang menyebut nilai bertag: dibungkus adaptor.
        if let (JrgType::Fn(p1, r1), JrgType::Fn(p2, r2)) = (&v.ty, target) {
            let bisa = |a: &JrgType, b: &JrgType| a == b || *a == JrgType::Any || *b == JrgType::Any
                || matches!((a, b), (JrgType::Array(x, _), JrgType::Slice(y)) if super::codegen_any::luruh(x) == **y || **y == JrgType::Any);
            if p1.len() == p2.len() && p1.iter().zip(p2.iter()).all(|(a, b)| bisa(a, b)) && (bisa(r1, r2) || **r2 == JrgType::Unit) {
                return self.adaptasi_fungsi(v, target);
            }
        }
        // `nil` boleh mengisi slot nilai-fungsi yang belum terisi.
        if matches!(v.ty, JrgType::Nil) {
            if let JrgType::Fn(..) = target {
                return Val { text: "zeroinitializer".to_string(), ty: target.clone() };
            }
        }

        // `array<T>` menuju tempat ber-elemen campur: tiap elemennya masuk kotak.
        if let (JrgType::Slice(e1), JrgType::Slice(e2)) = (&v.ty, target) {
            if **e2 == JrgType::Any && **e1 != JrgType::Any {
                let e1 = (**e1).clone();
                if let Some(r) = self.kotak_tiap_elemen(v, &e1) { return r; }
            }
            // Kebalikannya, untuk elemen skalar: tiap kotak dibuka dan diperiksa tagnya.
            let skalar = !matches!(**e2, JrgType::Any | JrgType::Slice(_) | JrgType::Array(..) | JrgType::Dict(_));
            if **e1 == JrgType::Any && skalar {
                let e2 = (**e2).clone();
                if let Some(r) = self.buka_tiap_elemen(v, &e2) { return r; }
            }
        }
        // `Option<nilai bertag>` (mis. `None` tanpa petunjuk) menuju `Option<T>`: isinya dibuka, tagnya diperiksa.
        if let (JrgType::Enum(a), JrgType::Enum(b)) = (&v.ty, target) {
            if let (Some(JrgType::Any), Some(tb)) = (self.option_args.get(a).cloned(), self.option_args.get(b).cloned()) {
                if tb != JrgType::Any && super::codegen_any::tag_untuk(&tb).is_some() {
                    if let Some(r) = self.buka_isi_opsi(v, &tb, b) { return r; }
                }
            }
        }
        // `Option<T>` menuju `Option` bernilai bertag: isinya (kalau ada) masuk kotak.
        if let (JrgType::Enum(a), JrgType::Enum(b)) = (&v.ty, target) {
            if let (Some(ta), Some(JrgType::Any)) = (self.option_args.get(a).cloned(), self.option_args.get(b).cloned()) {
                if ta != JrgType::Any && super::codegen_any::tag_untuk(&ta).is_some() {
                    if let Some(r) = self.kotak_isi_opsi(v, &ta, b) { return r; }
                }
            }
        }
        if let (JrgType::Dict(e1), JrgType::Dict(e2)) = (&v.ty, target) {
            if **e2 == JrgType::Any && **e1 != JrgType::Any {
                let e1 = (**e1).clone();
                if let Some(r) = self.kotak_tiap_nilai(v, &e1) { return r; }
            }
        }
        // Peluruhan di DALAM slice atau dict: `array<[2]i64>` jadi `array<array<i64>>`.
        if let (JrgType::Slice(e1), JrgType::Slice(e2)) = (&v.ty, target) {
            if e1 != e2 && (super::codegen_any::luruh(e1) == **e2 || isi_jadi_bertag(e1, e2) || isi_dibuka(e1, e2)) {
                let (e1, e2) = ((**e1).clone(), (**e2).clone());
                if let Some(r) = self.ubah_tiap_elemen(v, &e1, &e2) { return r; }
            }
        }
        if let (JrgType::Dict(e1), JrgType::Dict(e2)) = (&v.ty, target) {
            if e1 != e2 && (super::codegen_any::luruh(e1) == **e2 || isi_jadi_bertag(e1, e2)) {
                let (e1, e2) = ((**e1).clone(), (**e2).clone());
                if let Some(r) = self.ubah_tiap_nilai(v, &e1, &e2) { return r; }
            }
        }
        // Peluruhan BERSARANG: `[2][1]i64` jadi `array<array<i64>>`.
        if let (JrgType::Array(e1, n), JrgType::Slice(e2)) = (&v.ty, target) {
            if e1 != e2 && **e1 != JrgType::Unit {
                let (e1, e2, n) = ((**e1).clone(), (**e2).clone(), *n);
                let tempat = self.spill_to_place(v);
                let buf = self.buffer_baru(&n.to_string(), &e2);
                for i in 0..n {
                    let sp = self.fresh("nds.");
                    self.emit(format!("{} = getelementptr {}, ptr {}, i64 0, i64 {}",
                        sp, v.ty.llvm_ty(), tempat, i));
                    let sv = self.load_place(&sp, &e1);
                    let cv = self.coerce(&sv, &e2);
                    let dp = self.fresh("ndd.");
                    self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}",
                        dp, e2.llvm_ty(), buf, i));
                    self.emit(format!("store {} {}, ptr {}", e2.llvm_ty(), cv.text, dp));
                }
                return self.rakit_slice(&buf, &n.to_string(), &e2);
            }
        }
        if let JrgType::Array(e1, n) = &v.ty {
            if let JrgType::Slice(e2) = target {
                if e1 == e2 {
                    // Isinya DISALIN ke heap: slice hasilnya boleh hidup lebih lama dari fungsi ini
                    // (`return [v]`, `K { xs: [v] }`, `Some([v])`). Dulu ia menunjuk tumpahan di
                    // stack, dan pembacanya mendapat sampah sesudah fungsinya kembali.
                    let slot = self.spill_to_place(v);
                    let ptr = if *n == 0 { "null".to_string() } else {
                        let e = (**e1).clone();
                        let buf = self.buffer_baru(&n.to_string(), &e);
                        let byte = self.types.sizeof(&e) * *n;
                        self.ensure_extern("declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)", "llvm.memcpy.p0.p0.i64");
                        self.emit(format!("call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)", buf, slot, byte));
                        buf
                    };
                    let s1 = self.fresh("decayv.");
                    self.emit(format!("{} = insertvalue %JrgSlice undef, ptr {}, 0", s1, ptr));
                    let s2 = self.fresh("decayv.");
                    self.emit(format!("{} = insertvalue %JrgSlice {}, i64 {}, 1", s2, s1, n));
                    return Val { text: s2, ty: target.clone() };
                }
            }
        }
        // JENIS sebuah nilai tidak pernah berubah diam-diam: bulat jadi pecahan, rune jadi
        // angka (atau sebaliknya), dan apa pun jadi `bool` cuma lewat `coerce_longgar`.
        if !self.koersi_longgar && v.ty != JrgType::Any && v.ty != JrgType::Never {
            let ganti = (v.ty.is_int() && target.is_float())
                || ((v.ty == JrgType::Rune) != (*target == JrgType::Rune)
                    && v.ty.berbasis_int() && target.berbasis_int())
                || (*target == JrgType::Bool && v.ty != JrgType::Bool);
            if ganti {
                self.lapor_koersi(pesan_ganti_jenis(&v.ty.nama_typeof(), &target.nama_typeof()));
                return Val { text: "undef".to_string(), ty: target.clone() };
            }
        }
        // `berbasis_int`, bukan `is_int`: `rune` direpresentasikan sebagai `i32` dan
        // boleh di-cast ke/dari bilangan bulat (diukur: `'h' as int` = 104, `104 as …
        if v.ty.berbasis_int() && target.berbasis_int() {
            let from_bits = v.ty.int_bits().unwrap();
            let to_bits = target.int_bits().unwrap();
            if from_bits == to_bits { return Val { text: v.text.clone(), ty: target.clone() }; }
            let r = self.fresh("cvt.");
            if to_bits > from_bits {
                let instr = if v.ty.is_signed_int() { "sext" } else { "zext" };
                self.emit(format!("{} = {} {} {} to {}", r, instr, v.ty.llvm_ty(), v.text, target.llvm_ty()));
            } else {
                self.emit(format!("{} = trunc {} {} to {}", r, v.ty.llvm_ty(), v.text, target.llvm_ty()));
            }
            return Val { text: r, ty: target.clone() };
        }
        if v.ty.is_int() && target.is_float() {
            let r = self.fresh("cvt.");
            let instr = if v.ty.is_signed_int() { "sitofp" } else { "uitofp" };
            self.emit(format!("{} = {} {} {} to {}", r, instr, v.ty.llvm_ty(), v.text, target.llvm_ty()));
            return Val { text: r, ty: target.clone() };
        }
        // Pecahan tidak pernah jadi bilangan bulat diam-diam — di acuan tidak ada konversi
        // implisit yang membuang informasi. Dulu `1 + 2.5` = 3 dan `1 == 1.5` = true di sini.
        if v.ty.is_float() && target.is_int() {
            self.lapor_koersi(crate::interpreter::konversi::pesan_tanpa_potong_diam(
                "float", &target.nama_typeof()));
            return Val { text: "undef".to_string(), ty: target.clone() };
        }
        if v.ty.is_float() && target.is_float() {
            let from_bits = if v.ty == JrgType::F64 { 64 } else { 32 };
            let to_bits = if *target == JrgType::F64 { 64 } else { 32 };
            let r = self.fresh("cvt.");
            let instr = if to_bits > from_bits { "fpext" } else { "fptrunc" };
            self.emit(format!("{} = {} {} {} to {}", r, instr, v.ty.llvm_ty(), v.text, target.llvm_ty()));
            return Val { text: r, ty: target.clone() };
        }
        // ── ke `bool`: truthiness, bukan pemotongan bit ────────────────────
        if *target == JrgType::Bool && v.ty == JrgType::Nil {
            return Val { text: "0".to_string(), ty: JrgType::Bool };
        }
        if *target == JrgType::Bool && v.ty.is_int() {
            let r = self.fresh("tri.");
            self.emit(format!("{} = icmp ne {} {}, 0", r, v.ty.llvm_ty(), v.text));
            return self.from_i1(&r);
        }
        if *target == JrgType::Bool && v.ty.is_float() {
            // `one` = ordered-and-not-equal, jadi NaN menghasilkan **false**.
            let r = self.fresh("trf.");
            self.emit(format!("{} = fcmp one {} {}, 0.0", r, v.ty.llvm_ty(), v.text));
            return self.from_i1(&r);
        }
        if *target == JrgType::Bool && v.ty == JrgType::Str {
            // String: panjang, bukan isi. Diukur: `"0"` truthy.
            let (_, len) = self.split_str(v);
            let r = self.fresh("trs.");
            self.emit(format!("{} = icmp ne i64 {}, 0", r, len));
            return self.from_i1(&r);
        }

        // Sisa aturan truthiness acuan: yang kosong falsy, rune/struct/enum/fungsi selalu truthy.
        if *target == JrgType::Bool {
            let tetap = match &v.ty {
                JrgType::Unit => Some(false),
                JrgType::Array(_, n) => Some(*n != 0),
                JrgType::Tuple(ts) => Some(!ts.is_empty()),
                JrgType::Rune | JrgType::Struct(_) | JrgType::Enum(_) | JrgType::Fn(..) => Some(true),
                _ => None,
            };
            if let Some(b) = tetap {
                return Val { text: if b { "1" } else { "0" }.to_string(), ty: JrgType::Bool };
            }
            if matches!(v.ty, JrgType::Slice(_) | JrgType::Dict(_)) {
                let (n, r) = (self.fresh("trn."), self.fresh("tra."));
                self.emit(format!("{} = extractvalue {} {}, 1", n, v.ty.llvm_ty(), v.text));
                self.emit(format!("{} = icmp ne i64 {}, 0", r, n));
                return self.from_i1(&r);
            }
        }
        // Tuple ke tuple sama panjang: elemen demi elemen (`(i64, [2]i64)` ke `(i64, array<i64>)`).
        if let (JrgType::Tuple(a), JrgType::Tuple(b)) = (&v.ty, target) {
            if a.len() == b.len() && !a.is_empty() {
                let (a, b) = (a.clone(), b.clone());
                let mut kini = "undef".to_string();
                for (i, (ta, tb)) in a.iter().zip(b.iter()).enumerate() {
                    let e = self.fresh("tpe.");
                    self.emit(format!("{} = extractvalue {} {}, {}", e, v.ty.llvm_ty(), v.text, i));
                    let c = self.coerce(&Val { text: e, ty: ta.clone() }, tb);
                    let r = self.fresh("tpi.");
                    self.emit(format!("{} = insertvalue {} {}, {} {}, {}", r, target.llvm_ty(), kini, tb.llvm_ty(), c.text, i));
                    kini = r;
                }
                return Val { text: kini, ty: target.clone() };
            }
        }

        // `bool` bukan angka: `[1, 2][true]` dulu membaca elemen ke-1 di sini saja.
        if v.ty == JrgType::Bool && target.is_int() {
            self.lapor_koersi(crate::interpreter::konversi::pesan_tanpa_potong_diam(
                "bool", &target.nama_typeof()));
            return Val { text: "undef".to_string(), ty: target.clone() };
        }
        // `Never` is the type of an expression that cannot produce a value because
        // control flow already left (a `return`/`break` tail, a diverging arm).
        if v.ty == JrgType::Never {
            return Val { text: "undef".to_string(), ty: target.clone() };
        }
        if v.ty == JrgType::Any {
            if let Some(r) = self.buka_terperiksa(v, target) { return r; }
        }
        // Nothing above matched, so this is a conversion the backend has no lowering
        // for.
        self.lapor_koersi(format!(
            "tidak bisa memakai nilai bertipe `{}` sebagai `{}`{}",
            v.ty.display_name(),
            target.display_name(),
            if v.ty == JrgType::Unit {
                " — ekspresi ini tidak menghasilkan nilai. \
                 Statement `if`/`while`/`for`/`loop` bukan ekspresi di JRG: \
                 blok yang diakhiri salah satunya bernilai `()`. \
                 Pakai `match`, operator ternary `cond $ a : b`, atau `return` eksplisit"
            } else {
                ""
            }
        ));
        Val { text: "undef".to_string(), ty: target.clone() }
    }

    /// Konversi angka yang DITULIS pemakai (`x as int`, `true as u8`): pecahan dipotong dan
    /// bool jadi 0/1, sesudah `cast_eksplisit` memeriksa jangkauannya. `coerce` menolak keduanya.
    pub(crate) fn ubah_numerik(&mut self, v: &Val, target: &JrgType) -> Val {
        if v.ty.is_float() && target.is_int() {
            let r = self.fresh("cvt.");
            let instr = if target.is_signed_int() { "fptosi" } else { "fptoui" };
            self.emit(format!("{} = {} {} {} to {}", r, instr, v.ty.llvm_ty(), v.text, target.llvm_ty()));
            return Val { text: r, ty: target.clone() };
        }
        if v.ty == JrgType::Bool && target.is_int() {
            // `bool` sudah `i8`: ke lebar 8 bit cukup ganti label — `zext i8 to i8` ditolak clang.
            if target.int_bits() == Some(8) {
                return Val { text: v.text.clone(), ty: target.clone() };
            }
            let r = self.fresh("cvt.");
            self.emit(format!("{} = zext i8 {} to {}", r, v.text, target.llvm_ty()));
            return Val { text: r, ty: target.clone() };
        }
        self.coerce(v, target)
    }

    /// Indeks array/string/tuple: bilangan bulat saja, dengan kalimat acuan untuk yang lain.
    pub(crate) fn indeks_bulat(&mut self, idx: &Expr, wadah: &str, span: Span) -> Result<Val, String> {
        let iv = self.codegen_expr(idx, Some(&JrgType::I64))?;
        if !iv.ty.is_int() && !matches!(iv.ty, JrgType::Never | JrgType::Any) {
            let pesan = crate::interpreter::galat_index::tipe_index_salah(wadah, &iv.ty.nama_typeof());
            self.tempat_tiruan_sesudah(&pesan, span)?;
            return Ok(Val { text: "0".into(), ty: JrgType::I64 });
        }
        Ok(self.coerce_longgar(&iv, &JrgType::I64))
    }

    // ── places (lvalues) ────────────────────────────────────────────────

    pub fn get_place(&mut self, e: &Expr) -> Result<(String, JrgType), String> {
        // Dipasang oleh langkah sesudahnya: `e` adalah langkah ANTARA sebuah jalur tulis.
        let antara = std::mem::take(&mut self.tulis_antara);
        match e {
            // `const`/`let` modul dilipat di tempat pakai: nilainya diturunkan ke tempat sementara supaya
            // `NAMA[i]` bisa dibaca (menulis lewat konstanta sudah ditolak parser).
            Expr::Ident(name, _) if self.lookup_local(name).is_none() && !self.var_global.contains_key(&self.k(name))
                && self.tables.consts.contains_key(&self.k(name)) => {
                let v = self.codegen_expr(e, None)?;
                let slot = self.alloca_typed("cst.", &v.ty);
                self.emit(format!("store {} {}, ptr {}", v.ty.llvm_ty(), v.text, slot));
                Ok((slot, v.ty))
            }
            Expr::Ident(name, _) => self.lookup_local(name)
                .or_else(|| self.var_global.get(&self.k(name)).cloned())
                .ok_or_else(|| {
                    // Lihat catatan di `codegen_expr`: padanan bahasa lain menang
                    // atas kalimat khas situs ini.
                    if crate::interpreter::padanan_bahasa_lain(name).is_some() {
                        return crate::interpreter::pesan_nama_tak_dikenal(name, None);
                    }
                    format!("`{}` bukan variabel yang bisa dialamati", name)
                }),
            Expr::Field { obj, field, span } => {
                self.tulis_antara = true;
                let (optr, oty) = self.get_place(obj)?;
                // `d.c = 30` berarti `d["c"] = 30`, termasuk kalau `c` kunci BARU — dan itu
                // sebabnya ia dialihkan ke lengan `Index` di bawah alih-alih ke …
                if matches!(oty, JrgType::Dict(_) | JrgType::DictCampur(_)) {
                    let kunci = Expr::Index {
                        obj: obj.clone(),
                        idx: Box::new(Expr::Literal(
                            crate::parser::LitKind::Str(field.clone()), *span)),
                        span: *span,
                    };
                    self.tulis_antara = antara;
                    return self.get_place(&kunci);
                }
                self.get_field_place(&optr, &oty, field, *span)
            }
            Expr::Index { obj, idx, span } => {
                self.tulis_antara = true;
                let (optr, oty) = self.get_place(obj)?;
                // Menulis ke dict bisa MENUMBUHKANNYA (kunci baru), dan buffer barunya harus
                // disimpan balik ke tempat asalnya — kalau tidak, `d["baru"] = 1` menulis ke …
                if let JrgType::Dict(vty) = &oty {
                    let vty = (**vty).clone();
                    let d = self.load_place(&optr, &oty);
                    let k = self.kunci_dict(idx, *span)?;
                    if antara { self.wajib_kunci_ada(&d, &k)?; }
                    let (tempat, dbaru) = self.dict_tempat_tulis(&d, &k, *span)?;
                    self.emit(format!("store %JrgDict {}, ptr {}", dbaru.text, optr));
                    return Ok((tempat, vty));
                }
                // Dict bernilai campur.
                if let JrgType::DictCampur(_) = &oty {
                    let d = self.load_place(&optr, &oty);
                    let d = super::codegen_any::samakan_dict(&d);
                    let k = self.kunci_dict(idx, *span)?;
                    if antara { self.wajib_kunci_ada(&d, &k)?; }
                    let (tempat, dbaru) = self.dict_tempat_tulis(&d, &k, *span)?;
                    self.emit(format!("store %JrgDict {}, ptr {}", dbaru.text, optr));
                    return Ok((tempat, JrgType::Any));
                }
                // Tuple ditangani SEBELUM indeksnya diturunkan: elemennya
                // bertipe beda-beda, jadi indeksnya wajib konstan dan
                // diambil dari AST, bukan dari teks operand IR.
                if let JrgType::Tuple(_) = &oty {
                    return self.tempat_elemen_tuple(&optr, &oty, idx, *span);
                }
                // `p["x"] = v` atas struct adalah `p.x = v`.
                if let JrgType::Struct(_) = &oty {
                    let k = self.kunci_struct(idx, *span)?;
                    return self.get_field_place(&optr, &oty, &k, *span);
                }
                if !Self::bisa_ditulis_index(&oty) {
                    self.codegen_expr(idx, None)?;
                    let pesan = crate::interpreter::galat_index::tak_bisa_ditulis_lewat_index(&self.nama_acuan(&oty));
                    return self.tempat_tiruan_sesudah(&pesan, *span);
                }
                let wadah = oty.nama_typeof();
                let iv = self.indeks_bulat(idx, &wadah, *span)?;
                self.get_index_place(&optr, &oty, &iv, *span)
            }
            Expr::Unary { op: UnaryOp::Deref, .. } => {
                Err("dereference mentah (`*ptr`) belum didukung backend LLVM".into())
            }
            Expr::Unary { op: UnaryOp::Ref, expr, .. } | Expr::Unary { op: UnaryOp::RefMut, expr, .. } => self.get_place(expr),
            other => {
                let v = self.codegen_expr(other, None)?;
                let ptr = self.spill_to_place(&v);
                Ok((ptr, v.ty))
            }
        }
    }

    pub fn spill_to_place(&mut self, v: &Val) -> String {
        let slot = self.alloca_typed("spill.", &v.ty);
        if v.ty != JrgType::Unit {
            self.emit(format!("store {} {}, ptr {}", v.ty.llvm_ty(), v.text, slot));
        }
        slot
    }

    /// Gagal saat jalan dengan `pesan`, lalu beri tempat tiruan bertag yang tak pernah tercapai.
    fn tempat_tiruan_sesudah(&mut self, pesan: &str, span: Span) -> Result<(String, JrgType), String> {
        let lama = self.span_kini.replace(span);
        let r = self.gagal_sesudah_argumen(pesan, &[]);
        self.span_kini = lama;
        r?;
        Ok((self.alloca_typed("tiruan.", &JrgType::Any), JrgType::Any))
    }

    /// `p[k]` (baca) atas struct dengan kunci teks dari variabel: field dipilih saat jalan — dulu ditolak
    /// saat kompilasi. Hasilnya bertipe field kalau semua setipe, selain itu nilai bertag; field privat
    /// dan kunci yang tidak ada dijawab kalimat acuan.
    fn baca_struct_kunci(&mut self, ptr: &str, oty: &JrgType, idx: &Expr, span: Span) -> Result<Val, String> {
        use super::codegen_konversi::{Cetak, PENANDA};
        let JrgType::Struct(n) = oty else { unreachable!() };
        let layout = self.types.structs.get(n).cloned().ok_or_else(|| format!("struct `{}` tidak dikenal", n))?;
        let kv = self.codegen_expr(idx, Some(&JrgType::Str))?;
        let lama = self.span_kini.replace(span);
        let tipe: Vec<JrgType> = layout.fields.iter().map(|(_, t)| t.tanpa_kotak()).collect();
        let satu = tipe.first().filter(|t| tipe.iter().all(|x| x == *t)).cloned();
        let hasil_ty = satu.clone().unwrap_or(JrgType::Any);
        let hasil = self.alloca_typed("skk.", &hasil_ty);
        let (kp, kl) = self.split_str(&kv);
        self.ensure_extern("declare i32 @jrg_str_eq(ptr, i64, ptr, i64)", "jrg_str_eq");
        let usai = self.fresh_label("skk.usai.");
        let dasar = super::generik::dasar(n).to_string();
        for (f, _) in &layout.fields {
            let ft = self.intern_string(f);
            let (fp, fl) = self.split_str(&ft);
            let (r, c) = (self.fresh("skk."), self.fresh("skk."));
            self.emit(format!("{} = call i32 @jrg_str_eq(ptr {}, i64 {}, ptr {}, i64 {})", r, kp, kl, fp, fl));
            self.emit(format!("{} = icmp ne i32 {}, 0", c, r));
            let (ya, tidak) = (self.fresh_label("skk.ya."), self.fresh_label("skk.lain."));
            self.terminate(format!("br i1 {}, label %{}, label %{}", c, ya, tidak));
            self.start_block(ya);
            match crate::vm::visibilitas::periksa(&dasar, f, self.cur_self_ty.as_deref()) {
                Err(e) => self.panik(&e),
                Ok(()) => {
                    let (fptr, fty) = self.get_field_place(ptr, oty, f, span)?;
                    let v = self.load_place(&fptr, &fty);
                    let v = if satu.is_some() { v } else { self.kotak_any(&v, span)? };
                    self.emit(format!("store {} {}, ptr {}", hasil_ty.llvm_ty(), v.text, hasil));
                    self.terminate(format!("br label %{}", usai));
                }
            }
            self.start_block(tidak);
        }
        let ada: Vec<String> = layout.fields.iter().map(|(f, _)| f.clone()).collect();
        let pesan = crate::interpreter::galat_index::field_hilang(super::modul::tampil(&dasar), PENANDA, &ada);
        self.panik_sisip(&pesan, &Cetak::Teks(kv.clone()));
        self.start_block(usai);
        self.span_kini = lama;
        Ok(self.load_place(&hasil, &hasil_ty))
    }

    /// Kunci `p[k]` atas struct: field-nya harus pasti saat kompilasi, karena tipe tiap field berbeda.
    pub(crate) fn kunci_struct(&self, idx: &Expr, span: Span) -> Result<String, String> {
        match idx {
            Expr::Literal(LitKind::Str(t), _) | Expr::Literal(LitKind::RawStr(t), _) => Ok(t.clone()),
            Expr::Literal(LitKind::Int(v), _) => Ok(v.to_string()),
            _ => Err(super::galat::tandai(span,
                "indeks struct di backend LLVM harus teks literal (`p[\"x\"]`), atau tulis `p.x` — \
                 tipe tiap field bisa berbeda, jadi field yang dipilih saat jalan tidak bisa diberi tipe di sini")),
        }
    }

    /// Wadah yang bisa ditulis lewat `[...]`; sisanya gagal saat jalan dengan kalimat acuan.
    pub(crate) fn bisa_ditulis_index(t: &JrgType) -> bool {
        matches!(t, JrgType::Array(_, _) | JrgType::Slice(_) | JrgType::Dict(_) | JrgType::DictCampur(_)
            | JrgType::Tuple(_) | JrgType::Struct(_) | JrgType::Any)
    }

    /// Nama tipe seperti `typeof` acuan menyebutnya (`enum:Option`, `enum:E`).
    pub(crate) fn nama_acuan(&self, oty: &JrgType) -> String {
        match oty {
            JrgType::Enum(n) => format!("enum:{}", if self.option_args.contains_key(n) { "Option".to_string() }
                else if self.result_args.contains_key(n) { "Result".to_string() }
                else { super::modul::tampil(super::generik::dasar(n)).to_string() }),
            _ => oty.nama_typeof(),
        }
    }

    /// Tipe yang tidak punya field sama sekali: menulis `.f` ke sana gagal saat jalan.
    pub(crate) fn bukan_pemilik_field(t: &JrgType) -> bool {
        matches!(t, JrgType::Unit | JrgType::Bool | JrgType::Rune | JrgType::Str | JrgType::Array(_, _)
            | JrgType::Slice(_) | JrgType::Enum(_) | JrgType::Chan(_))
            || matches!(t.nama_typeof().as_str(), "int" | "float")
    }

    /// Nama handle pustaka (`Ptr`, `Future`, `Task`), kalau tipe ini memegangnya secara statis.
    pub(crate) fn nama_handle_pustaka(t: &JrgType) -> Option<&'static str> {
        match t {
            JrgType::Struct(n) => crate::interpreter::galat_index::HANDLE_PUSTAKA.iter().copied().find(|h| *h == n.as_str()),
            JrgType::Masa(_, false) => Some("Future"),
            JrgType::Masa(_, true) => Some("Task"),
            _ => None,
        }
    }

    /// Alamat sasaran penugasan: menulis ke field sebuah handle pustaka ditolak saat jalan.
    pub(crate) fn tempat_tulis(&mut self, t: &Expr) -> Result<(String, JrgType), String> {
        match t {
            Expr::Field { obj, field, span } => {
                if let Some(h) = self.tipe_jalur(obj).and_then(|ty| Self::nama_handle_pustaka(&ty)) {
                    self.tulis_antara = true;
                    let _ = self.get_place(obj)?;
                    return self.tempat_tiruan_sesudah(
                        &crate::interpreter::galat_index::tulis_handle(h, field), *span);
                }
            }
            Expr::Index { obj, idx, span } => {
                if let Some(h) = self.tipe_jalur(obj).and_then(|ty| Self::nama_handle_pustaka(&ty)) {
                    let k = self.kunci_struct(idx, *span)?;
                    self.tulis_antara = true;
                    let _ = self.get_place(obj)?;
                    return self.tempat_tiruan_sesudah(
                        &crate::interpreter::galat_index::tulis_handle(h, &k), *span);
                }
            }
            _ => {}
        }
        self.get_place(t)
    }

    pub(crate) fn get_field_place(&mut self, optr: &str, oty: &JrgType, field: &str, span: Span) -> Result<(String, JrgType), String> {
        match oty {
            JrgType::Struct(name) => {
                let layout = self.types.structs.get(name).cloned()
                    .ok_or_else(|| format!("struct `{}` tidak dikenal", name))?;
                // Namanya METHOD, bukan field: kalimatnya satu definisi dengan
                // dua mesin acuan, karena `P` MEMANG punya `dua` — cuma bukan field.
                if layout.field_index(field).is_none()
                    && self.tables.find_method(name, field).is_some() {
                    return Err(crate::llvm::galat::tandai(span,
                        crate::interpreter::galat_index::metode_tanpa_kurung(name, field)));
                }
                let Some(idx) = layout.field_index(field) else {
                    let nama: Vec<&str> = layout.fields.iter().map(|(f, _)| f.as_str()).collect();
                    let saran = crate::diag::saran_nama_terdekat(field, nama.into_iter());
                    return Err(crate::llvm::galat::tandai(span, crate::interpreter::galat_index::field_tak_dideklarasikan(
                        super::modul::tampil(super::generik::dasar(name)), field, saran.as_deref())));
                };
                // Satu pintu untuk BACA dan TULIS: `codegen_field` dan jalur target
                // penugasan dua-duanya lewat sini, jadi penegakan visibilitas tidak bisa …
                crate::vm::visibilitas::periksa(super::generik::dasar(name), field, self.cur_self_ty.as_deref())
                    .map_err(|e| crate::llvm::galat::tandai(span, e))?;
                let fty = layout.fields[idx].1.clone();
                let r = self.fresh("fld.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", r, oty.llvm_ty(), optr, idx));
                if let JrgType::Kotak(t) = fty { return Ok((self.isi_kotak(&r), *t)); }
                Ok((r, fty))
            }
            JrgType::Tuple(ts) => {
                let idx: usize = field.parse().map_err(|_| format!("field tuple `.{}` tidak valid", field))?;
                if idx >= ts.len() { return Err(format!("[{}:{}] index tuple `.{}` di luar jangkauan", span.lo.0, span.hi.0, idx)); }
                let fty = ts[idx].clone();
                let r = self.fresh("tfld.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", r, oty.llvm_ty(), optr, idx));
                Ok((r, fty))
            }
            JrgType::Slice(_) if field == "len" => {
                let r = self.fresh("slenf.");
                self.emit(format!("{} = getelementptr %JrgSlice, ptr {}, i32 0, i32 1", r, optr));
                Ok((r, JrgType::I64))
            }
            JrgType::Str if field == "len" => {
                let r = self.fresh("strlenf.");
                self.emit(format!("{} = getelementptr %JrgStr, ptr {}, i32 0, i32 1", r, optr));
                Ok((r, JrgType::I64))
            }
            JrgType::Sinkron(k) if crate::sinkron::metode_ada(if *k { "mutex" } else { "atomic" }, field) =>
                Err(crate::llvm::galat::tandai(span,
                    crate::interpreter::galat_index::metode_tanpa_kurung(&oty.nama_typeof(), field))),
            // Menulis `.f` ke yang bukan struct/dict: gagal SAAT JALAN dengan kalimat acuan; tempatnya tiruan.
            t if Self::bukan_pemilik_field(t) => self.tempat_tiruan_sesudah(
                &crate::interpreter::galat_index::bukan_pemilik_field(&oty.nama_typeof(), field), span),
            _ => Err(format!("[{}:{}] tidak bisa akses field `.{}` pada tipe `{}`", span.lo.0, span.hi.0, field, oty.display_name())),
        }
    }

    /// Nilai indeks yang bisa dipastikan saat KOMPILASI, kalau ada.
    pub(crate) fn indeks_konstan(e: &Expr) -> Option<i64> {
        match e {
            Expr::Literal(LitKind::Int(v), _) => i64::try_from(*v).ok(),
            Expr::Unary { op: UnaryOp::Neg, expr, .. } => Self::indeks_konstan(expr).map(|v| -v),
            _ => None,
        }
    }

    /// Alamat sebuah elemen tuple.
    pub(crate) fn tempat_elemen_tuple(&mut self, optr: &str, oty: &JrgType, idx: &Expr, span: Span) -> Result<(String, JrgType), String> {
        let JrgType::Tuple(elems) = oty else {
            return Err(super::galat::tandai(span, "bukan tuple"));
        };
        let n = elems.len() as i64;

        let Some(raw) = Self::indeks_konstan(idx) else {
            // Elemen setipe: tata letaknya sama dengan `[n x T]`, jadi tempatnya dihitung saat jalan.
            if let Some(t) = elems.first().filter(|t| elems.iter().all(|e| e == *t)).cloned() {
                let iv = self.indeks_bulat(idx, "tuple", span)?;
                let ix = self.emit_bounds_check_const(&iv, n, ("tuple", "elemen"));
                let r = self.fresh("tidx.");
                self.emit(format!("{} = getelementptr [{} x {}], ptr {}, i64 0, i64 {}", r, n, t.llvm_ty(), optr, ix.text));
                return Ok((r, t));
            }
            return Err(super::galat::tandai(span, format!(
                "index tuple harus berupa angka yang sudah pasti saat kompilasi\n\
                 bantuan: pakai angka langsung, mis. `t[0]`\n\
                 catatan: elemen tuple boleh bertipe berbeda-beda (`{}`), jadi tipe \
                 hasil `t[i]` tidak bisa ditentukan kalau `i` baru diketahui saat \
                 program jalan — interpreter bisa karena nilainya bertag, backend ini tidak",
                oty.display_name()
            )));
        };

        let i = if raw < 0 { raw + n } else { raw };
        if i < 0 || i >= n {
            let pesan = crate::interpreter::Interpreter::oob_msg("tuple", "elemen", raw as i128, n as usize);
            return self.tempat_tiruan_sesudah(&pesan, span);
        }

        let r = self.fresh("tidx.");
        self.emit(format!(
            "{} = getelementptr {}, ptr {}, i32 0, i32 {}",
            r, oty.llvm_ty(), optr, i
        ));
        Ok((r, elems[i as usize].clone()))
    }

    /// `t[i]` atas tuple berelemen beda tipe dengan `i` dari variabel: dipilih saat jalan, hasilnya
    /// nilai bertag — dulu ditolak saat kompilasi. Menulis lewat bentuk ini tetap ditolak.
    fn baca_tuple_campur(&mut self, ptr: &str, oty: &JrgType, idx: &Expr, span: Span) -> Result<Val, String> {
        let JrgType::Tuple(es) = oty else { unreachable!() };
        let es = es.clone();
        let iv = self.indeks_bulat(idx, "tuple", span)?;
        let ix = self.emit_bounds_check_const(&iv, es.len() as i64, ("tuple", "elemen"));
        let hasil = self.alloca_typed("tcm.", &JrgType::Any);
        let usai = self.fresh_label("tcm.usai.");
        let label: Vec<String> = es.iter().map(|_| self.fresh_label("tcm.e.")).collect();
        let kasus: Vec<String> = label.iter().enumerate().map(|(i, l)| format!("i64 {}, label %{}", i, l)).collect();
        self.terminate(format!("switch i64 {}, label %{} [ {} ]", ix.text, usai, kasus.join(" ")));
        for (i, (t, l)) in es.iter().zip(&label).enumerate() {
            self.start_block(l.clone());
            let ep = self.fresh("tcm.p.");
            self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", ep, oty.llvm_ty(), ptr, i));
            let v = self.load_place(&ep, t);
            let k = self.kotak_any(&v, span)?;
            self.emit(format!("store %JrgAny {}, ptr {}", k.text, hasil));
            self.terminate(format!("br label %{}", usai));
        }
        self.start_block(usai);
        Ok(self.load_place(&hasil, &JrgType::Any))
    }

    pub(crate) fn get_index_place(&mut self, optr: &str, oty: &JrgType, iv: &Val, span: Span) -> Result<(String, JrgType), String> {
        match oty {
            JrgType::Array(elem, n) => {
                let ix = self.emit_bounds_check_const(iv, *n as i64, ("array", "elemen"));
                let r = self.fresh("aidx.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i64 0, i64 {}", r, oty.llvm_ty(), optr, ix.text));
                Ok((r, (**elem).clone()))
            }
            JrgType::Slice(elem) => {
                let pfield = self.fresh("splc.");
                self.emit(format!("{} = getelementptr %JrgSlice, ptr {}, i32 0, i32 0", pfield, optr));
                let dataptr = self.fresh("sdata.");
                self.emit(format!("{} = load ptr, ptr {}", dataptr, pfield));
                let lfield = self.fresh("sllf.");
                self.emit(format!("{} = getelementptr %JrgSlice, ptr {}, i32 0, i32 1", lfield, optr));
                let lenv = self.fresh("slen2.");
                self.emit(format!("{} = load i64, ptr {}", lenv, lfield));
                let ix = self.emit_bounds_check_dyn(iv, &lenv, ("array", "elemen"));
                let r = self.fresh("sidx.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", r, elem.llvm_ty(), dataptr, ix.text));
                Ok((r, (**elem).clone()))
            }
            _ => Err(format!("[{}:{}] tidak bisa indexing pada tipe `{}`", span.lo.0, span.hi.0, oty.display_name())),
            // Str ditangani di `codegen_index`, bukan di sini: hasilnya sebuah
            // `rune` yang dihitung, bukan sebuah ALAMAT — jadi ia tidak punya
            // "tempat", dan itu juga alasan `s[0] = 'x'` tidak sah.
        }
    }

    /// Interning dua kata yang dipakai pesan batas: jenis koleksinya dan
    /// satuan isinya. Dikembalikan sebagai (ptr, len, ptr, len).
    pub(crate) fn teks_jenis(&mut self, jenis: (&str, &str)) -> (String, String, String, String) {
        let a = self.intern_string(jenis.0);
        let (ap, al) = self.split_str(&a);
        let b = self.intern_string(jenis.1);
        let (bp, bl) = self.split_str(&b);
        (ap, al, bp, bl)
    }

    pub(crate) fn emit_bounds_check_const(&mut self, iv: &Val, len_const: i64, jenis: (&str, &str)) -> Val {
        let lenv = Val { text: len_const.to_string(), ty: JrgType::I64 };
        self.emit_index_norm_check(iv, &lenv, jenis)
    }

    pub(crate) fn emit_bounds_check_dyn(&mut self, iv: &Val, len_reg: &str, jenis: (&str, &str)) -> Val {
        let lenv = Val { text: len_reg.to_string(), ty: JrgType::I64 };
        self.emit_index_norm_check(iv, &lenv, jenis)
    }

    /// Normalkan indeks negatif, lalu periksa batasnya.
    pub(crate) fn emit_index_norm_check(&mut self, iv: &Val, lenv: &Val, jenis: (&str, &str)) -> Val {
        // ── jalur konstan: dihitung saat KOMPILASI ────────────────────────
        let idx_konst = iv.text.parse::<i64>().ok();
        let len_konst = lenv.text.parse::<i64>().ok();

        if let (Some(i), Some(n)) = (idx_konst, len_konst) {
            let ternorm = if i < 0 { i + n } else { i };
            if ternorm >= 0 && ternorm < n {
                return Val { text: ternorm.to_string(), ty: JrgType::I64 };
            }
            // Di luar batas dan sudah pasti: panik tanpa cabang.
            let (jp, jl, sp, sl) = self.teks_jenis(jenis);
            self.ensure_extern(
                "declare void @jrg_panic_index_oob_jenis(i64, i64, ptr, i64, ptr, i64) cold nounwind noreturn",
                "jrg_panic_index_oob_jenis",
            );
            self.emit(format!(
                "call void @jrg_panic_index_oob_jenis(i64 {}, i64 {}, ptr {}, i64 {}, ptr {}, i64 {})",
                i, n, jp, jl, sp, sl));
            self.terminate("unreachable".to_string());
            let lanjut = self.fresh_label("oobkonst.");
            self.start_block(lanjut);
            return Val { text: "0".to_string(), ty: JrgType::I64 };
        }

        // Indeks konstan TAK NEGATIF atas panjang dinamis (slice): tidak
        // perlu normalisasi, cukup batas atasnya.
        let sudah_positif = matches!(idx_konst, Some(i) if i >= 0);

        let ix_norm = if sudah_positif {
            iv.text.clone()
        } else {
            let neg = self.fresh("ixneg.");
            self.emit(format!("{} = icmp slt i64 {}, 0", neg, iv.text));
            let plus = self.fresh("ixadd.");
            self.emit(format!("{} = add i64 {}, {}", plus, iv.text, lenv.text));
            let norm = self.fresh("ixn.");
            self.emit(format!("{} = select i1 {}, i64 {}, i64 {}", norm, neg, plus, iv.text));
            norm
        };

        // SATU perbandingan untuk dua syarat sekaligus.
        let bad = self.fresh("bcoob.");
        self.emit(format!("{} = icmp uge i64 {}, {}", bad, ix_norm, lenv.text));
        let badlbl = self.fresh_label("oob.");
        let oklbl = self.fresh_label("idxok.");
        self.terminate_unlikely_true(&bad, &badlbl, &oklbl);
        self.start_block(badlbl);
        let (jp, jl, sp, sl) = self.teks_jenis(jenis);
        self.ensure_extern(
            "declare void @jrg_panic_index_oob_jenis(i64, i64, ptr, i64, ptr, i64) cold nounwind noreturn",
            "jrg_panic_index_oob_jenis",
        );
        self.emit(format!(
            "call void @jrg_panic_index_oob_jenis(i64 {}, i64 {}, ptr {}, i64 {}, ptr {}, i64 {})",
            iv.text, lenv.text, jp, jl, sp, sl));
        self.terminate("unreachable".to_string());
        self.start_block(oklbl);
        return Val { text: ix_norm, ty: JrgType::I64 };
    }

    pub(crate) fn emit_slice_bounds_check(&mut self, lo: &Val, hi: &Val, len_reg: &str) -> (Val, Val) {
        let norm = |cg: &mut Self, v: &Val, tag: &str| -> Val {
            let neg = cg.fresh(&format!("sn{}.", tag));
            cg.emit(format!("{} = icmp slt i64 {}, 0", neg, v.text));
            let plus = cg.fresh(&format!("sa{}.", tag));
            cg.emit(format!("{} = add i64 {}, {}", plus, v.text, len_reg));
            let out = cg.fresh(&format!("sv{}.", tag));
            cg.emit(format!("{} = select i1 {}, i64 {}, i64 {}", out, neg, plus, v.text));
            Val { text: out, ty: JrgType::I64 }
        };
        let lo_n = norm(self, lo, "lo");
        let hi_n = norm(self, hi, "hi");

        let lo_neg = self.fresh("sblo.");
        self.emit(format!("{} = icmp slt i64 {}, 0", lo_neg, lo_n.text));
        let hi_neg = self.fresh("sbhi.");
        self.emit(format!("{} = icmp slt i64 {}, 0", hi_neg, hi_n.text));
        let reversed = self.fresh("sbrev.");
        self.emit(format!("{} = icmp sgt i64 {}, {}", reversed, lo_n.text, hi_n.text));
        let too_big = self.fresh("sbbig.");
        self.emit(format!("{} = icmp sgt i64 {}, {}", too_big, hi_n.text, len_reg));
        let invalid_start = self.fresh("sbor.");
        self.emit(format!("{} = or i1 {}, {}", invalid_start, lo_neg, hi_neg));
        let invalid_range = self.fresh("sbor.");
        self.emit(format!("{} = or i1 {}, {}", invalid_range, reversed, too_big));
        let invalid = self.fresh("sbor.");
        self.emit(format!("{} = or i1 {}, {}", invalid, invalid_start, invalid_range));

        let bad = self.fresh_label("sliceoob.");
        let ok = self.fresh_label("sliceok.");
        self.terminate_unlikely_true(&invalid, &bad, &ok);
        self.start_block(bad);
        self.ensure_extern(
            "declare void @jrg_panic_slice_oob(i64, i64, i64) cold nounwind noreturn",
            "jrg_panic_slice_oob",
        );
        self.emit(format!(
            "call void @jrg_panic_slice_oob(i64 {}, i64 {}, i64 {})",
            lo.text, hi.text, len_reg
        ));
        self.terminate("unreachable".to_string());
        self.start_block(ok);
        (lo_n, hi_n)
    }

    // ── rvalue field/index/slice ─────────────────────────────────────────

    pub fn codegen_field(&mut self, obj: &Expr, field: &str, span: Span) -> Result<Val, String> {
        // `d.a` pada sebuah dict berarti `d["a"]`, dan keduanya memang satu hal yang
        // sama — diukur di interpreter dan VM, yang menjawab sama untuk keduanya.
        if matches!(self.peek_expr_type(obj),
                    Some(JrgType::Dict(_)) | Some(JrgType::DictCampur(_)))
        {
            self.catat_pesan_field_dict(field, span);
            let kunci = Expr::Literal(crate::parser::LitKind::Str(field.to_string()), span);
            return self.codegen_index(obj, &kunci, span);
        }
        // `d.a.b` atas nilai bertag dari jembatan: `d.a` tidak punya alamat, jadi dibaca sebagai NILAI.
        let (ptr, oty) = if self.basis_bertag(obj) {
            let v = self.codegen_expr(obj, None)?;
            (self.spill_to_place(&v), v.ty)
        } else { self.get_place(obj)? };
        if oty == JrgType::Any {
            let v = self.load_place(&ptr, &oty);
            return self.field_titipan(&v, field, span);
        }
        // Field yang tidak ada, di jalur BACA: kalimat method/field acuan (jalur tulis punya kalimatnya sendiri).
        if let JrgType::Struct(n) = &oty {
            let tak_ada = self.types.structs.get(n).is_some_and(|l| l.field_index(field).is_none());
            if tak_ada && self.tables.find_method(n, field).is_none() {
                if let Some(p) = self.pesan_tak_punya(&oty, field) { return Err(crate::llvm::galat::tandai(span, p)); }
            }
        }
        // Dict yang tipenya baru terbaca sesudah diturunkan (`jawa::stats().alive_total`): sama, `d["a"]`.
        if matches!(oty, JrgType::Dict(_) | JrgType::DictCampur(_)) {
            self.catat_pesan_field_dict(field, span);
            let o = self.lokal_sementara(ptr, oty, span);
            let kunci = Expr::Literal(crate::parser::LitKind::Str(field.to_string()), span);
            return self.codegen_index(&o, &kunci, span);
        }
        if let Some(r) = self.field_bukan_struct(&ptr, &oty, field, span) { return r; }
        let (fptr, fty) = self.get_field_place(&ptr, &oty, field, span)?;
        Ok(self.load_place(&fptr, &fty))
    }

    /// `x.f` atau `x[k]` yang bertipe nilai bertag: tidak punya alamat, cuma nilai (jalur BACA saja).
    fn basis_bertag(&mut self, obj: &Expr) -> bool {
        match obj {
            Expr::Field { obj: o, .. } | Expr::Index { obj: o, .. } => {
                // `f(..)?` atas builtin jembatan menjawab nilai bertag; penduga tipenya tidak melihat ke sana.
                let jembatan = matches!(o.as_ref(), Expr::Propagate { expr, .. }
                    if matches!(expr.as_ref(), Expr::Call { func, .. }
                        if matches!(func.as_ref(), Expr::PathExpr { segments, .. }
                            if segments.iter().rev().nth(1).is_some_and(|ns| super::codegen_jembatan::ns_jembatan(ns)))));
                jembatan || self.peek_expr_type(o) == Some(JrgType::Any) || self.basis_bertag(o)
            }
            _ => false,
        }
    }

    /// `x.nama` dibaca atas nilai yang bukan struct, seperti `eval_field` acuan: `len`/`is_empty`
    /// teks dan array dijawab, method tanpa `()` ditolak saat jalan, sisanya lewat pintu method.
    fn field_bukan_struct(&mut self, ptr: &str, oty: &JrgType, field: &str, span: Span) -> Option<Result<Val, String>> {
        use crate::interpreter::galat_index as gi;
        let tname = match oty {
            JrgType::Struct(_) | JrgType::Sinkron(_) | JrgType::Any => return None,
            JrgType::Tuple(ts) => {
                let pesan = match field.parse::<i128>() {
                    Ok(ix) if ix >= 0 && (ix as usize) < ts.len() => return None,
                    Ok(ix) => crate::interpreter::Interpreter::oob_msg("tuple", "elemen", ix, ts.len()),
                    Err(_) => format!("`.{}` bukan index tuple yang sah — setelah titik harus angka bulat, \
                                       mis. `.0` untuk elemen pertama", field),
                };
                return Some(self.gagal_sesudah_argumen(&pesan, &[]));
            }
            _ => self.nama_acuan(oty),
        };
        let o = self.lokal_sementara(ptr.to_string(), oty.clone(), span);
        if gi::pseudo_field(&tname, field) {
            return Some(self.codegen_method(&o, field, &[], None, span));
        }
        let tipe = tname.rsplit(':').next().unwrap_or(&tname).to_string();
        let pemakai = matches!(oty, JrgType::Enum(n) if self.tables.find_method(n, field).is_some());
        let bawaan = gi::ns_metode_bawaan(&tname).is_some_and(|ns| crate::vm::ada_builtin(&format!("{}::{}", ns, field)));
        if pemakai || bawaan {
            return Some(self.gagal_sesudah_argumen(&gi::metode_tanpa_kurung(&tipe, field), &[]));
        }
        Some(self.codegen_method(&o, field, &[], None, span))
    }

    /// Kalimat acuan `d.k` atas dict (`Interpreter` metode.rs): saran dari method `dict::`, kuncinya diisi runtime.
    fn catat_pesan_field_dict(&mut self, field: &str, span: Span) {
        let calon = crate::interpreter::calon_method("dict");
        let saran = crate::diag::saran_nama_terdekat(field, calon.iter().map(|s| s.as_str()));
        let utuh = crate::interpreter::pesan_method_tak_ada("dict", field, "\u{0}", saran.as_deref());
        let (a, b) = utuh.split_once('\u{0}').expect("penanda kunci");
        self.pesan_field_dict.insert((span.lo.0, span.hi.0), (a.to_string(), b.to_string()));
    }

    /// Tipe yang dicatat untuk `d["kunci"]` ketika `d` sebuah dict bernilai
    /// campur dan kuncinya TERTULIS.
    pub(crate) fn peek_tipe_dict_campur(&mut self, e: &Expr) -> Option<JrgType> {
        let Expr::Index { obj, idx, .. } = e else { return None };
        let kunci = match &**idx {
            Expr::Literal(crate::parser::LitKind::Str(t), _)
            | Expr::Literal(crate::parser::LitKind::RawStr(t), _) => t.clone(),
            _ => return None,
        };
        let (_, oty) = self.get_place(obj).ok()?;
        let JrgType::DictCampur(tabel) = oty else { return None };
        tabel.iter().find(|(n, _)| *n == kunci).map(|(_, t)| t.clone())
    }

    /// `o.f` dibaca sebagai nilai: `o` bertag (tanpa alamat field), atau bukan tempat sama sekali.
    pub(crate) fn field_dibaca_nilai(&mut self, o: &Expr) -> bool {
        !matches!(o, Expr::Ident(..) | Expr::Field { .. } | Expr::Index { .. } | Expr::Unary { .. })
            || self.peek_expr_type(o) == Some(JrgType::Any)
    }

    pub fn codegen_index(&mut self, obj: &Expr, idx: &Expr, span: Span) -> Result<Val, String> {
        // String tidak lewat `get_place`: nilainya `%JrgStr` (ptr+len), dan `s[i]`
        // menghasilkan sebuah `rune` yang DIHITUNG, bukan alamat yang dibaca.
        let obj_ty = self.peek_expr_type(obj);
        if matches!(obj_ty, Some(JrgType::Str)) {
            let sv = self.codegen_expr(obj, Some(&JrgType::Str))?;
            let iv = self.indeks_bulat(idx, "string", span)?;
            return self.karakter_string(&sv, &iv);
        }

        // `req.params["id"]` atas nilai bertag: field-nya dibaca sebagai NILAI (titipan), karena
        // nilai bertag tidak punya alamat field — ini jalur baca, jadi tidak ada tulisan yang hilang.
        let field_bertag = matches!(obj, Expr::Field { obj: o, .. } if self.field_dibaca_nilai(o))
            || self.basis_bertag(obj);
        let (ptr, oty) = if field_bertag {
            let v = self.codegen_expr(obj, None)?;
            (self.spill_to_place(&v), v.ty)
        } else { self.get_place(obj)? };
        // Dict: kuncinya string, dan hasilnya nilai — bukan alamat, karena
        // pasangannya dicari saat jalan.
        if let JrgType::Dict(_) = &oty {
            let d = self.load_place(&ptr, &oty);
            let k = self.kunci_dict(idx, span)?;
            return self.dict_baca(&d, &k, span);
        }
        // Dict bernilai campur tipe.
        if oty == JrgType::Any {
            let kotak = self.load_place(&ptr, &oty);
            // Tanpa tipe yang diingat: kunci teks berarti dict bertag, selain itu array bertag —
            // dan isi yang bukan itu gagal saat jalan.
            let kunci_teks = matches!(idx, Expr::Literal(crate::parser::LitKind::Str(_), _)
                | Expr::Literal(crate::parser::LitKind::RawStr(_), _))
                || self.peek_expr_type(idx) == Some(JrgType::Str);
            let dalam = self.peek_tipe_dict_campur(obj).unwrap_or_else(|| {
                if kunci_teks { JrgType::Dict(Box::new(JrgType::Any)) }
                else { JrgType::Slice(Box::new(JrgType::Any)) }
            });
            // Dict tanpa tipe yang diingat: isinya bisa juga titipan milik `jrgc` (`req.params`).
            if dalam == JrgType::Dict(Box::new(JrgType::Any)) && kunci_teks {
                let k = self.kunci_dict(idx, span)?;
                let w = self.fresh("ixt.");
                self.emit(format!("{} = extractvalue %JrgAny {}, 0", w, kotak.text));
                // Tag 9: titipan milik `jrgc` (`jembatan/src/lib.rs`, `TAG_TITIPAN`).
                let w8 = self.fresh("ixt.");
                self.emit(format!("{} = and i32 {}, 255", w8, w));
                let c = self.fresh("ixt.");
                self.emit(format!("{} = icmp eq i32 {}, 9", c, w8));
                let (lt, ld, lz) = (self.fresh_label("ixt.titip."), self.fresh_label("ixt.dict."), self.fresh_label("ixt.akhir."));
                self.terminate(format!("br i1 {}, label %{}, label %{}", c, lt, ld));
                self.start_block(lt.clone());
                let a = self.indeks_titipan(&kotak, &k, span)?;
                let lt_akhir = self.cur_label.clone();
                self.terminate(format!("br label %{}", lz));
                self.start_block(ld);
                let nilai = self.buka_any(&kotak, &dalam, span)?;
                let tempat = self.spill_to_place(&nilai);
                let d = self.load_place(&tempat, &dalam);
                let b = self.dict_baca(&d, &k, span)?;
                let b = self.coerce(&b, &JrgType::Any);
                let ld_akhir = self.cur_label.clone();
                self.terminate(format!("br label %{}", lz));
                self.start_block(lz);
                let r = self.fresh("ixt.");
                self.emit(format!("{} = phi %JrgAny [ {}, %{} ], [ {}, %{} ]", r, a.text, lt_akhir, b.text, ld_akhir));
                return Ok(Val { text: r, ty: JrgType::Any });
            }
            let nilai = self.buka_any(&kotak, &dalam, span)?;
            let tempat = self.spill_to_place(&nilai);
            if let JrgType::Dict(_) = &dalam {
                let d = self.load_place(&tempat, &dalam);
                let k = self.kunci_dict(idx, span)?;
                return self.dict_baca(&d, &k, span);
            }
            let iv = self.codegen_expr(idx, Some(&JrgType::I64))?;
            let iv = self.coerce_longgar(&iv, &JrgType::I64);
            let (tp, tt) = self.get_index_place(&tempat, &dalam, &iv, span)?;
            return Ok(self.load_place(&tp, &tt));
        }
        if let JrgType::DictCampur(tabel) = &oty {
            let tabel = tabel.clone();
            let d = self.load_place(&ptr, &oty);
            let d = Val { text: d.text, ty: JrgType::Dict(Box::new(JrgType::Any)) };
            let k = self.kunci_dict(idx, span)?;
            let hasil = self.dict_baca(&d, &k, span)?;
            let tertulis = match idx {
                Expr::Literal(crate::parser::LitKind::Str(t), _)
                | Expr::Literal(crate::parser::LitKind::RawStr(t), _) => Some(t.clone()),
                _ => None,
            };
            return match tertulis.and_then(|t| tabel.iter().find(|(n, _)| *n == t).cloned()) {
                Some((_, ty)) => self.buka_any(&hasil, &ty, span),
                None => Ok(hasil),
            };
        }
        if let JrgType::Str = &oty {
            let sv = self.load_place(&ptr, &oty);
            let iv = self.indeks_bulat(idx, "string", span)?;
            return self.karakter_string(&sv, &iv);
        }
        if let JrgType::Tuple(es) = &oty {
            let campur = es.iter().any(|e| *e != es[0]);
            if campur && Self::indeks_konstan(idx).is_none() {
                return self.baca_tuple_campur(&ptr, &oty, idx, span);
            }
            let (eptr, ety) = self.tempat_elemen_tuple(&ptr, &oty, idx, span)?;
            return Ok(self.load_place(&eptr, &ety));
        }
        if let JrgType::Struct(n) = &oty {
            let k = match self.kunci_struct(idx, span) {
                Ok(k) => k,
                Err(_) if self.peek_expr_type(idx) == Some(JrgType::Str) => return self.baca_struct_kunci(&ptr, &oty, idx, span),
                Err(g) => return Err(g),
            };
            let layout = self.types.structs.get(n).cloned().ok_or_else(|| format!("struct `{}` tidak dikenal", n))?;
            if layout.field_index(&k).is_none() {
                let ada: Vec<String> = layout.fields.iter().map(|(f, _)| f.clone()).collect();
                let pesan = crate::interpreter::galat_index::field_hilang(
                    super::modul::tampil(super::generik::dasar(n)), &k, &ada);
                self.tempat_tiruan_sesudah(&pesan, span)?;
                return Ok(Val::unit());
            }
            let (fptr, fty) = self.get_field_place(&ptr, &oty, &k, span)?;
            return Ok(self.load_place(&fptr, &fty));
        }
        if !matches!(oty, JrgType::Array(_, _) | JrgType::Slice(_)) {
            self.codegen_expr(idx, None)?;
            let pesan = crate::interpreter::galat_index::bukan_wadah(&self.nama_acuan(&oty));
            self.tempat_tiruan_sesudah(&pesan, span)?;
            return Ok(Val::unit());
        }
        let iv = self.indeks_bulat(idx, &oty.nama_typeof(), span)?;
        let (eptr, ety) = self.get_index_place(&ptr, &oty, &iv, span)?;
        Ok(self.load_place(&eptr, &ety))
    }

    /// Jumlah karakter teks `(sptr, slen)`: memo runtime dibaca langsung di IR, runtime dipanggil cuma kalau
    /// memonya teks lain. Teks ASCII ⇔ jumlah karakter = panjang bita.
    pub(crate) fn jumlah_karakter(&mut self, sptr: &str, slen: &str) -> String {
        self.ensure_extern("declare i64 @jrg_str_char_count(ptr, i64)", "jrg_str_char_count");
        self.ensure_extern("@jrg_teks_memo_p = external global ptr\n@jrg_teks_memo_l = external global i64\n\
            @jrg_teks_memo_n = external global i64", "global.teks_memo");
        let (mp, ml, mn) = (self.fresh("tmp."), self.fresh("tml."), self.fresh("tmn."));
        self.emit(format!("{} = load ptr, ptr @jrg_teks_memo_p", mp));
        self.emit(format!("{} = load i64, ptr @jrg_teks_memo_l", ml));
        self.emit(format!("{} = load i64, ptr @jrg_teks_memo_n", mn));
        let (c1, c2, sama) = (self.fresh("tmc."), self.fresh("tmc."), self.fresh("tmc."));
        self.emit(format!("{} = icmp eq ptr {}, {}", c1, mp, sptr));
        self.emit(format!("{} = icmp eq i64 {}, {}", c2, ml, slen));
        self.emit(format!("{} = and i1 {}, {}", sama, c1, c2));
        let (kena, meleset, usai) = (self.fresh_label("tm.kena."), self.fresh_label("tm.meleset."), self.fresh_label("tm.usai."));
        self.terminate(format!("br i1 {}, label %{}, label %{}", sama, kena, meleset));
        self.start_block(kena.clone());
        self.terminate(format!("br label %{}", usai));
        self.start_block(meleset.clone());
        let n1 = self.fresh("tmh.");
        self.emit(format!("{} = call i64 @jrg_str_char_count(ptr {}, i64 {})", n1, sptr, slen));
        self.terminate(format!("br label %{}", usai));
        self.start_block(usai);
        let n = self.fresh("tmjk.");
        self.emit(format!("{} = phi i64 [ {}, %{} ], [ {}, %{} ]", n, mn, kena, n1, meleset));
        n
    }

    /// `s[i]` — karakter ke-`i` sebuah string, sebagai `rune`.
    pub(crate) fn karakter_string(&mut self, sv: &Val, iv: &Val) -> Result<Val, String> {
        self.ensure_extern("declare i32 @jrg_str_char_at(ptr, i64, i64)", "jrg_str_char_at");

        let (sptr, slen) = self.split_str(sv);
        // Panjang dalam KARAKTER, bukan byte — itu satuan yang dipakai
        // indeksnya, jadi pemeriksaan batas harus memakai satuan yang sama.
        let n = self.jumlah_karakter(&sptr, &slen);
        let ix = self.emit_bounds_check_dyn(iv, &n, ("string", "karakter"));

        // ASCII: karakternya bita ke-`i`, dibaca langsung; selain itu lewat runtime.
        let ascii = self.fresh("sasc.");
        self.emit(format!("{} = icmp eq i64 {}, {}", ascii, n, slen));
        let (baca, panggil, usai) = (self.fresh_label("schr.baca."), self.fresh_label("schr.panggil."), self.fresh_label("schr.usai."));
        self.terminate(format!("br i1 {}, label %{}, label %{}", ascii, baca, panggil));
        self.start_block(baca.clone());
        let (g, b, c1) = (self.fresh("schg."), self.fresh("schb."), self.fresh("schz."));
        self.emit(format!("{} = getelementptr i8, ptr {}, i64 {}", g, sptr, ix.text));
        self.emit(format!("{} = load i8, ptr {}", b, g));
        self.emit(format!("{} = zext i8 {} to i32", c1, b));
        self.terminate(format!("br label %{}", usai));
        self.start_block(panggil.clone());
        let c2 = self.fresh("schr.");
        self.emit(format!(
            "{} = call i32 @jrg_str_char_at(ptr {}, i64 {}, i64 {})",
            c2, sptr, slen, ix.text
        ));
        self.terminate(format!("br label %{}", usai));
        self.start_block(usai);
        let c = self.fresh("schp.");
        self.emit(format!("{} = phi i32 [ {}, %{} ], [ {}, %{} ]", c, c1, baca, c2, panggil));
        Ok(Val { text: c, ty: JrgType::Rune })
    }

    pub fn codegen_slice(&mut self, obj: &Expr, lo: &Option<Box<Expr>>, hi: &Option<Box<Expr>>, span: Span) -> Result<Val, String> {
        let (ptr, oty) = self.get_place(obj)?;
        // Kedua batas dievaluasi dulu, baru wadah dan jenisnya diperiksa — urutan acuan.
        let batas = |cg: &mut Self, e: &Option<Box<Expr>>| -> Result<Option<Val>, String> {
            e.as_ref().map(|e| cg.codegen_expr(e, Some(&JrgType::I64))).transpose()
        };
        let lo_e = batas(self, lo)?;
        let hi_e = batas(self, hi)?;
        if oty == JrgType::Any {
            let v = self.load_place(&ptr, &oty);
            return self.potong_bertag(&v, lo_e, hi_e, span);
        }
        if let JrgType::Tuple(_) = &oty {
            return self.potong_tuple(&ptr, &oty, (lo, lo_e), (hi, hi_e), span);
        }
        let (elem_ty, base_ptr, base_len) = match &oty {
            JrgType::Array(elem, n) => {
                let bp = self.fresh("slb.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i64 0, i64 0", bp, oty.llvm_ty(), ptr));
                ((**elem).clone(), bp, Val { text: n.to_string(), ty: JrgType::I64 })
            }
            JrgType::Slice(elem) => {
                let pfield = self.fresh("slsp.");
                self.emit(format!("{} = getelementptr %JrgSlice, ptr {}, i32 0, i32 0", pfield, ptr));
                let dataptr = self.fresh("slsd.");
                self.emit(format!("{} = load ptr, ptr {}", dataptr, pfield));
                let lfield = self.fresh("slsl.");
                self.emit(format!("{} = getelementptr %JrgSlice, ptr {}, i32 0, i32 1", lfield, ptr));
                let lenv = self.fresh("slslv.");
                self.emit(format!("{} = load i64, ptr {}", lenv, lfield));
                ((**elem).clone(), dataptr, Val { text: lenv, ty: JrgType::I64 })
            }
            // String: potongan per KARAKTER, hasilnya string — bukan slice
            // atas byte. Ditangani terpisah karena satuannya beda dari
            // semua tipe lain di sini.
            JrgType::Str => {
                let sv = self.load_place(&ptr, &oty);
                return self.potong_string_val(&sv, lo_e, hi_e, span);
            }
            _ => {
                let pesan = crate::interpreter::galat_index::bukan_bisa_dipotong(&self.nama_acuan(&oty));
                self.tempat_tiruan_sesudah(&pesan, span)?;
                return Ok(Val::unit());
            }
        };
        let Some((lo_v, hi_v)) = self.batas_potongan_dicek(lo_e, hi_e, &base_len, span)? else {
            return Ok(Val::unit());
        };
        let newlen = self.fresh("sll.");
        self.emit(format!("{} = sub i64 {}, {}", newlen, hi_v.text, lo_v.text));

        // ── slice MEMILIKI buffernya sendiri ─────────────────────────────
        let ukuran_elem = self.types.sizeof(&elem_ty);
        let align_elem = self.types.alignof(&elem_ty);
        self.ensure_extern("declare ptr @jrg_alloc(i64, i64)", "jrg_alloc");
        self.ensure_extern(
            "declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)",
            "llvm.memcpy.p0.p0.i64");

        let byte = self.fresh("slby.");
        self.emit(format!("{} = mul i64 {}, {}", byte, newlen, ukuran_elem));
        let newptr = self.fresh("slp.");
        self.emit(format!("{} = call ptr @jrg_alloc(i64 {}, i64 {})",
                          newptr, byte, align_elem));
        let asal = self.fresh("slsrc.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}",
                          asal, elem_ty.llvm_ty(), base_ptr, lo_v.text));
        self.emit(format!(
            "call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)",
            newptr, asal, byte));
        let s1 = self.fresh("slv.");
        self.emit(format!("{} = insertvalue %JrgSlice undef, ptr {}, 0", s1, newptr));
        let s2 = self.fresh("slv.");
        self.emit(format!("{} = insertvalue %JrgSlice {}, i64 {}, 1", s2, s1, newlen));
        Ok(Val { text: s2, ty: JrgType::Slice(Box::new(elem_ty)) })
    }

    /// Potongan atas nilai bertag, dipilih dari tagnya saat jalan — dulu ditolak saat kompilasi.
    /// Teks dipotong per karakter; isi yang tak bisa dipotong gagal dengan kalimat acuan.
    fn potong_bertag(&mut self, v: &Val, lo: Option<Val>, hi: Option<Val>, span: Span) -> Result<Val, String> {
        use super::codegen_any::{TAG_BURAM, TAG_SLICE, TAG_STR};
        let lama = self.span_kini.replace(span);
        let hasil = self.alloca_typed("ptgt.", &JrgType::Any);
        let tag = self.tag_any(v);
        let (lt, lb, lk, lz) = (self.fresh_label("ptgt.teks."), self.fresh_label("ptgt.lain."),
                                self.fresh_label("ptgt.koleksi."), self.fresh_label("ptgt.akhir."));
        let teks = self.fresh("ptgt.");
        self.emit(format!("{} = icmp eq i32 {}, {}", teks, tag.text, TAG_STR));
        self.terminate(format!("br i1 {}, label %{}, label %{}", teks, lt, lb));
        self.start_block(lt);
        let s = self.buka_any(v, &JrgType::Str, span)?;
        let r = self.potong_string_val(&s, lo, hi, span)?;
        let k = self.kotak_any(&r, span)?;
        self.emit(format!("store %JrgAny {}, ptr {}", k.text, hasil));
        self.terminate(format!("br label %{}", lz));
        self.start_block(lb);
        // Array dan tuple bertag bisa dipotong di acuan; tipe elemennya tidak diketahui di sini.
        let (a, b, c) = (self.fresh("ptgt."), self.fresh("ptgt."), self.fresh("ptgt."));
        self.emit(format!("{} = icmp eq i32 {}, {}", a, tag.text, TAG_SLICE));
        self.emit(format!("{} = icmp eq i32 {}, {}", b, tag.text, TAG_BURAM));
        self.emit(format!("{} = or i1 {}, {}", c, a, b));
        let (lg, ll) = (self.fresh_label("ptgt.gagal."), lk);
        self.terminate(format!("br i1 {}, label %{}, label %{}", c, ll, lg));
        self.start_block(ll);
        self.panik_tipe_any(v, |t| format!(
            "potongan atas `{}` di dalam nilai bertag belum didukung backend LLVM — beri anotasi tipe \
             di tempat nilainya diikat (`let a: []int = ..`)", t));
        self.start_block(lg);
        self.panik_tipe_any(v, |t| crate::interpreter::galat_index::bukan_bisa_dipotong(t));
        self.start_block(lz);
        self.span_kini = lama;
        Ok(self.load_place(&hasil, &JrgType::Any))
    }

    /// Batas potongan yang sudah dievaluasi, diperiksa urut acuan: jenis awal, jangkauan awal,
    /// jenis akhir, lalu sisanya. `None` = sudah gagal saat jalan (blok sesudahnya mati).
    fn batas_potongan_dicek(&mut self, lo: Option<Val>, hi: Option<Val>, panjang: &Val, span: Span)
        -> Result<Option<(Val, Val)>, String>
    {
        let salah = |v: &Option<Val>| v.as_ref().is_some_and(|v| !v.ty.is_int() && !matches!(v.ty, JrgType::Never | JrgType::Any));
        let nol = Val { text: "0".into(), ty: JrgType::I64 };
        if salah(&lo) || salah(&hi) {
            let sisi = if salah(&lo) { "awal" } else { "akhir" };
            if sisi == "akhir" {
                if let Some(l) = &lo {
                    let l = self.coerce_longgar(l, &JrgType::I64);
                    self.emit_slice_bounds_check(&l, panjang, &panjang.text);
                }
            }
            let v = if sisi == "awal" { lo.unwrap() } else { hi.unwrap() };
            let pesan = crate::interpreter::galat_index::batas_potongan_tipe(sisi, &v.ty.nama_typeof());
            self.tempat_tiruan_sesudah(&pesan, span)?;
            return Ok(None);
        }
        let lo_v = lo.map(|v| self.coerce_longgar(&v, &JrgType::I64)).unwrap_or(nol);
        let hi_v = hi.map(|v| self.coerce_longgar(&v, &JrgType::I64)).unwrap_or_else(|| panjang.clone());
        Ok(Some(self.emit_slice_bounds_check(&lo_v, &hi_v, &panjang.text)))
    }

    /// `t[a:b]`: tipe hasilnya bergantung pada batasnya, jadi batasnya harus pasti saat kompilasi.
    fn potong_tuple(&mut self, ptr: &str, oty: &JrgType, lo: (&Option<Box<Expr>>, Option<Val>),
                    hi: (&Option<Box<Expr>>, Option<Val>), span: Span) -> Result<Val, String> {
        use crate::interpreter::galat_index as g;
        let JrgType::Tuple(elems) = oty else { unreachable!() };
        let n = elems.len();
        for (sisi, v) in [("awal", &lo.1), ("akhir", &hi.1)] {
            if let Some(v) = v {
                if !v.ty.is_int() && !matches!(v.ty, JrgType::Never | JrgType::Any) {
                    self.tempat_tiruan_sesudah(&g::batas_potongan_tipe(sisi, &v.ty.nama_typeof()), span)?;
                    return Ok(Val::unit());
                }
            }
        }
        let konstan = |e: &Option<Box<Expr>>| -> Result<Option<i64>, String> {
            match e {
                None => Ok(None),
                Some(e) => Self::indeks_konstan(e).map(Some).ok_or_else(|| super::galat::tandai(span,
                    "batas potongan tuple di backend LLVM harus angka yang sudah pasti saat kompilasi (`t[1:]`) — \
                     elemen tuple boleh bertipe berbeda, jadi tipe hasil `t[a:b]` bergantung pada batasnya")),
            }
        };
        let (lo_k, hi_k) = (konstan(lo.0)?, konstan(hi.0)?);
        let mut pos = [0usize, n];
        for (j, (sisi, k)) in [("awal", lo_k), ("akhir", hi_k)].into_iter().enumerate() {
            let Some(raw) = k else { continue };
            let p = if raw < 0 { raw + n as i64 } else { raw };
            if p < 0 || p > n as i64 {
                self.tempat_tiruan_sesudah(&g::batas_potongan_luar(sisi, raw as i128, n), span)?;
                return Ok(Val::unit());
            }
            pos[j] = p as usize;
        }
        if pos[0] > pos[1] {
            self.tempat_tiruan_sesudah(&g::potongan_terbalik(pos[0], pos[1]), span)?;
            return Ok(Val::unit());
        }
        // Tuple tanpa elemen: `{ }`, tampil `()`, `typeof` `tuple` — seperti acuan (dulu ditolak).
        if pos[0] == pos[1] {
            return Ok(Val { text: "zeroinitializer".into(), ty: JrgType::Tuple(Vec::new()) });
        }
        let asal = self.load_place(ptr, oty);
        let hasil_ty = JrgType::Tuple(elems[pos[0]..pos[1]].to_vec());
        let mut agg = "undef".to_string();
        for (j, i) in (pos[0]..pos[1]).enumerate() {
            let e = self.fresh("tpot.");
            self.emit(format!("{} = extractvalue {} {}, {}", e, oty.llvm_ty(), asal.text, i));
            let r = self.fresh("tpot.");
            self.emit(format!("{} = insertvalue {} {}, {} {}, {}", r, hasil_ty.llvm_ty(), agg, elems[i].llvm_ty(), e, j));
            agg = r;
        }
        Ok(Val { text: agg, ty: hasil_ty })
    }

    /// `s[lo:hi]` — potongan string, per KARAKTER.
    pub(crate) fn potong_string(&mut self, sv: &Val, lo: &Option<Box<Expr>>, hi: &Option<Box<Expr>>) -> Result<Val, String> {
        self.ensure_extern("declare i64 @jrg_str_char_count(ptr, i64)", "jrg_str_char_count");
        self.ensure_extern("declare void @jrg_str_slice(ptr, i64, i64, i64, ptr, ptr)", "jrg_str_slice");

        let (sptr, slen) = self.split_str(sv);
        let n = self.fresh("sscnt.");
        self.emit(format!("{} = call i64 @jrg_str_char_count(ptr {}, i64 {})", n, sptr, slen));

        let lo_v = match lo {
            Some(e) => { let v = self.codegen_expr(e, Some(&JrgType::I64))?; self.coerce_longgar(&v, &JrgType::I64) }
            None => Val { text: "0".into(), ty: JrgType::I64 },
        };
        let hi_v = match hi {
            Some(e) => { let v = self.codegen_expr(e, Some(&JrgType::I64))?; self.coerce_longgar(&v, &JrgType::I64) }
            None => Val { text: n.clone(), ty: JrgType::I64 },
        };
        let (lo_v, hi_v) = self.emit_slice_bounds_check(&lo_v, &hi_v, &n);
        self.potong_string_sah(&sptr, &slen, &lo_v, &hi_v)
    }

    /// `s[lo:hi]` yang batasnya sudah dievaluasi: jenisnya diperiksa dengan kalimat acuan.
    fn potong_string_val(&mut self, sv: &Val, lo: Option<Val>, hi: Option<Val>, span: Span) -> Result<Val, String> {
        self.ensure_extern("declare i64 @jrg_str_char_count(ptr, i64)", "jrg_str_char_count");
        self.ensure_extern("declare void @jrg_str_slice(ptr, i64, i64, i64, ptr, ptr)", "jrg_str_slice");
        let (sptr, slen) = self.split_str(sv);
        let n = self.fresh("sscnt.");
        self.emit(format!("{} = call i64 @jrg_str_char_count(ptr {}, i64 {})", n, sptr, slen));
        let panjang = Val { text: n, ty: JrgType::I64 };
        let Some((lo_v, hi_v)) = self.batas_potongan_dicek(lo, hi, &panjang, span)? else {
            return Ok(Val::unit());
        };
        self.potong_string_sah(&sptr, &slen, &lo_v, &hi_v)
    }

    fn potong_string_sah(&mut self, sptr: &str, slen: &str, lo_v: &Val, hi_v: &Val) -> Result<Val, String> {
        let outp = self.alloca_raw("ssp.", "ptr", None);
        let outl = self.alloca_raw("ssl.", "i64", None);
        self.emit(format!(
            "call void @jrg_str_slice(ptr {}, i64 {}, i64 {}, i64 {}, ptr {}, ptr {})",
            sptr, slen, lo_v.text, hi_v.text, outp, outl));
        self.join_str(&outp, &outl)
    }

}


pub fn is_bare_array_type(te: &TypeExpr) -> bool {
    matches!(te, TypeExpr::Named(NamedType { name, args, .. }) if name == "array" && args.is_empty())
}

/// Dict di dalam dict yang isinya menuju nilai bertag (`dict<dict<int>>` ke `dict<dict<bertag>>`).
/// Kebalikan `isi_jadi_bertag` di dalam slice: `array<array<nilai bertag>>` ke `array<array<string>>`.
fn isi_dibuka(a: &JrgType, b: &JrgType) -> bool {
    match (a, b) {
        (JrgType::Slice(x), JrgType::Slice(y)) => **x == JrgType::Any && **y != JrgType::Any
            && !matches!(**y, JrgType::Slice(_) | JrgType::Array(..) | JrgType::Dict(_))
            && super::codegen_any::tag_untuk(y).is_some() || isi_dibuka(x, y),
        _ => false,
    }
}

fn isi_jadi_bertag(a: &JrgType, b: &JrgType) -> bool {
    match (a, b) {
        (JrgType::Dict(x), JrgType::Dict(y))
        // Slice juga: `array<array<string>>` ke `array<array<nilai bertag>>` (`std/diff`).
        | (JrgType::Slice(x) | JrgType::Array(x, _), JrgType::Slice(y)) => **x != JrgType::Any && **y == JrgType::Any
            && super::codegen_any::tag_untuk(x).is_some() || isi_jadi_bertag(x, y),
        _ => false,
    }
}
