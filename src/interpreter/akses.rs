// src/interpreter/akses.rs — cast, field, index, dan slice

use std::collections::HashMap;

use crate::parser::{
    AssignOp, BinaryOp, Block, BreakStmt, ContinueStmt, EnumDecl, EnumVariantKind,
    Expr, ExternBlock, ExternItem, ExternParam, FnBody,
    ForStmt, IfStmt, ImplItem, Item, LetStmt, LitKind, LoopStmt,
    MatchArm, MatchBody, MatchStmt, NamedType, OwnershipQual, ParsedFile, Pattern, PrimTy,
    ReturnStmt, SelectCase, SelectStmt, ShortDeclStmt, SpawnStmt, Stmt, TemplatePart, TypeExpr,
    UnaryOp, UsePath, VarStmt, WhileStmt,
    StructDecl, StructField, TraitDecl, TraitItem, Visibility,
};

use crate::interpreter::{
    Value, Env, Signal, RuntimeError, EvalResult, Interpreter, JrgFunction,
    ok_value, err_value, some_value, none_value,
    ChannelHandle, channel_new, channel_send, channel_recv,
    channel_try_send, channel_try_recv,
    enum_payload, enum_parts,
};
use crate::interpreter::builtin::{NativeFnKind, fisher_yates_shuffle};
use crate::runtime_err;
use crate::interpreter::eval_expr::{nama_tipe_sasaran, nilai_ke_usize, nilai_ke_i128, nilai_ke_f64};

impl Interpreter {
    /// `v as ty`. Aturannya di `konversi`, dan builtin `int()`/`uint()`/`float()`/
    /// `rune()` lewat sini juga — dulu keduanya salinan yang sudah menyimpang.
    pub(crate) fn eval_cast(&self, v: Value, ty: &TypeExpr) -> EvalResult {
        use crate::interpreter::konversi as k;
        let gagal = |e: String| Signal::RuntimeError(RuntimeError::new(e));
        match ty {
            TypeExpr::Primitive(prim) => {
                if let Some((nama, lebar)) = k::sasaran_bulat(prim) {
                    let n = match v {
                        Value::Int(n)    => k::cek_muat(n, nama, lebar),
                        Value::Float(f)  => k::float_ke_bulat(f, nama, lebar),
                        Value::Bool(b)   => Ok(b as i128),
                        Value::Rune(c)   => k::cek_muat_asli(c as i128, nama, lebar, || c.to_string()),
                        Value::Ptr(addr) => k::cek_muat_asli(addr as i128, nama, lebar,
                                                             || Value::Ptr(addr).to_string()),
                        Value::Str(s)    => k::teks_ke_bulat(&s, nama, lebar),
                        Value::Nil       => Err(k::pesan_nil_jadi(nama)),
                        lain             => Err(k::pesan_tak_bisa_jadi(&lain.type_name(), nama)),
                    };
                    return n.map(Value::Int).map_err(gagal);
                }
                match prim {
                    PrimTy::F32 | PrimTy::F64 | PrimTy::F128 => match v {
                        Value::Float(f) => Ok(Value::Float(f)),
                        Value::Int(n)   => Ok(Value::Float(n as f64)),
                        Value::Bool(b)  => Ok(Value::Float(b as u8 as f64)),
                        Value::Str(s)   => k::teks_ke_float(&s).map(Value::Float).map_err(gagal),
                        Value::Nil      => Err(gagal(k::pesan_nil_jadi("float"))),
                        lain            => Err(gagal(k::pesan_tak_bisa_jadi(&lain.type_name(), "float"))),
                    },
                    PrimTy::Bool => Ok(Value::Bool(self.truthy(&v))),
                    PrimTy::Str | PrimTy::String_ => Ok(Value::Str(format!("{}", v))),
                    PrimTy::Rune => match v {
                        Value::Rune(c) => Ok(Value::Rune(c)),
                        Value::Int(n)  => k::kode_ke_rune(n).map(Value::Rune).map_err(gagal),
                        lain => Err(gagal(k::pesan_tak_bisa_jadi(&lain.type_name(), "rune"))),
                    },
                    // Parser sudah menolak sasaran lain; ini cuma jaring pengaman.
                    lain => runtime_err!("{}", k::pesan_sasaran_cast_tak_sah(
                        &nama_tipe_sasaran(&TypeExpr::Primitive(lain.clone())))),
                }
            }
            TypeExpr::Named(n) => match crate::parser::prim_dari_nama(&n.name) {
                Some(p) => self.eval_cast(v, &TypeExpr::Primitive(p)),
                None => runtime_err!("{}", k::pesan_sasaran_cast_tak_sah(&n.name)),
            },
            // Ke tipe pointer (`*const T`, `*mut T`, `raw<T>`): 0 berarti `nil`.
            _ if k::sasaran_cast_sah(ty) => match v {
                Value::Ptr(addr) => Ok(Value::Ptr(addr)),
                Value::Nil       => Ok(Value::Nil),
                Value::Int(n)    => k::alamat_dari_bulat(n)
                    .map(|a| a.map_or(Value::Nil, Value::Ptr)).map_err(gagal),
                lain => Err(gagal(k::pesan_tak_bisa_jadi(&lain.type_name(), "pointer"))),
            },
            lain => runtime_err!("{}", k::pesan_sasaran_cast_tak_sah(&nama_tipe_sasaran(lain))),
        }
    }

