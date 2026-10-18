// src/llvm/codegen_arr.rs — llvm/codegen_arr.rs — namespace `arr::` untuk backend LLVM.

use crate::lexer::Span;
use crate::parser::{BinaryOp, Expr};

use super::codegen::{Codegen, Val};
use super::types::JrgType;

impl Codegen {
    /// Layani `arr::…`. `None` kalau namanya bukan milik namespace ini.
    pub fn coba_builtin_arr(
        &mut self,
        nama: &str,
        args: &[Expr],
        span: Span,
    ) -> Option<Result<Val, String>> {
        let item = nama.strip_prefix("arr::")?;

        // Bentuk namespace HOF (`arr::map(a, f)`) dialihkan ke bentuk method yang sudah ada di
        // backend ini — satu implementasi, dua pintu, seperti di dua mesin acuan.
        if matches!(item, "map" | "filter" | "find" | "each" | "any" | "all" | "reduce" | "fold") {
            let perlu = if matches!(item, "reduce" | "fold") { 3 } else { 2 };
            if args.len() < perlu {
                return Some(Err(super::galat::tandai(span, crate::interpreter::pesan_hof_kurang(nama))));
            }
            let (koleksi, _) = match self.arg_koleksi(&args[0], item, span) {
                Ok(x) => x,
                Err(e) => return Some(Err(e)),
            };
            return self.coba_hof_koleksi(&koleksi, item, &args[1..], span);
        }

        // `arr::pop(a)`, `arr::first(a)`, … : pintu namespace dari method yang sama.
        let aritas_opsi = match item { "pop" | "first" | "last" => Some(1), "get" => Some(2), "get_or" => Some(3), _ => None };
        if let Some(n) = aritas_opsi {
            if args.len() != n {
                return Some(Err(super::galat::tandai(span, format!(
                    "`arr::{}` butuh {} argumen, diberi {}", item, n, args.len()))));
            }
            let (koleksi, elem) = match self.arg_koleksi(&args[0], item, span) {
                Ok(x) => x,
                Err(e) => return Some(Err(e)),
            };
            if item == "pop" { return Some(self.metode_pop(&koleksi, &elem, span)); }
            return self.metode_opsi_koleksi(&koleksi, &elem, item, &args[1..], span);
        }

        if item == "swap" {
            if args.len() != 3 {
                return Some(Err(super::galat::tandai(span, format!(
                    "`arr::swap` butuh 3 argumen, diberi {}", args.len()))));
            }
            return Some(self.arr_tukar(args, span));
        }
        if matches!(item, "take" | "skip" | "drop") {
            if args.len() != 2 {
                return Some(Err(super::galat::tandai(span, format!(
                    "`arr::{}` butuh 2 argumen, diberi {}", item, args.len()))));
            }
            return Some(self.arr_ambil(item, args, span));
        }

        // Arity diperiksa SEBELUM dispatch, bukan di dalam tiap lengan.
        let arity = match item {
            "push" | "contains" | "concat" | "__sebar" => 2,
            "sum" | "product" | "reverse" | "sort" | "min" | "max" => 1,
            _ => return None,
        };
        if args.len() != arity {
            return Some(Err(super::galat::tandai(span, format!(
                "`arr::{}` butuh {} argumen, diberi {}", item, arity, args.len()))));
        }

        Some(match item {
            "push" => self.arr_push(args, span),
            "concat" => self.arr_concat(args, span),
            "__sebar" => self.arr_sebar(args, span),
            "sum" => self.arr_lipat(args, span, false),
            "product" => self.arr_lipat(args, span, true),
            "contains" => self.arr_contains(args, span),
            "reverse" => self.arr_reverse(args, span),
            "sort" => self.arr_sort(args, span),
            "min" => self.arr_ekstrem(args, span, true),
            "max" => self.arr_ekstrem(args, span, false),
            _ => unreachable!("arity sudah menyaring nama yang tidak dikenal"),
        })
    }

    /// Turunkan sebuah argumen dan pastikan ia koleksi. Kembalikan
    /// (nilai, tipe elemen).
    fn arg_koleksi(&mut self, e: &Expr, fungsi: &str, span: Span)
        -> Result<(Val, JrgType), String>
    {
        let v = self.codegen_expr(e, None)?;
        match &v.ty {
            JrgType::Array(el, _) | JrgType::Slice(el) => {
                let el = (**el).clone();
                Ok((v, el))
            }
            // Nilai bertag dibuka sebagai array bertag; yang bukan array gagal saat jalan.
            JrgType::Any => {
                let tujuan = JrgType::Slice(Box::new(JrgType::Any));
                match self.buka_terperiksa(&v, &tujuan) {
                    Some(buka) => Ok((buka, JrgType::Any)),
                    None => Err(super::galat::tandai(span, format!(
                        "`arr::{}` butuh array, diberi `nilai bertag`", fungsi))),
                }
            }
            other => Err(super::galat::tandai(span, format!(
                "`arr::{}` butuh array, diberi `{}`", fungsi, other.display_name()))),
        }
    }

