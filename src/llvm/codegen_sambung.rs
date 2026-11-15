// src/llvm/codegen_sambung.rs — `a + b + c + ..` atas teks dalam satu alokasi (`jrg_str_concat_n`).
// Dulu tiap `+` menyalin hasil sementaranya, dan `str(x)` di tengah rantai membuat teks sementara lagi;
// operandnya tetap dievaluasi kiri ke kanan, sama dengan bentuk berpasangan.

use crate::lexer::Span;
use crate::parser::*;

use super::codegen::{Codegen, Val};
use super::types::JrgType;

/// Satu operand rantai: teks, bilangan bulat yang ditulis langsung, atau isi `str(x)` yang mungkin bulat
/// (ditulis langsung kalau ternyata `int`, selain itu lewat `str` seperti biasa).
enum Bagian<'a> { Teks(&'a Expr), Bulat(&'a Expr), IsiStr(&'a Expr) }

impl Codegen {
    /// `Some` kalau `lhs + rhs` adalah rantai teks yang bisa disambung sekaligus; `None` = jalur biasa.
    pub(crate) fn coba_sambung_teks(&mut self, lhs: &Expr, rhs: &Expr, span: Span) -> Option<Result<Val, String>> {
        let mut ops: Vec<&Expr> = vec![rhs];
        let mut kini = lhs;
        while let Expr::Binary { op: BinaryOp::Add, lhs: l, rhs: r, .. } = kini {
            ops.push(r);
            kini = l;
        }
        ops.push(kini);
        ops.reverse();
        if ops.len() < 3 || self.peek_expr_type(ops[0]) != Some(JrgType::Str) { return None; }
        let str_bawaan = self.cari_fn_bersarang("str").is_none() && !self.tables.functions.contains_key(&self.k("str"))
            && self.lookup_local("str").is_none();
        let mut bagian = Vec::with_capacity(ops.len());
        for e in &ops {
            let isi_str = match e {
                Expr::Call { func, args, .. } if str_bawaan && args.len() == 1
                    && matches!(&**func, Expr::Ident(n, _) if n == "str") => Some(&args[0]),
                _ => None,
            };
            // `str(x)` selalu teks: bulat ditulis langsung, yang lain lewat `str` seperti biasa.
            if let Some(a) = isi_str {
                bagian.push(if self.peek_expr_type(a) == Some(JrgType::I64) { Bagian::Bulat(a) }
                    else if self.mungkin_bulat(a) { Bagian::IsiStr(a) } else { Bagian::Teks(e) });
                continue;
            }
            match self.peek_expr_type(e)? {
                JrgType::Str => bagian.push(Bagian::Teks(e)),
                JrgType::I64 => bagian.push(Bagian::Bulat(e)),
                _ => return None,
            }
        }
        Some(self.sambung_teks(&bagian, span))
    }

    /// Dugaan murah bahwa `e` bulat: literal bulat, jalur bertipe `int`, atau aritmetika atas keduanya.
    fn mungkin_bulat(&self, e: &Expr) -> bool {
        match e {
            Expr::Literal(LitKind::Int(_), _) => true,
            Expr::Ident(..) | Expr::Field { .. } | Expr::Index { .. } =>
                self.peek_expr_type(e).or_else(|| self.tipe_jalur(e)) == Some(JrgType::I64),
            Expr::Binary { op: BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem, lhs, rhs, .. } =>
                self.mungkin_bulat(lhs) && self.mungkin_bulat(rhs),
            _ => false,
        }
    }

    fn sambung_teks(&mut self, bagian: &[Bagian], _span: Span) -> Result<Val, String> {
        let n = bagian.len();
        let tabel = self.alloca_raw("sbn.", &format!("[{} x {{ i64, ptr, i64 }}]", n), Some(8));
        for (i, b) in bagian.iter().enumerate() {
            let (jenis, p, x) = match b {
                Bagian::Teks(e) => {
                    let v = self.codegen_expr(e, Some(&JrgType::Str))?;
                    if v.ty != JrgType::Str { return Err(format!("sambung teks: operand bertipe `{}`", v.ty.display_name())); }
                    let (p, l) = self.split_str(&v);
                    (0, p, l)
                }
                Bagian::IsiStr(e) => {
                    let v = self.codegen_expr(e, None)?;
                    if v.ty == JrgType::I64 { (1, "null".to_string(), v.text) } else {
                        let t = self.do_cast(&v, &JrgType::Str, e.span())?;
                        let (p, l) = self.split_str(&t);
                        (0, p, l)
                    }
                }
                Bagian::Bulat(e) => {
                    let v = self.codegen_expr(e, Some(&JrgType::I64))?;
                    if v.ty != JrgType::I64 { return Err(format!("sambung teks: operand bertipe `{}`", v.ty.display_name())); }
                    (1, "null".to_string(), v.text)
                }
            };
            for (k, (t, isi)) in [("i64", jenis.to_string()), ("ptr", p), ("i64", x)].into_iter().enumerate() {
                let g = self.fresh("sbg.");
                self.emit(format!("{} = getelementptr [{} x {{ i64, ptr, i64 }}], ptr {}, i64 0, i64 {}, i32 {}", g, n, tabel, i, k));
                self.emit(format!("store {} {}, ptr {}", t, isi, g));
            }
        }
        self.ensure_extern("declare void @jrg_str_concat_n(ptr, i64, ptr, ptr)", "jrg_str_concat_n");
        let outp = self.alloca_raw("sbp.", "ptr", None);
        let outl = self.alloca_raw("sbl.", "i64", None);
        self.emit(format!("call void @jrg_str_concat_n(ptr {}, i64 {}, ptr {}, ptr {})", tabel, n, outp, outl));
        self.join_str(&outp, &outl)
    }
}

