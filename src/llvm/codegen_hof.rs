// src/llvm/codegen_hof.rs — llvm/codegen_hof.rs — method tingkat-tinggi atas array dan slice.

use crate::lexer::Span;
use crate::parser::Expr;

use super::codegen::{Codegen, Val};
use super::types::JrgType;
use crate::parser::Pattern;

/// Fungsi yang diserahkan ke HOF: nilainya, atau kalimat gagal saat dipanggil.
#[derive(Clone)]
struct HofFn { f: Result<Val, String>, ret: JrgType, span: Span }

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
        // Yang bukan fungsi gagal saat DIPANGGIL, seperti acuan: koleksi kosong tidak pernah gagal.
        let fret = match &f {
            Ok(v) => match &v.ty {
                JrgType::Fn(_, r) => (**r).clone(),
                lain => return Err(super::galat::tandai(span, format!(
                    "`{}` butuh sebuah fungsi, diberi `{}`", method, lain.display_name()))),
            },
            Err(_) => match method {
                "map" => JrgType::Any,
                "each" => JrgType::Unit,
                "reduce" | "fold" => awal.as_ref().map(|a| a.ty.clone()).unwrap_or(JrgType::Any),
                _ => JrgType::Bool,
            },
        };
        let f = HofFn { f, ret: fret.clone(), span };

        let tempat = self.spill_to_place(obj);
        let (data, n) = self.data_dan_panjang(&tempat, &obj.ty, elem);
        match method {
            "each" => {
                let f = f.clone();
                self.loop_hof(&data, &n, elem, |cg, v| {
                    cg.panggil_hof(&f, &[v])?;
                    Ok(())
                })?;
                Ok(Val::unit())
            }
            "map" | "filter" => {
                // Hasil `map` berupa array berukuran tetap disimpan sebagai slice, seperti `push`:
                // `arr::map(a, |x| ["=", x])` lalu bisa disambung dengan array yang tumbuh.
                let hasil_elem = if method == "map" { super::codegen_any::luruh(&fret) } else { elem.clone() };
                let buf = self.buffer_baru(&n, &hasil_elem);
                let buf_k = buf.clone();
                let cacah = self.alloca_raw("hofc.", "i64", Some(8));
                let cacah_k = cacah.clone();
                self.emit(format!("store i64 0, ptr {}", cacah));
                let f = f.clone();
                let he = hasil_elem.clone();
                let is_map = method == "map";
                self.loop_hof(&data, &n, elem, move |cg, v| {
                    let r = cg.panggil_hof(&f, &[v.clone()])?;
                    let (simpan, isi) = if is_map { (true, cg.coerce(&r, &he)) } else {
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
                    let r = cg.panggil_hof(&f, &[a, v])?;
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
                    let r = cg.panggil_hof(&f, &[v])?;
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
                    let r = cg.panggil_hof(&f, &[v.clone()])?;
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
    /// `Err` di dalam berarti kalimat acuan yang diucapkan saat fungsinya dipanggil.
    fn turunkan_fungsi_arg(&mut self, e: &Expr, ptys: &[JrgType], span: Span)
        -> Result<Result<Val, String>, String>
    {
        // `is_move` DIBACA, bukan dibuang lewat `..`.
        if let Expr::Closure { params, body, is_move, .. } = e {
            if params.len() != ptys.len() {
                let polos = params.iter().all(|p| p.default.is_none() && !p.is_variadic);
                if !polos {
                    return Err(super::galat::tandai(span, format!(
                        "fungsi di sini butuh {} parameter, closure ini punya {}",
                        ptys.len(), params.len())));
                }
                let nama: Vec<String> = params.iter().map(|p| match &p.pattern {
                    Pattern::Ident { name, .. } => name.clone(),
                    _ => String::new(),
                }).collect();
                return Ok(Err(Self::pesan_aritas_hof("<closure>", &nama, ptys.len())));
            }
            return self.closure_dgn_params(params, body, ptys, *is_move, span).map(Ok);
        }
        // Nama builtin (`xs.filter(str::is_numeric)`, `xs.map(abs)`): closure `<native fn>` dengan tipe
        // parameter dari elemennya, jalur yang sama dengan closure literal.
        let bawaan = match e {
            Expr::Ident(n, _) if self.lookup_local(n).is_none() && !self.tables.functions.contains_key(&self.k(n)) => Some(n.clone()),
            Expr::PathExpr { segments, .. } if segments.len() == 2 && self.unit_alias(&segments[0]).is_none() => Some(segments.join("::")),
            _ => None,
        };
        if let Some(Expr::Closure { params, body, .. }) = bawaan.and_then(|n| self.closure_bawaan(&n, span, Some(ptys.len()))) {
            if params.len() == ptys.len() {
                self.closure_bernama = Some("<native fn>".to_string());
                let r = self.closure_dgn_params(&params, &body, ptys, false, span);
                self.closure_bernama = None;
                return r.map(Ok);
            }
        }
        // Bukan closure literal — sebuah nama fungsi, atau ekspresi apa pun
        // yang menghasilkan nilai fungsi.
        let v = self.codegen_expr(e, None)?;
        match &v.ty {
            JrgType::Fn(ps, _) if ps.len() != ptys.len() => {
                let decl = match e {
                    Expr::Ident(n, _) if self.lookup_local(n).is_none() =>
                        self.tables.functions.get(&self.k(n)).cloned().map(|d| (n.clone(), d)),
                    _ => None,
                };
                let Some((n, d)) = decl.filter(|(_, d)| d.params.iter().all(|p| p.default.is_none() && !p.is_variadic))
                else { return Ok(Ok(v)); };
                let nama: Vec<String> = d.params.iter().map(|p| match &p.pattern {
                    Pattern::Ident { name, .. } => name.clone(),
                    _ => String::new(),
                }).collect();
                Ok(Err(Self::pesan_aritas_hof(&n, &nama, ptys.len())))
            }
            JrgType::Fn(..) => Ok(Ok(v)),
            lain => Ok(Err(crate::interpreter::pesan_bukan_fungsi(&lain.nama_typeof()))),
        }
    }

    /// Kalimat acuan untuk fungsi yang dipanggil dengan `diberi` argumen.
    fn pesan_aritas_hof(fnama: &str, param: &[String], diberi: usize) -> String {
        if param.len() > diberi {
            crate::interpreter::pesan_parameter_wajib(fnama, &param[diberi], diberi + 1)
        } else {
            crate::interpreter::pesan_fungsi_kebanyakan(fnama, param.len(), diberi, param)
        }
    }

    /// Memanggil fungsi sebuah HOF, atau gagal di situ dengan kalimat acuan.
    fn panggil_hof(&mut self, f: &HofFn, args: &[Val]) -> Result<Val, String> {
        match &f.f {
            Ok(v) => self.panggil_val(v, args),
            Err(pesan) => {
                let lama = self.span_kini.replace(f.span);
                let r = self.gagal_sesudah_argumen(pesan, &[]);
                self.span_kini = lama;
                r?;
                Ok(if f.ret == JrgType::Unit { Val::unit() } else { Val { text: "undef".into(), ty: f.ret.clone() } })
            }
        }
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