    /// `arr::push(a, x)` — array baru dengan `x` di ujung.
    fn arr_push(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        let (koleksi, elem) = self.arg_koleksi(&args[0], "push", span)?;
        let x = self.codegen_expr(&args[1], Some(&elem))?;
        let x = self.coerce(&x, &elem);

        // Bungkus `x` jadi array satu elemen, lalu sambung.
        let satu_ty = JrgType::Array(Box::new(elem.clone()), 1);
        let slot = self.alloca_typed("push1.", &satu_ty);
        let dst = self.fresh("push1p.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 0, i64 0",
                          dst, satu_ty.llvm_ty(), slot));
        self.emit(format!("store {} {}, ptr {}", elem.llvm_ty(), x.text, dst));
        let satu = self.load_place(&slot, &satu_ty);

        self.gabung_nilai(&BinaryOp::Add, koleksi, satu, span)
    }

    /// `arr::concat(a, b)` — array baru berisi `a` lalu `b`. Penyambungannya
    /// `gabung_nilai` yang sama dengan `a + b` dan dengan `arr::push`.
    fn arr_concat(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        let (a, _) = self.arg_koleksi(&args[0], "concat", span)?;
        let (b, _) = self.arg_koleksi(&args[1], "concat", span)?;
        self.gabung_nilai(&BinaryOp::Add, a, b, span)
    }

    /// `arr::__sebar` — turunan parser untuk `[...x]`; pesannya menyebut `...`, bukan nama ini.
    fn arr_sebar(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        let mut sisi = Vec::with_capacity(2);
        for e in args {
            let v = self.codegen_expr(e, None)?;
            if !matches!(v.ty, JrgType::Array(_, _) | JrgType::Slice(_)) {
                return Err(super::galat::tandai(span,
                    crate::interpreter::pesan_sebar_bukan_array(&v.ty.nama_typeof())));
            }
            sisi.push(v);
        }
        let kanan = sisi.pop().expect("arity 2 sudah diperiksa");
        let kiri = sisi.pop().expect("arity 2 sudah diperiksa");
        self.gabung_nilai(&BinaryOp::Add, kiri, kanan, span)
    }

    /// `arr::sum(a)` — jumlah semua elemen. Nol untuk koleksi kosong.
    fn arr_lipat(&mut self, args: &[Expr], span: Span, kali: bool) -> Result<Val, String> {
        let nama = if kali { "product" } else { "sum" };
        let (koleksi, elem) = self.arg_koleksi(&args[0], nama, span)?;
        if !elem.is_numeric() {
            return Err(super::galat::tandai(span, format!(
                "`arr::{}` butuh elemen berupa angka, bukan `{}`", nama, elem.display_name())));
        }
        let (ptr, len) = self.bagian_koleksi_pub(&koleksi, &elem);
        let akum = self.alloca_typed("sum.", &elem);
        let nol = match (kali, elem.is_float()) { (false, true) => "0.0", (false, false) => "0", (true, true) => "1.0", (true, false) => "1" };
        self.emit(format!("store {} {}, ptr {}", elem.llvm_ty(), nol, akum));

        self.loop_koleksi(&ptr, &len, &elem, |cg, nilai| {
            let lama = cg.load_place(&akum, &elem);
            // Bilangan bulat dijumlah dengan luapan DIPERIKSA, seperti `+` (dulu membungkus).
            let baru = if elem.is_float() {
                let baru = cg.fresh("sumn.");
                cg.emit(format!("{} = {} {} {}, {}", baru, if kali { "fmul" } else { "fadd" }, elem.llvm_ty(), lama.text, nilai.text));
                baru
            } else {
                cg.hitung_terperiksa(if kali { &BinaryOp::Mul } else { &BinaryOp::Add }, &lama, &nilai).text
            };
            cg.emit(format!("store {} {}, ptr {}", elem.llvm_ty(), baru, akum));
            Ok(())
        })?;
        Ok(self.load_place(&akum, &elem))
    }

    /// `arr::contains(a, x)` — apakah `x` ada di dalamnya.
    fn arr_contains(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        let (koleksi, elem) = self.arg_koleksi(&args[0], "contains", span)?;
        let x = self.codegen_expr(&args[1], Some(&elem))?;
        // Bulat lawan pecahan dibandingkan sebagai pecahan, per elemen — dulu jarumnya
        // dikoersi ke tipe elemen, jadi `arr::contains([1, 2], 1.5)` menjawab `true`.
        let campur = x.ty.is_numeric() && elem.is_numeric() && x.ty.is_float() != elem.is_float();
        let x = if campur { x } else { self.coerce(&x, &elem) };
        let (ptr, len) = self.bagian_koleksi_pub(&koleksi, &elem);

        let slot = self.alloca_raw("has.", "i8", Some(1));
        self.emit("store i8 0, ptr ".to_string() + &slot);
        self.loop_koleksi(&ptr, &len, &elem, |cg, nilai| {
            let (kiri, kanan) = cg.naikkan_campuran(nilai, x.clone());
            let sama = cg.gabung_nilai(&BinaryOp::Eq, kiri, kanan, span)?;
            let lama = cg.fresh("hasl.");
            cg.emit(format!("{} = load i8, ptr {}", lama, slot));
            let baru = cg.fresh("hasn.");
            cg.emit(format!("{} = or i8 {}, {}", baru, lama, sama.text));
            cg.emit(format!("store i8 {}, ptr {}", baru, slot));
            Ok(())
        })?;
        Ok(self.load_place(&slot, &JrgType::Bool))
    }

    /// `arr::swap(a, i, j)` — salinan dengan dua elemennya ditukar (primitif C).
    fn arr_tukar(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        let (koleksi, elem) = self.arg_koleksi(&args[0], "swap", span)?;
        let mut ix = Vec::new();
        for a in &args[1..] {
            let v = self.codegen_expr(a, Some(&JrgType::I64))?;
            if !v.ty.is_int() {
                return Err(super::galat::tandai(span, crate::interpreter::pesan_bukan_bulat("arr::swap", &v.ty.display_name(), None)));
            }
            ix.push(self.coerce_longgar(&v, &JrgType::I64));
        }
        let (ptr, len) = self.bagian_koleksi_pub(&koleksi, &elem);
        // Kalimatnya dari definisi acuan; angka penanda diganti bita 1 (indeks) dan 2 (panjang).
        let luar = crate::interpreter::pesan_index_luar_batas("arr::swap", 7777777, 8888888)
            .replace("7777777", "\u{1}").replace("8888888", "\u{2}");
        let lv = self.intern_string(&luar);
        let (lp, ll) = self.split_str(&lv);
        self.ensure_extern("declare ptr @jrg_slice_tukar(ptr, i64, i64, i64, i64, i64, ptr, i64)", "jrg_slice_tukar");
        let buf = self.fresh("tukar.");
        self.emit(format!(
            "{} = call ptr @jrg_slice_tukar(ptr {}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {}, ptr {}, i64 {})",
            buf, ptr, len, ix[0].text, ix[1].text, self.types.sizeof(&elem), self.types.alignof(&elem).max(1), lp, ll));
        Ok(self.rakit_slice(&buf, &len, &elem))
    }

    /// `arr::take(a, n)` / `arr::skip(a, n)` — salinan n elemen pertama, atau sisanya.
    fn arr_ambil(&mut self, item: &str, args: &[Expr], span: Span) -> Result<Val, String> {
        let nama = format!("arr::{}", item);
        let (koleksi, elem) = self.arg_koleksi(&args[0], item, span)?;
        let n = self.codegen_expr(&args[1], Some(&JrgType::I64))?;
        if !n.ty.is_int() {
            return Err(super::galat::tandai(span, crate::interpreter::pesan_bukan_bulat(&nama, &n.ty.display_name(), None)));
        }
        let n = self.coerce_longgar(&n, &JrgType::I64);
        self.tolak_negatif(&nama, "jumlah", &n);
        let (ptr, len) = self.bagian_koleksi_pub(&koleksi, &elem);
        let lebih = self.fresh("amb.");
        self.emit(format!("{} = icmp sgt i64 {}, {}", lebih, n.text, len));
        let k = self.fresh("amb.");
        self.emit(format!("{} = select i1 {}, i64 {}, i64 {}", k, lebih, len, n.text));
        let (mulai, banyak) = if item == "take" { ("0".to_string(), k) } else {
            let sisa = self.fresh("amb.");
            self.emit(format!("{} = sub i64 {}, {}", sisa, len, k));
            (k, sisa)
        };
        let buf = self.buffer_baru(&banyak, &elem);
        let asal = self.fresh("amb.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", asal, elem.llvm_ty(), ptr, mulai));
        let byte = self.fresh("amb.");
        self.emit(format!("{} = mul i64 {}, {}", byte, banyak, self.types.sizeof(&elem)));
        self.ensure_extern("declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)", "llvm.memcpy.p0.p0.i64");
        self.emit(format!("call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)", buf, asal, byte));
        Ok(self.rakit_slice(&buf, &banyak, &elem))
    }

    /// `arr::sort` atas elemen bertag: salinan yang diurutkan primitif C, campuran ditolak saat jalan.
    fn urut_slice_any(&mut self, koleksi: &Val) -> Val {
        let (ptr, len) = self.bagian_koleksi_pub(koleksi, &JrgType::Any);
        let buf = self.buffer_baru(&len, &JrgType::Any);
        let byte = self.fresh("srta.");
        self.emit(format!("{} = mul i64 {}, {}", byte, len, self.types.sizeof(&JrgType::Any)));
        self.ensure_extern("declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)", "llvm.memcpy.p0.p0.i64");
        self.emit(format!("call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)", buf, ptr, byte));
        let pola = crate::interpreter::pesan_urut_tak_sebanding("arr::sort", "\u{1}", "\u{2}");
        let pv = self.intern_string(&pola);
        let (pp, pl) = self.split_str(&pv);
        self.ensure_extern("declare void @jrg_sort_any(ptr, i64, ptr, i64)", "jrg_sort_any");
        self.emit(format!("call void @jrg_sort_any(ptr {}, i64 {}, ptr {}, i64 {})", buf, len, pp, pl));
        self.rakit_slice(&buf, &len, &JrgType::Any)
    }

    /// `array<T>` ke `array<nilai bertag>`: tiap elemen dikotak ke penyangga baru.
    pub(crate) fn kotak_tiap_elemen(&mut self, v: &Val, elem: &JrgType) -> Option<Val> {
        super::codegen_any::tag_untuk(elem)?;
        self.ubah_tiap_elemen(v, elem, &JrgType::Any)
    }

    /// Array baru yang tiap elemennya `coerce` dari `elem` ke `ke` (masuk kotak, peluruhan).
    pub(crate) fn ubah_tiap_elemen(&mut self, v: &Val, elem: &JrgType, ke: &JrgType) -> Option<Val> {
        let (ptr, len) = self.bagian_koleksi_pub(v, elem);
        let buf = self.buffer_baru(&len, ke);
        let idx = self.alloca_raw("kti.", "i64", Some(8));
        self.emit(format!("store i64 0, ptr {}", idx));
        let kty = ke.llvm_ty();
        self.loop_koleksi(&ptr, &len, elem, |cg, nilai| {
            let k = cg.coerce(&nilai, ke);
            if k.ty != *ke { return Err("elemen tidak bisa diubah".to_string()); }
            let i = cg.fresh("ktv.");
            cg.emit(format!("{} = load i64, ptr {}", i, idx));
            let dst = cg.fresh("ktd.");
            cg.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", dst, kty, buf, i));
            cg.emit(format!("store {} {}, ptr {}", kty, k.text, dst));
            let n = cg.fresh("ktn.");
            cg.emit(format!("{} = add i64 {}, 1", n, i));
            cg.emit(format!("store i64 {}, ptr {}", n, idx));
            Ok(())
        }).ok()?;
        Some(self.rakit_slice(&buf, &len, ke))
    }

    /// Array bertag jadi array ber-elemen `elem`; elemen bertipe lain gagal saat jalan.
    pub(crate) fn buka_tiap_elemen(&mut self, v: &Val, elem: &JrgType) -> Option<Val> {
        super::codegen_any::tag_untuk(elem)?;
        let (ptr, len) = self.bagian_koleksi_pub(v, &JrgType::Any);
        let buf = self.buffer_baru(&len, elem);
        let idx = self.alloca_raw("bti.", "i64", Some(8));
        self.emit(format!("store i64 0, ptr {}", idx));
        let ety = elem.llvm_ty();
        self.loop_koleksi(&ptr, &len, &JrgType::Any, |cg, nilai| {
            let Some(b) = cg.buka_terperiksa(&nilai, elem) else {
                return Err("elemen bertag tidak bisa dibuka".to_string());
            };
            let i = cg.fresh("btv.");
            cg.emit(format!("{} = load i64, ptr {}", i, idx));
            let dst = cg.fresh("btd.");
            cg.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", dst, ety, buf, i));
            cg.emit(format!("store {} {}, ptr {}", ety, b.text, dst));
            let n = cg.fresh("btn.");
            cg.emit(format!("{} = add i64 {}, 1", n, i));
            cg.emit(format!("store i64 {}, ptr {}", n, idx));
            Ok(())
        }).ok()?;
        Some(self.rakit_slice(&buf, &len, elem))
    }

    /// `arr::reverse(a)` — salinan dengan urutan terbalik.
    fn arr_reverse(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        let (koleksi, elem) = self.arg_koleksi(&args[0], "reverse", span)?;
        let (ptr, len) = self.bagian_koleksi_pub(&koleksi, &elem);
        let buf = self.buffer_baru(&len, &elem);

        // Ditulis mundur dari ujung: indeks tujuan = len - 1 - i.
        let idx = self.alloca_raw("revi.", "i64", Some(8));
        self.emit(format!("store i64 0, ptr {}", idx));
        let ety = elem.llvm_ty();
        self.loop_koleksi(&ptr, &len, &elem, |cg, nilai| {
            let i = cg.fresh("revv.");
            cg.emit(format!("{} = load i64, ptr {}", i, idx));
            let sisa = cg.fresh("revs.");
            cg.emit(format!("{} = sub i64 {}, {}", sisa, len, i));
            let j = cg.fresh("revj.");
            cg.emit(format!("{} = sub i64 {}, 1", j, sisa));
            let dst = cg.fresh("revd.");
            cg.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", dst, ety, buf, j));
            cg.emit(format!("store {} {}, ptr {}", ety, nilai.text, dst));
            let inext = cg.fresh("revn.");
            cg.emit(format!("{} = add i64 {}, 1", inext, i));
            cg.emit(format!("store i64 {}, ptr {}", inext, idx));
            Ok(())
        })?;
        Ok(self.rakit_slice(&buf, &len, &elem))
    }

    /// Salinan `koleksi` yang terurut naik, untuk elemen yang punya urutan leksikografis.
    fn urut_dgn_pembanding(&mut self, koleksi: &Val, elem: &JrgType) -> Result<Val, String> {
        let nama = format!("banding.{}", elem.mangle_frag());
        if !self.is_fn_emitted_or_queued(&nama) {
            self.tandai_fn_terpakai(&nama);
            self.antre_fn(super::codegen::KerjaFn::Banding { nama: nama.clone(), ty: elem.clone() });
        }
        let (ptr, len) = self.bagian_koleksi_pub(koleksi, elem);
        let buf = self.buffer_baru(&len, elem);
        let ukuran = self.types.sizeof(elem).max(1);
        self.ensure_extern("declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)", "llvm.memcpy.p0.p0.i64");
        let byte = self.fresh("srtb.");
        self.emit(format!("{} = mul i64 {}, {}", byte, len, ukuran));
        self.emit(format!("call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)", buf, ptr, byte));
        self.ensure_extern("declare void @jrg_sort_umum(ptr, i64, i64, ptr)", "jrg_sort_umum");
        self.emit(format!("call void @jrg_sort_umum(ptr {}, i64 {}, i64 {}, ptr @{})",
            buf, len, ukuran, super::mangle::llvm_quoted_ident(&nama)));
        Ok(self.rakit_slice(&buf, &len, elem))
    }

    /// `arr::sort(a)` — salinan yang terurut naik.
    fn arr_sort(&mut self, args: &[Expr], span: Span) -> Result<Val, String> {
        let (koleksi, elem) = self.arg_koleksi(&args[0], "sort", span)?;
        if elem == JrgType::Any { return Ok(self.urut_slice_any(&koleksi)); }
        let (fungsi, tipe_ptr) = match &elem {
            t if t.is_int() && t.int_bits() == Some(64) => ("jrg_sort_i64", "i64"),
            JrgType::F64 => ("jrg_sort_f64", "double"),
            JrgType::Str => ("jrg_sort_str", "i8"),
            // Elemen gabungan (array, tuple): diurutkan stabil lewat pembanding yang dibangkitkan.
            JrgType::Array(..) | JrgType::Slice(_) | JrgType::Tuple(_) => return self.urut_dgn_pembanding(&koleksi, &elem),
            other => return Err(super::galat::tandai(span, format!(
                "`arr::sort` belum bisa mengurutkan elemen bertipe `{}`\n\
                 bantuan: yang didukung `int`, `float`, dan `string`\n\
                 catatan: mengurutkan butuh tahu cara membandingkan dua \
                 elemen, dan untuk tipe gabungan itu belum ada di backend ini",
                other.display_name()))),
        };

        let (ptr, len) = self.bagian_koleksi_pub(&koleksi, &elem);
        let buf = self.buffer_baru(&len, &elem);
        let ukuran = self.types.sizeof(&elem);
        self.ensure_extern(
            "declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)",
            "llvm.memcpy.p0.p0.i64");
        let byte = self.fresh("srtb.");
        self.emit(format!("{} = mul i64 {}, {}", byte, len, ukuran));
        self.emit(format!(
            "call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)",
            buf, ptr, byte));

        if elem == JrgType::F64 {
            let pesan = crate::interpreter::pesan_urut_tak_sebanding("arr::sort", "float", "float");
            let pv = self.intern_string(&pesan);
            let (pp, pl) = self.split_str(&pv);
            self.ensure_extern("declare void @jrg_sort_f64_ketat(ptr, i64, ptr, i64)", "jrg_sort_f64_ketat");
            self.emit(format!("call void @jrg_sort_f64_ketat(ptr {}, i64 {}, ptr {}, i64 {})", buf, len, pp, pl));
        }
        self.ensure_extern(&format!("declare void @{}(ptr, i64)", fungsi), fungsi);
        let _ = tipe_ptr;
        self.emit(format!("call void @{}(ptr {}, i64 {})", fungsi, buf, len));
        Ok(self.rakit_slice(&buf, &len, &elem))
    }

    /// `arr::min(a)` / `arr::max(a)`.
    fn arr_ekstrem(&mut self, args: &[Expr], span: Span, minimum: bool)
        -> Result<Val, String>
    {
        let nama = if minimum { "min" } else { "max" };
        let (koleksi, elem) = self.arg_koleksi(&args[0], nama, span)?;
        let panjang_pasti = match &koleksi.ty {
            JrgType::Array(_, n) => Some(*n),
            _ => None,
        };
        match panjang_pasti {
            Some(n) if n >= 1 => {}
            _ => return Err(super::galat::tandai(span, format!(
                "`arr::{}` butuh array yang panjangnya sudah pasti dan tidak nol\n\
                 bantuan: pakai array berukuran tetap, mis. `var a: [3]int = …`\n\
                 catatan: pada koleksi kosong jawabannya `nil`, dan `nil` belum \
                 punya perwujudan di backend `--emit-llvm` — jadi yang bisa \
                 dilayani cuma yang pasti berisi",
                nama))),
        }

        let (ptr, len) = self.bagian_koleksi_pub(&koleksi, &elem);
        let ety = elem.llvm_ty();
        let akum = self.alloca_typed("ext.", &elem);
        let awal = self.fresh("ext0.");
        self.emit(format!("{} = load {}, ptr {}", awal, ety, ptr));
        self.emit(format!("store {} {}, ptr {}", ety, awal, akum));

        let op = if minimum { BinaryOp::Lt } else { BinaryOp::Gt };
        // NaN tidak terbanding dengan apa pun: dua elemen atau lebih, salah satunya NaN, gagal.
        let jaga_nan = matches!(elem, JrgType::F64 | JrgType::F32) && panjang_pasti.map_or(false, |n| n >= 2);
        let pesan = crate::interpreter::pesan_min_max_tak_sebanding(
            &format!("arr::{}", nama), "float", "float");
        self.loop_koleksi(&ptr, &len, &elem, |cg, nilai| {
            let kini = cg.load_place(&akum, &elem);
            if jaga_nan {
                let nan = cg.fresh("extn.");
                cg.emit(format!("{} = fcmp uno {} {}, {}", nan, ety, nilai.text, kini.text));
                let (gagal, lanjut) = (cg.fresh_label("ext.nan"), cg.fresh_label("ext.ok"));
                cg.terminate(format!("br i1 {}, label %{}, label %{}, !prof !0", nan, gagal, lanjut));
                cg.start_block(gagal);
                cg.panik(&pesan);
                cg.start_block(lanjut);
            }
            let lebih = cg.gabung_nilai(&op, nilai.clone(), kini.clone(), span)?;
            let i1 = cg.to_i1(&lebih);
            let pilih = cg.fresh("extp.");
            cg.emit(format!("{} = select i1 {}, {} {}, {} {}",
                            pilih, i1, ety, nilai.text, ety, kini.text));
            cg.emit(format!("store {} {}, ptr {}", ety, pilih, akum));
            Ok(())
        })?;
        Ok(self.load_place(&akum, &elem))
    }

    // ── Perkakas bersama ────────────────────────────────────────────────

    /// (penunjuk data, panjang) dari sebuah koleksi — versi publik.
    pub fn bagian_koleksi_pub(&mut self, v: &Val, elem: &JrgType) -> (String, String) {
        self.bagian_koleksi_impl(v, elem)
    }

    /// Buffer heap baru sepanjang `len` elemen.
    pub fn buffer_baru(&mut self, len: &str, elem: &JrgType) -> String {
        let ukuran = self.types.sizeof(elem);
        let align = self.types.alignof(elem);
        self.ensure_extern("declare ptr @jrg_alloc(i64, i64)", "jrg_alloc");
        let byte = self.fresh("nbb.");
        self.emit(format!("{} = mul i64 {}, {}", byte, len, ukuran));
        let p = self.fresh("nbp.");
        self.emit(format!("{} = call ptr @jrg_alloc(i64 {}, i64 {})", p, byte, align));
        p
    }

    /// Rakit `{ptr, len}` jadi sebuah nilai slice.
    pub fn rakit_slice(&mut self, ptr: &str, len: &str, elem: &JrgType) -> Val {
        let a = self.fresh("mks.");
        self.emit(format!("{} = insertvalue %JrgSlice undef, ptr {}, 0", a, ptr));
        let b = self.fresh("mks.");
        self.emit(format!("{} = insertvalue %JrgSlice {}, i64 {}, 1", b, a, len));
        Val { text: b, ty: JrgType::Slice(Box::new(elem.clone())) }
    }

    /// Telusuri tiap elemen koleksi, memanggil `badan` dengan nilainya.
    fn loop_koleksi<F>(&mut self, ptr: &str, len: &str, elem: &JrgType, mut badan: F)
        -> Result<(), String>
    where
        F: FnMut(&mut Self, Val) -> Result<(), String>,
    {
        let idx = self.alloca_raw("lki.", "i64", Some(8));
        self.emit(format!("store i64 0, ptr {}", idx));

        let cond = self.fresh_label("lk.cond.");
        let body = self.fresh_label("lk.body.");
        let end = self.fresh_label("lk.end.");
        self.terminate(format!("br label %{}", cond));

        self.start_block(cond.clone());
        let i = self.fresh("lkv.");
        self.emit(format!("{} = load i64, ptr {}", i, idx));
        let lanjut = self.fresh("lkc.");
        self.emit(format!("{} = icmp slt i64 {}, {}", lanjut, i, len));
        self.terminate(format!("br i1 {}, label %{}, label %{}", lanjut, body, end));

        self.start_block(body);
        let ep = self.fresh("lkp.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i64 {}", ep, elem.llvm_ty(), ptr, i));
        let nilai = self.load_place(&ep, elem);
        badan(self, nilai)?;
        let inext = self.fresh("lkn.");
        self.emit(format!("{} = add i64 {}, 1", inext, i));
        self.emit(format!("store i64 {}, ptr {}", inext, idx));
        if !self.is_terminated() {
            self.terminate(format!("br label %{}", cond));
        }

        self.start_block(end);
        Ok(())
    }
}

