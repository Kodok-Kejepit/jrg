// src/interpreter/module.rs — interpreter/module.rs
// Loader modul: `load_module` (resolve import path → parse+run sekali,

use std::collections::HashMap;

use crate::parser::UsePath;
use super::Interpreter;
use super::value::{Value, RuntimeError};

impl Interpreter {
    /// Galat pemuatan yang belum berlokasi menunjuk baris `import` yang memintanya.
    pub(crate) fn di_baris_import(mut e: RuntimeError, span: crate::lexer::Span) -> RuntimeError {
        if e.span.is_none() { e.span = Some(span); }
        e
    }

    /// Load modul dari import path. Inject semua simbol sebagai
    /// `mod_name::symbol`. Idempoten — modul yang sudah di-cache langsung
    /// di-inject ulang tanpa parse/run ulang.
    pub fn load_module(&mut self, import_path: &str, alias: Option<&str>) -> Result<(), RuntimeError> {
        let mod_name = alias
            .map(|a| a.to_string())
            .unwrap_or_else(|| crate::module::ModuleResolver::module_name(import_path));

        // ── Cari sumbernya DULU: identitas modul adalah sumbernya, bukan namanya ──
        // Berkunci nama, `b` yang mengimpor `y/util` dulu menerima `x/util` milik `main`.
        enum Sumber { Std(&'static str), Berkas(std::path::PathBuf) }
        let (id, sumber) = match crate::module::ModuleResolver::std_module_name(import_path) {
            Some(nama) => match crate::interpreter::stdlib_src::source_of(nama) {
                Some(src) => (crate::module::identitas_std(nama), Sumber::Std(src)),
                // Dulu APA PUN yang diawali `std/` dianggap "sudah bawaan" lalu dilewati
                // diam-diam, jadi `import "std/salah_ketik"` sukses tanpa memuat apa pun dan …
                None if crate::module::ModuleResolver::is_stdlib(import_path) => {
                    return Ok(());
                }
                None => {
                    return Err(RuntimeError::new(
                        crate::interpreter::stdlib_src::pesan_modul_std_tak_ada(import_path),
                    ));
                }
            },
            None => {
                let resolver = crate::module::ModuleResolver::new(self.search_paths.clone());
                // Berkas nyata menang atas namespace bawaan. `import "math"`
                // dulu dilewati SEBELUM ini, jadi `math.jrg` milik pemakai
                // hilang tanpa suara.
                if resolver.resolve(import_path).is_none()
                    && crate::module::ModuleResolver::is_stdlib(import_path)
                {
                    return Ok(());
                }
                // Daftar milik RESOLVER (sudah tanpa tempat ganda), dan kalimatnya
                // satu definisi dengan VM.
                let file_path = resolver.resolve(import_path)
                    .ok_or_else(|| RuntimeError::new(resolver.pesan_tak_ketemu(import_path)))?;
                (crate::module::identitas(&file_path), Sumber::Berkas(file_path))
            }
        };
        self.alias_dikenal.insert(mod_name.clone());
        self.impor_tipe.push((mod_name.clone(), id.clone()));
        let asli = crate::module::ModuleResolver::module_name(import_path);
        if asli != mod_name { self.nama_beralias.insert(asli.clone(), mod_name.clone()); }

        // ── Cache hit: modul yang sama dipasang lagi di bawah alias pengimpor ini ──
        if let Some(exports) = self.module_cache.get(&id).cloned() {
            self.inject_exports(&mod_name, &exports);
            if let Some(b) = self.bawaan_modul.get(&id).cloned() {
                self.pasang_bawaan_modul(&mod_name, b);
            }
            return Ok(());
        }

        // ── Circular dependency guard ────────────────────────────────────
        if let Some(mulai) = self.loading.iter().position(|(i, _)| *i == id) {
            // `loading` adalah TUMPUKAN, jadi urutannya bisa digambar apa adanya: `a ->
            // b -> a`.
            let mut rantai: Vec<String> = self.loading[mulai..].iter().map(|(_, n)| n.clone()).collect();
            rantai.push(mod_name.clone());
            return Err(RuntimeError::new(crate::module::pesan_impor_melingkar(&rantai)));
        }

        let (nama_sumber, src, dir_induk) = match sumber {
            Sumber::Std(src) => (id.clone(), src.to_string(), None),
            Sumber::Berkas(file_path) => {
                let isi = crate::module::baca_sumber(&file_path).map_err(|e| {
                    RuntimeError::new(format!("tidak bisa baca '{}': {}", file_path.display(), crate::host::galat_io(&e)))
                })?;
                let induk = file_path.parent().map(|p| p.to_path_buf());
                (file_path.display().to_string(), isi, induk)
            }
        };

        // Tiap modul dapat `file_id` sendiri (0 milik berkas utama) dan sumbernya
        // didaftarkan, supaya diagnostik dari dalam modul menunjuk baris di BERKAS …
        let file_id = crate::diag::register_module_source(nama_sumber.clone(), src.clone());
        let sf = crate::lexer::SourceFile {
            id: file_id,
            name: nama_sumber,
            src,
        };
        let galat_modul = |g: Vec<(String, crate::lexer::Span)>| {
            let (pesan, span) = crate::pipa::galat_parse_modul(&sf.src, &g);
            let mut e = RuntimeError::new(pesan);
            e.span = Some(span);
            e
        };
        let tokens = crate::lexer::lex(&sf).map_err(|e| galat_modul(vec![(e.msg, e.span)]))?;
        let (mut ast, parse_errs) = crate::parser::parse(tokens, false);
        if !parse_errs.is_empty() {
            return Err(galat_modul(parse_errs.into_iter().map(|e| (e.msg, e.span)).collect()));
        }

        // ── Direktori modul dicari LEBIH DULU untuk import miliknya sendiri ──
        // Ditaruh di belakang, `x/pakai` yang mengimpor `util` menerima `util` di akar
        // proyek, bukan `x/util` di sebelahnya.
        let mut sub_paths = self.search_paths.clone();
        if let Some(pb) = dir_induk {
            sub_paths.retain(|p| *p != pb);
            sub_paths.insert(0, pb);
        }

        // ── Jalankan di fresh interpreter ────────────────────────────────
        self.loading.push((id.clone(), mod_name.clone()));

        let mut mod_interp = Interpreter::new_with_paths(sub_paths);
        // Rantai modul yang sedang dimuat IKUT diturunkan ke interpreter anak.
        mod_interp.loading = self.loading.clone();
        // Modul yang sudah selesai dimuat juga diturunkan, supaya
        // dependency berbentuk berlian (`a` dan `b` sama-sama pakai `c`)
        // tidak mem-parse dan menjalankan `c` dua kali.
        mod_interp.module_cache = self.module_cache.clone();
        mod_interp.bawaan_modul = self.bawaan_modul.clone();
        mod_interp.ruang = self.ruang.clone();

        // Handle transitive imports di modul ini
        for imp in &ast.imports {
            mod_interp.load_module(&imp.path, imp.alias.as_deref())
                .map_err(|e| sebut_modul(Self::di_baris_import(e, imp.span), import_path))?;
        }
        self.ruang.lock().unwrap_or_else(|p| p.into_inner())
            .olah(&id, &asli, &mut ast, &mod_interp.impor_tipe)
            .map_err(|(pesan, span)| { let mut e = RuntimeError::new(pesan); e.span = Some(span); e })?;
        // Hasil pemuatan transitif dibawa balik ke induk, biar cache-nya
        // tumbuh bersama, bukan hilang bersama interpreter anak.
        for (k, v) in &mod_interp.module_cache {
            self.module_cache.entry(k.clone()).or_insert_with(|| v.clone());
        }
        for (k, v) in &mod_interp.bawaan_modul {
            self.bawaan_modul.entry(k.clone()).or_insert_with(|| v.clone());
        }
        // Alias yang dipakai modul ini untuk import-nya sendiri: dikenal, tapi BUKAN
        // milik pengimpor — tiap berkas cuma melihat yang diimpornya sendiri.
        for a in &mod_interp.alias_dikenal { self.alias_dikenal.insert(a.clone()); }
        for (k, v) in &mod_interp.alasan_jalur {
            self.alasan_jalur.entry(k.clone()).or_insert_with(|| v.clone());
        }
        // Nama yang benar-benar DIDEKLARASIKAN oleh file modul ini sendiri (bukan
        // diturunkan dari diff environment sebelum/sesudah hoist).
        let mut milik_modul: std::collections::HashSet<String> = std::collections::HashSet::new();
        // `var` tingkat modul adalah KEADAAN milik modulnya: tidak diekspor. Dulu yang
        // keluar potret nilainya saat import, jadi `a::HITUNG` basi begitu modulnya berubah.
        let mut var_modul: Vec<String> = Vec::new();
        let mut privat: Vec<String> = Vec::new();
        let catat = |nama: &String, privat: &mut Vec<String>| {
            if nama.starts_with('_') { privat.push(nama.clone()); }
        };
        for item in &ast.items {
            match item {
                crate::parser::Item::Fn(fd)    => { catat(&fd.name, &mut privat); milik_modul.insert(fd.name.clone()); }
                crate::parser::Item::Const(cd) => { catat(&cd.name, &mut privat); milik_modul.insert(cd.name.clone()); }
                crate::parser::Item::Var(vd)   => {
                    if vd.name.starts_with('_') { privat.push(vd.name.clone()); } else { var_modul.push(vd.name.clone()); }
                }
                // Varian enum diikat dua bentuk (lihat `register_enum`): `Nama::Varian` dan
                // `Varian` telanjang.
                crate::parser::Item::Enum(ed) => {
                    for v in &ed.variants {
                        milik_modul.insert(format!("{}::{}", ed.name, v.name));
                        milik_modul.insert(v.name.clone());
                    }
                }
                crate::parser::Item::Extern(block) => {
                    for ei in &block.items {
                        if let crate::parser::ExternItem::Fn { name, .. } = ei {
                            milik_modul.insert(name.clone());
                        }
                    }
                }
                _ => {}
            }
        }

        // Simbol dari import milik modul ini (`bersama::nilai` di dalam `kiri`) TIDAK dibawa
        // ke pengimpor: fungsi modul membawa lingkungannya sendiri, jadi tidak perlu — dan
        // membawanya membuat berkas melihat modul yang tidak pernah diimpornya.

        // Hoist semua definisi (fn, const, struct, impl, enum, extern)
        mod_interp.hoist_top_level(&ast)
            .map_err(|e| sebut_modul(e, import_path))?;

        // Varian enum modul juga lewat nama TIPE-nya (`Warna::Rgb`), seperti tipenya
        // sendiri (`Titik {}`) dan pola (`Warna::Rgb(r, g, b)`); yang telanjang tetap
        // ber-awalan modul (`bantu::Rgb`), karena ia nilai milik modul itu.
        let mut bawaan = super::BawaanModul {
            varian: std::mem::take(&mut mod_interp.varian_modul),
            bukan_nilai: Vec::new(),
            varian_telanjang: Vec::new(),
            var_modul,
            privat,
            tipe: ast.items.iter().filter_map(|it| match it {
                crate::parser::Item::Struct(d) => Some(d.name.clone()),
                crate::parser::Item::Enum(d) => Some(d.name.clone()),
                crate::parser::Item::Trait(d) => Some(d.name.clone()),
                _ => None,
            }).collect(),
            contoh: crate::interpreter::contoh_isi_modul(&ast),
            trait_wajib: mod_interp.trait_wajib.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        };
        for item in &ast.items {
            if let crate::parser::Item::Enum(ed) = item {
                for v in &ed.variants {
                    let q = format!("{}::{}", ed.name, v.name);
                    if let Some(nilai) = mod_interp.env.get(&q) { bawaan.varian.insert(q, nilai); }
                    if !matches!(v.kind, crate::parser::EnumVariantKind::Struct(_)) {
                        bawaan.varian_telanjang.push((v.name.clone(), ed.name.clone()));
                    }
                }
            }
        }
        // Juga lewat awalan modulnya: `bantu::Warna::V` menjelaskan seperti `Warna::V`.
        bawaan.bukan_nilai = std::mem::take(&mut mod_interp.non_value_names).into_iter().collect();
        self.bawaan_modul.insert(id.clone(), bawaan.clone());
        self.pasang_bawaan_modul(&mod_name, bawaan);

        // ── Bawa pulang yang bukan nilai ────────────────────────────────
        for (nama, def) in mod_interp.struct_defs {
            self.struct_defs.entry(nama).or_insert(def);
        }
        for (nama, f) in mod_interp.varian_struct_field {
            self.varian_struct_field.entry(nama).or_insert(f);
        }
        for (nama, pemilik) in mod_interp.struct_variant_owner {
            self.struct_variant_owner.entry(nama).or_insert(pemilik);
        }
        // Method dari `impl` di modul.
        for (tipe, ms) in mod_interp.methods.instance {
            let tujuan = self.methods.instance.entry(tipe).or_default();
            for (nama, f) in ms { tujuan.entry(nama).or_insert(f); }
        }
        for (tipe, ms) in mod_interp.methods.static_ {
            let tujuan = self.methods.static_.entry(tipe).or_default();
            for (nama, f) in ms { tujuan.entry(nama).or_insert(f); }
        }
        // `impl Trait for Type` di modul, supaya method default trait-nya
        // ikut terjangkau lewat jalur fallback di `eval_method`.
        for (tipe, traits) in mod_interp.type_traits {
            let tujuan = self.type_traits.entry(tipe).or_default();
            for t in traits {
                if !tujuan.contains(&t) { tujuan.push(t); }
            }
        }

        // Badan default trait milik modul: tanpanya `impl hewan::Bersuara for Kucing` cuma
        // mendapat method yang ditulisnya sendiri.
        for (tr, ms) in mod_interp.trait_defaults {
            let tujuan = self.trait_defaults.entry(tr).or_default();
            for (nama, f) in ms { tujuan.entry(nama).or_insert(f); }
        }

        // ── Kumpulkan exports ────────────────────────────────────────────
        let exports: HashMap<String, Value> = mod_interp.env.globals.lock().unwrap_or_else(|p| p.into_inner())
            .iter()
            .filter(|(k, _)| {
                milik_modul.contains(k.as_str())
                // Konvensi privat modul: nama diawali `_` tidak ikut keluar.
                && !k.starts_with('_')
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        // ── Cache + inject ───────────────────────────────────────────────
        self.loading.retain(|(i, _)| *i != id);
        self.module_cache.insert(id, exports.clone());
        self.inject_exports(&mod_name, &exports);

        // Catatan: dulu di sini ada `eprintln!("[jrg] loaded module ...")`.
        Ok(())
    }

    /// Pasang varian enum modul lewat nama tipenya dan penjelasan nama-bukan-nilainya di
    /// bawah `alias` pengimpor ini, tanpa menimpa milik pengimpor.
    fn pasang_bawaan_modul(&mut self, alias: &str, b: super::BawaanModul) {
        for (q, nilai) in b.varian {
            if self.env.get(&q).is_none() { self.env.set_global(&q, nilai.clone()); }
            self.varian_modul.entry(q).or_insert(nilai);
        }
        for (varian, tipe) in b.varian_telanjang {
            let alasan = crate::interpreter::pesan_varian_modul(&varian, &tipe, alias);
            self.non_value_names.entry(varian).or_insert(alasan);
        }
        for (nama, alasan) in b.bukan_nilai {
            self.non_value_names.entry(format!("{}::{}", alias, nama)).or_insert_with(|| alasan.clone());
            self.non_value_names.entry(nama).or_insert(alasan);
        }
        // Import LANGSUNG menang atas penjelasan yang terbawa dari modul lain.
        for nama in b.var_modul {
            self.alasan_jalur.insert(format!("{}::{}", alias, nama),
                crate::interpreter::pesan_var_modul(&nama, alias));
        }
        for nama in b.privat {
            self.alasan_jalur.insert(format!("{}::{}", alias, nama),
                crate::interpreter::pesan_privat_modul(&nama, alias));
        }
        for nama in b.tipe {
            self.tipe_modul.insert(format!("{}::{}", alias, nama));
        }
        for (tr, wajib) in b.trait_wajib { self.trait_wajib.entry(tr).or_insert(wajib); }
        self.contoh_modul.entry(alias.to_string()).or_insert(b.contoh);
    }

    /// Inject semua exports ke env sebagai "mod_name::symbol_name".
    fn inject_exports(&mut self, mod_name: &str, exports: &HashMap<String, Value>) {
        for (sym, val) in exports {
            let key = format!("{}::{}", mod_name, sym);
            self.env.set_global(&key, val.clone());
        }
        // Nama modul sendiri SENGAJA tidak di-bind ke variabel apa pun.
    }
}

/// Galat dari dalam sebuah modul: yang berlokasi diteruskan UTUH (baris lokasinya
/// sudah menyebut berkasnya); yang tanpa lokasi diberi nama modulnya.
fn sebut_modul(e: RuntimeError, modul: &str) -> RuntimeError {
    if e.span.is_some() { return e; }
    RuntimeError::new(format!("modul `{}`: {}", modul, e.message))
}

