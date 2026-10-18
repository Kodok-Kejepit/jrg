// src/llvm/codegen_hof.rs — llvm/codegen_hof.rs — method tingkat-tinggi atas array dan slice.

use crate::lexer::Span;
use crate::parser::Expr;

use super::codegen::{Codegen, Val};
use super::types::JrgType;

impl Codegen {
    /// Layani method tingkat-tinggi atas koleksi. `None` kalau namanya bukan
    /// salah satunya — pemanggilnya lanjut ke penanganan lain.
    pub fn coba_hof_koleksi(
        &mut self, obj: &Val, method: &str, args: &[Expr], span: Span,
    ) -> Option<Result<Val, String>> {
        let elem = match &obj.ty {
            JrgType::Array(e, _) | JrgType::Slice(e) => (**e).clone(),
            _ => return None,
        };
        // Arity diperiksa sebelum dispatch: `None` di sini berarti "bukan method
        // ini", bukan "argumennya salah".
        let arity = match method {
            "map" | "filter" | "find" | "each" | "any" | "all" => 1,
            "reduce" | "fold" => 2,
            _ => return None,
        };
        if args.len() != arity {
            return Some(Err(super::galat::tandai(span,
                crate::interpreter::pesan_hof_method_argumen(method, args.len()))));
        }
        Some(self.hof(obj, &elem, method, args, span))
    }

