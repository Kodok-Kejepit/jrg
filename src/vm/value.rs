// src/vm/value.rs — src/vm/value.rs  (Arc-based — thread-safe)

use std::sync::{Arc, Mutex};
use std::fmt;

use super::chunk::Chunk;

// ─── VmValue ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub enum VmValue {
    Nil,
    /// Hasil dari sesuatu yang memang tidak menghasilkan nilai: fungsi yang
    /// selesai tanpa `return`, `return` telanjang, blok tanpa ekspresi ekor, dan …
    Unit,
    Bool(bool),
    /// Integer 128 bit — SAMA LEBARNYA dengan interpreter.
    Int(i128),
    /// Satu karakter Unicode — tipe tersendiri, bukan string 1-karakter dan bukan
    /// integer.
    Rune(char),
    Float(f64),
    /// Array numerik `wokwok::` — buffer BERTIPE dan KONTIGU.
    Nd(std::sync::Arc<crate::wokwok::Nd>),
    /// Jendela/kanvas gambar.
    Canvas(crate::canvas::CanvasHandle),
    Str(Arc<String>),
    Array(Arc<Mutex<Vec<VmValue>>>),
    /// Penyimpanannya sama dengan array, supaya elemennya bisa ditulis lewat jalur `Pinjam*`.
    Tuple(Arc<Mutex<Vec<VmValue>>>),
    /// Dict, instance struct, dan enum — jenisnya di `Peta::label`, bukan di isinya.
    Map(Arc<Peta>),
    Function(Arc<VmFunction>),
    Closure(Arc<VmClosure>),
    NativeFn(VmNativeFn),
    Channel(super::channel::VmChannel),
    /// `Atomic`/`Mutex`: pegangan ke keadaan bersama (`crate::sinkron`).
    Sinkron(crate::sinkron::Pegangan),
    /// Handle ke objek milik worker `py::`/`js::`/`php::`.
    Asing(super::ffi_asing::Asing),

    /// Alamat memori NATIVE yang datang dari kode C.
    Ptr(usize),

    /// Fungsi `extern "C"` yang sudah diresolusi ke simbol nyata.
    ExternFn(Arc<crate::ffi::native::ExternFnInfo>),

    /// Sel binding bersama — satu-satunya alias yang disengaja di JRG.
    Cell(Arc<Mutex<VmValue>>),

    /// Konstruktor varian enum ber-payload sebagai NILAI (`Some`, `E::A`) —
    /// padanan `Value::EnumCtor`, jadi `arr::map(xs, Some)` jalan di dua mesin.
    Ktor(Arc<Ktor>),
}

/// Isi `VmValue::Ktor`: varian mana yang dibangun, dan berapa payload-nya.
#[derive(Debug)]
pub struct Ktor { pub nama: &'static NamaVarian, pub arity: usize }

pub type VmNativeFn = fn(&[VmValue]) -> Result<VmValue, String>;

/// Apakah `pesan` seluruhnya cuma sebuah TANDA TANGAN, tanpa kalimat?
fn hanya_tanda_tangan(pesan: &str) -> bool {
    let t = pesan.trim();
    // Sebuah tanda tangan tidak pernah berbaris banyak.
    if t.contains('\n') { return false; }
    // Buang ekor tipe kembalian kalau ada; sisanya harus berakhir `)`.
    let inti = match t.split_once(" -> ") { Some((kiri, _)) => kiri.trim(), None => t };
    if !inti.ends_with(')') { return false; }
    let Some(kurung) = inti.find('(') else { return false; };
    let kepala = &inti[..kurung];
    !kepala.is_empty()
        && kepala.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
}

/// Ubah sebuah pesan native yang isinya CUMA tanda tangan jadi kalimat yang
/// menyebutkan apa yang salah.
pub fn perjelas_pesan_native(pesan: String, args: &[VmValue]) -> String {
    // `type_name_penuh`, bukan `type_name` (protokol pola yang menjawab `map`).
    let tipe: Vec<String> = args.iter().map(|v| v.type_name_penuh()).collect();
    perjelas_pesan_native_tipe(pesan, &tipe)
}

/// Inti [`perjelas_pesan_native`], bekerja atas NAMA TIPE alih-alih nilai.
pub fn perjelas_pesan_native_tipe(pesan: String, tipe: &[String]) -> String {
    if !hanya_tanda_tangan(&pesan) { return pesan; }
    let nama = pesan.trim().split('(').next().unwrap_or("").trim();
    if tipe.is_empty() {
        return format!(
            "`{}` dipanggil tanpa argumen — bentuk yang diterima: {}",
            nama, pesan.trim());
    }
    format!(
        "argumen tidak cocok untuk `{}` — dapat ({}), bentuk yang diterima: {}",
        nama, tipe.join(", "), pesan.trim())
}


// ─── Upvalue ─────────────────────────────────────────────────────────────

/// Dari mana satu tangkapan closure diambil, ditentukan SAAT KOMPILASI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpvalSrc {
    /// Slot lokal (relatif ke frame) milik fungsi yang MEMBUAT closure ini.
    ParentLocal(u32),
    /// Tangkapan ke-i milik fungsi yang MEMBUAT closure ini — dipakai kalau
    /// pembuatnya sendiri sebuah closure dan nama itu datang dari lebih luar.
    ParentUpval(u32),
}

// ─── VmFunction ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct VmFunction {
    pub name:  String,
    pub arity: u8,
    pub chunk: Chunk,
    /// Rencana penangkapan: satu entri per tangkapan, urut sesuai indeks yang
    /// dipakai `LoadUpval`/`StoreUpval` di dalam badan fungsi ini.
    pub upvals: Vec<UpvalSrc>,
    /// Jumlah parameter yang WAJIB diisi (yang tanpa nilai default).
    pub arity_min: u8,
    /// Parameter TERAKHIR mengumpulkan sisa argumen jadi sebuah array
    /// (`fn f(a, sisa...)`). Lihat `FnParam::is_variadic`.
    pub variadic: bool,
    /// Nama tiap parameter, dipakai diagnostik "parameter `x` wajib diisi".
    pub params: Vec<String>,
    /// Ditulis `|x| ...` di sumbernya, bukan `fn`. Hanya memengaruhi
    /// `typeof` dan tampilan — interpreter membedakan keduanya
    /// (`"closure"` vs `"function"`), jadi VM harus ikut membedakan.
    pub is_closure: bool,
    /// Ditulis `move |x| ...`. Mengubah CARA menangkap, bukan apa yang
    /// ditangkap: tiap tangkapan jadi sel BARU berisi salinan, bukan sel
    /// bersama milik binding aslinya.
    pub is_move: bool,
}
impl VmFunction {
    pub fn new(name: impl Into<String>, arity: u8, chunk: Chunk) -> Self {
        VmFunction { name: name.into(), arity, chunk: chunk.rampungkan(), arity_min: arity,
                     params: Vec::new(), upvals: Vec::new(), is_closure: false, variadic: false,
                     is_move: false }
    }
    pub fn closure(
        name: impl Into<String>,
        arity: u8,
        chunk: Chunk,
        upvals: Vec<UpvalSrc>,
        is_move: bool,
    ) -> Self {
        VmFunction { name: name.into(), arity, chunk: chunk.rampungkan(), arity_min: arity,
                     params: Vec::new(), upvals, is_closure: true, variadic: false, is_move }
    }
}

// ─── VmClosure ───────────────────────────────────────────────────────────

/// Fungsi + binding yang ditangkapnya.
#[derive(Clone, Debug)]
pub struct VmClosure {
    pub func: Arc<VmFunction>,
    pub captures: Vec<Arc<Mutex<VmValue>>>,
}

