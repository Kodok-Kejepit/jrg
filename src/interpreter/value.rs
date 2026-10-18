// src/interpreter/value.rs — interpreter/value.rs
// Tipe nilai inti: `Value`, hasil eksekusi (`Signal`/`RuntimeError`/

use std::collections::HashMap;
use std::fmt;

use crate::parser::{
    AssignOp, BinaryOp, Block, BreakStmt, ContinueStmt, EnumDecl, EnumVariantKind,
    Expr, ExternBlock, ExternItem, ExternParam, FnBody,
    ForStmt, IfStmt, ImplItem, Item, LetStmt, LitKind, LoopStmt,
    MatchArm, MatchBody, MatchStmt, NamedType, OwnershipQual, ParsedFile, Pattern, PrimTy,
    ReturnStmt, SelectCase, SelectStmt, ShortDeclStmt, SpawnStmt, Stmt, TypeExpr,
    UnaryOp, UsePath, VarStmt, WhileStmt,
    StructDecl, StructField, TraitDecl, TraitItem, Visibility,
};

/// Sisi kiri `=` yang bukan TEMPAT. Satu kalimat untuk parser (yang menolaknya sebelum
/// jalan), interpreter, dan VM; bentuk yang sering ditulis dari bahasa lain mendapat jalannya.
pub fn pesan_bukan_sasaran(e: &Expr) -> String {
    match e {
        Expr::PathExpr { segments, .. } => format!(
            "`{}` bukan sesuatu yang bisa di-assign — nama ber-`::` (milik modul, tipe, atau \
             pustaka bawaan) cuma bisa dibaca; kalau nilainya perlu berubah, ubah lewat fungsi \
             milik pemiliknya", segments.join("::")),
        // Di backend LLVM `a[0:2] = …` dulu DITERIMA dan menulis ke salinan, tanpa kata.
        Expr::Slice { .. } => "potongan (`x[a:b]`) bukan sesuatu yang bisa di-assign — potongan \
             adalah SALINAN, jadi menulis ke sana tidak mengubah asalnya; tulis elemennya satu \
             per satu (`x[i] = …`)".to_string(),
        Expr::Tuple { .. } => "tuple bukan sesuatu yang bisa di-assign — untuk menugasi beberapa \
             nama sekaligus, tulis tanpa kurung: `a, b = b, a`".to_string(),
        Expr::Array { .. } => "literal array bukan sesuatu yang bisa di-assign — untuk membongkar \
             array ke nama baru, tulis `let [a, b] = …`".to_string(),
        lain => format!("{} bukan sesuatu yang bisa di-assign — sisi kiri `=` harus variabel, \
             elemen array/dict (`x[...]`), atau field (`x.nama`)",
            crate::interpreter::Interpreter::gambar_expr(lain)),
    }
}

// `NativeFnKind` sekarang tinggal di `builtin/mod.rs` — dekat dengan
// `register_builtins`/`call_native` yang paling banyak memakainya.
use super::builtin::NativeFnKind;
use super::env::Env;

/// Penyimpanan agregat yang DIBAGI sampai ada yang menulis (*copy-on-write*).
pub struct Bersama<T: Clone>(std::sync::Arc<T>);

impl<T: Clone> Bersama<T> {
    pub fn baru(isi: T) -> Self { Bersama(std::sync::Arc::new(isi)) }

    /// Isi ini sebagai milik sendiri: GRATIS kalau kita pemegang terakhir,
    /// menyalin kalau masih ada pemegang lain. Dipakai situs yang memang
    /// butuh `T` utuh (mis. mengonsumsi array jadi iterator).
    pub fn ambil(self) -> T {
        std::sync::Arc::try_unwrap(self.0).unwrap_or_else(|arc| (*arc).clone())
    }

    /// Apakah penyimpanan ini cuma dipegang kita? Dipakai optimasi keunikan (mis.
    /// `a = a + [x]` yang bisa jadi push di tempat), BUKAN untuk memutuskan …
    pub fn sendirian(&self) -> bool { std::sync::Arc::strong_count(&self.0) == 1 }

    /// Apakah dua pemegang ini menunjuk penyimpanan yang SAMA — bukti bahwa tidak ada yang
    /// menulis di antara keduanya. Bukan pembanding isi: `==` selalu membandingkan isi.
    pub fn sama_penyimpanan(&self, lain: &Self) -> bool { std::sync::Arc::ptr_eq(&self.0, &lain.0) }
}

/// O(1) — inilah inti perbaikannya. Menyerahkan nilai ke tempat lain
/// sekarang cuma menambah pemegang, bukan menyalin isi.
impl<T: Clone> Clone for Bersama<T> {
    fn clone(&self) -> Self { Bersama(std::sync::Arc::clone(&self.0)) }
}

impl<T: Clone> std::ops::Deref for Bersama<T> {
    type Target = T;
    fn deref(&self) -> &T { &self.0 }
}

/// SATU-SATUNYA tempat aturan copy-on-write ditulis: meminjam agregat untuk
/// diubah memisahkan penyimpanannya dulu kalau masih dipegang orang lain.
impl<T: Clone> std::ops::DerefMut for Bersama<T> {
    fn deref_mut(&mut self) -> &mut T { std::sync::Arc::make_mut(&mut self.0) }
}

impl<T: Clone> From<T> for Bersama<T> {
    fn from(isi: T) -> Self { Bersama::baru(isi) }
}

/// Tampil persis seperti isinya — supaya keluaran `{:?}` tidak berubah
/// gara-gara pembungkus ini (beberapa pesan diagnostik memakainya).
impl<T: Clone + fmt::Debug> fmt::Debug for Bersama<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { (*self.0).fmt(f) }
}

/// Membuat `.collect()` tetap bekerja di situs yang membangun array baru
/// dari iterator — pola yang dipakai puluhan builtin `arr::`.
impl<T: Clone> FromIterator<T> for Bersama<Vec<T>> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Bersama::baru(iter.into_iter().collect())
    }
}

impl<K: Clone + std::hash::Hash + Eq, V: Clone> FromIterator<(K, V)> for Bersama<HashMap<K, V>> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        Bersama::baru(iter.into_iter().collect())
    }
}

/// Mengonsumsi agregat jadi iterator nilai — lewat `ambil`, jadi gratis
/// kalau kita pemegang terakhir.
impl<T: Clone> IntoIterator for Bersama<Vec<T>> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(self) -> Self::IntoIter { self.ambil().into_iter() }
}

impl<'a, T: Clone> IntoIterator for &'a Bersama<Vec<T>> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter { self.0.iter() }
}

impl<K: Clone, V: Clone> IntoIterator for Bersama<HashMap<K, V>> {
    type Item = (K, V);
    type IntoIter = std::collections::hash_map::IntoIter<K, V>;
    fn into_iter(self) -> Self::IntoIter { self.ambil().into_iter() }
}

impl<'a, K: Clone, V: Clone> IntoIterator for &'a Bersama<HashMap<K, V>> {
    type Item = (&'a K, &'a V);
    type IntoIter = std::collections::hash_map::Iter<'a, K, V>;
    fn into_iter(self) -> Self::IntoIter { self.0.iter() }
}

/// Array/tuple: `Vec<Value>` di belakang penyimpanan bersama ber-CoW.
pub type Isi = Bersama<Vec<Value>>;
/// Field struct (dan isi dict): `HashMap<String, Value>` yang sama sifatnya.
pub type Field = Bersama<HashMap<String, Value>>;

#[derive(Debug, Clone)]
pub enum Value {
    Int(i128),
    Float(f64),
    Bool(bool),
    Rune(char),
    Str(String),
    Unit,
    Nil,
    Array(Isi),
    /// Array numerik `wokwok::`. Lihat catatan lengkapnya di
    /// `VmValue::Nd` — payload dan aturannya identik di dua backend,
    /// sengaja, supaya tidak ada yang bisa melenceng.
    Nd(std::sync::Arc<crate::wokwok::Nd>),
    Tuple(Isi),
    Struct  { name: String, fields: Field },
    Enum    { type_name: String, variant: String, payload: Option<Box<Value>> },
    /// Di balik `Arc`: `JrgFunction` ±120 byte, dan menyimpannya langsung menggemukkan SETIAP
    /// `Value` (yang dipindah di tiap hasil ekspresi) serta membuat salinannya mengalokasi nama.
    Function(std::sync::Arc<JrgFunction>),
    /// Closure.
    Closure {
        params: std::sync::Arc<Vec<String>>,
        body: std::sync::Arc<Expr>,
        captures: Vec<(String, std::sync::Arc<std::sync::Mutex<Value>>)>,
        globals: crate::interpreter::Globals,
        /// `ruang` milik kode yang menciptakan closure ini (lihat `JrgFunction::ruang`).
        ruang: Option<std::sync::Arc<String>>,
    },
    NativeFn(NativeFnKind),
    /// Native canvas window — Arc<Mutex<CanvasInner>>, ref-counted & thread-safe.
    Canvas(crate::canvas::CanvasHandle),
    /// An opaque foreign pointer/handle — the address itself, with no notion of
    /// pointee type.
    Ptr(usize),
    /// A callable constructor for a tuple-style enum variant — what `Some`, `Ok`,
    /// `Err`, and any user-defined tuple variant (`enum Shape { Circle(f64) }`'s …
    EnumCtor { type_name: String, variant: String, arity: usize },
    /// A function resolved (or attempted to resolve) from an `extern "C" { ...
    ExternFn(crate::ffi::native::ExternFnInfo),
    /// A real, thread-safe MPMC channel (see `ChannelHandle` below).
    Channel(ChannelHandle),
    /// `Atomic`/`Mutex`: pegangan ke keadaan bersama (`crate::sinkron`).
    Sinkron(crate::sinkron::Pegangan),
    /// Objek yang tinggal di worker Python, diwakili sebuah handle.
    PyObj(crate::ffi::python::PyRef),
    /// Handle ke sebuah objek yang tinggal di worker JavaScript.
    JsObj(crate::ffi::javascript::JsRef),
    /// Handle ke sebuah nilai yang tinggal di worker PHP.
    PhpObj(crate::ffi::php::PhpRef),
}

/// Channel acuan: `Antrean` dengan muatan `Value` — **definisi yang sama**
/// dengan yang dipakai VM.
pub type ChannelInner = crate::antrean::Antrean<Value>;
pub type ChannelHandle = ChannelInner;

/// Channel baru dengan kapasitas buffer (`0` = tanpa buffer/sinkron, seperti
/// `make_chan(T)` di Go).
pub fn channel_new(capacity: usize) -> ChannelHandle {
    crate::antrean::Antrean::new(capacity)
}

/// Kirim yang memblokir. Gagal kalau channelnya sudah DITUTUP — nilainya
/// tidak akan pernah sampai, jadi melaporkannya lebih jujur daripada diam.
pub fn channel_send(chan: &ChannelHandle, val: Value) -> Result<(), String> {
    chan.kirim(val)
}

/// Kirim tanpa memblokir, dipakai `select`. `Err(true)` berarti "akan
/// memblokir sekarang, coba case lain"; `Err(false)` berarti sudah ditutup.
pub(crate) fn channel_try_send(chan: &ChannelHandle, val: Value) -> Result<(), bool> {
    match chan.coba_kirim(val) {
        Ok(true) => Ok(()),
        Ok(false) => Err(true),
        Err(_) => Err(false),
    }
}

/// Terima yang memblokir. Gagal hanya kalau channelnya ditutup DAN kosong:
/// isi yang sudah terkirim tetap dikuras lebih dulu.
pub fn channel_recv(chan: &ChannelHandle) -> Result<Value, String> {
    chan.terima().ok_or_else(|| crate::antrean::PESAN_TERIMA_TERTUTUP.to_string())
}

/// Ambil kalau sudah ada, tanpa memblokir.
pub fn channel_coba_ambil(chan: &ChannelHandle) -> Option<Value> {
    chan.coba_terima().ok()
}

