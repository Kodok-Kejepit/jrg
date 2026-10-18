// src/llvm/codegen_eq.rs — llvm/codegen_eq.rs — kesetaraan STRUKTURAL untuk nilai agregat.

use crate::lexer::Span;
use crate::parser::BinaryOp;

use super::codegen::{Codegen, Val};
use super::types::JrgType;

impl Codegen {
    /// Apakah tipe ini butuh perbandingan struktural (bukan satu `icmp`)?
    pub fn butuh_eq_struktural(ty: &JrgType) -> bool {
        matches!(
            ty,
            JrgType::Unit
                | JrgType::Array(_, _)
                | JrgType::Slice(_)
                | JrgType::Dict(_)
                | JrgType::Tuple(_)
                | JrgType::Struct(_)
                // Enum masuk daftar sejak sebelum ia didukung: waktu itu tanpa lengan ini ia
                // mendarat di penolakan generik `cmp_binop` dan pembacanya tidak tahu apa …
                | JrgType::Enum(_)
                // Kotak bertag: tagnya dibandingkan dulu, lalu isinya per
                // golongan.
                | JrgType::Any
                | JrgType::DictCampur(_)
        )
    }

    /// `a == b` / `a != b` untuk nilai agregat.
    pub fn agregat_cmp(&mut self, op: &BinaryOp, lv: &Val, rv: &Val, span: Span) -> Result<Val, String> {
        // Nilai bertag PUNYA urutan — kalau isinya angka atau string.
        if lv.ty == JrgType::Any && !matches!(op, BinaryOp::Eq | BinaryOp::Ne) {
            return self.urut_any(op, lv, rv, span);
        }
        match op {
            BinaryOp::Eq | BinaryOp::Ne => {}
            // Urutan array dan tuple: leksikografis, elemen per elemen, lalu panjangnya.
            _ => return self.urut_agregat(op, lv, rv, span),
        }

        if lv.ty != rv.ty {
            return Err(super::galat::tandai(span, format!(
                "tidak bisa membandingkan `{}` dengan `{}` — tipenya harus sama",
                lv.ty.display_name(), rv.ty.display_name()
            )));
        }

        let slot = self.alloca_raw("eqacc.", "i8", Some(1));
        self.emit(format!("store i8 1, ptr {}", slot));
        self.kumpul_sama(&slot, &lv.ty, &lv.text, &rv.text, span)?;

        let hasil = self.fresh("eqv.");
        self.emit(format!("{} = load i8, ptr {}", hasil, slot));

        // `!=` adalah negasi dari `==`, dihitung sekali di ujung — bukan
        // dibalik di tiap komponen, yang akan salah (a != b benar kalau SATU
        // komponen berbeda, bukan kalau SEMUA berbeda).
        if matches!(op, BinaryOp::Ne) {
            let i1 = self.fresh("eqi1.");
            self.emit(format!("{} = trunc i8 {} to i1", i1, hasil));
            let neg = self.fresh("nei1.");
            self.emit(format!("{} = xor i1 {}, true", neg, i1));
            return Ok(self.from_i1(&neg));
        }
        Ok(Val { text: hasil, ty: JrgType::Bool })
    }