impl VmClosure {
    pub fn new(func: Arc<VmFunction>, captures: Vec<Arc<Mutex<VmValue>>>) -> Self {
        VmClosure { func, captures }
    }
}

// ─── Peta: dict, struct, dan enum ─────────────────────────────────────────

/// Jenis sebuah `Map`, disimpan di LUAR isi dan di luar `Mutex`-nya: data pemakai
/// (literal, JSON, field bernama `variant`) tidak bisa memalsukannya, dan membacanya
/// tidak mengunci. Dulu jenisnya kunci biasa (`__type__`, `variant`, `__payload__`).
#[derive(Clone, Debug)]
pub enum Label {
    Dict,
    Struct(Arc<str>),
    Enum { nama: &'static NamaVarian, payload: Option<VmValue> },
}

/// Nama varian enum, diintern per proses: satu alamat per (tipe, varian), hidup selamanya. Menyalin atau
/// melepas sebuah enum jadi tidak menyentuh hitungan referensi nama, dan dua nama sama cukup dibanding alamatnya.
/// Terbatas oleh teks program (deklarasi, `Option`/`Result`, nama dari jembatan), jadi tidak tumbuh saat jalan.
#[derive(Debug)]
pub struct NamaVarian { pub tipe: Arc<str>, pub varian: Arc<str> }

impl NamaVarian {
    pub fn intern(tipe: &str, varian: &str) -> &'static NamaVarian {
        type Tabel = std::collections::HashMap<Box<str>, std::collections::HashMap<Box<str>, &'static NamaVarian>>;
        static TABEL: std::sync::OnceLock<Mutex<Tabel>> = std::sync::OnceLock::new();
        let mut t = TABEL.get_or_init(Default::default).lock().unwrap_or_else(|p| p.into_inner());
        if let Some(n) = t.get(tipe).and_then(|m| m.get(varian)) { return n; }
        let n: &'static NamaVarian = Box::leak(Box::new(NamaVarian { tipe: Arc::from(tipe), varian: Arc::from(varian) }));
        t.entry(tipe.into()).or_default().insert(varian.into(), n);
        n
    }
}

/// `Option`/`Result`: diintern sekali, dipakai tiap `Some(..)`/`Ok(..)` yang dibangun pustaka.
macro_rules! nama_tetap {
    ($tipe:literal, $varian:literal) => {{
        static N: std::sync::OnceLock<&'static NamaVarian> = std::sync::OnceLock::new();
        *N.get_or_init(|| NamaVarian::intern($tipe, $varian))
    }};
}

/// Hasher isi `Peta`: dict memakai SipHash berkunci acak (kuncinya bisa datang dari luar — HashDoS,
/// Fase AMAN-7); struct dan enum hash cepat, tapi cuma sampai `KUNCI_CEPAT` kunci (`json::merge` bisa
/// memberi struct kunci dari dict) — tabrakan sebanyak itu tidak berarti apa-apa.
#[derive(Clone, Debug)]
pub struct HashPeta(pub(crate) Option<std::collections::hash_map::RandomState>);

pub enum PengacakPeta {
    Sip(std::collections::hash_map::DefaultHasher),
    Cepat(u64),
}

impl std::hash::Hasher for PengacakPeta {
    #[inline(always)]
    fn write_u8(&mut self, x: u8) {
        match self { PengacakPeta::Sip(h) => h.write_u8(x), PengacakPeta::Cepat(s) => *s = (s.rotate_left(5) ^ x as u64).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95) }
    }
    #[inline(always)]
    fn write(&mut self, b: &[u8]) {
        match self {
            PengacakPeta::Sip(h) => h.write(b),
            // FxHash: dua instruksi per 8 byte.
            PengacakPeta::Cepat(s) => {
                let mut c = b.chunks_exact(8);
                for k in &mut c { *s = (s.rotate_left(5) ^ u64::from_le_bytes(k.try_into().unwrap())).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95); }
                for &x in c.remainder() { *s = (s.rotate_left(5) ^ x as u64).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95); }
            }
        }
    }
    #[inline(always)]
    fn finish(&self) -> u64 {
        match self { PengacakPeta::Sip(h) => h.finish(), PengacakPeta::Cepat(s) => *s }
    }
}

impl std::hash::BuildHasher for HashPeta {
    type Hasher = PengacakPeta;
    #[inline(always)]
    fn build_hasher(&self) -> PengacakPeta {
        match &self.0 { Some(r) => PengacakPeta::Sip(r.build_hasher()), None => PengacakPeta::Cepat(0) }
    }
}


/// Kunci `Peta`: teks bersama, jadi struct baru tidak menyalin nama field-nya (dibagi dengan konstanta chunk).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Kunci(pub Arc<String>);

impl std::borrow::Borrow<str> for Kunci { fn borrow(&self) -> &str { self.0.as_str() } }
impl std::borrow::Borrow<String> for Kunci { fn borrow(&self) -> &String { &self.0 } }
impl std::ops::Deref for Kunci { type Target = String; fn deref(&self) -> &String { &self.0 } }
impl std::fmt::Display for Kunci { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) } }
impl From<String> for Kunci { fn from(s: String) -> Self { Kunci(Arc::new(s)) } }
impl From<&str> for Kunci { fn from(s: &str) -> Self { Kunci(Arc::new(s.to_string())) } }
impl From<&String> for Kunci { fn from(s: &String) -> Self { Kunci(Arc::new(s.clone())) } }
impl From<Kunci> for String { fn from(k: Kunci) -> String { Arc::try_unwrap(k.0).unwrap_or_else(|a| (*a).clone()) } }
impl Kunci {
    /// Kunci dari nilai indeks: teks dipakai bersama, yang lain bentuk tampilnya (aturan kunci dict).
    pub fn dari_nilai(v: &VmValue) -> Kunci {
        match v { VmValue::Str(s) => Kunci(s.clone()), lain => format!("{}", lain).into() }
    }
}

impl PartialEq<str> for Kunci { fn eq(&self, o: &str) -> bool { self.0.as_str() == o } }
impl PartialEq<String> for Kunci { fn eq(&self, o: &String) -> bool { *self.0 == *o } }

/// Isi sebuah `Peta`.
pub use super::peta::IsiPeta;
use super::peta::{Bentuk, RAPAT_MAKS};

/// Isi sebuah `Map` plus labelnya. Isinya cuma field atau key milik program.
#[derive(Debug)]
pub struct Peta {
    pub label: Label,
    isi: Mutex<IsiPeta>,
}

impl Peta {
    pub fn baru<K: Into<Kunci>>(label: Label, isi: impl IntoIterator<Item = (K, VmValue)>) -> Self {
        let it = isi.into_iter();
        let n = it.size_hint().0;
        Self::baru_n(label, n, it)
    }

    /// Dict: tabel SipHash. Struct/enum sampai `RAPAT_MAKS` field tanpa nama kembar: bentuk rapat; selebihnya
    /// tabel ber-hash cepat yang pindah ke SipHash di atas `KUNCI_CEPAT` (`IsiPeta::insert`).
    pub fn baru_n<K: Into<Kunci>>(label: Label, n: usize, isi: impl IntoIterator<Item = (K, VmValue)>) -> Self {
        let dict = matches!(label, Label::Dict);
        let isi = isi.into_iter().map(|(k, v)| (k.into(), v));
        if dict {
            let mut peta = IsiPeta::tabel(n, true);
            peta.extend(isi);
            return Peta { label, isi: Mutex::new(peta) };
        }
        let (nama, nilai): (Vec<Kunci>, Vec<VmValue>) = isi.unzip();
        // Isi kosong (varian enum, `S {}` tanpa field): tabel kosong tidak mengalokasi apa pun.
        let peta = if !nama.is_empty() && nama.len() <= RAPAT_MAKS && Bentuk::unik(&nama) {
            IsiPeta::rapat(Bentuk::baru(nama), nilai)
        } else {
            let mut t = IsiPeta::tabel(nama.len(), false);
            t.extend(nama.into_iter().zip(nilai));
            t
        };
        Peta { label, isi: Mutex::new(peta) }
    }

