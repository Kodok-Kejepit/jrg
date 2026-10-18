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
            if e1 != e2 && super::codegen_any::luruh(e1) == **e2 {
                let (e1, e2) = ((**e1).clone(), (**e2).clone());
                if let Some(r) = self.ubah_tiap_elemen(v, &e1, &e2) { return r; }
            }
        }
        if let (JrgType::Dict(e1), JrgType::Dict(e2)) = (&v.ty, target) {
            if e1 != e2 && super::codegen_any::luruh(e1) == **e2 {
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
        if !iv.ty.is_int() && iv.ty != JrgType::Never {
            return Err(super::galat::tandai(span, crate::interpreter::galat_index::tipe_index_salah(
                wadah, &iv.ty.nama_typeof())));
        }
        Ok(self.coerce_longgar(&iv, &JrgType::I64))
    }

    // ── places (lvalues) ────────────────────────────────────────────────

    pub fn get_place(&mut self, e: &Expr) -> Result<(String, JrgType), String> {
        // Dipasang oleh langkah sesudahnya: `e` adalah langkah ANTARA sebuah jalur tulis.
        let antara = std::mem::take(&mut self.tulis_antara);
        match e {
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
                let iv = self.codegen_expr(idx, Some(&JrgType::I64))?;
                let iv = self.coerce_longgar(&iv, &JrgType::I64);
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
                let idx = layout.field_index(field)
                    .ok_or_else(|| format!("struct `{}` tidak punya field `{}`", name, field))?;
                // Satu pintu untuk BACA dan TULIS: `codegen_field` dan jalur target
                // penugasan dua-duanya lewat sini, jadi penegakan visibilitas tidak bisa …
                crate::vm::visibilitas::periksa(super::generik::dasar(name), field, self.cur_self_ty.as_deref())
                    .map_err(|e| crate::llvm::galat::tandai(span, e))?;
                let fty = layout.fields[idx].1.clone();
                let r = self.fresh("fld.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", r, oty.llvm_ty(), optr, idx));
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
            return Err(super::galat::tandai(span, format!(
                "index [{}] di luar batas — tuple ini isinya {} elemen \
                 (index valid: 0..{}, atau -1..-{} kalau mau hitung dari belakang)",
                raw, n, n - 1, n
            )));
        }

        let r = self.fresh("tidx.");
        self.emit(format!(
            "{} = getelementptr {}, ptr {}, i32 0, i32 {}",
            r, oty.llvm_ty(), optr, i
        ));
        Ok((r, elems[i as usize].clone()))
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
            let kunci = Expr::Literal(crate::parser::LitKind::Str(field.to_string()), span);
            return self.codegen_index(obj, &kunci, span);
        }
        let (ptr, oty) = self.get_place(obj)?;
        if oty == JrgType::Any {
            let v = self.load_place(&ptr, &oty);
            return self.field_titipan(&v, field, span);
        }
        let (fptr, fty) = self.get_field_place(&ptr, &oty, field, span)?;
        Ok(self.load_place(&fptr, &fty))
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

    pub fn codegen_index(&mut self, obj: &Expr, idx: &Expr, span: Span) -> Result<Val, String> {
        // String tidak lewat `get_place`: nilainya `%JrgStr` (ptr+len), dan `s[i]`
        // menghasilkan sebuah `rune` yang DIHITUNG, bukan alamat yang dibaca.
        let obj_ty = self.peek_expr_type(obj);
        if matches!(obj_ty, Some(JrgType::Str)) {
            let sv = self.codegen_expr(obj, Some(&JrgType::Str))?;
            let iv = self.indeks_bulat(idx, "string", span)?;
            return self.karakter_string(&sv, &iv);
        }

        let (ptr, oty) = self.get_place(obj)?;
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
        if let JrgType::Tuple(_) = &oty {
            let (eptr, ety) = self.tempat_elemen_tuple(&ptr, &oty, idx, span)?;
            return Ok(self.load_place(&eptr, &ety));
        }
        let iv = self.indeks_bulat(idx, &oty.nama_typeof(), span)?;
        let (eptr, ety) = self.get_index_place(&ptr, &oty, &iv, span)?;
        Ok(self.load_place(&eptr, &ety))
    }

    /// `s[i]` — karakter ke-`i` sebuah string, sebagai `rune`.
    pub(crate) fn karakter_string(&mut self, sv: &Val, iv: &Val) -> Result<Val, String> {
        self.ensure_extern("declare i64 @jrg_str_char_count(ptr, i64)", "jrg_str_char_count");
        self.ensure_extern("declare i32 @jrg_str_char_at(ptr, i64, i64)", "jrg_str_char_at");

        let (sptr, slen) = self.split_str(sv);
        // Panjang dalam KARAKTER, bukan byte — itu satuan yang dipakai
        // indeksnya, jadi pemeriksaan batas harus memakai satuan yang sama.
        let n = self.fresh("scnt.");
        self.emit(format!("{} = call i64 @jrg_str_char_count(ptr {}, i64 {})", n, sptr, slen));

        let ix = self.emit_bounds_check_dyn(iv, &n, ("string", "karakter"));

        let c = self.fresh("schr.");
        self.emit(format!(
            "{} = call i32 @jrg_str_char_at(ptr {}, i64 {}, i64 {})",
            c, sptr, slen, ix.text
        ));
        Ok(Val { text: c, ty: JrgType::Rune })
    }

    pub fn codegen_slice(&mut self, obj: &Expr, lo: &Option<Box<Expr>>, hi: &Option<Box<Expr>>, span: Span) -> Result<Val, String> {
        let (ptr, oty) = self.get_place(obj)?;
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
                return self.potong_string(&sv, lo, hi);
            }
            _ => return Err(format!("[{}:{}] tidak bisa slicing pada tipe `{}`", span.lo.0, span.hi.0, oty.display_name())),
        };
        let lo_v = match lo {
            Some(e) => { let v = self.codegen_expr(e, Some(&JrgType::I64))?; self.coerce_longgar(&v, &JrgType::I64) }
            None => Val { text: "0".into(), ty: JrgType::I64 },
        };
        let hi_v = match hi {
            Some(e) => { let v = self.codegen_expr(e, Some(&JrgType::I64))?; self.coerce_longgar(&v, &JrgType::I64) }
            None => base_len.clone(),
        };
        let (lo_v, hi_v) = self.emit_slice_bounds_check(&lo_v, &hi_v, &base_len.text);
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
