// src/interpreter/penugasan.rs — jalur target assignment

use std::collections::HashMap;

use crate::parser::{
    AssignOp, BinaryOp, Block, BreakStmt, ContinueStmt, DeferStmt, EnumDecl, EnumVariantKind,
    Expr, ExternBlock, ExternItem, ExternParam, FnBody,
    ForStmt, IfStmt, ImplItem, Item, LetStmt, LitKind, LoopStmt,
    MatchArm, MatchBody, MatchStmt, NamedType, OwnershipQual, ParsedFile, Pattern, PrimTy,
    ReturnStmt, SelectCase, SelectStmt, ShortDeclStmt, SpawnStmt, Stmt, TypeExpr,
    UnaryOp, UsePath, VarStmt, WhileStmt,
    StructDecl, StructField, TraitDecl, TraitItem, Visibility,
};

use crate::interpreter::{
    Value, Env, Signal, RuntimeError, EvalResult, Interpreter, JrgFunction, DeferredCall,
    ok_value, err_value, some_value, none_value,
    ChannelHandle, channel_send, channel_recv,
    channel_try_send, channel_try_recv,
};
use crate::runtime_err;


use crate::interpreter::eval_stmt::*;

/// Galat runtime yang lokasinya sudah diketahui di tempat lahirnya.
pub(crate) fn galat_di(pesan: String, span: crate::lexer::Span) -> Signal {
    let mut e = RuntimeError::new(pesan);
    e.span = Some(span);
    Signal::RuntimeError(e)
}

/// Nama akar sebuah jalur target (`x` pada `x[0].a`) beserta lokasinya.
fn akar_jalur(tgt: &Expr) -> Option<(&str, crate::lexer::Span)> {
    let mut kini = tgt;
    loop {
        match kini {
            Expr::Ident(name, span) => return Some((name.as_str(), *span)),
            Expr::Index { obj, .. } | Expr::Field { obj, .. } => kini = obj,
            _ => return None,
        }
    }
}

impl Interpreter {
    pub(crate) fn exec_assign(&mut self, s: &crate::parser::AssignStmt, env: &mut Env) -> EvalResult {
        if let Some(v) = self.coba_tumbuh_di_tempat(s, env)? { return Ok(v); }
        if let Some(v) = self.coba_ubah_di_tempat(s, env)?   { return Ok(v); }
        if let Some(v) = self.coba_tumbuh_jalur(s, env)?     { return Ok(v); }
        if let Some(v) = self.coba_konsumsi_penerima(s, env)? { return Ok(v); }

        // `a, b = e`: satu nilai untuk banyak sasaran berarti membongkar tuple.
        if s.targets.len() > 1 && s.values.len() == 1 { return self.exec_bongkar(s, env); }
        if s.targets.len() == 1 && s.values.len() == 1 {
            let v = self.eval_expr(&s.values[0], env)?;
            self.assign_target(&s.targets[0], s.op.clone(), v, env)?;
            return Ok(Value::Unit);
        }
        let mut rhs: Vec<Value> = Vec::with_capacity(s.values.len());
        for e in &s.values { rhs.push(self.eval_expr(e, env)?); }
        for (i, tgt) in s.targets.iter().enumerate() {
            let r = rhs.get(i).cloned().unwrap_or(Value::Unit);
            self.assign_target(tgt, s.op.clone(), r, env)?;
        }
        Ok(Value::Unit)
    }