/// Lihat apakah ada nilai siap, TANPA mengambilnya. Versi lama `try_recv` lalu
/// `try_send` balik — penerima lain bisa menyelak di antaranya, dan
/// pengembaliannya bisa gagal kalau buffer penuh.
pub fn channel_ada_isi(chan: &ChannelHandle) -> bool {
    chan.ada_isi()
}

/// Terima tanpa memblokir, dipakai `select`. `Err(true)` belum ada isi,
/// `Err(false)` ditutup dan kosong.
pub(crate) fn channel_try_recv(chan: &ChannelHandle) -> Result<Value, bool> {
    chan.coba_terima()
}

impl Value {
    /// Apakah nilai ini bisa DIPANGGIL — satu jawaban untuk tiap pintu yang menerima
    /// fungsi; dulu tiap pintu punya daftarnya sendiri dan konstruktor terlewat.
    pub fn bisa_dipanggil(&self) -> bool {
        matches!(self, Value::Function(_) | Value::Closure { .. } | Value::NativeFn(_)
                     | Value::EnumCtor { .. } | Value::ExternFn(_))
    }

    /// Nama tipe nilai ini, dalam bentuk yang dipakai `typeof()` maupun pesan
    /// error.
    pub fn type_name(&self) -> String {
        match self {
            Value::Int(_)                 => "int".to_string(),
            Value::Float(_)               => "float".to_string(),
            Value::Bool(_)                => "bool".to_string(),
            Value::Rune(_)                => "rune".to_string(),
            Value::Str(_)                 => "string".to_string(),
            Value::Unit                   => "unit".to_string(),
            Value::Nil                    => "nil".to_string(),
            Value::Array(_)               => "array".to_string(),
            // Tipenya menyebut dtype-nya juga: untuk kerja numerik,
            // "array f64" dan "array i64" itu dua hal yang berbeda, dan
            // bedanya justru sering jadi sumber hasil yang mengejutkan.
            Value::Nd(a)                  => format!("wokwok:{}", a.dtype()),
            Value::Tuple(_)               => "tuple".to_string(),
            // Dict diwakili `Value::Struct` bernama "dict" di dalam, tapi dari sisi
            // bahasa ia koleksi tersendiri — bukan struct buatan pemakai.
            Value::Struct { name, .. } if name == "dict" => "dict".to_string(),
            Value::Struct { name, .. }    => format!("struct:{}", name),
            Value::Enum { type_name, .. } => format!("enum:{}", type_name),
            Value::Function(_)            => "function".to_string(),
            Value::Closure { .. }         => "closure".to_string(),
            Value::NativeFn(_)            => "native_fn".to_string(),
            Value::Canvas(_)              => "canvas".to_string(),
            Value::Ptr(_)                 => "ptr".to_string(),
            Value::EnumCtor { type_name, variant, .. } => format!("enum_ctor:{}::{}", type_name, variant),
            Value::ExternFn(_)            => "extern_fn".to_string(),
            Value::Channel(_)             => "channel".to_string(),
            Value::Sinkron(p)             => p.nama_tipe().to_string(),
            // Tipenya menyebut tipe PYTHON-nya, bukan cuma "pyobj":
            // `typeof(df)` yang menjawab `py:pandas.DataFrame` jauh lebih
            // berguna daripada yang menjawab nama pembungkusnya.
            Value::PyObj(h)               => format!("py:{}", h.tipe),
            Value::JsObj(h)               => format!("js:{}", h.tipe),
            Value::PhpObj(h)              => format!("php:{}", h.tipe),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n)    => write!(f, "{}", n),
            Value::Float(n)  => {
                if n.fract() == 0.0 && n.is_finite() { write!(f, "{:.1}", n) }
                else { write!(f, "{}", n) }
            }
            Value::Bool(b)   => write!(f, "{}", b),
            Value::Nd(a)     => write!(f, "{}", a),
            Value::Rune(c)   => write!(f, "{}", c),
            Value::Str(s)    => write!(f, "{}", s),
            Value::Unit      => write!(f, "()"),
            Value::Nil       => write!(f, "nil"),
            Value::Array(a)  => {
                write!(f, "[")?;
                for (i, v) in a.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            Value::Tuple(t)  => {
                write!(f, "(")?;
                for (i, v) in t.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", v)?;
                }
                write!(f, ")")
            }
            Value::Struct { name, fields } => {
                write!(f, "{} {{", crate::ruang_tipe::bersih(name))?;
                let mut kv: Vec<_> = fields.iter().collect();
                kv.sort_by_key(|(k, _)| k.as_str());
                for (i, (k, v)) in kv.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
            Value::Enum { type_name, variant, payload } => {
                write!(f, "{}::{}", crate::ruang_tipe::bersih(type_name), variant)?;
                if let Some(p) = payload { write!(f, "({})", p)?; }
                Ok(())
            }
            Value::Function(func) => write!(f, "<fn {}>", func.name),
            Value::Closure { .. } => write!(f, "<closure>"),
            // `<native fn>`, BUKAN nama variannya.
            Value::NativeFn(_)    => write!(f, "<native fn>"),
            Value::Canvas(handle) => {
                match handle.try_lock() {
                    Ok(inner) => write!(f, "<Canvas {}x{} '{}'>",
                                        inner.width, inner.height, inner.title),
                    Err(_)    => write!(f, "<Canvas [busy]>"),
                }
            }
            Value::Ptr(addr) => write!(f, "<ptr 0x{:x}>", addr),
            Value::EnumCtor { type_name, variant, .. } => write!(f, "<ctor {}::{}>", crate::ruang_tipe::bersih(type_name), variant),
            Value::ExternFn(info) => write!(f, "<extern fn {}>", info.name),
            Value::Channel(c) => write!(f, "<chan cap={}>", c.kapasitas()),
            Value::Sinkron(p) => write!(f, "{}", p.tampil()),
            // Sengaja TIDAK memanggil `repr()` ke worker: `Display` dipakai di jalur
            // error dan pencetakan, dan jalur itu tidak boleh diam-diam melakukan I/O ke …
            Value::PyObj(h) => write!(f, "<py {} #{}>", h.tipe, h.id),
            Value::JsObj(h) => write!(f, "<js {} #{}>", h.tipe, h.id),
            Value::PhpObj(h) => write!(f, "<php {} #{}>", h.tipe, h.id),
        }
    }
}

// ---------------------------------------------------------------------------
// Result<T, E> / Option<T> construction helpers

/// `str * n` — aturan pengulangan string, satu definisi untuk semua backend.
pub const MAKS_ELEMEN: i128 = 16 * 1024 * 1024;

/// Periksa sebuah permintaan alokasi SEBELUM alokasinya terjadi.
pub fn cek_batas_elemen(nama: &str, n: i128) -> Result<(), String> {
    if n > MAKS_ELEMEN {
        return Err(format!(
            "{}: minta {} elemen sekaligus, batasnya {} — permintaan sebesar itu \
             mematikan prosesnya sebelum sempat menjawab. Cek lagi angkanya",
            nama, n, MAKS_ELEMEN));
    }
    Ok(())
}

/// Argumen yang semestinya bilangan bulat, sudah dipilah tiap mesin dari
/// nilainya sendiri — supaya kalimat penolakannya satu definisi.
pub enum ArgAngka { Bulat(i128), Pecahan(f64), Lain(String) }

fn bulat_dari(nama: &str, ke: usize, a: &ArgAngka) -> Result<i128, String> {
    match a {
        ArgAngka::Bulat(n) => Ok(*n),
        ArgAngka::Pecahan(f) => Err(format!(
            "{}: argumen ke-{} butuh bilangan bulat, dapat pecahan `{}` — kalau memang \
             mau dipotong, tulis eksplisit: `{} as int`",
            nama, ke, Value::Float(*f), Value::Float(*f))),
        ArgAngka::Lain(t) => Err(format!(
            "{}: argumen ke-{} butuh bilangan bulat, dapat `{}`", nama, ke, t)),
    }
}

/// Field yang tidak dideklarasikan sebuah varian ber-field, satu kalimat untuk dua mesin.
pub fn pesan_field_varian(field: &str, varian: &str) -> String {
    format!("field `{}` tidak ada pada variant {}", field, varian)
}

/// Jumlah elemen `[x; n]`, satu aturan untuk dua mesin. Dulu `[0; 99999999999]`
/// mematikan keduanya lewat OOM, dan `--vm` menjawab `[]` untuk `n` negatif/bukan int.
pub fn jumlah_ulang(n: &ArgAngka) -> Result<usize, String> {
    let k = match n {
        ArgAngka::Bulat(k) => *k,
        ArgAngka::Pecahan(f) => return Err(format!(
            "`[x; n]`: n harus bilangan bulat, dapat pecahan `{}` — kalau memang mau \
             dipotong, tulis eksplisit: `{} as int`", Value::Float(*f), Value::Float(*f))),
        ArgAngka::Lain(t) => return Err(format!("`[x; n]`: n harus bilangan bulat, dapat `{}`", t)),
    };
    if k < 0 {
        return Err(format!("`[x; n]`: n tidak boleh negatif, dapat {}", k));
    }
    cek_batas_elemen("`[x; n]`", k)?;
    Ok(k as usize)
}

/// Bentuk sah `range` — satu teks untuk kalimat hitungnya DAN petunjuk pintu native.
pub const BENTUK_RANGE: &str = "(akhir), (awal, akhir) atau (awal, akhir, langkah)";

/// Bentuk argumen `range`: `(akhir)`, `(awal, akhir)`, atau `(awal, akhir, langkah)`.
pub fn range_argumen(args: &[ArgAngka]) -> Result<(i128, i128, i128), String> {
    // Jumlahnya lebih dulu: argumen ke-4 yang kebetulan salah tipe tidak boleh
    // menyembunyikan bahwa argumennya memang kebanyakan.
    if !(1..=3).contains(&args.len()) {
        return Err(format!("`range` butuh 1 sampai 3 argumen — {} — dapat {}",
                           BENTUK_RANGE, args.len()));
    }
    let v: Vec<i128> = args.iter().enumerate()
        .map(|(i, a)| bulat_dari("range", i + 1, a)).collect::<Result<_, _>>()?;
    Ok(match v.as_slice() {
        [akhir] => (0, *akhir, 1),
        [awal, akhir] => (*awal, *akhir, 1),
        _ => (v[0], v[1], v[2]),
    })
}

/// Argumen dua-batas: `range_inclusive(awal, akhir)` dan literal `a..b`/`a..=b`.
pub fn range_batas(nama: &str, awal: &ArgAngka, akhir: &ArgAngka) -> Result<(i128, i128), String> {
    Ok((bulat_dari(nama, 1, awal)?, bulat_dari(nama, 2, akhir)?))
}

/// Berapa elemen sebuah range — SATU definisi untuk `range(...)`,
/// `range_inclusive(...)` dan literal `a..b`/`a..=b`, di dua mesin. Jumlahnya
/// dibatasi SEBELUM satu elemen pun dibuat, jadi tidak ada alokasi yang meledak.
pub fn range_jumlah(
    nama: &str, awal: i128, akhir: i128, langkah: i128, inklusif: bool,
) -> Result<usize, String> {
    let jumlah = range_hitung(nama, awal, akhir, langkah, inklusif)?;
    cek_batas_elemen(nama, i128::try_from(jumlah).unwrap_or(i128::MAX))?;
    Ok(jumlah as usize)
}