    /// `<`/`<=`/`>`/`>=` atas array atau tuple, lewat pembanding tiga-arah.
    fn urut_agregat(&mut self, op: &BinaryOp, lv: &Val, rv: &Val, span: Span) -> Result<Val, String> {
        // Dua array beda panjang (atau array lawan slice) dibandingkan sebagai slice.
        let sebagai_slice = |t: &JrgType| match t {
            JrgType::Array(e, _) | JrgType::Slice(e) => Some(JrgType::Slice(Box::new(super::codegen_any::luruh(e)))),
            _ => None,
        };
        let (lv, rv) = match (sebagai_slice(&lv.ty), sebagai_slice(&rv.ty)) {
            (Some(a), Some(b)) if a == b && lv.ty != rv.ty => (self.coerce(lv, &a), self.coerce(rv, &b)),
            _ => (lv.clone(), rv.clone()),
        };
        if lv.ty != rv.ty {
            return Err(super::galat::tandai(span, format!(
                "tidak bisa membandingkan `{}` dengan `{}` — tipenya harus sama",
                lv.ty.display_name(), rv.ty.display_name())));
        }
        let hasil = self.alloca_raw("urt.", "i32", Some(4));
        self.emit(format!("store i32 0, ptr {}", hasil));
        let l_end = self.fresh_label("urt.end.");
        self.banding_ke(&hasil, &l_end, &lv.ty, &lv.text, &rv.text, span)?;
        if !self.is_terminated() { self.terminate(format!("br label %{}", l_end)); }
        self.start_block(l_end);
        let c = self.fresh("urtc.");
        self.emit(format!("{} = load i32, ptr {}", c, hasil));
        let pred = match op { BinaryOp::Lt => "slt", BinaryOp::Le => "sle", BinaryOp::Gt => "sgt", _ => "sge" };
        let r = self.fresh("urtr.");
        self.emit(format!("{} = icmp {} i32 {}, 0", r, pred, c));
        Ok(self.from_i1(&r))
    }

    /// Badan `@banding.<tipe>(ptr a, ptr b) -> i32`: pembanding tiga-arah untuk pengurutan.
    pub(crate) fn pancarkan_banding(&mut self, nama: &str, ty: &JrgType) -> Result<(), String> {
        let nol = Span { lo: crate::lexer::BytePos(0), hi: crate::lexer::BytePos(0), file_id: 0 };
        self.mulai_fungsi_kosong(JrgType::I32);
        let a = self.load_place("%a", ty);
        let b = self.load_place("%b", ty);
        let hasil = self.alloca_raw("bnd.", "i32", Some(4));
        self.emit(format!("store i32 0, ptr {}", hasil));
        let l_end = self.fresh_label("bnd.end.");
        self.banding_ke(&hasil, &l_end, ty, &a.text, &b.text, nol)?;
        if !self.is_terminated() { self.terminate(format!("br label %{}", l_end)); }
        self.start_block(l_end);
        let r = self.fresh("bndr.");
        self.emit(format!("{} = load i32, ptr {}", r, hasil));
        self.terminate(format!("ret i32 {}", r));
        let badan = self.take_function_body();
        self.tambah_definisi(format!(
            "define internal i32 @{}(ptr %a, ptr %b) nounwind {{\n{}}}\n", super::mangle::llvm_quoted_ident(nama), badan));
        self.selesai_fungsi_kosong();
        Ok(())
    }

