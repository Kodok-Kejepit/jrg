// src/interpreter/mod.rs — penerjemah pohon: acuan semantik JRG
// Tree-Walking Interpreter — sekarang dipecah jadi beberapa file supaya

#![allow(
    dead_code, unused_variables, unused_imports,
    clippy::boxed_local, clippy::ptr_arg,
    clippy::single_match, clippy::collapsible_match
)]

mod value;
mod env;
mod method_registry;
pub mod arity;
pub mod eval_expr;
// Badan `impl Interpreter` untuk ekspresi tersebar di empat berkas ini —
// satu impl inheren boleh dipecah lintas modul dalam satu crate.
mod ekspresi_op;
mod akses;
mod metode_str;
mod metode;
pub(crate) mod eval_stmt;
// Badan `impl Interpreter` untuk pernyataan tersebar di empat berkas ini.
mod penugasan;
mod alur;
mod panggilan;
mod pola;
pub mod builtin;
pub use builtin::SegmenFormat;
mod module;
pub mod capture;
/// Pengurutan bersama dua mesin: stabil, dan pembandingnya boleh gagal.
pub mod urut;
pub mod tampil;
pub mod galat_index;
pub mod konversi;
pub mod operator;
pub mod tugas;
pub mod stdlib_src;

pub use value::*;
pub use urut::*;
pub use env::*;
pub use method_registry::*;

use std::collections::HashMap;
use std::fmt;
use std::io::{self, BufRead, Write};

use crate::parser::{
    AssignOp, BinaryOp, Block, BreakStmt, ContinueStmt, EnumDecl, EnumVariantKind,
    Expr, ExternBlock, ExternItem, ExternParam, FnBody,
    ForStmt, IfStmt, ImplItem, Item, LetStmt, LitKind, LoopStmt,
    MatchArm, MatchBody, MatchStmt, NamedType, OwnershipQual, ParsedFile, Pattern, PrimTy,
    ReturnStmt, SelectCase, SelectStmt, ShortDeclStmt, SpawnStmt, Stmt, TypeExpr,
    UnaryOp, UsePath, VarStmt, WhileStmt,
    StructDecl, StructField, TraitDecl, TraitItem, Visibility,
};

#[allow(unused_imports)]
use crate::parser::ImplDecl;

/// Info struct decl yang disimpan supaya pewarisan field (`struct Dog: Animal
/// { ...
#[derive(Debug, Clone)]
pub struct StructDefInfo {
    pub parent: Option<String>,
    pub fields: Vec<StructField>,
}

/// Yang dibawa pulang satu modul selain nilai ekspornya: varian ber-tipe (`Warna::Rgb`)
/// dan bahan penjelasan nama-bukan-nilai. Kalimat yang menyebut ALIAS disusun saat
/// dipasang, karena tiap pengimpor boleh memberi modul yang sama alias yang berbeda.
#[derive(Clone, Default)]
pub struct BawaanModul {
    pub varian: HashMap<String, Value>,
    /// `(nama, alasan)` milik modulnya sendiri: dipasang telanjang dan ber-awalan alias.
    pub bukan_nilai: Vec<(String, String)>,
    /// `(varian, enum)` untuk varian telanjang milik modul.
    pub varian_telanjang: Vec<(String, String)>,
    /// `var` tingkat modul: keadaan milik modulnya, tidak diekspor.
    pub var_modul: Vec<String>,
    /// Nama ber-awalan `_`: privat milik modulnya.
    pub privat: Vec<String>,
    /// Tipe (struct/enum/trait) yang dideklarasikan berkas modulnya sendiri.
    pub tipe: Vec<String>,
    /// Contoh isi untuk kalimat "itu nama modul".
    pub contoh: String,
    /// Trait yang terlihat dari modul ini (miliknya dan milik import-nya) → method abstraknya.
    pub trait_wajib: Vec<(String, Vec<String>)>,
}

/// Satu pemanggilan yang ditunda oleh `defer`.
#[derive(Debug, Clone)]
pub enum DeferredCall {
    /// `defer f(a, b)`
    Fn { callee: Value, args: Vec<Value> },
    /// `defer obj.m(a, b)`
    Method { obj: Value, method: String, args: Vec<Value> },
}