/// Jumlah elemen sebuah range TANPA batas alokasi — dipakai `for` atas
/// rentang yang ditulis di kepalanya, yang tidak pernah membuat array.
pub fn range_hitung(
    nama: &str, awal: i128, akhir: i128, langkah: i128, inklusif: bool,
) -> Result<u128, String> {
    if langkah == 0 {
        return Err(format!("{}: langkah tidak boleh 0 — urutannya tidak akan \
                            pernah sampai ke akhir", nama));
    }
    let maju = langkah > 0;
    let ada = if maju { awal < akhir } else { awal > akhir } || (inklusif && awal == akhir);
    let jumlah: u128 = if !ada { 0 } else {
        // Selisih dua i128 yang terurut selalu muat di u128, jadi dihitung
        // persis; cuma `MIN..=MAX` (2^128 elemen) yang dijenuhkan.
        let d = (if maju { akhir.wrapping_sub(awal) } else { awal.wrapping_sub(akhir) }) as u128;
        let st = langkah.unsigned_abs();
        if inklusif { (d / st).saturating_add(1) } else { d.div_ceil(st) }
    };
    Ok(jumlah)
}

/// Elemen ke-`k` = `awal + k * langkah`, untuk `k < jumlah` — dihitung dari
/// indeksnya, bukan dengan menambah terus, jadi langkah terakhir tidak bisa
/// meluap dan membuat urutan satu elemen jadi tak berujung.
pub fn range_isi(awal: i128, langkah: i128, jumlah: usize) -> impl ExactSizeIterator<Item = i128> {
    (0..jumlah).map(move |k| awal.wrapping_add((k as i128).wrapping_mul(langkah)))
}

/// Nilai acuan jadi argumen yang dipilah — satu pemilahan, satu kalimat.
pub fn arg_angka(v: &Value) -> ArgAngka {
    match v {
        Value::Int(n) => ArgAngka::Bulat(*n),
        Value::Float(f) => ArgAngka::Pecahan(*f),
        lain => ArgAngka::Lain(lain.type_name()),
    }
}

/// Bilangan bulat acak di `[lo, hi]` dari 128 bit acak. Satu definisi untuk
/// `rand::int` dan `crypto::random_int`: rentang `i128` penuh tidak meluap.
pub fn acak_dalam_rentang(lo: i128, hi: i128, acak: u128) -> i128 {
    let lebar = hi.wrapping_sub(lo) as u128;
    let geser = if lebar == u128::MAX { acak } else { acak % (lebar + 1) };
    lo.wrapping_add(geser as i128)
}

pub fn ulang_string(s: &str, n: i128) -> Result<String, String> {
    if n < 0 {
        return Err(format!(
            "tidak bisa mengulang string sebanyak {} kali — jumlah pengulangan tidak boleh negatif",
            n
        ));
    }
    const MAKS_BYTE: i128 = 256 * 1024 * 1024;
    let total = (s.len() as i128).saturating_mul(n);
    if total > MAKS_BYTE {
        return Err(format!(
            "hasil pengulangan kegedean: {} karakter x {} kali = sekitar {} byte \
             (batasnya 256 MiB). Cek lagi angka pengulangannya",
            s.chars().count(), n, total
        ));
    }
    Ok(s.repeat(n as usize))
}

/// Apakah sebuah float dianggap benar dalam konteks kondisi?
#[inline]
/// Pesan kegagalan `assert`.
pub fn pesan_sort_by_satu_param() -> String {
    "sort_by butuh fungsi pembanding dengan DUA parameter (`|a, b| ...`) yang \
     mengembalikan angka: negatif kalau `a` duluan, positif kalau `b` duluan.\n  \
     bantuan: kalau maksudnya menentukan dasar pengurutan dari tiap elemen, \
     pakai `sort_by_key(|x| ...)`".to_string()
}

/// Pesan "penerima ini tidak punya method/field bernama X" — satu definisi
/// untuk kedua backend dan untuk setiap jenis penerima.
pub const BUKAN_METODE_STR: &[&str] = &["join"];

/// Sama untuk channel: `chan::make(kapasitas)` MEMBUAT channel, jadi `c.make()`
/// tidak menanyakan apa pun tentang `c`.
pub const BUKAN_METODE_CHAN: &[&str] = &["make"];

/// Sama untuk array: `arr::count(a, f)` alias `arr::count_if` di bentuk NAMESPACE saja —
/// `.count(f)` bukan method di mesin mana pun, jadi tidak dilayani dan tidak disarankan.
pub const BUKAN_METODE_ARR: &[&str] = &["count"];

/// Nama yang boleh disarankan sebagai method sebuah penerima — SATU definisi,
/// dipakai interpreter dan VM. Dua salinan berarti `.cap()` disarankan `map`
/// di satu mesin dan tidak di mesin lain, untuk program yang sama.
pub fn calon_method(ns: &str) -> Vec<String> {
    let awalan = format!("{}::", ns);
    let mut v: Vec<String> = crate::vm::daftar_builtin()
        .into_iter()
        .filter_map(|n| n.strip_prefix(&awalan).map(|s| s.to_string()))
        .filter(|s| !s.starts_with("__"))
        // Yang bukan method tidak boleh disarankan: sebuah saran yang menuntun
        // ke pintu yang juga menolak cuma memindahkan kebingungannya.
        .filter(|s| match ns {
            "dict" => crate::interpreter::builtin::dict::METODE_DICT.contains(&s.as_str()),
            "str" => !BUKAN_METODE_STR.contains(&s.as_str()),
            "chan" => !BUKAN_METODE_CHAN.contains(&s.as_str()),
            "arr" => !BUKAN_METODE_ARR.contains(&s.as_str()),
            _ => true,
        })
        .collect();
    // `len` dan nama HOF didispatch tanpa lewat `globals`, jadi keduanya tidak
    // ikut terpanen di atas dan harus ditambahkan di sini.
    if ns == "arr" || ns == "str" {
        v.push("len".to_string());
        v.extend(crate::vm::Vm::NAMA_HOF.iter().map(|s| s.to_string()));
    }
    v
}

/// Method bawaan sebuah ENUM, per tipe enumnya.
pub fn calon_method_enum(tipe: &str) -> &'static [&'static str] {
    match tipe {
        "Option" => &["and_then", "expect", "is_none", "is_some", "map", "ok",
                      "to_string", "unwrap", "unwrap_or"],
        "Result" => &["and_then", "err", "expect", "is_err", "is_ok", "map",
                      "map_err", "ok", "to_string", "unwrap", "unwrap_err",
                      "unwrap_or"],
        // Enum buatan pemakai tidak mewarisi satu pun dari keduanya. Yang
        // dimilikinya cuma hook tampilan — dan method `impl`-nya sendiri,
        // yang ditambahkan pemanggil karena cuma ia yang punya tabelnya.
        _ => &["to_string"],
    }
}

/// Sama untuk nilai skalar. `int` dan `float` sengaja berbeda: `math::sqrt(5)`
/// ada, `(5).sqrt()` tidak, dan menyarankan nama yang pintunya juga menolak
/// bukan bantuan.
pub fn calon_method_skalar(tipe: &str) -> &'static [&'static str] {
    match tipe {
        "float" => &["abs", "ceil", "cos", "exp", "floor", "is_finite", "is_inf",
                     "is_nan", "ln", "log10", "pow", "round", "sin", "sqrt",
                     "tan", "to_string"],
        "int" => &["abs", "to_string"],
        "bool" | "rune" => &["to_string"],
        _ => &[],
    }
}

/// `(5).sqrt()` — ejaannya benar, pintunya yang salah; "cek lagi penulisannya"
/// menyuruh memeriksa ejaan yang benar. Tiap klausa di sini harus benar untuk
/// SEMUA nama: `math::` menolak `bool`/`rune`, dan `math::PI` bukan fungsi.
pub fn padanan_math(tipe: &str, method: &str) -> Option<String> {
    if !matches!(tipe, "int" | "float")
        || calon_method_skalar(tipe).contains(&method)
        || !crate::vm::ada_builtin(&format!("math::{}", method))
    {
        return None;
    }
    Some(if calon_method_skalar("float").contains(&method) {
        format!("method itu cuma milik `float`; untuk `{}` pakai `math::{}`", tipe, method)
    } else {
        format!("yang ada `math::{}`, bukan method", method)
    })
}

/// `(2.5).sqrt()` dan `(2.0).pow(3)` dilayani `math::` yang SAMA di dua mesin.
/// Jalur panas, jadi tanpa alokasi: bahwa tiap nama di sini memang ada di
/// `math::` dikunci `metode_skalar_satu_implementasi_dengan_math`.
pub fn metode_skalar_math(tipe: &str, method: &str) -> bool {
    matches!(tipe, "int" | "float") && method != "to_string"
        && calon_method_skalar(tipe).contains(&method)
}

/// Method skalar lewat pintu yang sama dengan method bawaan lain.
pub fn pesan_arg_metode_skalar(tipe: &str, method: &str, diberi: usize) -> Option<String> {
    if !metode_skalar_math(tipe, method) { return None; }
    pesan_jumlah_arg_metode("math", method, diberi)
}

/// Apakah `method` memang method bawaan penerima ber-namespace `ns`. Nama di
/// luar daftar ini dijawab "tidak punya method", bukan kalimat jumlah argumen.
fn metode_bawaan_ada(ns: &str, method: &str) -> bool {
    match ns {
        "str" => !BUKAN_METODE_STR.contains(&method),
        "chan" => !BUKAN_METODE_CHAN.contains(&method),
        "arr" => !BUKAN_METODE_ARR.contains(&method),
        "dict" => crate::interpreter::builtin::dict::METODE_DICT.contains(&method)
            || method == "to_string",
        "Option" | "Result" => calon_method_enum(ns).contains(&method),
        "atomic" | "mutex" => crate::sinkron::metode_ada(ns, method),
        _ => true,
    }
}

/// Paling banyak berapa argumen DITULIS: batas tabel aritas fungsi yang
/// melayani method itu, dikurangi penerimanya. Tanpa entri berarti tidak
/// diperiksa — daftar yang tidak lengkap aman, daftar yang salah tidak.
fn maks_arg_metode(ns: &str, method: &str) -> Option<usize> {
    use crate::interpreter::arity::maks_ns;
    let penuh = match (ns, method) {
        (_, "to_string") => return Some(0),
        // Method angka tidak punya argumen opsional: `pow` pasti 1, sisanya 0.
        // Tabel aritas tidak memuat seluruh `math::`, jadi bukan dari sana.
        ("math", _) => return Some(bentuk_metode("math", method).map(|(n, _)| n).unwrap_or(0)),
        ("arr" | "str", "len") => maks_ns("", "len"),
        ("arr", "fill") => maks_ns("arr", "__metode_fill"),
        ("arr", "starts_with") => maks_ns("arr", "__metode_starts_with"),
        ("arr", "ends_with") => maks_ns("arr", "__metode_ends_with"),
        ("str", "parse_int") => maks_ns("str", "to_int"),
        ("str", "parse_float") => maks_ns("str", "to_float"),
        _ => maks_ns(ns, method),
    }?;
    penuh.checked_sub(1)
}

/// Jumlah argumen yang salah untuk method bawaan, kurang MAUPUN lebih — yang
/// lebih dulu dibuang diam-diam di dua mesin. Kalimatnya kalimat method.
pub fn pesan_jumlah_arg_metode(ns: &str, method: &str, diberi: usize) -> Option<String> {
    if !metode_bawaan_ada(ns, method) { return None; }
    let maks = maks_arg_metode(ns, method);
    // Jalur panas: tepat di batasnya berarti minimumnya juga terpenuhi —
    // minimum <= maksimum dikunci `minimum_tidak_melebihi_maksimum`.
    if maks == Some(diberi) { return None; }
    let (min, bentuk) = bentuk_metode(ns, method).unwrap_or((0, ""));
    if diberi < min {
        return Some(pesan_metode_argumen(ns, method, diberi));
    }
    let maks = maks?;
    if diberi <= maks { return None; }
    Some(if min == maks && maks == 0 {
        format!("method `{}` butuh 0 argumen, dapat {}", method, diberi)
    } else if min == maks {
        format!("method `{}` butuh {} argumen {}, dapat {}", method, maks, bentuk, diberi)
    } else {
        format!("method `{}` butuh paling banyak {} argumen, dapat {}", method, maks, diberi)
    })
}

