// src/vm/mod.rs
// Bytecode VM — Public API

pub mod tampil;
pub mod opcode;
pub mod chunk;
pub mod ffi_jembatan;
pub mod ffi_asing;
pub mod value;
pub mod channel;
pub mod visibilitas;
pub mod compiler;
// Badan `impl Compiler` tersebar di tiga berkas ini, sama alasannya
// dengan `impl Vm` di atas.
mod kom_stmt;
mod kom_ekspresi;
pub(crate) mod kom_pola;
pub mod vm;
// Badan `impl Vm` tersebar di beberapa berkas — Rust mengizinkan satu
// impl inheren dipecah lintas modul dalam satu crate, dan itu yang
// membuat `vm.rs` bisa tinggal jadi rangkanya saja.
mod jalankan;
mod panggil;
pub(crate) mod info_native;
mod akses;
mod hof;
pub(crate) mod bawaan;

pub use compiler::{compile, CompileError};
pub use vm::{Vm, VmError};
pub use chunk::Chunk;
pub use value::{VmValue, VmFunction};
pub use channel::VmChannel;

use crate::parser::ParsedFile;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Kompilasi + eksekusi via Bytecode VM.
pub fn run_file(
    file:      &ParsedFile,
    emit_bc:   bool,
    profiling: bool,
) -> Result<(), VmError> {
    run_file_with_paths(file, Vec::new(), emit_bc, profiling)
}

/// Jalankan sebuah program lewat VM.
pub fn run_file_with_paths(
    file: &ParsedFile,
    search_paths: Vec<PathBuf>,
    emit_bc: bool,
    profiling: bool,
) -> Result<(), VmError> {
    let hasil = jalankan_berkas(file, search_paths, emit_bc, profiling);
    // Sama dengan `interpret_with_paths`: terminal kembali SEBELUM galatnya dilaporkan.
    crate::term::pulihkan();
    hasil
}

fn jalankan_berkas(
    file: &ParsedFile,
    search_paths: Vec<PathBuf>,
    emit_bc: bool,
    profiling: bool,
) -> Result<(), VmError> {
    let mut vm = Vm::new();
    if profiling { vm.enable_profiling(); }
    let (impor, file) = &Pemuat::new(emit_bc).muat_utama(&mut vm, file, &search_paths)?;
    let chunk = compile(file, impor).map_err(|e| VmError::new(e.to_string()))?;
    if emit_bc { chunk.disassemble(); }
    let hasil = vm.run(chunk);
    // Goroutine ditunggu SESUDAH program utama selesai — berhasil maupun gagal,
    // sama seperti `Interpreter::join_all_goroutines`.
    crate::vm::channel::join_all_goroutines();
    hasil
}

/// Jalankan berkas ini lewat VM dalam **mode uji**.
pub fn jalankan_uji_with_paths(
    file: &ParsedFile,
    search_paths: Vec<PathBuf>,
    filter: Option<&str>,
    src: Option<&str>,
) -> Result<usize, VmError> {
    let hasil = jalankan_uji(file, search_paths, filter, src);
    crate::term::pulihkan();
    hasil
}