// ---------------------------------------------------------------------------
// Interpreter
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Interpreter {
    pub env:     Env,
    pub methods: MethodRegistry,
    /// Maps a struct-variant's bare name (e.g.
    pub struct_variant_owner: crate::hash_cepat::PetaCepat<String, String>,
    /// Field yang dideklarasikan tiap varian ber-field (`enum E { V { a: int } }`).
    pub varian_struct_field: crate::hash_cepat::PetaCepat<String, Vec<String>>,
    /// Deklarasi tiap struct (field + induk pewarisan opsional), dipakai untuk:
    /// (1) mengisi field induk otomatis saat konstruksi `Expr::Struct` (lihat …
    pub struct_defs: crate::hash_cepat::PetaCepat<String, std::sync::Arc<StructDefInfo>>,
    /// Nama yang DIKENAL tapi bukan nilai: nama tipe (struct/enum) dan `extern
    /// static` yang belum didukung.
    pub non_value_names: HashMap<String, String>,
    /// Nama modul → contoh isinya untuk kalimat "itu nama modul" (`contoh_isi_modul`).
    pub contoh_modul: HashMap<String, String>,
    /// Varian enum milik modul yang diimpor (transitif), lewat nama TIPE-nya: `Warna::Rgb`.
    pub varian_modul: HashMap<String, Value>,
    /// Yang dibawa pulang tiap modul yang sudah dimuat, supaya impor yang kena CACHE
    /// (modul yang sama diimpor dua modul) tetap memasangnya.
    pub bawaan_modul: HashMap<String, BawaanModul>,
    /// Tumpukan `defer` per panggilan fungsi (lihat `Stmt::Defer`).
    pub defer_frames: Vec<Vec<DeferredCall>>,
    /// Nama yang disebut badan tiap closure, per badan (kuncinya alamat `Arc`-nya; `Arc`
    /// ikut disimpan supaya alamat itu tidak dipakai ulang). Dulu dihitung ulang tiap closure dibuat.
    pub(crate) nama_closure: HashMap<usize, (std::sync::Arc<Expr>, std::sync::Arc<Vec<String>>)>,
    /// Method default dari `trait Foo { fn bar(self) { ...
    pub trait_defaults: crate::hash_cepat::PetaCepat<String, crate::hash_cepat::PetaCepat<String, JrgFunction>>,
    /// Tipe → daftar nama trait yang di-`impl`-nya (`impl Foo for Bar`).
    pub type_traits: crate::hash_cepat::PetaCepat<String, Vec<String>>,
    /// Tiap trait yang terlihat → method abstraknya (`parser::periksa_impl`).
    pub trait_wajib: crate::hash_cepat::PetaCepat<String, Vec<String>>,
    /// Every goroutine spawned so far that hasn't been joined yet (`spawn { ...
    pub spawn_handles: std::sync::Arc<std::sync::Mutex<Vec<std::thread::JoinHandle<()>>>>,

    // ── Module System (Tier 2) ──────────────────────────────────────────
    /// Cache modul yang sudah dimuat: IDENTITAS sumbernya → {simbol → nilai}. Berkunci
    /// sumber, bukan nama: dua berkas bernama sama adalah dua modul.
    module_cache: HashMap<String, HashMap<String, Value>>,
    /// Penjelasan untuk jalur `alias::nama` yang dikenal tapi tidak bisa dibaca dari luar
    /// (`var` modul, nama ber-awalan `_`).
    pub alasan_jalur: HashMap<String, String>,
    /// Alias tiap modul yang dimuat unit MANA PUN — untuk menjelaskan `x::f` yang gagal
    /// karena `x` diimpor berkas lain, bukan berkas ini.
    pub alias_dikenal: std::collections::HashSet<String>,
    /// Nama asli → alias untuk import ber-`as` milik berkas ini: `set::new` sesudah
    /// `import "std/set" as s` menyebut `s::new`.
    pub nama_beralias: HashMap<String, String>,
    /// `alias::Tipe` untuk tiap tipe milik modul yang diimpor unit ini — pintu
    /// `alias::Tipe::metode`.
    pub tipe_modul: std::collections::HashSet<String>,
    /// Pemilik tiap nama tipe; satu untuk seluruh program (`crate::ruang_tipe`).
    pub ruang: crate::ruang_tipe::RuangBersama,
    /// `(alias, identitas)` import milik berkas ini sendiri.
    pub impor_tipe: Vec<(String, String)>,
    /// Search paths untuk resolusi import
    pub search_paths: Vec<std::path::PathBuf>,
    /// Guard circular dependency Tumpukan modul yang sedang dimuat, dari yang
    /// paling luar: `(identitas, nama tampil)`.
    loading: Vec<(String, String)>,
    /// `const`/`let`/`var` tingkat atas yang barisnya belum dijalankan.
    pub(crate) belum_diisi: std::collections::HashSet<String>,
    /// Kedalaman pemanggilan fungsi yang sedang aktif — lihat
    /// `MAX_CALL_DEPTH` dan penjagaannya di `call_jrg`.
    pub(crate) call_depth: usize,
    /// `ruang` badan yang sedang berjalan (lihat `JrgFunction::ruang`).
    pub(crate) ruang_kini: Option<std::sync::Arc<String>>,
    /// Berapa hook tampilan sedang bersarang. Lihat `tampil::MAX_SARANG_HOOK`.
    pub(crate) sarang_hook: usize,
}


impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            env: Env::new(),
            methods: MethodRegistry::new(),
            struct_variant_owner: Default::default(),
            varian_struct_field: Default::default(),
            struct_defs: Default::default(),
            non_value_names: HashMap::new(),
            contoh_modul: HashMap::new(),
            varian_modul: HashMap::new(),
            bawaan_modul: HashMap::new(),
            defer_frames: Vec::new(),
            nama_closure: HashMap::new(),
            trait_defaults: Default::default(),
            type_traits: Default::default(),
            trait_wajib: Default::default(),
            spawn_handles: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            module_cache: HashMap::new(),
            alasan_jalur: HashMap::new(),
            alias_dikenal: std::collections::HashSet::new(),
            nama_beralias: HashMap::new(),
            tipe_modul: std::collections::HashSet::new(),
            ruang: Default::default(),
            impor_tipe: Vec::new(),
            search_paths: Vec::new(),
            loading: Vec::new(),
            belum_diisi: std::collections::HashSet::new(),
            call_depth: 0,
            ruang_kini: None,
            sarang_hook: 0,
        }
    }

    /// Batas kedalaman rekursi sebelum interpreter menyerah dengan diagnostik.
    pub const MAX_CALL_DEPTH: usize = 10_000;

    /// Stack untuk thread yang menjalankan program JRG.
    /// Profil dev memakai bingkai beberapa kali lebih besar, jadi cadangannya ikut besar.
    pub const INTERPRETER_STACK_SIZE: usize =
        if cfg!(debug_assertions) { 2048 } else { 256 } * 1024 * 1024;

    /// Buat interpreter dengan search paths untuk module resolution.
    pub fn new_with_paths(paths: Vec<std::path::PathBuf>) -> Self {
        let mut i = Self::new();
        i.search_paths = paths;
        i
    }

    /// Set search paths setelah konstruksi.
    pub fn set_search_paths(&mut self, paths: Vec<std::path::PathBuf>) {
        self.search_paths = paths;
    }

    // -----------------------------------------------------------------------
    // Entry point
    // -----------------------------------------------------------------------

    /// Jalankan berkas ini dalam **mode uji**: tiap fungsi bernama `test_*` atau
    /// `uji_*` dipanggil sekali, urut sumber, dan kegagalannya **ditangkap** …
    pub fn run_tests(&mut self, file: &ParsedFile, filter: Option<&str>, src: Option<&str>)
        -> Result<usize, RuntimeError>
    {
        let file = &self.siapkan_utama(file)?;
        self.hoist_top_level(file)?;

        let mut daftar = crate::uji::daftar_uji(file);
        if let Some(f) = filter { daftar.retain(|n| n.contains(f)); }
        if daftar.is_empty() {
            return Err(RuntimeError::new(crate::uji::tidak_ada_uji()));
        }

        let (mut lulus, mut gagal) = (0usize, 0usize);
        let mut indeks: Option<crate::uji::IndeksBaris> = None;
        for nama in &daftar {
            let mut baris: Option<usize> = None;
            let hasil = match self.env.get(nama) {
                Some(Value::Function(f)) => {
                    // Tiap test dapat frame `defer`-nya sendiri, sama seperti `main`.
                    self.defer_frames.push(Vec::new());
                    let r = self.call_jrg(f, Vec::new());
                    let r = self.run_defers(r);
                    match r {
                        Ok(_) | Err(Signal::Return(_)) => Ok(()),
                        Err(Signal::RuntimeError(e)) => {
                            // Lokasinya diambil dari `Span` yang sudah dibawa
                            // galatnya sejak lama — tidak ada mekanisme baru
                            // di sini, cuma dipakai.
                            baris = e.span.zip(src)
                                .map(|(sp, t)| indeks.get_or_insert_with(|| crate::uji::IndeksBaris::baru(t))
                                    .baris(sp.lo.0 as usize));
                            Err(e.message.clone())
                        }
                        Err(_) => Err(String::from(
                            "sinyal `break`/`continue` keluar dari fungsi test")),
                    }
                }
                // Sebuah nama ber-awalan test yang ternyata bukan fungsi
                // dilaporkan, bukan dilewati diam-diam: yang menulisnya
                // hampir pasti mengira ia dijalankan.
                Some(other) => Err(format!("bukan fungsi, tapi nilai bertipe `{}`", other.type_name())),
                None => Err(String::from("tidak ditemukan saat dijalankan")),
            };
            if hasil.is_ok() { lulus += 1; } else { gagal += 1; }
            crate::host::tulis_baris(&crate::uji::baris_hasil(nama, &hasil, baris));
        }
        crate::host::tulis_baris(&crate::uji::rangkuman(lulus, gagal));
        self.join_all_goroutines();
        Ok(gagal)
    }

    /// Muat import berkas utama, lalu jawab salinannya dengan nama tipe yang sudah
    /// diganti labelnya. Tipe berkas utama diklaim SEBELUM modulnya dimuat.
    fn siapkan_utama(&mut self, file: &ParsedFile) -> Result<ParsedFile, RuntimeError> {
        use crate::ruang_tipe::UNIT_UTAMA;
        self.ruang.lock().unwrap_or_else(|p| p.into_inner()).klaim(UNIT_UTAMA, UNIT_UTAMA, file);
        for imp in &file.imports {
            self.load_module(&imp.path, imp.alias.as_deref())
                .map_err(|e| Self::di_baris_import(e, imp.span))?;
        }
        let mut salinan = file.clone();
        self.ruang.lock().unwrap_or_else(|p| p.into_inner())
            .olah(UNIT_UTAMA, UNIT_UTAMA, &mut salinan, &self.impor_tipe)
            .map_err(|(pesan, span)| { let mut e = RuntimeError::new(pesan); e.span = Some(span); e })?;
        Ok(salinan)
    }

    pub fn run_file(&mut self, file: &ParsedFile) -> Result<(), RuntimeError> {
        // ── Proses top-level imports dulu (Tier 2 module system) ───────────
        let file = &self.siapkan_utama(file)?;
        self.hoist_top_level(file)?;
        let result = match self.env.get("main") {
            Some(Value::Function(func)) => {
                let mut ce = Env::with_globals(self.env.globals.clone());
                ce.push_scope();
                // `main()` dipanggil lewat jalur KHUSUS ini, bukan lewat `call_jrg` seperti
                // pemanggilan fungsi biasa — makanya frame `defer` juga harus dibuka manual …
                self.defer_frames.push(Vec::new());
                // `main` diprofilkan di sini, bukan lewat `call_jrg` — ia memang dipanggil
                // lewat jalur khusus di atas, dan itulah kenapa ia HILANG dari laporan …
                crate::profiler::enter("main");
                let hasil_body = self.exec_fn_body(&func.body, &mut ce);
                let hasil_body = self.run_defers(hasil_body);
                crate::profiler::exit();
                match hasil_body {
                    // Nilai balik `main` TIDAK boleh diabaikan begitu saja.
                    Ok(nilai) | Err(Signal::Return(nilai)) => match &nilai {
                        Value::Enum { type_name, variant, payload }
                            if variant == "Err" || variant == "None" || variant == "Error" =>
                        {
                            let isi = payload
                                .as_ref()
                                .map(|p| format!(": {}", p))
                                .unwrap_or_default();
                            Err(RuntimeError::new(format!(
                                "program berhenti karena `{}::{}` merambat keluar dari `main`{} \
                                 — tangani dengan `match` di dalam `main`, atau pakai \
                                 `.unwrap_or(...)`, kalau kegagalan ini memang boleh terjadi",
                                type_name, variant, isi
                            )))
                        }
                        _ => Ok(()),
                    },
                    Err(Signal::RuntimeError(e))   => Err(e),
                    // `break`/`continue` yang sampai ke sini berarti tidak ada loop yang mau
                    // menampungnya.
                    Err(Signal::Break(Some(l))) => Err(RuntimeError::new(format!(
                        "`break {}` tidak menemukan loop berlabel `{}` yang membungkusnya — cek lagi penulisan labelnya, atau jangan-jangan sudah di luar loop itu",
                        l, l
                    ))),
                    Err(Signal::Continue(Some(l))) => Err(RuntimeError::new(format!(
                        "`continue {}` tidak menemukan loop berlabel `{}` yang membungkusnya — cek lagi penulisan labelnya, atau jangan-jangan sudah di luar loop itu",
                        l, l
                    ))),
                    Err(Signal::Break(None)) => Err(RuntimeError::new(
                        "`break` dipakai di luar loop apa pun — tidak ada yang mau dihentikan"
                    )),
                    Err(Signal::Continue(None)) => Err(RuntimeError::new(
                        "`continue` dipakai di luar loop apa pun — tidak ada yang mau dilanjutkan"
                    )),
                }
            }
            Some(_) => Err(RuntimeError::new(PESAN_MAIN_BUKAN_FN)),
            None => Err(RuntimeError::new(PESAN_TANPA_MAIN)),
        };
        // Wait for every goroutine spawned during the run (directly from
        // `main`, or transitively from another goroutine) to finish,
        // whether or not `main` itself succeeded. See `join_all_goroutines`.
        self.join_all_goroutines();
        result
    }

    /// Waits for every goroutine spawned so far to finish.
    pub fn join_all_goroutines(&self) {
        loop {
            let handle = {
                let mut guard = self.spawn_handles.lock().unwrap_or_else(|p| p.into_inner());
                guard.pop()
            };
            match handle {
                Some(h) => { let _ = h.join(); }
                None => break,
            }
        }
    }