/// Pesan "operator ini tidak berlaku antara dua tipe ini" — satu definisi
/// untuk kedua backend dan untuk seluruh keluarga operator.
pub fn pesan_op_unary(op: &str, tipe: &str) -> String {
    let untuk = match op { "-" => "angka (int/float)", "~" => "bilangan bulat", _ => "bool" };
    format!("operator `{}` cuma untuk {}, dapat `{}`", op, untuk, tipe)
}

/// Operator bit (`&` `|` `^` `<<` `>>` `&^`) atas operand yang bukan bilangan bulat.
pub fn pesan_op_bit(op: &str, kiri: &str, kanan: &str) -> String {
    format!("tidak bisa `{}` antara `{}` dan `{}` — operator bit cuma untuk bilangan bulat",
            op, kiri, kanan)
}

pub fn pesan_op_biner(op: &str, kiri: &str, kanan: &str) -> String {
    // `"%d" % 5` — kebiasaan Python. Kalimat biasa di bawah benar dan buntu:
    // ia menyebut tipenya, dan tidak menyebut bahwa yang dicari memang ADA.
    if op == "%" && kiri == "string" {
        return format!(
            concat!(
                "tidak bisa `%` antara `string` dan `{}` — di JRG `%` cuma sisa bagi. ",
                "Untuk menyisipkan nilai ke dalam teks pakai `str::format(\"{{}}\", x)` ",
                "atau template `\"\"\"...${{x}}\"\"\"`"),
            kanan);
    }
    format!("tidak bisa `{}` antara `{}` dan `{}`{}", op, kiri, kanan,
            petunjuk_operand_fungsi(kiri, kanan))
}

/// Kalau salah satu operand sebuah FUNGSI, yang terjadi hampir pasti bukan
/// "operator ini tidak berlaku" melainkan tanda kurung pemanggilan yang lupa.
/// Diturunkan dari NAMA TIPE, jadi kedua mesin mendapatkannya dari satu tempat.
pub fn petunjuk_operand_fungsi(kiri: &str, kanan: &str) -> String {
    if !tipe_bisa_dipanggil(kiri) && !tipe_bisa_dipanggil(kanan) { return String::new(); }
    "\n  bantuan: salah satu operand di sini sebuah fungsi — kalau maksudnya \
     memanggilnya, tulis pakai kurung, mis. `nama(argumen)`\n  \
     catatan: tanpa kurung, baris ini terbaca sebagai operasi aritmetika biasa \
     antara sebuah fungsi dan nilai lain, dan itu memang tidak ada artinya"
        .to_string()
}

/// Apakah nama tipe ini sebuah nilai yang bisa dipanggil.
pub fn tipe_bisa_dipanggil(t: &str) -> bool {
    matches!(t, "function" | "closure" | "native_fn" | "extern_fn")
        || t.starts_with("enum_ctor:")
}

/// Pesan untuk pembandingan (`<` `<=` `>` `>=`) antara dua tipe yang tidak
/// saling terbanding. Lihat [`pesan_op_biner`] — dipisah karena kata kerjanya
/// beda, bukan karena aturannya beda.
pub fn pesan_banding(kiri: &str, kanan: &str) -> String {
    let dasar = format!("tidak bisa membandingkan `{}` dengan `{}`", kiri, kanan);
    // Dua nilai yang sama-sama bisa DIPANGGIL: satu kalimat untuk `==`, `!=` dan
    // keempat urutan sekaligus.
    if !tipe_bisa_dipanggil(kiri) || !tipe_bisa_dipanggil(kanan) { return dasar; }
    format!(
        "{} — sebuah fungsi tidak punya isi untuk dibanding, dan di JRG nilai \
         tidak pernah beralias jadi ia juga tidak punya identitas yang bisa \
         ditanyakan\n  bantuan: bandingkan penandanya, bukan fungsinya — simpan \
         `{{\"nama\": \"kirim\", \"fn\": kirim}}` lalu bandingkan `.nama`, atau pakai \
         dict berkunci nama sebagai daftar handler",
        dasar)
}

/// Pesan untuk pengurutan yang elemennya tidak saling terbanding.
pub fn pesan_urut_tak_sebanding(konteks: &str, kiri: &str, kanan: &str) -> String {
    format!(
        "{}: tidak bisa membandingkan `{}` dengan `{}`, jadi urutannya tidak \
         terdefinisi. Samakan dulu tipe isinya, atau pakai `sort_by_key` untuk \
         menentukan sendiri dasar pengurutannya",
        konteks, kiri, kanan)
}

/// Pesan untuk `len()` atas nilai yang tidak punya panjang.
pub fn pesan_len_tak_bisa(tipe: &str) -> String {
    format!("len(): tidak bisa ukur `{}` — yang punya panjang cuma array, \
             string, tuple, dict, dan channel", tipe)
}

/// Bentuk path atas method ber-`self` (`P::tambah(p, 4)`) dipanggil dengan
/// penerima bertipe lain.
pub fn pesan_arg_harus_array(fname: &str, ke: usize, dapat: &str) -> String {
    format!("{}: argumen ke-{} harus array, dapat `{}`", fname, ke, dapat)
}

/// Satu kalimat untuk "argumen ini harus teks", dipakai argumen PERTAMA
/// seluruh `str::` dan argumen POLA-nya, di dua mesin. `peran` menyebut
/// bagian mana yang salah ("argumen pertama", "yang dicari", "pemisahnya").
pub fn pesan_arg_harus_teks(nama: &str, peran: &str, dapat: &str) -> String {
    format!("{}: {} harus string, dapat `{}` — kalau yang dimaksud memang bentuk \
             TAMPILANNYA, tulis `str(x)` dulu", nama, peran, dapat)
}

/// Pasangan `pesan_arg_harus_array` untuk seluruh `dict::`. `dapat` datang dari
/// `type_name()` interpreter atau `type_name_penuh()` VM, yang kata-katanya sama.
pub fn pesan_arg_harus_dict(fname: &str, ke: usize, dapat: &str) -> String {
    format!("{}: argumen ke-{} harus dict, dapat `{}`{}", fname, ke, dapat,
            crate::interpreter::galat_index::kurung_kosong(dapat))
}

/// Operand operator channel (`<-`, `select`) yang bukan channel — menyebut TIPE-nya,
/// bukan isinya, supaya pesannya tidak tumbuh mengikuti data.
pub fn pesan_bukan_channel(operasi: &str, dapat: &str) -> String {
    format!("{} butuh channel, dapat `{}`", operasi, dapat)
}

/// Kapasitas `Chan::new` yang tidak sah. Angka negatif disebut ANGKANYA — dulu
/// kalimatnya "harus int >= 0, dapat `int`", yang menyangkal dirinya sendiri.
pub fn pesan_kapasitas_chan(negatif: Option<i128>, tipe: &str) -> String {
    match negatif {
        Some(n) => format!("Chan::new: kapasitas tidak boleh negatif, dapat {}", n),
        None => format!("Chan::new: kapasitas harus int >= 0, dapat `{}`", tipe),
    }
}

/// Argumen pertama keluarga `chan::` yang bukan channel — satu kalimat, dua mesin.
pub fn pesan_arg_harus_channel(fname: &str, dapat: &str) -> String {
    format!("{}: argumen ke-1 harus channel, dapat `{}`", fname, dapat)
}

/// Elemen `dict::from_pairs` yang bukan pasangan. Menyebut tipe atau panjangnya,
/// bukan isinya: pesannya tidak boleh tumbuh mengikuti data.
pub fn pesan_bukan_pasangan(fname: &str, index: usize, tipe: &str) -> String {
    format!("{}: elemen di index {} harus pasangan [kunci, nilai], dapat `{}`", fname, index, tipe)
}

pub fn pesan_pasangan_bukan_dua(fname: &str, index: usize, n: usize) -> String {
    format!("{}: elemen di index {} harus pasangan [kunci, nilai], dapat {} elemen", fname, index, n)
}

/// Jumlah argumen minimal dan bentuk tiap `dict::` — satu tabel untuk dua
/// backend. VM dulu mengisi yang kosong dengan `nil` alih-alih menolak.
fn bentuk_dict(nama: &str) -> Option<(usize, &'static str)> {
    Some(match nama {
        "dict::from_pairs" => (1, "(pasangan) — mis. dict::from_pairs([[\"a\", 1]])"),
        "dict::get" | "dict::has" | "dict::remove" => (2, "(d, kunci)"),
        "dict::get_or" => (3, "(d, kunci, cadangan)"),
        "dict::set" => (3, "(d, kunci, nilai)"),
        "dict::merge" => (2, "(d1, d2) — isi d2 menang kalau kuncinya sama"),
        "dict::len" | "dict::is_empty" | "dict::keys" | "dict::values" | "dict::items" => (1, "(d)"),
        _ => return None,
    })
}

pub fn pesan_dict_kurang(nama: &str) -> String {
    match bentuk_dict(nama) {
        Some((_, bentuk)) => format!("{} butuh {}", nama, bentuk),
        None => format!("{} kurang argumen", nama),
    }
}

/// `Err` berisi kalimat `pesan_dict_kurang` kalau argumennya kurang.
pub fn cek_dict_kurang(nama: &str, diberi: usize) -> Result<(), String> {
    match bentuk_dict(nama) {
        Some((minimal, _)) if diberi < minimal => Err(pesan_dict_kurang(nama)),
        _ => Ok(()),
    }
}

/// `for … in x` atas nilai yang tidak bisa diiterasi. Menyebut TIPE, bukan
/// nilai; struct dapat kalimatnya sendiri karena jalan keluarnya lain.
pub fn pesan_tak_bisa_iterasi(tipe: &str) -> String {
    match tipe.strip_prefix("struct:") {
        Some(nama) => format!(
            "for-in: `{}` itu struct, bukan koleksi — field struct tidak diiterasi. \
             Kalau memang butuh pasangan key/nilai, pakai dict (`{{ \"a\": 1 }}`)", nama),
        None => format!(
            "for-in: tidak bisa iterate `{}` — yang bisa: array, string, dict, \
             range (`0..n`), dan angka", tipe),
    }
}

/// Basis `...x` di literal struct harus punya field untuk disalin.
pub fn pesan_sebar_bukan_array(tipe: &str) -> String {
    let saran = match tipe {
        "string" => " (untuk menyebar karakternya: `...str::chars(s)`)",
        "dict" => " (untuk menyebar kuncinya: `...dict::keys(d)`)",
        _ => "",
    };
    format!("`...` di literal array dan argumen pemanggilan cuma bisa menyebar array — dapat `{}`{}",
        tipe, saran)
}

pub fn pesan_sebar_bukan_wadah(tipe: &str) -> String {
    format!(
        "`...` di literal struct cuma bisa menyalin dari struct, dict, atau varian enum \
         ber-field — dapat `{}`, yang tidak punya field untuk disalin",
        tipe
    )
}

/// Nama parameter untuk pesan: pola yang diturunkan parser (`__pola0`) disebut posisinya.
fn nama_param(nama: &str, posisi: usize) -> String {
    if nama.is_empty() || nama.starts_with("__pola") { format!("ke-{}", posisi) } else { format!("`{}`", nama) }
}

/// Fungsi ATAU closure JRG dipanggil dengan argumen lebih — satu kalimat untuk dua
/// mesin dan tiap jalur (langsung, `terap`, callback). Closure dulu dijawab
/// `closure: butuh 1 arg, dapat 2` di acuan saja.
pub fn pesan_fungsi_kebanyakan(nama: &str, menerima: usize, diberi: usize, params: &[String]) -> String {
    let daftar = if params.is_empty() {
        "tidak ada".to_string()
    } else {
        params.iter().enumerate().map(|(i, p)| nama_param(p, i + 1).replace('`', ""))
            .collect::<Vec<_>>().join(", ")
    };
    format!("fungsi `{}` kebanyakan argumen — dia cuma menerima {}, tapi dikasih {}.\n  \
             bantuan: parameternya ({})", nama, menerima, diberi, daftar)
}

