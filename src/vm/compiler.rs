// src/vm/compiler.rs — src/vm/compiler.rs  (FIXED — sesuai parser AST actual)
// AST → Bytecode Compiler

use crate::parser::{
    ParsedFile, Item, FnDecl, FnBody, FnParam, Block,
    Stmt, IfStmt, Expr,
    Pattern, ImplItem, TraitItem, TypeExpr, PrimTy, EnumVariantKind, ExternItem,
};
use super::chunk::Chunk;
use super::opcode::OpCode;
use super::value::{VmValue, VmFunction, UpvalSrc};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

// ─── Error ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CompileError { pub msg: String, pub line: u32 }

impl CompileError {
    pub(crate) fn new(msg: impl Into<String>) -> Self { CompileError { msg: msg.into(), line: 0 } }
}
impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "compile error: {}", self.msg)
    }
}

pub(crate) type CR<T> = Result<T, CompileError>;

thread_local! {
    /// Lihat catatannya di `compile_unit`.
    pub(crate) static BOLEH_JALUR_ARR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };

    /// Unit yang SEDANG dikompilasi: nama miliknya sendiri dan import-nya.
    static UNIT: RefCell<Unit> = RefCell::new(Unit::default());
}

/// Jenis nama nilai tingkat atas milik sebuah unit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Milik { Fungsi, Tetap, Var }

/// Modul yang sudah dikompilasi, dilihat dari unit yang mengimpornya.
#[derive(Debug, Default, Clone)]
pub struct InfoModul {
    /// Awalan nama global miliknya (`alat#1`): `#` tidak bisa ditulis di nama mana pun,
    /// jadi ia tidak pernah bertabrakan dengan builtin, tipe, maupun modul bernama sama.
    pub awalan: String,
    /// Yang terjangkau sebagai `alias::NAMA` → nama globalnya.
    pub ekspor: HashMap<String, String>,
    /// `var` tingkat modul: keadaan milik modulnya, tidak diekspor.
    pub var: Vec<String>,
    /// Nama ber-awalan `_`: privat milik modulnya.
    pub privat: Vec<String>,
    /// `(varian, enum)` telanjang milik modul — di luar modulnya ia ber-awalan.
    pub varian_telanjang: Vec<(String, String)>,
    /// `(nama, alasan)` nama-bukan-nilai miliknya, dipasang lagi di bawah alias pengimpor.
    pub bukan_nilai: Vec<(String, String)>,
    /// Contoh isi untuk kalimat "itu nama modul".
    pub contoh: String,
}

/// Alias `import` sebuah unit → modulnya.
pub type PetaImpor = HashMap<String, std::sync::Arc<InfoModul>>;

#[derive(Default, Clone)]
struct Unit {
    /// Awalan nama global milik unit ini; `None` untuk berkas utama (nama polos).
    awalan: Option<String>,
    milik: HashMap<String, Milik>,
    impor: PetaImpor,
    /// Nama asli → alias untuk import ber-`as` milik unit ini.
    beralias: HashMap<String, String>,
}

fn nama_global(awalan: Option<&str>, nama: &str) -> String {
    match awalan {
        Some(a) => format!("{}::{}", a, nama),
        None => nama.to_string(),
    }
}

/// Nama telanjang milik unit yang sedang dikompilasi: nama globalnya dan jenisnya.
/// Di VM semua unit berbagi satu tabel global, jadi nama milik modul diterjemahkan DI
/// SINI — dulu ia juga didefinisikan telanjang, dan dua modul saling menimpa.
pub(crate) fn milik_unit(nama: &str) -> Option<(String, Milik)> {
    UNIT.with(|u| {
        let u = u.borrow();
        let jenis = *u.milik.get(nama)?;
        Some((nama_global(u.awalan.as_deref(), nama), jenis))
    })
}

/// `var` tingkat atas milik unit ini — satu-satunya global yang boleh ditugasi.
pub(crate) fn var_unit() -> Vec<String> {
    UNIT.with(|u| u.borrow().milik.iter()
        .filter(|(_, j)| **j == Milik::Var).map(|(n, _)| n.clone()).collect())
}

/// Ke mana sebuah jalur `a::b` menunjuk, diputuskan saat kompilasi.
pub(crate) enum Jalur {
    /// Global milik modul yang diimpor unit ini.
    Global(String),
    /// Bawaan bahasa, dirujuk lewat NILAINYA — tidak ada nama untuk dibayangi.
    Bawaan(VmValue),
    /// Tidak terjangkau; kalimatnya sudah diketahui, galatnya tetap saat JALAN.
    Tolak(String),
    /// Dicari saat jalan dengan nama yang ditulis (`Tipe::metode`).
    Biasa,
}

pub(crate) fn resolusi_jalur(segments: &[String]) -> Jalur {
    use crate::interpreter as it;
    use super::bawaan::bawaan;
    // `::nama`: bawaan saja, tanpa peduli deklarasi program.
    if segments.first().is_some_and(|s| s.is_empty()) {
        let nama = segments[1..].join("::");
        return match super::bawaan::bawaan_atau_konstruktor(&nama) {
            Some(v) => Jalur::Bawaan(v),
            None => {
                let daftar = crate::vm::daftar_builtin();
                let saran = crate::diag::saran_nama_terdekat(&nama, daftar.iter().map(|s| s.as_str()));
                Jalur::Tolak(it::pesan_bukan_bawaan(&nama, saran.as_deref()))
            }
        };
    }
    let full = segments.join("::");
    // Nama ber-jalur yang dideklarasikan unit ini sendiri (`extern "C++" { fn math::abs }`)
    // menang atas bawaan, seperti nama telanjangnya.
    if let Some((g, _)) = milik_unit(&full) { return Jalur::Global(g); }
    // `None`: bukan alias unit ini. `Some(None)`: alias, tapi namanya bukan ekspor.
    let lewat_alias = UNIT.with(|u| {
        let u = u.borrow();
        let info = u.impor.get(&segments[0])?;
        Some(info.ekspor.get(&segments[1..].join("::")).cloned())
    });
    if let Some(Some(g)) = lewat_alias { return Jalur::Global(g); }
    // Berkas nyata menang per NAMA; sisanya tetap bawaan (`math::sqrt` di samping `math.jrg`).
    if let Some(v) = bawaan(&full) { return Jalur::Bawaan(v); }
    if lewat_alias.is_none() { return Jalur::Biasa; }
    if let Some(alasan) = bukan_nilai_lookup(&full) { return Jalur::Tolak(alasan); }
    let daftar = crate::vm::daftar_builtin();
    let lain = crate::diag::nama_di_namespace_lain(&full, daftar.iter().map(|s| s.as_str()));
    let terlihat = nama_terlihat_unit();
    let saran = crate::diag::saran_nama_terdekat(&full, daftar.iter().map(|s| s.as_str()))
        .or_else(|| crate::diag::saran_nama_terdekat(&full, terlihat.iter().map(|s| s.as_str())));
    let ekor = segments.last().map(|s| s.as_str()).unwrap_or("");
    let telanjang = milik_unit(ekor).is_some() || bawaan(ekor).is_some();
    Jalur::Tolak(it::pesan_path_tak_dikenal_plus(&full, saran.as_deref(), &lain, telanjang))
}

/// Nama yang bisa ditulis dari unit ini selain bawaan: miliknya sendiri, dan
/// `alias::nama` tiap ekspor modul yang diimpornya — calon saran ejaan.
pub(crate) fn nama_terlihat_unit() -> Vec<String> {
    UNIT.with(|u| {
        let u = u.borrow();
        let mut v: Vec<String> = u.milik.keys().cloned().collect();
        for (alias, info) in &u.impor {
            v.extend(info.ekspor.keys().map(|k| format!("{}::{}", alias, k)));
        }
        v.sort();
        v
    })
}

// ─── Local / Loop ────────────────────────────────────────────────────────

#[derive(Clone)]
pub(crate) struct Local {
    pub(crate) name: String,
    pub(crate) depth: usize,
    /// Indeks stack SESUNGGUHNYA (relatif frame), bukan sekadar posisi di
    /// `locals`.
    pub(crate) slot: usize,
}

/// Satu tangkapan closure, dari sisi compiler: namanya (untuk resolusi di
/// badan closure) plus dari mana nilainya diambil saat closure dibuat.
#[derive(Clone)]
pub(crate) struct Upval { pub(crate) name: String, pub(crate) src: UpvalSrc }

pub(crate) struct LoopCtx {
    pub(crate) break_patches: Vec<usize>,
    pub(crate) continue_patches: Vec<usize>,
    pub(crate) break_base: usize,
    pub(crate) continue_base: usize,
    /// Label loop ini (`'luar: for ...`), kalau ada.
    pub(crate) label: Option<String>,
}

// ─── Compiler ────────────────────────────────────────────────────────────