impl Codegen {
    /// `a.first()` / `a.last()` / `a.get(i)` / `a.get_or(i, d)`.
    pub fn metode_opsi_koleksi(
        &mut self, v: &Val, elem: &JrgType, metode: &str, args: &[Expr], span: Span,
    ) -> Option<Result<Val, String>> {
        let arity = match metode {
            "first" | "last" => 0,
            "get" => 1,
            "get_or" => 2,
            _ => return None,
        };
        if args.len() != arity { return None; }
        Some((|| {
            let panjang = self.panjang_koleksi(v, span)?;
            let idx = match metode {
                "first" => Val { text: "0".into(), ty: JrgType::I64 },
                "last" => {
                    let r = self.fresh("lastix.");
                    self.emit(format!("{} = sub i64 {}, 1", r, panjang.text));
                    Val { text: r, ty: JrgType::I64 }
                }
                _ => {
                    // Aturan `a[i]`: negatif dihitung dari belakang; di luar batas tetap `None`/cadangan.
                    let i = self.codegen_expr(&args[0], Some(&JrgType::I64))?;
                    let i = self.bulat_arg(&i, &format!("arr::{}", metode), span)?;
                    let (neg, tambah, r) = (self.fresh("ixneg."), self.fresh("ixplus."), self.fresh("ixn."));
                    self.emit(format!("{} = icmp slt i64 {}, 0", neg, i.text));
                    self.emit(format!("{} = add i64 {}, {}", tambah, i.text, panjang.text));
                    self.emit(format!("{} = select i1 {}, i64 {}, i64 {}", r, neg, tambah, i.text));
                    Val { text: r, ty: JrgType::I64 }
                }
            };
            // Di dalam batas? `first`/`last` pada koleksi kosong menjawab
            // `None` lewat pemeriksaan yang sama — `last` di situ menghitung
            // indeks `-1`, yang gagal syarat `>= 0`.
            let ge0 = self.fresh("okge.");
            self.emit(format!("{} = icmp sge i64 {}, 0", ge0, idx.text));
            let ltn = self.fresh("oklt.");
            self.emit(format!("{} = icmp slt i64 {}, {}", ltn, idx.text, panjang.text));
            let ada = self.fresh("okada.");
            self.emit(format!("{} = and i1 {}, {}", ada, ge0, ltn));

            let hasil_ty = if metode == "get_or" {
                elem.clone()
            } else {
                JrgType::Enum(self.instantiate_option(elem))
            };
            let slot = self.alloca_typed("okslot.", &hasil_ty);
            let l_ada = self.fresh_label("ok.ada.");
            let l_tidak = self.fresh_label("ok.tidak.");
            let l_end = self.fresh_label("ok.end.");
            self.terminate(format!("br i1 {}, label %{}, label %{}", ada, l_ada, l_tidak));

            self.start_block(l_ada);
            let tempat = self.spill_to_place(v);
            let (ep, ety) = self.get_index_place(&tempat, &v.ty, &idx, span)?;
            let ev = self.load_place(&ep, &ety);
            let simpan = if metode == "get_or" {
                self.coerce(&ev, elem)
            } else {
                let en = self.instantiate_option(elem);
                let layout = self.types.enums.get(&en).cloned()
                    .ok_or_else(|| "layout Option tidak terdaftar".to_string())?;
                let vs = layout.variant("Some").cloned()
                    .ok_or_else(|| "variant Some tidak ada".to_string())?;
                let ev = self.coerce(&ev, elem);
                self.build_enum_value(&en, &layout, &vs, &[ev])?
            };
            self.emit(format!("store {} {}, ptr {}", hasil_ty.llvm_ty(), simpan.text, slot));
            self.terminate(format!("br label %{}", l_end));

            self.start_block(l_tidak);
            let kosong = if metode == "get_or" {
                let d = self.codegen_expr(&args[1], Some(elem))?;
                self.coerce(&d, elem)
            } else {
                let en = self.instantiate_option(elem);
                self.build_enum_unit_variant(&en, "None")?
            };
            self.emit(format!("store {} {}, ptr {}", hasil_ty.llvm_ty(), kosong.text, slot));
            self.terminate(format!("br label %{}", l_end));

            self.start_block(l_end);
            Ok(self.load_place(&slot, &hasil_ty))
        })())
    }
}