/// Pasangannya untuk parameter wajib yang tidak dapat argumen.
pub fn pesan_parameter_wajib(nama: &str, param: &str, posisi: usize) -> String {
    format!("fungsi `{}`: parameter {} (posisi {}) wajib diisi — tidak dapat argumen \
             dan tidak punya nilai default", nama, nama_param(param, posisi), posisi)
}

/// Argumen bernama ke sesuatu yang tidak menyimpan nama parameternya.
pub fn pesan_bernama_tanpa_param(tipe: &str) -> String {
    format!("argumen bernama cuma untuk fungsi yang ditulis di JRG — `{}` tidak menyimpan \
             nama parameternya (builtin, konstruktor, dan `extern fn` begitu); isi \
             argumennya berurutan", tipe)
}

/// Konstruktor varian dipanggil dengan jumlah payload yang salah — satu kalimat
/// untuk tiga mesin, langsung maupun lewat nilai (`let f = E::A; f(1, 2)`).
pub fn pesan_konstruktor_argumen(tipe: &str, varian: &str, arity: usize, dapat: usize) -> String {
    format!("konstruktor `{}::{}` butuh {} argumen, dapat {}", tipe, varian, arity, dapat)
}

/// Jumlah sub-pola tidak sama dengan jumlah payload-nya. Dulu diam-diam TIDAK
/// COCOK di dua mesin acuan, jadi salah ketik satu sub-pola membuat lengannya
/// dilewati. Satu definisi untuk tiga mesin.
pub fn pesan_subpola_varian(varian: &str, butuh: usize, dapat: usize) -> String {
    format!("varian `{}` butuh {} sub-pola, dapat {}", varian, butuh, dapat)
}

/// Nama yang DIDEKLARASIKAN tapi bukan nilai, dipakai sebagai nilai. Satu definisi
/// untuk dua mesin — `--vm` dulu menuduh nama-nama ini salah ketik.
pub fn pesan_nama_struct(nama: &str) -> String {
    format!("`{}` itu nama struct (tipe), bukan variabel. Untuk membuat nilainya tulis \
             literal struct: `{} {{ ... }}`", nama, nama)
}

pub fn pesan_nama_enum(nama: &str, varian_pertama: Option<&str>) -> String {
    format!("`{}` itu nama enum (tipe), bukan variabel. Sebut varian-nya, mis. `{}::{}`",
            nama, nama, varian_pertama.unwrap_or("Varian"))
}

pub fn pesan_nama_trait(nama: &str) -> String {
    format!("`{}` itu nama trait, bukan variabel — trait cuma dipakai di `impl {} for Tipe` \
             dan di batas generik `<X: {}>`", nama, nama, nama)
}

/// `E::V` untuk varian ber-field: acuan dulu menjawab `nil` diam-diam.
pub fn pesan_varian_berfield(tipe: &str, varian: &str, field: &[String]) -> String {
    let isi = field.iter().map(|f| format!("{}: ...", f)).collect::<Vec<_>>().join(", ");
    format!("`{}::{}` itu varian ber-field, bukan nilai — bangun nilainya dengan literal: \
             `{}::{} {{ {} }}`", tipe, varian, tipe, varian, isi)
}

/// Varian TELANJANG milik modul yang diimpor: nilai milik modulnya, jadi ber-awalan.
pub fn pesan_varian_modul(varian: &str, tipe: &str, modul: &str) -> String {
    format!("`{}` itu varian `{}::{}` milik modul `{}` — dari luar modulnya tulis `{}::{}` \
             atau `{}::{}`", varian, tipe, varian, modul, tipe, varian, modul, varian)
}

/// Sama, tapi di posisi POLA: di sini nama telanjang tidak sekadar gagal,
/// ia mengikat dan cocok dengan APA PUN. Aturannya satu dengan posisi nilai.
pub fn pesan_varian_modul_pola(varian: &str, tipe: &str, modul: &str) -> String {
    format!("{} (di posisi pola nama telanjang malah MENGIKAT dan cocok dengan apa pun, \
             jadi lengan ini akan menelan semuanya)",
            pesan_varian_modul(varian, tipe, modul))
}

/// `var` tingkat modul yang dibaca dari luar: keadaan milik modulnya, tidak diekspor.
pub fn pesan_var_modul(nama: &str, modul: &str) -> String {
    // Nama fungsinya tidak boleh sama dengan `var`-nya: dua nama tingkat atas yang sama ditolak.
    let kecil = nama.to_lowercase();
    let pembaca = if kecil != nama { kecil } else { format!("ambil_{}", nama) };
    format!("`{}::{}` tidak bisa dibaca dari luar — `{}` adalah `var` milik modul `{}`: nilainya \
             berubah-ubah, jadi yang diekspor cuma fungsi dan nilai tetap. Sediakan fungsi di \
             modulnya (mis. `pub fn {}() {{ return {} }}`), atau jadikan `const`/`let` kalau \
             memang tidak pernah berubah", modul, nama, nama, modul, pembaca, nama)
}

/// Nama ber-awalan `_` milik modul: privat, tidak terjangkau dari luar.
pub fn pesan_privat_modul(nama: &str, modul: &str) -> String {
    format!("`{}::{}` tidak ditemukan — `{}` diawali `_`, jadi ia privat milik modul `{}` dan \
             cuma bisa dipakai dari dalam modulnya", modul, nama, nama, modul)
}

/// `asli::nama` di berkas yang mengimpor modul `asli` dengan nama lain (`import "std/set" as s`).
pub fn pesan_modul_beralias(path: &str, asli: &str, alias: &str) -> String {
    let ekor = path.split_once("::").map(|(_, e)| e).unwrap_or("");
    format!("`{}` tidak ditemukan — modul `{}` diimpor dengan nama `{}` di berkas ini, jadi \
             tulis `{}::{}`", path, asli, alias, alias, ekor)
}

/// `main` yang dideklarasikan sebagai `const`/`let`/`var`. Dulu dua kalimat: acuan mencetak
/// NILAINYA, VM menjawab "nilai bertipe `int` bukan fungsi".
pub const PESAN_MAIN_BUKAN_FN: &str =
    "`main` di berkas ini bukan fungsi — program mulai dari `pub fn main() { … }`";

/// `x::f` di berkas yang tidak mengimpor `x`, sementara `x` diimpor berkas LAIN.
pub fn pesan_modul_tak_diimpor(path: &str, modul: &str) -> String {
    format!("`{}` tidak ditemukan — modul `{}` memang dimuat, tapi oleh berkas lain: tiap berkas \
             cuma melihat modul yang di-`import`-nya sendiri. Tambahkan `import`-nya di berkas ini",
            path, modul)
}

/// Nama tingkat atas yang dibaca sebelum barisnya sempat dijalankan. Dulu dijawab "belum
/// dikenal … lupa `let B`?" untuk nama yang jelas dideklarasikan beberapa baris di bawahnya.
pub fn pesan_belum_diisi(nama: &str) -> String {
    format!("`{}` dipakai sebelum nilainya diisi — ia dideklarasikan di tingkat atas, tapi \
             barisnya belum dijalankan: tingkat atas dijalankan berurutan dari atas. Pindahkan \
             deklarasi `{}` ke atas baris ini, atau pakai di dalam fungsi yang dipanggil \
             sesudahnya", nama, nama)
}

/// Berkas ber-`package` tanpa `main` — satu kalimat untuk dua mesin (VM dulu menjawab
/// "variabel `main` belum dikenal … lupa `let main = ...`").
pub const PESAN_TANPA_MAIN: &str = "tidak ada `pub fn main()`";

/// Penugasan ke nama yang belum dideklarasikan. Dulu `x = 5` diam-diam MEMBUAT variabel
/// (lokal di interpreter, global di VM), jadi salah ketik satu huruf lolos tanpa suara.
pub fn pesan_tugas_tanpa_deklarasi(nama: &str, saran: Option<&str>) -> String {
    let ekor = match saran {
        Some(s) => format!("; kalau maksudnya `{}`, betulkan ejaannya", s),
        None => String::new(),
    };
    format!("`{}` belum dideklarasikan, jadi tidak bisa ditugasi — tulis `var {} = …` (atau \
             `{} := …`) untuk membuat variabel baru{}", nama, nama, nama, ekor)
}

/// Penugasan ke nama yang ADA tapi bukan variabel (fungsi, builtin, varian enum). Dulu
/// `f = 5` menimpa fungsinya di acuan, dan di VM `A = 5` atas varian `A` tidak berakibat apa-apa.
pub fn pesan_tugas_bukan_variabel(nama: &str, bisa_dipanggil: bool) -> String {
    let jenis = if bisa_dipanggil { "fungsi" } else { "nilai tetap" };
    format!("`{}` bukan variabel — di sini ia {}, jadi tidak bisa ditugasi. Kalau memang perlu \
             variabel bernama itu, deklarasikan dulu: `var {} = …`", nama, jenis, nama)
}

/// `::nama` — bawaan bahasa tanpa peduli apa yang dideklarasikan program — untuk nama
/// yang bukan bawaan.
pub fn pesan_bukan_bawaan(nama: &str, saran: Option<&str>) -> String {
    let ekor = match saran {
        Some(s) => format!(" — maksudnya `::{}`?", s),
        None => String::new(),
    };
    format!("`::{}` tidak ditemukan: `::` di depan nama berarti bawaan bahasa (yang tidak bisa \
             dibayangi deklarasi program), dan tidak ada bawaan bernama `{}`{}", nama, nama, ekor)
}

pub fn pesan_extern_static(nama: &str) -> String {
    format!("`{}` itu `extern static` (variabel global dari pustaka luar) — membaca/menulisnya \
             belum didukung jrgc, jadi nilainya tidak bisa dipakai. Bungkus lewat `extern fn` \
             yang mengembalikan nilainya", nama)
}

pub fn pesan_nama_modul(nama: &str, contoh: &str) -> String {
    format!("`{}` itu nama modul, bukan variabel — modul tidak bisa dipakai sebagai nilai. \
             Sebut isinya pakai `::`, mis. `{}::{}`", nama, nama, contoh)
}

/// Contoh isi modul untuk kalimat di atas: fungsi PERTAMA yang dideklarasikannya,
/// diturunkan dari berkasnya supaya dua mesin menyebut nama yang sama.
pub fn contoh_isi_modul(file: &crate::parser::ParsedFile) -> String {
    use crate::parser::Item;
    let publik = |n: &String| !n.starts_with('_');
    file.items.iter().find_map(|it| match it { Item::Fn(fd) if publik(&fd.name) => Some(fd.name.clone()), _ => None })
        .or_else(|| file.items.iter().find_map(|it| match it {
            Item::Const(c) if publik(&c.name) => Some(c.name.clone()),
            Item::Var(v) if publik(&v.name) => Some(v.name.clone()),
            _ => None,
        }))
        .unwrap_or_else(|| "nama_fungsi".to_string())
}

pub fn pesan_penerima_salah(jalur: &str, pemilik: &str, dapat: &str) -> String {
    format!(
        "`{}` adalah method milik `{}`, jadi argumen pertamanya harus `{}` \
         (atau turunannya) — yang dikasih `{}`.\n  \
         bantuan: bentuk method-nya (`x.{}( … )`) memilih sendiri method milik \
         tipe `x`, jadi ia tidak bisa meleset seperti ini",
        jalur, pemilik, pemilik, dapat,
        jalur.rsplit("::").next().unwrap_or(jalur),
    )
}

/// Sama, tapi ekornya sebuah JALAN KELUAR alih-alih saran ejaan.
pub fn pesan_method_tak_ada_dgn_jalan(tipe: &str, method: &str, jalan: &str) -> String {
    format!("`{}` tidak punya method atau field bernama `{}` — {}", tipe, method, jalan)
}

