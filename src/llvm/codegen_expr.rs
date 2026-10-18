// src/llvm/codegen_expr.rs — jrgc LLVM backend — src/llvm/codegen_expr.rs
// Expression-level lowering. Continuation of `Codegen` from codegen.rs.

#![allow(dead_code)]

use crate::parser::*;
use crate::lexer::Span;

use super::codegen::{Codegen, Val};
use super::sema::*;
use super::types::*;

pub fn fmt_hexfloat(v: f64) -> String {
    format!("0x{:016X}", v.to_bits())
}

/// Tipe yang dipakai sebuah literal kalau TIDAK ada konteks yang memberinya.
fn tipe_literal_tanpa_konteks(lit: &LitKind) -> Option<JrgType> {
    match lit {
        LitKind::Int(_) => Some(JrgType::I64),
        LitKind::Float(_) => Some(JrgType::F64),
        _ => None,
    }
}

/// Tipe bawaan sebuah EKSPRESI literal (termasuk yang dibungkus `-`).
pub(crate) fn tipe_bawaan_ekspresi(e: &Expr) -> Option<JrgType> {
    match e {
        Expr::Literal(lit, _) => tipe_literal_tanpa_konteks(lit),
        Expr::Unary { op: UnaryOp::Neg, expr, .. } => tipe_bawaan_ekspresi(expr),
        _ => None,
    }
}

fn is_malleable_literal(e: &Expr) -> bool {
    match e {
        Expr::Literal(LitKind::Int(_), _) | Expr::Literal(LitKind::Float(_), _) => true,
        Expr::Unary { op: UnaryOp::Neg, expr, .. } => is_malleable_literal(expr),
        // `[]` termasuk di sini karena sifatnya PERSIS sama dengan literal angka:
        // tipenya belum ditentukan sampai konteks memberinya.
        Expr::Array { elems, .. } if elems.is_empty() => true,
        _ => false,
    }
}

/// Eksponen bulat negatif yang DITULIS (`2 ** -1`): hasilnya pecahan, seperti di dua mesin
/// lain, dan tipenya bisa diputuskan saat kompilasi karena tandanya terlihat di sumber.
pub(crate) fn eksponen_negatif_tertulis(e: &Expr) -> Option<f64> {
    match e {
        Expr::Unary { op: UnaryOp::Neg, expr, .. } => match expr.as_ref() {
            Expr::Literal(LitKind::Int(n), _) if *n > 0 => Some(-(*n as f64)),
            _ => None,
        },
        _ => None,
    }
}

impl Codegen {
    /// Pembungkus: jalankan lowering-nya, lalu kalau gagal TANPA lokasi, tandai
    /// dengan span ekspresi ini.
    pub fn codegen_expr(&mut self, expr: &Expr, expected: Option<&JrgType>) -> Result<Val, String> {
        // Span aktif juga DIPASANG di sini, bukan cuma dipakai saat gagal:
        // jalur yang mengumpulkan error (`coerce` → `lapor`) tidak lewat
        // `map_err`, jadi ia butuh span yang sedang berlaku.
        let lama = self.masuk_span(expr.span());
        let hasil = self.codegen_expr_dalam(expr, expected)
            .map_err(|e| crate::llvm::galat::beri_span_bila_kosong(e, expr.span()));
        self.pulih_span(lama);
        hasil
    }

    fn codegen_expr_dalam(&mut self, expr: &Expr, expected: Option<&JrgType>) -> Result<Val, String> {
        match expr {
            Expr::Literal(lit, span) => self.codegen_literal(lit, expected, *span),
            Expr::Ident(name, span) => self.codegen_ident(name, expected, *span),
            Expr::Unary { op, expr, span } => self.codegen_unary(op, expr, expected, *span),
            Expr::Binary { op, lhs, rhs, span } => self.codegen_binary(op, lhs, rhs, *span),
            Expr::Ternary { cond, then, else_, span } => self.codegen_ternary(cond, then, else_, expected, *span),
            Expr::Pipe { lhs, rhs, span } => self.codegen_pipe(lhs, rhs, expected, *span),
            Expr::Cast { expr, ty, span } => self.codegen_cast(expr, ty, *span),
            Expr::Field { obj, field, span } => self.codegen_field(obj, field, *span),
            Expr::Method { obj, method, args, span } => self.codegen_method(obj, method, args, expected, *span),
            Expr::Index { obj, idx, span } => self.codegen_index(obj, idx, *span),
            Expr::Slice { obj, lo, hi, span } => self.codegen_slice(obj, lo, hi, *span),
            Expr::Call { func, args, span } => self.codegen_call(func, args, expected, *span),
            Expr::Await { .. } => Err("`await` belum didukung backend LLVM v1 (async/concurrency belum diimplementasi)".into()),
            Expr::Propagate { expr, span } => self.codegen_propagate(expr, *span),
            Expr::Closure { params, body, is_move, span, .. } =>
                self.codegen_closure(params, body, *is_move, expected, *span),
            Expr::Block(b) => self.codegen_block_expr(b, expected),
            Expr::Struct { ty, fields, spread, span } => self.codegen_struct_lit(ty, fields, spread, expected, *span),
            Expr::Array { elems, span } => self.codegen_array_lit(elems, expected, *span),
            Expr::ArrayRepeat { elem, count, span } => self.codegen_array_repeat(elem, count, expected, *span),
            Expr::Tuple { elems, .. } if elems.is_empty() => Ok(Val::unit()),
            Expr::Tuple { elems, span } => self.codegen_tuple_lit(elems, expected, *span),
            Expr::Range { .. } => Err("nilai `Range` berdiri sendiri belum didukung (pakai langsung sebagai target `for x in lo..hi`)".into()),
            Expr::MakeChan { ty, cap, span } => self.codegen_make_chan(ty, cap, *span),
            Expr::Send { chan, val, span } => self.codegen_send(chan, val, *span),
            Expr::Recv { chan, span } => self.codegen_recv(chan, *span),
            Expr::PathExpr { segments, span } => self.codegen_path_expr(segments, expected, *span),
            Expr::Match { scrutinee, arms, span } => self.codegen_match_expr(scrutinee, arms, expected, *span),
            Expr::If(s) => self.codegen_if_expr(s, expected),
            // Sama seperti di VM: ditolak terang-terangan, bukan diam-diam
            // menghasilkan string kosong. Interpreter adalah acuan semantik.
            Expr::Template { parts, span } => self.codegen_template(parts, *span),
            Expr::Error(_) => Err("ekspresi tidak valid (parse error)".into()),
        }
    }

    pub fn codegen_block_expr(&mut self, block: &Block, expected: Option<&JrgType>) -> Result<Val, String> {
        self.push_scope();
        for stmt in &block.stmts {
            self.codegen_stmt(stmt)?;
            if self.is_terminated() { break; }
        }
        let result = if self.is_terminated() {
            Val { text: "undef".into(), ty: expected.cloned().unwrap_or(JrgType::Unit) }
        } else if let Some(tail) = &block.tail {
            self.codegen_expr(tail, expected)?
        } else {
            Val::unit()
        };
        self.pop_scope();
        Ok(result)
    }

    // ── literals / idents ───────────────────────────────────────────────

    fn codegen_literal(&mut self, lit: &LitKind, expected: Option<&JrgType>, _span: Span) -> Result<Val, String> {
        match lit {
            LitKind::Int(v) => {
                // Literal bulat tetap bulat di tempat bertipe pecahan, seperti di dua mesin lain;
                // tempat yang menyimpannya menolak (`pesan_ganti_jenis`), aritmetika mempromosikannya.
                let ty = match expected {
                    Some(t) if t.is_numeric() && !t.is_float() => t.clone(),
                    _ => tipe_literal_tanpa_konteks(lit).expect("Int selalu punya bawaan"),
                };
                if ty.is_float() {
                    Ok(Val { text: fmt_hexfloat(*v as f64), ty })
                } else {
                    // Dulu dipotong diam-diam: `170141183460469231731687303715884105727` jadi `-1`.
                    let (lo, hi) = super::codegen_konversi::jangkauan(&ty);
                    if *v > hi as u128 {
                        return Err(super::galat::tandai(_span, pesan_literal_tak_muat(*v, &ty, lo, hi)));
                    }
                    Ok(Val { text: v.to_string(), ty })
                }
            }
            LitKind::Float(v) => {
                let ty = match expected {
                    Some(t) if t.is_float() => t.clone(),
                    _ => tipe_literal_tanpa_konteks(lit).expect("Float selalu punya bawaan"),
                };
                Ok(Val { text: fmt_hexfloat(*v), ty })
            }
            LitKind::Imag(_) => Err("bilangan imajiner/kompleks belum didukung backend LLVM v1".into()),
            LitKind::Rune(c) => Ok(Val { text: (*c as u32).to_string(), ty: JrgType::Rune }),
            LitKind::Str(s) | LitKind::RawStr(s) => Ok(self.intern_string(s)),
            LitKind::Bool(b) => Ok(Val { text: (if *b { "1" } else { "0" }).to_string(), ty: JrgType::Bool }),
            LitKind::Nil => Ok(Val { text: "zeroinitializer".into(), ty: JrgType::Nil }),
        }
    }

    fn codegen_ident(&mut self, name: &str, expected: Option<&JrgType>, span: Span) -> Result<Val, String> {
        if let Some((ptr, ty)) = self.lookup_local(name) {
            return Ok(self.load_place(&ptr, &ty));
        }
        // `var` tingkat modul dicari SESUDAH lokal: sebuah lokal bernama sama
        // menutupinya, sama seperti di acuannya.
        if let Some((sym, ty)) = self.var_global.get(&self.k(name)).cloned() {
            return Ok(self.load_place(&sym, &ty));
        }
        // Varian enum TANPA payload, disebut telanjang (`B`, bukan `E::B`).
        if let Some(r) = self.varian_unit_telanjang(name, span) { return r; }
        // Nama fungsi yang dipakai sebagai nilai. Sesudah lokal dan global,
        // supaya sebuah variabel bernama sama tetap menang.
        if let Some(r) = self.nilai_fungsi_bernama(name, expected, span) { return r; }
        if name == "None" {
            let t = match expected {
                Some(JrgType::Enum(en)) if self.option_args.contains_key(en) => self.option_args[en].clone(),
                _ if self.mode_duga => return Ok(Self::nilai_belum_tahu()),
                _ => JrgType::Any,
            };
            let en = self.instantiate_option(&t);
            return self.build_enum_unit_variant(&en, "None");
        }
        if let Some(c) = self.tables.consts.get(&self.k(name)).cloned() {
            let want = match &c.ty {
                Some(te) => Some(self.resolve_type_pub(te)?),
                None => expected.cloned(),
            };
            return self.codegen_expr(&c.value, want.as_ref());
        }
        // Kata dari bahasa lain dijawab padanannya lewat definisi BERSAMA, bukan
        // tebakan ejaan — Fase MUDAH-2 menutup ini untuk dua mesin, dan backend ini …
        if crate::interpreter::padanan_bahasa_lain(name).is_some() {
            return Err(crate::interpreter::pesan_nama_tak_dikenal(name, None));
        }
        if let Some(r) = self.nilai_konstruktor(name, expected, span) { return r; }
        if let Some(p) = self.penjelasan_bukan_nilai(name) { return Err(p); }
        // Bawaan bahasa yang disebut tanpa dipanggil: namanya ADA, yang belum ada nilainya di sini.
        if crate::vm::ada_builtin(name) {
            return Err(format!(
                "`{0}` dipakai sebagai NILAI (tanpa dipanggil), dan backend `--emit-llvm` belum mendukung \
                 fungsi bawaan sebagai nilai\nbantuan: bungkus dengan closure — `|x| {0}(x)` — atau jalankan tanpa `--emit-llvm`", name));
        }
        Err(format!("`{}` tidak ditemukan (variabel/konstanta tidak dikenal)", name))
    }

    // ── unary / binary ──────────────────────────────────────────────────