    fn hof(&mut self, obj: &Val, elem: &JrgType, method: &str, args: &[Expr], span: Span)
        -> Result<Val, String>
    {
        // Untuk `reduce`, parameter pertama closure bertipe akumulator —
        // yang datang dari nilai awalnya, jadi itu diturunkan lebih dulu.
        // `reduce(f, awal)` dan `fold(awal, f)`: letak nilai awal dan fungsinya bertukar.
        let (i_fungsi, i_awal) = if method == "fold" { (1, 0) } else { (0, 1) };
        let awal = if matches!(method, "reduce" | "fold") {
            Some(self.codegen_expr(&args[i_awal], None)?)
        } else {
            None
        };
        let ptys = match &awal {
            Some(a) => vec![a.ty.clone(), elem.clone()],
            None => vec![elem.clone()],
        };
        let f = self.turunkan_fungsi_arg(&args[i_fungsi], &ptys, span)?;
        let JrgType::Fn(_, fret) = f.ty.clone() else {
            return Err(super::galat::tandai(span, format!(
                "`{}` butuh sebuah fungsi, diberi `{}`", method, f.ty.display_name())));
        };
        let fret = *fret;

        let tempat = self.spill_to_place(obj);
        let (data, n) = self.data_dan_panjang(&tempat, &obj.ty, elem);
        match method {
            "each" => {
                let f = f.clone();
                self.loop_hof(&data, &n, elem, |cg, v| {
                    cg.panggil_val(&f, &[v])?;
                    Ok(())
                })?;
                Ok(Val::unit())
            }
            "map" | "filter" => {
                let hasil_elem = if method == "map" { fret.clone() } else { elem.clone() };
                let buf = self.buffer_baru(&n, &hasil_elem);
                let buf_k = buf.clone();
                let cacah = self.alloca_raw("hofc.", "i64", Some(8));
                let cacah_k = cacah.clone();
                self.emit(format!("store i64 0, ptr {}", cacah));
                let f = f.clone();
                let he = hasil_elem.clone();
                let is_map = method == "map";
                self.loop_hof(&data, &n, elem, move |cg, v| {
                    let r = cg.panggil_val(&f, &[v.clone()])?;
                    let (simpan, isi) = if is_map { (true, r) } else {
                        let b = cg.to_i1(&r);
                        let ya = cg.fresh_label("hof.ya.");
                        let usai = cg.fresh_label("hof.usai.");
                        cg.terminate(format!("br i1 {}, label %{}, label %{}", b, ya, usai));
                        cg.start_block(ya);
                        let c = cg.load_place(&cacah_k, &JrgType::I64);
                        let p = cg.fresh("hofp.");
                        cg.emit(format!("{} = getelementptr {}, ptr {}, i64 {}",
                            p, he.llvm_ty(), buf_k, c.text));
                        if he != JrgType::Unit {
                            cg.emit(format!("store {} {}, ptr {}", he.llvm_ty(), v.text, p));
                        }
                        let c1 = cg.fresh("hofc1.");
                        cg.emit(format!("{} = add i64 {}, 1", c1, c.text));
                        cg.emit(format!("store i64 {}, ptr {}", c1, cacah_k));
                        if !cg.is_terminated() { cg.terminate(format!("br label %{}", usai)); }
                        cg.start_block(usai);
                        (false, v)
                    };
                    if simpan {
                        let c = cg.load_place(&cacah_k, &JrgType::I64);
                        let p = cg.fresh("hofp.");
                        cg.emit(format!("{} = getelementptr {}, ptr {}, i64 {}",
                            p, he.llvm_ty(), buf_k, c.text));
                        if he != JrgType::Unit {
                            cg.emit(format!("store {} {}, ptr {}", he.llvm_ty(), isi.text, p));
                        }
                        let c1 = cg.fresh("hofc1.");
                        cg.emit(format!("{} = add i64 {}, 1", c1, c.text));
                        cg.emit(format!("store i64 {}, ptr {}", c1, cacah_k));
                    }
                    Ok(())
                })?;
                let c = self.load_place(&cacah, &JrgType::I64);
                Ok(self.rakit_slice(&buf, &c.text, &hasil_elem))
            }
            "reduce" | "fold" => {
                let awal = awal.expect("`reduce` selalu menurunkan nilai awalnya");
                let akum = self.alloca_typed("hofa.", &awal.ty);
                let akum_k = akum.clone();
                if awal.ty != JrgType::Unit {
                    self.emit(format!("store {} {}, ptr {}", awal.ty.llvm_ty(), awal.text, akum));
                }
                let aty = awal.ty.clone();
                let f = f.clone();
                self.loop_hof(&data, &n, elem, move |cg, v| {
                    let a = cg.load_place(&akum_k, &aty);
                    let r = cg.panggil_val(&f, &[a, v])?;
                    let r = cg.coerce(&r, &aty);
                    if aty != JrgType::Unit {
                        cg.emit(format!("store {} {}, ptr {}", aty.llvm_ty(), r.text, akum_k));
                    }
                    Ok(())
                })?;
                Ok(self.load_place(&akum, &awal.ty))
            }
            "any" | "all" => {
                // `all` mulai dari benar dan dipatahkan satu yang salah;
                // `any` sebaliknya. Satu slot, dua nilai awal.
                let semua = method == "all";
                let slot = self.alloca_raw("hofs.", "i8", Some(1));
                let slot_k = slot.clone();
                self.emit(format!("store i8 {}, ptr {}", if semua { 1 } else { 0 }, slot));
                let f = f.clone();
                self.loop_hof(&data, &n, elem, move |cg, v| {
                    let r = cg.panggil_val(&f, &[v])?;
                    let b = cg.to_i1(&r);
                    let ubah = cg.fresh_label("hof.ubah.");
                    let usai = cg.fresh_label("hof.usai.");
                    if semua {
                        cg.terminate(format!("br i1 {}, label %{}, label %{}", b, usai, ubah));
                    } else {
                        cg.terminate(format!("br i1 {}, label %{}, label %{}", b, ubah, usai));
                    }
                    cg.start_block(ubah);
                    cg.emit(format!("store i8 {}, ptr {}", if semua { 0 } else { 1 }, slot_k));
                    if !cg.is_terminated() { cg.terminate(format!("br label %{}", usai)); }
                    cg.start_block(usai);
                    Ok(())
                })?;
                Ok(self.load_place(&slot, &JrgType::Bool))
            }
            "find" => {
                // Hasilnya `Option`, bukan nilai telanjang: itu aturan
                // seluruh bahasa untuk apa pun yang mungkin tidak menemukan.
                let en = self.instantiate_option(elem);
                let oty = JrgType::Enum(en.clone());
                let slot = self.alloca_typed("hoff.", &oty);
                let kosong = self.build_enum_unit_variant(&en, "None")?;
                self.emit(format!("store {} {}, ptr {}", oty.llvm_ty(), kosong.text, slot));
                let ketemu = self.alloca_raw("hofk.", "i8", Some(1));
                self.emit(format!("store i8 0, ptr {}", ketemu));
                let ketemu_k = ketemu.clone();
                let slot_k = slot.clone();
                let f = f.clone();
                let el = elem.clone();
                let oty2 = oty.clone();
                self.loop_hof(&data, &n, elem, move |cg, v| {
                    let sudah = cg.load_place(&ketemu_k, &JrgType::Bool);
                    let s1 = cg.fresh("hofs1.");
                    cg.emit(format!("{} = trunc i8 {} to i1", s1, sudah.text));
                    let cek = cg.fresh_label("hof.cek.");
                    let usai = cg.fresh_label("hof.usai.");
                    cg.terminate(format!("br i1 {}, label %{}, label %{}", s1, usai, cek));

                    cg.start_block(cek);
                    let r = cg.panggil_val(&f, &[v.clone()])?;
                    let b = cg.to_i1(&r);
                    let isi = cg.fresh_label("hof.isi.");
                    cg.terminate(format!("br i1 {}, label %{}, label %{}", b, isi, usai));

                    cg.start_block(isi);
                    let layout = cg.types.enums.get(&en).cloned()
                    .ok_or_else(|| format!("enum `{}` tidak dikenal", en))?;
                let varian = layout.variant("Some").cloned()
                    .ok_or_else(|| format!("enum `{}` tidak punya variant `Some`", en))?;
                let _ = &el;
                let some = cg.build_enum_value(&en, &layout, &varian, &[v])?;
                    cg.emit(format!("store {} {}, ptr {}", oty2.llvm_ty(), some.text, slot_k));
                    cg.emit(format!("store i8 1, ptr {}", ketemu_k));
                    if !cg.is_terminated() { cg.terminate(format!("br label %{}", usai)); }
                    cg.start_block(usai);
                    Ok(())
                })?;
                Ok(self.load_place(&slot, &oty))
            }
            lain => Err(super::galat::tandai(span, format!(
                "method tingkat-tinggi `{}` belum didukung backend LLVM v1", lain))),
        }
    }