fn jalankan_uji(
    file: &ParsedFile,
    search_paths: Vec<PathBuf>,
    filter: Option<&str>,
    src: Option<&str>,
) -> Result<usize, VmError> {
    let mut daftar = crate::uji::daftar_uji(file);
    if let Some(f) = filter { daftar.retain(|n| n.contains(f)); }
    if daftar.is_empty() {
        return Err(VmError::new(crate::uji::tidak_ada_uji()));
    }

    let mut vm = Vm::new();
    let (impor, file) = &Pemuat::new(false).muat_utama(&mut vm, file, &search_paths)?;
    // Tanpa pemanggilan `main`: chunk ini cuma mendefinisikan globalnya.
    let chunk = compiler::compile_tanpa_main(file, impor).map_err(|e| VmError::new(e.to_string()))?;
    vm.run(chunk)?;

    let (mut lulus, mut gagal) = (0usize, 0usize);
    let mut indeks: Option<crate::uji::IndeksBaris> = None;
    for nama in &daftar {
        let mut baris: Option<usize> = None;
        let hasil = match vm.globals.get(nama).cloned() {
            Some(f @ (VmValue::Function(_) | VmValue::Closure(_))) => {
                match vm.call_value_immediate(f, Vec::new()) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        // `Span` yang dibawa `VmError` sejak fase SPAN-VM.
                        baris = e.span.zip(src)
                            .map(|(sp, t)| indeks.get_or_insert_with(|| crate::uji::IndeksBaris::baru(t))
                                .baris(sp.lo.0 as usize));
                        Err(e.msg.clone())
                    }
                }
            }
            Some(other) => Err(format!("bukan fungsi, tapi nilai bertipe `{}`", other.type_name_penuh())),
            None => Err(String::from("tidak ditemukan saat dijalankan")),
        };
        if hasil.is_ok() { lulus += 1; } else { gagal += 1; }
        crate::host::tulis_baris(&crate::uji::baris_hasil(nama, &hasil, baris));
    }
    crate::host::tulis_baris(&crate::uji::rangkuman(lulus, gagal));
    Ok(gagal)
}

/// Setiap nama builtin yang benar-benar terdaftar, urut abjad.
pub fn daftar_builtin() -> Vec<String> {
    daftar_builtin_tersimpan().clone()
}

/// Sama, tapi cuma menanyakan KEANGGOTAAN — tanpa menyalin 658 `String`.
/// Panggil builtin `nama` langsung dengan nilai VM — pintu untuk jembatan backend LLVM.
/// `None` kalau namanya bukan fungsi bawaan.
pub fn panggil_bawaan(nama: &str, args: &[VmValue]) -> Option<Result<VmValue, String>> {
    match bawaan::bawaan(nama)? {
        VmValue::NativeFn(f) => Some(f(args)),
        _ => None,
    }
}

/// Panggil sebuah `extern fn` dari deskripsi teksnya (`ffi::native::deskripsi_extern`): pintu
/// backend LLVM lewat jembatan. Resolusinya sekali per deskripsi.
pub fn panggil_extern(deskripsi: &str, args: &[VmValue]) -> Result<VmValue, String> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    static TABEL: OnceLock<Mutex<HashMap<String, Arc<crate::ffi::native::ExternFnInfo>>>> = OnceLock::new();
    let info = {
        let mut t = TABEL.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap_or_else(|p| p.into_inner());
        match t.get(deskripsi) {
            Some(i) => i.clone(),
            None => {
                let i = Arc::new(crate::ffi::native::dari_deskripsi(deskripsi)?);
                t.insert(deskripsi.to_string(), i.clone());
                i
            }
        }
    };
    ffi_jembatan::panggil(&info, args)
}

pub fn ada_builtin(nama: &str) -> bool {
    daftar_builtin_tersimpan().binary_search_by(|x| x.as_str().cmp(nama)).is_ok()
}

/// Daftarnya dibangun SEKALI per proses.
fn daftar_builtin_tersimpan() -> &'static Vec<String> {
    static DAFTAR: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    DAFTAR.get_or_init(bangun_daftar_builtin)
}

fn bangun_daftar_builtin() -> Vec<String> {
    // HOF bernamespace (`arr::map`) ada di tabelnya sebagai NILAI (`vm::hof`), jadi
    // daftarnya tidak lagi menebak nama dari bentuk method-nya.
    let mut nama: Vec<String> = bawaan::tabel_bawaan().keys().cloned().collect();
    // Nama berawalan `__` adalah detail implementasi (`arr::__metode_fill`,
    // `__periksa_hasil_main__`), dan daftar ini yang melayani `--cari`, `--doc` …
    nama.retain(|n| !n.starts_with("__") && !n.contains("::__"));
    nama.sort();
    nama.dedup();
    nama
}