    /// Nama field yang DIDEKLARASIKAN sebuah struct, termasuk warisannya — atau
    /// `None` kalau `tname` bukan struct yang dideklarasikan program.
    pub(crate) fn field_dideklarasikan(&self, tname: &str) -> Option<Vec<String>> {
        self.struct_defs.get(tname)?;
        let mut out: Vec<String> = Vec::new();
        for t in self.lineage_of(tname) {
            let Some(d) = self.struct_defs.get(&t) else { continue };
            for f in &d.fields {
                if !out.iter().any(|x| x == &f.name) { out.push(f.name.clone()); }
            }
        }
        Some(out)
    }

    /// Deklarasi `field` di sepanjang rantai warisan `tname`, TANPA alokasi: `None` kalau
    /// `tname` bukan struct milik program, `Some(None)` kalau field-nya tidak dideklarasikan,
    /// selain itu `(struct yang mendeklarasikan, privat?)`. Dilewati tiap akses field.
    pub(crate) fn deklarasi_field(&self, tname: &str, field: &str) -> Option<Option<(&str, bool)>> {
        let (mut nama, mut info) = self.struct_defs.get_key_value(tname).map(|(k, v)| (k.as_str(), v))?;
        for _ in 0..=64 {
            if let Some(f) = info.fields.iter().find(|f| f.name == field) {
                return Some(Some((nama, matches!(f.vis, Visibility::Private))));
            }
            let Some(p) = info.parent.as_deref() else { break };
            let Some((k, v)) = self.struct_defs.get_key_value(p) else { break };
            nama = k.as_str(); info = v;
        }
        Some(None)
    }

    /// Rantai pewarisan dari `tname` sendiri sampai leluhur paling atas (`[tname,
    /// parent, grandparent, ...]`), dipakai baik oleh `check_field_access` …
    pub(crate) fn lineage_of(&self, tname: &str) -> Vec<String> {
        let mut chain = vec![tname.to_string()];
        let mut cur = self.struct_defs.get(tname).and_then(|d| d.parent.clone());
        let mut guard = 0;
        while let Some(p) = cur {
            if chain.contains(&p) || guard > 64 { break; }
            guard += 1;
            cur = self.struct_defs.get(&p).and_then(|d| d.parent.clone());
            chain.push(p);
        }
        chain
    }

    /// ENCAPSULATION: field yang dideklarasikan tanpa `pub` (private) cuma boleh
    /// diakses (baca ATAU tulis) dari kode yang lagi jalan di dalam method milik …
    pub(crate) fn check_field_access(&self, obj: &Value, field: &str, env: &Env) -> Result<(), Signal> {
        let _ = env;
        self.check_field_access_as(obj, field, self.ruang_kini.as_deref().map(|s| s.as_str()))
    }