/// Ambil galat runtime dari sebuah `Signal` TANPA men-stringify-nya.
fn galat_item(s: Signal) -> RuntimeError {
    match s {
        Signal::RuntimeError(e) => e,
        // `return`/`break`/`continue` di posisi ini tidak bisa terjadi —
        // parser tidak menerimanya di luar badan fungsi/loop. Kalau toh
        // sampai, ia dilaporkan apa adanya alih-alih ditelan.
        lain => RuntimeError::new(format!("{}", lain)),
    }
}

    pub(crate) fn hoist_top_level(&mut self, file: &ParsedFile) -> Result<(), RuntimeError> {
        // Pass 1: register all top-level fn names first (forward reference support)
        for item in &file.items {
            if let Item::Fn(fd) = item {
                let f = self.make_fn_val(fd, &self.env.clone());
                self.env.define(&fd.name, f);
            }
        }
        // Pass 2: const, var, struct names, impl methods
        for item in &file.items {
            if let Item::Trait(td) = item {
                self.trait_wajib.insert(td.name.clone(), crate::parser::metode_abstrak(td));
            }
        }
        for item in &file.items {
            match item {
                Item::Const(cd) => { self.belum_diisi.insert(cd.name.clone()); }
                Item::Var(vd) => { self.belum_diisi.insert(vd.name.clone()); }
                _ => {}
            }
        }
        for item in &file.items {
            match item {
                Item::Fn(_) => { /* already done */ }

                Item::Const(cd) => {
                    let v = self.eval_expr(&cd.value, &mut self.env.clone())
                        .map_err(Self::galat_item)?;
                    self.env.define(&cd.name, v);
                    self.belum_diisi.remove(&cd.name);
                }

                Item::Var(vd) => {
                    let nol = vd.ty.as_ref().and_then(|t| crate::parser::nilai_nol(t, vd.span));
                    let v = if let Some(init) = vd.value.as_deref().or(nol.as_ref()) {
                        self.eval_expr(init, &mut self.env.clone())
                            .map_err(Self::galat_item)?
                    } else { Value::Nil };
                    self.env.define(&vd.name, v);
                    self.env.tandai_var(&vd.name);
                    self.belum_diisi.remove(&vd.name);
                }

                // Register struct type name in env as a nil placeholder.
                Item::Struct(sd) => {
                    self.non_value_names.insert(
                        sd.name.clone(), crate::interpreter::pesan_nama_struct(&sd.name));
                    self.struct_defs.insert(sd.name.clone(), std::sync::Arc::new(StructDefInfo {
                        parent: sd.parent.clone(),
                        fields: sd.fields.clone(),
                    }));
                }

                // Enums were previously parsed but completely inert at runtime: no variant
                // could ever be constructed, and every `Pattern::Enum`/`Pattern::Struct` …
                Item::Enum(ed) => {
                    self.register_enum(ed);
                }

                // `extern "C" { ...
                Item::Extern(block) => {
                    self.register_extern_block(block);
                }

                // Register impl methods into MethodRegistry + env for static calls
                Item::Impl(impl_decl) => {
                    let type_name = match &impl_decl.self_type {
                        TypeExpr::Named(n) => n.name.clone(),
                        other => format!("{:?}", other),
                    };
                    // `impl Trait for Type` — catat supaya `eval_method` bisa jatuh ke default
                    // method trait itu kalau `Type` (atau leluhurnya) tidak override sendiri.
                    if let Some(pesan) = crate::parser::periksa_impl(impl_decl, |n| self.trait_wajib.get(n).cloned()) {
                        let mut e = RuntimeError::new(crate::ruang_tipe::bersihkan(pesan));
                        e.span = Some(impl_decl.span);
                        return Err(e);
                    }
                    if let Some(TypeExpr::Named(tr)) = &impl_decl.trait_ref {
                        self.type_traits.entry(type_name.clone()).or_default().push(tr.name.clone());
                    }
                    let global_snap = self.env.clone();
                    for impl_item in &impl_decl.items {
                        if let ImplItem::Method(fn_decl) = impl_item {
                            let func = JrgFunction {
                                // Nama BERKUALIFIKASI (`Point::dist`), bukan `dist` telanjang.
                                name: format!("{}::{}", type_name, fn_decl.name),
                                params: fn_decl.params.iter().map(|p| match &p.pattern {
                                    Pattern::Ident { name, .. } => name.clone(),
                                    _ => "_".to_string(),
                                }).collect::<Vec<_>>().into(),
                                defaults: fn_decl.params.iter()
                                    .map(|p| p.default.as_ref().map(|b| (**b).clone()))
                                    .collect::<Vec<_>>().into(),
                                variadic: fn_decl.params.last()
                                    .map_or(false, |p| p.is_variadic),
                                body: std::sync::Arc::clone(&fn_decl.body),
                                env:  std::sync::Arc::new(global_snap.clone()),
                                // Method tidak punya "saudara seblok":
                                // namanya sudah dicari lewat dispatch
                                // method, bukan lewat scope.
                                group: None,
                                pemilik: None,
                                ruang: Some(std::sync::Arc::new(type_name.clone())),
                            };
                            // Instance method: first param named "self"
                            let is_instance = fn_decl.params.first().map(|p| matches!(
                                &p.pattern, Pattern::Ident { name, .. } if name == "self"
                            )).unwrap_or(false);

                            if is_instance {
                                self.methods.reg_instance(&type_name, &fn_decl.name, func);
                            } else {
                                // Static — also register in env as "TypeName::method"
                                self.methods.reg_static(&type_name, &fn_decl.name, func.clone());
                                let key = format!("{}::{}", type_name, fn_decl.name);
                                self.env.set_global(&key, Value::Function(func.into()));
                            }
                        }
                    }
                }

                // Module import: `use utils::helper` (Tier 2 module system)
                Item::Use(use_decl) => {
                    let import_path = match &use_decl.path {
                        UsePath::Simple(segs) => segs.join("/"),
                        UsePath::Aliased(segs, _) => segs.join("/"),
                        _ => String::new(),
                    };
                    let alias = match &use_decl.path {
                        UsePath::Aliased(_, a) => Some(a.as_str()),
                        _ => None,
                    };
                    if !import_path.is_empty() {
                        if let Err(e) = self.load_module(&import_path, alias) {
                            crate::host::galat_baris(&format!("warning: {}", e));
                        }
                    }
                }

                // `trait Foo { fn bar(self) { ...
                Item::Trait(td) => {
                    self.non_value_names.insert(
                        td.name.clone(), crate::interpreter::pesan_nama_trait(&td.name));
                    let global_snap = self.env.clone();
                    for item in &td.items {
                        if let TraitItem::DefaultMethod(fn_decl) = item {
                            let func = JrgFunction {
                                name: fn_decl.name.clone(),
                                params: fn_decl.params.iter().map(|p| match &p.pattern {
                                    Pattern::Ident { name, .. } => name.clone(),
                                    _ => "_".to_string(),
                                }).collect::<Vec<_>>().into(),
                                defaults: fn_decl.params.iter()
                                    .map(|p| p.default.as_ref().map(|b| (**b).clone()))
                                    .collect::<Vec<_>>().into(),
                                variadic: fn_decl.params.last()
                                    .map_or(false, |p| p.is_variadic),
                                body: std::sync::Arc::clone(&fn_decl.body),
                                env:  std::sync::Arc::new(global_snap.clone()),
                                // Method tidak punya "saudara seblok":
                                // namanya sudah dicari lewat dispatch
                                // method, bukan lewat scope.
                                group: None,
                                pemilik: None,
                                // Diisi tipe penerimanya oleh `resolusi_metode`.
                                ruang: None,
                            };
                            self.trait_defaults.entry(td.name.clone()).or_default()
                                .insert(fn_decl.name.clone(), func);
                        }
                    }
                }

                // TypeAlias, Mod — belum diimplementasi. (Enum, Extern,
                // dan Trait sudah ditangani di atas, bukan di sini.)
                _ => {}
            }
        }
        // Dulu ada "Pass 3" di sini yang nge-patch ULANG `env.scopes[0]` tiap
        // `JrgFunction`/method yang sudah didaftarkan, supaya fungsi yang dihoist …
        Ok(())
    }

    /// Registers one `enum` declaration's variants as real values (unit variants)
    /// or callable constructors (tuple variants), and records struct variants so …
    fn register_enum(&mut self, ed: &EnumDecl) {
        self.non_value_names.insert(
            ed.name.clone(),
            crate::interpreter::pesan_nama_enum(
                &ed.name, ed.variants.first().map(|v| v.name.as_str())),
        );
        for variant in &ed.variants {
            let qualified = format!("{}::{}", ed.name, variant.name);
            match &variant.kind {
                EnumVariantKind::Unit => {
                    let v = Value::Enum {
                        type_name: ed.name.clone(),
                        variant: variant.name.clone(),
                        payload: None,
                    };
                    self.env.set_global(&qualified, v.clone());
                    self.env.set_global(&variant.name, v);
                }
                EnumVariantKind::Tuple(types) => {
                    let arity = types.len();
                    if arity == 0 {
                        let v = Value::Enum {
                            type_name: ed.name.clone(),
                            variant: variant.name.clone(),
                            payload: None,
                        };
                        self.env.set_global(&qualified, v.clone());
                        self.env.set_global(&variant.name, v);
                    } else {
                        let ctor = Value::EnumCtor {
                            type_name: ed.name.clone(),
                            variant: variant.name.clone(),
                            arity,
                        };
                        self.env.set_global(&qualified, ctor.clone());
                        self.env.set_global(&variant.name, ctor);
                    }
                }
                EnumVariantKind::Struct(fields) => {
                    self.struct_variant_owner.insert(variant.name.clone(), ed.name.clone());
                    // Juga lewat nama lengkapnya: dua enum boleh punya varian bernama sama.
                    self.struct_variant_owner.insert(qualified.clone(), ed.name.clone());
                    // Dibangun lewat `Expr::Struct`. Menyebutnya sebagai nilai dulu menjawab
                    // `nil` (sebuah placeholder); sekarang dijelaskan cara membangunnya.
                    let nama: Vec<String> = fields.iter().map(|f| f.name.clone()).collect();
                    self.varian_struct_field.insert(variant.name.clone(), nama.clone());
                    self.varian_struct_field.insert(qualified.clone(), nama.clone());
                    let alasan = crate::interpreter::pesan_varian_berfield(&ed.name, &variant.name, &nama);
                    self.non_value_names.entry(variant.name.clone()).or_insert_with(|| alasan.clone());
                    self.non_value_names.insert(qualified.clone(), alasan);
                }
            }
        }
    }

    /// Registers one `extern "ABI" ["lib"] { ...
    fn register_extern_block(&mut self, block: &ExternBlock) {
        let lib_outcome = crate::ffi::native::muat_pustaka_blok(&block.abi, block.lib.as_deref());
        for item in &block.items {
            match item {
                ExternItem::Fn { name, params, ret, .. } => {
                    let info = crate::ffi::native::resolve_extern_fn_abi(
                        &block.abi, name, params, ret, &lib_outcome);
                    self.env.set_global(name, Value::ExternFn(info));
                }
                ExternItem::Static { name, .. } => {
                    // Reading or writing a foreign global variable in place isn't implemented
                    // yet — doing that safely means knowing the exact byte width and layout of …
                    self.non_value_names.insert(
                        name.clone(), crate::interpreter::pesan_extern_static(name));
                }
            }
        }
    }

    fn make_fn_val(&self, fd: &crate::parser::FnDecl, env: &Env) -> Value {
        self.make_fn_val_in_group(fd, env, None)
    }

    /// Seperti `make_fn_val`, tapi ikut mencatat saudara sebloknya —
    /// lihat `JrgFunction::group`.
    fn make_fn_val_in_group(
        &self,
        fd: &crate::parser::FnDecl,
        env: &Env,
        group: Option<std::sync::Arc<Vec<crate::parser::FnDecl>>>,
    ) -> Value {
        Value::Function(JrgFunction {
            name: fd.name.clone(),
            params: fd.params.iter().map(|p| match &p.pattern {
                Pattern::Ident { name, .. } => name.clone(),
                _ => "_".to_string(),
            }).collect::<Vec<_>>().into(),
            defaults: fd.params.iter()
                .map(|p| p.default.as_ref().map(|b| (**b).clone()))
                .collect::<Vec<_>>().into(),
            variadic: fd.params.last().map_or(false, |p| p.is_variadic),
            body: std::sync::Arc::clone(&fd.body),
            env:  std::sync::Arc::new(env.clone()),
            group,
            pemilik: None,
            ruang: None,
        }.into())
    }

    // -----------------------------------------------------------------------
    // Statements
    // -----------------------------------------------------------------------

}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run a fully-parsed JRG file: hoist top-level items, call main().
pub fn interpret(file: &ParsedFile) -> Result<(), RuntimeError> {
    let mut interp = Interpreter::new();
    interp.run_file(file)
}