    /// Simpan -1/1 ke `slot` dan lompat ke `l_end` begitu `l` dan `r` berbeda; kalau sama, lanjut.
    fn banding_ke(&mut self, slot: &str, l_end: &str, ty: &JrgType, l: &str, r: &str, span: Span) -> Result<(), String> {
        // Dari dua `i1` "lebih kecil"/"lebih besar": simpan hasilnya, keluar kalau berbeda.
        let putus = |cg: &mut Self, lt: String, gt: String| {
            let a = cg.fresh("bk.");
            cg.emit(format!("{} = select i1 {}, i32 1, i32 0", a, gt));
            let c = cg.fresh("bk.");
            cg.emit(format!("{} = select i1 {}, i32 -1, i32 {}", c, lt, a));
            cg.emit(format!("store i32 {}, ptr {}", c, slot));
            let beda = cg.fresh("bkb.");
            cg.emit(format!("{} = icmp ne i32 {}, 0", beda, c));
            let lanjut = cg.fresh_label("bk.sama.");
            cg.terminate(format!("br i1 {}, label %{}, label %{}", beda, l_end, lanjut));
            cg.start_block(lanjut);
        };
        match ty {
            JrgType::Unit | JrgType::Nil => Ok(()),
            t if t.is_int() || matches!(t, JrgType::Bool | JrgType::Rune) => {
                let (pl, pg) = if t.is_signed_int() { ("slt", "sgt") } else { ("ult", "ugt") };
                let (lt, gt) = (self.fresh("bkl."), self.fresh("bkg."));
                self.emit(format!("{} = icmp {} {} {}, {}", lt, pl, t.llvm_ty(), l, r));
                self.emit(format!("{} = icmp {} {} {}, {}", gt, pg, t.llvm_ty(), l, r));
                putus(self, lt, gt);
                Ok(())
            }
            t if t.is_float() => {
                // NaN tidak terbanding: pembandingannya batal, seperti di dua mesin lain.
                let nan = self.fresh("bkn.");
                self.emit(format!("{} = fcmp uno {} {}, {}", nan, t.llvm_ty(), l, r));
                let (gagal, lanjut) = (self.fresh_label("bk.nan."), self.fresh_label("bk.ok."));
                self.terminate(format!("br i1 {}, label %{}, label %{}, !prof !0", nan, gagal, lanjut));
                self.start_block(gagal);
                self.panik(&crate::interpreter::pesan_banding("float", "float"));
                self.start_block(lanjut);
                let (lt, gt) = (self.fresh("bkl."), self.fresh("bkg."));
                self.emit(format!("{} = fcmp olt {} {}, {}", lt, t.llvm_ty(), l, r));
                self.emit(format!("{} = fcmp ogt {} {}, {}", gt, t.llvm_ty(), l, r));
                putus(self, lt, gt);
                Ok(())
            }
            JrgType::Str => {
                let (a, b) = (Val { text: l.to_string(), ty: JrgType::Str }, Val { text: r.to_string(), ty: JrgType::Str });
                let lt = self.str_cmp(&BinaryOp::Lt, &a, &b)?;
                let gt = self.str_cmp(&BinaryOp::Gt, &a, &b)?;
                let (lt1, gt1) = (self.to_i1(&lt), self.to_i1(&gt));
                putus(self, lt1, gt1);
                Ok(())
            }
            JrgType::Tuple(elems) => {
                for (i, ety) in elems.iter().enumerate() {
                    let le = self.ambil_elemen(ty, l, i, ety);
                    let re = self.ambil_elemen(ty, r, i, ety);
                    self.banding_ke(slot, l_end, ety, &le, &re, span)?;
                }
                Ok(())
            }
            JrgType::Array(elem, _) | JrgType::Slice(elem) => {
                let elem = (**elem).clone();
                let (lp, ln) = self.bagian_koleksi_pub(&Val { text: l.to_string(), ty: ty.clone() }, &elem);
                let (rp, rn) = self.bagian_koleksi_pub(&Val { text: r.to_string(), ty: ty.clone() }, &elem);
                let pendek = self.fresh("bkm.");
                let lk = self.fresh("bkm.");
                self.emit(format!("{} = icmp slt i64 {}, {}", lk, ln, rn));
                self.emit(format!("{} = select i1 {}, i64 {}, i64 {}", pendek, lk, ln, rn));
                let idx = self.alloca_raw("bki.", "i64", Some(8));
                self.emit(format!("store i64 0, ptr {}", idx));
                let (cek, badan, usai) = (self.fresh_label("bk.cek."), self.fresh_label("bk.badan."), self.fresh_label("bk.usai."));
                self.terminate(format!("br label %{}", cek));
                self.start_block(cek.clone());
                let i = self.fresh("bki.");
                self.emit(format!("{} = load i64, ptr {}", i, idx));
                let c = self.fresh("bkc.");
                self.emit(format!("{} = icmp slt i64 {}, {}", c, i, pendek));
                self.terminate(format!("br i1 {}, label %{}, label %{}", c, badan, usai));
                self.start_block(badan);
                let ety = elem.llvm_ty();
                let (ep, fp) = (self.fresh("bke."), self.fresh("bke."));
                self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", ep, ety, lp, i));
                self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", fp, ety, rp, i));
                let le = self.load_place(&ep, &elem);
                let re = self.load_place(&fp, &elem);
                self.banding_ke(slot, l_end, &elem, &le.text, &re.text, span)?;
                let n1 = self.fresh("bkn.");
                self.emit(format!("{} = add i64 {}, 1", n1, i));
                self.emit(format!("store i64 {}, ptr {}", n1, idx));
                self.terminate(format!("br label %{}", cek));
                self.start_block(usai);
                // Awalan yang sama: yang lebih pendek lebih kecil.
                let (lt, gt) = (self.fresh("bkl."), self.fresh("bkg."));
                self.emit(format!("{} = icmp slt i64 {}, {}", lt, ln, rn));
                self.emit(format!("{} = icmp sgt i64 {}, {}", gt, ln, rn));
                putus(self, lt, gt);
                Ok(())
            }
            lain => Err(super::galat::tandai(span, format!(
                "backend LLVM belum bisa mengurutkan tipe `{}` — cuma `==` dan `!=`", lain.display_name()))),
        }
    }