    /// Tipe `self` yang sedang berlaku di `env`, kalau ada.
    pub(crate) fn nama_lama_memori(segments: &[String]) -> Option<String> {
        let (kepala, sisa) = segments.split_first()?;
        let ns_baru = match kepala.as_str() {
            "new" => "jawa",
            "delete" => "jawir",
            _ => return None,
        };
        let mut out = ns_baru.to_string();
        for bagian in sisa {
            out.push_str("::");
            out.push_str(bagian);
        }
        Some(out)
    }

    /// Sama dengan `check_field_access`, tapi tipe `self`-nya diberikan
    /// langsung alih-alih dicari dari `env`.
    pub(crate) fn check_field_access_as(
        &self,
        obj: &Value,
        field: &str,
        self_type: Option<&str>,
    ) -> Result<(), Signal> {
        let Value::Struct { name: struct_name, .. } = obj else { return Ok(()) };
        // Field yang tidak dideklarasikan di mana pun sepanjang rantai (key dict, struct
        // hasil konstruksi native) dianggap publik. Jalur ini tidak mengalokasi apa pun.
        let Some(Some((pemilik, is_private))) = self.deklarasi_field(struct_name, field) else {
            return Ok(());
        };
        if !is_private { return Ok(()); }
        if let Some(st) = self_type {
            if st == struct_name { return Ok(()); }
            if self.lineage_of(struct_name).iter().any(|t| t == st)
                || self.lineage_of(st).contains(struct_name) {
                return Ok(());
            }
        }
        // Yang disebut adalah struct yang MENDEKLARASIKAN field-nya
        // (`pemilik`), bukan tipe objeknya — buat field warisan, di situlah
        // `pub` harus ditambahkan kalau memang mau dibuka.
        Err(Signal::RuntimeError(RuntimeError::new(format!(
            "field `{}` di `{}` bersifat private — cuma bisa diakses dari dalam method tipe itu sendiri atau turunannya. Tambahkan `pub` di depan field itu di deklarasi struct kalau memang harus publik.",
            field, pemilik
        ))))
    }

    pub(crate) fn eval_field(&mut self, obj: Value, field: &str) -> EvalResult {
        if matches!(obj, Value::PyObj(_) | Value::JsObj(_) | Value::PhpObj(_)) {
            return runtime_err!("{}", crate::interpreter::galat_index::field_handle_asing(&obj.type_name(), field));
        }
        match &obj {
            Value::Struct { fields, .. } => {
                if let Some(v) = fields.get(field) { return Ok(v.clone()); }
            }
            // `t.0`, `t.1`, ...
            Value::Tuple(t) => {
                return match field.parse::<i128>() {
                    Ok(ix) => match Self::resolve_index(ix, t.len()) {
                        Some(i) => Ok(t[i].clone()),
                        None => runtime_err!("{}", Self::oob_msg("tuple", "elemen", ix, t.len())),
                    },
                    Err(_) => runtime_err!(
                        "`.{}` bukan index tuple yang sah — setelah titik harus angka bulat, \
                         mis. `.0` untuk elemen pertama",
                        field
                    ),
                };
            }
            Value::Str(s) => match field {
                "len"      => return Ok(Value::Int(s.chars().count() as i128)),
                "is_empty" => return Ok(Value::Bool(s.is_empty())),
                _ => {}
            },
            Value::Array(a) => match field {
                "len"      => return Ok(Value::Int(a.len() as i128)),
                "is_empty" => return Ok(Value::Bool(a.is_empty())),
                _ => {}
            },
            _ => {}
        }

        // Bukan field beneran. Kalau namanya ternyata METHOD, bentuk tanpa `()`
        // ditolak: memanggilnya diam-diam membuat `typeof(x.m)` punya efek samping.
        use crate::interpreter::galat_index as gi;
        let tname = obj.type_name();
        let tipe = tname.rsplit(':').next().unwrap_or(&tname).to_string();
        let metode_pemakai = self.resolusi_metode(&tipe, field).is_some();
        // Method BAWAAN ditanyakan ke registry yang sedang berjalan, lewat
        // pemetaan namespace yang dipakai kedua mesin — bukan daftar tulis tangan.
        let metode_bawaan = !gi::pseudo_field(&tname, field)
            && gi::ns_metode_bawaan(&tname)
                .is_some_and(|ns| crate::vm::ada_builtin(&format!("{}::{}", ns, field)));
        if metode_pemakai || metode_bawaan {
            return Err(Signal::RuntimeError(RuntimeError::new(
                gi::metode_tanpa_kurung(&tipe, field))));
        }
        self.eval_method(obj, field, Vec::new())
    }