impl Codegen {
    /// Ubah `i64` yang memakai `-1` sebagai "tidak ketemu" jadi `Option<int>`.
    pub fn indeks_ke_option(&mut self, r: &str) -> Result<Val, String> {
        let elem = JrgType::I64;
        let en = self.instantiate_option(&elem);
        let hasil_ty = JrgType::Enum(en.clone());
        let slot = self.alloca_typed("ixopt.", &hasil_ty);

        let ada = self.fresh("ixada.");
        self.emit(format!("{} = icmp sge i64 {}, 0", ada, r));
        let l_ada = self.fresh_label("ixo.ada.");
        let l_tidak = self.fresh_label("ixo.tidak.");
        let l_end = self.fresh_label("ixo.end.");
        self.terminate(format!("br i1 {}, label %{}, label %{}", ada, l_ada, l_tidak));

        self.start_block(l_ada);
        let layout = self.types.enums.get(&en).cloned()
            .ok_or_else(|| "layout Option tidak terdaftar".to_string())?;
        let vs = layout.variant("Some").cloned()
            .ok_or_else(|| "variant Some tidak ada".to_string())?;
        let isi = Val { text: r.to_string(), ty: JrgType::I64 };
        let some = self.build_enum_value(&en, &layout, &vs, &[isi])?;
        self.emit(format!("store {} {}, ptr {}", hasil_ty.llvm_ty(), some.text, slot));
        self.terminate(format!("br label %{}", l_end));

        self.start_block(l_tidak);
        let none = self.build_enum_unit_variant(&en, "None")?;
        self.emit(format!("store {} {}, ptr {}", hasil_ty.llvm_ty(), none.text, slot));
        self.terminate(format!("br label %{}", l_end));

        self.start_block(l_end);
        Ok(self.load_place(&slot, &hasil_ty))
    }