pub fn pesan_method_tak_ada(
    tipe: &str, method: &str, tambahan: &str, saran: Option<&str>,
) -> String {
    match saran {
        // Saran menggantikan "cek lagi penulisannya ya": begitu ada nama
        // konkret untuk ditawarkan, menyuruh orang mengecek ulang cuma
        // menunda hal yang sudah diketahui.
        Some(s) => format!(
            "`{}` tidak punya method atau field bernama `{}`{} — maksudnya `{}`?{}",
            tipe, method, tambahan, s, crate::interpreter::galat_index::kurung_kosong(tipe)),
        None => format!(
            "`{}` tidak punya method atau field bernama `{}`{} — cek lagi penulisannya ya{}",
            tipe, method, tambahan, crate::interpreter::galat_index::kurung_kosong(tipe)),
    }
}

/// `math::abs(i128::MIN)`: satu-satunya integer yang lawan positifnya tidak muat.
pub fn pesan_abs_min() -> String {
    format!("math::abs({}): tidak ada lawan positifnya di integer 128 bit — \
             batas bawahnya memang satu lebih jauh dari batas atasnya", i128::MIN)
}

/// `-x` untuk `i128::MIN` — dulu membungkus diam-diam jadi dirinya sendiri, di dua mesin.
pub fn pesan_neg_min() -> String {
    format!("hasil `-({})` kelewat besar buat integer — tidak ada lawan positifnya di \
             integer 128 bit, karena batas bawahnya memang satu lebih jauh dari batas atasnya",
            i128::MIN)
}

/// Operator aritmetika integer yang meluap — SATU kalimat; dulu interpreter dan VM
/// masing-masing punya kalimatnya sendiri untuk kesalahan yang sama.
pub fn pesan_luber(op: &str, x: i128, y: i128) -> String {
    format!(
        "hasil `{} {} {}` kelewat besar buat integer — integer JRG maksimal 128 bit \
         (kira-kira 1.7e38). Kalau memang butuh angka sebesar itu, pakai float \
         (mis. `{}.0 {} {}.0`), tapi ingat float tidak eksak untuk bilangan bulat besar",
        x, op, y, x, op, y)
}

/// Literal integer di luar jangkauan — ditolak PARSER, jadi tiga mesin satu kalimat.
pub fn pesan_literal_luber(tulisan: &str) -> String {
    format!(
        "literal integer `{}` di luar jangkauan integer JRG — paling besar {} (`i128::MAX`), \
         dan cuma `-{}` yang boleh satu lebih jauh\n  bantuan: kalau memang butuh angka \
         sebesar itu, tulis sebagai float (`{}.0`), tapi float tidak eksak untuk bilangan \
         bulat besar",
        tulisan, i128::MAX, i128::MAX as u128 + 1, tulisan)
}

pub fn pesan_log_kurang() -> String {
    "math::log butuh (x, basis) — mis. math::log(8, 2) == 3".to_string()
}

/// Galat `asli` sedang membawa sebuah fungsi keluar, dan `defer`-nya ikut gagal.
pub fn pesan_defer_ikut_gagal(asli: &str, tambahan: &str) -> String {
    format!("{}\n  catatan: sebuah `defer` juga gagal saat membereskan: {}", asli, tambahan)
}

/// `match` yang tidak punya lengan untuk nilainya. Nilainya DIPOTONG.
pub fn pesan_match_tak_cocok(nilai: &str) -> String {
    format!("match tidak exhaustive: tidak ada arm yang cocok untuk nilai `{}`",
        crate::interpreter::galat_index::potong_nilai(nilai))
}

/// `a ?? b` dengan `a` yang bukan `Option`.
pub fn pesan_koalesensi_bukan_option(tipe: &str) -> String {
    format!("`??` cuma untuk `Option`, dapat `{}`\n  bantuan: sisi kirinya harus `Option` — \
             mis. hasil `.get(i)`, `.first()` atau `dict::get(d, k)`; untuk `nil` tulis \
             `x == nil $ bawaan : x`", tipe)
}

/// Rekursi yang melewati batas kedalaman — kalimat yang sama di tiga mesin.
pub fn pesan_rekursi_dalam(nama: &str, batas: usize) -> String {
    format!("rekursi terlalu dalam di fungsi `{}` — sudah lebih dari {} panggilan bersarang \
             dan belum berhenti juga.\n  \
             bantuan: pastikan ada kondisi berhenti (base case) yang PASTI tercapai, \
             dan tiap panggilan rekursif bergerak mendekatinya\n  \
             catatan: kalau memang butuh kedalaman sebesar ini, ubah jadi perulangan \
             (`while`/`for`) — itu tidak memakai stack per iterasi", nama, batas)
}

/// `a, b = e` dengan `e` yang bukan tuple — satu kalimat untuk tiga mesin.
pub fn pesan_bongkar_bukan_tuple(sasaran: usize, tipe: &str) -> String {
    format!("`=` dengan {} sasaran butuh nilai berupa tuple berisi {} nilai di sisi kanan, dapat `{}`\n  \
             bantuan: tulis satu nilai per sasaran (`a, b = x, y`), atau pastikan sisi kanannya \
             mengembalikan tuple", sasaran, sasaran, tipe)
}

/// `a, b = e` dengan tuple yang isinya tidak sebanyak sasarannya.
pub fn pesan_bongkar_jumlah(sasaran: usize, isi: usize) -> String {
    format!("`=` dengan {} sasaran, tapi tuple di sisi kanan berisi {} nilai — jumlahnya harus sama", sasaran, isi)
}

/// Pola `let` (atau parameter/`for` yang diturunkan ke `let`) yang tidak cocok.
pub fn pesan_let_tak_cocok(asal: u8, tipe: &str) -> String {
    let (apa, jalan) = match asal {
        1 => ("argumen", "terima argumennya dengan nama biasa lalu `match` di badan fungsinya"),
        2 => ("elemen `for`", "ambil elemennya dengan nama biasa lalu `match` di badan loop"),
        3 => return format!("`:=` dengan beberapa nama butuh tuple berisi sebanyak namanya di sisi kanan, dapat `{}`\n  \
                             bantuan: tulis satu nilai per nama (`a, b := x, y`), atau pastikan sisi kanannya \
                             mengembalikan tuple yang jumlah isinya sama", tipe),
        _ => ("nilai `let`", "pakai `if let`, `match`, atau `let … else { … }`"),
    };
    format!("{} bertipe `{}` tidak cocok dengan polanya\n  bantuan: pola di sini harus \
             PASTI cocok; kalau nilainya bisa tidak cocok, {}", apa, tipe, jalan)
}

pub fn pesan_bagi_nol(op: &str, kiri: &str) -> String {
    if op == "%" {
        format!("`{} % 0` — nggak bisa cari sisa bagi dengan nol. \
                 Cek lagi angka pembaginya sebelum dipakai `%`", kiri)
    } else {
        format!("`{} / 0` — nggak bisa bagi dengan nol, hasilnya nggak pernah \
                 terdefinisi. Cek lagi angka pembaginya sebelum dipakai bagi", kiri)
    }
}

/// Pesan index di luar batas untuk builtin `arr::`.
/// Index argumen builtin dengan aturan `a[i]`: negatif dihitung dari belakang (`-1` = terakhir).
/// `sisip` (`insert`) boleh menunjuk satu lewat ujung; di luar itu `None`, tidak pernah dijepit.
pub fn normal_indeks(i: i128, panjang: usize, sisip: bool) -> Option<usize> {
    let n = panjang as i128;
    let j = if i < 0 { n + i } else { i };
    let batas = if sisip { n } else { n - 1 };
    (j >= 0 && j <= batas).then_some(j as usize)
}

pub fn pesan_index_luar_batas(nama: &str, index: i128, panjang: usize) -> String {
    format!("{}: index {} di luar batas (panjang {})", nama, index, panjang)
}

/// Argumen yang harus bilangan bulat — satu kalimat, dua mesin, dengan NAMA
/// fungsinya. Pecahan mendapat petunjuk `as int`; yang lain disebut tipenya.
pub fn pesan_bukan_bulat(nama: &str, tipe: &str, pecahan: Option<&str>) -> String {
    match pecahan {
        Some(p) => format!(
            "{}: butuh bilangan bulat, dapat pecahan `{}` — kalau memang mau dipotong, \
             tulis eksplisit: `{} as int`", nama, p, p),
        None => format!("{}: butuh bilangan bulat, dapat `{}`", nama, tipe),
    }
}

pub fn pesan_ukuran_tak_masuk_akal(nama: &str, apa: &str, dapat: i128) -> String {
    if dapat < 0 {
        format!("{}: {} tidak boleh negatif, dapat {}", nama, apa, dapat)
    } else {
        format!("{}: {} harus lebih dari 0", nama, apa)
    }
}

/// Bentuk method HOF bawaan atas array, Option dan Result: (jumlah argumen, bentuknya).
/// Satu definisi untuk interpreter dan VM — dulu tiap mesin punya kalimat dan aturannya sendiri.
pub fn bentuk_hof_method(method: &str) -> Option<(usize, &'static str)> {
    Some(match method {
        "map" | "filter" | "any" | "all" | "find" | "find_idx" | "count_if" | "each"
        | "flat_map" | "partition" | "group_by" | "min_by" | "max_by"
        | "map_err" | "and_then" => (1, "(fn)"),
        "sort_by" => (1, "(fungsi pembanding `|a, b| ...`)"),
        "sort_by_key" => (1, "(fungsi kunci `|x| ...`)"),
        "reduce" => (2, "(fn, awal)"),
        "fold" => (2, "(awal, fn)"),
        "zip_with" => (2, "(array_lain, fn)"),
        _ => return None,
    })
}

/// Bentuk argumen method BUKAN-HOF, dikunci ke penerimanya: `join` menerima
/// pemisah atas sebuah array dan sub-string atas sebuah string, jadi kuncinya …
pub fn bentuk_metode(ns: &str, method: &str) -> Option<(usize, &'static str)> {
    Some(match (ns, method) {
        ("arr", "get") | ("arr", "remove") => (1, "(indeks)"),
        ("arr", "get_or") => (2, "(indeks, cadangan)"),
        ("arr", "index_of") | ("arr", "contains") | ("arr", "binary_search")
        | ("arr", "push") | ("arr", "fill") => (1, "(nilai)"),
        ("arr", "join") => (1, "(pemisah)"),
        // `drop` ejaan kedua untuk `skip`: satu operasi, jadi satu kalimat.
        ("arr", "take") | ("arr", "skip") | ("arr", "drop") | ("arr", "repeat")
        | ("arr", "rotate") => (1, "(n)"),
        ("arr", "chunks") => (1, "(ukuran)"),
        ("arr", "windows") => (1, "(lebar)"),
        ("arr", "insert") => (2, "(indeks, nilai)"),
        ("arr", "swap") => (2, "(i, j)"),
        ("arr", "concat") | ("arr", "starts_with") | ("arr", "ends_with")
        | ("arr", "union") | ("arr", "intersect") | ("arr", "difference")
        | ("arr", "zip") => (1, "(array)"),

        ("str", "split") => (1, "(pemisah)"),
        ("str", "starts_with") | ("str", "strip_prefix") => (1, "(awalan)"),
        ("str", "ends_with") | ("str", "strip_suffix") => (1, "(akhiran)"),
        ("str", "contains") | ("str", "find") | ("str", "index_of")
        | ("str", "count") => (1, "(sub)"),
        ("str", "repeat") => (1, "(n)"),
        ("str", "char_at") => (1, "(indeks)"),
        ("str", "pad_left") | ("str", "pad_right") | ("str", "center")
        | ("str", "ljust") | ("str", "rjust") => (1, "(lebar)"),
        ("str", "replace") => (2, "(dari, jadi)"),
        ("str", "substr") => (1, "(mulai[, panjang])"),
        // Method matematika atas penerima angka. `pow` satu-satunya yang butuh
        // argumen; sisanya (`sqrt`, `abs`, …) tidak, jadi tidak ada di sini.
        ("math", "pow") => (1, "(eksponen)"),

        // Dulu bentuk method ini dijawab kalimat bentuk NAMESPACE —
        // `d.get()` → "dict::get butuh (d, kunci)", menyuruh menyerahkan `d` lagi.
        ("dict", "get") | ("dict", "has") | ("dict", "remove") => (1, "(kunci)"),
        ("dict", "get_or") => (2, "(kunci, cadangan)"),
        ("dict", "set") => (2, "(kunci, nilai)"),
        ("dict", "merge") => (1, "(dict)"),
        ("chan", "send") | ("chan", "try_send") => (1, "(nilai)"),
        ("atomic", "set") | ("atomic", "add") | ("atomic", "sub") => (1, "(nilai)"),
        ("atomic", "cas") => (2, "(lama, baru)"),
        ("mutex", "with_lock") => (1, "(fungsi)"),
        // Tanpa cadangan `unwrap_or()` dulu MENGARANG satu: `()` di interpreter,
        // `nil` di `--vm`. `expect` tanpa pesan cuma `unwrap` yang lebih panjang.
        ("Option", "unwrap_or") | ("Result", "unwrap_or") => (1, "(cadangan)"),
        ("Option", "expect") | ("Result", "expect") => (1, "(pesan)"),
        _ => return None,
    })
}