    pub(crate) fn assign_target(&mut self, tgt: &Expr, op: AssignOp, rhs: Value, env: &mut Env) -> EvalResult {
        // Variabel telanjang: jalur paling umum, tidak perlu menyusuri apa pun.
        if let Expr::Ident(name, span) = tgt {
            // `_ = f()` membuang nilainya, di tiga mesin; membacanya ditolak parser.
            if name == "_" && matches!(op, AssignOp::Eq) { return Ok(Value::Unit); }
            // Nama yang belum dideklarasikan DITOLAK. Dulu `x = 5` mendefinisikannya di lingkup
            // blok ini (dan sebagai global di VM), jadi salah ketik `totl = total + 1` lolos.
            let belum = |env: &Env| galat_di(env.pesan_sasaran_tugas(name), *span);
            let new_val = if matches!(op, AssignOp::Eq) { rhs } else {
                let cur = env.get_variabel(name).ok_or_else(|| belum(env))?;
                // Galat operatornya lahir tanpa lokasi; di VM caret-nya di nama sasarannya.
                self.apply_assign_op(op, cur, rhs).map_err(|e| match e {
                    Signal::RuntimeError(r) => Signal::RuntimeError(r.fill_span(*span)),
                    lain => lain,
                })?
            };
            if !env.set_variabel(name, new_val) { return Err(belum(env)); }
            return Ok(Value::Unit);
        }

        // Akar jalur diperiksa DULU, sebelum index mana pun dihitung: `x[f()] = 1` atas `x`
        // yang tidak ada gagal tanpa memanggil `f`, urutan yang sama dengan VM.
        if let Some((akar, span)) = akar_jalur(tgt) {
            if env.get(akar).is_none() {
                return Err(galat_di(env.pesan_sasaran_tugas(akar), span));
            }
        }
        // Jalur bersarang: `a[i]`, `p.x`, `grid[0][1]`, `daftar[0].nama`,
        // `d["k"][2].isi`, dan seterusnya.
        let (root, steps) = self.flatten_assign_path(tgt, env)?;
        // `b.s += x` / `a[i] += x` atas teks/array: nilai lamanya DIPINDAH keluar dan disambung
        // di tempat (dulu disalin utuh tiap kali — kuadratik). Kanan yang butuh hook tampilan
        // tidak lewat sini: hook bisa membaca wadahnya sendiri, yang sementara berisi nil.
        let boleh_ambil = matches!(op, AssignOp::PlusEq)
            && (!Self::perlu_tampil(&rhs) || matches!(rhs, Value::Array(_)));
        if boleh_ambil {
            let lama = self.tulis_jalur(&root, &steps, None, true, env)?
                .expect("lintasan ambil selalu membawa nilai lama");
            let bisa_sambung = match &lama {
                Value::Str(_) => !Self::perlu_tampil(&rhs),
                Value::Array(_) => true,
                _ => false,
            };
            if bisa_sambung {
                let mut acc = lama;
                let awal = crate::interpreter::eval_stmt::panjang_koleksi(&acc);
                let mut cadangan = None;
                return match self.tambah_di_tempat(&mut acc, rhs, &mut cadangan) {
                    Ok(()) => { self.tulis_jalur(&root, &steps, Some(acc), false, env)?; Ok(Value::Unit) }
                    Err(e) => {
                        let asli = crate::interpreter::eval_stmt::pulihkan_koleksi(cadangan.unwrap_or(acc), awal);
                        self.tulis_jalur(&root, &steps, Some(asli), false, env)?;
                        Err(match steps.last() { Some(t) => beri_span(e, t), None => e })
                    }
                };
            }
            // Teks + array: hook tampilan elemennya bisa berjalan, jadi kembalikan dulu nilainya.
            if matches!(lama, Value::Str(_) | Value::Array(_)) {
                self.tulis_jalur(&root, &steps, Some(lama.clone()), false, env)?;
            }
            let baru = self.apply_assign_op(op, lama, rhs)
                .map_err(|e| match steps.last() { Some(t) => beri_span(e, t), None => e })?;
            self.tulis_jalur(&root, &steps, Some(baru), false, env)?;
            return Ok(Value::Unit);
        }
        // Operator gabungan (`a[i] += v`): baca nilai lama lewat penelusuran yang SAMA,
        // hitung dengan aturan `Expr::Binary` (operator pemakai, hook tampilan) — butuh
        // `&mut self`, jadi di luar pinjaman binding — lalu tulis sebagai `=`.
        if !matches!(op, AssignOp::Eq) {
            let lama = self.tulis_jalur(&root, &steps, None, false, env)?
                .expect("lintasan ambil selalu membawa nilai lama");
            let baru = self.apply_assign_op(op, lama, rhs)
                .map_err(|e| match steps.last() { Some(t) => beri_span(e, t), None => e })?;
            self.tulis_jalur(&root, &steps, Some(baru), false, env)?;
            return Ok(Value::Unit);
        }
        self.tulis_jalur(&root, &steps, Some(rhs), false, env)?;
        Ok(Value::Unit)
    }