/// `jrgc --periksa`: berkas dan tiap import-nya (transitif) diurai dan DIKOMPILASI, tapi tidak
/// ada yang dijalankan — inisialisasi tingkat atas sebuah modul adalah kode.
pub fn periksa_dengan_paths(file: &ParsedFile, search_paths: Vec<PathBuf>) -> Result<(), Vec<VmError>> {
    let mut vm = Vm::new();
    let mut pemuat = Pemuat::new(false);
    pemuat.periksa_saja = true;
    let (impor, file) = &pemuat.muat_utama(&mut vm, file, &search_paths).map_err(|e| vec![e])?;
    let utama = compile(file, impor).map_err(|e| vec![VmError::new(e.to_string())])?;
    pemuat.unit_periksa.push(utama);
    let galat = nama_tak_dideklarasikan(&vm, &pemuat.unit_periksa);
    if galat.is_empty() { Ok(()) } else { Err(galat) }
}

/// Nama global yang DIBACA sebuah unit tapi tidak didefinisikan unit mana pun: saat jalan ia
/// gagal di baris itu, dan di sini dilaporkan tanpa menjalankan apa pun, dengan kalimat yang sama.
fn nama_tak_dideklarasikan(vm: &Vm, unit: &[Chunk]) -> Vec<VmError> {
    use opcode::OpCode;
    fn telusuri<'a>(c: &'a Chunk, f: &mut dyn FnMut(&'a Chunk)) {
        f(c);
        for anak in &c.functions { telusuri(anak, f); }
        for k in &c.constants {
            match k {
                VmValue::Function(g) => telusuri(&g.chunk, f),
                VmValue::Closure(g) => telusuri(&g.func.chunk, f),
                _ => {}
            }
        }
    }
    let mut ada: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for u in unit {
        telusuri(u, &mut |c| for i in &c.code {
            if matches!(i.op, OpCode::DefGlobal | OpCode::DefVar) {
                if let Some(n) = c.names.get(i.arg as usize) { ada.insert(n.as_str()); }
            }
        });
    }
    let mut galat: Vec<VmError> = Vec::new();
    let mut sudah: std::collections::HashSet<(u32, u32, u32)> = std::collections::HashSet::new();
    for u in unit {
        telusuri(u, &mut |c| for i in &c.code {
            if i.op != OpCode::LoadGlobal { continue; }
            let Some(n) = c.names.get(i.arg as usize) else { continue };
            if ada.contains(n.as_str()) { continue; }
            let span = c.spans.get(i.span as usize).copied();
            if let Some(s) = span {
                if !sudah.insert((s.file_id, s.lo.0, s.hi.0)) { continue; }
            }
            galat.push(VmError::at(vm.jelaskan_global_hilang_di(Some(c), n), span));
        });
        telusuri(u, &mut |c| for (pesan, si) in &c.sasaran_ditolak {
            let span = c.spans.get(*si as usize).copied();
            if let Some(s) = span {
                if !sudah.insert((s.file_id, s.lo.0, s.hi.0)) { continue; }
            }
            galat.push(VmError::at(pesan.clone(), span));
        });
    }
    galat
}

/// Pemuat modul satu program. Identitas modul adalah SUMBERNYA: satu berkas dimuat dan
/// dijalankan sekali, berapa pun alias dan pengimpornya — dulu kuncinya `sumber::alias`,
/// jadi dua alias atas satu berkas menjalankannya dua kali.
struct Pemuat {
    /// Identitas sumber → modul yang sudah dikompilasi dan dijalankan.
    selesai: HashMap<String, Arc<compiler::InfoModul>>,
    /// Tumpukan `(identitas, nama)` yang SEDANG dimuat — penjaga import melingkar.
    sedang: Vec<(String, String)>,
    /// Nomor urut modul, untuk awalan nama globalnya.
    nomor: usize,
    emit_bc: bool,
    /// `--periksa`: modul dikompilasi tanpa dijalankan.
    periksa_saja: bool,
    /// Hasil kompilasi tiap unit, disimpan cuma di mode `--periksa`.
    unit_periksa: Vec<Chunk>,
    /// Pemilik tiap nama tipe (`crate::ruang_tipe`).
    ruang: crate::ruang_tipe::Ruang,
}