/// Jalankan file dengan search paths untuk module resolution.
pub fn interpret_with_paths(
    file: &ParsedFile,
    search_paths: Vec<std::path::PathBuf>,
) -> Result<(), RuntimeError> {
    let hasil = run_on_interpreter_stack(move |file| {
        let mut interp = Interpreter::new_with_paths(search_paths);
        interp.run_file(file)
    }, file);
    // Sebelum siapa pun melapor: galat yang dicetak di layar alternatif ikut lenyap
    // saat layarnya ditinggalkan, dan di raw mode barisnya menuruni layar bertangga.
    crate::term::pulihkan();
    hasil
}

/// Seperti `interpret_with_paths`, tapi dalam mode uji. Mengembalikan
/// jumlah test yang gagal.
pub fn jalankan_uji_with_paths(
    file: &ParsedFile,
    search_paths: Vec<std::path::PathBuf>,
    filter: Option<&str>,
    src: Option<&str>,
) -> Result<usize, RuntimeError> {
    let filter = filter.map(|s| s.to_string());
    let src = src.map(|s| s.to_string());
    let hasil = uji_on_interpreter_stack(move |file| {
        let mut interp = Interpreter::new_with_paths(search_paths);
        interp.run_tests(file, filter.as_deref(), src.as_deref())
    }, file);
    crate::term::pulihkan();
    hasil
}