    /// Ubah index yang ditulis programmer (boleh negatif = hitung dari belakang)
    /// jadi posisi sebenarnya, atau `None` kalau di luar batas.
    pub(crate) fn resolve_index(raw: i128, len: usize) -> Option<usize> {
        let len_i = len as i128;
        let ix = if raw < 0 { raw + len_i } else { raw };
        if ix < 0 || ix >= len_i { None } else { Some(ix as usize) }
    }

    /// Pesan seragam untuk index di luar batas — dipakai array maupun
    /// string supaya keduanya menjelaskan hal yang sama dengan cara sama.
    pub(crate) fn oob_msg(what: &str, satuan: &str, raw: i128, len: usize) -> String {
        if len == 0 {
            format!(
                "index [{}] di luar batas — {} ini kosong (0 {}), jadi belum ada index yang valid",
                raw, what, satuan
            )
        } else {
            format!(
                "index [{}] di luar batas — {} ini isinya {} {} (index valid: 0..{}, \
                 atau -1..{} kalau mau hitung dari belakang)",
                raw, what, len, satuan, len - 1, -(len as i128)
            )
        }
    }

    pub(crate) fn eval_index(&self, obj: Value, idx: Value) -> EvalResult {
        match (obj, &idx) {
            (Value::Array(a), Value::Int(i)) => {
                match Self::resolve_index(*i, a.len()) {
                    Some(ix) => Ok(a[ix].clone()),
                    None => runtime_err!("{}", Self::oob_msg("array", "elemen", *i, a.len())),
                }
            }
            (Value::Str(s), Value::Int(i)) => {
                let ch: Vec<char> = s.chars().collect();
                match Self::resolve_index(*i, ch.len()) {
                    Some(ix) => Ok(Value::Rune(ch[ix])),
                    None => runtime_err!("{}", Self::oob_msg("string", "karakter", *i, ch.len())),
                }
            }
            // Dict dan struct sama-sama `Value::Struct`, tapi index-nya dijawab dengan
            // KOSAKATA yang berbeda: sebuah dict punya "key", sebuah struct punya …
            (Value::Struct { name, fields }, indeks) => {
                let k = format!("{}", indeks);
                match fields.get(k.as_str()) {
                    Some(v) => Ok(v.clone()),
                    None => {
                        let ada: Vec<String> = fields.keys().cloned().collect();
                        let pesan = if name == crate::interpreter::builtin::dict::DICT_TYPE_NAME {
                            crate::interpreter::galat_index::key_hilang(&k, &ada)
                        } else {
                            crate::interpreter::galat_index::field_hilang(&name, &k, &ada)
                        };
                        Err(Signal::RuntimeError(RuntimeError::new(pesan)))
                    }
                }
            }
            // Tuple pakai aturan index yang sama dengan array — termasuk
            // index negatif — supaya tidak ada dua aturan berbeda untuk
            // sintaks yang kelihatannya identik.
            (Value::Tuple(t), Value::Int(i)) => {
                match Self::resolve_index(*i, t.len()) {
                    Some(ix) => Ok(t[ix].clone()),
                    None => runtime_err!("{}", Self::oob_msg("tuple", "elemen", *i, t.len())),
                }
            }
            // `{:?}` DIHAPUS di sini.
            (o, Value::Array(_)) => runtime_err!(
                "{}",
                crate::interpreter::galat_index::range_dikira_potongan(&o.type_name())
            ),
            (o, i) => {
                let w = o.type_name();
                // Dua kesalahan yang berbeda, dan sebelumnya dijawab satu kalimat: sebuah
                // array yang di-index dengan string masih wadah yang sah (index-nya yang …
                if matches!(o, Value::Array(_) | Value::Str(_) | Value::Tuple(_)) {
                    runtime_err!("{}", crate::interpreter::galat_index::tipe_index_salah(&w, &i.type_name()))
                } else {
                    runtime_err!("{}", crate::interpreter::galat_index::bukan_wadah(&w))
                }
            }
        }
    }