    fn codegen_unary(&mut self, op: &UnaryOp, e: &Expr, expected: Option<&JrgType>, span: Span) -> Result<Val, String> {
        match op {
            UnaryOp::Neg => {
                let hint = expected.cloned();
                // `-9223372036854775808`: literalnya sendiri satu di atas batas, negasinya muat.
                if let Expr::Literal(LitKind::Int(n), _) = e {
                    let ty = match &hint { Some(t) if t.is_int() => t.clone(), _ => JrgType::I64 };
                    let (lo, hi) = super::codegen_konversi::jangkauan(&ty);
                    if lo < 0 && *n == hi as u128 + 1 && !hint.as_ref().is_some_and(|t| t.is_float()) {
                        return Ok(Val { text: lo.to_string(), ty });
                    }
                }
                let v = self.codegen_expr(e, hint.as_ref())?;
                if let Some(h) = self.operator_pengguna_unary(op, &v, span)? {
                    return Ok(h);
                }
                if v.ty.is_float() {
                    let r = self.fresh("neg.");
                    self.emit(format!(
                        "{} = fneg{} {} {}",
                        r,
                        self.fast_math_flag(),
                        v.ty.llvm_ty(),
                        v.text
                    ));
                    Ok(Val { text: r, ty: v.ty })
                } else if v.ty.is_int() {
                    let nol = Val { text: "0".into(), ty: v.ty.clone() };
                    Ok(self.hitung_terperiksa(&BinaryOp::Sub, &nol, &v))
                } else {
                    Err(format!("operator unary `-` tidak berlaku untuk tipe `{}`", v.ty))
                }
            }
            UnaryOp::Not => {
                let v = self.codegen_expr(e, Some(&JrgType::Bool))?;
                if v.ty.is_int() || v.ty.is_float() || matches!(v.ty, JrgType::Str | JrgType::Rune | JrgType::Nil) {
                    if v.ty.is_int() {
                        return Err("operator `!` cuma untuk bool, dapat `int` — kalau maksudnya \
                            membalik bit, pakai `~`; kalau maksudnya \"nilainya nol atau \
                            bukan\", tulis perbandingannya eksplisit (mis. `x == 0`)".to_string());
                    }
                    return Err(crate::interpreter::pesan_op_unary("!", &v.ty.nama_typeof()));
                }
                let v = self.coerce_longgar(&v, &JrgType::Bool);
                let i1 = self.to_i1(&v);
                let r = self.fresh("not.");
                self.emit(format!("{} = xor i1 {}, true", r, i1));
                Ok(self.from_i1(&r))
            }
            UnaryOp::BitNot => {
                let hint = expected.cloned();
                let v = self.codegen_expr(e, hint.as_ref())?;
                if !v.ty.is_int() { return Err(format!("operator `~` butuh tipe integer, dapat `{}`", v.ty)); }
                let r = self.fresh("bnot.");
                self.emit(format!("{} = xor {} {}, -1", r, v.ty.llvm_ty(), v.text));
                Ok(Val { text: r, ty: v.ty })
            }
            UnaryOp::Ref | UnaryOp::RefMut | UnaryOp::Move => self.codegen_expr(e, expected),
            UnaryOp::Deref => Err("dereference mentah (`*ptr`) belum didukung backend LLVM v1".into()),
        }
    }

    fn codegen_binary(&mut self, op: &BinaryOp, lhs: &Expr, rhs: &Expr, span: Span) -> Result<Val, String> {
        if matches!(op, BinaryOp::And | BinaryOp::Or) {
            return self.codegen_short_circuit(op, lhs, rhs, span);
        }
        // ── operand diturunkan SEKALI, keputusannya dari tipe NYATA ───────
        let lhs_malleable = is_malleable_literal(lhs);
        let rhs_malleable = is_malleable_literal(rhs);

        /// Petunjuk tipe untuk sisi KEDUA, dari tipe nyata sisi pertama.
        fn petunjuk(ty: &JrgType) -> Option<JrgType> {
            // `nil` dan `()` tidak pernah memaksa tipe sisi lain — `()` yang
            // memaksa membuat literal `6` di `kosong() == 6` ikut jadi `()`.
            if matches!(ty, JrgType::Nil | JrgType::Unit) {
                return None;
            }
            // Sebuah string tidak pernah memaksa tipe sisi lain, operator APA PUN — pegangan
            // (`Atomic`, `Mutex`, channel) juga: `a == 1` menjawab `false`, seperti dua mesin lain.
            if *ty == JrgType::Str || matches!(ty, JrgType::Sinkron(_) | JrgType::Chan(_)) {
                None
            } else {
                Some(ty.clone())
            }
        }

        // Koersi penyatuan di bawah ini untuk literal ANGKA: `1 + 2.5` menjadikan
        // `1` sebuah f64.
        fn skalar(t: &JrgType) -> bool {
            !matches!(t, JrgType::Array(_, _) | JrgType::Slice(_)
                       | JrgType::Tuple(_) | JrgType::Struct(_))
        }

        // Literal PECAHAN tidak pernah menyesuaikan diri ke bilangan bulat: itu memotong.
        let boleh_ikut = |dari: &JrgType, ke: &JrgType| !(dari.is_float() && ke.is_int());
        let (lv, rv) = if lhs_malleable && !rhs_malleable {
            let rv = self.codegen_expr(rhs, None)?;
            let h = petunjuk(&rv.ty);
            let lv = self.codegen_expr(lhs, h.as_ref())?;
            let lv = if h.is_some() && skalar(&lv.ty) && skalar(&rv.ty) && boleh_ikut(&lv.ty, &rv.ty) {
                self.coerce_longgar(&lv, &rv.ty)
            } else { lv };
            (lv, rv)
        } else {
            let lv = self.codegen_expr(lhs, None)?;
            // Operan kiri milik `var` modul masih menunjuk penyangganya; operan kanan yang bisa
            // menulisnya akan ikut terbaca di sini. Acuannya: kiri dibaca LEBIH DULU.
            let lv = if (Self::punya_penunjuk(&lv.ty) || self.isi_dalam_perlu_salin(&lv.ty))
                && self.akar_var_modul(lhs) && self.bisa_menulis_modul(rhs) { self.salin_dalam(&lv) } else { lv };
            let h = petunjuk(&lv.ty);
            let rv = self.codegen_expr(rhs, h.as_ref())?;
            let rv = if rhs_malleable && h.is_some() && skalar(&lv.ty) && skalar(&rv.ty)
                && boleh_ikut(&rv.ty, &lv.ty) {
                self.coerce_longgar(&rv, &lv.ty)
            } else { rv };
            (lv, rv)
        };
        // `==` lintas golongan skalar diputuskan SEBELUM penyatuan: literal angka di samping rune
        // atau bool sudah ikut tipenya, padahal `'o' == 111` dan `true == 1` tidak pernah sama.
        if matches!(op, BinaryOp::Eq | BinaryOp::Ne) {
            let angka = |e: &Expr| matches!(e, Expr::Literal(LitKind::Int(_) | LitKind::Float(_), _));
            let bukan_angka = |t: &JrgType| matches!(t, JrgType::Rune | JrgType::Bool);
            if (angka(lhs) && bukan_angka(&rv.ty)) || (angka(rhs) && bukan_angka(&lv.ty)) {
                return Ok(Val { text: if *op == BinaryOp::Ne { "1" } else { "0" }.to_string(), ty: JrgType::Bool });
            }
            if let Some(v) = Self::banding_beda_golongan(op, &lv.ty, &rv.ty) { return Ok(v); }
        }
        let (lv, rv) = self.naikkan_campuran(lv, rv);
        if *op == BinaryOp::Pow && lv.ty.is_int() && rv.ty.is_int() {
            if let Some(k) = eksponen_negatif_tertulis(rhs) { return Ok(self.pangkat_pecahan(&lv, k)); }
        }

        // Operator yang didefinisikan pengguna didahulukan atas aturan bawaan.
        if let Some(h) = self.operator_pengguna_biner(op, &lv, &rv, span)? {
            return Ok(h);
        }
        self.gabung_nilai(op, lv, rv, span)
    }

    /// Bulat lawan pecahan: yang bulat NAIK jadi pecahan, aturan acuan — `1 + 2.5` = 3.5 dan
    /// `1 < 1.5` = true. Dulu literal pecahannya dipotong ke `int` tanpa kata.
    pub(crate) fn naikkan_campuran(&mut self, lv: Val, rv: Val) -> (Val, Val) {
        if lv.ty.is_int() && rv.ty.is_float() {
            let t = rv.ty.clone();
            (self.coerce_longgar(&lv, &t), rv)
        } else if lv.ty.is_float() && rv.ty.is_int() {
            let t = lv.ty.clone();
            let r = self.coerce_longgar(&rv, &t);
            (lv, r)
        } else {
            (lv, rv)
        }
    }

    /// Layani operator awalan lewat metode milik pengguna.
    fn operator_pengguna_unary(
        &mut self,
        op: &UnaryOp,
        v: &Val,
        span: Span,
    ) -> Result<Option<Val>, String> {
        use crate::interpreter::operator as op_tabel;
        let Some(metode) = op_tabel::nama_unary(op) else { return Ok(None) };
        let nama = match &v.ty {
            JrgType::Struct(n) | JrgType::Enum(n) => n.clone(),
            _ => return Ok(None),
        };
        let Some(decl) = self.tables.find_method(&nama, metode).cloned() else { return Ok(None) };
        let place = self.spill_to_place(v);
        Ok(Some(self.panggil_operator(&nama, &decl, &place, &v.ty, None, span)?))
    }

    /// Layani operator biner lewat metode milik pengguna, kalau tipenya punya.
    pub(crate) fn operator_pengguna_biner(
        &mut self,
        op: &BinaryOp,
        lv: &Val,
        rv: &Val,
        span: Span,
    ) -> Result<Option<Val>, String> {
        use crate::interpreter::operator as op_tabel;
        let Some(metode) = op_tabel::nama_biner(op) else { return Ok(None) };
        let nama = match &lv.ty {
            JrgType::Struct(n) | JrgType::Enum(n) => n.clone(),
            _ => return Ok(None),
        };
        let Some(decl) = self.tables.find_method(&nama, metode).cloned() else { return Ok(None) };

        let place = self.spill_to_place(lv);
        let hasil = self.panggil_operator(&nama, &decl, &place, &lv.ty, Some(rv.clone()), span)?;

        if op_tabel::pakai_eq(op) {
            if hasil.ty != JrgType::Bool {
                return Err(format!("[{}:{}] {}", span.lo.0, span.hi.0,
                    op_tabel::pesan_hasil(&nama, metode, "bool", &hasil.ty.display_name())));
            }
            if matches!(op, BinaryOp::Ne) {
                // `bool` di backend ini diwakili `i8`, bukan `i1` — lewat
                // `to_i1`/`from_i1` supaya tidak ada representasi kedua.
                let i1 = self.to_i1(&hasil);
                let r = self.fresh("opne.");
                self.emit(format!("{} = xor i1 {}, true", r, i1));
                return Ok(Some(self.from_i1(&r)));
            }
            return Ok(Some(hasil));
        }
        if op_tabel::pakai_cmp(op) {
            if !hasil.ty.is_int() {
                return Err(format!("[{}:{}] {}", span.lo.0, span.hi.0,
                    op_tabel::pesan_hasil(&nama, metode, "int", &hasil.ty.display_name())));
            }
            // Hasil `cmp` ditafsirkan di IR, bukan di Rust: nilainya baru ada
            // saat program jalan.
            let pred = match op {
                BinaryOp::Lt => "slt", BinaryOp::Le => "sle",
                BinaryOp::Gt => "sgt", _ => "sge",
            };
            let r = self.fresh("opcmp.");
            self.emit(format!("{} = icmp {} {} {}, 0", r, pred, hasil.ty.llvm_ty(), hasil.text));
            return Ok(Some(self.from_i1(&r)));
        }
        Ok(Some(hasil))
    }

