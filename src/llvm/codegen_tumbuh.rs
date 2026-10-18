// src/llvm/codegen_tumbuh.rs — menumbuhkan array di tempat: `a = a + [x]`, `a += [x]`,
// `a = arr::push(a, x)`, `a = arr::concat(a, b)` atas variabel bertipe slice.

use crate::parser::*;

use super::codegen::{Codegen, Val};
use super::types::*;

enum Tambahan<'a> { Elemen(&'a Expr), Koleksi(&'a Expr) }

/// Akhiran simbol varian method yang penerimanya dipindahkan, bukan disalin.
pub(crate) const AKHIRAN_MILIK: &str = "$milik";

/// Jalur tanpa efek samping: nama, `.field`, `[literal]`, `[nama]`.
fn jalur_murni(e: &Expr) -> bool {
    match e {
        Expr::Ident(..) => true,
        Expr::Field { obj, .. } => jalur_murni(obj),
        Expr::Index { obj, idx, .. } =>
            matches!(&**idx, Expr::Literal(LitKind::Int(_) | LitKind::Str(_), _) | Expr::Ident(..)) && jalur_murni(obj),
        _ => false,
    }
}

fn jalur_sama(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Ident(x, _), Expr::Ident(y, _)) => x == y,
        (Expr::Field { obj: p, field: f, .. }, Expr::Field { obj: q, field: g, .. }) => f == g && jalur_sama(p, q),
        (Expr::Index { obj: p, idx: i, .. }, Expr::Index { obj: q, idx: j, .. }) => {
            let sama = match (&**i, &**j) {
                (Expr::Literal(LitKind::Int(x), _), Expr::Literal(LitKind::Int(y), _)) => x == y,
                (Expr::Literal(LitKind::Str(x), _), Expr::Literal(LitKind::Str(y), _)) => x == y,
                (Expr::Ident(x, _), Expr::Ident(y, _)) => x == y,
                _ => false,
            };
            sama && jalur_sama(p, q)
        }
        _ => false,
    }
}

fn akar(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(n, _) => Some(n.as_str()),
        Expr::Field { obj, .. } | Expr::Index { obj, .. } => akar(obj),
        _ => None,
    }
}

impl Codegen {
    /// Bentuk `d = dict::set(d, k, v)` atas satu nama, sebagai penugasan `d[k] = v`.
    pub(crate) fn pasang_di_tempat(a: &AssignStmt) -> Option<AssignStmt> {
        let ([Expr::Ident(n, _)], AssignOp::Eq, [Expr::Call { func, args, span }]) =
            (a.targets.as_slice(), &a.op, a.values.as_slice()) else { return None };
        let Expr::PathExpr { segments, .. } = func.as_ref() else { return None };
        if !matches!(super::infer::tanpa_awalan_bawaan(segments), [x, y] if x == "dict" && y == "set") { return None; }
        let [Expr::Ident(m, _), k, v] = args.as_slice() else { return None };
        if m != n { return None; }
        let sasaran = Expr::Index { obj: Box::new(args[0].clone()), idx: Box::new(k.clone()), span: *span };
        Some(AssignStmt { targets: vec![sasaran], op: AssignOp::Eq, values: vec![v.clone()], span: a.span })
    }