    /// Ubah satu batas potongan jadi posisi sebenarnya.
    pub(crate) fn slice_bound(v: &Value, len: usize, sisi: &str) -> Result<usize, Signal> {
        let raw = match v {
            Value::Int(n) => *n,
            Value::Float(f) => return runtime_err!(
                "batas {} potongan harus bilangan bulat, dapat pecahan `{}` — \
                 tulis `{} as int` kalau memang mau dipotong ke bawah",
                sisi, f, f
            ),
            lain => return runtime_err!(
                "batas {} potongan harus bilangan bulat, dapat `{}` (tipe {})",
                sisi, lain, lain.type_name()
            ),
        };
        let len_i = len as i128;
        let pos = if raw < 0 { raw + len_i } else { raw };
        if pos < 0 || pos > len_i {
            return runtime_err!(
                "batas {} potongan [{}] di luar jangkauan — panjangnya {}, \
                 jadi batas yang sah 0..{} (atau -1..{} kalau dihitung dari belakang)",
                sisi, raw, len, len, -len_i
            );
        }
        Ok(pos as usize)
    }

    /// Validasi batas `obj[awal:akhir]` setelah keduanya jadi posisi nyata.
    pub(crate) fn slice_bounds(awal: usize, akhir: usize) -> Result<(usize, usize), Signal> {
        if awal > akhir {
            return runtime_err!(
                "potongan [{}:{}] terbalik — batas awal ({}) lebih besar dari batas akhir ({}); \
                 tulis yang kecil dulu, mis. [{}:{}]",
                awal, akhir, awal, akhir, akhir, awal
            );
        }
        Ok((awal, akhir))
    }

    pub(crate) fn eval_slice(&self, obj: Value, lo: Option<Value>, hi: Option<Value>) -> EvalResult {
        Self::potong_nilai(obj, lo, hi)
    }

    /// Aturan potong untuk array, string, dan tuple — bebas `self`, supaya VM
    /// bisa memanggil yang persis sama lewat jembatan nilai.
    pub fn potong_nilai(obj: Value, lo: Option<Value>, hi: Option<Value>) -> EvalResult {
        match obj {
            Value::Array(a) => {
                let n = a.len();
                let s = lo.as_ref().map(|v| Self::slice_bound(v, n, "awal")).transpose()?.unwrap_or(0);
                let e = hi.as_ref().map(|v| Self::slice_bound(v, n, "akhir")).transpose()?.unwrap_or(n);
                let (s, e) = Self::slice_bounds(s, e)?;
                Ok(Value::Array(a[s..e].to_vec().into()))
            }
            Value::Str(s) => Self::potong_str(&s, lo, hi),
            // Tuple ikut aturan index yang sama, jadi wajar juga bisa
            // dipotong — hasilnya tuple baru.
            Value::Tuple(t) => {
                let n = t.len();
                let s = lo.as_ref().map(|v| Self::slice_bound(v, n, "awal")).transpose()?.unwrap_or(0);
                let e = hi.as_ref().map(|v| Self::slice_bound(v, n, "akhir")).transpose()?.unwrap_or(n);
                let (s, e) = Self::slice_bounds(s, e)?;
                Ok(Value::Tuple(t[s..e].to_vec().into()))
            }
            o => runtime_err!(
                "tidak bisa slice `{}` (tipe {}) — yang bisa dipotong: array, string, tuple",
                o, o.type_name()
            ),
        }
    }


    // -----------------------------------------------------------------------
    // OOP Method Dispatch
    // -----------------------------------------------------------------------


}