pub struct Compiler {
    pub(crate) chunk:       Chunk,
    pub(crate) locals:      Vec<Local>,
    pub(crate) scope_depth: usize,
    pub(crate) loops:       Vec<LoopCtx>,
    /// Nama lokal yang pembacaan BERIKUTNYA memindahkan nilainya keluar (`x = x.m(..)`).
    pub(crate) konsumsi_penerima: Option<String>,
    /// Slot lokal yang sudah ditangkap sebuah closure di fungsi ini (jadi bisa berupa sel).
    pub(crate) slot_tertangkap: std::collections::HashSet<usize>,
    /// Nama-nama yang ditangkap dari scope pembuat, urut sesuai indeks
    /// `LoadUpval`/`StoreUpval`.
    pub(crate) upvals:      Vec<Upval>,
    /// Fungsi bersarang yang terlihat dari titik kompilasi ini, dipetakan ke nama
    /// global uniknya.
    pub(crate) saudara:     Vec<(String, String)>,
    /// Berapa nilai SEMENTARA yang sedang menumpuk di stack pada titik kompilasi
    /// ini — nilai yang sudah didorong tapi belum dikonsumsi, dan yang bukan …
    pub(crate) temps:       usize,
}

// ─── Enum / Option / Result variant constructors
// ───────────────────────────
thread_local! {
    /// Nama method yang dideklarasikan `impl`/`trait` mana pun dalam program ini
    /// — TERMASUK dari modul, karena modul dikompilasi lebih dulu dan himpunan …
    pub(crate) static METODE_PEMAKAI: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    static ENUM_CTORS: RefCell<HashMap<String, (String, usize)>> = RefCell::new(HashMap::new());
    static ENUM_STRUCT_CTORS: RefCell<HashMap<String, (String, Vec<String>)>> = RefCell::new(HashMap::new());
    static STRUCT_DEFS: RefCell<HashMap<String, StructDefInfo>> = RefCell::new(HashMap::new());
    /// Nama yang dideklarasikan tapi bukan nilai → penjelasannya. MENUMPUK lintas unit
    /// (modul dikompilasi lebih dulu), seperti `METODE_PEMAKAI`; kalimatnya milik interpreter.
    static BUKAN_NILAI: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    /// Sama, untuk nama yang penjelasannya bergantung pada ALIAS import unit yang
    /// sedang dikompilasi — diganti tiap unit, karena alias cuma berlaku di berkasnya.
    static BUKAN_NILAI_UNIT: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// Penjelasan nama-bukan-nilai milik `file` sendiri: dipasang telanjang (tipe bersifat
/// global), dan dikembalikan — bersama `(varian, enum)` telanjangnya — supaya tiap
/// pengimpor memasangnya lagi di bawah ALIAS-nya sendiri.
pub(crate) fn catat_bukan_nilai_berkas(
    file: &ParsedFile,
) -> (Vec<(String, String)>, Vec<(String, String)>) {
    use crate::interpreter as it;
    let mut isi: Vec<(String, String)> = Vec::new();
    let mut varian_telanjang: Vec<(String, String)> = Vec::new();
    for item in &file.items {
        match item {
            Item::Struct(sd) => isi.push((sd.name.clone(), it::pesan_nama_struct(&sd.name))),
            Item::Trait(td) => isi.push((td.name.clone(), it::pesan_nama_trait(&td.name))),
            Item::Enum(ed) => {
                isi.push((ed.name.clone(),
                          it::pesan_nama_enum(&ed.name, ed.variants.first().map(|v| v.name.as_str()))));
                for v in &ed.variants {
                    if let EnumVariantKind::Struct(fields) = &v.kind {
                        let nama: Vec<String> = fields.iter().map(|f| f.name.clone()).collect();
                        let alasan = it::pesan_varian_berfield(&ed.name, &v.name, &nama);
                        isi.push((format!("{}::{}", ed.name, v.name), alasan.clone()));
                        isi.push((v.name.clone(), alasan));
                    } else {
                        varian_telanjang.push((v.name.clone(), ed.name.clone()));
                    }
                }
            }
            Item::Extern(eb) => for ei in &eb.items {
                if let ExternItem::Static { name, .. } = ei {
                    isi.push((name.clone(), it::pesan_extern_static(name)));
                }
            },
            _ => {}
        }
    }
    BUKAN_NILAI.with(|m| {
        let mut m = m.borrow_mut();
        for (k, v) in &isi { m.entry(k.clone()).or_insert_with(|| v.clone()); }
    });
    (isi, varian_telanjang)
}

/// Penjelasan yang bergantung pada ALIAS, dibangun ulang tiap unit: nama modulnya,
/// `alias::Tipe`, varian telanjangnya, dan `alias::nama` yang tidak diekspor.
fn pasang_penjelasan_impor(impor: &PetaImpor) {
    use crate::interpreter as it;
    let mut milik_unit: HashMap<String, String> = HashMap::new();
    let mut telanjang: Vec<(String, String)> = Vec::new();
    // Urutan alias dibuat tetap, supaya dua modul yang sama-sama punya varian `Merah`
    // tidak bergantian menang antar-jalan.
    let mut alias: Vec<&String> = impor.keys().collect();
    alias.sort();
    for a in alias {
        let info = &impor[a];
        milik_unit.insert(a.clone(), it::pesan_nama_modul(a, &info.contoh));
        for (k, v) in &info.bukan_nilai {
            milik_unit.insert(format!("{}::{}", a, k), v.clone());
        }
        for (varian, tipe) in &info.varian_telanjang {
            telanjang.push((varian.clone(), it::pesan_varian_modul(varian, tipe, a)));
        }
        for nama in &info.var {
            milik_unit.insert(format!("{}::{}", a, nama), it::pesan_var_modul(nama, a));
        }
        for nama in &info.privat {
            milik_unit.insert(format!("{}::{}", a, nama), it::pesan_privat_modul(nama, a));
        }
    }
    for (k, v) in telanjang { milik_unit.entry(k).or_insert(v); }
    BUKAN_NILAI_UNIT.with(|m| *m.borrow_mut() = milik_unit);
}

/// Penjelasan untuk `nama`: yang bergantung pada import unit ini dulu, baru yang
/// menumpuk lintas unit (tipe bersifat global).
pub(crate) fn bukan_nilai_lookup(nama: &str) -> Option<String> {
    BUKAN_NILAI_UNIT.with(|m| m.borrow().get(nama).cloned())
        .or_else(|| BUKAN_NILAI.with(|m| m.borrow().get(nama).cloned()))
        .or_else(|| {
            // Dipakai cuma kalau globalnya memang tidak ada saat jalan, jadi `Tipe::m` yang
            // kebetulan senama dengan modul ber-alias tidak dituduh.
            let (kepala, _) = nama.split_once("::")?;
            UNIT.with(|u| u.borrow().beralias.get(kepala).cloned())
                .map(|alias| crate::interpreter::pesan_modul_beralias(nama, kepala, &alias))
        })
}

thread_local! {
    /// Deklarasi TIPE tiap modul yang sudah dikompilasi, terlihat dari unit pengimpornya
    /// (transitif) seperti `struct_defs` interpreter. Dulu dikosongkan tiap unit, jadi
    /// `Titik {}` milik modul kehilangan default-nya di VM.
    static TIPE_MODUL: RefCell<HashMap<String, TipeModul>> = RefCell::new(HashMap::new());
}

#[derive(Clone, Default)]
struct TipeModul {
    /// `Warna::Rgb` → (`Warna`, 3): cuma bentuk ber-TIPE; yang telanjang tetap nilai modul.
    enum_ctor: HashMap<String, (String, usize)>,
    enum_struct: HashMap<String, (String, Vec<String>)>,
    struct_def: HashMap<String, StructDefInfo>,
    /// Tipe → nama method yang terpasang untuknya di unit itu (`impl`, warisan, default trait).
    metode: HashMap<String, Vec<String>>,
    /// Trait milik unit itu, untuk `impl` di unit pengimpornya.
    trait_def: HashMap<String, TraitModul>,
    impor: Vec<String>,
}

/// Sebuah trait dilihat dari unit lain: method abstraknya, dan badan default-nya bersama
/// konteks unit pemiliknya — nama telanjang di badan itu merujuk modul si trait.
#[derive(Clone)]
struct TraitModul {
    wajib: Vec<String>,
    default: Vec<FnDecl>,
    konteks: std::sync::Arc<Konteks>,
}

/// Semua yang dibaca kompilasi sebuah badan fungsi dari unitnya.
#[derive(Clone, Default)]
struct Konteks {
    unit: Unit,
    ctor: HashMap<String, (String, usize)>,
    ctor_struct: HashMap<String, (String, Vec<String>)>,
    struct_def: HashMap<String, StructDefInfo>,
    bukan_nilai: HashMap<String, String>,
    jalur_arr: bool,
}

fn konteks_kini() -> Konteks {
    Konteks {
        unit: UNIT.with(|u| u.borrow().clone()),
        ctor: ENUM_CTORS.with(|r| r.borrow().clone()),
        ctor_struct: ENUM_STRUCT_CTORS.with(|r| r.borrow().clone()),
        struct_def: STRUCT_DEFS.with(|r| r.borrow().clone()),
        bukan_nilai: BUKAN_NILAI_UNIT.with(|r| r.borrow().clone()),
        jalur_arr: BOLEH_JALUR_ARR.with(|f| f.get()),
    }
}

/// Pasang `k` sebagai konteks kompilasi; yang digantikannya dikembalikan untuk dipulihkan.
fn pasang_konteks(k: Konteks) -> Konteks {
    Konteks {
        unit: UNIT.with(|u| u.replace(k.unit)),
        ctor: ENUM_CTORS.with(|r| r.replace(k.ctor)),
        ctor_struct: ENUM_STRUCT_CTORS.with(|r| r.replace(k.ctor_struct)),
        struct_def: STRUCT_DEFS.with(|r| r.replace(k.struct_def)),
        bukan_nilai: BUKAN_NILAI_UNIT.with(|r| r.replace(k.bukan_nilai)),
        jalur_arr: BOLEH_JALUR_ARR.with(|f| f.replace(k.jalur_arr)),
    }
}

/// Awalan tiap modul yang diimpor unit yang sedang dikompilasi — kunci `TIPE_MODUL`.
/// Berkunci alias, dua berkas bernama sama saling menimpa tipe-tipenya.
fn awalan_impor() -> Vec<String> {
    UNIT.with(|u| {
        let mut v: Vec<String> = u.borrow().impor.values().map(|i| i.awalan.clone()).collect();
        v.sort();
        v.dedup();
        v
    })
}

/// Gabungan deklarasi tipe semua modul yang terlihat dari unit ini (transitif).
fn tipe_terlihat() -> TipeModul {
    TIPE_MODUL.with(|t| {
        let t = t.borrow();
        let mut hasil = TipeModul::default();
        let (mut antre, mut sudah) = (awalan_impor(), HashSet::new());
        while let Some(ns) = antre.pop() {
            if !sudah.insert(ns.clone()) { continue; }
            let Some(m) = t.get(&ns) else { continue };
            for (k, v) in &m.enum_ctor { hasil.enum_ctor.entry(k.clone()).or_insert_with(|| v.clone()); }
            for (k, v) in &m.enum_struct { hasil.enum_struct.entry(k.clone()).or_insert_with(|| v.clone()); }
            for (k, v) in &m.struct_def { hasil.struct_def.entry(k.clone()).or_insert_with(|| v.clone()); }
            for (k, v) in &m.metode { hasil.metode.entry(k.clone()).or_insert_with(|| v.clone()); }
            for (k, v) in &m.trait_def { hasil.trait_def.entry(k.clone()).or_insert_with(|| v.clone()); }
            antre.extend(m.impor.iter().cloned());
        }
        hasil
    })
}

#[derive(Clone)]
pub(crate) struct StructDefInfo {
    pub(crate) parent: Option<String>,
    pub(crate) fields: Vec<crate::parser::StructField>,
}

/// Pasang metadata parameter (nama + berapa yang wajib) pada sebuah
/// `VmFunction`, supaya runtime bisa memeriksa arity sebagai RENTANG dan …
pub(crate) fn catat_metode_pemakai(file: &ParsedFile) {
    METODE_PEMAKAI.with(|m| {
        let mut m = m.borrow_mut();
        for item in &file.items {
            match item {
                Item::Impl(id) => {
                    for it in &id.items {
                        if let crate::parser::ImplItem::Method(fd) = it {
                            m.insert(fd.name.clone());
                        }
                    }
                }
                Item::Trait(td) => {
                    for it in &td.items {
                        if let crate::parser::TraitItem::Method(fd) = it {
                            m.insert(fd.name.clone());
                        }
                    }
                }
                _ => {}
            }
        }
    });
}

pub(crate) fn pasang_param(mut f: VmFunction, fd: &FnDecl) -> VmFunction {
    f.params = fd.params.iter().map(param_name).collect();
    f.arity_min = fd.params.iter().filter(|p| p.default.is_none()).count() as u8;
    f.variadic = fd.params.last().map_or(false, |p| p.is_variadic);
    // Parameter pengumpul tidak pernah wajib: tanpa argumen sisa ia berisi
    // array kosong, bukan "tidak dapat argumen".
    if f.variadic && f.arity_min > 0 { f.arity_min -= 1; }
    f
}

/// Nomor urut untuk nama global fungsi bersarang.
pub(crate) fn nomor_bersarang() -> usize {
    use std::cell::Cell;
    thread_local!(static N: Cell<usize> = Cell::new(0));
    N.with(|n| { let v = n.get() + 1; n.set(v); v })
}

pub(crate) fn enum_ctor_lookup(name: &str) -> Option<(String, usize)> {
    ENUM_CTORS.with(|r| r.borrow().get(name).cloned())
}

/// Varian TANPA payload milik modul, dicari lewat nama telanjangnya. Registry
/// cuma memegang bentuk berkualifikasi untuk varian modul; di posisi POLA nama
/// telanjang yang tidak menguji apa pun adalah pengikat yang cocok dengan apa pun.
pub(crate) fn enum_ctor_unit_akhiran(nama: &str) -> Option<(String, usize)> {
    let akhir = format!("::{}", nama);
    ENUM_CTORS.with(|r| {
        r.borrow().iter()
            .find(|(k, v)| v.1 == 0 && k.ends_with(&akhir))
            .map(|(_, v)| v.clone())
    })
}

pub(crate) fn enum_struct_ctor_lookup(name: &str) -> Option<(String, Vec<String>)> {
    ENUM_STRUCT_CTORS.with(|r| r.borrow().get(name).cloned())
}

pub(crate) fn struct_def_lookup(name: &str) -> Option<StructDefInfo> {
    STRUCT_DEFS.with(|defs| defs.borrow().get(name).cloned())
}

pub(crate) fn collect_enum_ctors(file: &ParsedFile, namespace: Option<&str>) {
    let terlihat = tipe_terlihat();
    let mut m: HashMap<String, (String, usize)> = terlihat.enum_ctor;
    let mut struct_m: HashMap<String, (String, Vec<String>)> = terlihat.enum_struct;
    let mut milik = TipeModul { impor: awalan_impor(), ..Default::default() };
    // Bentuk TELANJANG dan bentuk BERKUALIFIKASI, keduanya — sama seperti enum
    // pengguna beberapa baris di bawah, yang selalu didaftarkan dua kali (`E::V` …
    for (bare, tipe, arity) in [
        ("Some", "Option", 1usize), ("None", "Option", 0),
        ("Ok", "Result", 1),        ("Err", "Result", 1),
    ] {
        m.insert(bare.to_string(), (tipe.to_string(), arity));
        m.insert(format!("{}::{}", tipe, bare), (tipe.to_string(), arity));
    }
    for item in &file.items {
        if let Item::Enum(ed) = item {
            for v in &ed.variants {
                let arity = match &v.kind {
                    EnumVariantKind::Unit      => 0,
                    EnumVariantKind::Tuple(f)  => f.len(),
                    EnumVariantKind::Struct(fields) => {
                        let isi = (
                            ed.name.clone(),
                            fields.iter().map(|field| field.name.clone()).collect::<Vec<_>>(),
                        );
                        milik.enum_struct.insert(format!("{}::{}", ed.name, v.name), isi.clone());
                        struct_m.insert(format!("{}::{}", ed.name, v.name), isi.clone());
                        struct_m.insert(v.name.clone(), isi);
                        continue;
                    }
                };
                m.insert(format!("{}::{}", ed.name, v.name), (ed.name.clone(), arity));
                milik.enum_ctor.insert(format!("{}::{}", ed.name, v.name), (ed.name.clone(), arity));
                // Nama TELANJANG juga, sama seperti interpreter, yang mengikat tiap varian
                // dalam dua bentuk (`register_enum`).
                m.insert(v.name.clone(), (ed.name.clone(), arity));
            }
        }
    }
    ENUM_CTORS.with(|r| *r.borrow_mut() = m);
    ENUM_STRUCT_CTORS.with(|r| *r.borrow_mut() = struct_m);
    if let Some(ns) = namespace {
        TIPE_MODUL.with(|t| {
            let mut t = t.borrow_mut();
            let e = t.entry(ns.to_string()).or_default();
            e.enum_ctor = milik.enum_ctor;
            e.enum_struct = milik.enum_struct;
            e.impor = milik.impor;
        });
    }
}

pub(crate) fn collect_struct_defs(file: &ParsedFile, namespace: Option<&str>) {
    let mut defs = tipe_terlihat().struct_def;
    let mut milik = HashMap::new();
    for item in &file.items {
        if let Item::Struct(decl) = item {
            let info = StructDefInfo { parent: decl.parent.clone(), fields: decl.fields.clone() };
            milik.insert(decl.name.clone(), info.clone());
            defs.insert(decl.name.clone(), info);
        }
    }
    if let Some(ns) = namespace {
        TIPE_MODUL.with(|t| t.borrow_mut().entry(ns.to_string()).or_default().struct_def = milik);
    }
    // Sebagian kecil dari tabel ini juga dipublikasikan ke registri PROSES,
    // bukan thread — VM memakainya saat menjalankan program, dan goroutine …
    crate::vm::visibilitas::pasang_dari_deklarasi(
        file.items.iter().filter_map(|i| match i {
            Item::Struct(d) => Some(d),
            _ => None,
        }),
    );
    STRUCT_DEFS.with(|registry| *registry.borrow_mut() = defs);
}

pub(crate) fn resolved_struct_fields(name: &str) -> Vec<crate::parser::StructField> {
    let mut chain = Vec::new();
    let mut current = Some(name.to_string());
    while let Some(type_name) = current {
        if chain.iter().any(|seen: &String| seen == &type_name) || chain.len() >= 64 {
            break;
        }
        current = struct_def_lookup(&type_name).and_then(|info| info.parent);
        chain.push(type_name);
    }

    let mut fields: Vec<crate::parser::StructField> = Vec::new();
    for type_name in chain.into_iter().rev() {
        if let Some(info) = struct_def_lookup(&type_name) {
            for field in info.fields {
                if let Some(index) = fields.iter().position(|existing| existing.name == field.name) {
                    fields[index] = field;
                } else {
                    fields.push(field);
                }
            }
        }
    }
    fields
}

/// Nama fungsi yang butuh manggil balik closure milik user — dikompilasi jadi
/// `OpCode::HofCall` (lihat komentarnya di opcode.rs), bukan `Call` biasa …
pub(crate) fn is_hof_name(name: &str) -> bool {
    super::hof::layanan_hof(name).is_some()
}

pub(crate) fn is_hof_method(name: &str) -> bool {
    matches!(
        name,
        "map"
            | "filter"
            | "reduce"
            | "fold"
            | "any"
            | "all"
            | "find"
            | "map_err"
            | "and_then"
            | "partition"
            | "group_by"
            | "zip_with"
            // Kedelapan berikut memanggil sebuah fungsi JRG per elemen, jadi mereka
            // harus lewat jalur HOF — sebuah builtin biasa tidak bisa memanggil balik ke …
            | "each"
            | "count_if"
            | "find_idx"
            | "flat_map"
            | "min_by"
            | "max_by"
            | "sort_by"
            | "sort_by_key"
    )
}


impl Compiler {
    pub(crate) fn new(name: impl Into<String>, arity: u8) -> Self {
        Compiler { chunk: Chunk::new(name, arity), locals: Vec::new(), konsumsi_penerima: None, slot_tertangkap: Default::default(),
                   scope_depth: 0, loops: Vec::new(), upvals: Vec::new(),
                   saudara: Vec::new(), temps: 0 }
    }

    // ── Public entry points ───────────────────────────────────────────────

    pub fn compile_file(file: &ParsedFile, impor: &PetaImpor) -> CR<Chunk> {
        Self::compile_unit(file, None, true, impor).map(|(c, _)| c)
    }

    /// Seperti `compile_file`, tapi TIDAK memanggil `main` di akhir.
    pub fn compile_file_tanpa_main(file: &ParsedFile, impor: &PetaImpor) -> CR<Chunk> {
        Self::compile_unit(file, None, false, impor).map(|(c, _)| c)
    }

    /// `awalan`: awalan nama global modul ini, unik per SUMBER (bukan per alias).
    pub fn compile_module(file: &ParsedFile, awalan: &str, impor: &PetaImpor) -> CR<(Chunk, InfoModul)> {
        Self::compile_unit(file, Some(awalan), false, impor)
    }

    pub(crate) fn compile_unit(
        file: &ParsedFile,
        namespace: Option<&str>,
        call_main: bool,
        impor: &PetaImpor,
    ) -> CR<(Chunk, InfoModul)> {
        // Unit dipasang PALING AWAL: tabel tipe, penjelasan, dan tiap badan fungsi di bawah
        // membaca nama miliknya dan import-nya dari sini.
        let mut milik: HashMap<String, Milik> = HashMap::new();
        for item in &file.items {
            match item {
                Item::Fn(fd) => { milik.insert(fd.name.clone(), Milik::Fungsi); }
                Item::Const(cd) => { milik.insert(cd.name.clone(), Milik::Tetap); }
                Item::Var(vd) => { milik.insert(vd.name.clone(), Milik::Var); }
                Item::Extern(eb) => for ei in &eb.items {
                    if let ExternItem::Fn { name, .. } = ei { milik.insert(name.clone(), Milik::Fungsi); }
                },
                _ => {}
            }
        }
        // Cuma modul yang benar-benar dimuat: `import "std/math" as m` tidak memuat apa pun
        // (`math::` bawaan), jadi menyuruh menulis `m::…` di sana menyesatkan.
        let beralias: HashMap<String, String> = file.imports.iter().filter_map(|i| {
            let alias = i.alias.clone()?;
            let asli = crate::module::ModuleResolver::module_name(&i.path);
            (asli != alias && impor.contains_key(&alias)).then_some((asli, alias))
        }).collect();
        UNIT.with(|u| *u.borrow_mut() = Unit {
            awalan: namespace.map(|a| a.to_string()),
            milik,
            impor: impor.clone(),
            beralias,
        });
        // Pass 0: kumpulkan semua enum + variant Option/Result bawaan SEBELUM apa
        // pun lain dikompilasi, karena Pass 1/1b (di bawah) sudah langsung …
        collect_enum_ctors(file, namespace);
        collect_struct_defs(file, namespace);
        catat_metode_pemakai(file);
        let (bukan_nilai, varian_telanjang) = catat_bukan_nilai_berkas(file);
        pasang_penjelasan_impor(impor);
        // Jalur cepat `arr::push(a, x)` dkk mengenali fungsinya dari NAMA: sah selama unit
        // ini tidak mengimpor modul bernama `arr`, yang ekspornya menang atas bawaan.
        BOLEH_JALUR_ARR.with(|f| f.set(!impor.contains_key("arr")));

        let chunk_name = namespace
            .map(|name| format!("<module:{}>", name))
            .unwrap_or_else(|| "<script>".to_string());
        let mut c = Compiler::new(chunk_name, 0);
        // Yang terjangkau pengimpor sebagai `alias::NAMA`: padanan ekspor interpreter.
        let mut ekspor: HashMap<String, String> = HashMap::new();
        let mut catat_ekspor = |nama: &str, global: &str| {
            if !nama.starts_with('_') { ekspor.insert(nama.to_string(), global.to_string()); }
        };

        // Pass 1: hoist semua top-level fn sebagai globals. Nama TAMPIL-nya telanjang
        // (seperti acuan); nama globalnya ber-awalan modul, tanpa salinan telanjang.
        for item in &file.items {
            if let Item::Fn(fd) = item {
                let fn_chunk = Compiler::compile_fn(fd)?;
                let global_name = nama_global(namespace, &fd.name);
                let fn_val   = pasang_param(VmFunction::new(&fd.name, fd.params.len() as u8, fn_chunk), fd);
                let ci = c.chunk.add_constant(VmValue::Function(std::sync::Arc::new(fn_val)));
                let ni = c.chunk.add_name(&global_name);
                c.chunk.emit_arg(OpCode::Const,      ci);
                c.chunk.emit_arg(OpCode::DefGlobal,  ni);
                c.chunk.emit(OpCode::Pop);
                catat_ekspor(&fd.name, &global_name);
            }
        }

        // Varian enum milik MODUL jadi global ber-awalan (`bantu::Warna::Rgb`, `bantu::Rgb`),
        // padanan ekspor interpreter: unit jadi nilainya, ber-payload jadi konstruktor.
        // Unit utama tidak butuh ini — ia memakai konstanta saat kompilasi.
        if let Some(ns) = namespace {
            for item in &file.items {
                let Item::Enum(ed) = item else { continue };
                for v in &ed.variants {
                    let nilai = match &v.kind {
                        EnumVariantKind::Tuple(f) if !f.is_empty() =>
                            VmValue::ktor(&ed.name, &v.name, f.len()),
                        EnumVariantKind::Unit | EnumVariantKind::Tuple(_) =>
                            VmValue::varian(&ed.name, &v.name, None),
                        EnumVariantKind::Struct(_) => continue,
                    };
                    let ci = c.chunk.add_constant(nilai);
                    c.chunk.emit_arg(OpCode::Const, ci);
                    for nama in [format!("{}::{}", ed.name, v.name), v.name.clone()] {
                        let global = format!("{}::{}", ns, nama);
                        let ni = c.chunk.add_name(&global);
                        c.chunk.emit_arg(OpCode::DefGlobal, ni);
                        catat_ekspor(&nama, &global);
                    }
                    c.chunk.emit(OpCode::Pop);
                }
            }
        }

        // Pass 1b: hoist impl blocks → TypeName::method sebagai globals
        let mut direct_methods: HashMap<String, HashMap<String, FnDecl>> = HashMap::new();
        for item in &file.items {
            if let Item::Impl(impl_decl) = item {
                let type_name = type_expr_name(&impl_decl.self_type);
                let methods = direct_methods.entry(type_name).or_default();
                for impl_item in &impl_decl.items {
                    if let ImplItem::Method(fd) = impl_item {
                        methods.insert(fd.name.clone(), fd.clone());
                    }
                }
            }
        }

        let mut trait_defaults: HashMap<String, HashMap<String, FnDecl>> = HashMap::new();
        let trait_luar = tipe_terlihat().trait_def;
        let mut trait_milik: HashMap<String, TraitModul> = HashMap::new();
        let konteks_unit = file.items.iter().any(|i| matches!(i, Item::Trait(_)))
            .then(|| std::sync::Arc::new(konteks_kini()));
        for item in &file.items {
            if let Item::Trait(trait_decl) = item {
                let defaults = trait_defaults.entry(trait_decl.name.clone()).or_default();
                for trait_item in &trait_decl.items {
                    if let TraitItem::DefaultMethod(fd) = trait_item {
                        defaults.insert(fd.name.clone(), fd.clone());
                    }
                }
                let mut default: Vec<FnDecl> = defaults.values().cloned().collect();
                default.sort_by(|a, b| a.name.cmp(&b.name));
                if let Some(k) = &konteks_unit {
                    trait_milik.insert(trait_decl.name.clone(), TraitModul {
                        wajib: crate::parser::metode_abstrak(trait_decl), default, konteks: k.clone(),
                    });
                }
            }
        }
        // `impl` atas trait yang tidak terlihat, atau yang method abstraknya belum diisi:
        // ditolak saat unitnya JALAN (sebelum `main`), dengan lokasi `impl`-nya.
        for item in &file.items {
            let Item::Impl(impl_decl) = item else { continue };
            let wajib = |n: &str| trait_milik.get(n).or_else(|| trait_luar.get(n)).map(|t| t.wajib.clone());
            if let Some(pesan) = crate::parser::periksa_impl(impl_decl, wajib) {
                let lama = c.chunk.masuk_span(impl_decl.span);
                c.emit_tolak_sasaran(&crate::ruang_tipe::bersihkan(pesan));
                c.chunk.pulih_span(lama);
            }
        }

        let metode_luar = tipe_terlihat().metode;
        let mut emitted_methods: HashSet<(String, String)> = HashSet::new();
        for item in &file.items {
            if let Item::Impl(impl_decl) = item {
                let type_name = type_expr_name(&impl_decl.self_type);
                for impl_item in &impl_decl.items {
                    if let ImplItem::Method(fd) = impl_item {
                        let fn_name  = format!("{}::{}", type_name, fd.name);
                        // Titip tipe pemiliknya: VM tidak punya env, jadi
                        // "dari dalam tipe apa akses ini terjadi" harus
                        // ikut terkompilasi bersama badan methodnya.
                        let fn_chunk = Compiler::compile_fn_milik(fd, Some(type_name.clone()))?;
                        let arity    = fd.params.len() as u8;
                        let fn_val   = pasang_param(VmFunction::new(&fn_name, arity, fn_chunk), fd);
                        let ci = c.chunk.add_constant(VmValue::Function(std::sync::Arc::new(fn_val)));
                        let ni = c.chunk.add_name(&fn_name);
                        c.chunk.emit_arg(OpCode::Const,     ci);
                        c.chunk.emit_arg(OpCode::DefGlobal, ni);
                        c.chunk.emit(OpCode::Pop);
                        emitted_methods.insert((type_name.clone(), fd.name.clone()));
                    }
                }
            }
        }

        // Struct child mewarisi method parent terdekat yang tidak dioverride.
        for item in &file.items {
            let Item::Struct(struct_decl) = item else { continue };
            let child_name = struct_decl.name.clone();
            let mut current = struct_decl.parent.clone();
            let mut visited = HashSet::new();
            while let Some(parent_name) = current {
                if !visited.insert(parent_name.clone()) || visited.len() > 64 {
                    break;
                }
                // Induk dari unit LAIN: badan methodnya milik unit induknya (nama telanjang di
                // dalamnya merujuk modul itu), jadi fungsinya DIPAKAI, bukan dikompilasi ulang.
                // Dulu method induk dari modul lain tidak terjangkau sama sekali di VM.
                if let Some(nama) = metode_luar.get(&parent_name) {
                    let mut nama: Vec<&String> = nama.iter().collect();
                    nama.sort();
                    for method_name in nama {
                        // Yang di-`impl` berkas ini untuk induk itu dikompilasi blok di bawah.
                        let di_sini = direct_methods.get(&parent_name)
                            .is_some_and(|m| m.contains_key(method_name));
                        if di_sini || emitted_methods.contains(&(child_name.clone(), method_name.clone())) {
                            continue;
                        }
                        let asal = c.chunk.add_name(&format!("{}::{}", parent_name, method_name));
                        c.chunk.emit_arg(OpCode::LoadGlobal, asal);
                        let ni = c.chunk.add_name(&format!("{}::{}", child_name, method_name));
                        c.chunk.emit_arg(OpCode::DefGlobal, ni);
                        c.chunk.emit(OpCode::Pop);
                        emitted_methods.insert((child_name.clone(), method_name.clone()));
                    }
                }
                if let Some(methods) = direct_methods.get(&parent_name) {
                    for (method_name, fd) in methods {
                        if emitted_methods.contains(&(child_name.clone(), method_name.clone())) {
                            continue;
                        }
                        let fn_name = format!("{}::{}", child_name, method_name);
                        // Method WARISAN: yang berlaku adalah tipe anaknya,
                        // karena di situlah methodnya sekarang hidup — dan
                        // aturan visibilitas memang mengizinkan turunan.
                        let fn_chunk = Compiler::compile_fn_milik(fd, Some(child_name.clone()))?;
                        let fn_val = pasang_param(VmFunction::new(&fn_name, fd.params.len() as u8, fn_chunk), fd);
                        let ci = c.chunk.add_constant(VmValue::Function(std::sync::Arc::new(fn_val)));
                        let ni = c.chunk.add_name(&fn_name);
                        c.chunk.emit_arg(OpCode::Const, ci);
                        c.chunk.emit_arg(OpCode::DefGlobal, ni);
                        c.chunk.emit(OpCode::Pop);
                        emitted_methods.insert((child_name.clone(), method_name.clone()));
                    }
                }
                current = struct_def_lookup(&parent_name).and_then(|info| info.parent);
            }
        }

        // `impl Trait for Type` mendapat default method trait yang tidak
        // dioverride oleh impl langsung maupun method warisan.
        // Tipe anak ikut mendapatkannya, leluhur terdekat lebih dulu.
        let mut pasangan: Vec<(String, String)> = file.items.iter().filter_map(|item| {
            let Item::Impl(d) = item else { return None };
            let Some(TypeExpr::Named(t)) = &d.trait_ref else { return None };
            Some((type_expr_name(&d.self_type), t.name.clone()))
        }).collect();
        let langsung = pasangan.clone();
        for item in &file.items {
            let Item::Struct(sd) = item else { continue };
            let (mut kini, mut dilihat) = (sd.parent.clone(), HashSet::new());
            while let Some(induk) = kini {
                if !dilihat.insert(induk.clone()) || dilihat.len() > 64 { break; }
                for (t, tr) in &langsung {
                    if *t == induk { pasangan.push((sd.name.clone(), tr.clone())); }
                }
                kini = struct_def_lookup(&induk).and_then(|info| info.parent);
            }
        }
        for (type_name, nama_trait) in pasangan {
            // Trait milik unit lain: badannya dikompilasi dalam konteks unit ITU, ditambah
            // struct milik unit ini (untuk `Self { .. }`).
            let luar = (!trait_defaults.contains_key(&nama_trait))
                .then(|| trait_luar.get(&nama_trait)).flatten();
            let daftar: Vec<&FnDecl> = match (trait_defaults.get(&nama_trait), luar) {
                (Some(d), _) => d.values().collect(),
                (None, Some(t)) => t.default.iter().collect(),
                (None, None) => continue,
            };
            for fd in daftar {
                let method_name = &fd.name;
                if emitted_methods.contains(&(type_name.clone(), method_name.clone())) {
                    continue;
                }
                let fn_name = format!("{}::{}", type_name, method_name);
                // Method DEFAULT trait: badannya ditulis di trait, tapi ia dipasang untuk
                // tipe konkret ini — jadi dari sudut pandang visibilitas ia berjalan "di …
                let fn_chunk = match luar {
                    None => Compiler::compile_fn_milik(fd, Some(type_name.clone())),
                    Some(t) => {
                        let mut k = (*t.konteks).clone();
                        STRUCT_DEFS.with(|r| for (n, d) in r.borrow().iter() {
                            k.struct_def.entry(n.clone()).or_insert_with(|| d.clone());
                        });
                        let lama = pasang_konteks(k);
                        let hasil = Compiler::compile_fn_milik(fd, Some(type_name.clone()));
                        pasang_konteks(lama);
                        hasil
                    }
                }?;
                let fn_val = pasang_param(VmFunction::new(&fn_name, fd.params.len() as u8, fn_chunk), fd);
                let ci = c.chunk.add_constant(VmValue::Function(std::sync::Arc::new(fn_val)));
                let ni = c.chunk.add_name(&fn_name);
                c.chunk.emit_arg(OpCode::Const, ci);
                c.chunk.emit_arg(OpCode::DefGlobal, ni);
                c.chunk.emit(OpCode::Pop);
                emitted_methods.insert((type_name.clone(), method_name.clone()));
            }
        }

        // Pass 2: const / var top-level
        for item in &file.items {
            match item {
                Item::Const(cd) => {
                    c.compile_expr(&cd.value)?;
                    let global_name = nama_global(namespace, &cd.name);
                    let ni = c.chunk.add_name(&global_name);
                    c.chunk.emit_arg(OpCode::DefGlobal, ni);
                    c.chunk.emit(OpCode::Pop);
                    catat_ekspor(&cd.name, &global_name);
                }
                // `var` tingkat modul jadi SEL bersama: goroutine melihat perubahan pemiliknya
                // (dan sebaliknya), seperti di acuan — dulu tiap goroutine mendapat salinan.
                Item::Var(vd) => {
                    let nol = vd.ty.as_ref().and_then(|t| crate::parser::nilai_nol(t, vd.span));
                    if let Some(init) = vd.value.as_deref().or(nol.as_ref()) { c.compile_expr(init)?; }
                    else { c.chunk.emit(OpCode::Nil); }
                    let ni = c.chunk.add_name(&nama_global(namespace, &vd.name));
                    c.chunk.emit_arg(OpCode::DefVar, ni);
                    c.chunk.emit(OpCode::Pop);
                }
                // FFI `extern "C" { ...
                Item::Extern(eb) => {
                    let hasil_lib = crate::ffi::native::muat_pustaka_blok(&eb.abi, eb.lib.as_deref());
                    for it in &eb.items {
                        match it {
                            ExternItem::Fn { name, params, ret, .. } => {
                                let info = crate::ffi::native::resolve_extern_fn_abi(
                                    &eb.abi, name, params, ret, &hasil_lib,
                                );
                                let ci = c.chunk.add_constant(VmValue::ExternFn(
                                    std::sync::Arc::new(info),
                                ));
                                c.chunk.emit_arg(OpCode::Const, ci);
                                let global_name = nama_global(namespace, name);
                                let ni = c.chunk.add_name(&global_name);
                                c.chunk.emit_arg(OpCode::DefGlobal, ni);
                                c.chunk.emit(OpCode::Pop);
                                catat_ekspor(name, &global_name);
                            }
                            // Membaca/menulis variabel global pustaka luar belum didukung — sama
                            // seperti interpreter; penjelasannya dari `catat_bukan_nilai_berkas`.
                            ExternItem::Static { .. } => {}
                        }
                    }
                }
                _ => {}
            }
        }

        if call_main {
            // Nilai balik `main` diperiksa, bukan dibuang.
            if milik_unit("main").is_none() {
                c.emit_tolak(crate::interpreter::PESAN_TANPA_MAIN);
            } else if !matches!(milik_unit("main"), Some((_, Milik::Fungsi))) {
                c.emit_tolak(crate::interpreter::PESAN_MAIN_BUKAN_FN);
            } else {
                c.emit_bawaan("__periksa_hasil_main__")?;
                let ni = c.chunk.add_name("main");
                c.chunk.emit_arg(OpCode::LoadGlobal, ni);
                c.chunk.emit_arg(OpCode::Call, 0);
                c.chunk.emit_arg(OpCode::Call, 1);
                c.chunk.emit(OpCode::Pop);
            }
        }
        c.chunk.emit(OpCode::Halt);

        c.chunk.local_count = c.locals.len();

        // `alias::Tipe::metode` merujuk method yang sama dengan `Tipe::metode`, untuk tipe
        // yang dideklarasikan berkas ini — tipe bersifat global, jadi namanya polos.
        let tipe_sendiri: HashSet<&str> = file.items.iter().filter_map(|it| match it {
            Item::Struct(d) => Some(d.name.as_str()),
            Item::Enum(d) => Some(d.name.as_str()),
            Item::Trait(d) => Some(d.name.as_str()),
            _ => None,
        }).collect();
        for (tipe, metode) in &emitted_methods {
            if tipe_sendiri.contains(tipe.as_str()) {
                let nama = format!("{}::{}", tipe, metode);
                ekspor.insert(nama.clone(), nama);
            }
        }
        if let Some(ns) = namespace {
            let mut per_tipe: HashMap<String, Vec<String>> = HashMap::new();
            for (tipe, metode) in &emitted_methods {
                per_tipe.entry(tipe.clone()).or_default().push(metode.clone());
            }
            TIPE_MODUL.with(|t| {
                let mut t = t.borrow_mut();
                let e = t.entry(ns.to_string()).or_default();
                e.metode = per_tipe;
                e.trait_def = trait_milik;
            });
        }
        let mut info = InfoModul {
            awalan: namespace.unwrap_or("").to_string(),
            ekspor,
            bukan_nilai,
            varian_telanjang,
            contoh: crate::interpreter::contoh_isi_modul(file),
            ..Default::default()
        };
        for item in &file.items {
            let (nama, var) = match item {
                Item::Fn(fd) => (&fd.name, false),
                Item::Const(cd) => (&cd.name, false),
                Item::Var(vd) => (&vd.name, true),
                _ => continue,
            };
            if nama.starts_with('_') { info.privat.push(nama.clone()); }
            else if var { info.var.push(nama.clone()); }
        }
        Ok((c.chunk, info))
    }

    /// Dorong bawaan bernama `nama` lewat NILAINYA: penurunan sintaks (`a..b`, `x as T`)
    /// dulu mencari nama global, jadi `fn range` milik pemakai ikut terpanggil.
    pub(crate) fn emit_bawaan(&mut self, nama: &str) -> CR<()> {
        let v = super::bawaan::bawaan(nama).ok_or_else(|| CompileError::new(format!(
            "bawaan `{}` tidak terdaftar — ini bug di jrgc, bukan di programmu", nama)))?;
        let ci = self.chunk.add_constant(v);
        self.chunk.emit_arg(OpCode::Const, ci);
        Ok(())
    }

    pub fn compile_fn(fd: &FnDecl) -> CR<Chunk> {
        Self::compile_fn_milik(fd, None)
    }

    /// Seperti `compile_fn`, tapi tahu ia sedang mengompilasi method milik tipe
    /// apa.
    pub fn compile_fn_milik(fd: &FnDecl, tipe_self: Option<String>) -> CR<Chunk> {
        let mut c = Compiler::new(&fd.name, fd.params.len() as u8);
        c.chunk.tipe_self = tipe_self;
        c.scope_depth = 1;

        // Bind parameters sebagai locals slot 0..n
        for p in &fd.params {
            let name = param_name(p);
            let slot = c.locals.len();
            c.locals.push(Local { name, depth: 1, slot });
        }

        // Compile body
        Compiler::isi_badan_fn(&mut c, fd)?;
        c.chunk.local_count = c.locals.len();
        Ok(c.chunk)
    }

    /// Kompilasi badan sebuah `fn` ke dalam compiler `c` yang parameternya sudah
    /// terpasang.
    pub(crate) fn isi_badan_fn(c: &mut Compiler, fd: &FnDecl) -> CR<()> {
        // Prolog nilai default.
        for (i, p) in fd.params.iter().enumerate() {
            let Some(default) = &p.default else { continue };

            // Default hanya boleh menoleh ke KIRI.
            let disebut = crate::interpreter::capture::names_mentioned(default);
            for (j, kanan) in fd.params.iter().enumerate().skip(i + 1) {
                let nama_kanan = param_name(kanan);
                if disebut.contains(&nama_kanan) {
                    return Err(CompileError::new(format!(
                        "fungsi `{}`: nilai default parameter `{}` menyebut `{}`, yang \
                         posisinya ada di KANANNYA (posisi {} lawan {}) — waktu default \
                         ini dihitung, `{}` belum terisi. Pindahkan `{}` ke sebelah kiri \
                         `{}`, atau pakai nilai yang tidak bergantung padanya",
                        fd.name, param_name(p), nama_kanan, i + 1, j + 1,
                        nama_kanan, nama_kanan, param_name(p)
                    )));
                }
            }

            c.chunk.emit(OpCode::LoadArgc);
            let ci = c.chunk.add_constant(VmValue::Int(i as i128));
            c.chunk.emit_arg(OpCode::Const, ci);
            // argc > i berarti parameter ini memang diberikan → lewati.
            c.chunk.emit(OpCode::Gt);
            let lewati = c.chunk.emit_jump(OpCode::JumpTrue);
            c.dgn_temp(0, |cc| cc.compile_value_expr(default))?;
            c.chunk.emit_arg(OpCode::SimpanBuang, i as u32);
            c.chunk.patch_jump(lewati);
        }

        // Tail expression sebuah fungsi (baris terakhir tanpa `return`)
        // adalah NILAI BALIKNYA — bukan statement yang nilainya dibuang.
        match &*fd.body {
            FnBody::Block(blk) => {
                let saudara_lama = c.saudara.len();
                c.hoist_fn_bersarang(&blk.stmts)?;
                for s in &blk.stmts { c.compile_stmt(s)?; }
                if let Some(tail) = &blk.tail {
                    c.compile_expr(tail)?;
                    c.chunk.emit(OpCode::Return);
                } else {
                    c.chunk.emit(OpCode::ReturnNil);
                }
                c.saudara.truncate(saudara_lama);
            }
            FnBody::Expr(e) => {
                c.compile_expr(e)?;
                c.chunk.emit(OpCode::Return);
            }
            // Tidak ada badan untuk dikompilasi.
            FnBody::Abstrak => {
                return Err(CompileError::new(format!(
                    "`{}` dideklarasikan tanpa badan di trait-nya, jadi tidak ada yang \
                     bisa dikompilasi — `impl` yang dipakai belum menyediakannya",
                    fd.name
                )));
            }
        }
        Ok(())
    }

    // ── Block helpers ─────────────────────────────────────────────────────

    pub(crate) fn compile_block_stmts(&mut self, blk: &Block) -> CR<()> {
        let saudara_lama = self.saudara.len();
        self.hoist_fn_bersarang(&blk.stmts)?;
        for s in &blk.stmts { self.compile_stmt(s)?; }
        // Fungsi bersarang cuma terlihat di bloknya sendiri.
        self.saudara.truncate(saudara_lama);
        // tail expression (block value) — compile tapi langsung pop
        if let Some(tail) = &blk.tail {
            self.compile_expr(tail)?;
            self.chunk.emit(OpCode::Pop);
        }
        Ok(())
    }

    /// Daftarkan semua `fn` bersarang di blok ini SEBELUM statement mana pun
    /// dikompilasi.
    pub(crate) fn hoist_fn_bersarang(&mut self, stmts: &[Stmt]) -> CR<()> {
        let bersarang: Vec<&FnDecl> = stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::Fn(fd) => Some(fd),
                _ => None,
            })
            .collect();
        if bersarang.is_empty() {
            return Ok(());
        }
        // Pass 1: pasang namanya dulu, semua sekaligus.
        let mut dipasang = Vec::new();
        for fd in &bersarang {
            let unik = format!("__bersarang{}__{}", nomor_bersarang(), fd.name);
            self.saudara.push((fd.name.clone(), unik.clone()));
            dipasang.push(unik);
        }
        // Pass 2: baru kompilasi badannya, dengan peta yang sudah lengkap.
        for (fd, unik) in bersarang.iter().zip(dipasang) {
            let mut c = Compiler::new(&fd.name, fd.params.len() as u8);
            c.scope_depth = 1;
            c.saudara = self.saudara.clone();
            for p in &fd.params {
                let slot = c.locals.len();
                c.locals.push(Local { name: param_name(p), depth: 1, slot });
            }
            Compiler::isi_badan_fn(&mut c, fd)?;
            let fn_val = pasang_param(VmFunction::new(&fd.name, fd.params.len() as u8, c.chunk), fd);
            let ci = self.chunk.add_constant(VmValue::function(fn_val));
            self.chunk.emit_arg(OpCode::Const, ci);
            let ni = self.chunk.add_name(&unik);
            self.chunk.emit_arg(OpCode::DefGlobal, ni);
            self.chunk.emit(OpCode::Pop);
        }
        Ok(())
    }

    pub(crate) fn resolve_saudara(&self, name: &str) -> Option<String> {
        self.saudara.iter().rev().find(|(n, _)| n == name).map(|(_, g)| g.clone())
    }

    /// `if` in statement position — the original lowering, unchanged: each
    /// branch runs for effect only and nothing is left on the stack.
    pub(crate) fn compile_if_stmt(&mut self, is: &IfStmt) -> CR<()> {
        self.begin_scope();
        if let Some(init) = &is.init {
            self.compile_stmt(init)?;
        }
        self.compile_expr(&is.cond)?;
        let jf = self.chunk.emit_jump(OpCode::JumpFalse);

        self.begin_scope();
        self.compile_block_stmts(&is.then)?;
        self.end_scope();

        let has_alternative = !is.else_ifs.is_empty() || is.else_.is_some();
        let mut end_jumps = Vec::new();
        if has_alternative {
            end_jumps.push(self.chunk.emit_jump(OpCode::Jump));
        }
        self.chunk.patch_jump(jf);

        for (init, cond, blk) in &is.else_ifs {
            if let Some(init) = init {
                self.compile_stmt(init)?;
            }
            self.compile_expr(cond)?;
            let jf2 = self.chunk.emit_jump(OpCode::JumpFalse);
            self.begin_scope();
            self.compile_block_stmts(blk)?;
            self.end_scope();
            end_jumps.push(self.chunk.emit_jump(OpCode::Jump));
            self.chunk.patch_jump(jf2);
        }

        if let Some(else_blk) = &is.else_ {
            self.begin_scope();
            self.compile_block_stmts(else_blk)?;
            self.end_scope();
        }
        for jump in end_jumps {
            self.chunk.patch_jump(jump);
        }
        self.end_scope();
        Ok(())
    }

    /// `if` in expression position.
    pub(crate) fn compile_if_expr(&mut self, is: &IfStmt) -> CR<()> {
        // Outer scope only exists so an `if init; cond` header's init
        // statement can declare locals that the branches can see.
        self.begin_scope();
        if let Some(init) = &is.init {
            self.compile_stmt(init)?;
        }
        self.compile_expr(&is.cond)?;
        let jf = self.chunk.emit_jump(OpCode::JumpFalse);

        self.compile_block_as_value(&is.then)?;
        let mut end_jumps = vec![self.chunk.emit_jump(OpCode::Jump)];
        self.chunk.patch_jump(jf);

        for (init, cond, blk) in &is.else_ifs {
            if let Some(init) = init {
                self.compile_stmt(init)?;
            }
            self.compile_expr(cond)?;
            let jf2 = self.chunk.emit_jump(OpCode::JumpFalse);
            self.compile_block_as_value(blk)?;
            end_jumps.push(self.chunk.emit_jump(OpCode::Jump));
            self.chunk.patch_jump(jf2);
        }

        if let Some(else_blk) = &is.else_ {
            self.compile_block_as_value(else_blk)?;
        } else {
            // `if` tanpa `else` bernilai unit, bukan nil (lihat VmValue::Unit).
            self.chunk.emit(OpCode::Unit);
        }
        for jump in end_jumps {
            self.chunk.patch_jump(jump);
        }
        // Keeps the branch value while discarding any local the init
        // statement introduced.
        self.end_scope_keep_top();
        Ok(())
    }

    pub(crate) fn compile_block_as_value(&mut self, blk: &Block) -> CR<()> {
        self.begin_scope();
        // Blok-sebagai-nilai juga bisa memuat `fn` bersarang, dan `Stmt::Fn`
        // sendiri sudah tidak meng-emit apa pun (semuanya dikerjakan saat
        // hoist). Tanpa baris ini deklarasinya jadi no-op SENYAP.
        let saudara_lama = self.saudara.len();
        self.hoist_fn_bersarang(&blk.stmts)?;
        for s in &blk.stmts { self.compile_stmt(s)?; }
        if let Some(tail) = &blk.tail {
            self.compile_expr(tail)?;
        } else {
            // Blok tanpa ekspresi ekor bernilai unit, bukan nil.
            self.chunk.emit(OpCode::Unit);
        }
        self.saudara.truncate(saudara_lama);
        self.end_scope_keep_top();
        Ok(())
    }

    // ── Scope helpers ─────────────────────────────────────────────────────

    pub(crate) fn begin_scope(&mut self) { self.scope_depth += 1; }

    pub(crate) fn emit_runtime_cleanup(&mut self, keep_locals: usize) {
        for _ in keep_locals..self.locals.len() {
            self.chunk.emit(OpCode::Pop);
        }
    }

    pub(crate) fn end_scope(&mut self) {
        self.scope_depth -= 1;
        while self.locals.last().map(|l| l.depth > self.scope_depth).unwrap_or(false) {
            self.locals.pop();
            self.chunk.emit(OpCode::Pop);
        }
    }

    /// end_scope tapi jaga nilai di top-of-stack (untuk block expressions)
    /// end_scope tapi pertahankan nilai di top-of-stack.
    /// Pakai opcode SlideDown: pop semua locals, keep top value.
    pub(crate) fn end_scope_keep_top(&mut self) {
        self.scope_depth -= 1;
        let n = {
            let mut count = 0usize;
            for l in self.locals.iter().rev() {
                if l.depth > self.scope_depth { count += 1; } else { break; }
            }
            count
        };
        for _ in 0..n { self.locals.pop(); }
        if n > 0 {
            // SlideDown: jaga top-of-stack, pop n nilai di bawahnya
            self.chunk.emit_arg(OpCode::SlideDown, n as u32);
        }
    }

    /// Indeks stack tempat nilai BERIKUTNYA akan mendarat.
    pub(crate) fn slot_berikutnya(&self) -> usize { self.locals.len() + self.temps }

    pub(crate) fn define_local(&mut self, name: &str) {
        let slot = self.locals.len() + self.temps;
        self.locals.push(Local { name: name.to_string(), depth: self.scope_depth, slot });
    }

    /// Kompilasi `f` sementara menganggap ada `n` nilai sementara tambahan
    /// di stack. Dipakai di tiap tempat yang MENAHAN nilai di stack sambil
    /// mengompilasi sub-ekspresi lain.
    pub(crate) fn dgn_temp<T>(&mut self, n: usize, f: impl FnOnce(&mut Self) -> CR<T>) -> CR<T> {
        self.temps += n;
        let hasil = f(self);
        self.temps -= n;
        hasil
    }

    /// Kompilasi ekspresi sebagai NILAI yang diserahkan ke tempat lain.
    pub(crate) fn compile_value_expr(&mut self, e: &Expr) -> CR<()> {
        // Sejak fase copy-on-write, penyerahan nilai TIDAK lagi menyalin.
        self.compile_expr(e)
    }

    /// Indeks loop yang dituju `break`/`continue`.
    pub(crate) fn loop_tujuan(&self, label: Option<&str>, kata: &str) -> CR<usize> {
        match label {
            None => self
                .loops
                .len()
                .checked_sub(1)
                .ok_or_else(|| CompileError::new(format!("{} di luar loop", kata))),
            Some(l) => self
                .loops
                .iter()
                .rposition(|ctx| ctx.label.as_deref() == Some(l))
                .ok_or_else(|| {
                    CompileError::new(format!(
                        "`{} '{}` tidak menemukan loop berlabel `{}` yang membungkusnya \
                         — cek lagi penulisan labelnya, atau jangan-jangan sudah di luar \
                         loop itu",
                        kata, l, l
                    ))
                }),
        }
    }

    /// Catat penjelasan `name` kalau ia dideklarasikan tapi bukan nilai. Dipakai hanya
    /// kalau global bernama itu memang tidak ada saat jalan (`fn P` menang atas `struct P`).
    pub(crate) fn catat_bukan_nilai(&mut self, name: &str) {
        if self.chunk.bukan_nilai.iter().any(|(n, _)| n == name) { return; }
        if let Some(alasan) = bukan_nilai_lookup(name) {
            self.chunk.bukan_nilai.push((name.to_string(), alasan));
        }
    }

    /// Saran ejaan untuk nama telanjang yang tidak dikenal unit ini, dihitung SEKARANG:
    /// saat jalan nama milik modul sudah ber-awalan dan lokal sudah jadi slot. Lokal
    /// didahulukan (lebih dekat ke pembaca), lalu nama milik unit, import-nya, dan bawaan.
    pub(crate) fn catat_saran_nama(&mut self, name: &str) {
        if self.chunk.saran_lokal.iter().any(|(n, _)| n == name) { return; }
        // Lokal ber-awalan `__` adalah slot buatan kompilator, bukan nama yang ditulis.
        let lokal: Vec<&str> = self.locals.iter().map(|l| l.name.as_str())
            .chain(self.upvals.iter().map(|u| u.name.as_str()))
            .chain(self.saudara.iter().map(|(n, _)| n.as_str()))
            .filter(|n| !n.starts_with("__"))
            .collect();
        let saran = crate::diag::saran_nama_terdekat(name, lokal.into_iter()).or_else(|| {
            let terlihat = nama_terlihat_unit();
            let daftar = crate::vm::daftar_builtin();
            crate::diag::saran_nama_terdekat(
                name, terlihat.iter().chain(daftar.iter()).map(|s| s.as_str()))
        });
        if let Some(s) = saran {
            self.chunk.saran_lokal.push((name.to_string(), s));
        }
    }

    pub(crate) fn catat_tingkat_atas(&mut self, name: &str) {
        if !self.chunk.tingkat_atas.iter().any(|n| n == name) {
            self.chunk.tingkat_atas.push(name.to_string());
        }
    }

    /// Tolak SAAT JALAN dengan kalimat yang sudah diketahui sekarang: keluaran sebelum
    /// baris ini tetap tercetak, seperti di acuan.
    pub(crate) fn emit_tolak(&mut self, pesan: &str) {
        let ci = self.chunk.add_constant(VmValue::str(pesan));
        self.chunk.emit_arg(OpCode::Const, ci);
        self.chunk.emit(OpCode::Panic);
    }

    /// `emit_tolak` untuk sasaran penugasan, dicatat supaya `--periksa` ikut melaporkannya.
    pub(crate) fn emit_tolak_sasaran(&mut self, pesan: &str) {
        self.emit_tolak(pesan);
        if let Some(i) = self.chunk.code.last() {
            self.chunk.sasaran_ditolak.push((pesan.to_string(), i.span));
        }
    }

    /// Kalimat untuk penugasan ke `name` yang bukan variabel di unit ini — padanan
    /// `Env::pesan_sasaran_tugas`, diputuskan dari yang diketahui saat kompilasi.
    pub(crate) fn pesan_sasaran_tugas(&self, name: &str) -> String {
        use crate::interpreter as it;
        let dipanggil = match milik_unit(name) {
            Some((_, Milik::Fungsi)) => Some(true),
            Some(_) => Some(false),
            None => match enum_ctor_lookup(name) {
                Some((_, arity)) => Some(arity > 0),
                None => super::bawaan::bawaan(name).map(|v| v.bisa_dipanggil()),
            },
        };
        match dipanggil {
            Some(d) => it::pesan_tugas_bukan_variabel(name, d),
            None => {
                let calon: Vec<String> = self.locals.iter().map(|l| l.name.clone())
                    .chain(self.upvals.iter().map(|u| u.name.clone()))
                    .filter(|n| !n.starts_with("__"))
                    .chain(var_unit())
                    .collect();
                let saran = crate::diag::saran_nama_terdekat(name, calon.iter().map(|s| s.as_str()));
                it::pesan_tugas_tanpa_deklarasi(name, saran.as_deref())
            }
        }
    }

    pub(crate) fn resolve_local(&self, name: &str) -> Option<usize> {
        self.locals.iter().rev().find(|l| l.name == name).map(|l| l.slot)
    }

    pub(crate) fn resolve_upval(&self, name: &str) -> Option<usize> {
        self.upvals.iter().position(|u| u.name == name)
    }

    /// Rencana tangkapan untuk sebuah closure yang muncul DI SINI.
    pub(crate) fn rencana_tangkapan(&self, params: &[String], body: &Expr) -> Vec<Upval> {
        let mut disebut: Vec<String> = crate::interpreter::capture::names_mentioned(body)
            .into_iter()
            .filter(|n| !params.contains(n))
            .collect();
        // `names_mentioned` mengembalikan `HashSet`, yang urutan iterasinya tidak
        // stabil.
        disebut.sort();

        let mut out = Vec::new();
        for n in disebut {
            if let Some(slot) = self.resolve_local(&n) {
                out.push(Upval { name: n, src: UpvalSrc::ParentLocal(slot as u32) });
            } else if let Some(i) = self.resolve_upval(&n) {
                out.push(Upval { name: n, src: UpvalSrc::ParentUpval(i as u32) });
            }
        }
        out
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────

pub(crate) fn param_name(p: &FnParam) -> String {
    pat_name(&p.pattern)
}

pub(crate) fn pat_name(pat: &Pattern) -> String {
    match pat {
        Pattern::Ident { name, .. } => name.clone(),
        Pattern::Wildcard(_)        => "_".to_string(),
        _ => "_".to_string(),
    }
}

/// Ekstrak nama tipe dari TypeExpr (untuk impl block dan struct literal)
pub(crate) fn type_expr_name(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Named(nt) => nt.name.clone(),
        TypeExpr::Primitive(prim) => format!("{:?}", prim).to_lowercase(),
        TypeExpr::Pointer(_) => "pointer".to_string(),
        TypeExpr::Array(_) => "array".to_string(),
        TypeExpr::Slice(_) => "slice".to_string(),
        TypeExpr::Map(_) => "map".to_string(),
        TypeExpr::Chan(_) => "chan".to_string(),
        TypeExpr::Fn(_) => "fn".to_string(),
        TypeExpr::Tuple(_) => "tuple".to_string(),
        TypeExpr::Ownership(owned) => type_expr_name(&owned.inner),
        TypeExpr::Never => "never".to_string(),
        TypeExpr::Inferred => "_".to_string(),
        TypeExpr::Error(_) => "<error>".to_string(),
    }
}