    /// Kerjakan penugasan ini sebagai pertumbuhan di tempat kalau bentuknya cocok. `false`
    /// berarti tidak ada yang diturunkan, dan jalur penugasan biasa yang mengerjakannya.
    pub(crate) fn coba_tumbuh_di_tempat(&mut self, a: &AssignStmt) -> Result<bool, String> {
        if a.targets.len() != 1 || a.values.len() != 1 { return Ok(false); }
        let Expr::Ident(nama, _) = &a.targets[0] else { return Ok(false) };
        let sama = |e: &Expr| matches!(e, Expr::Ident(n, _) if n == nama);
        let tambahan = match (&a.op, &a.values[0]) {
            (AssignOp::PlusEq, r) => Tambahan::Koleksi(r),
            (AssignOp::Eq, Expr::Binary { op: BinaryOp::Add, lhs, rhs, .. }) if sama(lhs) =>
                Tambahan::Koleksi(rhs),
            (AssignOp::Eq, Expr::Call { func, args, .. }) if args.len() == 2 && sama(&args[0]) => {
                let Expr::PathExpr { segments, .. } = &**func else { return Ok(false) };
                // Modul pemakai yang ber-alias `arr` menang atas bawaan: bukan urusan jalur ini.
                if segments.len() != 2 || segments[0] != "arr" || self.unit_alias("arr").is_some() {
                    return Ok(false);
                }
                match segments[1].as_str() {
                    "push" => Tambahan::Elemen(&args[1]),
                    "concat" => Tambahan::Koleksi(&args[1]),
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        };
        let kanan = match &tambahan { Tambahan::Elemen(e) | Tambahan::Koleksi(e) => *e };
        if matches!(a.op, AssignOp::Eq) && self.kanan_bisa_menulis(nama, kanan) { return Ok(false); }
        let Some((slot, ty)) = self.lookup_local(nama)
            .or_else(|| self.var_global.get(&self.k(nama)).cloned()) else { return Ok(false) };
        let JrgType::Slice(elem) = &ty else { return Ok(false) };
        let elem = (**elem).clone();
        // Cuma elemen DATAR: elemen yang memegang penyangga sendiri (array di dalam array) butuh
        // salinan dalam, dan jalur biasa yang tahu caranya.
        let datar = elem.is_numeric() || matches!(elem, JrgType::Bool | JrgType::Rune | JrgType::Str);
        let ukuran = self.types.sizeof(&elem);
        if !datar || ukuran == 0 { return Ok(false); }

        // Sisi kanan diturunkan LEBIH DULU, selagi `a` masih utuh — `a = a + f(a)` membaca yang lama.
        let (rp, rn) = match tambahan {
            Tambahan::Elemen(e) => {
                let x = self.codegen_expr(e, Some(&elem))?;
                let x = self.coerce(&x, &elem);
                (self.spill_to_place(&x), "1".to_string())
            }
            Tambahan::Koleksi(r) => {
                let v = self.codegen_expr(r, Some(&ty))?;
                let v = self.coerce(&v, &ty);
                self.bagian_koleksi_pub(&v, &elem)
            }
        };
        // Kotak milik closure hidup lebih lama dari satu panggilan: kapasitasnya dicatat di
        // tabel umum, bukan di slot fungsi ini yang lahir nol tiap kali dipanggil.
        if !slot.starts_with('@') && (self.tangkapan.contains(nama) || self.tangkap_masuk.contains(nama)) {
            self.ensure_extern(
                "declare void @jrg_slice_tumbuh_umum(ptr, ptr, i64, i64, i64)", "jrg_slice_tumbuh_umum");
            self.emit(format!(
                "call void @jrg_slice_tumbuh_umum(ptr {}, ptr {}, i64 {}, i64 {}, i64 {})",
                slot, rp, rn, ukuran, self.types.alignof(&elem)));
            return Ok(true);
        }
        let kap = self.slot_kapasitas(&slot);
        self.ensure_extern(
            "declare void @jrg_slice_tumbuh(ptr, ptr, ptr, i64, i64, i64)", "jrg_slice_tumbuh");
        self.emit(format!(
            "call void @jrg_slice_tumbuh(ptr {}, ptr {}, ptr {}, i64 {}, i64 {}, i64 {})",
            slot, kap, rp, rn, ukuran, self.types.alignof(&elem)));
        Ok(true)
    }

    /// Sama untuk tempat MANA PUN yang berakar pada sebuah nama (`self.isi += [x]`,
    /// `p.xs = arr::push(p.xs, x)`) dan untuk elemen yang tidak datar. Kapasitasnya dicatat
    /// primitif C per alamat penyangga, bukan per variabel.
    pub(crate) fn coba_tumbuh_jalur(&mut self, a: &AssignStmt) -> Result<bool, String> {
        if a.targets.len() != 1 || a.values.len() != 1 { return Ok(false); }
        let sasaran = &a.targets[0];
        if !jalur_murni(sasaran) { return Ok(false); }
        let Some(nama_akar) = akar(sasaran) else { return Ok(false) };
        let sama = |e: &Expr| jalur_sama(e, sasaran);
        let tambahan = match (&a.op, &a.values[0]) {
            (AssignOp::PlusEq, r) => Tambahan::Koleksi(r),
            (AssignOp::Eq, Expr::Binary { op: BinaryOp::Add, lhs, rhs, .. }) if sama(lhs) => Tambahan::Koleksi(rhs),
            (AssignOp::Eq, Expr::Call { func, args, .. }) if args.len() == 2 && sama(&args[0]) => {
                let Expr::PathExpr { segments, .. } = &**func else { return Ok(false) };
                if segments.len() != 2 || segments[0] != "arr" || self.unit_alias("arr").is_some() { return Ok(false); }
                match segments[1].as_str() {
                    "push" => Tambahan::Elemen(&args[1]),
                    "concat" => Tambahan::Koleksi(&args[1]),
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        };
        // Sisi kanan dihitung sesudah tempatnya dicari, jadi ia tidak boleh menyentuh akarnya.
        let kanan = match &tambahan { Tambahan::Elemen(e) | Tambahan::Koleksi(e) => *e };
        if crate::interpreter::capture::names_mentioned(kanan).contains(nama_akar) { return Ok(false); }
        if matches!(a.op, AssignOp::Eq) && self.kanan_bisa_menulis(nama_akar, kanan) { return Ok(false); }
        // Tipe tempatnya dibaca tanpa memancarkan apa pun: yang bukan array lewat jalur biasa.
        let Some(JrgType::Slice(elem)) = self.tipe_jalur(sasaran) else { return Ok(false) };
        let elem = (*elem).clone();
        let ukuran = self.types.sizeof(&elem);
        if ukuran == 0 { return Ok(false); }
        let ty = JrgType::Slice(Box::new(elem.clone()));
        let (rp, rn) = match tambahan {
            Tambahan::Elemen(e) => {
                let x = self.codegen_expr(e, Some(&elem))?;
                let x = self.coerce(&x, &elem);
                let x = self.ambil_milik(e, x)?;
                (self.spill_to_place(&x), "1".to_string())
            }
            Tambahan::Koleksi(r) => {
                let v = self.codegen_expr(r, Some(&ty))?;
                if !matches!(v.ty, JrgType::Slice(_) | JrgType::Array(_, _)) {
                    return Err(super::galat::tandai(a.span, format!(
                        "tidak bisa memakai nilai bertipe `{}` sebagai `{}`", v.ty.display_name(), ty.display_name())));
                }
                let v = self.coerce(&v, &ty);
                let v = self.ambil_milik(r, v)?;
                self.bagian_koleksi_pub(&v, &elem)
            }
        };
        let (slot, _) = self.get_place(sasaran)?;
        self.ensure_extern("declare void @jrg_slice_tumbuh_umum(ptr, ptr, i64, i64, i64)", "jrg_slice_tumbuh_umum");
        self.emit(format!("call void @jrg_slice_tumbuh_umum(ptr {}, ptr {}, i64 {}, i64 {}, i64 {})",
            slot, rp, rn, ukuran, self.types.alignof(&elem).max(1)));
        Ok(true)
    }

    /// `x = f(x, ..)` atas fungsi bernama: argumen pertama DIPINDAHKAN (tanpa salinan masuk),
    /// karena `x` langsung ditimpa hasilnya. Mengembalikan nama yang boleh dipindahkan.
    pub(crate) fn argumen_dipindahkan(&self, a: &AssignStmt) -> Option<String> {
        let ([Expr::Ident(nama, _)], AssignOp::Eq, [Expr::Call { func, args, .. }]) =
            (a.targets.as_slice(), &a.op, a.values.as_slice()) else { return None };
        let Expr::Ident(f, _) = &**func else { return None };
        if !matches!(args.first(), Some(Expr::Ident(n, _)) if n == nama) { return None; }
        if args[1..].iter().any(|x| crate::interpreter::capture::names_mentioned(x).contains(nama.as_str())) { return None; }
        if nama == "self" || self.tangkapan.contains(nama) || self.lookup_local(nama).is_none() { return None; }
        if self.lookup_local(f).is_some() || !self.tables.functions.contains_key(&self.k(f)) { return None; }
        Some(nama.clone())
    }

    /// `a, b = e`: tuple di sisi kanan dibongkar ke sasarannya. Bentuk `x, v = x.m(..)` atas
    /// method yang selalu `return (self, ..)` berjalan di tempat `x` (varian milik): elemen
    /// pertamanya sudah ada di sana, jadi cuma sisanya yang ditugaskan.
    pub(crate) fn codegen_bongkar(&mut self, a: &AssignStmt) -> Result<(), String> {
        let n = a.targets.len();
        let nilai = &a.values[0];
        let mut mulai = 0;
        let mut hasil: Option<Val> = None;
        if let (Expr::Ident(nama, _), Expr::Method { obj, method, args, span }) = (&a.targets[0], nilai) {
            let sama = matches!(&**obj, Expr::Ident(m, _) if m == nama);
            let bersih = !args.iter().any(|x| crate::interpreter::capture::names_mentioned(x).contains(nama.as_str()));
            if sama && bersih && !(nama == "self" && self.self_dipinjam) && !self.tangkapan.contains(nama) {
                if let Some((slot, ty)) = self.lookup_local(nama) {
                    if let JrgType::Struct(tipe) = &ty {
                        let d = self.tables.find_method(tipe, method).cloned();
                        let cocok = d.as_ref().map_or(false, |d| matches!(&*d.body,
                            FnBody::Block(b) if crate::interpreter::capture::blok_kembali_tuple_self(b)));
                        if let (true, Some(d)) = (cocok, d) {
                            hasil = Some(self.call_method_impl_dgn(tipe, &d, &slot, &ty, args, true, *span)?);
                            mulai = 1;
                        }
                    }
                }
            }
        }
        let hasil = match hasil {
            Some(v) => v,
            None => self.codegen_expr(nilai, None)?,
        };
        let JrgType::Tuple(ts) = hasil.ty.clone() else {
            return Err(super::galat::tandai(nilai.span(),
                crate::interpreter::pesan_bongkar_bukan_tuple(n, &hasil.ty.nama_typeof())));
        };
        if ts.len() != n {
            return Err(super::galat::tandai(nilai.span(), crate::interpreter::pesan_bongkar_jumlah(n, ts.len())));
        }
        // Tiap elemen jadi lokal sementara, lalu ditugaskan lewat jalur penugasan biasa —
        // satu aturan untuk sasaran apa pun (nama, `_`, field, elemen).
        self.push_scope();
        let mut sementara = Vec::with_capacity(n);
        for (i, t) in ts.iter().enumerate().skip(mulai) {
            let r = self.fresh("bkr.");
            self.emit(format!("{} = extractvalue {} {}, {}", r, hasil.ty.llvm_ty(), hasil.text, i));
            let nama = format!("$bongkar{}_{}", i, r.trim_start_matches('%'));
            let slot = self.alloca_local(&nama, t.clone());
            if *t != JrgType::Unit { self.emit(format!("store {} {}, ptr {}", t.llvm_ty(), r, slot)); }
            sementara.push((i, nama));
        }
        for (i, nama) in sementara {
            let satu = AssignStmt {
                targets: vec![a.targets[i].clone()], op: AssignOp::Eq,
                values: vec![Expr::Ident(nama, nilai.span())], span: a.span,
            };
            self.codegen_assign(&satu)?;
        }
        self.pop_scope();
        Ok(())
    }

    /// Tipe sebuah jalur murni, dibaca dari tabel tanpa memancarkan apa pun.
    pub(crate) fn tipe_jalur(&self, e: &Expr) -> Option<JrgType> {
        match e {
            Expr::Ident(n, _) => self.lookup_local(n).map(|(_, t)| t)
                .or_else(|| self.var_global.get(&self.k(n)).map(|(_, t)| t.clone())),
            Expr::Field { obj, field, .. } => match self.tipe_jalur(obj)? {
                JrgType::Struct(n) => self.types.structs.get(&n)?.field_type(field).cloned(),
                JrgType::Tuple(ts) => ts.get(field.parse::<usize>().ok()?).cloned(),
                _ => None,
            },
            Expr::Index { obj, .. } => match self.tipe_jalur(obj)? {
                JrgType::Slice(el) | JrgType::Array(el, _) => Some(*el),
                _ => None,
            },
            _ => None,
        }
    }

    /// `x = x.m(..)` (dan rantainya) atas struct milik lokal, untuk method yang mengubah `self`
    /// lalu mengembalikannya: dijalankan langsung di atas tempat `x`.
    pub(crate) fn coba_panggil_milik(&mut self, a: &AssignStmt) -> Result<bool, String> {
        if a.targets.len() != 1 || a.values.len() != 1 || !matches!(a.op, AssignOp::Eq) { return Ok(false); }
        let Expr::Ident(nama, _) = &a.targets[0] else { return Ok(false) };
        // `self = self.m(..)` boleh kalau `self` di sini memang milik method ini (salinan atau
        // varian milik), bukan penunjuk ke milik pemanggil.
        if nama == "self" && self.self_dipinjam { return Ok(false); }
        let mut rantai: Vec<(&String, &Vec<Expr>, crate::lexer::Span)> = Vec::new();
        let mut kini = &a.values[0];
        loop {
            match kini {
                Expr::Method { obj, method, args, span } => { rantai.push((method, args, *span)); kini = obj; }
                Expr::Ident(n, _) if n == nama && !rantai.is_empty() => break,
                _ => return Ok(false),
            }
        }
        rantai.reverse();
        // Nama yang ditangkap closure bisa DIBACA selagi method jalan (lewat closure yang
        // dipanggilnya), dan di sana ia harus masih bernilai lama.
        if self.tangkapan.contains(nama) { return Ok(false); }
        let Some((slot, ty)) = self.lookup_local(nama) else { return Ok(false) };
        let JrgType::Struct(tipe) = &ty else { return Ok(false) };
        let mut dekl = Vec::with_capacity(rantai.len());
        for (metode, args, _) in &rantai {
            if args.iter().any(|x| crate::interpreter::capture::names_mentioned(x).contains(nama.as_str())) { return Ok(false); }
            let Some(d) = self.tables.find_method(tipe, metode).cloned() else { return Ok(false) };
            let FnBody::Block(b) = &*d.body else { return Ok(false) };
            if !crate::interpreter::capture::blok_mengubah_lalu_kembali_self(b) { return Ok(false); }
            dekl.push(d);
        }
        for ((_, args, span), d) in rantai.iter().zip(dekl.iter()) {
            self.call_method_impl_dgn(tipe, d, &slot, &ty, args, true, *span)?;
        }
        Ok(true)
    }

    /// `A = A + f()`: nilai `A` dibaca SEBELUM `f` jalan. Kalau `f` bisa menulis `A` (`var` modul,
    /// nama yang dipakai bersama closure, `self` pinjaman), menyambung di tempat sesudahnya
    /// memakai nilai BARU — jalur biasa yang mengerjakannya.
    fn kanan_bisa_menulis(&self, akar: &str, kanan: &Expr) -> bool {
        let polos = self.lookup_local(akar).is_some() && !self.tangkapan.contains(akar)
            && !self.tangkap_masuk.contains(akar) && !(akar == "self" && self.self_dipinjam);
        !polos && crate::interpreter::capture::ada_panggilan(kanan)
    }

    /// Slot `{ pemilik, kap }` milik sebuah slot variabel, dibuat sekali dan bernilai nol.
    fn slot_kapasitas(&mut self, slot: &str) -> String {
        if let Some(k) = self.kap_tumbuh.get(slot) { return k.clone(); }
        let k = if slot.starts_with('@') {
            let k = format!("@gvkap.{}", self.kap_tumbuh.len());
            self.var_global_baris.push(format!("{} = internal global {{ ptr, i64 }} zeroinitializer", k));
            k
        } else {
            let k = self.alloca_raw("kap.", "{ ptr, i64 }", None);
            self.entry_allocas.push(format!("  store {{ ptr, i64 }} zeroinitializer, ptr {}", k));
            k
        };
        self.kap_tumbuh.insert(slot.to_string(), k.clone());
        k
    }
}
