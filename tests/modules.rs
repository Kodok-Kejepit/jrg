// tests/modules.rs

mod bersama;
use std::path::{Path, PathBuf};

use jrgc::interpreter::interpret_with_paths;
use jrgc::lexer::{lex, SourceFile};
use jrgc::parser::parse;

const STACK_SIZE: usize = 32 * 1024 * 1024;

/// Direktori sementara yang menghapus dirinya sendiri saat keluar dari
/// scope, supaya kasus yang gagal pun tidak meninggalkan sampah.
struct DirSementara(PathBuf);

impl DirSementara {
    fn baru(nama: &str) -> Self {
        let dir = crate::bersama::dir_unik(&format!("jrg-modules-{}", nama));
        std::fs::create_dir_all(&dir).expect("tidak bisa bikin direktori sementara");
        DirSementara(dir)
    }

    fn tulis(&self, nama_file: &str, isi: &str) -> PathBuf {
        let path = self.0.join(nama_file);
        std::fs::write(&path, isi).expect("tidak bisa menulis file uji");
        path
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for DirSementara {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Jalankan satu file utama dengan direktori proyek sebagai search path.
fn jalankan(utama: &Path, root: &Path) -> Result<(), String> {
    let src = std::fs::read_to_string(utama).map_err(|e| format!("baca: {}", e))?;
    let sf = SourceFile { id: 0, name: utama.display().to_string(), src };

    let tokens = lex(&sf).map_err(|e| format!("lex: {}", e.msg))?;
    let (ast, errors) = parse(tokens, false);
    if !errors.is_empty() {
        let msgs: Vec<&str> = errors.iter().map(|e| e.msg.as_str()).collect();
        return Err(format!("parse: {}", msgs.join("; ")));
    }
    interpret_with_paths(&ast, vec![root.to_path_buf()]).map_err(|e| format!("runtime: {}", e.message))
}

fn jalankan_di_stack_besar(utama: PathBuf, root: PathBuf) -> Result<(), String> {
    std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(move || jalankan(&utama, &root))
        .expect("tidak bisa spawn thread")
        .join()
        .map_err(|_| "program panik saat dijalankan".to_string())?
}

fn pesan_error(hasil: Result<(), String>) -> String {
    match hasil {
        Ok(()) => panic!("program TIDAK error padahal seharusnya error"),
        Err(e) => e,
    }
}

fn wajib_menyebut(pesan: &str, kata: &[&str]) {
    let lower = pesan.to_lowercase();
    for k in kata {
        assert!(
            lower.contains(&k.to_lowercase()),
            "pesan error tidak menyebut `{}`\n  pesan: {}",
            k,
            pesan
        );
    }
}

// ── Dasar ──────────────────────────────────────────────────────────────

#[test]
fn simbol_modul_dipakai_lewat_nama_modul() {
    let dir = DirSementara::baru("dasar");
    dir.tulis(
        "matek.jrg",
        "package matek\n\
         pub fn tambah(a: int, b: int) -> int { return a + b }\n\
         pub fn kali(a: int, b: int) -> int { return a * b }\n\
         pub const DUA = 2\n",
    );
    let utama = dir.tulis(
        "utama.jrg",
        "package main\n\
         import \"matek\"\n\
         pub fn main() {\n\
             assert_eq(matek::tambah(2, 3), 5)\n\
             assert_eq(matek::kali(4, 5), 20)\n\
             assert_eq(matek::DUA, 2)\n\
         }\n",
    );

    jalankan_di_stack_besar(utama, dir.path().to_path_buf()).expect("program modul dasar gagal");
}

#[test]
fn struct_dan_enum_dari_modul_ikut_terbawa() {
    let dir = DirSementara::baru("tipe");
    dir.tulis(
        "bentuk.jrg",
        "package bentuk\n\
         pub struct Titik { pub x: int, pub y: int }\n\
         pub fn asal() -> Titik { return Titik { x: 0, y: 0 } }\n\
         pub fn geser(t: Titik, d: int) -> Titik { return Titik { x: t.x + d, y: t.y + d } }\n",
    );
    let utama = dir.tulis(
        "utama.jrg",
        "package main\n\
         import \"bentuk\"\n\
         pub fn main() {\n\
             let a = bentuk::asal()\n\
             assert_eq(a.x, 0)\n\
             let b = bentuk::geser(a, 3)\n\
             assert_eq(b.x, 3)\n\
             assert_eq(b.y, 3)\n\
         }\n",
    );

    jalankan_di_stack_besar(utama, dir.path().to_path_buf()).expect("program modul tipe gagal");
}

#[test]
fn alias_import_mengganti_nama_modul() {
    let dir = DirSementara::baru("alias");
    dir.tulis(
        "panjang_banget.jrg",
        "package panjang_banget\npub fn nilai() -> int { return 7 }\n",
    );
    let utama = dir.tulis(
        "utama.jrg",
        "package main\n\
         import \"panjang_banget\" as pb\n\
         pub fn main() { assert_eq(pb::nilai(), 7) }\n",
    );

    jalankan_di_stack_besar(utama, dir.path().to_path_buf()).expect("alias import gagal");
}

// ── Batas ekspor ───────────────────────────────────────────────────────

#[test]
fn modul_tidak_mengekspor_bawaan_interpreter() {
    // Dulu penyaring ekspor cuma membuang `NativeFn`, sehingga konstruktor enum
    // bawaan (`Ok`, `Err`, `Some`, `None`) ikut "diekspor" oleh setiap modul.
    let dir = DirSementara::baru("ekspor");
    dir.tulis("lib.jrg", "package lib\npub fn punyaku() -> int { return 1 }\n");
    let utama = dir.tulis(
        "utama.jrg",
        "package main\n\
         import \"lib\"\n\
         pub fn main() {\n\
             assert_eq(lib::punyaku(), 1)\n\
             wok(lib::Err(\"bukan punya lib\"))\n\
         }\n",
    );

    let pesan = pesan_error(jalankan_di_stack_besar(utama, dir.path().to_path_buf()));
    wajib_menyebut(&pesan, &["lib::Err"]);
}

#[test]
fn simbol_berawalan_garis_bawah_tetap_privat() {
    let dir = DirSementara::baru("privat");
    dir.tulis(
        "lib.jrg",
        "package lib\n\
         pub fn publik() -> int { return _rahasia() }\n\
         fn _rahasia() -> int { return 42 }\n",
    );
    let utama = dir.tulis(
        "utama.jrg",
        "package main\n\
         import \"lib\"\n\
         pub fn main() {\n\
             assert_eq(lib::publik(), 42)\n\
             wok(lib::_rahasia())\n\
         }\n",
    );

    let pesan = pesan_error(jalankan_di_stack_besar(utama, dir.path().to_path_buf()));
    wajib_menyebut(&pesan, &["_rahasia"]);
}

// ── Nama modul bukan nilai ─────────────────────────────────────────────

#[test]
fn nama_modul_telanjang_adalah_error_bukan_nil() {
    // Dulu nama modul di-bind ke `Value::Nil` supaya menyebutnya tidak
    // crash. Efek sampingnya: `let x = lib` diam-diam `nil`, `if lib {}`
    // diam-diam false, `lib == nil` diam-diam true.
    let dir = DirSementara::baru("bare");
    dir.tulis("lib.jrg", "package lib\npub fn nilai() -> int { return 1 }\n");
    let utama = dir.tulis(
        "utama.jrg",
        "package main\n\
         import \"lib\"\n\
         pub fn main() { let x = lib; wok(x) }\n",
    );

    let pesan = pesan_error(jalankan_di_stack_besar(utama, dir.path().to_path_buf()));
    wajib_menyebut(&pesan, &["lib", "modul", "::"]);
}

// ── Siklus dan berbagi ─────────────────────────────────────────────────

#[test]
fn import_muter_muter_dilaporkan_bukan_bikin_stack_habis() {
    // Penjaga siklus dulu tidak pernah kena: tiap modul dimuat oleh
    // interpreter anak yang daftar "sedang dimuat"-nya kosong, jadi
    // a -> b -> a turun terus sampai stack habis dan proses mati.
    let dir = DirSementara::baru("siklus");
    dir.tulis(
        "a.jrg",
        "package a\nimport \"b\"\npub fn dari_a() -> int { return 1 }\n",
    );
    dir.tulis(
        "b.jrg",
        "package b\nimport \"a\"\npub fn dari_b() -> int { return 2 }\n",
    );
    let utama = dir.tulis(
        "utama.jrg",
        "package main\nimport \"a\"\npub fn main() { wok(a::dari_a()) }\n",
    );

    let pesan = pesan_error(jalankan_di_stack_besar(utama, dir.path().to_path_buf()));
    wajib_menyebut(&pesan, &["muter"]);
}

#[test]
fn modul_yang_dipakai_bersama_dimuat_sekali() {
    // Dependency berbentuk berlian: utama -> {kiri, kanan} -> bersama.
    // `bersama` punya efek samping saat di-hoist (`const` yang memanggil
    // fungsi); kalau dimuat dua kali, efeknya kejadian dua kali.
    let dir = DirSementara::baru("berlian");
    dir.tulis(
        "bersama.jrg",
        "package bersama\npub fn nilai() -> int { return 10 }\n",
    );
    dir.tulis(
        "kiri.jrg",
        "package kiri\nimport \"bersama\"\npub fn ambil() -> int { return bersama::nilai() }\n",
    );
    dir.tulis(
        "kanan.jrg",
        "package kanan\nimport \"bersama\"\npub fn ambil() -> int { return bersama::nilai() + 1 }\n",
    );
    let utama = dir.tulis(
        "utama.jrg",
        "package main\n\
         import \"kiri\"\n\
         import \"kanan\"\n\
         pub fn main() {\n\
             assert_eq(kiri::ambil(), 10)\n\
             assert_eq(kanan::ambil(), 11)\n\
         }\n",
    );

    jalankan_di_stack_besar(utama, dir.path().to_path_buf()).expect("dependency berlian gagal");
}

// ── Error yang menolong ────────────────────────────────────────────────

#[test]
fn modul_yang_tidak_ada_menyebutkan_tempat_yang_sudah_dicari() {
    let dir = DirSementara::baru("hilang");
    let utama = dir.tulis(
        "utama.jrg",
        "package main\nimport \"tidak_ada_modul_ini\"\npub fn main() { wok(1) }\n",
    );

    let pesan = pesan_error(jalankan_di_stack_besar(utama, dir.path().to_path_buf()));
    wajib_menyebut(&pesan, &["tidak_ada_modul_ini"]);
}

#[test]
fn simbol_yang_tidak_ada_di_modul_dilaporkan() {
    let dir = DirSementara::baru("simbol");
    dir.tulis("lib.jrg", "package lib\npub fn ada() -> int { return 1 }\n");
    let utama = dir.tulis(
        "utama.jrg",
        "package main\nimport \"lib\"\npub fn main() { wok(lib::tidak_ada()) }\n",
    );

    let pesan = pesan_error(jalankan_di_stack_besar(utama, dir.path().to_path_buf()));
    wajib_menyebut(&pesan, &["tidak_ada"]);
}

#[test]
fn error_runtime_dari_dalam_modul_tetap_dilaporkan() {
    // Error yang terjadi di dalam badan fungsi milik modul harus sampai ke
    // pemanggil apa adanya, bukan tertelan di batas modul.
    let dir = DirSementara::baru("errormodul");
    dir.tulis(
        "rusak.jrg",
        "package rusak\npub fn bagi(a: int, b: int) -> int { return a / b }\n",
    );
    let utama = dir.tulis(
        "utama.jrg",
        "package main\nimport \"rusak\"\npub fn main() { wok(rusak::bagi(1, 0)) }\n",
    );

    let pesan = pesan_error(jalankan_di_stack_besar(utama, dir.path().to_path_buf()));
    wajib_menyebut(&pesan, &["nol"]);
}

#[test]
fn modul_bisa_memakai_modul_yang_diimpornya_sendiri() {
    // Fungsi JRG dijalankan terhadap `globals` interpreter yang MEMANGGIL, jadi
    // tanpa penanganan khusus badan `kiri::ambil()` tidak bisa melihat …
    let dir = DirSementara::baru("transitif");
    dir.tulis("bersama.jrg", "package bersama\npub fn nilai() -> int { return 10 }\n");
    dir.tulis(
        "kiri.jrg",
        "package kiri\nimport \"bersama\"\npub fn ambil() -> int { return bersama::nilai() * 2 }\n",
    );
    let utama = dir.tulis(
        "utama.jrg",
        "package main\nimport \"kiri\"\npub fn main() { assert_eq(kiri::ambil(), 20) }\n",
    );

    jalankan_di_stack_besar(utama, dir.path().to_path_buf())
        .expect("modul tidak bisa memakai import-nya sendiri");
}

// ── Cakupan (scope) fungsi lintas modul ───────────────────────────────

#[test]
fn modul_bisa_memanggil_sesama_fungsinya_sendiri_lewat_nama_telanjang() {
    // `call_jrg` dulu membangun env pemanggilan dari globals interpreter
    // PEMANGGIL, bukan globals milik fungsi itu sendiri saat didefinisikan.
    let dir = DirSementara::baru("sesama-fn");
    dir.tulis(
        "lib.jrg",
        "package lib\n\
         pub fn publik(n: int) -> int { return privat(n) + 1 }\n\
         fn privat(n: int) -> int { return n * 10 }\n",
    );
    let utama = dir.tulis(
        "utama.jrg",
        "package main\nimport \"lib\"\npub fn main() { assert_eq(lib::publik(4), 41) }\n",
    );

    jalankan_di_stack_besar(utama, dir.path().to_path_buf())
        .expect("fungsi modul tidak bisa memanggil sesama fungsinya sendiri");
}

#[test]
fn variabel_modul_terlihat_oleh_fungsi_modul_itu_sendiri_dari_luar() {
    // Sama seperti kasus di atas, tapi untuk state (`var` level modul) alih-alih
    // fungsi: fungsi publik yang membaca/menulis `var` level modulnya sendiri …
    let dir = DirSementara::baru("var-modul");
    dir.tulis(
        "penghitung.jrg",
        "package penghitung\n\
         var n = 0\n\
         pub fn naik() -> int { n = n + 1; return n }\n\
         pub fn nilai_sekarang() -> int { return n }\n",
    );
    let utama = dir.tulis(
        "utama.jrg",
        "package main\n\
         import \"penghitung\"\n\
         pub fn main() {\n\
             assert_eq(penghitung::naik(), 1)\n\
             assert_eq(penghitung::naik(), 2)\n\
             assert_eq(penghitung::naik(), 3)\n\
             assert_eq(penghitung::nilai_sekarang(), 3)\n\
         }\n",
    );

    jalankan_di_stack_besar(utama, dir.path().to_path_buf())
        .expect("state level-modul tidak terlihat/tidak bertahan lintas panggilan");
}

#[test]
fn nama_yang_bentrok_dengan_bawaan_interpreter_tetap_diekspor() {
    // Dulu "milik modul ini" ditentukan dari diff kunci `globals` sebelum vs
    // sesudah hoist.
    let dir = DirSementara::baru("bentrok-nama");
    dir.tulis(
        "custom.jrg",
        "package custom\n\
         // `zip` dan `enumerate` juga nama fungsi bawaan interpreter.\n\
         pub fn zip(a: int, b: int) -> int { return a * 100 + b }\n\
         pub fn enumerate(n: int) -> int { return n + 1 }\n",
    );
    let utama = dir.tulis(
        "utama.jrg",
        "package main\n\
         import \"custom\"\n\
         pub fn main() {\n\
             assert_eq(custom::zip(3, 4), 304)\n\
             assert_eq(custom::enumerate(9), 10)\n\
         }\n",
    );

    jalankan_di_stack_besar(utama, dir.path().to_path_buf())
        .expect("fungsi modul yang namanya bentrok dengan bawaan gagal diekspor");
}

/// Struct dari modul harus membawa SELURUH deklarasinya, bukan cuma nilainya.
#[test]
fn struct_dari_modul_membawa_default_method_dan_visibilitasnya() {
    let dir = DirSementara::baru("modul_struct_lengkap");
    dir.tulis(
        "bentuk.jrg",
        "package bentuk\n\
         pub struct Kotak {\n\
             pub lebar: int = 3,\n\
             pub tinggi: int = 4,\n\
             rahasia: int = 99,\n\
         }\n\
         impl Kotak {\n\
             pub fn luas(self) -> int { return self.lebar * self.tinggi }\n\
             pub fn intip(self) -> int { return self.rahasia }\n\
         }\n\
         pub fn bawaan() { return Kotak{} }\n\
         pub fn sebagian() { return Kotak{lebar: 10} }\n",
    );
    let utama = dir.tulis(
        "utama.jrg",
        "package main\n\
         import \"bentuk\"\n\
         pub fn main() {\n\
             let k = bentuk::bawaan()\n\
             assert_eq(k.lebar, 3)\n\
             assert_eq(k.tinggi, 4)\n\
             let s = bentuk::sebagian()\n\
             assert_eq(s.lebar, 10)\n\
             assert_eq(s.tinggi, 4)\n\
             assert_eq(k.luas(), 12)\n\
             assert_eq(k.intip(), 99)\n\
         }\n",
    );
    jalankan_di_stack_besar(utama, dir.path().to_path_buf())
        .expect("struct dari modul kehilangan default/method-nya");
}

/// Field privat sebuah struct modul tetap privat di luar modulnya.
#[test]
fn field_privat_modul_tetap_privat_di_luar() {
    let dir = DirSementara::baru("modul_visibilitas");
    dir.tulis(
        "bentuk.jrg",
        "package bentuk\n\
         pub struct Kotak { rahasia: int = 99 }\n\
         pub fn bawaan() { return Kotak{} }\n",
    );
    let utama = dir.tulis(
        "utama.jrg",
        "package main\n\
         import \"bentuk\"\n\
         pub fn main() { let k = bentuk::bawaan(); wok(k.rahasia) }\n",
    );
    let pesan = pesan_error(jalankan_di_stack_besar(utama, dir.path().to_path_buf()));
    wajib_menyebut(&pesan, &["rahasia", "private"]);
}