fn uji_on_interpreter_stack<F>(f: F, file: &ParsedFile) -> Result<usize, RuntimeError>
where
    F: FnOnce(&ParsedFile) -> Result<usize, RuntimeError> + Send,
{
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(Interpreter::INTERPRETER_STACK_SIZE)
            .spawn_scoped(s, || f(file))
            .expect("gagal membuat thread interpreter")
            .join()
            .unwrap_or_else(|_| Err(RuntimeError::new(
                "interpreter berhenti tak terduga saat menjalankan test")))
    })
}

/// Menjalankan `f` di thread ber-stack besar dan menunggu hasilnya.
fn run_on_interpreter_stack<F>(f: F, file: &ParsedFile) -> Result<(), RuntimeError>
where
    F: FnOnce(&ParsedFile) -> Result<(), RuntimeError> + Send,
{
    // `f` dibungkus `Option` supaya jalur fallback masih bisa memakainya
    // kalau thread gagal dibuat: `spawn_scoped` memindahkan closure-nya,
    // jadi tanpa ini `f` sudah dianggap habis terpakai di jalur itu.
    let mut slot = Some(f);
    let spawned = std::thread::scope(|scope| {
        let taken = slot.take().expect("slot terisi tepat sekali");
        match std::thread::Builder::new()
            .stack_size(Interpreter::INTERPRETER_STACK_SIZE)
            .spawn_scoped(scope, move || {
                let hasil = taken(file);
                // Profiler menumpuk di thread-local demi kecepatan, jadi hasil thread ini
                // harus DISETOR sebelum ia mati — kalau tidak, seluruh kerjanya hilang tanpa …
                crate::profiler::flush();
                hasil
            })
        {
            Ok(h) => Some(match h.join() {
                Ok(r) => r,
                // Panic di dalam thread interpreter berarti bug di
                // interpreter itu sendiri, bukan kesalahan program JRG —
                // dilaporkan sebagai error, bukan dibiarkan hilang.
                Err(_) => Err(RuntimeError::new(
                    "interpreter berhenti tidak wajar (panic internal) — ini bug di jrgc, bukan di program kamu",
                )),
            }),
            Err(_) => None,
        }
    });
    match spawned {
        Some(r) => r,
        // Thread gagal dibuat (mis. batas memori/ulimit).
        None => Err(RuntimeError::new(
            "tidak bisa membuat thread untuk menjalankan program (batas memori/thread OS?)",
        )),
    }
}