/// Method bawaan BUKAN-HOF dengan jumlah argumen yang salah.
pub fn pesan_metode_argumen(ns: &str, method: &str, diberi: usize) -> String {
    let (n, bentuk) = bentuk_metode(ns, method).unwrap_or((1, "(nilai)"));
    format!("method `{}` butuh {} argumen {}, dapat {}", method, n, bentuk, diberi)
}

/// Method HOF bawaan dengan jumlah argumen yang salah — kurang maupun lebih.
pub fn pesan_hof_method_argumen(method: &str, diberi: usize) -> String {
    let (n, bentuk) = bentuk_hof_method(method).unwrap_or((1, "(fn)"));
    format!("method `{}` butuh {} argumen {}, dapat {}", method, n, bentuk, diberi)
}

/// Memanggil nilai yang bukan fungsi. Menyebut TIPE, bukan isi: pesannya tidak boleh
/// tumbuh bersama datanya, dan tidak menyebut `(...)` yang tidak selalu ditulis.
pub fn pesan_bukan_fungsi(tipe: &str) -> String {
    format!(
        "nilai bertipe `{}` bukan fungsi, jadi tidak bisa dipanggil — pastikan yang dipanggil \
         atau diserahkan memang fungsi atau closure",
        tipe
    )
}

/// `Future::new`/`Task::spawn` atas yang bukan fungsi — ditolak di pintunya, bukan
/// sesudah thread-nya jalan, supaya kesalahannya ketahuan di baris yang menulisnya.
pub fn pesan_tugas_bukan_fungsi(nama: &str, tipe: &str) -> String {
    format!("{}: argumen ke-1 harus fungsi atau closure, dapat `{}` — fungsinya yang \
             dijalankan di thread lain", nama, tipe)
}

pub fn pesan_hof_kurang(nama: &str) -> String {
    let bentuk = match nama {
        "arr::reduce"   => "(arr, fn, init)",
        "arr::fold"     => "(arr, awal, fn)",
        "arr::zip_with" => "(arr1, arr2, fn) — fn(a, b) per pasangan",
        _               => "(arr, fn)",
    };
    format!("{} butuh {}", nama, bentuk)
}

pub fn pesan_zip_kurang() -> String {
    "zip butuh dua array: zip(a, b)".to_string()
}

pub fn pesan_rotate_kurang() -> String {
    "arr::rotate butuh (arr, n) — n>0 geser ke kiri, n<0 ke kanan".to_string()
}

/// Pesan `arr::rotate` yang diberi `n` bukan bilangan bulat.
pub fn pesan_rotate_n(tipe: &str) -> String {
    format!("arr::rotate: `n` harus bilangan bulat, dapat `{}`", tipe)
}

pub fn pesan_elemen_bukan_angka(nama: &str, tipe: &str) -> String {
    format!("{}: semua elemen harus angka, tapi ada yang bertipe `{}`", nama, tipe)
}

/// Pesan untuk KUNCI yang tidak saling terbanding (`sort_by_key`, `min_by`,
/// `max_by`). Menyebut tipe, bukan nilai — kunci bisa saja sebesar datanya.
pub fn pesan_kunci_tak_sebanding(konteks: &str, kiri: &str, kanan: &str) -> String {
    format!("{}: kunci bertipe `{}` dan `{}` tidak bisa dibandingkan, jadi \
             urutannya tidak terdefinisi", konteks, kiri, kanan)
}

/// Pesan untuk `min`/`max` atas isi yang tidak saling terbanding.
pub fn pesan_min_max_tak_sebanding(konteks: &str, kiri: &str, kanan: &str) -> String {
    format!("{}: `{}` dan `{}` tidak bisa dibandingkan, jadi tidak ada yang \
             bisa disebut paling kecil atau paling besar", konteks, kiri, kanan)
}

/// Pesan untuk pembanding `sort_by` yang tidak menjawab angka.
pub fn pesan_pembanding_bukan_angka(tipe: &str) -> String {
    format!("fungsi pembanding `sort_by` harus mengembalikan angka \
             (negatif kalau yang kiri lebih dulu, 0 kalau sama, positif kalau \
             yang kanan lebih dulu), dapat `{}`", tipe)
}

/// Pesan untuk urutan yang memuat NaN: ia tidak punya urutan terhadap apa pun,
/// jadi "median"/"persentil" dari data begitu tidak berarti apa-apa.
pub fn pesan_nan_tak_terurut(nama: &str) -> String {
    format!("{}: array mengandung NaN, urutannya tidak terdefinisi", nama)
}

pub fn pesan_koleksi_kosong(nama: &str, apa: &str) -> String {
    format!("{}: array kosong tidak punya {}", nama, apa)
}

/// "`nama` butuh N argumen (param), dapat M" — bentuk yang sama dengan pintu
/// method dan keluarga `math::`, supaya "kurang argumen" tidak dapat ejaan baru.
pub fn pesan_butuh_argumen(nama: &str, n: usize, bentuk: &str, diberi: usize) -> String {
    format!("`{}` butuh {} argumen {}, dapat {}", nama, n, bentuk, diberi)
}

pub fn pesan_butuh_nilai(nama: &str) -> String {
    format!("{} butuh sebuah nilai, tidak diberi argumen — sebuah pertanyaan \
             tentang nilai, tanpa nilai, bukan pertanyaan", nama)
}

pub fn pesan_assert_kurang(nama: &str, minimal: usize, diberi: usize) -> String {
    format!(
        "{} butuh {} argumen, diberi {}.\n  \
         bantuan: sebuah assert tanpa argumen lengkap LULUS tanpa memeriksa \
         apa pun — itu lebih buruk daripada tidak ada assert sama sekali",
        nama, minimal, diberi)
}

pub fn pesan_assert(pesan: Option<&str>) -> String {
    match pesan {
        Some(p) => format!("assert gagal — {}", p),
        None    => "assert gagal — kondisinya tidak terpenuhi".to_string(),
    }
}

/// Pesan kegagalan `assert_eq`.
pub fn pesan_assert_eq(
    kiri: &str, tipe_kiri: &str, kanan: &str, tipe_kanan: &str, tambahan: Option<&str>,
) -> String {
    let ekor = tambahan.map(|t| format!(" — {}", t)).unwrap_or_default();
    // Nilainya DIPOTONG, tidak dihapus.
    format!("assert_eq gagal — kiri `{}` ({}), kanan `{}` ({}){}",
        crate::interpreter::galat_index::potong_nilai(kiri), tipe_kiri,
        crate::interpreter::galat_index::potong_nilai(kanan), tipe_kanan, ekor)
}

/// Kalimat untuk sebuah **nama telanjang** yang tidak dikenal.
pub fn padanan_bahasa_lain(nama: &str) -> Option<&'static str> {
    Some(match nama {
        "null" | "NULL" | "undefined" | "nothing" =>
            "di JRG namanya `nil`. Tapi kalau yang dimaksud \"mungkin tidak ada\", yang dipakai `Option`: `Option::None` dan `Option::Some(x)`",
        "true_" | "True" => "di JRG ditulis huruf kecil: `true`",
        "False" => "di JRG ditulis huruf kecil: `false`",
        "console" => "`console.log(...)` itu JavaScript. Di JRG mencetak pakai `wok(...)`",
        "println" | "printf" | "puts" | "echo" | "print" | "printf_s" =>
            "di JRG mencetak pakai `wok(...)` — `wok_` tanpa baris baru, `eprint` ke aliran galat",
        "System" => "`System.out.println(...)` itu Java. Di JRG mencetak pakai `wok(...)`",
        "elif" => "di JRG ditulis `else if`, dua kata",
        "and" => "di JRG operator DAN ditulis `&&`",
        "or" => "di JRG operator ATAU ditulis `||`",
        "not" => "di JRG operator BUKAN ditulis `!`, di depan nilainya",
        "elsif" | "elseif" => "di JRG ditulis `else if`, dua kata",
        "func" | "function" | "def" | "sub" | "proc" =>
            "di JRG sebuah fungsi ditulis `fn nama(a, b) { ... }`",
        "lambda" => "di JRG sebuah closure ditulis `|x| x * 2`",
        "self_" | "this" => "di JRG penerimanya bernama `self`, dan cuma berarti di dalam `impl`",
        "new" => "di JRG tidak ada `new`: sebuah struct dibuat dengan `Nama { field: nilai }`",
        "len_" | "length" | "size" | "count" =>
            "di JRG panjangnya `len(x)`, atau `x.len()`",
        "append" | "push_back" => "di JRG menambah isi array pakai `a = arr::push(a, x)`",
        "input" | "gets" | "scanf" | "readline" =>
            "di JRG membaca satu baris pakai `fmt::tok()`",
        "range_" | "xrange" => "di JRG ditulis `range(awal, akhir)`",
        "nullptr" => "di JRG tidak ada penunjuk kosong; yang setara `nil`, dan \"mungkin tidak ada\" diwakili `Option`",
        _ => return None,
        })
    }

pub fn pesan_nama_tak_dikenal(nama: &str, saran: Option<&str>) -> String {
    // Kata dari bahasa lain dijawab padanannya, bukan tebakan ejaan: yang
    // menulis `null` tidak salah ketik, ia cuma belum tahu nama JRG-nya.
    if let Some(padanan) = padanan_bahasa_lain(nama) {
        return format!("`{}` bukan nama di JRG — {}", nama, padanan);
    }
    match saran {
        Some(s) => format!(
            "variabel `{}` belum dikenal — maksudnya `{}`? kalau bukan, cek lagi \
             penulisannya atau jangan-jangan lupa `let {} = ...` dulu", nama, s, nama),
        None => format!(
            "variabel `{}` belum dikenal — cek lagi penulisannya, atau jangan-jangan \
             lupa `let {} = ...` dulu sebelum dipakai", nama, nama),
    }
}

/// Kalimat untuk sebuah **path** (`ns::nama`) yang tidak dikenal.
/// Lihat [`pesan_nama_tak_dikenal`].
pub fn pesan_path_tak_dikenal(path: &str, saran: Option<&str>) -> String {
    pesan_path_tak_dikenal_plus(path, saran, &[], false)
}

