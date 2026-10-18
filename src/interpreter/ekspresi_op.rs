// src/interpreter/ekspresi_op.rs — operator: literal, unary, biner, banding,
// urut

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
    pub(crate) fn eval_lit(&self, lit: &LitKind) -> Value {
        match lit {
            LitKind::Int(n)    => Value::Int(*n as i128),
            LitKind::Float(f)  => Value::Float(*f),
            LitKind::Imag(f)   => Value::Float(*f),
            LitKind::Rune(c)   => Value::Rune(*c),
            LitKind::Str(s)    => Value::Str(s.clone()),
            LitKind::RawStr(s) => Value::Str(s.clone()),
            LitKind::Bool(b)   => Value::Bool(*b),
            LitKind::Nil       => Value::Nil,
        }
    }

    pub(crate) fn eval_unary(&self, op: &UnaryOp, v: Value) -> EvalResult {
        match op {
            UnaryOp::Neg => match v {
                Value::Int(n)   => n.checked_neg().map(Value::Int)
                    .ok_or_else(|| Signal::RuntimeError(RuntimeError::new(
                        crate::interpreter::pesan_neg_min()))),
                Value::Float(f) => Ok(Value::Float(-f)),
                _ => runtime_err!("{}", crate::interpreter::pesan_op_unary("-", &v.type_name())),
            },
            // `!` adalah NEGASI LOGIKA, dan cuma untuk bool.
            UnaryOp::Not => match v {
                Value::Bool(b) => Ok(Value::Bool(!b)),
                Value::Int(_)  => runtime_err!(
                    "operator `!` cuma untuk bool, dapat `int` — kalau maksudnya \
                     membalik bit, pakai `~`; kalau maksudnya \"nilainya nol atau \
                     bukan\", tulis perbandingannya eksplisit (mis. `x == 0`)"
                ),
                _ => runtime_err!("{}", crate::interpreter::pesan_op_unary("!", &v.type_name())),
            },
            UnaryOp::BitNot => match v {
                Value::Int(n) => Ok(Value::Int(!n)),
                _ => runtime_err!("{}", crate::interpreter::pesan_op_unary("~", &v.type_name())),
            },
            UnaryOp::Deref | UnaryOp::Ref | UnaryOp::RefMut | UnaryOp::Move => Ok(v),
        }
    }

    // Core binary operator dispatch
    pub(crate) fn op_bin(&self, op: BinaryOp, a: Value, b: Value) -> EvalResult {
        // Petunjuk "operand-nya fungsi" sekarang ikut `pesan_op_biner`, jadi
        // kedua mesin mendapatkannya dari satu tempat.
        self.op_bin_inner(op, a, b)
    }

    pub(crate) fn op_bin_inner(&self, op: BinaryOp, a: Value, b: Value) -> EvalResult {
        match op {
            BinaryOp::Add    => self.v_add(a, b),
            BinaryOp::Sub    => self.v_sub(a, b),
            BinaryOp::Mul    => self.v_mul(a, b),
            BinaryOp::Div    => self.v_div(a, b),
            BinaryOp::Rem    => self.v_rem(a, b),
            BinaryOp::Pow    => self.v_pow(a, b),
            BinaryOp::BitAnd => self.v_ibin(a, b, |x, y| x & y, "&"),
            BinaryOp::BitOr  => self.v_ibin(a, b, |x, y| x | y, "|"),
            BinaryOp::BitXor => self.v_ibin(a, b, |x, y| x ^ y, "^"),
            BinaryOp::Shl    => self.v_shift(a, b, true),
            BinaryOp::Shr    => self.v_shift(a, b, false),
            BinaryOp::AndNot => self.v_ibin(a, b, |x, y| x & !y, "&^"),
            BinaryOp::Eq     => Ok(Value::Bool(self.val_eq(&a, &b)?)),
            BinaryOp::Ne     => Ok(Value::Bool(!self.val_eq(&a, &b)?)),
            BinaryOp::Lt     => self.v_cmp(a, b, std::cmp::Ordering::Less, false),
            BinaryOp::Le     => self.v_cmp(a, b, std::cmp::Ordering::Less, true),
            BinaryOp::Gt     => self.v_cmp(a, b, std::cmp::Ordering::Greater, false),
            BinaryOp::Ge     => self.v_cmp(a, b, std::cmp::Ordering::Greater, true),
            BinaryOp::And    => Ok(Value::Bool(self.truthy(&a) && self.truthy(&b))),
            BinaryOp::Or     => Ok(Value::Bool(self.truthy(&a) || self.truthy(&b))),
            // `BinaryOp::Chan` (`token_to_binary_op`'s mapping of `<-`) is unreachable
            // in practice: `<-` is not in `infix_bp`, so the Pratt parser never actually …
            BinaryOp::Chan   => runtime_err!("channel operator tidak didukung"),
        }
    }

    /// Pesan seragam untuk integer yang kelewat besar — kalimatnya milik bersama.
    pub(crate) fn overflow_err(op: &str, x: i128, y: i128) -> Signal {
        Signal::RuntimeError(RuntimeError::new(crate::interpreter::pesan_luber(op, x, y)))
    }

    pub(crate) fn v_add(&self, a: Value, b: Value) -> EvalResult {
        match (a, b) {
            (Value::Int(x),   Value::Int(y))   => x.checked_add(y).map(Value::Int)
                .ok_or_else(|| Self::overflow_err("+", x, y)),
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x + y)),
            (Value::Int(x),   Value::Float(y)) => Ok(Value::Float(x as f64 + y)),
            (Value::Float(x), Value::Int(y))   => Ok(Value::Float(x + y as f64)),
            (Value::Str(x),   Value::Str(y))   => Ok(Value::Str(x + &y)),
            // Menyambung ke `x` yang sudah dimiliki, bukan `format!` yang menyalinnya utuh.
            (Value::Str(mut x), r) => {
                use std::fmt::Write;
                write!(x, "{}", r).expect("Display Value tidak boleh gagal");
                Ok(Value::Str(x))
            }
            (l,               Value::Str(y))   => Ok(Value::Str(format!("{}{}", l, y))),
            // Gabung dua array jadi array baru — semantik yang SAMA dengan method
            // `.concat()` yang sudah ada, cuma lewat operator.
            (Value::Array(mut x), Value::Array(y)) => { x.extend(y); Ok(Value::Array(x)) }
            (l, r) => runtime_err!("{}", crate::interpreter::pesan_op_biner(
                "+", &l.type_name(), &r.type_name())),
        }
    }
    pub(crate) fn v_sub(&self, a: Value, b: Value) -> EvalResult {
        match (a, b) {
            (Value::Int(x),   Value::Int(y))   => x.checked_sub(y).map(Value::Int)
                .ok_or_else(|| Self::overflow_err("-", x, y)),
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x - y)),
            (Value::Int(x),   Value::Float(y)) => Ok(Value::Float(x as f64 - y)),
            (Value::Float(x), Value::Int(y))   => Ok(Value::Float(x - y as f64)),
            (l, r) => runtime_err!("{}", crate::interpreter::pesan_op_biner(
                "-", &l.type_name(), &r.type_name())),
        }
    }
    pub(crate) fn v_mul(&self, a: Value, b: Value) -> EvalResult {
        match (a, b) {
            (Value::Int(x),   Value::Int(y))   => x.checked_mul(y).map(Value::Int)
                .ok_or_else(|| Self::overflow_err("*", x, y)),
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x * y)),
            (Value::Int(x),   Value::Float(y)) => Ok(Value::Float(x as f64 * y)),
            (Value::Float(x), Value::Int(y))   => Ok(Value::Float(x * y as f64)),
            // Pengulangan string. Dulu jumlah negatif dijepit ke 0, jadi
            // `"ab" * -3` diam-diam memberi string kosong — tidak bisa
            // dibedakan dari pengulangan nol kali yang memang disengaja.
            (Value::Str(s),   Value::Int(n))   => Self::v_repeat_str(&s, n),
            (Value::Int(n),   Value::Str(s))   => Self::v_repeat_str(&s, n),
            (l, r) => runtime_err!("{}", crate::interpreter::pesan_op_biner(
                "*", &l.type_name(), &r.type_name())),
        }
    }
    /// `str * n` — aturannya ada di [`crate::interpreter::ulang_string`],
    /// dipakai bersama dengan VM supaya tidak menyimpang lagi.
    pub(crate) fn v_repeat_str(s: &str, n: i128) -> EvalResult {
        match crate::interpreter::ulang_string(s, n) {
            Ok(hasil) => Ok(Value::Str(hasil)),
            Err(pesan) => runtime_err!("{}", pesan),
        }
    }

    pub(crate) fn v_div(&self, a: Value, b: Value) -> EvalResult {
        match (a, b) {
            (Value::Int(x),   Value::Int(y)) => {
                if y == 0 { return runtime_err!("{}", crate::interpreter::pesan_bagi_nol("/", &x.to_string())); }
                // `checked_div`, bukan `/` telanjang: ada TEPAT SATU pembagian yang meluap,
                // yaitu `i128::MIN / -1` (hasilnya `i128::MAX + 1`).
                x.checked_div(y).map(Value::Int)
                    .ok_or_else(|| Self::overflow_err("/", x, y))
            }
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x / y)),
            (Value::Int(x),   Value::Float(y)) => Ok(Value::Float(x as f64 / y)),
            (Value::Float(x), Value::Int(y))   => Ok(Value::Float(x / y as f64)),
            (l, r) => runtime_err!("{}", crate::interpreter::pesan_op_biner(
                "/", &l.type_name(), &r.type_name())),
        }
    }
    pub(crate) fn v_rem(&self, a: Value, b: Value) -> EvalResult {
        match (a, b) {
            (Value::Int(x),   Value::Int(y)) => {
                if y == 0 { return runtime_err!("{}", crate::interpreter::pesan_bagi_nol("%", &x.to_string())); }
                // Sama seperti `/`: `i128::MIN % -1` meluap dan dulu bikin
                // interpreter panik alih-alih memberi diagnostik JRG.
                x.checked_rem(y).map(Value::Int)
                    .ok_or_else(|| Self::overflow_err("%", x, y))
            }
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x % y)),
            // Sama dengan `/`: bulat lawan pecahan jadi pecahan. Dulu `7.5 / 2` = 3.75 tapi
            // `7.5 % 2` ditolak.
            (Value::Int(x),   Value::Float(y)) => Ok(Value::Float(x as f64 % y)),
            (Value::Float(x), Value::Int(y))   => Ok(Value::Float(x % y as f64)),
            (l, r) => runtime_err!("{}", crate::interpreter::pesan_op_biner(
                "%", &l.type_name(), &r.type_name())),
        }
    }
    pub(crate) fn v_pow(&self, a: Value, b: Value) -> EvalResult {
        match (a, b) {
            (Value::Int(x),   Value::Int(y)) => {
                if y < 0 { return Ok(Value::Float(Self::pangkat_bulat(x as f64, y))); }
                Self::pangkat_int(x, y).map(Value::Int)
                    .ok_or_else(|| Self::overflow_err("**", x, y))
            }
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x.powf(y))),
            (Value::Int(x),   Value::Float(y)) => Ok(Value::Float((x as f64).powf(y))),
            (Value::Float(x), Value::Int(y))   => Ok(Value::Float(Self::pangkat_bulat(x, y))),
            (l, r) => runtime_err!("{}", crate::interpreter::pesan_op_biner(
                "**", &l.type_name(), &r.type_name())),
        }
    }
    /// `<<` dan `>>`.
    pub(crate) fn v_shift(&self, a: Value, b: Value, ke_kiri: bool) -> EvalResult {
        let op = if ke_kiri { "<<" } else { ">>" };
        let (Value::Int(x), Value::Int(y)) = (&a, &b) else {
            return runtime_err!("{}", crate::interpreter::pesan_op_bit(
                op, &a.type_name(), &b.type_name()));
        };
        match Self::geser_terperiksa(*x, *y, ke_kiri) {
            Ok(v) => Ok(Value::Int(v)),
            Err(e) => runtime_err!("{}", e),
        }
    }

    /// `x ** y` untuk eksponen BULAT, hasil pecahan: `powi` selama eksponennya muat `i32`,
    /// `powf` di luar itu — `y as i32` memotong, jadi `2 ** -4294967297` dulu menjawab `0.5`.
    pub(crate) fn pangkat_bulat(x: f64, y: i128) -> f64 {
        match i32::try_from(y) { Ok(e) => x.powi(e), Err(_) => x.powf(y as f64) }
    }

    /// `x ** y` antar bilangan bulat, `y >= 0`; `None` kalau meluap 128 bit. Eksponen di atas
    /// `u32` cuma muat untuk basis -1, 0 dan 1 — dulu ketiganya ikut dijawab "meluap".
    pub(crate) fn pangkat_int(x: i128, y: i128) -> Option<i128> {
        match u32::try_from(y) {
            Ok(e) => x.checked_pow(e),
            Err(_) => match x {
                0 | 1 => Some(x),
                -1 => Some(if y % 2 == 0 { 1 } else { -1 }),
                _ => None,
            },
        }
    }

    /// Aturan geser bit, terpisah dari `Value` supaya VM bisa memakai yang SAMA
    /// alih-alih menumbuhkan salinan kedua (yang, waktu ia memang ada, langsung …
    pub(crate) fn geser_terperiksa(x: i128, y: i128, ke_kiri: bool) -> Result<i128, String> {
        let op = if ke_kiri { "<<" } else { ">>" };
        if y < 0 {
            return Err(format!(
                "`{} {} {}` — jumlah geseran tidak boleh negatif; kalau maksudnya menggeser \
                 ke arah sebaliknya, pakai `{}` dengan jumlah positif",
                x, op, y, if ke_kiri { ">>" } else { "<<" }
            ));
        }
        if y >= 128 {
            return Err(format!(
                "`{} {} {}` — integer JRG cuma 128 bit, jadi menggeser {} kali tidak punya arti \
                 (semua bitnya sudah keluar jauh sebelum itu)",
                x, op, y, y
            ));
        }
        let n = y as u32;
        match if ke_kiri { x.checked_shl(n) } else { x.checked_shr(n) } {
            Some(v) => Ok(v),
            None => Err(format!("`{} {} {}` gagal dihitung", x, op, y)),
        }
    }

    pub(crate) fn v_ibin<F: Fn(i128, i128) -> i128>(&self, a: Value, b: Value, f: F, op: &str) -> EvalResult {
        match (a, b) {
            (Value::Int(x), Value::Int(y)) => Ok(Value::Int(f(x, y))),
            (l, r) => runtime_err!("{}", crate::interpreter::pesan_op_bit(
                op, &l.type_name(), &r.type_name())),
        }
    }
    /// Urutan antara dua nilai untuk keperluan pengurutan, atau `None` kalau
    /// keduanya memang tidak bisa dibandingkan.
    pub(crate) fn cmp_values(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
        use std::cmp::Ordering;
        match (a, b) {
            (Value::Int(x),   Value::Int(y))   => Some(x.cmp(y)),
            (Value::Str(x),   Value::Str(y))   => Some(x.cmp(y)),
            (Value::Rune(x),  Value::Rune(y))  => Some(x.cmp(y)),
            (Value::Bool(x),  Value::Bool(y))  => Some(x.cmp(y)),
            // Angka campur int/float dibandingkan sebagai angka, bukan
            // dianggap tidak sebanding — `[1, 2.5, 0]` adalah daftar angka
            // biasa yang wajar diurutkan.
            (Value::Int(_) | Value::Float(_), Value::Int(_) | Value::Float(_)) => {
                let fa = match a { Value::Int(v) => *v as f64, Value::Float(v) => *v, _ => unreachable!() };
                let fb = match b { Value::Int(v) => *v as f64, Value::Float(v) => *v, _ => unreachable!() };
                // NaN tidak punya urutan terhadap apa pun — bukan "sama".
                fa.partial_cmp(&fb)
            }
            // Array dibandingkan leksikografis, elemen per elemen.
            (Value::Array(x), Value::Array(y)) => {
                for (ex, ey) in x.iter().zip(y.iter()) {
                    match Self::cmp_values(ex, ey)? {
                        Ordering::Equal => continue,
                        lain => return Some(lain),
                    }
                }
                Some(x.len().cmp(&y.len()))
            }
            (Value::Tuple(x), Value::Tuple(y)) => {
                for (ex, ey) in x.iter().zip(y.iter()) {
                    match Self::cmp_values(ex, ey)? {
                        Ordering::Equal => continue,
                        lain => return Some(lain),
                    }
                }
                Some(x.len().cmp(&y.len()))
            }
            _ => None,
        }
    }

    /// Nilai terkecil/terbesar dari sebuah array.
    pub(crate) fn array_min_max(a: &[Value], cari_max: bool, konteks: &str) -> EvalResult {
        let Some((pertama, sisanya)) = a.split_first() else {
            // Array kosong DITOLAK, bukan dijawab `nil`. VM sudah menolaknya;
            // di sini ia menjawab `nil` — sebuah asimetri di mana yang salah
            // justru mesin acuannya. Lihat `pesan_koleksi_kosong`.
            return runtime_err!("{}", crate::interpreter::pesan_koleksi_kosong(
                konteks,
                if cari_max { "nilai terbesar" } else { "nilai terkecil" }));
        };
        let mut m = pertama.clone();
        for v in sisanya {
            let Some(o) = Self::cmp_values(v, &m) else {
                return runtime_err!("{}", crate::interpreter::pesan_min_max_tak_sebanding(
                    konteks, &v.type_name(), &m.type_name()));
            };
            let ganti = if cari_max {
                o == std::cmp::Ordering::Greater
            } else {
                o == std::cmp::Ordering::Less
            };
            if ganti { m = v.clone(); }
        }
        Ok(m)
    }

    /// Array angka (int atau float) → `Vec<f64>`, atau error kalau ada elemen
    /// yang bukan angka.
    pub(crate) fn numeric_vec(a: &[Value], konteks: &str) -> Result<Vec<f64>, Signal> {
        a.iter().map(|v| match v {
            Value::Int(n) => Ok(*n as f64),
            Value::Float(f) => Ok(*f),
            // Menyebut TIPE, bukan nilai: sebuah pesan tidak boleh tumbuh
            // mengikuti besar datanya (aturan Fase GALAT-INDEX).
            lain => runtime_err!("{}", crate::interpreter::pesan_elemen_bukan_angka(
                konteks, &lain.type_name())),
        }).collect()
    }

    /// Urutkan `Vec<f64>` untuk keperluan median/percentile, dan laporkan
    /// kalau ada NaN — urutan yang melibatkan NaN tidak terdefinisi, dan
    /// "median" dari data begitu tidak berarti apa-apa.
    pub(crate) fn sort_f64_checked(mut fs: Vec<f64>, konteks: &str) -> Result<Vec<f64>, Signal> {
        if fs.iter().any(|x| x.is_nan()) {
            return runtime_err!("{}", crate::interpreter::pesan_nan_tak_terurut(konteks));
        }
        fs.sort_by(|a, b| a.total_cmp(b));
        Ok(fs)
    }

    /// Tiap elemen harus angka. Dipisah dari `numeric_vec` karena `arr::sum`
    /// dan `arr::product` MEMVALIDASI tanpa mengonversi — lewat `f64`,
    /// `arr::product([1,2,3])` akan menjawab `6.0` alih-alih `6`.
    pub(crate) fn cek_semua_angka(a: &[Value], konteks: &str) -> Result<(), Signal> {
        for v in a {
            if !matches!(v, Value::Int(_) | Value::Float(_)) {
                return runtime_err!("{}", crate::interpreter::pesan_elemen_bukan_angka(
                    konteks, &v.type_name()));
            }
        }
        Ok(())
    }

    /// Empat statistik, satu definisi untuk bentuk namespace DAN bentuk method.
    /// Array kosong ditolak, tidak dijawab `NaN`: `NaN` sebuah ANGKA, jadi ia
    /// mengalir lewat aritmetika berikutnya tanpa ada yang menghentikannya.
    pub(crate) fn stat_mean(a: &[Value]) -> EvalResult {
        if a.is_empty() {
            return runtime_err!("{}", crate::interpreter::pesan_koleksi_kosong(
                "arr::mean", "rata-rata"));
        }
        let fs = Self::numeric_vec(a, "arr::mean")?;
        Ok(Value::Float(fs.iter().sum::<f64>() / fs.len() as f64))
    }

    pub(crate) fn stat_median(a: &[Value]) -> EvalResult {
        if a.is_empty() {
            return runtime_err!("{}", crate::interpreter::pesan_koleksi_kosong(
                "arr::median", "median"));
        }
        let fs = Self::sort_f64_checked(Self::numeric_vec(a, "arr::median")?, "arr::median")?;
        let tengah = fs.len() / 2;
        Ok(Value::Float(if fs.len() % 2 == 0 {
            (fs[tengah - 1] + fs[tengah]) / 2.0
        } else {
            fs[tengah]
        }))
    }

    pub(crate) fn stat_variance(a: &[Value]) -> EvalResult {
        if a.is_empty() {
            return runtime_err!("{}", crate::interpreter::pesan_koleksi_kosong(
                "arr::variance", "variansi"));
        }
        let fs = Self::numeric_vec(a, "arr::variance")?;
        let rata = fs.iter().sum::<f64>() / fs.len() as f64;
        Ok(Value::Float(fs.iter().map(|x| (x - rata).powi(2)).sum::<f64>() / fs.len() as f64))
    }

    pub(crate) fn stat_std_dev(a: &[Value]) -> EvalResult {
        if a.is_empty() {
            return runtime_err!("{}", crate::interpreter::pesan_koleksi_kosong(
                "arr::std_dev", "simpangan baku"));
        }
        let fs = Self::numeric_vec(a, "arr::std_dev")?;
        let rata = fs.iter().sum::<f64>() / fs.len() as f64;
        Ok(Value::Float(
            (fs.iter().map(|x| (x - rata).powi(2)).sum::<f64>() / fs.len() as f64).sqrt(),
        ))
    }

    /// Berapa parameter yang diterima sebuah nilai yang bisa dipanggil, atau
    /// `None` kalau tidak diketahui (mis. fungsi bawaan).
    pub(crate) fn arity_of(&self, f: &Value) -> Option<usize> {
        match f {
            Value::Function(fun) => Some(fun.params.len()),
            Value::Closure { params, .. } => Some(params.len()),
            _ => None,
        }
    }

    /// Urutkan dengan fungsi pembanding buatan pemakai (`|a, b| ...`).
    pub(crate) fn sort_with_comparator_public(&mut self, v: Vec<Value>, cmp: Value) -> EvalResult {
        self.sort_with_comparator(v, cmp)
    }

    pub(crate) fn sort_with_comparator(&mut self, v: Vec<Value>, cmp: Value) -> EvalResult {
        // Sisip biner: stabil, dan urutan panggilan pembandingnya satu definisi
        // bersama `--vm` — efek samping di dalam pembanding terlihat pemakainya.
        let out = crate::interpreter::urut_sisip_biner(v, |item, pembanding| {
            match self.call_fn(cmp.clone(), vec![item.clone(), pembanding.clone()])? {
                Value::Int(n)   => Ok(n),
                Value::Float(f) => Ok(if f < 0.0 { -1 } else if f > 0.0 { 1 } else { 0 }),
                lain => Err(Signal::RuntimeError(RuntimeError::new(
                    crate::interpreter::pesan_pembanding_bukan_angka(&lain.type_name())))),
            }
        })?;
        Ok(Value::Array(out.into()))
    }

    /// Urutkan berdasarkan kunci yang dihitung dari tiap elemen.
    pub(crate) fn sort_by_key_public(&mut self, v: Vec<Value>, key_fn: Value) -> EvalResult {
        self.sort_by_key_checked(v, key_fn)
    }

    pub(crate) fn sort_by_key_checked(&mut self, v: Vec<Value>, key_fn: Value) -> EvalResult {
        let mut keyed: Vec<(Value, Value)> = Vec::with_capacity(v.len());
        for item in v {
            let k = self.call_fn(key_fn.clone(), vec![item.clone()])?;
            keyed.push((k, item));
        }
        let keyed = crate::interpreter::urut_stabil(keyed, |(ka, _), (kb, _)| {
            match Self::cmp_values(ka, kb) {
                Some(o) => Ok(o),
                None => Err(Signal::RuntimeError(RuntimeError::new(
                    crate::interpreter::pesan_kunci_tak_sebanding(
                        "arr::sort_by_key", &ka.type_name(), &kb.type_name())))),
            }
        })?;
        Ok(Value::Array(keyed.into_iter().map(|(_, v)| v).collect()))
    }

    /// Urutkan, dan laporkan kalau ada dua nilai yang tidak bisa dibandingkan
    /// alih-alih diam-diam menganggapnya sama.
    pub(crate) fn sort_checked(
        v: Vec<Value>,
        desc: bool,
        konteks: &str,
    ) -> Result<Vec<Value>, Signal> {
        // `desc` membalik PEMBANDINGNYA, bukan hasilnya: membalik hasilnya ikut
        // membalik elemen yang dianggap sama, jadi tidak stabil.
        crate::interpreter::urut_stabil(v, |x, y| match Self::cmp_values(x, y) {
            Some(o) => Ok(if desc { o.reverse() } else { o }),
            None => Err(Signal::RuntimeError(RuntimeError::new(
                crate::interpreter::pesan_urut_tak_sebanding(
                    konteks, &x.type_name(), &y.type_name())))),
        })
    }

    pub(crate) fn v_cmp(&self, a: Value, b: Value, ord: std::cmp::Ordering, or_eq: bool) -> EvalResult {
        Self::banding_lepas(a, b, ord, or_eq)
    }

    /// Badan `v_cmp`, bebas `self` — `math::min`/`max`/`clamp` memakainya dari
    /// DUA mesin, jadi `math::max(1, 2.5)` tidak bisa lagi diterima satu mesin
    /// dan ditolak mesin lain.
    pub(crate) fn banding_lepas(a: Value, b: Value, ord: std::cmp::Ordering, or_eq: bool) -> EvalResult {
        // Float dibandingkan lewat `partial_cmp` (`Option<Ordering>`), BUKAN `.cmp`,
        // karena float punya nilai yang tidak berurutan sama sekali terhadap apa …
        let r = match (&a, &b) {
            (Value::Int(x),   Value::Int(y))   => Some(x.cmp(y)),
            (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
            (Value::Int(x),   Value::Float(y)) => (*x as f64).partial_cmp(y),
            (Value::Float(x), Value::Int(y))   => x.partial_cmp(&(*y as f64)),
            (Value::Str(x),   Value::Str(y))   => Some(x.cmp(y)),
            (Value::Rune(x),  Value::Rune(y))  => Some(x.cmp(y)),
            // Array dan tuple dibandingkan LEKSIKOGRAFIS — lewat `cmp_values`, definisi
            // yang sudah dipakai `arr::sort`, `arr::min`/`max`, `sort_by_key` dan …
            (Value::Array(_), Value::Array(_)) | (Value::Tuple(_), Value::Tuple(_)) => {
                let (xs, ys): (&[Value], &[Value]) = match (&a, &b) {
                    (Value::Array(x), Value::Array(y)) => (x, y),
                    (Value::Tuple(x), Value::Tuple(y)) => (x, y),
                    _ => unreachable!(),
                };
                let mut hasil = None;
                for (ex, ey) in xs.iter().zip(ys.iter()) {
                    // Kalau ELEMEN-nya yang tidak sebanding, yang disebut adalah tipe ELEMEN itu
                    // — bukan `array dengan array`.
                    let Some(o) = Self::cmp_values(ex, ey) else {
                        return runtime_err!("{}", crate::interpreter::pesan_banding(
                            &ex.type_name(), &ey.type_name()));
                    };
                    if o != std::cmp::Ordering::Equal { hasil = Some(o); break; }
                }
                Some(hasil.unwrap_or_else(|| xs.len().cmp(&ys.len())))
            }
            _ => return runtime_err!("{}", crate::interpreter::pesan_banding(
                &a.type_name(), &b.type_name())),
        };
        Ok(Value::Bool(match r {
            Some(o) => o == ord || (or_eq && o == std::cmp::Ordering::Equal),
            // NaN di salah satu sisi: tidak berurutan terhadap apa pun,
            // jadi `<`/`<=`/`>`/`>=` semuanya `false` — tidak ada nilai
            // "masuk akal" lain yang bisa dijawab di sini.
            None => false,
        }))
    }
    /// Pintu kesamaan sisi interpreter. Mengembalikan `Signal`, bukan `String`,
    /// supaya ke-13 situs pemanggilnya cukup menulis `?` — sebuah situs yang
    /// harus menerjemahkan galatnya sendiri adalah situs yang bisa menelannya.
    pub fn val_eq(&self, a: &Value, b: &Value) -> Result<bool, crate::interpreter::Signal> {
        Self::nilai_sama(a, b).map_err(|e| {
            crate::interpreter::Signal::RuntimeError(crate::interpreter::RuntimeError::new(e))
        })
    }

    /// `position` versi boleh-gagal, sisi interpreter.
    pub fn posisi_eq<'a>(
        &self, it: impl Iterator<Item = &'a Value>, t: &Value,
    ) -> Result<Option<usize>, crate::interpreter::Signal> {
        for (i, v) in it.enumerate() {
            if self.val_eq(v, t)? { return Ok(Some(i)); }
        }
        Ok(None)
    }

    /// Kesamaan dua nilai, bebas `self`, sudah dalam bentuk `Signal`.
    pub fn eq_lepas(a: &Value, b: &Value) -> Result<bool, crate::interpreter::Signal> {
        Self::nilai_sama(a, b).map_err(|e| {
            crate::interpreter::Signal::RuntimeError(crate::interpreter::RuntimeError::new(e))
        })
    }

    /// Kesamaan berpasangan atas dua urutan, sisi interpreter.
    pub fn semua_eq<'a>(
        &self, it: impl Iterator<Item = (&'a Value, &'a Value)>,
    ) -> Result<bool, crate::interpreter::Signal> {
        for (x, y) in it {
            if !self.val_eq(x, y)? { return Ok(false); }
        }
        Ok(true)
    }

    /// `any` versi boleh-gagal, sisi interpreter.
    pub fn ada_eq<'a>(
        &self, it: impl Iterator<Item = &'a Value>, t: &Value,
    ) -> Result<bool, crate::interpreter::Signal> {
        Ok(self.posisi_eq(it, t)?.is_some())
    }

    /// Kesamaan dua nilai — bebas `self`, supaya VM bisa memanggil yang persis
    /// sama lewat jembatan nilai.
    pub fn nilai_sama(a: &Value, b: &Value) -> Result<bool, String> {
        if a.bisa_dipanggil() && b.bisa_dipanggil() {
            return Err(crate::interpreter::pesan_banding(&a.type_name(), &b.type_name()));
        }
        Ok(match (a, b) {
            (Value::Int(x),   Value::Int(y))   => x == y,
            (Value::Float(x), Value::Float(y)) => x == y,
            (Value::Int(x),   Value::Float(y)) => *x as f64 == *y,
            (Value::Float(x), Value::Int(y))   => *x == *y as f64,
            (Value::Bool(x),  Value::Bool(y))  => x == y,
            (Value::Rune(x),  Value::Rune(y))  => x == y,
            (Value::Str(x),   Value::Str(y))   => x == y,
            (Value::Nil,      Value::Nil)       => true,
            (Value::Unit,     Value::Unit)      => true,
            (Value::Array(x), Value::Array(y)) =>
                x.len() == y.len() && Self::semua_sama(x, y)?,
            (Value::Tuple(x), Value::Tuple(y)) =>
                x.len() == y.len() && Self::semua_sama(x, y)?,
            // Struct and enum equality were both previously missing entirely (silently
            // falling through to the catch-all `false`), so `==`/`!=` and array …
            (Value::Struct { name: n1, fields: f1 }, Value::Struct { name: n2, fields: f2 }) =>
                n1 == n2 && f1.len() == f2.len() && {
                    let mut sama = true;
                    for (k, v) in f1.iter() {
                        match f2.get(k) {
                            Some(v2) if Self::nilai_sama(v, v2)? => {}
                            _ => { sama = false; break; }
                        }
                    }
                    sama
                },
            (Value::Enum { type_name: t1, variant: v1, payload: p1 },
             Value::Enum { type_name: t2, variant: v2, payload: p2 }) =>
                t1 == t2 && v1 == v2 && match (p1, p2) {
                    (Some(x), Some(y)) => Self::nilai_sama(x, y)?,
                    (None, None)       => true,
                    _                  => false,
                },
            (Value::Ptr(x), Value::Ptr(y)) => x == y,

            // ── Handle sumber daya ber-`Arc` ─────────────────────────────
            (Value::Channel(a), Value::Channel(b)) => a.sama(b),
            (Value::Sinkron(a), Value::Sinkron(b)) => a.sama(b),
            (Value::Canvas(a),  Value::Canvas(b))  => std::sync::Arc::ptr_eq(a, b),
            (Value::PyObj(a),   Value::PyObj(b))   => a.id == b.id && a.generasi == b.generasi,
            (Value::JsObj(a),   Value::JsObj(b))   => a.id == b.id && a.generasi == b.generasi,
            (Value::PhpObj(a),  Value::PhpObj(b))  => a.id == b.id && a.generasi == b.generasi,
            // Dua array numerik sama kalau ISINYA sama — sama seperti `Value::Array`,
            // dan bukan sekadar kalau kebetulan berbagi penyimpanan.
            (Value::Nd(a),      Value::Nd(b))      => a == b,

            _ => false,
        })
    }

    /// Kesamaan elemen per elemen, meneruskan kegagalan pembandingnya.
    pub fn semua_sama(x: &[Value], y: &[Value]) -> Result<bool, String> {
        for (a, b) in x.iter().zip(y.iter()) {
            if !Self::nilai_sama(a, b)? { return Ok(false); }
        }
        Ok(true)
    }

    /// `position` versi boleh-gagal — dipakai `contains`, `index_of`, `dedup`,
    /// `union` dan saudaranya, supaya ke-17 situsnya tidak masing-masing
    /// menulis loopnya dan tidak satu pun bisa menelan kegagalannya.
    pub fn posisi_sama<'a>(
        it: impl Iterator<Item = &'a Value>, t: &Value,
    ) -> Result<Option<usize>, String> {
        for (i, v) in it.enumerate() {
            if Self::nilai_sama(v, t)? { return Ok(Some(i)); }
        }
        Ok(None)
    }

    /// `any` versi boleh-gagal.
    pub fn ada_yang_sama<'a>(
        it: impl Iterator<Item = &'a Value>, t: &Value,
    ) -> Result<bool, String> {
        Ok(Self::posisi_sama(it, t)?.is_some())
    }

    /// Sebab sebuah namespace tidak tersedia di interpreter, kalau memang tercatat.
    pub(crate) fn jelaskan_namespace_terblokir(path: &str) -> Option<String> {
        if path.starts_with("future::") || path.starts_with("task::") {
            // Sebagian nama `task::` ditolak di KEDUA backend; alasannya
            // dibaca dari satu tempat bersama.
            if let Some(alasan) = crate::stdlib::future::alasan_ditolak_dua_backend(path) {
                return Some(alasan.to_string());
            }
            // Kalimat generik "belum ada di interpreter ...
            return None;
        }
        None
    }

}