impl Codegen {
    /// `get_place(e)` akan menjawab tempat bertag: elemen/kunci wadah bernilai bertag atau field bertag.
    /// Tipe BACA-nya bisa lain (dict campur mengingat jenis tiap kunci), jadi yang ditanya tipe wadahnya.
    pub(crate) fn tempat_bertag(&self, e: &Expr) -> bool {
        let bertag = |t: &JrgType| matches!(t, JrgType::Any);
        match e {
            Expr::Index { obj, .. } => match self.peek_expr_type(obj) {
                Some(JrgType::DictCampur(_)) => true,
                Some(JrgType::Dict(v) | JrgType::Slice(v) | JrgType::Array(v, _)) => bertag(&v),
                _ => false,
            },
            Expr::Field { obj, field, .. } => match self.peek_expr_type(obj) {
                Some(JrgType::DictCampur(_)) => true,
                Some(JrgType::Dict(v)) => bertag(&v),
                Some(JrgType::Struct(n)) => self.types.structs.get(&n)
                    .and_then(|s| s.field_type(field)).is_some_and(|t| bertag(&t.tanpa_kotak())),
                _ => false,
            },
            _ => false,
        }
    }

    /// `x[i] = v` atas lokal bertag atau tempat antara bertag (`d[k][i] = v`, dicari sesudah sisi kanan): isinya
    /// dibuka menurut tag saat jalan sebagai `array<any>`/`dict<any>`, ditulis lewat penugasan biasa, dikotakkan
    /// kembali; jenis lain gagal saat jalan dengan kalimat acuan.
    pub(crate) fn tulis_indeks_bertag(&mut self, wadah: &Expr, indeks: &Expr, nilai: &Expr, span: Span)
        -> Result<(), String> {
        use super::codegen_any::{TAG_DICT, TAG_SLICE};
        let v = self.codegen_expr(nilai, None)?;
        let v = self.ambil_milik(nilai, v)?;
        let vk = self.kotak_any(&v, span)?;
        let nilai_slot = self.alloca_typed("tbn.", &JrgType::Any);
        self.emit(format!("store %JrgAny {}, ptr {}", vk.text, nilai_slot));
        let slot = match wadah {
            Expr::Ident(nama, _) => self.lookup_local(nama).ok_or("lokal bertag hilang")?.0,
            _ => {
                // Kunci antara yang hilang ditunjuk di tempat antaranya, seperti acuan.
                let lama = self.masuk_span(wadah.span());
                self.tulis_antara = true;
                let r = self.get_place(wadah);
                self.pulih_span(lama);
                let (p, t) = r?;
                if t != JrgType::Any { return Err(format!("tempat bertag ternyata `{}`", t.display_name())); }
                p
            }
        };
        let x = self.load_place(&slot, &JrgType::Any);
        let tag = self.tag_any(&x);
        let (l_arr, l_cek, l_dict, l_lain, usai) = (self.fresh_label("tb.arr."), self.fresh_label("tb.cek."),
            self.fresh_label("tb.dict."), self.fresh_label("tb.lain."), self.fresh_label("tb.usai."));
        let (ca, cd) = (self.fresh("tba."), self.fresh("tbd."));
        self.emit(format!("{} = icmp eq i32 {}, {}", ca, tag.text, TAG_SLICE));
        self.terminate(format!("br i1 {}, label %{}, label %{}", ca, l_arr, l_cek));
        self.start_block(l_cek);
        self.emit(format!("{} = icmp eq i32 {}, {}", cd, tag.text, TAG_DICT));
        self.terminate(format!("br i1 {}, label %{}, label %{}", cd, l_dict, l_lain));
        for (lbl, wadah) in [(l_arr, JrgType::Slice(Box::new(JrgType::Any))), (l_dict, JrgType::Dict(Box::new(JrgType::Any)))] {
            self.start_block(lbl);
            let isi = self.buka_any(&x, &wadah, span)?;
            let tmp = self.alloca_typed("tbw.", &wadah);
            self.emit(format!("store {} {}, ptr {}", wadah.llvm_ty(), isi.text, tmp));
            self.push_scope();
            self.define_local("$tulis_wadah", tmp.clone(), wadah.clone());
            self.define_local("$tulis_nilai", nilai_slot.clone(), JrgType::Any);
            let tugas = AssignStmt {
                targets: vec![Expr::Index { obj: Box::new(Expr::Ident("$tulis_wadah".into(), span)),
                    idx: Box::new(indeks.clone()), span }],
                op: AssignOp::Eq, values: vec![Expr::Ident("$tulis_nilai".into(), span)], span,
            };
            let lama = self.masuk_span(span);
            let r = self.codegen_assign(&tugas);
            self.pulih_span(lama);
            self.pop_scope();
            r?;
            let baru = self.load_place(&tmp, &wadah);
            let k = self.kotak_any(&baru, span)?;
            self.emit(format!("store %JrgAny {}, ptr {}", k.text, slot));
            if !self.terminated { self.terminate(format!("br label %{}", usai)); }
        }
        self.start_block(l_lain);
        // Lokasinya sasaran `x[i]`, bukan seluruh penugasan — seperti acuan.
        let lama = self.masuk_span(span);
        self.panik_tipe_any(&x, crate::interpreter::galat_index::tak_bisa_ditulis_lewat_index);
        self.pulih_span(lama);
        self.start_block(usai);
        Ok(())
    }
}