    /// Telusuri `root` + `steps` lalu, di langkah terakhir, tulis `nilai` — atau, kalau
    /// `nilai` kosong, kembalikan isi lamanya tanpa menulis apa pun.
    fn tulis_jalur(
        &mut self, root: &str, steps: &[Langkah], nilai: Option<Value>, ambil: bool, env: &mut Env,
    ) -> Result<Option<Value>, Signal> {

        // Tipe `self` dihitung DULU, selagi `env` masih bisa dipinjam
        // biasa — pengecekan visibilitas field di bawah tidak lagi butuh
        // `env`, jadi binding-nya bisa dipinjam mutable tanpa bentrok.
        let self_type = self.ruang_kini.clone();

        // Nilai akar diubah DI TEMPAT, bukan disalin-ubah-tulis balik.
        let me = &*self;
        let mut nilai_baru = nilai;
        let hasil = env.with_binding_mut(root, |nilai| -> Result<Option<Value>, Signal> {
            // Telusuri semua langkah KECUALI yang terakhir; yang terakhir
            // ditangani terpisah karena cuma di situ key dict baru boleh
            // dibuat, dan cuma di situ operator gabungan (`+=`) dipakai.
            let (terakhir, awal) = steps.split_last()
                .expect("jalur bersarang selalu punya minimal satu langkah");
            let mut kursor = nilai;
            for langkah in awal {
                kursor = me.step_mut(kursor, langkah).map_err(|e| beri_span(e, langkah))?;
            }

            // `t.0 = v` atas tuple adalah `t[0] = v`.
            let ganti;
            let terakhir = match Self::field_tuple(kursor, terakhir) {
                Some(l) => { ganti = l; &ganti }
                None => terakhir,
            };
            match terakhir {
            Langkah::Field(nama, _) => {
                me.check_field_access_as(&*kursor, nama, self_type.as_deref().map(|s| s.as_str()))?;
                let Value::Struct { name, fields } = kursor else {
                    return runtime_err!("{}", crate::interpreter::galat_index::bukan_pemilik_field(
                        &kursor.type_name(), nama));
                };
                // Handle pustaka (bukan struct bernama sama milik program) tidak bisa ditulisi.
                if crate::interpreter::galat_index::HANDLE_PUSTAKA.contains(&name.as_str())
                    && me.field_dideklarasikan(name).is_none()
                {
                    return runtime_err!("{}",
                        crate::interpreter::galat_index::tulis_handle(name, nama));
                }
                // Field yang tidak dideklarasikan ditolak untuk struct yang dideklarasikan
                // PROGRAM.
                if !fields.contains_key(nama.as_str()) {
                    if let Some(dideklarasikan) = me.field_dideklarasikan(name) {
                        let saran = crate::diag::saran_nama_terdekat(
                            nama, dideklarasikan.iter().map(|x| x.as_str()));
                        return runtime_err!("{}",
                            crate::interpreter::galat_index::field_tak_dideklarasikan(
                                name, nama, saran.as_deref()));
                    }
                }
                let Some(baru) = nilai_baru.take() else {
                    return Ok(Some(match fields.get_mut(nama) {
                        Some(v) => keluarkan(v, ambil),
                        None => Value::Nil,
                    }));
                };
                fields.insert(nama.clone(), baru);
            }
            Langkah::Index(ixv, _) => { let wadah = if matches!(kursor, Value::Tuple(_)) { "tuple" } else { "array" };
            match (&mut *kursor, ixv) {
                // dict: key apa pun (dipakai bentuk Display-nya), boleh key baru — ini
                // satu-satunya langkah yang boleh menambah entri.
                (Value::Struct { fields, .. }, _) => {
                    let key = format!("{}", ixv);
                    let Some(baru) = nilai_baru.take() else {
                        return Ok(Some(match fields.get_mut(key.as_str()) {
                            Some(v) => keluarkan(v, ambil),
                            None => Value::Nil,
                        }));
                    };
                    fields.insert(key, baru);
                }
                // Array dan tuple: aturan index PERSIS sama dengan pembacaannya — int saja,
                // negatif dari belakang, di luar batas error. Tuple ikut sejak Fase TULIS-TUPLE,
                // lewat `Bersama` yang sama, jadi salinan lain tidak tersentuh.
                (Value::Array(elems) | Value::Tuple(elems), _) => {
                    let Value::Int(raw) = ixv else {
                        return runtime_err!("{}", crate::interpreter::galat_index::tipe_index_salah(
                            wadah, &ixv.type_name()));
                    };
                    let raw = *raw;
                    let len = elems.len();
                    let Some(ix) = Self::resolve_index(raw, len) else {
                        return runtime_err!("{}", Self::oob_msg(wadah, "elemen", raw, len));
                    };
                    let Some(baru) = nilai_baru.take() else { return Ok(Some(keluarkan(&mut elems[ix], ambil))) };
                    elems[ix] = baru;
                }
                (other, _) => {
                    return runtime_err!("{}", crate::interpreter::galat_index::tak_bisa_ditulis_lewat_index(
                        &other.type_name()));
                }
            } },
            }
            Ok(None)
        });

        // Langkah TERAKHIR ditangani di dalam closure tanpa lewat `step_mut`, jadi
        // lokasinya diisi di sini.
        let hasil = match (hasil, steps.last()) {
            (Some(Err(e)), Some(t)) => Some(Err(beri_span(e, t))),
            (lain, _) => lain,
        };
        match hasil {
            Some(r) => r,
            None => runtime_err!("{}", env.pesan_sasaran_tugas(root)),
        }
    }