/// Sama, plus nama yang SAMA PERSIS di namespace lain.
pub fn pesan_path_tak_dikenal_plus(
    path: &str, saran: Option<&str>, di_tempat_lain: &[String],
    ekor_ada_telanjang: bool,
) -> String {
    // Ejaan yang mirip DIDAHULUKAN kalau ada: `jawa::free` salah di bagian
    // NAMESPACE-nya, dan `jawir::free` menjawab persis itu.
    if let Some((ns, ekor)) = path.rsplit_once("::") {
        // Nama telanjangnya ADA: itu jawaban yang paling berguna, karena ia
        // menyebutkan bentuk yang benar-benar bisa ditulis.
        if ekor_ada_telanjang {
            return format!(
                "`{}` tidak ditemukan — `{}` memang ada, tapi bukan sebagai isi `{}`. \
                 Tulis `{}` saja kalau memang itu yang dimaksud",
                path, ekor, ns, ekor);
        }
        if saran.is_none() && !di_tempat_lain.is_empty() {
            let daftar = di_tempat_lain.iter()
                .map(|s| format!("`{}`", s)).collect::<Vec<_>>().join(" atau ");
            return format!(
                "`{}` tidak ditemukan — `{}` memang ada, tapi sebagai {}",
                path, ekor, daftar);
        }
    }
    match saran {
        Some(s) => format!("`{}` tidak ditemukan — maksudnya `{}`?", path, s),
        None    => format!("`{}` tidak ditemukan — cek lagi penulisan namespace/nama-nya", path),
    }
}

pub fn float_truthy(f: f64) -> bool {
    f != 0.0 && !f.is_nan()
}

/// `terap_bernama(f, posisional, nama, nilai)` dipanggil langsung dengan bentuk salah:
/// daftar yang bukan array dulu dibaca KOSONG, dan nilai yang kurang diisi `nil`.
pub fn pesan_terap_bernama(masalah: &str) -> String {
    format!("terap_bernama: {} — bentuknya `terap_bernama(f, [posisional], [\"nama\"], [nilai])`, \
             mesin di balik `f(a, b: 2)`", masalah)
}

/// Builds `Result::Ok(value)`.
pub fn susun_argumen_bernama(
    nama_param: &[String],
    jumlah_posisional: usize,
    nama_diberi: &[String],
) -> Result<(Vec<usize>, usize), String> {
    let mut posisi = Vec::with_capacity(nama_diberi.len());
    let mut terisi: Vec<bool> = vec![false; nama_param.len().max(jumlah_posisional)];
    for i in 0..jumlah_posisional.min(terisi.len()) { terisi[i] = true; }
    if jumlah_posisional > nama_param.len() {
        return Err(format!(
            "diberi {} argumen posisional, sementara fungsinya cuma punya {} parameter",
            jumlah_posisional, nama_param.len()));
    }
    for n in nama_diberi {
        let Some(idx) = nama_param.iter().position(|p| p == n) else {
            return Err(format!(
                "tidak ada parameter bernama `{}` — yang ada: {}",
                n,
                if nama_param.is_empty() { "(fungsinya tanpa parameter)".to_string() }
                else { nama_param.join(", ") }));
        };
        if idx < jumlah_posisional {
            return Err(format!(
                "parameter `{}` sudah diisi lewat posisi, jadi `{}:` mengisinya dua kali",
                n, n));
        }
        if terisi[idx] {
            return Err(format!("parameter `{}` diberi dua kali", n));
        }
        terisi[idx] = true;
        posisi.push(idx);
    }
    let panjang = terisi.iter().rposition(|x| *x).map_or(0, |i| i + 1);
    for i in 0..panjang {
        if !terisi[i] {
            return Err(format!(
                "parameter `{}` dilewati — argumen bernama tidak bisa melompati sebuah \
                 parameter, karena nilai defaultnya dihitung di dalam fungsinya dan tidak \
                 ada yang bisa mengisinya dari luar", nama_param[i]));
        }
    }
    Ok((posisi, panjang))
}

pub fn ok_value(v: Value) -> Value {
    Value::Enum { type_name: "Result".to_string(), variant: "Ok".to_string(), payload: Some(Box::new(v)) }
}

/// Builds `Result::Err(message)` from any message-like value.
pub fn err_value(msg: impl Into<String>) -> Value {
    Value::Enum {
        type_name: "Result".to_string(),
        variant: "Err".to_string(),
        payload: Some(Box::new(Value::Str(msg.into()))),
    }
}

/// Builds `Option::Some(value)`.
pub fn some_value(v: Value) -> Value {
    Value::Enum { type_name: "Option".to_string(), variant: "Some".to_string(), payload: Some(Box::new(v)) }
}

/// Builds `Option::None`.
pub fn none_value() -> Value {
    Value::Enum { type_name: "Option".to_string(), variant: "None".to_string(), payload: None }
}

/// Extracts just the payload from an owned `Value::Enum`, discarding
/// `type_name`/`variant` — used by `Result`/`Option` methods (`unwrap`, `ok` …
pub(crate) fn enum_payload(v: Value) -> Option<Value> {
    match v {
        Value::Enum { payload, .. } => payload.map(|b| *b),
        _ => unreachable!("enum_payload dipanggil pada non-enum"),
    }
}

/// Like `enum_payload`, but also returns the owned `type_name`/`variant`
/// — used by methods (`map`, `map_err`) that need to rebuild a new
/// `Value::Enum` carrying the same tag but a transformed payload.
pub(crate) fn enum_parts(v: Value) -> (String, String, Option<Value>) {
    match v {
        Value::Enum { type_name, variant, payload } => (type_name, variant, payload.map(|b| *b)),
        _ => unreachable!("enum_parts dipanggil pada non-enum"),
    }
}

/// Fisher–Yates shuffle memakai `crypto::secure_random_bytes` (OS RNG lewat
/// `/dev/urandom` bila ada) sebagai sumber keacakan — dipakai oleh …

#[derive(Debug, Clone)]
pub struct JrgFunction {
    pub name:   String,
    /// `Arc` di empat field ini: menyebut nama fungsi menyalin `JrgFunction`, dan dulu itu
    /// berarti menyalin seluruh AST badannya di TIAP panggilan (Fase PANGGIL-SALIN).
    pub params: std::sync::Arc<Vec<String>>,
    /// Nilai default per-parameter, sejajar index dengan `params` (`defaults[i]`
    /// adalah default untuk `params[i]`, `None` kalau parameter itu wajib diisi).
    pub defaults: std::sync::Arc<Vec<Option<Expr>>>,
    /// Parameter TERAKHIR mengumpulkan sisa argumen jadi sebuah array
    /// (`fn f(a, sisa...)`). Lihat `FnParam::is_variadic`.
    pub variadic: bool,
    pub body:   std::sync::Arc<FnBody>,
    pub env:    std::sync::Arc<Env>,
    /// Semua `fn` bernama yang dideklarasikan di BLOK YANG SAMA dengan fungsi ini
    /// (termasuk dirinya sendiri), atau `None` untuk fungsi top-level.
    pub group:  Option<std::sync::Arc<Vec<crate::parser::FnDecl>>>,
    /// Tipe pemilik method ini — diisi HANYA waktu method dicapai lewat bentuk
    /// path (`P::tambah`).
    pub pemilik: Option<String>,
    /// Tipe yang "di dalamnya" badan ini berjalan, untuk aturan field privat — LEKSIKAL,
    /// sama dengan `Chunk::tipe_self` di VM. `None` untuk fungsi biasa.
    pub ruang: Option<std::sync::Arc<String>>,
}


#[derive(Debug)]
pub enum Signal {
    Return(Value),
    Break(Option<String>),
    Continue(Option<String>),
    RuntimeError(RuntimeError),
}

impl fmt::Display for Signal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Signal::Return(v)       => write!(f, "return {}", v),
            Signal::Break(l)        => write!(f, "break{}", l.as_deref().map(|s| format!(" '{}", s)).unwrap_or_default()),
            Signal::Continue(l)     => write!(f, "continue{}", l.as_deref().map(|s| format!(" '{}", s)).unwrap_or_default()),
            Signal::RuntimeError(e) => write!(f, "runtime error: {}", e),
        }
    }
}

#[derive(Debug)]
pub struct RuntimeError {
    pub message: String,
    /// Posisi di sumber, kalau diketahui.
    pub span: Option<crate::lexer::Span>,
    /// Rantai pemanggilan menuju galat ini, TERDALAM lebih dulu.
    pub jejak: Vec<crate::diag::BingkaiJejak>,
    /// Bingkai yang tidak disimpan karena melewati batas — lihat
    /// [`crate::diag::MAKS_BINGKAI`].
    pub jejak_sisa: usize,
}

impl RuntimeError {
    pub fn new(msg: impl Into<String>) -> Self {
        RuntimeError { message: crate::ruang_tipe::bersihkan(msg.into()), span: None, jejak: Vec::new(), jejak_sisa: 0 }
    }

    /// Catat bahwa galat ini keluar dari badan `nama`.
    pub fn tambah_bingkai(mut self, nama: impl Into<String>) -> Self {
        // Yang disimpan bingkai TERDALAM — yang didorong duluan, karena
        // galat merambat dari dalam ke luar. Itu yang paling dekat dengan
        // penyebab, jadi kalau ada yang harus dibuang, yang terluar.
        if self.jejak.len() < crate::diag::MAKS_BINGKAI {
            self.jejak.push(crate::diag::BingkaiJejak::baru(nama, None));
        } else {
            self.jejak_sisa += 1;
        }
        self
    }

    /// Isi span bingkai terakhir kalau masih kosong.
    pub fn isi_span_bingkai(mut self, span: crate::lexer::Span) -> Self {
        // Sesudah batas tercapai, bingkai terakhir yang tersimpan sudah punya
        // span-nya sendiri dan yang lewat di sini adalah pemanggil yang lebih luar — …
        if self.jejak_sisa == 0 {
            if let Some(fr) = self.jejak.last_mut() {
                if fr.span.is_none() { fr.span = Some(span); }
            }
        }
        self
    }

    /// Isi span kalau belum ada. Sengaja tidak menimpa span yang sudah
    /// terisi supaya lokasi terdalam (yang paling menunjuk ke penyebab)
    /// tidak tertimpa oleh ekspresi pembungkusnya saat error merambat naik.
    pub fn fill_span(mut self, span: crate::lexer::Span) -> Self {
        if self.span.is_none() {
            self.span = Some(span);
        }
        self
    }

    /// Pinjam lokasi dari error lain — dipakai saat sebuah error dibungkus
    /// ulang dengan pesan tambahan tapi tetap menunjuk tempat yang sama.
    pub fn fill_span_from(mut self, lain: &RuntimeError) -> Self {
        if self.span.is_none() {
            self.span = lain.span;
        }
        self
    }
}
impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{}", self.message) }
}

#[macro_export]
macro_rules! runtime_err {
    ($($a:tt)*) => {
        Err($crate::interpreter::Signal::RuntimeError(
            $crate::interpreter::RuntimeError::new(format!($($a)*))
        ))
    };

}

pub type EvalResult = Result<Value, Signal>;

#[cfg(test)]
mod tes_arg_metode {
    use super::*;

    /// Minimum dari `bentuk_metode` tidak boleh melebihi maksimum dari tabel
    /// aritas: kalau iya, pemanggilan yang BENAR ikut ditolak "paling banyak".
    #[test]
    fn minimum_tidak_melebihi_maksimum() {
        let mut diperiksa = 0;
        let mut pasangan: Vec<(String, String)> = Vec::new();
        for ns in ["arr", "str", "dict", "chan", "canvas", "atomic", "mutex"] {
            for m in calon_method(ns) { pasangan.push((ns.to_string(), m)); }
        }
        for ns in ["Option", "Result"] {
            for m in calon_method_enum(ns) { pasangan.push((ns.to_string(), m.to_string())); }
        }
        for m in calon_method_skalar("float") { pasangan.push(("math".to_string(), m.to_string())); }
        for (ns, m) in &pasangan {
            let min = bentuk_metode(ns, m).map(|(n, _)| n).unwrap_or(0);
            if let Some(maks) = maks_arg_metode(ns, m) {
                assert!(min <= maks, "`{}.{}`: minimum {} melebihi maksimum {}", ns, m, min, maks);
                diperiksa += 1;
            }
        }
        assert!(diperiksa > 150, "cuma {} method diperiksa", diperiksa);
    }
}