    /// Isi kosong sesuai label (dict: SipHash).
    pub fn isi_untuk(label: &Label, n: usize) -> IsiPeta { IsiPeta::tabel(n, matches!(label, Label::Dict)) }

    /// Dari isi yang sudah jadi (salinan peta lain); dict selalu ber-SipHash.
    pub fn dari_isi(label: Label, isi: IsiPeta) -> Self {
        if matches!(label, Label::Dict) && !isi.pakai_sip() {
            let mut baru = IsiPeta::tabel(isi.len(), true);
            baru.extend(isi);
            return Peta { label, isi: Mutex::new(baru) };
        }
        Peta { label, isi: Mutex::new(isi) }
    }

    /// Prototipe literal: nama field berurutan (juga untuk dict — cuma pembawa urutan, tidak pernah dibaca).
    pub fn prototipe(label: Label, nama: Vec<Kunci>) -> Self {
        let n = nama.len();
        Peta { label, isi: Mutex::new(IsiPeta::rapat(Bentuk::intern(nama), vec![VmValue::Nil; n])) }
    }

    /// Struct dari literal (`BuildStruct`): nilai field berurutan menurut bentuk prototipenya (`bentuk()`),
    /// yang dibagi; dict dan nama kembar lewat jalur tabel.
    pub fn struktur_dari(label: Label, bentuk: Arc<Bentuk>, nilai: Vec<VmValue>) -> Peta {
        if bentuk.unik && !matches!(label, Label::Dict) {
            Peta::dari_isi(label, IsiPeta::rapat(bentuk, nilai))
        } else {
            let n = nilai.len();
            Peta::baru_n(label, n, bentuk.nama.iter().cloned().zip(nilai))
        }
    }

    /// `struktur_dari` langsung di dalam `Arc`-nya: `Peta` (±150 byte) tidak dibangun di stack lalu disalin.
    #[inline]
    pub fn struktur_arc(label: Label, bentuk: Arc<Bentuk>, nilai: Vec<VmValue>) -> Arc<Peta> {
        if bentuk.unik && !matches!(label, Label::Dict) {
            Arc::new(Peta { label, isi: Mutex::new(IsiPeta::Rapat { bentuk, nilai }) })
        } else {
            Arc::new(Peta::struktur_dari(label, bentuk, nilai))
        }
    }

    /// Bentuk rapat isi ini, kalau rapat.
    pub fn bentuk(&self) -> Option<Arc<Bentuk>> {
        match &*self.isi.lock().unwrap_or_else(|p| p.into_inner()) {
            IsiPeta::Rapat { bentuk, .. } => Some(bentuk.clone()),
            IsiPeta::Hash(_) => None,
        }
    }

    pub fn sisip(&self, k: impl Into<Kunci>, v: VmValue) {
        self.isi.lock().unwrap_or_else(|p| p.into_inner()).insert(k.into(), v);
    }

    pub fn lock(&self) -> std::sync::LockResult<std::sync::MutexGuard<'_, IsiPeta>> {
        self.isi.lock()
    }

    /// Isi peta milik tunggal pemegang ini, tanpa kunci (`tunggal`).
    #[inline]
    pub fn isi_tunggal(m: &mut Arc<Peta>) -> Option<&mut IsiPeta> {
        tunggal(m).map(|p| p.isi.get_mut().unwrap_or_else(|e| e.into_inner()))
    }

    /// Nama tipe untuk mencari method (`P::m`, `Option::m`); `None` untuk dict.
    pub fn nama_tipe(&self) -> Option<&str> {
        match &self.label {
            Label::Dict => None,
            Label::Struct(n) => Some(n.as_ref()),
            Label::Enum { nama, .. } => Some(nama.tipe.as_ref()),
        }
    }
}

/// Isi penyimpanan yang tidak dipegang `Arc` lain (kuat maupun lemah), tanpa operasi atomik tulis-baca:
/// pemegang `&mut` ini satu-satunya jalan ke sana, jadi thread lain tidak bisa menjangkaunya. Pagar Acquire
/// menyusul pelepasan pemegang terakhir sebelumnya. Satu definisi untuk larik dan peta.
#[inline]
pub fn tunggal<T>(a: &mut Arc<T>) -> Option<&mut T> {
    if Arc::strong_count(a) != 1 || Arc::weak_count(a) != 0 { return None; }
    std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
    // SAFETY: tidak ada `Arc` lain ke isinya dan `a` dipinjam eksklusif selama hasil ini hidup.
    Some(unsafe { &mut *(Arc::as_ptr(a) as *mut T) })
}

// ─── Constructors ─────────────────────────────────────────────────────────