/// Evaluate a single expression in a fresh environment.
/// Useful for REPL / unit tests.
pub fn interpret_expr(expr: &Expr, env: &mut Env) -> EvalResult {
    let mut interp = Interpreter::new();
    interp.eval_expr(expr, env)
}

#[cfg(test)]
mod tests {
    use super::interpret;
    use crate::lexer::{lex, SourceFile};
    use crate::parser::parse;

    #[test]
    fn spawn_explicit_capture_list_uses_parent_values() {
        let source = SourceFile {
            id: 0,
            name: "<interpreter-test>".to_string(),
            src: r#"
package main

pub fn main() {
    let captured = 7
    let ch = make_chan(int, 1)
    spawn |captured: int, ch: chan int| {
        ch <- captured
    }
    let received = <-ch
    assert_eq(received, 7)
    assert(num_cpus() > 0)
}
"#
            .to_string(),
        };
        let tokens = lex(&source).expect("source harus bisa di-lex");
        let (file, errors) = parse(tokens, false);
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        interpret(&file).expect("explicit spawn capture harus memakai nilai parent scope");
    }

    /// `if` is expression-valued here too, so the three backends agree.
    /// Before this, a block ending in an `if` evaluated to `()` — which the
    /// LLVM backend then silently turned into `0`.
    #[test]
    fn if_expression_yields_branch_value() {
        let source = SourceFile {
            id: 0,
            name: "<interpreter-test>".to_string(),
            src: r#"
package main

enum E { A, B }

fn from_match(e: E, v: int) -> int {
    match e {
        E::A => { if v > 5 { 100 } else { 200 } }
        E::B => 7,
    }
}

fn chain(v: int) -> int {
    if v < 10 { 1 } else if v < 20 { 2 } else { 3 }
}

pub fn main() {
    assert_eq(from_match(E::A, 10), 100)
    assert_eq(from_match(E::A, 1), 200)
    assert_eq(from_match(E::B, 0), 7)

    assert_eq(chain(5), 1)
    assert_eq(chain(15), 2)
    assert_eq(chain(25), 3)

    let x = if true { 100 } else { 200 }
    assert_eq(x, 100)
    assert_eq(1 + if true { 10 } else { 20 }, 11)

    let s = if false { "no" } else { "yes" }
    assert_eq(s, "yes")

    let nested = { { 77 } }
    assert_eq(nested, 77)

    var side = 0
    if true { side = 5 }
    assert_eq(side, 5)
}
"#
            .to_string(),
        };
        let tokens = lex(&source).expect("source harus bisa di-lex");
        let (file, errors) = parse(tokens, false);
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        interpret(&file).expect("if sebagai ekspresi harus menghasilkan nilai cabangnya");
    }