    /// Bongkar target assignment jadi nama variabel akar + urutan langkah.
    ///
    /// Ekspresi index dievaluasi di sini, sekali, dari kiri ke kanan.
    pub(crate) fn flatten_assign_path(
        &mut self,
        tgt: &Expr,
        env: &mut Env,
    ) -> Result<(String, Vec<Langkah>), Signal> {
        let mut steps: Vec<Langkah> = Vec::new();
        let mut kini = tgt;
        let root = loop {
            match kini {
                Expr::Ident(name, _) => break name.clone(),
                Expr::Index { obj, idx, .. } => {
                    let v = self.eval_expr(idx, env)?;
                    steps.push(Langkah::Index(v, kini.span()));
                    kini = obj;
                }
                Expr::Field { obj, field, .. } => {
                    steps.push(Langkah::Field(field.clone(), kini.span()));
                    kini = obj;
                }
                lain => {
                    return runtime_err!("{}", crate::interpreter::pesan_bukan_sasaran(lain));
                }
            }
        };
        // Dikumpulkan dari luar ke dalam, jadi dibalik supaya urutannya
        // sama dengan urutan penulisannya.
        steps.reverse();
        Ok((root, steps))
    }

    /// `.N` atas sebuah tuple, dibaca sebagai langkah `[N]`.
    fn field_tuple(kursor: &Value, langkah: &Langkah) -> Option<Langkah> {
        let (Value::Tuple(_), Langkah::Field(nama, sp)) = (kursor, langkah) else { return None };
        if nama.is_empty() || !nama.chars().all(|c| c.is_ascii_digit()) { return None; }
        nama.parse::<i128>().ok().map(|n| Langkah::Index(Value::Int(n), *sp))
    }