/// Potret isi sebuah tuple. Kuncinya dilepas sebelum isinya dipakai, jadi dua tuple yang
/// berbagi penyimpanan tidak pernah saling menunggu (kelas `x < x` di Fase URUT).
pub fn isi_tuple(t: &Arc<Mutex<Vec<VmValue>>>) -> Vec<VmValue> {
    t.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

impl VmValue {
    /// Salinan yang benar-benar TERPISAH dari aslinya.
    pub fn deep_copy(&self) -> VmValue {
        match self {
            VmValue::Array(a) => {
                let isi = a.lock().unwrap_or_else(|p| p.into_inner());
                VmValue::array(isi.iter().map(|v| v.deep_copy()).collect())
            }
            VmValue::Map(m) => {
                let isi: Vec<(Kunci, VmValue)> = m.lock().unwrap_or_else(|p| p.into_inner())
                    .iter().map(|(k, v)| (k.clone(), v.deep_copy())).collect();
                let label = match &m.label {
                    Label::Enum { nama, payload } => Label::Enum {
                        nama, payload: payload.as_ref().map(|p| p.deep_copy()),
                    },
                    lain => lain.clone(),
                };
                VmValue::Map(Arc::new(Peta::baru(label, isi)))
            }
            VmValue::Tuple(t) => VmValue::tuple(isi_tuple(t).iter().map(|v| v.deep_copy()).collect()),
            // Sel binding tidak pernah sampai ke sini lewat jalur normal (`LoadLocal`
            // sudah men-deref-nya).
            VmValue::Cell(c) => c.lock().unwrap_or_else(|p| p.into_inner()).deep_copy(),
            lain => lain.clone(),
        }
    }

    /// Apakah nilai ini punya penyimpanan yang bisa beralias?
    pub fn bisa_beralias(&self) -> bool {
        matches!(self, VmValue::Array(_) | VmValue::Map(_) | VmValue::Tuple(_) | VmValue::Cell(_))
    }

    /// Pastikan penyimpanan nilai ini cuma dimiliki DIA SENDIRI, dengan
    /// memisahkannya lebih dulu kalau ternyata masih dipegang bersama.
    pub fn pisahkan(&mut self) {
        match self {
            VmValue::Array(a) if Arc::strong_count(a) > 1 => {
                let isi = a.lock().unwrap_or_else(|p| p.into_inner()).clone();
                *self = VmValue::array(isi);
            }
            VmValue::Map(m) if Arc::strong_count(m) > 1 => {
                let isi = m.lock().unwrap_or_else(|p| p.into_inner()).clone();
                let label = m.label.clone();
                *self = VmValue::Map(Arc::new(Peta::baru(label, isi)));
            }
            VmValue::Tuple(t) if Arc::strong_count(t) > 1 => {
                let isi = isi_tuple(t);
                *self = VmValue::tuple(isi);
            }
            // Skalar/teks/fungsi tidak punya penyimpanan yang bisa ditulis di tempat;
            // `Cell` justru HARUS tetap dibagi — ia satu-satunya alias yang disengaja …
            _ => {}
        }
    }

    pub fn str(s: impl Into<String>) -> Self { VmValue::Str(Arc::new(s.into())) }
    pub fn array(v: Vec<VmValue>)    -> Self { VmValue::Array(Arc::new(Mutex::new(v))) }
    pub fn tuple(v: Vec<VmValue>)    -> Self { VmValue::Tuple(Arc::new(Mutex::new(v))) }
    pub fn function(f: VmFunction)   -> Self { VmValue::Function(Arc::new(f)) }
    /// Dict biasa.
    pub fn map<K: Into<Kunci>>(m: impl IntoIterator<Item = (K, VmValue)>) -> Self {
        VmValue::Map(Arc::new(Peta::baru(Label::Dict, m)))
    }
    /// Nama struct milik program, diintern per proses saat kompilasi: semua prototipe tipe yang sama (lintas
    /// fungsi dan modul) berbagi satu `Arc`, jadi label dua nilai sebentuk sama menurut `Arc::ptr_eq`.
    pub fn intern_nama_struct(nama: &str) -> Arc<str> {
        static TABEL: std::sync::OnceLock<Mutex<std::collections::HashMap<Box<str>, Arc<str>>>> = std::sync::OnceLock::new();
        let mut t = TABEL.get_or_init(Default::default).lock().unwrap_or_else(|p| p.into_inner());
        t.entry(nama.into()).or_insert_with(|| Arc::from(nama)).clone()
    }
    /// Instance struct `nama`, termasuk handle bawaan (`Ptr`, `Future`, `Regex`, …).
    pub fn struktur<K: Into<Kunci>>(nama: &str, isi: impl IntoIterator<Item = (K, VmValue)>) -> Self {
        VmValue::Map(Arc::new(Peta::baru(Label::Struct(Arc::from(nama)), isi)))
    }
    /// Varian enum `tipe::varian`, dengan payload kalau ada.
    pub fn varian(tipe: &str, varian: &str, payload: Option<VmValue>) -> Self {
        VmValue::varian_nama(NamaVarian::intern(tipe, varian), payload)
    }
    /// Varian dari nama yang sudah diintern (prototipe konstanta, `Option`/`Result` tetap).
    #[inline(always)]
    pub fn varian_nama(nama: &'static NamaVarian, payload: Option<VmValue>) -> Self {
        // Dibangun langsung di dalam `Arc`-nya (isi kosong = tabel tanpa alokasi), tanpa `Peta` antara yang disalin.
        VmValue::Map(Arc::new(Peta { label: Label::Enum { nama, payload }, isi: Mutex::new(IsiPeta::tabel(0, false)) }))
    }
    /// Varian berpayload dari nama milik prototipenya (`BuildEnum`, juga dipakai JIT).
    #[inline(always)]
    pub fn varian_berisi(nama: &'static NamaVarian, payload: VmValue) -> Self {
        VmValue::varian_nama(nama, Some(payload))
    }
    /// `(tipe, varian, payload)` kalau nilai ini enum.
    pub fn bagian_enum(&self) -> Option<(&str, &str, Option<&VmValue>)> {
        match self {
            VmValue::Map(m) => match &m.label {
                Label::Enum { nama, payload } =>
                    Some((nama.tipe.as_ref(), nama.varian.as_ref(), payload.as_ref())),
                _ => None,
            },
            _ => None,
        }
    }
    /// Nama struct kalau nilai ini instance struct — bukan dict, bukan enum.
    pub fn nama_struct(&self) -> Option<&str> {
        match self {
            VmValue::Map(m) => match &m.label { Label::Struct(n) => Some(n.as_ref()), _ => None },
            _ => None,
        }
    }
    /// Nama tipe untuk mencari method, bagi struct maupun enum.
    pub fn nama_tipe(&self) -> Option<&str> {
        match self { VmValue::Map(m) => m.nama_tipe(), _ => None }
    }
}

// ─── Truthiness / type_name ───────────────────────────────────────────────

impl VmValue {
    #[inline]
    pub fn is_truthy(&self) -> bool {
        match self {
            VmValue::Nil         => false,
            VmValue::Unit        => false,
            VmValue::Bool(b)     => *b,
            VmValue::Int(n)      => *n != 0,
            // Rune tidak pernah kosong (teks kosong yang falsy), jadi selalu truthy — seperti acuan.
            VmValue::Rune(_)     => true,
            VmValue::Ptr(a)      => *a != 0,
            VmValue::ExternFn(_) => true,
            // NaN itu FALSY.
            VmValue::Float(f)    => crate::interpreter::float_truthy(*f),
            VmValue::Str(s)      => !s.is_empty(),
            VmValue::Array(a)    => !a.lock().unwrap().is_empty(),
            // Array numerik kosong itu falsy, sama seperti array biasa
            // kosong — bukan "ada objeknya, jadi true".
            VmValue::Nd(a)       => a.size() != 0,
            VmValue::Canvas(_)   => true,
            VmValue::Tuple(t)    => !t.lock().unwrap_or_else(|p| p.into_inner()).is_empty(),
            // Dict kosong falsy; struct dan enum truthy — dulu kunci labelnya sendiri
            // yang membuat petanya tidak pernah kosong.
            VmValue::Map(m)      => match m.label {
                Label::Dict => !m.lock().unwrap().is_empty(),
                _ => true,
            },
            VmValue::Cell(c)     => c.lock().unwrap_or_else(|p| p.into_inner()).is_truthy(),
            VmValue::Function(_) | VmValue::Closure(_) | VmValue::NativeFn(_) | VmValue::Channel(_)
            | VmValue::Ktor(_) | VmValue::Sinkron(_) | VmValue::Asing(_) => true,
        }
    }

    /// Apakah nilai ini bisa DIPANGGIL — satu jawaban untuk tiap pintu yang menerima
    /// fungsi (`terap`, HOF, handler route, `Future::new`), padanan `Value::bisa_dipanggil`.
    pub fn bisa_dipanggil(&self) -> bool {
        matches!(self, VmValue::Function(_) | VmValue::Closure(_) | VmValue::NativeFn(_)
                     | VmValue::ExternFn(_) | VmValue::Ktor(_)
                     | VmValue::Asing(crate::vm::ffi_asing::Asing::Fungsi(_)))
    }

    /// Konstruktor varian `tipe::varian` ber-payload `arity` sebagai nilai.
    pub fn ktor(tipe: &str, varian: &str, arity: usize) -> Self {
        VmValue::Ktor(Arc::new(Ktor { nama: NamaVarian::intern(tipe, varian), arity }))
    }

    /// `Option::Some(v)` — SATU definisi; dulu ada lima salinan berkunci teks.
    pub fn some(v: VmValue) -> VmValue { VmValue::varian_nama(nama_tetap!("Option", "Some"), Some(v)) }

    /// `Option::None` sebagai `VmValue`.
    pub fn none() -> VmValue { VmValue::varian_nama(nama_tetap!("Option", "None"), None) }

    /// Nama tipe LENGKAP seperti yang dijawab `typeof` di interpreter.
    pub fn type_name_penuh(&self) -> String {
        match self {
            VmValue::Map(m) => match &m.label {
                Label::Dict => "dict".to_string(),
                Label::Struct(n) => format!("struct:{}", n),
                Label::Enum { nama, .. } => format!("enum:{}", nama.tipe),
            },
            VmValue::Ktor(k) => format!("enum_ctor:{}::{}", k.nama.tipe, k.nama.varian),
            lain => lain.type_name().to_string(),
        }
    }

    /// # PERINGATAN: nilainya MENOPANG PENCOCOKAN POLA
    pub fn type_name(&self) -> &'static str {
        match self {
            VmValue::Nil         => "nil",
            VmValue::Unit        => "unit",
            VmValue::Bool(_)     => "bool",
            VmValue::Int(_)      => "int",
            VmValue::Rune(_)     => "rune",
            VmValue::Ptr(_)      => "ptr",
            VmValue::ExternFn(_) => "extern_fn",
            VmValue::Float(_)    => "float",
            VmValue::Str(_)      => "string",
            // Tiga konstanta, bukan `format!`: fungsi ini menjawab `&'static str`.
            VmValue::Canvas(_) => "canvas",
            VmValue::Nd(a) => match a.dtype() {
                "f64" => "wokwok:f64",
                "i64" => "wokwok:i64",
                _     => "wokwok:bool",
            },
            VmValue::Array(_)    => "array",
            VmValue::Tuple(_)    => "tuple",
            VmValue::Map(_)      => "map",
            // Interpreter membedakan `fn` dari `|x| ...` (`typeof` menjawab "function"
            // vs "closure").
            VmValue::Function(f) => if f.is_closure { "closure" } else { "function" },
            VmValue::Closure(_)  => "closure",
            VmValue::NativeFn(_) => "native_fn",
            VmValue::Channel(_)  => "channel",
            VmValue::Sinkron(p)  => p.nama_tipe(),
            VmValue::Asing(a)    => super::ffi_asing::nama_tipe_statis(a),
            VmValue::Ktor(_)     => "enum_ctor",
            // Tidak pernah terlihat program; kalau bocor, tipe yang benar
            // adalah tipe NILAI di dalam binding, bukan "cell".
            VmValue::Cell(c)     => c.lock().unwrap_or_else(|p| p.into_inner()).type_name(),
        }
    }
}