    /// Turunkan argumen fungsi sebuah HOF, dengan tipe parameternya sudah
    /// diketahui tapi tipe hasilnya belum.
    fn turunkan_fungsi_arg(&mut self, e: &Expr, ptys: &[JrgType], span: Span)
        -> Result<Val, String>
    {
        // `is_move` DIBACA, bukan dibuang lewat `..`.
        if let Expr::Closure { params, body, is_move, .. } = e {
            if params.len() != ptys.len() {
                return Err(super::galat::tandai(span, format!(
                    "fungsi di sini butuh {} parameter, closure ini punya {}",
                    ptys.len(), params.len())));
            }
            return self.closure_dgn_params(params, body, ptys, *is_move, span);
        }
        // Bukan closure literal — sebuah nama fungsi, atau ekspresi apa pun
        // yang menghasilkan nilai fungsi.
        let v = self.codegen_expr(e, None)?;
        if !matches!(v.ty, JrgType::Fn(..)) {
            return Err(super::galat::tandai(span, format!(
                "butuh sebuah fungsi di sini, diberi `{}`", v.ty.display_name())));
        }
        Ok(v)
    }

    /// Loop atas `n` elemen mulai dari `ptr`. Sama bentuknya dengan
    /// `loop_koleksi` di `codegen_arr.rs`, tapi badannya boleh membuat blok
    /// sendiri — jadi ia membaca ulang indeksnya sesudah badan berjalan.
    fn loop_hof<F>(&mut self, ptr: &str, len: &str, elem: &JrgType, mut badan: F)
        -> Result<(), String>
    where
        F: FnMut(&mut Self, Val) -> Result<(), String>,
    {
        let idx = self.alloca_raw("hi.", "i64", Some(8));
        self.emit(format!("store i64 0, ptr {}", idx));
        let (cond, body, end) = (self.fresh_label("hof.cond."), self.fresh_label("hof.body."),
                                 self.fresh_label("hof.end."));
        self.terminate(format!("br label %{}", cond));

        self.start_block(cond.clone());
        let i = self.load_place(&idx, &JrgType::I64);
        let lanjut = self.fresh("hlc.");
        self.emit(format!("{} = icmp slt i64 {}, {}", lanjut, i.text, len));
        self.terminate(format!("br i1 {}, label %{}, label %{}", lanjut, body, end));

        self.start_block(body);
        let ep = self.fresh("hlp.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", ep, elem.llvm_ty(), ptr, i.text));
        let nilai = self.load_place(&ep, elem);
        badan(self, nilai)?;
        // Indeksnya dibaca ULANG dari slotnya, bukan dipakai `i` di atas:
        // badan HOF membuat blok sendiri (cabang `filter`, keluar awal
        // `find`), jadi register `i` tidak lagi mendominasi tempat ini.
        let i2 = self.load_place(&idx, &JrgType::I64);
        let inext = self.fresh("hln.");
        self.emit(format!("{} = add i64 {}, 1", inext, i2.text));
        self.emit(format!("store i64 {}, ptr {}", inext, idx));
        if !self.is_terminated() { self.terminate(format!("br label %{}", cond)); }

        self.start_block(end);
        Ok(())
    }
}