    /// Turun satu langkah, mengembalikan tempat yang bisa diubah.
    pub(crate) fn step_mut<'v>(
        &self,
        kursor: &'v mut Value,
        langkah: &Langkah,
    ) -> Result<&'v mut Value, Signal> {
        use crate::interpreter::builtin::dict::DICT_TYPE_NAME;
        use crate::interpreter::galat_index::{
            belum_ada_untuk_ditulis, bukan_pemilik_field, sebutan_anggota,
            tak_bisa_ditulis_lewat_index, tipe_index_salah,
        };
        let ganti;
        let langkah = match Self::field_tuple(kursor, langkah) {
            Some(l) => { ganti = l; &ganti }
            None => langkah,
        };
        match langkah {
            // Kalimatnya sama dengan langkah `[...]` di bawah dan dengan VM: menulis
            // lewat `d.a` dan lewat `d["a"]` adalah satu kesalahan, bukan dua.
            Langkah::Field(nama, _) => match kursor {
                Value::Struct { name, fields } => {
                    let sebutan = sebutan_anggota(name == DICT_TYPE_NAME);
                    fields.get_mut(nama).ok_or_else(|| {
                        Signal::RuntimeError(RuntimeError::new(belum_ada_untuk_ditulis(sebutan, nama)))
                    })
                }
                lain => runtime_err!("{}", bukan_pemilik_field(&lain.type_name(), nama)),
            },
            Langkah::Index(ixv, _) => { let wadah = if matches!(kursor, Value::Tuple(_)) { "tuple" } else { "array" };
            match (&mut *kursor, ixv) {
                // Index DI-STRINGKAN, aturan yang sama dengan jalur baca dan dengan seluruh
                // keluarga `dict::`.
                (Value::Struct { name, fields }, _) => {
                    let sebutan = sebutan_anggota(name == DICT_TYPE_NAME);
                    let key = format!("{}", ixv);
                    // Key antara harus SUDAH ada: `d["a"]["b"] = 1` pada
                    // `d` yang belum punya "a" tidak boleh diam-diam
                    // membuat dict kosong di tengah jalan.
                    fields.get_mut(key.as_str()).ok_or_else(|| {
                        Signal::RuntimeError(RuntimeError::new(belum_ada_untuk_ditulis(sebutan, &key)))
                    })
                }
                (Value::Array(elems) | Value::Tuple(elems), _) => {
                    let Value::Int(raw) = ixv else {
                        return runtime_err!("{}", tipe_index_salah(wadah, &ixv.type_name()));
                    };
                    let (raw, len) = (*raw, elems.len());
                    match Self::resolve_index(raw, len) {
                        Some(ix) => Ok(&mut elems[ix]),
                        None => runtime_err!("{}", Self::oob_msg(wadah, "elemen", raw, len)),
                    }
                }
                (lain, _) => runtime_err!("{}", tak_bisa_ditulis_lewat_index(&lain.type_name())),
            } },
        }
    }

    /// Gambaran singkat sebuah ekspresi untuk pesan error, tanpa membocorkan
    /// bentuk `{:?}` AST yang tidak berarti apa-apa buat programmer.
    pub(crate) fn gambar_expr(e: &Expr) -> String {
        match e {
            Expr::Ident(n, _) => n.clone(),
            Expr::Call { .. } => "hasil pemanggilan fungsi".to_string(),
            Expr::Method { .. } => "hasil pemanggilan method".to_string(),
            Expr::Literal(..) => "nilai literal".to_string(),
            Expr::Unary { op: UnaryOp::Deref, .. } => "isi pointer (`*p`)".to_string(),
            Expr::Binary { .. } => "hasil operasi".to_string(),
            Expr::Slice { .. } => "potongan (`x[a:b]`)".to_string(),
            _ => "ekspresi ini".to_string(),
        }
    }

    /// `a op= b` adalah `a = a op b`, dengan urutan aturan yang SAMA dengan `Expr::Binary`:
    /// operator milik pemakai, lalu hook tampilan untuk `+`, lalu aturan bawaan. Dulu
    /// langsung `op_bin`, jadi `p += q` atas struct ber-`add` ditolak sementara `--vm` jalan.
    pub(crate) fn apply_assign_op(&mut self, op: AssignOp, lhs: Value, rhs: Value) -> EvalResult {
        let bop = match op {
            AssignOp::Eq        => return Ok(rhs),
            AssignOp::PlusEq    => BinaryOp::Add,
            AssignOp::PowEq     => BinaryOp::Pow,
            AssignOp::MinusEq   => BinaryOp::Sub,
            AssignOp::StarEq    => BinaryOp::Mul,
            AssignOp::SlashEq   => BinaryOp::Div,
            AssignOp::PercentEq => BinaryOp::Rem,
            AssignOp::AndEq     => BinaryOp::BitAnd,
            AssignOp::OrEq      => BinaryOp::BitOr,
            AssignOp::CaretEq   => BinaryOp::BitXor,
            AssignOp::ShlEq     => BinaryOp::Shl,
            AssignOp::ShrEq     => BinaryOp::Shr,
        };
        if let Some(h) = self.operator_biner_pengguna(&bop, &lhs, &rhs)? { return Ok(h); }
        let (lhs, rhs) = self.siapkan_gabung_string(&bop, lhs, rhs)?;
        self.op_bin(bop, lhs, rhs)
    }

}

/// Isi lama sebuah tempat: teks/array DIPINDAH keluar (meninggalkan nil sementara) kalau
/// `ambil`, selain itu disalin.
fn keluarkan(v: &mut Value, ambil: bool) -> Value {
    if ambil && matches!(v, Value::Str(_) | Value::Array(_)) {
        std::mem::replace(v, Value::Nil)
    } else {
        v.clone()
    }
}