pub(crate) fn qualified_type_expr_name(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Named(nt) => match &nt.package {
            Some(package) => format!("{}::{}", package, nt.name),
            None => nt.name.clone(),
        },
        _ => type_expr_name(ty),
    }
}

/// Apa yang dipanggil VM untuk `x as T`. Lebar sempit membawa NOMOR sasarannya
/// ke `__ubah_bulat`, karena `int`/`uint` saja tidak tahu 300 tidak muat di `u8`.
pub(crate) enum SasaranCast { Builtin(&'static str), Lebar(usize), Pointer }

pub(crate) fn sasaran_cast(ty: &TypeExpr) -> Option<SasaranCast> {
    use crate::interpreter::konversi as k;
    match ty {
        TypeExpr::Primitive(p) => {
            // Nama yang ditulis ikut ke pesannya, jadi `i128` tidak boleh jadi `int`.
            if let Some(nomor) = k::nomor_sasaran_bulat(p) {
                return Some(match p {
                    PrimTy::Int  => SasaranCast::Builtin("int"),
                    PrimTy::Uint => SasaranCast::Builtin("uint"),
                    _ => SasaranCast::Lebar(nomor),
                });
            }
            Some(SasaranCast::Builtin(match p {
                PrimTy::F32 | PrimTy::F64 | PrimTy::F128 => "float",
                PrimTy::Bool => "bool",
                PrimTy::Str | PrimTy::String_ => "str",
                PrimTy::Rune => "rune",
                _ => return None,
            }))
        }
        TypeExpr::Named(nt) => crate::parser::prim_dari_nama(&nt.name)
            .and_then(|p| sasaran_cast(&TypeExpr::Primitive(p))),
        _ if k::sasaran_cast_sah(ty) => Some(SasaranCast::Pointer),
        _ => None,
    }
}

// ─── Public API ───────────────────────────────────────────────────────────

pub fn compile(file: &ParsedFile, impor: &PetaImpor) -> Result<Chunk, CompileError> {
    Compiler::compile_file(file, impor)
}

pub fn compile_tanpa_main(file: &ParsedFile, impor: &PetaImpor) -> Result<Chunk, CompileError> {
    Compiler::compile_file_tanpa_main(file, impor)
}

pub fn compile_module(
    file: &ParsedFile, awalan: &str, impor: &PetaImpor,
) -> Result<(Chunk, InfoModul), CompileError> {
    Compiler::compile_module(file, awalan, impor)
}