    /// AND-kan hasil perbandingan satu komponen ke akumulator.
    pub fn dan_ke_slot(&mut self, slot: &str, komponen: &Val) {
        let lama = self.fresh("eqold.");
        self.emit(format!("{} = load i8, ptr {}", lama, slot));
        let baru = self.fresh("eqand.");
        self.emit(format!("{} = and i8 {}, {}", baru, lama, komponen.text));
        self.emit(format!("store i8 {}, ptr {}", baru, slot));
    }

    /// Bandingkan dua nilai bertipe `ty`, AND-kan hasilnya ke `slot`.
    pub fn kumpul_sama(
        &mut self,
        slot: &str,
        ty: &JrgType,
        lval: &str,
        rval: &str,
        span: Span,
    ) -> Result<(), String> {
        match ty {
            // `() == ()` selalu benar, dan tidak ada bit apa pun untuk dibandingkan.
            JrgType::Unit | JrgType::Nil => Ok(()),

            JrgType::Tuple(elems) => {
                for (i, ety) in elems.iter().enumerate() {
                    let le = self.ambil_elemen(ty, lval, i, ety);
                    let re = self.ambil_elemen(ty, rval, i, ety);
                    self.kumpul_sama(slot, ety, &le, &re, span)?;
                }
                Ok(())
            }

            JrgType::Struct(name) => {
                let layout = self.types.structs.get(name).cloned().ok_or_else(|| {
                    super::galat::tandai(span, format!("struct `{}` tidak dikenal", name))
                })?;
                // Visibilitas field TIDAK diperiksa di sini, dan itu memang benar: yang
                // dibandingkan adalah nilainya sebagai satu kesatuan, bukan sebuah akses …
                for (i, (_, fty)) in layout.fields.iter().enumerate() {
                    let le = self.ambil_elemen(ty, lval, i, fty);
                    let re = self.ambil_elemen(ty, rval, i, fty);
                    self.kumpul_sama(slot, fty, &le, &re, span)?;
                }
                Ok(())
            }

            JrgType::Array(elem, n) => {
                let panjang = Val { text: n.to_string(), ty: JrgType::I64 };
                self.bandingkan_berurut(slot, ty, lval, rval, elem, &panjang, span)
            }

            JrgType::DictCampur(_) => {
                self.kumpul_sama(slot, &JrgType::Dict(Box::new(JrgType::Any)), lval, rval, span)
            }

            JrgType::Dict(vty) => {
                let vty = (**vty).clone();
                self.kumpul_sama_dict(slot, &vty, lval, rval, span)
            }

            JrgType::Slice(elem) => {
                // Panjang dibandingkan lebih dulu: dua slice beda panjang
                // tidak sama, dan tanpa ini loopnya akan membaca melewati
                // ujung yang lebih pendek.
                let ll = self.fresh("sll.");
                self.emit(format!("{} = extractvalue %JrgSlice {}, 1", ll, lval));
                let rl = self.fresh("srl.");
                self.emit(format!("{} = extractvalue %JrgSlice {}, 1", rl, rval));
                let sama_panjang = self.fresh("slc.");
                self.emit(format!("{} = icmp eq i64 {}, {}", sama_panjang, ll, rl));
                let sp8 = self.from_i1(&sama_panjang);
                self.dan_ke_slot(slot, &sp8);

                let panjang = Val { text: ll, ty: JrgType::I64 };
                self.bandingkan_berurut(slot, ty, lval, rval, elem, &panjang, span)
            }

            // Enum: bandingkan TAG dulu, lalu payload variant itu saja.
            JrgType::Any => {
                let lt = self.tag_any(&Val { text: lval.to_string(), ty: JrgType::Any });
                let rt = self.tag_any(&Val { text: rval.to_string(), ty: JrgType::Any });
                let l_gab = self.fresh_label("anyeq.gab.");
                // Bulat lawan pecahan: tagnya beda tapi nilainya terbanding, seperti di dua mesin lain.
                let mut angka = Vec::new();
                for (nilai, tag) in [(lval, &lt.text), (rval, &rt.text)] {
                    let tmp = self.alloca_typed("anyeqn.", &JrgType::Any);
                    self.emit(format!("store %JrgAny {}, ptr {}", nilai, tmp));
                    let (mp, w, bulat, pecah, fi, ff, d) = (self.fresh("aqm."), self.fresh("aqw."), self.fresh("aqb."),
                        self.fresh("aqp."), self.fresh("aqi."), self.fresh("aqf."), self.fresh("aqd."));
                    self.emit(format!("{} = getelementptr %JrgAny, ptr {}, i32 0, i32 1", mp, tmp));
                    self.emit(format!("{} = load i64, ptr {}", w, mp));
                    self.emit(format!("{} = icmp eq i32 {}, {}", bulat, tag, super::codegen_any::TAG_INT));
                    self.emit(format!("{} = icmp eq i32 {}, {}", pecah, tag, super::codegen_any::TAG_FLOAT));
                    self.emit(format!("{} = sitofp i64 {} to double", fi, w));
                    self.emit(format!("{} = bitcast i64 {} to double", ff, w));
                    self.emit(format!("{} = select i1 {}, double {}, double {}", d, bulat, fi, ff));
                    angka.push((bulat, pecah, d));
                }
                let (c1, c2, campur) = (self.fresh("aqc."), self.fresh("aqc."), self.fresh("aqc."));
                self.emit(format!("{} = and i1 {}, {}", c1, angka[0].0, angka[1].1));
                self.emit(format!("{} = and i1 {}, {}", c2, angka[0].1, angka[1].0));
                self.emit(format!("{} = or i1 {}, {}", campur, c1, c2));
                let (l_camp, l_biasa) = (self.fresh_label("anyeq.camp."), self.fresh_label("anyeq.biasa."));
                self.terminate(format!("br i1 {}, label %{}, label %{}", campur, l_camp, l_biasa));
                self.start_block(l_camp);
                let fs = self.fresh("aqs.");
                self.emit(format!("{} = fcmp oeq double {}, {}", fs, angka[0].2, angka[1].2));
                let fs8 = self.from_i1(&fs);
                self.dan_ke_slot(slot, &fs8);
                self.terminate(format!("br label %{}", l_gab));
                self.start_block(l_biasa);
                let sama_tag = self.fresh("anyeqt.");
                self.emit(format!("{} = icmp eq i32 {}, {}", sama_tag, lt.text, rt.text));
                let st8 = self.from_i1(&sama_tag);
                self.dan_ke_slot(slot, &st8);

                let l_isi = self.fresh_label("anyeq.isi.");
                self.terminate(format!(
                    "br i1 {}, label %{}, label %{}", sama_tag, l_isi, l_gab));

                self.start_block(l_isi);
                use super::codegen_any::{TAG_STR, TAG_FLOAT, TAG_SLICE, TAG_DICT, TAG_BURAM};
                let l_bur = self.fresh_label("anyeq.bur.");
                let l_str = self.fresh_label("anyeq.str.");
                let l_num = self.fresh_label("anyeq.num.");
                let l_flt = self.fresh_label("anyeq.flt.");
                let l_tolak = self.fresh_label("anyeq.tolak.");
                self.terminate(format!(
                    "switch i32 {}, label %{} [ i32 {}, label %{} i32 {}, label %{} \
                     i32 {}, label %{} i32 {}, label %{} i32 {}, label %{} ]",
                    lt.text, l_num, TAG_STR, l_str, TAG_FLOAT, l_flt,
                    TAG_SLICE, l_tolak, TAG_DICT, l_tolak, TAG_BURAM, l_bur));

                let lv = Val { text: lval.to_string(), ty: JrgType::Any };
                let rv = Val { text: rval.to_string(), ty: JrgType::Any };

                self.start_block(l_str);
                let ls = self.buka_any(&lv, &JrgType::Str, span)?;
                let rs = self.buka_any(&rv, &JrgType::Str, span)?;
                self.kumpul_sama(slot, &JrgType::Str, &ls.text, &rs.text, span)?;
                self.terminate(format!("br label %{}", l_gab));

                self.start_block(l_flt);
                let lf = self.buka_any(&lv, &JrgType::F64, span)?;
                let rf = self.buka_any(&rv, &JrgType::F64, span)?;
                self.kumpul_sama(slot, &JrgType::F64, &lf.text, &rf.text, span)?;
                self.terminate(format!("br label %{}", l_gab));

                self.start_block(l_num);
                let li = self.buka_any(&lv, &JrgType::I64, span)?;
                let ri = self.buka_any(&rv, &JrgType::I64, span)?;
                self.kumpul_sama(slot, &JrgType::I64, &li.text, &ri.text, span)?;
                self.terminate(format!("br label %{}", l_gab));

                // Tipe buram: kata-tag penuhnya harus sama, lalu isinya lewat pembanding tipenya.
                self.start_block(l_bur);
                let (lw, lp) = self.bagian_buram(&lv);
                let (rw, rp) = self.bagian_buram(&rv);
                self.pakai_sama_buram = true;
                let setipe = self.fresh("anyeqw.");
                self.emit(format!("{} = icmp eq i32 {}, {}", setipe, lw, rw));
                let (l_isi_b, l_beda) = (self.fresh_label("anyeq.buri."), self.fresh_label("anyeq.burb."));
                self.terminate(format!("br i1 {}, label %{}, label %{}", setipe, l_isi_b, l_beda));
                self.start_block(l_isi_b);
                let rb = self.fresh("anyeqb.");
                self.emit(format!("{} = call i8 @jrg_sama_buram(i32 {}, ptr {}, ptr {})", rb, lw, lp, rp));
                self.dan_ke_slot(slot, &Val { text: rb, ty: JrgType::Bool });
                self.terminate(format!("br label %{}", l_gab));
                // Dua tuple sama panjang tapi beda tipe elemen bisa saja sama isinya (`(1, 2)` lawan
                // `(1.0, 2.0)`): belum bisa dibandingkan di sini, jadi ditolak, bukan dijawab `false`.
                self.start_block(l_beda);
                let (la, ra, sp, ad, dua) = (self.fresh("aqa."), self.fresh("aqa."), self.fresh("aqa."), self.fresh("aqa."), self.fresh("aqa."));
                self.emit(format!("{} = call i32 @jrg_buram_aritas(i32 {})", la, lw));
                self.emit(format!("{} = call i32 @jrg_buram_aritas(i32 {})", ra, rw));
                self.emit(format!("{} = icmp eq i32 {}, {}", sp, la, ra));
                self.emit(format!("{} = icmp ne i32 {}, 0", ad, la));
                self.emit(format!("{} = and i1 {}, {}", dua, sp, ad));
                let (l_tup, l_lain) = (self.fresh_label("anyeq.tup."), self.fresh_label("anyeq.lain."));
                self.terminate(format!("br i1 {}, label %{}, label %{}, !prof !0", dua, l_tup, l_lain));
                self.start_block(l_tup);
                self.panik("membandingkan dua tuple yang tipe elemennya berbeda di tempat campur tipe belum didukung backend --emit-llvm");
                self.start_block(l_lain);
                self.emit(format!("store i8 0, ptr {}", slot));
                self.terminate(format!("br label %{}", l_gab));

                self.start_block(l_tolak);
                self.panik(
                    "membandingkan array/dict yang tersimpan di tempat campur tipe belum didukung backend --emit-llvm");
                // `panik` sudah menutup bloknya dengan `unreachable`, jadi tidak
                // ada cabang balik ke blok gabung dari sini.

                self.start_block(l_gab);
                Ok(())
            }

            JrgType::Enum(nama) => {
                let layout = self.types.enums.get(nama).cloned().ok_or_else(|| {
                    super::galat::tandai(span, format!("enum `{}` tidak dikenal", nama))
                })?;
                let lp = self.alloca_typed("eqle.", ty);
                self.emit(format!("store {} {}, ptr {}", ty.llvm_ty(), lval, lp));
                let rp = self.alloca_typed("eqre.", ty);
                self.emit(format!("store {} {}, ptr {}", ty.llvm_ty(), rval, rp));

                let ltp = self.fresh("eqlt.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 0", ltp, ty.llvm_ty(), lp));
                let lt = self.fresh("eqltv.");
                self.emit(format!("{} = load i32, ptr {}", lt, ltp));
                let rtp = self.fresh("eqrt.");
                self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 0", rtp, ty.llvm_ty(), rp));
                let rt = self.fresh("eqrtv.");
                self.emit(format!("{} = load i32, ptr {}", rt, rtp));
                let sama_tag = self.fresh("eqet.");
                self.emit(format!("{} = icmp eq i32 {}, {}", sama_tag, lt, rt));
                let st8 = self.from_i1(&sama_tag);
                self.dan_ke_slot(slot, &st8);

                let berpayload: Vec<_> = layout.variants.iter()
                    .filter(|v| !v.payload.field_types().is_empty())
                    .cloned().collect();
                if berpayload.is_empty() {
                    return Ok(());
                }

                let pilih_lbl = self.fresh_label("eq.enum.pilih.");
                let gabung_lbl = self.fresh_label("eq.enum.gabung.");
                self.terminate(format!(
                    "br i1 {}, label %{}, label %{}", sama_tag, pilih_lbl, gabung_lbl));

                self.start_block(pilih_lbl);
                let label_var: Vec<String> = berpayload.iter()
                    .map(|v| self.fresh_label(&format!("eq.enum.{}.", v.name.to_lowercase())))
                    .collect();
                let kasus: Vec<String> = berpayload.iter().zip(&label_var)
                    .map(|(v, l)| format!("i32 {}, label %{}", v.tag, l))
                    .collect();
                self.terminate(format!(
                    "switch i32 {}, label %{} [ {} ]", lt, gabung_lbl, kasus.join(" ")));

                for (v, lbl) in berpayload.iter().zip(&label_var) {
                    self.start_block(lbl.clone());
                    let ps = v.payload.as_llvm_struct();
                    let lb = self.fresh("eqlb.");
                    self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", lb, ty.llvm_ty(), lp));
                    let rb = self.fresh("eqrb.");
                    self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", rb, ty.llvm_ty(), rp));
                    for (i, fty) in v.payload.field_types().iter().enumerate() {
                        let lfp = self.fresh("eqlf.");
                        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", lfp, ps, lb, i));
                        let lf = self.fresh("eqlfv.");
                        self.emit(format!("{} = load {}, ptr {}", lf, fty.llvm_ty(), lfp));
                        let rfp = self.fresh("eqrf.");
                        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 {}", rfp, ps, rb, i));
                        let rf = self.fresh("eqrfv.");
                        self.emit(format!("{} = load {}, ptr {}", rf, fty.llvm_ty(), rfp));
                        self.kumpul_sama(slot, fty, &lf, &rf, span)?;
                    }
                    self.terminate(format!("br label %{}", gabung_lbl));
                }

                self.start_block(gabung_lbl);
                Ok(())
            }

            // Sisa tipe sudah ditangani `cmp_binop` sebelum sampai ke sini,
            // tapi bisa muncul sebagai ELEMEN sebuah agregat, jadi jalurnya
            // dipanggil balik — bukan diduplikasi.
            _ => {
                let l = Val { text: lval.to_string(), ty: ty.clone() };
                let r = Val { text: rval.to_string(), ty: ty.clone() };
                let c = self.cmp_binop_pub_entry(&BinaryOp::Eq, &l, &r, span)?;
                self.dan_ke_slot(slot, &c);
                Ok(())
            }
        }
    }

    /// `extractvalue` satu komponen agregat pada indeks tetap.
    fn ambil_elemen(&mut self, agg_ty: &JrgType, agg: &str, idx: usize, _elem_ty: &JrgType) -> String {
        let r = self.fresh("eqel.");
        self.emit(format!(
            "{} = extractvalue {} {}, {}",
            r, agg_ty.llvm_ty(), agg, idx
        ));
        r
    }

    /// Bandingkan `panjang` elemen berurutan dari dua agregat, lewat loop.
    fn bandingkan_berurut(
        &mut self,
        slot: &str,
        agg_ty: &JrgType,
        lval: &str,
        rval: &str,
        elem: &JrgType,
        panjang: &Val,
        span: Span,
    ) -> Result<(), String> {
        let (lbase, rbase) = match agg_ty {
            JrgType::Slice(_) => {
                let lp = self.fresh("slp.");
                self.emit(format!("{} = extractvalue %JrgSlice {}, 0", lp, lval));
                let rp = self.fresh("srp.");
                self.emit(format!("{} = extractvalue %JrgSlice {}, 0", rp, rval));
                (lp, rp)
            }
            _ => {
                let ls = self.alloca_typed("eqls.", agg_ty);
                self.emit(format!("store {} {}, ptr {}", agg_ty.llvm_ty(), lval, ls));
                let rs = self.alloca_typed("eqrs.", agg_ty);
                self.emit(format!("store {} {}, ptr {}", agg_ty.llvm_ty(), rval, rs));
                (ls, rs)
            }
        };

        let iv = self.alloca_raw("eqi.", "i64", Some(8));
        self.emit(format!("store i64 0, ptr {}", iv));

        let cond_lbl = self.fresh_label("eq.cond.");
        let body_lbl = self.fresh_label("eq.body.");
        let end_lbl = self.fresh_label("eq.end.");
        self.terminate(format!("br label %{}", cond_lbl));

        self.start_block(cond_lbl.clone());
        let i = self.fresh("eqiv.");
        self.emit(format!("{} = load i64, ptr {}", i, iv));
        let belum_habis = self.fresh("eqlt.");
        self.emit(format!("{} = icmp slt i64 {}, {}", belum_habis, i, panjang.text));
        // Keluar juga kalau sudah terbukti berbeda.
        let acc = self.fresh("eqa.");
        self.emit(format!("{} = load i8, ptr {}", acc, slot));
        let masih_sama = self.fresh("eqas.");
        self.emit(format!("{} = icmp eq i8 {}, 1", masih_sama, acc));
        let lanjut = self.fresh("eqgo.");
        self.emit(format!("{} = and i1 {}, {}", lanjut, belum_habis, masih_sama));
        self.terminate(format!("br i1 {}, label %{}, label %{}", lanjut, body_lbl, end_lbl));

        self.start_block(body_lbl);
        let lep = self.fresh("eqlp.");
        let rep = self.fresh("eqrp.");
        match agg_ty {
            // Slice: `lbase` sudah pointer ke elemen pertama.
            JrgType::Slice(_) => {
                self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", lep, elem.llvm_ty(), lbase, i));
                self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", rep, elem.llvm_ty(), rbase, i));
            }
            // Array tetap: `lbase` pointer ke seluruh array, jadi butuh
            // indeks nol dulu untuk masuk ke dalamnya.
            _ => {
                self.emit(format!("{} = getelementptr {}, ptr {}, i64 0, i64 {}", lep, agg_ty.llvm_ty(), lbase, i));
                self.emit(format!("{} = getelementptr {}, ptr {}, i64 0, i64 {}", rep, agg_ty.llvm_ty(), rbase, i));
            }
        }
        let le = self.fresh("eqle.");
        self.emit(format!("{} = load {}, ptr {}", le, elem.llvm_ty(), lep));
        let re = self.fresh("eqre.");
        self.emit(format!("{} = load {}, ptr {}", re, elem.llvm_ty(), rep));

        self.kumpul_sama(slot, elem, &le, &re, span)?;

        let inext = self.fresh("eqin.");
        self.emit(format!("{} = add i64 {}, 1", inext, i));
        self.emit(format!("store i64 {}, ptr {}", inext, iv));
        if !self.is_terminated() {
            self.terminate(format!("br label %{}", cond_lbl));
        }

        self.start_block(end_lbl);
        Ok(())
    }
}