    // ── pemulihan pemanggilan tanpa kurung ──────────────────────────────

    fn run_src(src: &str) -> Result<(), String> {
        let source = SourceFile {
            id: 0,
            name: "<interpreter-test>".to_string(),
            src: src.to_string(),
        };
        let tokens = lex(&source).map_err(|e| format!("lex: {}", e.msg))?;
        let (file, errors) = parse(tokens, false);
        if !errors.is_empty() {
            return Err(format!("parse: {:?}", errors.iter().map(|e| &e.msg).collect::<Vec<_>>()));
        }
        interpret(&file).map_err(|e| e.message)
    }

    /// Argumen harus benar-benar sampai ke fungsinya, bukan dibuang.
    #[test]
    fn parenless_call_keeps_its_argument() {
        run_src(
            r#"
package main

fn ambil(x: int) -> int { return x }

pub fn main() {
    let a = ambil 7
    assert_eq(a, 7)
}
"#,
        )
        .expect("pemanggilan tanpa kurung harus membawa argumennya");
    }

    /// Beberapa argumen dipisah koma juga ikut terbawa.
    #[test]
    fn parenless_call_supports_multiple_arguments() {
        run_src(
            r#"
package main

fn tambah(a: int, b: int) -> int { return a + b }

pub fn main() {
    let hasil = tambah 3, 4
    assert_eq(hasil, 7)
}
"#,
        )
        .expect("argumen kedua harus ikut terbawa");
    }

    /// ATURAN KERAS: bentuk yang ambigu tidak boleh ditebak.
    #[test]
    fn ambiguous_parenless_call_is_not_guessed() {
        let err = run_src(
            r#"
package main

fn ambil(x: int) -> int { return x }

pub fn main() {
    ambil -1
    wok("lanjut")
}
"#,
        )
        .expect_err("bentuk ambigu tidak boleh diam-diam ditebak");
        assert!(
            err.contains("fungsi") && err.contains("kurung"),
            "pesan harus menjelaskan soal kurung, aktual: {}",
            err
        );
    }

    /// Fungsi yang butuh argumen tapi ditulis telanjang sebagai statement
    /// dulu cuma diperingatkan lalu DILEWATI — pemanggilan yang tidak
    /// pernah terjadi, tanpa jejak. Sekarang itu error.
    #[test]
    fn bare_function_needing_arguments_is_an_error() {
        let err = run_src(
            r#"
package main

fn tambah(a: int, b: int) -> int { return a + b }

pub fn main() {
    tambah
    wok("lanjut")
}
"#,
        )
        .expect_err("fungsi telanjang yang butuh argumen harus error");
        assert!(err.contains("tambah"), "pesan harus menyebut nama fungsinya: {}", err);
    }

    /// Tapi memakai fungsi sebagai NILAI tetap sah — perbaikan di atas
    /// tidak boleh menutup first-class function.
    #[test]
    fn function_used_as_a_value_still_works() {
        run_src(
            r#"
package main

fn tambah(a: int, b: int) -> int { return a + b }

pub fn main() {
    let f = tambah
    assert_eq(f(2, 3), 5)
}
"#,
        )
        .expect("fungsi sebagai nilai harus tetap jalan");
    }
}