// ─── Arithmetic ───────────────────────────────────────────────────────────

/// Luapan integer 128 bit — kalimatnya milik bersama dengan interpreter.
fn luber(op: &str, x: i128, y: i128) -> String {
    crate::interpreter::pesan_luber(op, x, y)
}

impl VmValue {
    /// Seperti `add`, tapi MEMILIKI sisi kirinya.
    pub fn tambah_milik(self, rhs: &VmValue) -> Result<VmValue, String> {
        if let (VmValue::Array(a), VmValue::Array(b)) = (&self, rhs) {
            if Arc::strong_count(a) == 1 {
                let tambahan: Vec<VmValue> =
                    b.lock().unwrap_or_else(|p| p.into_inner()).iter().cloned().collect();
                a.lock().unwrap_or_else(|p| p.into_inner()).extend(tambahan);
                return Ok(self);
            }
        }
        // Teks di kiri: sama dengan cabang `(Str, _)` milik `add`, tapi menyambung di
        // tempat kalau Arc-nya unik — `make_mut` menyalin cuma kalau dipegang bersama.
        if let VmValue::Str(mut a) = self {
            use std::fmt::Write;
            // Tampilan teks adalah isinya sendiri: disalin langsung, tanpa mesin `fmt`.
            match rhs {
                VmValue::Str(r) => Arc::make_mut(&mut a).push_str(r),
                _ => write!(Arc::make_mut(&mut a), "{}", rhs).expect("Display VmValue tidak boleh gagal"),
            }
            return Ok(VmValue::Str(a));
        }
        self.add(rhs)
    }