    /// Gabungkan dua nilai yang SUDAH diturunkan dengan sebuah operator.
    pub fn gabung_nilai(&mut self, op: &BinaryOp, lv: Val, rv: Val, span: Span)
        -> Result<Val, String>
    {
        //
        // Diputuskan di sini, SESUDAH kedua sisi punya tipe nyata, jadi tidak
        // ada bentuk yang bisa lolos hanya karena `peek` tidak mengenalinya.
        if lv.ty == JrgType::Str || rv.ty == JrgType::Str {
            match op {
                BinaryOp::Add => {
                    let l = self.jadikan_string(&lv, span)?;
                    let r = self.jadikan_string(&rv, span)?;
                    return self.str_concat(&l, &r);
                }
                // `"ab" * 3` dan `3 * "ab"` — dua arah.
                BinaryOp::Mul => {
                    let (sv, nv) = if lv.ty == JrgType::Str { (lv, rv) } else { (rv, lv) };
                    if sv.ty != JrgType::Str || !nv.ty.is_int() {
                        return Err(super::galat::tandai(span, format!(
                            "`*` antara `{}` dan `{}` tidak berarti apa-apa\n\
                             bantuan: `str * n` mengulang stringnya sebanyak n kali",
                            sv.ty.display_name(), nv.ty.display_name()
                        )));
                    }
                    return self.ulang_string(&sv, &nv);
                }
                _ => {}
            }
        }

        // ── konkatenasi array ─────────────────────────────────────────────
        if *op == BinaryOp::Add {
            let elem_dari = |t: &JrgType| match t {
                JrgType::Array(e, _) | JrgType::Slice(e) => Some((**e).clone()),
                _ => None,
            };
            if let (Some(le), Some(re)) = (elem_dari(&lv.ty), elem_dari(&rv.ty)) {
                if le != re {
                    return Err(super::galat::tandai(span, format!(
                        "tidak bisa menyambung array `{}` dengan `{}` — tipe elemennya beda",
                        le.display_name(), re.display_name()
                    )));
                }
                // Dua array berukuran tetap: panjang hasilnya diketahui saat
                // KOMPILASI, jadi hasilnya tetap `[N x T]` dan tidak perlu
                // menyentuh heap sama sekali.
                if let (JrgType::Array(_, ln), JrgType::Array(_, rn)) = (&lv.ty, &rv.ty) {
                    let (ln, rn) = (*ln, *rn);
                    return self.sambung_array(&lv, &rv, &le, ln, rn);
                }
                // Begitu salah satu sisinya slice, panjang hasilnya baru diketahui saat
                // JALAN — dan itulah yang membuat array bertumbuh mungkin: `a = a + [x]` …
                return self.sambung_slice(&lv, &rv, &le, span);
            }
        }

        // `nil` dibandingkan dengan apa pun.
        if matches!(op, BinaryOp::Eq | BinaryOp::Ne)
            && (lv.ty == JrgType::Nil || rv.ty == JrgType::Nil)
        {
            // Nilai bertag: `nil`-nya ada di tag, dibaca saat jalan (dulu selalu dijawab `false`).
            let lain = if lv.ty == JrgType::Nil { &rv } else { &lv };
            if lain.ty == JrgType::Any {
                let tag = self.tag_any(lain);
                let r = self.fresh("nilq.");
                let pred = if matches!(op, BinaryOp::Eq) { "eq" } else { "ne" };
                self.emit(format!("{} = icmp {} i32 {}, {}", r, pred, tag.text, super::codegen_any::TAG_NIL));
                return Ok(self.from_i1(&r));
            }
            let sama = lv.ty == JrgType::Nil && rv.ty == JrgType::Nil;
            let hasil = if matches!(op, BinaryOp::Eq) { sama } else { !sama };
            return Ok(Val {
                text: if hasil { "1" } else { "0" }.to_string(),
                ty: JrgType::Bool,
            });
        }

        // `()` lawan nilai bertipe lain: `==` lintas tipe menjawab `false`, seperti
        // acuan. Tanpa ini literal angka menyesuaikan diri ke `()` lewat `coerce`,
        // dan `kosong() == 6` dikompilasi jadi konstanta `true`.
        if matches!(op, BinaryOp::Eq | BinaryOp::Ne)
            && (lv.ty == JrgType::Unit) != (rv.ty == JrgType::Unit)
            && lv.ty != JrgType::Any && rv.ty != JrgType::Any
        {
            let hasil = matches!(op, BinaryOp::Ne);
            return Ok(Val {
                text: if hasil { "1" } else { "0" }.to_string(),
                ty: JrgType::Bool,
            });
        }

        // Array berukuran tetap dibandingkan dengan slice: yang dibandingkan ISINYA,
        // jadi keduanya diratakan jadi slice lebih dulu.
        let (lv, rv) = if matches!(op, BinaryOp::Eq | BinaryOp::Ne) {
            match (&lv.ty, &rv.ty) {
                (JrgType::Array(a, _), JrgType::Slice(b)) if a == b => {
                    let t = JrgType::Slice(a.clone());
                    let l = self.coerce_longgar(&lv, &t);
                    (l, rv)
                }
                (JrgType::Slice(a), JrgType::Array(b, _)) if a == b => {
                    let t = JrgType::Slice(a.clone());
                    let r = self.coerce_longgar(&rv, &t);
                    (lv, r)
                }
                _ => (lv, rv),
            }
        } else {
            (lv, rv)
        };

        // Salah satu sisi bertag: yang lain dikotak, lalu keduanya dibandingkan
        // lewat jalur kotak — yang membandingkan TAG dulu, baru isinya sesuai …
        let (lv, rv) = if lv.ty == JrgType::Any || rv.ty == JrgType::Any {
            let l = self.coerce(&lv, &JrgType::Any);
            let r = self.coerce(&rv, &JrgType::Any);
            (l, r)
        } else { (lv, rv) };

        // Dua array yang cuma beda PANJANG (atau array lawan slice) tetap terbanding: sebagai slice.
        let sebagai_slice = |t: &JrgType| match t {
            JrgType::Array(e, _) | JrgType::Slice(e) => Some(JrgType::Slice(Box::new(super::codegen_any::luruh(e)))),
            _ => None,
        };
        let (lv, rv) = match (sebagai_slice(&lv.ty), sebagai_slice(&rv.ty)) {
            (Some(a), Some(b)) if a == b && lv.ty != rv.ty => (self.coerce_longgar(&lv, &a), self.coerce_longgar(&rv, &b)),
            _ => (lv, rv),
        };

        // Dua bilangan bulat beda lebar atau tanda: dibandingkan NILAINYA, di lebar yang
        // memuat keduanya — acuan cuma punya satu bilangan bulat.
        let banding = matches!(op, BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge);
        if banding && lv.ty != rv.ty && lv.ty.is_int() && rv.ty.is_int() {
            let lebar = lv.ty.int_bits().unwrap_or(64).max(rv.ty.int_bits().unwrap_or(64)) * 2;
            let mut lebarkan = |v: &Val| {
                let r = self.fresh("lbr.");
                let cara = if v.ty.is_signed_int() { "sext" } else { "zext" };
                self.emit(format!("{} = {} {} {} to i{}", r, cara, v.ty.llvm_ty(), v.text, lebar));
                r
            };
            let (l, r) = (lebarkan(&lv), lebarkan(&rv));
            let pred = match op {
                BinaryOp::Eq => "eq", BinaryOp::Ne => "ne", BinaryOp::Lt => "slt",
                BinaryOp::Le => "sle", BinaryOp::Gt => "sgt", _ => "sge",
            };
            let h = self.fresh("ic.");
            self.emit(format!("{} = icmp {} i{} {}, {}", h, pred, lebar, l, r));
            return Ok(self.from_i1(&h));
        }

        // Aritmetika atas dua bilangan bulat beda lebar atau tanda: dikerjakan di tipe
        // terkecil yang memuat keduanya, dengan pemeriksaan luapan milik tipe itu.
        let (lv, rv) = match Self::int_bersama(&lv.ty, &rv.ty) {
            Some(t) if !banding && !matches!(op, BinaryOp::Shl | BinaryOp::Shr) =>
                (self.coerce_longgar(&lv, &t), self.coerce_longgar(&rv, &t)),
            _ => (lv, rv),
        };

        if let Some(v) = Self::banding_beda_golongan(op, &lv.ty, &rv.ty) { return Ok(v); }
        if lv.ty != rv.ty {
            // Kalimat BERSAMA, dan tipenya ditulis dengan nama BAHASA (`int`, bukan
            // `i64`).
            let (mut ki, mut ka) = (lv.ty.nama_typeof(), rv.ty.nama_typeof());
            // Dua lebar yang sama-sama `int` bagi `typeof`: sebut lebarnya, atau kalimatnya menipu.
            if ki == ka { ki = lv.ty.display_name(); ka = rv.ty.display_name(); }
            let pesan = match op {
                BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge =>
                    crate::interpreter::pesan_banding(&ki, &ka),
                _ => crate::interpreter::pesan_op_biner(
                        crate::interpreter::operator::simbol_biner(op), &ki, &ka),
            };
            return Err(crate::llvm::galat::tandai(span, pesan));
        }

        match op {
            BinaryOp::Pow => self.pow_binop(&lv, &rv, span),
            BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge =>
                self.cmp_binop(op, &lv, &rv, span),
            BinaryOp::Chan => Err("operator channel tidak didukung".into()),
            _ => self.arith_binop(op, &lv, &rv, span),
        }
    }

    fn codegen_short_circuit(&mut self, op: &BinaryOp, lhs: &Expr, rhs: &Expr, _span: Span) -> Result<Val, String> {
        let lv = self.codegen_expr(lhs, Some(&JrgType::Bool))?;
        let lv = self.coerce_longgar(&lv, &JrgType::Bool);
        let li1 = self.to_i1(&lv);
        let rhs_lbl = self.fresh_label("sc.rhs.");
        let skip_lbl = self.fresh_label("sc.skip.");
        let merge_lbl = self.fresh_label("sc.merge.");
        let slot = self.alloca_raw("sc.slot.", "i8", Some(1));
        if *op == BinaryOp::And {
            self.terminate(format!("br i1 {}, label %{}, label %{}", li1, rhs_lbl, skip_lbl));
        } else {
            self.terminate(format!("br i1 {}, label %{}, label %{}", li1, skip_lbl, rhs_lbl));
        }

        self.start_block(skip_lbl);
        self.emit(format!("store i8 {}, ptr {}", if *op == BinaryOp::And { "0" } else { "1" }, slot));
        self.terminate(format!("br label %{}", merge_lbl));

        self.start_block(rhs_lbl);
        let rv = self.codegen_expr(rhs, Some(&JrgType::Bool))?;
        let rv = self.coerce_longgar(&rv, &JrgType::Bool);
        if !self.is_terminated() {
            self.emit(format!("store i8 {}, ptr {}", rv.text, slot));
            self.terminate(format!("br label %{}", merge_lbl));
        }

        self.start_block(merge_lbl);
        Ok(self.load_place(&slot, &JrgType::Bool))
    }