impl Pemuat {
    fn new(emit_bc: bool) -> Self {
        Pemuat { selesai: HashMap::new(), sedang: Vec::new(), nomor: 0, emit_bc, periksa_saja: false,
                 unit_periksa: Vec::new(), ruang: Default::default() }
    }

    /// Berkas utama: tipenya diklaim SEBELUM modulnya dimuat, lalu salinannya ditulis
    /// ulang dengan label tipe (`crate::ruang_tipe`), sama dengan interpreter.
    fn muat_utama(
        &mut self, vm: &mut Vm, file: &ParsedFile, search_paths: &[PathBuf],
    ) -> Result<(compiler::PetaImpor, ParsedFile), VmError> {
        use crate::ruang_tipe::UNIT_UTAMA;
        self.ruang.klaim(UNIT_UTAMA, UNIT_UTAMA, file);
        let (impor, daftar) = self.muat(vm, file, search_paths)?;
        let mut salinan = file.clone();
        self.ruang.olah(UNIT_UTAMA, UNIT_UTAMA, &mut salinan, &daftar)
            .map_err(|(pesan, span)| VmError::at(pesan, Some(span)))?;
        Ok((impor, salinan))
    }

    /// Muat tiap import `file`; jawabannya peta alias → modul untuk kompilasi `file`,
    /// dan `(alias, identitas)` tiap import-nya.
    fn muat(
        &mut self, vm: &mut Vm, file: &ParsedFile, search_paths: &[PathBuf],
    ) -> Result<(compiler::PetaImpor, Vec<(String, String)>), VmError> {
        use crate::lexer::{lex, SourceFile};
        use crate::module::ModuleResolver;
        use crate::parser::parse;

        let resolver = ModuleResolver::new(search_paths.to_vec());
        let mut impor = compiler::PetaImpor::new();
        let mut daftar: Vec<(String, String)> = Vec::new();
        for import in &file.imports {
            // Dari mana sumbernya: modul `std/...` yang ditanam di binary, atau sebuah
            // berkas di disk.
            let (id, nama_sumber, source_text, dir_induk) =
                match ModuleResolver::std_module_name(&import.path) {
                    Some(nama) => match crate::interpreter::stdlib_src::source_of(nama) {
                        Some(src) => (crate::module::identitas_std(nama),
                                      format!("<std/{}>", nama), src.to_string(), None),
                        None if ModuleResolver::is_stdlib(&import.path) => continue,
                        None => {
                            return Err(VmError::at(
                                crate::interpreter::stdlib_src::pesan_modul_std_tak_ada(&import.path),
                                Some(import.span)))
                        }
                    },
                    None => {
                        // Berkas nyata menang atas namespace bawaan; urutan yang
                        // sama dengan interpreter.
                        if resolver.resolve(&import.path).is_none()
                            && ModuleResolver::is_stdlib(&import.path)
                        {
                            continue;
                        }
                        let module_path = resolver.resolve(&import.path)
                            .ok_or_else(|| VmError::at(resolver.pesan_tak_ketemu(&import.path), Some(import.span)))?;
                        let isi = crate::module::baca_sumber(&module_path).map_err(|error| {
                            VmError::new(format!("tidak bisa baca '{}': {}", module_path.display(), error))
                        })?;
                        let induk = module_path.parent().map(|p| p.to_path_buf());
                        (crate::module::identitas(&module_path),
                         module_path.display().to_string(), isi, induk)
                    }
                };
            let alias = import.alias.clone()
                .unwrap_or_else(|| ModuleResolver::module_name(&import.path));
            // Alias tiap modul yang dimuat unit MANA PUN: untuk menjelaskan `x::f` yang
            // gagal karena `x` diimpor berkas lain, bukan berkas ini.
            vm.namespace_modul.insert(alias.clone());
            daftar.push((alias.clone(), id.clone()));
            if let Some(info) = self.selesai.get(&id) {
                impor.insert(alias, info.clone());
                continue;
            }
            if let Some(mulai) = self.sedang.iter().position(|(i, _)| *i == id) {
                let mut rantai: Vec<String> =
                    self.sedang[mulai..].iter().map(|(_, n)| n.clone()).collect();
                rantai.push(alias);
                return Err(VmError::at(crate::module::pesan_impor_melingkar(&rantai), Some(import.span)));
            }

            // `file_id` HARUS unik per modul, dan didaftarkan lewat pintu yang sama
            // dengan interpreter.
            let file_id = crate::diag::register_module_source(
                nama_sumber.clone(), source_text.clone()
            );
            let source = SourceFile { id: file_id, name: nama_sumber, src: source_text };
            let galat_modul = |g: Vec<(String, crate::lexer::Span)>| {
                let (pesan, span) = crate::pipa::galat_parse_modul(&source.src, &g);
                let mut e = VmError::new(pesan);
                e.span = Some(span);
                e
            };
            let tokens = lex(&source).map_err(|e| galat_modul(vec![(e.msg, e.span)]))?;
            let (mut module, errors) = parse(tokens, false);
            if !errors.is_empty() {
                return Err(galat_modul(errors.into_iter().map(|e| (e.msg, e.span)).collect()));
            }

            // Direktori modul dicari LEBIH DULU untuk import miliknya sendiri: `x/pakai`
            // yang mengimpor `util` menerima `x/util`, bukan `util` di akar proyek.
            let mut nested_paths = search_paths.to_vec();
            if let Some(parent) = dir_induk {
                nested_paths.retain(|p| *p != parent);
                nested_paths.insert(0, parent);
            }
            // Yang berlokasi diteruskan utuh, seperti `sebut_modul` di interpreter.
            let sebut = |e: VmError| if e.span.is_some() { e } else {
                VmError::new(format!("modul `{}`: {}", import.path, e.msg))
            };
            self.sedang.push((id.clone(), alias.clone()));
            let (impor_modul, daftar_modul) = self.muat(vm, &module, &nested_paths).map_err(sebut)?;
            self.ruang.olah(&id, &ModuleResolver::module_name(&import.path), &mut module, &daftar_modul)
                .map_err(|(pesan, span)| VmError::at(pesan, Some(span)))?;

            // Awalan nama globalnya unik per sumber; nama modulnya ikut supaya terbaca
            // di `--emit-bc`.
            self.nomor += 1;
            let awalan = format!("{}#{}", ModuleResolver::module_name(&import.path), self.nomor);
            let (chunk, info) = compiler::compile_module(&module, &awalan, &impor_modul)
                .map_err(|error| VmError::new(format!("modul `{}`: {}", import.path, error)))?;
            if self.emit_bc { chunk.disassemble(); }
            if self.periksa_saja { self.unit_periksa.push(chunk); } else { vm.run(chunk).map_err(sebut)?; }
            self.sedang.pop();
            let info = Arc::new(info);
            self.selesai.insert(id, info.clone());
            impor.insert(alias, info);
        }
        Ok((impor, daftar))
    }
}