    pub fn add(&self, rhs: &VmValue) -> Result<VmValue, String> {
        match (self, rhs) {
            (VmValue::Int(a),   VmValue::Int(b))   =>
                a.checked_add(*b).map(VmValue::Int).ok_or_else(|| luber("+", *a, *b)),
            (VmValue::Float(a), VmValue::Float(b)) => Ok(VmValue::Float(a + b)),
            (VmValue::Int(a),   VmValue::Float(b)) => Ok(VmValue::Float(*a as f64 + b)),
            (VmValue::Float(a), VmValue::Int(b))   => Ok(VmValue::Float(a + *b as f64)),
            // Array + array = gabung (fitur interpreter sejak Phase 5C).
            (VmValue::Array(a), VmValue::Array(b)) => {
                let mut isi = a.lock().unwrap_or_else(|p| p.into_inner()).clone();
                isi.extend(b.lock().unwrap_or_else(|p| p.into_inner()).iter().cloned());
                Ok(VmValue::array(isi))
            }
            // Kalau salah satu sisi teks, hasilnya teks — sama seperti
            // interpreter (`"a" + 1` → `a1`, `1 + "a"` → `1a`).
            (VmValue::Str(_), _) | (_, VmValue::Str(_)) =>
                Ok(VmValue::str(format!("{}{}", self, rhs))),
            _ => Err(crate::interpreter::pesan_op_biner("+", self.type_name_penuh().as_str(), rhs.type_name_penuh().as_str())),
        }
    }
    pub fn sub(&self, rhs: &VmValue) -> Result<VmValue, String> {
        match (self, rhs) {
            (VmValue::Int(a),   VmValue::Int(b))   =>
                a.checked_sub(*b).map(VmValue::Int).ok_or_else(|| luber("-", *a, *b)),
            (VmValue::Float(a), VmValue::Float(b)) => Ok(VmValue::Float(a - b)),
            (VmValue::Int(a),   VmValue::Float(b)) => Ok(VmValue::Float(*a as f64 - b)),
            (VmValue::Float(a), VmValue::Int(b))   => Ok(VmValue::Float(a - *b as f64)),
            _ => Err(crate::interpreter::pesan_op_biner("-", self.type_name_penuh().as_str(), rhs.type_name_penuh().as_str())),
        }
    }
    pub fn mul(&self, rhs: &VmValue) -> Result<VmValue, String> {
        match (self, rhs) {
            (VmValue::Int(a),   VmValue::Int(b))   =>
                a.checked_mul(*b).map(VmValue::Int).ok_or_else(|| luber("*", *a, *b)),
            (VmValue::Float(a), VmValue::Float(b)) => Ok(VmValue::Float(a * b)),
            (VmValue::Int(a),   VmValue::Float(b)) => Ok(VmValue::Float(*a as f64 * b)),
            (VmValue::Float(a), VmValue::Int(b))   => Ok(VmValue::Float(a * *b as f64)),
            // Aturan pengulangan dipakai bersama interpreter — lihat
            // `interpreter::ulang_string` untuk tiga divergensi yang dulu hidup di baris …
            (VmValue::Str(s),   VmValue::Int(n))   =>
                crate::interpreter::ulang_string(s, *n).map(VmValue::str),
            (VmValue::Int(n),   VmValue::Str(s))   =>
                crate::interpreter::ulang_string(s, *n).map(VmValue::str),
            _ => Err(crate::interpreter::pesan_op_biner("*", self.type_name_penuh().as_str(), rhs.type_name_penuh().as_str())),
        }
    }
    pub fn div(&self, rhs: &VmValue) -> Result<VmValue, String> {
        match (self, rhs) {
            // Pembagian INTEGER dengan nol tetap error — tidak ada bilangan bulat yang
            // mewakilinya.
            (VmValue::Int(a), VmValue::Int(0)) => Err(
                crate::interpreter::pesan_bagi_nol("/", &a.to_string())),
            // `i64::MIN / -1` meluap dan di Rust itu PANIC, bukan nilai salah —
            // artinya seluruh proses VM mati. Interpreter menolaknya dengan
            // diagnostik (diperbaiki di Phase 5D); di sini juga.
            (VmValue::Int(a),   VmValue::Int(b))   =>
                a.checked_div(*b).map(VmValue::Int).ok_or_else(|| luber("/", *a, *b)),
            (VmValue::Float(a), VmValue::Float(b)) => Ok(VmValue::Float(a / b)),
            (VmValue::Int(a),   VmValue::Float(b)) => Ok(VmValue::Float(*a as f64 / b)),
            (VmValue::Float(a), VmValue::Int(b))   => Ok(VmValue::Float(a / *b as f64)),
            _ => Err(crate::interpreter::pesan_op_biner("/", self.type_name_penuh().as_str(), rhs.type_name_penuh().as_str())),
        }
    }
    pub fn modulo(&self, rhs: &VmValue) -> Result<VmValue, String> {
        match (self, rhs) {
            // Tipe diperiksa SEBELUM nol: `"s" % 0` salah tipe, bukan salah pembagi — dan kalimat
            // bagi-nol dulu mencetak ISI sisi kirinya, array sebesar apa pun.
            (VmValue::Int(a), VmValue::Int(0)) => Err(
                crate::interpreter::pesan_bagi_nol("%", &a.to_string())),
            // Sama seperti `/`: `i64::MIN % -1` panic di Rust.
            (VmValue::Int(a), VmValue::Int(b))     =>
                a.checked_rem(*b).map(VmValue::Int).ok_or_else(|| luber("%", *a, *b)),
            (VmValue::Float(a), VmValue::Float(b)) => Ok(VmValue::Float(a % b)),
            (VmValue::Int(a),   VmValue::Float(b)) => Ok(VmValue::Float(*a as f64 % b)),
            (VmValue::Float(a), VmValue::Int(b))   => Ok(VmValue::Float(a % *b as f64)),
            _ => Err(crate::interpreter::pesan_op_biner("%", self.type_name_penuh().as_str(), rhs.type_name_penuh().as_str())),
        }
    }
    pub fn pow(&self, rhs: &VmValue) -> Result<VmValue, String> {
        match (self, rhs) {
            (VmValue::Int(a),   VmValue::Int(b)) if *b >= 0 => {
                crate::interpreter::Interpreter::pangkat_int(*a, *b)
                    .map(VmValue::Int)
                    .ok_or_else(|| luber("**", *a, *b))
            }
            (VmValue::Int(a),   VmValue::Int(b))   =>
                Ok(VmValue::Float(crate::interpreter::Interpreter::pangkat_bulat(*a as f64, *b))),
            (VmValue::Float(a), VmValue::Float(b)) => Ok(VmValue::Float(a.powf(*b))),
            (VmValue::Int(a),   VmValue::Float(b)) => Ok(VmValue::Float((*a as f64).powf(*b))),
            (VmValue::Float(a), VmValue::Int(b))   =>
                Ok(VmValue::Float(crate::interpreter::Interpreter::pangkat_bulat(*a, *b))),
            _ => Err(crate::interpreter::pesan_op_biner("**", self.type_name_penuh().as_str(), rhs.type_name_penuh().as_str())),
        }
    }
    pub fn neg(&self) -> Result<VmValue, String> {
        match self {
            VmValue::Int(n)   => n.checked_neg().map(VmValue::Int)
                .ok_or_else(crate::interpreter::pesan_neg_min),
            VmValue::Float(f) => Ok(VmValue::Float(-f)),
            _ => Err(crate::interpreter::pesan_op_unary("-", &self.type_name_penuh())),
        }
    }
    pub fn bit_not(&self) -> Result<VmValue, String> {
        match self {
            VmValue::Int(n) => Ok(VmValue::Int(!n)),
            _ => Err(crate::interpreter::pesan_op_unary("~", &self.type_name_penuh())),
        }
    }
    pub fn bit_and(&self, rhs: &VmValue) -> Result<VmValue, String> {
        self.int_bin(rhs, "&", |a, b| a & b)
    }
    pub fn bit_or(&self, rhs: &VmValue) -> Result<VmValue, String> {
        self.int_bin(rhs, "|", |a, b| a | b)
    }
    pub fn bit_xor(&self, rhs: &VmValue) -> Result<VmValue, String> {
        self.int_bin(rhs, "^", |a, b| a ^ b)
    }
    /// Geser bit, dengan jumlah geseran DIPERIKSA.
    pub fn shl(&self, rhs: &VmValue) -> Result<VmValue, String> {
        self.geser(rhs, true)
    }
    pub fn shr(&self, rhs: &VmValue) -> Result<VmValue, String> {
        self.geser(rhs, false)
    }
    fn geser(&self, rhs: &VmValue, ke_kiri: bool) -> Result<VmValue, String> {
        let op = if ke_kiri { "<<" } else { ">>" };
        let (VmValue::Int(x), VmValue::Int(y)) = (self, rhs) else {
            return Err(crate::interpreter::pesan_op_bit(
                op, &self.type_name_penuh(), &rhs.type_name_penuh()));
        };
        crate::interpreter::Interpreter::geser_terperiksa(*x, *y, ke_kiri).map(VmValue::Int)
    }
    pub fn and_not(&self, rhs: &VmValue) -> Result<VmValue, String> {
        self.int_bin(rhs, "&^", |a, b| a & !b)
    }
    fn int_bin(
        &self,
        rhs: &VmValue,
        op: &str,
        f: impl FnOnce(i128, i128) -> i128,
    ) -> Result<VmValue, String> {
        match (self, rhs) {
            (VmValue::Int(a), VmValue::Int(b)) => Ok(VmValue::Int(f(*a, *b))),
            _ => Err(crate::interpreter::pesan_op_bit(
                op, &self.type_name_penuh(), &rhs.type_name_penuh())),
        }
    }
    /// Kesamaan dua nilai VM. Boleh GAGAL: dua nilai yang sama-sama bisa
    /// dipanggil tidak punya jawaban, dan menjawab `false` di situ adalah
    /// jawaban yang salah dan diam. Cerminan `Interpreter::nilai_sama`.
    pub fn vm_eq(&self, rhs: &VmValue) -> Result<bool, String> {
        if crate::interpreter::stack_menipis() == Some(true) {
            return crate::interpreter::di_stack_lega(|| self.vm_eq(rhs));
        }
        if self.bisa_dipanggil() && rhs.bisa_dipanggil() {
            return Err(crate::interpreter::pesan_banding(
                &self.type_name_penuh(), &rhs.type_name_penuh()));
        }
        Ok(match (self, rhs) {
            (VmValue::Nil,      VmValue::Nil)       => true,
            // `unit` bukan `nil`: pembandingannya cuma sama dengan unit.
            (VmValue::Unit,     VmValue::Unit)      => true,
            (VmValue::Bool(a),  VmValue::Bool(b))   => a == b,
            (VmValue::Int(a),   VmValue::Int(b))    => a == b,
            // Rune cuma sama dengan rune — bukan dengan string 1-karakter,
            // bukan dengan kode angkanya.
            (VmValue::Rune(a),  VmValue::Rune(b))   => a == b,
            (VmValue::Ptr(a),   VmValue::Ptr(b))    => a == b,
            (VmValue::Float(a), VmValue::Float(b))  => a == b,
            (VmValue::Int(a),   VmValue::Float(b))  => (*a as f64) == *b,
            (VmValue::Float(a), VmValue::Int(b))    => *a == (*b as f64),
            (VmValue::Str(a),   VmValue::Str(b))    => a == b,
            // Dua array numerik sama kalau ISINYA sama — sama seperti `Value::Array`,
            // dan bukan sekadar kalau kebetulan berbagi penyimpanan.
            (VmValue::Nd(a),    VmValue::Nd(b))    => a == b,
            // Handle dibandingkan lewat IDENTITASNYA, bukan isinya —
            // dua jendela berbeda yang kebetulan menampilkan hal yang sama
            // tetap dua jendela. Aturan yang sama dengan interpreter.
            (VmValue::Canvas(a), VmValue::Canvas(b)) => Arc::ptr_eq(a, b),
            (VmValue::Array(a), VmValue::Array(b))  => {
                // Penyimpanan yang SAMA dikunci sekali lalu dibandingkan dengan dirinya: siapa
                // pemilik penyimpanannya tidak boleh terlihat — `[NaN] == [NaN]` itu `false`.
                if Arc::ptr_eq(a, b) {
                    let g = a.lock().unwrap();
                    return semua_vm_eq(g.iter(), g.iter());
                }
                let a = a.lock().unwrap();
                let b = b.lock().unwrap();
                a.len() == b.len() && semua_vm_eq(a.iter(), b.iter())?
            }
            (VmValue::Tuple(a), VmValue::Tuple(b))  => {
                let (a, b) = (isi_tuple(a), isi_tuple(b));
                a.len() == b.len() && semua_vm_eq(a.iter(), b.iter())?
            }
            (VmValue::Map(a), VmValue::Map(b)) => {
                // Sama seperti array: penyimpanan bersama tetap dibandingkan isinya.
                if Arc::ptr_eq(a, b) {
                    if let Label::Enum { payload: Some(p), .. } = &a.label {
                        if !p.vm_eq(p)? { return Ok(false); }
                    }
                    let g = a.lock().unwrap();
                    for v in g.values() { if !v.vm_eq(v)? { return Ok(false); } }
                    return Ok(true);
                }
                let label_sama = match (&a.label, &b.label) {
                    (Label::Dict, Label::Dict) => true,
                    (Label::Struct(x), Label::Struct(y)) => x == y,
                    (Label::Enum { nama: n1, payload: p1 }, Label::Enum { nama: n2, payload: p2 }) =>
                        std::ptr::eq(*n1, *n2) && match (p1, p2) {
                            (Some(x), Some(y)) => x.vm_eq(y)?,
                            (None, None) => true,
                            _ => false,
                        },
                    _ => false,
                };
                if !label_sama { return Ok(false); }
                let ga = a.lock().unwrap();
                let gb = b.lock().unwrap();
                if ga.len() != gb.len() { return Ok(false); }
                let mut sama = true;
                for (k, v) in ga.iter() {
                    match gb.get(k) {
                        Some(other) if v.vm_eq(other)? => {}
                        _ => { sama = false; break; }
                    }
                }
                sama
            }
            (VmValue::Channel(a), VmValue::Channel(b)) => a.sama(b),
            (VmValue::Sinkron(a), VmValue::Sinkron(b)) => a.sama(b),
            (VmValue::Asing(a), VmValue::Asing(b)) => a.sama(b),
            // Membandingkan binding = membandingkan nilai yang disimpannya.
            (VmValue::Cell(a), _) => a.lock().unwrap_or_else(|p| p.into_inner()).vm_eq(rhs)?,
            (_, VmValue::Cell(b)) => self.vm_eq(&b.lock().unwrap_or_else(|p| p.into_inner()))?,
            _ => false,
        })
    }
    pub fn lt(&self, rhs: &VmValue) -> Result<bool, String> { self.banding(rhs, std::cmp::Ordering::Less, false) }
    pub fn le(&self, rhs: &VmValue) -> Result<bool, String> { self.banding(rhs, std::cmp::Ordering::Less, true) }
    pub fn gt(&self, rhs: &VmValue) -> Result<bool, String> { self.banding(rhs, std::cmp::Ordering::Greater, false) }
    pub fn ge(&self, rhs: &VmValue) -> Result<bool, String> { self.banding(rhs, std::cmp::Ordering::Greater, true) }