    /// `a.pop()` — `(sisa, Option<terakhir>)`.
    pub fn metode_pop(&mut self, v: &Val, elem: &JrgType, span: Span)
        -> Result<Val, String>
    {
        let panjang = self.panjang_koleksi(v, span)?;
        let ada = self.fresh("popada.");
        self.emit(format!("{} = icmp sgt i64 {}, 0", ada, panjang.text));
        let baru_n = self.fresh("popn.");
        self.emit(format!("{} = sub i64 {}, 1", baru_n, panjang.text));
        // Panjang sisa tidak boleh negatif: koleksi kosong tetap menghasilkan
        // koleksi kosong, bukan slice sepanjang -1.
        let n_aman = self.fresh("popna.");
        self.emit(format!("{} = select i1 {}, i64 {}, i64 0", n_aman, ada, baru_n));

        let opt_en = self.instantiate_option(elem);
        let opt_ty = JrgType::Enum(opt_en.clone());
        let slot_opt = self.alloca_typed("popo.", &opt_ty);
        let l_ada = self.fresh_label("pop.ada.");
        let l_tidak = self.fresh_label("pop.tidak.");
        let l_end = self.fresh_label("pop.end.");
        self.terminate(format!("br i1 {}, label %{}, label %{}", ada, l_ada, l_tidak));

        self.start_block(l_ada);
        let tempat = self.spill_to_place(v);
        let ix = Val { text: baru_n.clone(), ty: JrgType::I64 };
        let (ep, ety) = self.get_index_place(&tempat, &v.ty, &ix, span)?;
        let ev = self.load_place(&ep, &ety);
        let ev = self.coerce(&ev, elem);
        let layout = self.types.enums.get(&opt_en).cloned()
            .ok_or_else(|| "layout Option tidak terdaftar".to_string())?;
        let vs = layout.variant("Some").cloned()
            .ok_or_else(|| "variant Some tidak ada".to_string())?;
        let some = self.build_enum_value(&opt_en, &layout, &vs, &[ev])?;
        self.emit(format!("store {} {}, ptr {}", opt_ty.llvm_ty(), some.text, slot_opt));
        self.terminate(format!("br label %{}", l_end));

        self.start_block(l_tidak);
        let none = self.build_enum_unit_variant(&opt_en, "None")?;
        self.emit(format!("store {} {}, ptr {}", opt_ty.llvm_ty(), none.text, slot_opt));
        self.terminate(format!("br label %{}", l_end));

        self.start_block(l_end);
        let opt = self.load_place(&slot_opt, &opt_ty);

        // Sisanya sebuah slice atas buffer SENDIRI.
        let irisan = self.iris_awal(v, &Val { text: n_aman, ty: JrgType::I64 }, elem, span)?;
        let tup_ty = JrgType::Tuple(vec![irisan.ty.clone(), opt_ty.clone()]);
        let slot = self.alloca_typed("poptup.", &tup_ty);
        let p0 = self.fresh("poptf.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 0", p0, tup_ty.llvm_ty(), slot));
        self.emit(format!("store {} {}, ptr {}", irisan.ty.llvm_ty(), irisan.text, p0));
        let p1 = self.fresh("poptf.");
        self.emit(format!("{} = getelementptr {}, ptr {}, i32 0, i32 1", p1, tup_ty.llvm_ty(), slot));
        self.emit(format!("store {} {}, ptr {}", opt_ty.llvm_ty(), opt.text, p1));
        Ok(self.load_place(&slot, &tup_ty))
    }

    /// `n` elemen pertama sebuah koleksi, sebagai slice dengan buffer sendiri.
    fn iris_awal(&mut self, v: &Val, n: &Val, elem: &JrgType, span: Span)
        -> Result<Val, String>
    {
        let _ = span;
        let uk = self.types.sizeof(elem).max(1);
        let al = self.types.alignof(elem).max(1);
        let byte = self.fresh("irb.");
        self.emit(format!("{} = mul i64 {}, {}", byte, n.text, uk));
        self.ensure_extern("declare ptr @jrg_alloc(i64, i64)", "jrg_alloc");
        let buf = self.fresh("irbuf.");
        self.emit(format!("{} = call ptr @jrg_alloc(i64 {}, i64 {})", buf, byte, al));
        let asal = self.spill_to_place(v);
        let src = match &v.ty {
            JrgType::Slice(_) => {
                let p = self.fresh("irsp.");
                self.emit(format!("{} = extractvalue %JrgSlice {}, 0", p, v.text));
                p
            }
            _ => asal,
        };
        self.ensure_extern(
            "declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)",
            "llvm.memcpy.p0.p0.i64");
        self.emit(format!(
            "call void @llvm.memcpy.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)",
            buf, src, byte));
        Ok(self.rakit_slice(&buf, &n.text, elem))
    }
}