    pub fn arith_binop(&mut self, op: &BinaryOp, lv: &Val, rv: &Val, span: Span) -> Result<Val, String> {
        // Salah satu sisinya bertag: golongannya dibaca saat jalan, lalu tiap cabang
        // memanggil BALIK fungsi ini dengan nilai yang sudah telanjang.
        if lv.ty == JrgType::Any || rv.ty == JrgType::Any {
            return self.arith_any(op, lv, rv, span);
        }
        // `"n=" + 5` dan `5 + "=n"` dua-duanya sah dan mengonversi sisi yang bukan
        // string.
        if lv.ty == JrgType::Str || rv.ty == JrgType::Str {
            if *op == BinaryOp::Add {
                let l = self.jadikan_string(lv, span)?;
                let r = self.jadikan_string(rv, span)?;
                return self.str_concat(&l, &r);
            }
            if let Some(v) = Self::banding_beda_golongan(op, &lv.ty, &rv.ty) { return Ok(v); }
            return Err(crate::llvm::galat::tandai(span, crate::interpreter::pesan_op_biner(
                crate::interpreter::operator::simbol_biner(op),
                &lv.ty.nama_typeof(), &rv.ty.nama_typeof())));
        }
        if lv.ty.is_float() {
            let instr = match op {
                BinaryOp::Add => "fadd", BinaryOp::Sub => "fsub", BinaryOp::Mul => "fmul",
                BinaryOp::Div => "fdiv", BinaryOp::Rem => "frem",
                _ => return Err(format!("operator `{:?}` tidak berlaku untuk float", op)),
            };
            let r = self.fresh("f.");
            self.emit(format!(
                "{} = {}{} {} {}, {}",
                r,
                instr,
                self.fast_math_flag(),
                lv.ty.llvm_ty(),
                lv.text,
                rv.text
            ));
            return Ok(Val { text: r, ty: lv.ty.clone() });
        }
        if !lv.ty.is_int() {
            return Err(format!("operator aritmetika/bitwise tidak berlaku untuk tipe `{}`", lv.ty));
        }
        if *op == BinaryOp::AndNot {
            let notr = self.fresh("not.");
            self.emit(format!("{} = xor {} {}, -1", notr, rv.ty.llvm_ty(), rv.text));
            let r = self.fresh("andnot.");
            self.emit(format!("{} = and {} {}, {}", r, lv.ty.llvm_ty(), lv.text, notr));
            return Ok(Val { text: r, ty: lv.ty.clone() });
        }
        let signed = lv.ty.is_signed_int();
        if matches!(op, BinaryOp::Div | BinaryOp::Rem) {
            self.emit_div_zero_check(lv, rv, *op == BinaryOp::Rem);
        }
        if matches!(op, BinaryOp::Shl | BinaryOp::Shr) {
            return Ok(self.geser_terperiksa(lv, rv, *op == BinaryOp::Shl));
        }
        let instr = match (op, signed) {
            (BinaryOp::Add, _) => "add", (BinaryOp::Sub, _) => "sub", (BinaryOp::Mul, _) => "mul",
            (BinaryOp::Div, true) => "sdiv", (BinaryOp::Div, false) => "udiv",
            (BinaryOp::Rem, true) => "srem", (BinaryOp::Rem, false) => "urem",
            (BinaryOp::BitAnd, _) => "and", (BinaryOp::BitOr, _) => "or", (BinaryOp::BitXor, _) => "xor",
            (BinaryOp::Shl, _) => "shl",
            (BinaryOp::Shr, true) => "ashr", (BinaryOp::Shr, false) => "lshr",
            _ => return Err(format!("operator `{:?}` tidak dikenal untuk integer", op)),
        };
        if matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul) {
            return Ok(self.hitung_terperiksa(op, lv, rv));
        }
        let r = self.fresh("i.");
        self.emit(format!("{} = {} {} {}, {}", r, instr, lv.ty.llvm_ty(), lv.text, rv.text));
        Ok(Val { text: r, ty: lv.ty.clone() })
    }

    /// `+`/`-`/`*` bilangan bulat dengan luapan DIPERIKSA (Fase LEBAR-LLVM): dulu membungkus
    /// diam-diam di lebar tipenya (`9223372036854775807 + 1` jadi negatif), sementara dua mesin
    /// lain menghitung di 128 bit. Yang sengaja membungkus menulis `math::wrap_*`.
    pub(crate) fn hitung_terperiksa(&mut self, op: &BinaryOp, lv: &Val, rv: &Val) -> Val {
        let t = lv.ty.llvm_ty();
        let bertanda = lv.ty.is_signed_int();
        let (nama, kode) = match op {
            BinaryOp::Add => ("add", 0), BinaryOp::Sub => ("sub", 1), _ => ("mul", 2),
        };
        let intr = format!("llvm.{}{}.with.overflow.{}", if bertanda { "s" } else { "u" }, nama, t);
        self.ensure_extern(&format!("declare {{ {}, i1 }} @{}({}, {})", t, intr, t, t), &intr);
        let p = self.fresh("cek.");
        self.emit(format!("{} = call {{ {}, i1 }} @{}({} {}, {} {})", p, t, intr, t, lv.text, t, rv.text));
        let r = self.fresh("i.");
        self.emit(format!("{} = extractvalue {{ {}, i1 }} {}, 0", r, t, p));
        let o = self.fresh("luber.");
        self.emit(format!("{} = extractvalue {{ {}, i1 }} {}, 1", o, t, p));
        let bad = self.fresh_label("luber.");
        let ok = self.fresh_label("muat.");
        self.terminate_unlikely_true(&o, &bad, &ok);
        self.start_block(bad);
        self.ensure_extern(
            "declare void @jrg_panic_luber(i64, i64, i32, i32, i32) cold nounwind noreturn",
            "jrg_panic_luber",
        );
        let (a, b) = (self.ke_i64_tampil(lv), self.ke_i64_tampil(rv));
        self.emit(format!("call void @jrg_panic_luber(i64 {}, i64 {}, i32 {}, i32 {}, i32 {})",
            a, b, kode, lv.ty.int_bits().unwrap_or(64), if bertanda { 1 } else { 0 }));
        self.terminate("unreachable".to_string());
        self.start_block(ok);
        Val { text: r, ty: lv.ty.clone() }
    }

    /// Perlebar bilangan bulat ke `i64` untuk dicetak di kalimat galat.
    fn ke_i64_tampil(&mut self, v: &Val) -> String {
        let t = v.ty.llvm_ty();
        if t == "i64" { return v.text.clone(); }
        let r = self.fresh("lebar.");
        let cara = if v.ty.int_bits().unwrap_or(64) > 64 { "trunc" }
            else if v.ty.is_signed_int() { "sext" } else { "zext" };
        self.emit(format!("{} = {} {} {} to i64", r, cara, t, v.text));
        r
    }

    /// `<<`/`>>` dengan jumlah geseran DIPERIKSA. Geseran sebesar lebar tipenya atau lebih
    /// adalah poison di LLVM: `1 << 70` dulu menjawab `1` dan `8 >> -1` menjawab `1`.
    fn geser_terperiksa(&mut self, lv: &Val, rv: &Val, ke_kiri: bool) -> Val {
        let t = lv.ty.llvm_ty();
        let bit = lv.ty.int_bits().unwrap_or(64);
        let signed = lv.ty.is_signed_int();
        let y = self.coerce_longgar(rv, &lv.ty);
        let (neg, lebar, besar) = (self.fresh("gs.neg."), self.fresh("gs.lbr."), self.fresh("gs.bsr."));
        self.emit(format!("{} = icmp slt {} {}, 0", neg, t, y.text));
        self.emit(format!("{} = icmp uge {} {}, {}", lebar, t, y.text, bit));
        // Tipe 8 bit tidak bisa memuat 128, jadi di sana "128 atau lebih" tidak pernah terjadi.
        if bit > 8 {
            self.emit(format!("{} = icmp sge {} {}, 128", besar, t, y.text));
        } else {
            self.emit(format!("{} = or i1 false, false", besar));
        }
        let mut buruk = self.fresh("gs.brk.");
        self.emit(format!("{} = or i1 {}, {}", buruk, neg, besar));
        if ke_kiri {
            // Ke kiri melewati lebarnya cuma sah kalau tidak ada bit yang dibuang.
            let (ada, luber, b2) = (self.fresh("gs.ada."), self.fresh("gs.lbh."), self.fresh("gs.brk."));
            self.emit(format!("{} = icmp ne {} {}, 0", ada, t, lv.text));
            self.emit(format!("{} = and i1 {}, {}", luber, lebar, ada));
            self.emit(format!("{} = or i1 {}, {}", b2, buruk, luber));
            buruk = b2;
        }
        let bad = self.fresh_label("geser.salah.");
        let ok = self.fresh_label("geser.ok.");
        self.terminate_unlikely_true(&buruk, &bad, &ok);
        self.start_block(bad);
        self.ensure_extern(
            "declare void @jrg_panic_geser(i64, i64, i32, i32) cold nounwind noreturn",
            "jrg_panic_geser",
        );
        let (x64, y64) = (self.ke_i64_tampil(lv), self.ke_i64_tampil(&y));
        self.emit(format!("call void @jrg_panic_geser(i64 {}, i64 {}, i32 {}, i32 {})",
            x64, y64, if ke_kiri { 1 } else { 0 }, bit));
        self.terminate("unreachable".to_string());
        self.start_block(ok);
        // Jumlah yang sah tapi >= lebar: dijepit ke lebar-1, lalu hasilnya dibetulkan.
        let (jepit, mentah) = (self.fresh("gs.jpt."), self.fresh("gs.mth."));
        self.emit(format!("{} = select i1 {}, {} {}, {} {}", jepit, lebar, t, bit - 1, t, y.text));
        let instr = if ke_kiri { "shl" } else if signed { "ashr" } else { "lshr" };
        self.emit(format!("{} = {} {} {}, {}", mentah, instr, t, lv.text, jepit));
        // `ashr` yang dijepit sudah benar (semua bit jadi bit tanda); dua lainnya jadi nol.
        if !ke_kiri && signed { return Val { text: mentah, ty: lv.ty.clone() }; }
        let r = self.fresh("i.");
        self.emit(format!("{} = select i1 {}, {} 0, {} {}", r, lebar, t, t, mentah));
        Val { text: r, ty: lv.ty.clone() }
    }

    fn emit_div_zero_check(&mut self, lv: &Val, rv: &Val, sisa: bool) {
        let cmp = self.fresh("dzchk.");
        self.emit(format!("{} = icmp eq {} {}, 0", cmp, rv.ty.llvm_ty(), rv.text));
        let bad = self.fresh_label("divzero.");
        let ok = self.fresh_label("divok.");
        self.terminate_unlikely_true(&cmp, &bad, &ok);
        self.start_block(bad);
        self.ensure_extern(
            "declare void @jrg_panic_div_by_zero(i64, i32) cold nounwind noreturn",
            "jrg_panic_div_by_zero",
        );
        let kiri = self.ke_i64_tampil(lv);
        self.emit(format!("call void @jrg_panic_div_by_zero(i64 {}, i32 {})", kiri, if sisa { 1 } else { 0 }));
        self.terminate("unreachable".to_string());
        self.start_block(ok);
    }

    /// `x ** k` dengan `x` bulat dan `k` negatif tertulis: `powi` seperti `pangkat_bulat` acuan
    /// (eksponen di luar `i32` lewat `pow`), jadi angkanya sama sampai bit terakhir.
    fn pangkat_pecahan(&mut self, lv: &Val, k: f64) -> Val {
        let x = self.coerce_longgar(lv, &JrgType::F64);
        let r = self.fresh("powi.");
        if k >= i32::MIN as f64 {
            self.ensure_extern("declare double @llvm.powi.f64.i32(double, i32)", "llvm.powi.f64.i32");
            self.emit(format!("{} = call double @llvm.powi.f64.i32(double {}, i32 {})", r, x.text, k as i32));
        } else {
            self.ensure_extern("declare double @llvm.pow.f64(double, double)", "llvm.pow.f64");
            self.emit(format!("{} = call double @llvm.pow.f64(double {}, double 0x{:016X})", r, x.text, k.to_bits()));
        }
        Val { text: r, ty: JrgType::F64 }
    }

    pub fn pow_binop(&mut self, lv: &Val, rv: &Val, _span: Span) -> Result<Val, String> {
        if lv.ty.is_float() {
            self.ensure_extern(
                "declare double @llvm.pow.f64(double, double)",
                "llvm.pow.f64",
            );
            let lv64 = self.coerce_longgar(lv, &JrgType::F64);
            let rv64 = self.coerce_longgar(rv, &JrgType::F64);
            let r = self.fresh("pow.");
            self.emit(format!(
                "{} = call{} double @llvm.pow.f64(double {}, double {})",
                r,
                self.fast_math_flag(),
                lv64.text,
                rv64.text
            ));
            let out = Val { text: r, ty: JrgType::F64 };
            return Ok(self.coerce_longgar(&out, &lv.ty));
        }
        if lv.ty.is_int() {
            self.ensure_extern("declare i64 @jrg_ipow_i64(i64, i64)", "jrg_ipow_i64");
            let l64 = self.coerce_longgar(lv, &JrgType::I64);
            let r64 = self.coerce_longgar(rv, &JrgType::I64);
            let r = self.fresh("ipow.");
            self.emit(format!("{} = call i64 @jrg_ipow_i64(i64 {}, i64 {})", r, l64.text, r64.text));
            let out = Val { text: r, ty: JrgType::I64 };
            return Ok(self.coerce_longgar(&out, &lv.ty));
        }
        Err(format!("operator `**` tidak berlaku untuk tipe `{}`", lv.ty))
    }

    /// Tipe bulat terkecil yang memuat semua nilai `a` dan `b` yang BERBEDA tipe; `None`
    /// kalau sama, bukan bilangan bulat, atau butuh lebih dari 128 bit.
    fn int_bersama(a: &JrgType, b: &JrgType) -> Option<JrgType> {
        if a == b || !a.is_int() || !b.is_int() { return None; }
        let (ba, bb) = (a.int_bits()?, b.int_bits()?);
        if a.is_signed_int() == b.is_signed_int() {
            return Some(if ba >= bb { a.clone() } else { b.clone() });
        }
        let (bt, btt) = if a.is_signed_int() { (ba, bb) } else { (bb, ba) };
        Some(match bt.max(btt * 2) {
            8 => JrgType::I8, 16 => JrgType::I16, 32 => JrgType::I32,
            64 => JrgType::I64, 128 => JrgType::I128, _ => return None,
        })
    }

    pub fn cmp_binop_pub_entry(&mut self, op: &BinaryOp, lv: &Val, rv: &Val, span: Span) -> Result<Val, String> {
        self.cmp_binop(op, lv, rv, span)
    }

    fn cmp_binop(&mut self, op: &BinaryOp, lv: &Val, rv: &Val, _span: Span) -> Result<Val, String> {
        if lv.ty == JrgType::Str {
            return self.str_cmp(op, lv, rv);
        }
        if lv.ty.is_float() {
            let pred = match op {
                BinaryOp::Eq => "oeq", BinaryOp::Ne => "une", BinaryOp::Lt => "olt",
                BinaryOp::Le => "ole", BinaryOp::Gt => "ogt", BinaryOp::Ge => "oge",
                _ => unreachable!(),
            };
            let r = self.fresh("fc.");
            self.emit(format!(
                "{} = fcmp{} {} {} {}, {}",
                r,
                self.fast_math_flag(),
                pred,
                lv.ty.llvm_ty(),
                lv.text,
                rv.text
            ));
            return Ok(self.from_i1(&r));
        }
        if lv.ty == JrgType::Bool {
            let pred = match op { BinaryOp::Eq => "eq", BinaryOp::Ne => "ne", _ => return Err("bool hanya mendukung `==`/`!=`".into()) };
            let r = self.fresh("bc.");
            self.emit(format!("{} = icmp {} i8 {}, {}", r, pred, lv.text, rv.text));
            return Ok(self.from_i1(&r));
        }
        // `rune` ikut jalur ini: perbandingan codepoint, tak bertanda.
        // Diukur: `'a' < 'b'` bernilai true, `'a' == 'a'` true.
        if lv.ty.berbasis_int() {
            let signed = lv.ty.is_signed_int();
            let pred = match (op, signed) {
                (BinaryOp::Eq, _) => "eq", (BinaryOp::Ne, _) => "ne",
                (BinaryOp::Lt, true) => "slt", (BinaryOp::Lt, false) => "ult",
                (BinaryOp::Le, true) => "sle", (BinaryOp::Le, false) => "ule",
                (BinaryOp::Gt, true) => "sgt", (BinaryOp::Gt, false) => "ugt",
                (BinaryOp::Ge, true) => "sge", (BinaryOp::Ge, false) => "uge",
                _ => unreachable!(),
            };
            let r = self.fresh("ic.");
            self.emit(format!("{} = icmp {} {} {}, {}", r, pred, lv.ty.llvm_ty(), lv.text, rv.text));
            return Ok(self.from_i1(&r));
        }
        // Pegangan (`Atomic`, `Mutex`, channel) sama kalau menunjuk keadaan yang sama.
        if matches!(lv.ty, JrgType::Sinkron(_) | JrgType::Chan(_)) {
            let pred = match op {
                BinaryOp::Eq => "eq", BinaryOp::Ne => "ne",
                _ => return Err(super::galat::tandai(_span, crate::interpreter::pesan_banding(
                    &lv.ty.nama_typeof(), &rv.ty.nama_typeof()))),
            };
            let r = self.fresh("pc.");
            self.emit(format!("{} = icmp {} ptr {}, {}", r, pred, lv.text, rv.text));
            return Ok(self.from_i1(&r));
        }
        if Self::butuh_eq_struktural(&lv.ty) {
            // Agregat dibandingkan komponen demi komponen — lihat
            // `codegen_eq.rs`. Ditaruh di ujung, sesudah semua tipe skalar,
            // supaya jalur terpanas (`icmp` int) tidak kena cabang tambahan.
            return self.agregat_cmp(op, lv, rv, _span);
        }
        // Dua nilai yang sama-sama bisa dipanggil: DITOLAK, dengan kalimat
        // yang sama persis dengan dua mesin acuan — satu definisi untuk tiga
        // mesin, bukan tiga kalimat untuk satu kesalahan.
        if matches!(lv.ty, JrgType::Fn(..)) && matches!(rv.ty, JrgType::Fn(..)) {
            return Err(super::galat::tandai(_span, crate::interpreter::pesan_banding(
                &lv.ty.nama_typeof(), &rv.ty.nama_typeof())));
        }
        // `{:?}` di sini dulu mencetak NAMA VARIAN Rust (`Eq`) kepada orang
        // yang menulis `==` — kelas Fase EJAAN, di pintu yang belum tersapu.
        Err(format!("perbandingan `{}` tidak didukung untuk tipe `{}`",
                    crate::interpreter::operator::simbol_biner(op), lv.ty))
    }

    /// Ubah sebuah nilai jadi `string` untuk keperluan konkatenasi.
    fn jadikan_string(&mut self, v: &Val, span: Span) -> Result<Val, String> {
        if v.ty == JrgType::Str { return Ok(v.clone()); }
        // Agregat dan tipe pemakai lewat pintu TAMPILAN (hook `to_string` ikut), seperti `wok`.
        if matches!(v.ty, JrgType::Struct(_) | JrgType::Enum(_) | JrgType::Array(..)
            | JrgType::Slice(_) | JrgType::Tuple(_) | JrgType::Dict(_) | JrgType::DictCampur(_)) {
            return self.build_display_str(v);
        }
        self.do_cast(v, &JrgType::Str, span)
    }

    /// `str * n` — pengulangan string.
    pub(crate) fn ulang_string(&mut self, sv: &Val, nv: &Val) -> Result<Val, String> {
        self.ensure_extern(
            "declare void @jrg_str_repeat(ptr, i64, i64, ptr, ptr)", "jrg_str_repeat");
        self.ensure_extern(
            "declare void @jrg_panic(ptr, i64) cold nounwind noreturn", "jrg_panic");

        let n64 = self.coerce_longgar(nv, &JrgType::I64);
        let neg = self.fresh("rpneg.");
        self.emit(format!("{} = icmp slt i64 {}, 0", neg, n64.text));
        let badlbl = self.fresh_label("rpbad.");
        let oklbl = self.fresh_label("rpok.");
        self.terminate_unlikely_true(&neg, &badlbl, &oklbl);
        self.start_block(badlbl);
        let msg = self.intern_string(
            "tidak bisa mengulang string dengan jumlah negatif — jumlah pengulangan tidak boleh negatif");
        let (mp, ml) = self.split_str(&msg);
        self.emit(format!("call void @jrg_panic(ptr {}, i64 {})", mp, ml));
        self.terminate("unreachable".to_string());
        self.start_block(oklbl);

        let (sptr, slen) = self.split_str(sv);
        let outp = self.alloca_raw("rpp.", "ptr", None);
        let outl = self.alloca_raw("rpl.", "i64", None);
        self.emit(format!(
            "call void @jrg_str_repeat(ptr {}, i64 {}, i64 {}, ptr {}, ptr {})",
            sptr, slen, n64.text, outp, outl));
        self.join_str(&outp, &outl)
    }

    /// `[a..] + [b..]` — array baru berisi keduanya, berurutan.
    fn sambung_array(&mut self, lv: &Val, rv: &Val, elem: &JrgType, ln: u64, rn: u64) -> Result<Val, String> {
        let hasil_ty = JrgType::Array(Box::new(elem.clone()), ln + rn);
        let slot = self.alloca_typed("cat.", &hasil_ty);

        let lsrc = self.spill_to_place(lv);
        let rsrc = self.spill_to_place(rv);
        let ukuran_elem = self.types.sizeof(elem);

        self.ensure_extern(
            "declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)",
            "llvm.memcpy.p0.p0.i64",
        );
        // Sisi kiri ditulis di awal.
        self.emit(format!(
            "call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)",
            slot, lsrc, ukuran_elem * ln
        ));
        // Sisi kanan tepat sesudahnya.
        let ofs = self.fresh("catr.");
        self.emit(format!(
            "{} = getelementptr {}, ptr {}, i64 0, i64 {}",
            ofs, hasil_ty.llvm_ty(), slot, ln
        ));
        self.emit(format!(
            "call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)",
            ofs, rsrc, ukuran_elem * rn
        ));
        Ok(self.load_place(&slot, &hasil_ty))
    }

    /// Sambung dua koleksi ketika panjang hasilnya baru diketahui saat jalan.
    fn sambung_slice(&mut self, lv: &Val, rv: &Val, elem: &JrgType, span: Span)
        -> Result<Val, String>
    {
        let _ = span;
        let ukuran = self.types.sizeof(elem);
        let align = self.types.alignof(elem);
        self.ensure_extern("declare ptr @jrg_alloc(i64, i64)", "jrg_alloc");
        self.ensure_extern(
            "declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)",
            "llvm.memcpy.p0.p0.i64");

        let (lp, ln) = self.bagian_koleksi_impl(lv, elem);
        let (rp, rn) = self.bagian_koleksi_impl(rv, elem);

        let total = self.fresh("ctn.");
        self.emit(format!("{} = add i64 {}, {}", total, ln, rn));
        let byte = self.fresh("ctb.");
        self.emit(format!("{} = mul i64 {}, {}", byte, total, ukuran));
        let buf = self.fresh("ctp.");
        self.emit(format!("{} = call ptr @jrg_alloc(i64 {}, i64 {})", buf, byte, align));

        let lbyte = self.fresh("ctlb.");
        self.emit(format!("{} = mul i64 {}, {}", lbyte, ln, ukuran));
        self.emit(format!(
            "call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)",
            buf, lp, lbyte));
        let ujung = self.fresh("ctu.");
        self.emit(format!("{} = getelementptr i8, ptr {}, i64 {}", ujung, buf, lbyte));
        let rbyte = self.fresh("ctrb.");
        self.emit(format!("{} = mul i64 {}, {}", rbyte, rn, ukuran));
        self.emit(format!(
            "call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)",
            ujung, rp, rbyte));

        let s1 = self.fresh("ctv.");
        self.emit(format!("{} = insertvalue %JrgSlice undef, ptr {}, 0", s1, buf));
        let s2 = self.fresh("ctv.");
        self.emit(format!("{} = insertvalue %JrgSlice {}, i64 {}, 1", s2, s1, total));
        Ok(Val { text: s2, ty: JrgType::Slice(Box::new(elem.clone())) })
    }

    /// (penunjuk data, panjang) dari sebuah array tetap atau slice.
    pub(crate) fn bagian_koleksi_impl(&mut self, v: &Val, _elem: &JrgType) -> (String, String) {
        match &v.ty {
            JrgType::Slice(_) => {
                let p = self.fresh("bkp.");
                self.emit(format!("{} = extractvalue %JrgSlice {}, 0", p, v.text));
                let n = self.fresh("bkn.");
                self.emit(format!("{} = extractvalue %JrgSlice {}, 1", n, v.text));
                (p, n)
            }
            JrgType::Array(_, n) => {
                let slot = self.spill_to_place(v);
                let p = self.fresh("bkp.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i64 0, i64 0",
                                  p, v.ty.llvm_ty(), slot));
                (p, n.to_string())
            }
            _ => (v.text.clone(), "0".to_string()),
        }
    }

    /// Template literal `«teks ${ekspresi} teks»`.
    fn codegen_template(&mut self, parts: &[TemplatePart], span: Span) -> Result<Val, String> {
        let mut hasil: Option<Val> = None;
        for part in parts {
            let potongan = match part {
                TemplatePart::Text(t) => self.intern_string(t),
                TemplatePart::Expr(e) => {
                    let v = self.codegen_expr(e, None)?;
                    self.val_to_str(&v)?
                }
            };
            hasil = Some(match hasil {
                None => potongan,
                Some(a) => self.str_concat(&a, &potongan)?,
            });
        }
        let _ = span;
        Ok(hasil.unwrap_or_else(|| self.intern_string("")))
    }

    /// Jadikan sebuah nilai MILIK SENDIRI sebelum ia diikat ke nama baru.
    pub fn ambil_milik(&mut self, e: &Expr, v: Val) -> Result<Val, String> {
        // Bukan cuma tipe yang SENDIRI sebuah penunjuk: sebuah struct berisi slice
        // juga harus disalin, dan versi pertama pemeriksaan ini cuma melihat lapis …
        if !Self::punya_penunjuk(&v.ty) && !self.isi_dalam_perlu_salin(&v.ty) {
            return Ok(v);
        }
        // Nilai yang baru dibuat memang segar DI TINGKAT LUARNYA, tapi tidak
        // otomatis di dalamnya: `Kotak { isi: s }` adalah struct baru yang memuat …
        if Self::pasti_segar(e) && !self.isi_dalam_perlu_salin(&v.ty) && !self.hasil_menumpang(e) {
            return Ok(v);
        }
        Ok(self.salin_dalam(&v))
    }

    /// Apakah tipe ini SENDIRI sebuah penunjuk ke buffer yang bisa dibagi?
    pub fn punya_penunjuk(t: &JrgType) -> bool {
        matches!(t, JrgType::Slice(_) | JrgType::Dict(_))
    }

    /// Apakah ada penunjuk semacam itu DI DALAMNYA?
    pub fn isi_dalam_perlu_salin(&self, t: &JrgType) -> bool {
        match t {
            JrgType::Slice(e) | JrgType::Dict(e) => self.butuh_salin(e),
            JrgType::Array(e, n) => *n > 0 && self.butuh_salin(e),
            _ => self.butuh_salin(t),
        }
    }

    /// `==`/`!=` dua skalar dari golongan BERBEDA (teks, rune, bool, angka):
    /// tidak pernah sama, seperti di dua mesin lain. `None` untuk pasangan lain.
    fn banding_beda_golongan(op: &BinaryOp, l: &JrgType, r: &JrgType) -> Option<Val> {
        if !matches!(op, BinaryOp::Eq | BinaryOp::Ne) { return None; }
        let golongan = |t: &JrgType| match t {
            JrgType::Str => Some(0),
            JrgType::Rune => Some(1),
            JrgType::Bool => Some(2),
            t if t.is_int() || t.is_float() => Some(3),
            JrgType::Sinkron(k) => Some(4 + *k as i32),
            JrgType::Chan(_) => Some(6),
            _ => None,
        };
        let (a, b) = (golongan(l)?, golongan(r)?);
        if a == b { return None; }
        Some(Val { text: if matches!(op, BinaryOp::Ne) { "1" } else { "0" }.to_string(), ty: JrgType::Bool })
    }

    /// Apakah nilai bertipe ini memegang penyangga yang bisa terbagi — menembus tuple,
    /// struct (termasuk yang bersarang) dan payload enum (`Option<[]int>`).
    pub fn butuh_salin(&self, t: &JrgType) -> bool {
        self.butuh_salin_dalam(t, 0)
    }

    fn butuh_salin_dalam(&self, t: &JrgType, dalam: u8) -> bool {
        if dalam > 12 { return false; }
        match t {
            JrgType::Slice(_) | JrgType::Dict(_) | JrgType::DictCampur(_) | JrgType::Any => true,
            JrgType::Array(e, n) => *n > 0 && self.butuh_salin_dalam(e, dalam + 1),
            JrgType::Tuple(ts) => ts.iter().any(|x| self.butuh_salin_dalam(x, dalam + 1)),
            JrgType::Struct(nama) => self.types.structs.get(nama)
                .map_or(false, |l| l.fields.iter().any(|f| self.butuh_salin_dalam(&f.1, dalam + 1))),
            JrgType::Enum(nama) => self.types.enums.get(nama)
                .map_or(false, |l| l.variants.iter().any(|v|
                    v.payload.field_types().iter().any(|x| self.butuh_salin_dalam(x, dalam + 1)))),
            _ => false,
        }
    }

    pub fn isi_perlu_salin(t: &JrgType) -> bool {
        match t {
            JrgType::Slice(_) | JrgType::Dict(_) => true,
            JrgType::Array(e, n) => *n > 0 && Self::isi_perlu_salin(e),
            JrgType::Tuple(ts) => ts.iter().any(Self::isi_perlu_salin),
            _ => false,
        }
    }

    /// Salin sebuah nilai sampai ke dalam, sehingga hasilnya tidak berbagi satu
    /// buffer pun dengan aslinya.
    pub fn salin_dalam(&mut self, v: &Val) -> Val {
        match &v.ty {
            JrgType::Dict(_) | JrgType::DictCampur(_) => {
                // Nilai di dalam dict ikut, lewat jalur yang sama.
                self.salin_dict(v)
            }
            // Kotak: array/dict di dalamnya baru diketahui saat jalan, jadi disalin primitif C.
            JrgType::Any => {
                let tempat = self.spill_to_place(v);
                self.ensure_extern("declare void @jrg_any_salin(ptr)", "jrg_any_salin");
                self.emit(format!("call void @jrg_any_salin(ptr {})", tempat));
                self.load_place(&tempat, &JrgType::Any)
            }
            JrgType::Slice(e) => {
                let luar = self.salin_slice(v);
                if !self.butuh_salin(e) { return luar; }
                let e = (**e).clone();
                let (data, n) = self.bagian_slice(&luar);
                let _ = self.salin_tiap(&data, &n, &e);
                luar
            }
            JrgType::Array(e, n) if *n > 0 && self.butuh_salin(e) => {
                let e = (**e).clone();
                let tempat = self.spill_to_place(v);
                let p = self.fresh("sdp.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i64 0, i64 0",
                    p, v.ty.llvm_ty(), tempat));
                self.salin_tiap(&p, &n.to_string(), &e);
                self.load_place(&tempat, &v.ty)
            }
            JrgType::Tuple(ts) if self.butuh_salin(&v.ty) => {
                let ts = ts.clone();
                let tempat = self.spill_to_place(v);
                for (i, t) in ts.iter().enumerate() {
                    if !self.butuh_salin(t) { continue; }
                    let fp = self.fresh("sdt.");
                    self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}",
                        fp, v.ty.llvm_ty(), tempat, i));
                    let fv = self.load_place(&fp, t);
                    let baru = self.salin_dalam(&fv);
                    self.emit(format!("store {} {}, ptr {}", t.llvm_ty(), baru.text, fp));
                }
                self.load_place(&tempat, &v.ty)
            }
            JrgType::Struct(nama) => {
                let Some(layout) = self.types.structs.get(nama).cloned() else { return v.clone() };
                if !self.butuh_salin(&v.ty) {
                    return v.clone();
                }
                let tempat = self.spill_to_place(v);
                for (i, f) in layout.fields.iter().enumerate() {
                    if !self.butuh_salin(&f.1) { continue; }
                    let fp = self.fresh("sds.");
                    self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}",
                        fp, v.ty.llvm_ty(), tempat, i));
                    let fv = self.load_place(&fp, &f.1);
                    let baru = self.salin_dalam(&fv);
                    self.emit(format!("store {} {}, ptr {}", f.1.llvm_ty(), baru.text, fp));
                }
                self.load_place(&tempat, &v.ty)
            }
            // Enum: cuma payload varian yang SEDANG dipegang yang disalin, dipilih dari tagnya.
            JrgType::Enum(nama) if self.butuh_salin(&v.ty) => {
                let Some(layout) = self.types.enums.get(nama).cloned() else { return v.clone() };
                let tempat = self.spill_to_place(v);
                let tp = self.fresh("sde.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 0", tp, v.ty.llvm_ty(), tempat));
                let tag = self.fresh("sdet.");
                self.emit(format!("{} = load i32, ptr {}", tag, tp));
                let perlu: Vec<_> = layout.variants.iter()
                    .filter(|x| x.payload.field_types().iter().any(|t| self.butuh_salin(t))).cloned().collect();
                let usai = self.fresh_label("sde.usai.");
                let label: Vec<String> = perlu.iter().map(|_| self.fresh_label("sde.var.")).collect();
                let kasus: Vec<String> = perlu.iter().zip(&label)
                    .map(|(x, l)| format!("i32 {}, label %{}", x.tag, l)).collect();
                self.terminate(format!("switch i32 {}, label %{} [ {} ]", tag, usai, kasus.join(" ")));
                for (x, l) in perlu.iter().zip(&label) {
                    self.start_block(l.clone());
                    let ps = x.payload.as_llvm_struct();
                    let blob = self.fresh("sdeb.");
                    self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", blob, v.ty.llvm_ty(), tempat));
                    for (i, t) in x.payload.field_types().iter().enumerate() {
                        if !self.butuh_salin(t) { continue; }
                        let fp = self.fresh("sdef.");
                        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", fp, ps, blob, i));
                        let fv = self.load_place(&fp, t);
                        let baru = self.salin_dalam(&fv);
                        self.emit(format!("store {} {}, ptr {}", t.llvm_ty(), baru.text, fp));
                    }
                    if !self.is_terminated() { self.terminate(format!("br label %{}", usai)); }
                }
                self.start_block(usai);
                self.load_place(&tempat, &v.ty)
            }
            _ => v.clone(),
        }
    }

    /// Salin-dalam setiap elemen buffer `ptr` sepanjang `n`, di tempat.
    fn salin_tiap(&mut self, ptr: &str, n: &str, elem: &JrgType) {
        let i = self.alloca_raw("sti.", "i64", Some(8));
        self.emit(format!("store i64 0, ptr {}", i));
        let (cek, badan, usai) = (self.fresh_label("st.cek."), self.fresh_label("st.badan."),
                                  self.fresh_label("st.usai."));
        self.terminate(format!("br label %{}", cek));
        self.start_block(cek.clone());
        let iv = self.load_place(&i, &JrgType::I64);
        let c = self.fresh("stc.");
        self.emit(format!("{} = icmp slt i64 {}, {}", c, iv.text, n));
        self.terminate(format!("br i1 {}, label %{}, label %{}", c, badan, usai));
        self.start_block(badan);
        let ep = self.fresh("stp.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", ep, elem.llvm_ty(), ptr, iv.text));
        let ev = self.load_place(&ep, elem);
        let baru = self.salin_dalam(&ev);
        self.emit(format!("store {} {}, ptr {}", elem.llvm_ty(), baru.text, ep));
        let iv2 = self.load_place(&i, &JrgType::I64);
        let inc = self.fresh("stn.");
        self.emit(format!("{} = add i64 {}, 1", inc, iv2.text));
        self.emit(format!("store i64 {}, ptr {}", inc, i));
        if !self.is_terminated() { self.terminate(format!("br label %{}", cek)); }
        self.start_block(usai);
    }

    /// `(data, panjang)` sebuah slice.
    pub fn bagian_slice(&mut self, v: &Val) -> (String, String) {
        let p = self.fresh("bsp.");
        self.emit(format!("{} = extractvalue %JrgSlice {}, 0", p, v.text));
        let n = self.fresh("bsn.");
        self.emit(format!("{} = extractvalue %JrgSlice {}, 1", n, v.text));
        (p, n)
    }

    /// Panggilan bawaan yang hasilnya bisa berupa ISI salah satu argumennya (`ys.first()`,
    /// `o.unwrap()`, `arr::get_or(ys, i, d)`): ada argumen yang memuat penyangga di dalamnya.
    /// Fungsi milik program selalu menjawab nilai miliknya sendiri.
    fn hasil_menumpang(&self, e: &Expr) -> bool {
        let mut lokal = super::infer::Lokal::new();
        for lapis in &self.locals {
            for (n, (_, t)) in lapis { lokal.insert(n.clone(), t.clone()); }
        }
        let memuat = |x: &Expr| match self.tipe_ekspresi(x, &lokal, &self.cur_fn_generics, "") {
            Some(t) => self.isi_dalam_perlu_salin(&t),
            None => !matches!(x, Expr::Literal(..) | Expr::Closure { .. }),
        };
        match e {
            Expr::Call { func, args, .. } => {
                let milik_program = match &**func {
                    Expr::Ident(n, _) => self.lookup_local(n).is_none()
                        && self.tables.functions.contains_key(&self.k(n)),
                    Expr::PathExpr { segments, .. } => matches!(segments.as_slice(),
                        [ns, _] if self.unit_alias(ns).is_some()),
                    _ => false,
                };
                !milik_program && args.iter().any(memuat)
            }
            Expr::Method { obj, method, args, .. } => {
                let milik_program = match self.tipe_ekspresi(obj, &lokal, &self.cur_fn_generics, "") {
                    Some(JrgType::Struct(n)) | Some(JrgType::Enum(n)) =>
                        self.tables.find_method(&n, method).is_some(),
                    _ => false,
                };
                !milik_program && (memuat(obj) || args.iter().any(memuat))
            }
            _ => false,
        }
    }

    /// Apakah ekspresi ini PASTI menghasilkan buffer yang belum dipegang siapa
    /// pun?
    fn pasti_segar(e: &Expr) -> bool {
        match e {
            // Hasil pemanggilan, potongan, penyambungan, dan literal
            // semuanya buffer baru — masing-masing dialokasikan di tempat
            // pembuatannya.
            Expr::Call { .. } | Expr::Method { .. } | Expr::Slice { .. }
            | Expr::Array { .. } | Expr::ArrayRepeat { .. } => true,
            Expr::Binary { op: BinaryOp::Add, .. } => true,
            _ => false,
        }
    }

    /// Salin isi sebuah slice ke buffer baru.
    pub fn salin_slice(&mut self, v: &Val) -> Val {
        let JrgType::Slice(elem) = &v.ty else { return v.clone() };
        let elem = (**elem).clone();
        let ukuran = self.types.sizeof(&elem);
        let align = self.types.alignof(&elem);
        self.ensure_extern("declare ptr @jrg_alloc(i64, i64)", "jrg_alloc");
        self.ensure_extern(
            "declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)",
            "llvm.memcpy.p0.p0.i64");

        let p = self.fresh("cpp.");
        self.emit(format!("{} = extractvalue %JrgSlice {}, 0", p, v.text));
        let n = self.fresh("cpn.");
        self.emit(format!("{} = extractvalue %JrgSlice {}, 1", n, v.text));
        let byte = self.fresh("cpb.");
        self.emit(format!("{} = mul i64 {}, {}", byte, n, ukuran));
        let buf = self.fresh("cpbuf.");
        self.emit(format!("{} = call ptr @jrg_alloc(i64 {}, i64 {})", buf, byte, align));
        self.emit(format!(
            "call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)",
            buf, p, byte));
        let a = self.fresh("cpv.");
        self.emit(format!("{} = insertvalue %JrgSlice undef, ptr {}, 0", a, buf));
        let b = self.fresh("cpv.");
        self.emit(format!("{} = insertvalue %JrgSlice {}, i64 {}, 1", b, a, n));
        Val { text: b, ty: v.ty.clone() }
    }

    pub fn str_concat(&mut self, lv: &Val, rv: &Val) -> Result<Val, String> {
        self.ensure_extern(
            "declare void @jrg_str_concat(ptr, i64, ptr, i64, ptr, ptr)", "jrg_str_concat");
        let (lptr, llen) = self.split_str(lv);
        let (rptr, rlen) = self.split_str(rv);
        let outp = self.alloca_raw("catp.", "ptr", None);
        let outl = self.alloca_raw("catl.", "i64", None);
        self.emit(format!("call void @jrg_str_concat(ptr {}, i64 {}, ptr {}, i64 {}, ptr {}, ptr {})",
            lptr, llen, rptr, rlen, outp, outl));
        self.join_str(&outp, &outl)
    }

    pub(crate) fn str_cmp(&mut self, op: &BinaryOp, lv: &Val, rv: &Val) -> Result<Val, String> {
        let (lptr, llen) = self.split_str(lv);
        let (rptr, rlen) = self.split_str(rv);
        match op {
            BinaryOp::Eq | BinaryOp::Ne => {
                self.ensure_extern("declare i32 @jrg_str_eq(ptr, i64, ptr, i64)", "jrg_str_eq");
                let r = self.fresh("streq.");
                self.emit(format!("{} = call i32 @jrg_str_eq(ptr {}, i64 {}, ptr {}, i64 {})", r, lptr, llen, rptr, rlen));
                let i1 = self.fresh("streqi1.");
                self.emit(format!("{} = icmp ne i32 {}, 0", i1, r));
                let out = self.from_i1(&i1);
                if *op == BinaryOp::Ne {
                    let r2 = self.fresh("strne.");
                    let i1b = self.to_i1(&out);
                    self.emit(format!("{} = xor i1 {}, true", r2, i1b));
                    return Ok(self.from_i1(&r2));
                }
                Ok(out)
            }
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                self.ensure_extern("declare i32 @jrg_str_lt(ptr, i64, ptr, i64)", "jrg_str_lt");
                let (a_ptr, a_len, b_ptr, b_len, negate) = match op {
                    BinaryOp::Lt => (lptr.clone(), llen.clone(), rptr.clone(), rlen.clone(), false),
                    BinaryOp::Ge => (lptr.clone(), llen.clone(), rptr.clone(), rlen.clone(), true),
                    BinaryOp::Gt => (rptr.clone(), rlen.clone(), lptr.clone(), llen.clone(), false),
                    BinaryOp::Le => (rptr.clone(), rlen.clone(), lptr.clone(), llen.clone(), true),
                    _ => unreachable!(),
                };
                let r = self.fresh("strlt.");
                self.emit(format!("{} = call i32 @jrg_str_lt(ptr {}, i64 {}, ptr {}, i64 {})", r, a_ptr, a_len, b_ptr, b_len));
                let i1 = self.fresh("strlti1.");
                self.emit(format!("{} = icmp ne i32 {}, 0", i1, r));
                if negate {
                    let r2 = self.fresh("strnlt.");
                    self.emit(format!("{} = xor i1 {}, true", r2, i1));
                    Ok(self.from_i1(&r2))
                } else {
                    Ok(self.from_i1(&i1))
                }
            }
            _ => unreachable!(),
        }
    }

    /// Split a `%JrgStr` value into its (ptr, len) scalar components — the
    /// calling convention every runtime string function actually uses.
    pub fn split_str(&mut self, v: &Val) -> (String, String) {
        let p = self.fresh("sp.");
        self.emit(format!("{} = extractvalue %JrgStr {}, 0", p, v.text));
        let l = self.fresh("sl.");
        self.emit(format!("{} = extractvalue %JrgStr {}, 1", l, v.text));
        (p, l)
    }

    /// Load a (ptr,len) pair written through two out-parameter allocas
    /// back into one `%JrgStr` Val — the mirror of `split_str`, used after
    /// any runtime call using the out-param string-return convention.
    pub fn join_str(&mut self, out_ptr_alloca: &str, out_len_alloca: &str) -> Result<Val, String> {
        let p = self.fresh("jp.");
        self.emit(format!("{} = load ptr, ptr {}", p, out_ptr_alloca));
        let l = self.fresh("jl.");
        self.emit(format!("{} = load i64, ptr {}", l, out_len_alloca));
        let s1 = self.fresh("js.");
        self.emit(format!("{} = insertvalue %JrgStr undef, ptr {}, 0", s1, p));
        let s2 = self.fresh("js.");
        self.emit(format!("{} = insertvalue %JrgStr {}, i64 {}, 1", s2, s1, l));
        Ok(Val { text: s2, ty: JrgType::Str })
    }

    // ── ternary / pipe ───────────────────────────────────────────────────

    fn codegen_ternary(&mut self, cond: &Expr, then_e: &Expr, else_e: &Expr, expected: Option<&JrgType>, _span: Span) -> Result<Val, String> {
        let result_ty = match expected.cloned()
            .or_else(|| self.peek_expr_type(then_e))
            .or_else(|| self.peek_expr_type(else_e))
            // Kalau kedua cabang literal angka, pakai bawaan yang SAMA dengan yang
            // dipakai literal itu di posisi mana pun.
            .or_else(|| tipe_bawaan_ekspresi(then_e))
            .or_else(|| tipe_bawaan_ekspresi(else_e))
        {
            Some(t) => t,
            None => return Err(
                // Openernya `$`, bukan `?`. Pesan ini masih menulis `?` jauh
                // setelah fase SINTAKS-TERNARY memindahkannya — pesan error
                // yang mengajarkan sintaks yang sudah tidak ada.
                "operator ternary `cond $ a : b` butuh tipe yang bisa disimpulkan \
                 dari konteks\n\
                 bantuan: beri anotasi tipe pada penerimanya, mis. \
                 `var x: int = c $ a : b`".into()
            ),
        };
        let cv = self.codegen_expr(cond, Some(&JrgType::Bool))?;
        let cv = self.coerce_longgar(&cv, &JrgType::Bool);
        let ci1 = self.to_i1(&cv);
        let then_lbl = self.fresh_label("tern.then.");
        let else_lbl = self.fresh_label("tern.else.");
        let merge_lbl = self.fresh_label("tern.end.");
        let slot = if result_ty != JrgType::Unit {
            self.alloca_typed("tern.slot.", &result_ty)
        } else {
            String::new()
        };
        self.terminate(format!("br i1 {}, label %{}, label %{}", ci1, then_lbl, else_lbl));

        self.start_block(then_lbl);
        let tv = self.codegen_expr(then_e, Some(&result_ty))?;
        if !self.is_terminated() {
            let tv = self.coerce(&tv, &result_ty);
            if result_ty != JrgType::Unit { self.emit(format!("store {} {}, ptr {}", result_ty.llvm_ty(), tv.text, slot)); }
            self.terminate(format!("br label %{}", merge_lbl));
        }

        self.start_block(else_lbl);
        let ev = self.codegen_expr(else_e, Some(&result_ty))?;
        if !self.is_terminated() {
            let ev = self.coerce(&ev, &result_ty);
            if result_ty != JrgType::Unit { self.emit(format!("store {} {}, ptr {}", result_ty.llvm_ty(), ev.text, slot)); }
            self.terminate(format!("br label %{}", merge_lbl));
        }

        self.start_block(merge_lbl);
        Ok(self.load_place(&slot, &result_ty))
    }

    /// Best-effort, side-effect-free type peek used only to give ternary a
    /// fighting chance at inferring its result type without an explicit …
    pub(crate) fn peek_expr_type(&self, e: &Expr) -> Option<JrgType> {
        match e {
            Expr::Ident(name, _) => self.lookup_local(name).map(|(_, t)| t),
            Expr::Literal(LitKind::Bool(_), _) => Some(JrgType::Bool),
            Expr::Literal(LitKind::Str(_), _) | Expr::Literal(LitKind::RawStr(_), _) => Some(JrgType::Str),
            Expr::Unary { op: UnaryOp::Neg, expr, .. } => self.peek_expr_type(expr),
            // `match` juga sebuah EKSPRESI, dan tipenya adalah tipe arm-nya.
            Expr::Match { arms, .. } => self.tipe_hasil_arm(arms),
            Expr::Call { func, .. } => {
                if let Expr::Ident(name, _) = func.as_ref() {
                    let f = self.tables.functions.get(&self.k(name))?;
                    let te = f.ret_type.as_ref()?;
                    let ctx = TypeResolveCtx { tables: &self.tables, subst: &TypeSubst::new() };
                    resolve_type_expr(te, &ctx).ok()
                } else { None }
            }
            _ => None,
        }
    }

    fn codegen_pipe(&mut self, lhs: &Expr, rhs: &Expr, expected: Option<&JrgType>, span: Span) -> Result<Val, String> {
        // `x |> f(args...)`  ==  `f(x, args...)`
        // `x |> f`           ==  `f(x)`
        match rhs {
            Expr::Call { func, args, span: cspan } => {
                let mut new_args = Vec::with_capacity(args.len() + 1);
                new_args.push(lhs.clone());
                new_args.extend(args.iter().cloned());
                self.codegen_call(func, &new_args, expected, *cspan)
            }
            other => self.codegen_call(&Box::new(other.clone()), std::slice::from_ref(lhs), expected, span),
        }
    }

    // ── casts ────────────────────────────────────────────────────────────

    fn codegen_cast(&mut self, e: &Expr, ty: &TypeExpr, span: Span) -> Result<Val, String> {
        // Pointer di backend ini cuma bilangan bulat tipe tujuannya, jadi `(-1) as *const int`
        // menjawab -1 dan `x as *const int` menjawab x — ditolak sampai pointer benar-benar ada.
        if matches!(ty, TypeExpr::Pointer(_)) {
            return Err(super::galat::tandai(span,
                "cast ke tipe pointer belum didukung backend LLVM v1 — di sini pointer cuma \
                 bilangan bulat, jadi hasilnya akan berbeda dari acuan. Jalankan tanpa `--emit-llvm`"));
        }
        // Sasaran angka dan rune lewat aturan acuan; `bool`/`string` tetap
        // truthiness dan tampilan biasa.
        if let Some(s) = super::codegen_konversi::Sasaran::dari_tipe(ty) {
            let v = self.codegen_expr(e, None)?;
            return self.cast_eksplisit(&v, &s, span);
        }
        let target = self.resolve_type_pub(ty)?;
        let v = self.codegen_expr(e, None)?;
        self.do_cast(&v, &target, span)
    }

    /// Konversi yang DITULIS: jenis memang diminta berganti.
    pub fn do_cast(&mut self, v: &Val, target: &JrgType, span: Span) -> Result<Val, String> {
        let lama = std::mem::replace(&mut self.koersi_longgar, true);
        let r = self.do_cast_dalam(v, target, span);
        self.koersi_longgar = lama;
        r
    }

    fn do_cast_dalam(&mut self, v: &Val, target: &JrgType, _span: Span) -> Result<Val, String> {
        // Nilai bertag ke teks: tampilannya menurut tag saat jalan.
        if v.ty == JrgType::Any && *target == JrgType::Str { return self.tampilan_any(v, _span); }
        if matches!(v.ty, JrgType::Fn(..)) && *target == JrgType::Str { return Ok(self.tampilan_fungsi(v)); }
        // `str(x)` atas agregat adalah tampilannya — sama dengan yang dicetak `wok`.
        if *target == JrgType::Str && matches!(v.ty, JrgType::Array(..) | JrgType::Slice(_) | JrgType::Tuple(_)
            | JrgType::Struct(_) | JrgType::Enum(_) | JrgType::Dict(_) | JrgType::DictCampur(_) | JrgType::Unit | JrgType::Nil
            | JrgType::Sinkron(_) | JrgType::Chan(_) | JrgType::Masa(..)) {
            return self.build_display_str(v);
        }
        if v.ty == *target { return Ok(v.clone()); }
        // `nil` boleh jadi `bool` (selalu `false`, diukur) tapi tidak boleh
        // jadi apa pun yang lain — `nil as int` ditolak acuannya.
        if v.ty == JrgType::Nil && *target == JrgType::Bool {
            return Ok(self.coerce(v, target));
        }
        let bisa_langsung = |t: &JrgType| t.is_numeric() || *t == JrgType::Bool || *t == JrgType::Rune;
        if bisa_langsung(&v.ty) && bisa_langsung(target) {
            return Ok(self.coerce(v, target));
        }
        // `bool(str)` bukan parsing, melainkan truthiness: diukur, `bool("0")`
        // bernilai **true** karena panjangnya 1.
        if v.ty == JrgType::Str && *target == JrgType::Bool {
            return Ok(self.coerce(v, target));
        }
        if v.ty.is_numeric() && *target == JrgType::Str {
            return self.num_to_str(v);
        }
        if v.ty == JrgType::Bool && *target == JrgType::Str {
            return self.bool_to_str(v);
        }
        if v.ty == JrgType::Rune && *target == JrgType::Str {
            return self.rune_to_str(v);
        }
        Err(format!("cast dari `{}` ke `{}` tidak didukung", v.ty.display_name(), target.display_name()))
    }

    fn num_to_str(&mut self, v: &Val) -> Result<Val, String> {
        let outp = self.alloca_raw("nsp.", "ptr", None);
        let outl = self.alloca_raw("nsl.", "i64", None);
        match &v.ty {
            JrgType::I8 | JrgType::I16 | JrgType::I32 | JrgType::I64 => {
                self.ensure_extern("declare void @jrg_i64_to_str(i64, ptr, ptr)", "jrg_i64_to_str");
                let v64 = self.coerce_longgar(v, &JrgType::I64);
                self.emit(format!("call void @jrg_i64_to_str(i64 {}, ptr {}, ptr {})", v64.text, outp, outl));
            }
            JrgType::U8 | JrgType::U16 | JrgType::U32 | JrgType::U64 => {
                self.ensure_extern("declare void @jrg_u64_to_str(i64, ptr, ptr)", "jrg_u64_to_str");
                let v64 = self.coerce_longgar(v, &JrgType::U64);
                self.emit(format!("call void @jrg_u64_to_str(i64 {}, ptr {}, ptr {})", v64.text, outp, outl));
            }
            JrgType::I128 => {
                self.ensure_extern("declare void @jrg_i128_to_str(i64, i64, ptr, ptr)", "jrg_i128_to_str");
                let (lo, hi) = self.split_i128(v);
                self.emit(format!("call void @jrg_i128_to_str(i64 {}, i64 {}, ptr {}, ptr {})", lo, hi, outp, outl));
            }
            JrgType::U128 => {
                self.ensure_extern("declare void @jrg_u128_to_str(i64, i64, ptr, ptr)", "jrg_u128_to_str");
                let (lo, hi) = self.split_i128(v);
                self.emit(format!("call void @jrg_u128_to_str(i64 {}, i64 {}, ptr {}, ptr {})", lo, hi, outp, outl));
            }
            JrgType::F32 => {
                self.ensure_extern("declare void @jrg_f32_to_str(float, ptr, ptr)", "jrg_f32_to_str");
                self.emit(format!("call void @jrg_f32_to_str(float {}, ptr {}, ptr {})", v.text, outp, outl));
            }
            JrgType::F64 => {
                self.ensure_extern("declare void @jrg_f64_to_str(double, ptr, ptr)", "jrg_f64_to_str");
                self.emit(format!("call void @jrg_f64_to_str(double {}, ptr {}, ptr {})", v.text, outp, outl));
            }
            other => return Err(format!("konversi `{}` ke string belum didukung", other.display_name())),
        }
        self.join_str(&outp, &outl)
    }

    /// `{:.N}` — satu nilai dengan jumlah desimal tetap.
    pub fn str_desimal(&mut self, v: &Val, n: usize, span: Span) -> Result<Val, String> {
        let outp = self.alloca_raw("dsp.", "ptr", None);
        let outl = self.alloca_raw("dsl.", "i64", None);
        match &v.ty {
            JrgType::F64 | JrgType::F32 => {
                let d = if v.ty == JrgType::F32 {
                    self.do_cast(v, &JrgType::F64, span)?
                } else { v.clone() };
                self.ensure_extern(
                    "declare void @jrg_f64_to_str_desimal(double, i64, ptr, ptr)",
                    "jrg_f64_to_str_desimal");
                self.emit(format!(
                    "call void @jrg_f64_to_str_desimal(double {}, i64 {}, ptr {}, ptr {})",
                    d.text, n, outp, outl));
            }
            t if t.is_int() => {
                let i = if v.ty == JrgType::I64 { v.clone() }
                        else { self.do_cast(v, &JrgType::I64, span)? };
                self.ensure_extern(
                    "declare void @jrg_i64_to_str_desimal(i64, i64, ptr, ptr)",
                    "jrg_i64_to_str_desimal");
                self.emit(format!(
                    "call void @jrg_i64_to_str_desimal(i64 {}, i64 {}, ptr {}, ptr {})",
                    i.text, n, outp, outl));
            }
            other => return Err(format!(
                "[{}:{}] str::format: spec `{{:.{}}}` cuma untuk angka, dapat `{}`",
                span.lo.0, span.hi.0, n, other.display_name())),
        }
        self.join_str(&outp, &outl)
    }

    /// `rune` → `string`: satu karakter, dikodekan UTF-8.
    fn rune_to_str(&mut self, v: &Val) -> Result<Val, String> {
        self.ensure_extern("declare void @jrg_char_to_str(i32, ptr, ptr)", "jrg_char_to_str");
        let outp = self.alloca_raw("rsp.", "ptr", None);
        let outl = self.alloca_raw("rsl.", "i64", None);
        self.emit(format!("call void @jrg_char_to_str(i32 {}, ptr {}, ptr {})", v.text, outp, outl));
        self.join_str(&outp, &outl)
    }

    fn bool_to_str(&mut self, v: &Val) -> Result<Val, String> {
        self.ensure_extern("declare void @jrg_bool_to_str(i32, ptr, ptr)", "jrg_bool_to_str");
        let outp = self.alloca_raw("bsp.", "ptr", None);
        let outl = self.alloca_raw("bsl.", "i64", None);
        let v32 = self.fresh("b32.");
        self.emit(format!("{} = zext i8 {} to i32", v32, v.text));
        self.emit(format!("call void @jrg_bool_to_str(i32 {}, ptr {}, ptr {})", v32, outp, outl));
        self.join_str(&outp, &outl)
    }

    pub fn split_i128(&mut self, v: &Val) -> (String, String) {
        let lo = self.fresh("lo.");
        self.emit(format!("{} = trunc i128 {} to i64", lo, v.text));
        let sh = self.fresh("sh.");
        self.emit(format!("{} = lshr i128 {}, 64", sh, v.text));
        let hi = self.fresh("hi.");
        self.emit(format!("{} = trunc i128 {} to i64", hi, sh));
        (lo, hi)
    }

    pub fn align_of_hint_pub(&self, ty: &JrgType) -> u64 {
        self.align_of_hint(ty)
    }
}

/// Literal bilangan bulat yang tidak muat di tipe tempatnya diturunkan backend ini.
fn pesan_literal_tak_muat(v: u128, ty: &JrgType, lo: i128, hi: i128) -> String {
    if matches!(ty, JrgType::I64) {
        return format!("literal `{}` tidak muat di `int` backend LLVM v1, yang 64 bit ({} sampai {}) — \
            dua mesin lain menyimpan `int` 128 bit. Jalankan tanpa `--emit-llvm`", v, lo, hi);
    }
    format!("literal `{}` tidak muat di `{}` ({} sampai {}) — backend LLVM menyimpan nilai selebar \
        anotasinya. Pakai tipe yang lebih lebar, atau jalankan tanpa `--emit-llvm`", v, ty.display_name(), lo, hi)
}