    /// Cerminan `Interpreter::banding_lepas`: SATU badan untuk keempat urutan. Dulu `a > b`
    /// dihitung sebagai `b < a` (kalimat gagalnya terbalik), dan elemen agregat lewat `<` biasa
    /// — jadi `[true] < [false]` ditolak di sini dan `[1.0] < [NaN]` dijawab, kebalikan acuan.
    #[inline]
    pub fn banding(&self, rhs: &VmValue, ord: std::cmp::Ordering, or_eq: bool) -> Result<bool, String> {
        use std::cmp::Ordering;
        if crate::interpreter::stack_menipis() == Some(true) {
            return crate::interpreter::di_stack_lega(|| self.banding(rhs, ord, or_eq));
        }
        let r = match (self, rhs) {
            (VmValue::Int(a),   VmValue::Int(b))   => Some(a.cmp(b)),
            (VmValue::Float(a), VmValue::Float(b)) => a.partial_cmp(b),
            (VmValue::Int(a),   VmValue::Float(b)) => (*a as f64).partial_cmp(b),
            (VmValue::Float(a), VmValue::Int(b))   => a.partial_cmp(&(*b as f64)),
            (VmValue::Str(a),   VmValue::Str(b))   => Some(a.as_str().cmp(b.as_str())),
            (VmValue::Rune(a),  VmValue::Rune(b))  => Some(a.cmp(b)),
            (VmValue::Array(x), VmValue::Array(y)) => {
                // Penyimpanan yang sama dikunci SEKALI: mengunci Mutex yang sama dua kali
                // menggantung selamanya.
                if Arc::ptr_eq(x, y) {
                    let g = x.lock().unwrap_or_else(|p| p.into_inner());
                    banding_leksikografis(&g, &g)?
                } else {
                    let (xa, ya) = (x.lock().unwrap_or_else(|p| p.into_inner()),
                                    y.lock().unwrap_or_else(|p| p.into_inner()));
                    banding_leksikografis(&xa, &ya)?
                }
            }
            (VmValue::Tuple(x), VmValue::Tuple(y)) => banding_leksikografis(&isi_tuple(x), &isi_tuple(y))?,
            _ => return Err(crate::interpreter::pesan_banding(
                self.type_name_penuh().as_str(), rhs.type_name_penuh().as_str())),
        };
        // NaN di salah satu sisi tidak berurutan terhadap apa pun: keempat operator `false`.
        Ok(match r {
            Some(o) => o == ord || (or_eq && o == Ordering::Equal),
            None => false,
        })
    }
}

/// Leksikografis untuk `<`/`>`: elemen lewat `cmp_vm` (urutan `arr::sort`), dan elemen yang
/// tidak sebanding membatalkan pembandingannya sambil menyebut tipe ELEMEN-nya.
fn banding_leksikografis(xs: &[VmValue], ys: &[VmValue]) -> Result<Option<std::cmp::Ordering>, String> {
    for (ex, ey) in xs.iter().zip(ys.iter()) {
        let Some(o) = crate::stdlib::koleksi::cmp_vm(ex, ey) else {
            return Err(crate::interpreter::pesan_banding(
                ex.type_name_penuh().as_str(), ey.type_name_penuh().as_str()));
        };
        if o != std::cmp::Ordering::Equal { return Ok(Some(o)); }
    }
    Ok(Some(xs.len().cmp(&ys.len())))
}

/// Kesamaan elemen per elemen, meneruskan kegagalan pembandingnya.
pub fn semua_vm_eq<'a>(
    x: impl Iterator<Item = &'a VmValue>, y: impl Iterator<Item = &'a VmValue>,
) -> Result<bool, String> {
    for (a, b) in x.zip(y) {
        if !a.vm_eq(b)? { return Ok(false); }
    }
    Ok(true)
}

/// `position` versi boleh-gagal — satu definisi untuk `contains`, `index_of`,
/// `unique`, `union` dan saudaranya di sisi VM.
pub fn posisi_vm_eq<'a>(
    it: impl Iterator<Item = &'a VmValue>, t: &VmValue,
) -> Result<Option<usize>, String> {
    for (i, v) in it.enumerate() {
        if v.vm_eq(t)? { return Ok(Some(i)); }
    }
    Ok(None)
}