/// Kompilasi saja — untuk inspeksi / --emit-bc
pub fn compile_only(file: &ParsedFile) -> Result<Chunk, String> {
    compile(file, &compiler::PetaImpor::new()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::{lex, SourceFile};
    use crate::parser::parse;

    fn parse_source(source: &str) -> Result<ParsedFile, String> {
        let source = SourceFile {
            id: 0,
            name: "<vm-test>".to_string(),
            src: source.to_string(),
        };
        let tokens = lex(&source).map_err(|error| format!("{:?}", error))?;
        let (file, errors) = parse(tokens, false);
        if errors.is_empty() {
            Ok(file)
        } else {
            Err(errors
                .iter()
                .map(|error| format!("{:?}", error))
                .collect::<Vec<_>>()
                .join("\n"))
        }
    }

    fn run_source(source: &str) -> Result<(), String> {
        let file = parse_source(source)?;
        let chunk = compile(&file, &compiler::PetaImpor::new()).map_err(|error| error.to_string())?;
        Vm::new().run(chunk).map_err(|error| error.to_string())
    }

    #[test]
    fn assignments_bitwise_structs_and_slices() {
        run_source(
            r#"
package main

var global = 1

struct Box {
    // `pub` ditambahkan waktu VM mulai menegakkan visibilitas field.
    // Programnya memang tidak pernah sah — interpreter sudah lama
    // menolaknya — tapi tes ini hijau selama VM belum memeriksa. Contoh
    // persis kenapa asimetri backend berbahaya: ia bikin tes yang salah
    // terlihat benar.
    pub value: int,
}

pub fn main() {
    var x = 3
    x += 4
    x <<= 1
    x ^= 2
    assert(x == 12, "local compound assignment")

    global = x
    global |= 1
    assert(global == 13, "global assignment")

    // `var`, bukan `let`: baris di bawah MENGUBAH isinya, dan sejak
    // fase MULTIPARADIGMA `let` benar-benar mengikat. Ini kejadian
    // KETIGA sebuah tes di berkas ini hijau cuma karena bahasanya belum
    // memeriksa — dua sebelumnya soal visibilitas field.
    var box = Box { value: 2 }
    box.value = 7
    box.value += 5
    assert(box.value == 12, "field assignment")

    var data = [1, 2, 3, 4]
    data[0] += 4
    data[1] = 8
    assert(data == [5, 8, 3, 4], "index assignment")

    let middle = data[1:3]
    assert(middle == [8, 3], "array slice")
    assert("compiler"[1:5] == "ompi", "string slice")

    let moved = Box { value: 21, ...box }
    assert(moved.value == 21, "struct spread override")
}
"#,
        )
        .unwrap();
    }

    #[test]
    fn loops_ranges_break_continue_and_multiple_returns() {
        run_source(
            r#"
package main

fn pair() {
    return 4, 9
}

pub fn main() {
    let values = pair()
    assert(values == (4, 9), "multiple return tuple")

    var one_var_sum = 0
    for i in 10..=14 {
        if i == 1 {
            continue
        }
        one_var_sum += i
        if i == 3 {
            break
        }
    }
    assert(one_var_sum == 5, "single range variable is index")

    var index_sum = 0
    var value_sum = 0
    for index, value in 2..=5 {
        if value == 3 {
            continue
        }
        index_sum += index
        value_sum += value
        if value == 5 {
            break
        }
    }
    assert(index_sum == 5, "range index")
    assert(value_sum == 11, "inclusive range values")
}
"#,
        )
        .unwrap();
    }

    #[test]
    fn spawn_captures_locals_and_channel_methods_work() {
        run_source(
            r#"
package main

pub fn main() {
    let explicit = make_chan(int, 1)
    let value = 7
    spawn |explicit, value| {
        explicit <- value
    }
    let explicit_value = <-explicit
    assert(explicit_value == 7, "explicit spawn captures")

    let implicit = make_chan(int, 1)
    spawn {
        implicit <- 11
    }
    assert(implicit.cap() == 1, "channel cap method")
    let implicit_value = <-implicit
    assert(implicit_value == 11, "implicit spawn captures")
    assert(num_cpus() >= 1, "num_cpus")

    let selected = make_chan(int, 1)
    spawn {
        selected <- 23
    }
    var selected_value = 0
    select {
        case value := <-selected: {
            selected_value = value
        }
    }
    assert(selected_value == 23, "blocking select")

    let empty = make_chan(int)
    var used_default = false
    select {
        case value := <-empty: {
            used_default = false
        }
        default: {
            used_default = true
        }
    }
    assert(used_default, "non-blocking select default")
}
"#,
        )
        .unwrap();
    }

    #[test]
    fn result_option_and_recursive_pattern_matching() {
        run_source(
            r#"
package main

enum Shape {
    Point(int, int),
    Triangle { base: int, height: int },
    Empty,
}

fn checked(value: int) -> Result<int, string> {
    if value < 0 {
        return Err("negative")
    }
    Ok(value)
}

fn doubled(value: int) -> Result<int, string> {
    let valid = checked(value)?
    Ok(valid * 2)
}

fn classify(point: (int, int)) -> string {
    match point {
        (0, 0) => "origin",
        (_, 0) => "x-axis",
        (x, y) if x == y => "diagonal",
        _ => "other",
    }
}

pub fn main() {
    assert(classify((0, 0)) == "origin", "nested tuple literals")
    assert(classify((5, 0)) == "x-axis", "tuple wildcard")
    assert(classify((3, 3)) == "diagonal", "tuple bindings and guard")
    assert(classify((2, 7)) == "other", "tuple mismatch")

    let triangle = Shape::Triangle { base: 6, height: 4 }
    let area = match triangle {
        Shape::Triangle { base, height } => base * height / 2,
        _ => 0,
    }
    assert(area == 12, "enum struct pattern")

    let point = Shape::Point(4, 9)
    let total = match point {
        Shape::Point(0, 0) => 0,
        Shape::Point(x, y) => x + y,
        _ => -1,
    }
    assert(total == 13, "enum tuple nested pattern")

    let band = match 7 {
        1 | 3 | 5 => "odd-small",
        6..=8 => "middle",
        _ => "other",
    }
    assert(band == "middle", "or and range patterns")

    let mapped = Some(4).map(fn(value) => value * 3)
    assert(mapped.unwrap_or(0) == 12, "Option.map")
    assert(doubled(5).unwrap_or(0) == 10, "Result propagation success")
    assert(doubled(-1).is_err(), "Result propagation error")
}
"#,
        )
        .unwrap();
    }

    #[test]
    fn imported_modules_are_loaded_into_the_same_vm() {
        let temp_dir = std::env::temp_dir().join(format!(
            "jrg-vm-module-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id(),
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();
        std::fs::write(
            temp_dir.join("helpers.jrg"),
            r#"
package helpers

pub fn add(a: int, b: int) -> int {
    a + b
}

pub fn double(value: int) -> int {
    add(value, value)
}
"#,
        )
        .unwrap();

        let file = parse_source(
            r#"
package main

import "helpers"

pub fn main() {
    assert(helpers::add(10, 32) == 42, "qualified module function")
    assert(helpers::double(21) == 42, "module internal function")
}
"#,
        )
        .unwrap();
        let result = run_file_with_paths(&file, vec![temp_dir.clone()], false, false);
        let _ = std::fs::remove_dir_all(&temp_dir);
        result.unwrap();
    }

    #[test]
    fn struct_defaults_inheritance_and_trait_defaults() {
        run_source(
            r#"
package main

struct Animal {
    pub name: string = "unknown",
    pub lives: int = 3,
}

impl Animal {
    fn is_alive(self) -> bool {
        self.lives > 0
    }
}

struct Dog: Animal {
    // `pub`, karena `main` membacanya. Program ini juga tidak pernah sah —
    // interpreter menolaknya sejak lama — dan tesnya lolos sebelum VM
    // memeriksa visibilitas.
    pub breed: string = "mixed",
}

impl Dog {
    fn label(self) -> string {
        self.name + ":" + self.breed
    }
}

trait Described {
    fn describe(self) -> string {
        "default-description"
    }
}

impl Described for Dog {}

pub fn main() {
    let dog = Dog { name: "Rex" }
    assert(dog.breed == "mixed", "child field default")
    assert(dog.lives == 3, "inherited field default")
    assert(dog.is_alive(), "inherited method")
    assert(dog.label() == "Rex:mixed", "child method")
    assert(dog.describe() == "default-description", "trait default method")
}
"#,
        )
        .unwrap();
    }

    #[test]
    fn extended_array_methods_execute_user_callbacks() {
        run_source(
            r#"
package main

pub fn main() {
    assert([3, 1, 2].sort_desc() == [3, 2, 1], "sort_desc")
    assert([1, 2, 3].union([2, 3, 4]) == [1, 2, 3, 4], "union")
    assert([1, 2, 3].intersect([2, 3, 4]) == [2, 3], "intersect")
    assert([1, 2, 3].difference([2, 4]) == [1, 3], "difference")

    let split = [1, 2, 3, 4].partition(fn(value) => value % 2 == 0)
    assert(split == ([2, 4], [1, 3]), "partition callback")

    let zipped = [1, 2, 3].zip_with([10, 20, 30], fn(a, b) => a + b)
    assert(zipped == [11, 22, 33], "zip_with callback")

    // Tes ini dulu mengunci array pasangan, dan hijau HANYA karena bentuk
    // method menyimpang dari `arr::group_by` yang sudah menjawab dict.
    let grouped = [1, 2, 3, 4].group_by(fn(value) => value % 2)
    assert(grouped == arr::group_by([1, 2, 3, 4], fn(value) => value % 2), "group_by satu kontrak")
    assert(grouped["1"] == [1, 3], "group_by kunci ganjil")
    assert(grouped["0"] == [2, 4], "group_by kunci genap")
}
"#,
        )
        .unwrap();
    }

    #[test]
    fn crypto_and_shared_pointer_builtins() {
        run_source(
            r#"
package main

pub fn main() {
    assert(
        crypto::sha256("abc") == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "sha256"
    )

    let key = crypto::aes_gen_key()
    let iv = crypto::aes_gen_iv()
    let encrypted = crypto::aes_encrypt("secret", key, iv).unwrap()
    let decrypted = crypto::aes_decrypt(encrypted, key, iv).unwrap()
    assert(decrypted == "secret", "AES roundtrip")

    let pointer = ptr::new(1)
    assert(ptr::get(pointer) == 1, "ptr get")
    ptr::set(pointer, 4)
    assert(ptr::update(pointer, fn(value) => value + 3) == 7, "ptr update")

    let done = make_chan(bool, 1)
    spawn {
        ptr::update(pointer, fn(value) => value + 5)
        done <- true
    }
    let completed = <-done
    assert(completed, "ptr goroutine completed")
    assert(ptr::get(pointer) == 12, "ptr shared across goroutine")
}
"#,
        )
        .unwrap();
    }

    /// Literal di luar jangkauan integer JRG ditolak saat kompilasi.
    #[test]
    fn integer_literal_range_is_128_bit() {
        // 2^64 — dulu di luar jangkauan, sekarang muat.
        let file = parse_source(
            r#"
package main

pub fn main() {
    let besar = 18446744073709551616
    wok(besar)
}
"#,
        )
        .unwrap();
        assert!(
            compile(&file, &compiler::PetaImpor::new()).is_ok(),
            "2^64 harus muat di integer 128 bit"
        );

        // 2^128 — benar-benar di luar jangkauan. Ditolak lebih awal, di
        // LEXER (sebelum sempat jadi AST), dan itu sah: yang penting
        // penolakannya keras dan sama di kedua backend.
        assert!(
            parse_source(
                r#"
package main

pub fn main() {
    let kelewat = 340282366920938463463374607431768211456
}
"#,
            )
            .is_err(),
            "literal di luar 128 bit harus ditolak"
        );
    }

    /// `if` is expression-valued, and every path through the VM lowering must
    /// leave exactly one value on the stack — including the `else if` chain and …
    #[test]
    fn if_expression_yields_branch_value_with_balanced_stack() {
        run_source(
            r#"
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
    assert(from_match(E::A, 10) == 100, "if-expr dalam match arm (then)")
    assert(from_match(E::A, 1) == 200, "if-expr dalam match arm (else)")
    assert(from_match(E::B, 0) == 7, "arm biasa")

    assert(chain(5) == 1, "else-if chain 1")
    assert(chain(15) == 2, "else-if chain 2")
    assert(chain(25) == 3, "else-if chain 3")

    let x = if true { 100 } else { 200 }
    assert(x == 100, "if-expr di let")
    assert(1 + if true { 10 } else { 20 } == 11, "if-expr di dalam ekspresi")

    let s = if false { "no" } else { "yes" }
    assert(s == "yes", "if-expr non-integer")

    let nested = { { 77 } }
    assert(nested == 77, "blok telanjang di posisi ekor adalah nilai blok")

    // Statement-position `if` still runs for effect and yields nothing.
    var side: int = 0
    if true { side = 5 }
    assert(side == 5, "if statement tetap jalan")
}
"#,
        )
        .unwrap();
    }
}