/// `any` versi boleh-gagal, sisi VM.
/// `posisi_vm_eq` atas array milik penunjuk, tanpa menyalin seluruh isinya: kunci diambil sebentar per
/// elemen (bukan selama `vm_eq`, yang bisa mengunci array yang sama — `arr::index_of(a, a)`).
pub fn posisi_vm_eq_di(v: &std::sync::Mutex<Vec<VmValue>>, t: &VmValue) -> Result<Option<usize>, String> {
    let mut i = 0;
    loop {
        let Some(e) = v.lock().unwrap_or_else(|p| p.into_inner()).get(i).cloned() else { return Ok(None) };
        if e.vm_eq(t)? { return Ok(Some(i)); }
        i += 1;
    }
}

pub fn ada_vm_eq<'a>(
    it: impl Iterator<Item = &'a VmValue>, t: &VmValue,
) -> Result<bool, String> {
    Ok(posisi_vm_eq(it, t)?.is_some())
}

// ─── Display ─────────────────────────────────────────────────────────────

impl fmt::Display for VmValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if crate::interpreter::stack_menipis() == Some(true) {
            return f.write_str(&crate::interpreter::di_stack_lega(|| self.to_string()));
        }
        match self {
            VmValue::Nil          => write!(f, "nil"),
            VmValue::Unit         => write!(f, "()"),
            VmValue::Bool(b)      => write!(f, "{}", b),
            VmValue::Int(n)       => write!(f, "{}", n),
            VmValue::Rune(c)      => write!(f, "{}", c),
            VmValue::Ptr(a)       => write!(f, "<ptr 0x{:x}>", a),
            VmValue::ExternFn(i)  => write!(f, "<extern fn {}>", i.name),
            // Syaratnya HARUS sama persis dengan `Value` di interpreter.
            VmValue::Float(n)     => {
                if n.fract() == 0.0 && n.is_finite() { write!(f, "{:.1}", n) }
                else { write!(f, "{}", n) }
            }
            VmValue::Str(s)       => write!(f, "{}", s),
            VmValue::Nd(a)        => write!(f, "{}", a),
            VmValue::Canvas(c)    => {
                let g = c.lock().unwrap_or_else(|p| p.into_inner());
                // Mengikuti interpreter PERSIS, termasuk judulnya: ia mesin acuan, dan
                // `wok(Canvas::new(4,4))` dulu mencetak `<Canvas 4x4 'JRG Canvas'>` di sana …
                write!(f, "<Canvas {}x{} '{}'>", g.width, g.height, g.title)
            }
            // Ditulis langsung ke formatter: string per anak dulu menyalin ulang isi tiap tingkat (kuadratik).
            VmValue::Array(a)     => {
                let v = a.lock().unwrap();
                f.write_str("[")?;
                for (i, x) in v.iter().enumerate() { if i > 0 { f.write_str(", ")?; } write!(f, "{}", x)?; }
                f.write_str("]")
            }
            VmValue::Tuple(t)     => {
                f.write_str("(")?;
                for (i, x) in isi_tuple(t).iter().enumerate() { if i > 0 { f.write_str(", ")?; } write!(f, "{}", x)?; }
                f.write_str(")")
            }
            // Map dipakai VM untuk tiga hal sekaligus; jenisnya dibaca dari labelnya.
            VmValue::Map(m)       => match &m.label {
                // Enum: `Tipe::Varian` atau `Tipe::Varian(payload)`.
                Label::Enum { nama, payload } => match payload {
                    Some(p) => write!(f, "{}::{}({})", crate::ruang_tipe::bersih(&nama.tipe), nama.varian, p),
                    None    => write!(f, "{}::{}", crate::ruang_tipe::bersih(&nama.tipe), nama.varian),
                },
                label => {
                    let g = m.lock().unwrap();
                    let mut kunci: Vec<&String> = g.keys().map(|k| &**k).collect();
                    kunci.sort();
                    // Struct punya nama tipe; dict biasa diberi awalan `dict`.
                    let awalan = match label { Label::Struct(n) => n.as_ref(), _ => "dict" };
                    write!(f, "{} {{", crate::ruang_tipe::bersih(awalan))?;
                    for (i, k) in kunci.iter().enumerate() {
                        if i > 0 { f.write_str(", ")?; }
                        write!(f, "{}: {}", k, g[*k])?;
                    }
                    f.write_str("}")
                }
            },
            // Interpreter mencetak `<fn nama>` untuk `fn` dan `<closure>`
            // (tanpa nama) untuk `|x| ...`. Closure tak-menangkap tetap
            // berupa `Function`, jadi penandanya dibaca dari `is_closure`.
            VmValue::Function(fn_) => {
                if fn_.is_closure { write!(f, "<closure>") } else { write!(f, "<fn {}>", fn_.name) }
            }
            VmValue::Closure(_)    => write!(f, "<closure>"),
            VmValue::NativeFn(_)   => write!(f, "<native fn>"),
            VmValue::Channel(ch)   => {
                // `cap=N`, mengikuti interpreter. Bentuk `buf=x/N` di sini
                // lebih informatif, dan itu justru bukan alasan yang cukup:
                // satu program tidak boleh punya dua keluaran.
                write!(f, "<chan cap={}>", ch.kapasitas())
            }
            VmValue::Sinkron(p)    => write!(f, "{}", p.tampil()),
            VmValue::Asing(a)      => write!(f, "{}", a.tampil()),
            VmValue::Cell(c)       => write!(f, "{}", c.lock().unwrap_or_else(|p| p.into_inner())),
            VmValue::Ktor(k)       => write!(f, "<ctor {}::{}>", crate::ruang_tipe::bersih(&k.nama.tipe), k.nama.varian),
        }
    }
}

// ─── From impls ───────────────────────────────────────────────────────────

impl From<bool>   for VmValue { fn from(v: bool)   -> Self { VmValue::Bool(v) } }
impl From<i64>    for VmValue { fn from(v: i64)    -> Self { VmValue::Int(v as i128) } }
impl From<i128>   for VmValue { fn from(v: i128)   -> Self { VmValue::Int(v) } }
impl From<f64>    for VmValue { fn from(v: f64)    -> Self { VmValue::Float(v) } }
impl From<usize>  for VmValue { fn from(v: usize)  -> Self { VmValue::Int(v as i128) } }
impl From<String> for VmValue { fn from(v: String) -> Self { VmValue::str(v) } }
impl From<&str>   for VmValue { fn from(v: &str)   -> Self { VmValue::str(v) } }

/// `b[..n]` ASCII (`0 < n <= b.len()`), delapan byte sekali baca; ekornya jendela delapan byte yang berakhir
/// di `n` (bertumpang dengan yang sudah diperiksa), atau awal teks bertopeng kalau `n < 8`.
#[inline(always)]
pub fn prefiks_ascii(b: &[u8], n: usize) -> bool {
    const M: u64 = 0x8080_8080_8080_8080;
    let kata = |k: usize| u64::from_le_bytes(b[k..k + 8].try_into().unwrap());
    if b.len() < 8 { return b[..n].iter().all(|&c| c < 0x80); }
    if n < 8 { return kata(0) & (M >> (8 * (8 - n))) == 0; }
    let mut k = 0;
    while k + 8 < n { if kata(k) & M != 0 { return false; } k += 8; }
    kata(n - 8) & M == 0
}

/// Banyaknya karakter `s` (`len` teks). Teks pendek ASCII lewat `prefiks_ascii`: `chars().count()` di bawah
/// 32 byte menghitung per byte; yang panjang sudah per kata.
#[inline]
pub fn jumlah_karakter(s: &str) -> usize {
    let b = s.as_bytes();
    if b.len() < 32 && (b.is_empty() || prefiks_ascii(b, b.len())) { b.len() } else { s.chars().count() }
}
