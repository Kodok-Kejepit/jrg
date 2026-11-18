//! Mesin kelima (`--emit-wasm`): program JRG lewat WebAssembly (WASI, node) harus menjawab sama dengan interpreter,
//! keluaran DAN kode keluar. Alat WASI tidak ada = tes dilewati dengan alasan dicetak, KECUALI `JRG_WASI_WAJIB=1`
//! (CI), yang membuatnya gagal. Rancangan: `dokumentasi/wasm/rencana.md`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn jrgc() -> &'static str { env!("CARGO_BIN_EXE_jrgc") }

struct Hasil { kode: i32, stdout: String, stderr: String }

fn jalan(args: &[&str], dir: &Path) -> Hasil {
    let o = Command::new(jrgc()).args(args).current_dir(dir).env("JRG_SOPAN", "1").output().expect("jrgc jalan");
    Hasil {
        kode: o.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

fn folder(nama: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jrg-wasm-uji-{}-{}", nama, std::process::id()));
    std::fs::create_dir_all(&d).expect("folder sementara");
    d
}

/// `None` = alat WASI tidak ada dan tidak diwajibkan (tes dilewati, alasannya dicetak).
fn wasm(berkas: &Path, kerja: &Path) -> Option<Hasil> {
    let keluar = kerja.join(format!("{}.wasm", berkas.file_stem().unwrap().to_string_lossy()));
    let h = jalan(&[berkas.to_str().unwrap(), "--emit-wasm", "-o", keluar.to_str().unwrap()], berkas.parent().unwrap());
    if h.kode != 0 && (h.stderr.contains("butuh wasi-sysroot") || h.stderr.contains("butuh compiler-rt wasm")
        || h.stderr.contains("rustup target add wasm32-wasip1") || h.stderr.contains("`node` tidak ada")) {
        if std::env::var_os("JRG_WASI_WAJIB").is_some() { panic!("alat --emit-wasm tidak ada, padahal JRG_WASI_WAJIB:\n{}", h.stderr); }
        eprintln!("DILEWATI: alat --emit-wasm tidak ada di mesin ini:\n{}", h.stderr);
        return None;
    }
    Some(h)
}

fn tulis(kerja: &Path, nama: &str, isi: &str) -> PathBuf {
    let p = kerja.join(nama);
    std::fs::write(&p, isi).expect("tulis program");
    p
}

#[test]
fn konformans_lewat_wasm_sama_dengan_interpreter() {
    let akar = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("conformance");
    let kerja = folder("konformans");
    let mut berkas: Vec<PathBuf> = std::fs::read_dir(&akar).unwrap().flatten().map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jrg")).collect();
    berkas.sort();
    let mut beda = Vec::new();
    for b in &berkas {
        let acuan = jalan(&[b.to_str().unwrap()], &akar);
        let Some(w) = wasm(b, &kerja) else { return };
        if (acuan.kode, &acuan.stdout) != (w.kode, &w.stdout) {
            beda.push(format!("{}: kode {} / {}\n--- interpreter\n{}\n--- wasm\n{}{}", b.display(), acuan.kode, w.kode,
                acuan.stdout, w.stdout, w.stderr));
        }
    }
    let _ = std::fs::remove_dir_all(&kerja);
    assert!(beda.is_empty(), "{} dari {} berkas konformans beda lewat --emit-wasm:\n{}", beda.len(), berkas.len(), beda.join("\n"));
}

/// Closure di larik: `%JrgFn` 8 byte di wasm32 (`CodegenOptions::ptr32`); dengan ukuran 64 bit elemen kedua dibaca di
/// offset yang salah dan panggilannya trap.
const CLOSURE_DI_LARIK: &str = r#"fn main() {
    var fs = []
    for i in 0..5 { fs = fs + [|| i * 10] }
    var t = 0
    for _, f in fs { t = t + f() }
    let g = |a, b| a + b
    var gs = [g, g]
    wok(t, fs[3](), gs[1](2, 3))
}
"#;

#[test]
fn closure_di_larik_lewat_wasm() {
    let kerja = folder("closure");
    let p = tulis(&kerja, "closure.jrg", CLOSURE_DI_LARIK);
    let acuan = jalan(&[p.to_str().unwrap()], &kerja);
    let Some(w) = wasm(&p, &kerja) else { return };
    let _ = std::fs::remove_dir_all(&kerja);
    assert_eq!((acuan.kode, acuan.stdout.as_str()), (w.kode, w.stdout.as_str()), "{}", w.stderr);
}

include!("bersama/paritas_llvm.rs");

/// Konkurensi (W1c: korutin lewat Asyncify): tiap kasus `PARITAS_LLVM` yang memakai goroutine, channel, `select`,
/// Future/Task, `Atomic`, `Mutex` atau `tangkap` menjawab sama lewat wasm. Satu tabel dengan LLVM, bukan salinan.
const PENANDA_KORUTIN: &[&str] = &["spawn", "Chan::new", "Future::new", "Task::spawn", "select", "Mutex::new",
    "Atomic::new", "tangkap("];
/// Kasus konkurensi yang memanggil builtin yang tidak ada di WASI (`proses::`: tanpa proses anak), ditolak di sana.
const LEWAT_JEMBATAN: &[&str] = &["goroutine tetap mendapat giliran selama main menunggu proses::"];

#[test]
fn korutin_lewat_wasm_sama_dengan_interpreter() {
    let kerja = folder("korutin");
    let mut beda = Vec::new();
    let mut n = 0;
    for (i, (nama, src)) in PARITAS_LLVM.iter().enumerate() {
        if !PENANDA_KORUTIN.iter().any(|p| src.contains(p)) || LEWAT_JEMBATAN.contains(nama) { continue; }
        n += 1;
        let p = tulis(&kerja, &format!("k{i}.jrg"), src);
        let acuan = jalan(&[p.to_str().unwrap()], &kerja);
        let Some(w) = wasm(&p, &kerja) else { return };
        if (acuan.kode, &acuan.stdout) != (w.kode, &w.stdout) {
            beda.push(format!("{nama}: kode {} / {}\n--- interpreter\n{}--- wasm\n{}{}", acuan.kode, w.kode,
                acuan.stdout, w.stdout, w.stderr));
        }
    }
    let _ = std::fs::remove_dir_all(&kerja);
    assert!(n >= 8, "cuma {n} kasus konkurensi terbaca dari PARITAS_LLVM: penandanya basi?");
    assert!(beda.is_empty(), "{} dari {n} kasus konkurensi beda lewat --emit-wasm:\n{}", beda.len(), beda.join("\n"));
}

/// W1d: builtin lewat jembatan `jrgc` ditaut di wasm (`wasm-rt/`: runtime + jembatan satu pustaka statis) dan
/// menjawab sama dengan interpreter, termasuk galatnya.
const JEMBATAN: &str = r#"fn main() {
    wok("awal")
    wok(crypto::sha256("abc"))
    wok(re::find_all("[0-9]+", "a12 b345 c6"))
    wok(url::parse("https://x.id:8080/a?b=1"))
    wok(semver::compare("1.2.3", "1.10.0"))
    let p = ptr::new(5)
    ptr::update(p, |x| x * 3)
    wok(ptr::get(p))
    wok(re::find("(", "x"))
}
"#;

#[test]
fn builtin_jembatan_lewat_wasm() {
    let kerja = folder("jembatan");
    let p = tulis(&kerja, "jembatan.jrg", JEMBATAN);
    let acuan = jalan(&[p.to_str().unwrap()], &kerja);
    let Some(w) = wasm(&p, &kerja) else { return };
    let _ = std::fs::remove_dir_all(&kerja);
    assert_eq!((acuan.kode, acuan.stdout.as_str()), (w.kode, w.stdout.as_str()), "{}", w.stderr);
    let galat = |e: &str| e.lines().find(|l| l.starts_with("error")).unwrap_or("").to_string();
    assert_eq!(galat(&acuan.stderr), galat(&w.stderr), "kalimat galat jembatan beda:\n{}", w.stderr);
}

/// Bingkai `jrgc` di bawah jembatan tidak diinstrumentasi Asyncify: goroutine yang berganti di dalam callback builtin
/// ditolak keras dengan kalimat yang menyebutnya, bukan dilanjutkan dari stack yang rusak.
#[test]
fn goroutine_berganti_di_callback_jembatan_ditolak_keras() {
    let kerja = folder("jembatan-ganti");
    let src = "fn main() {\n    let c = Chan::new()\n    spawn { wok(<-c) }\n    let p = ptr::new(1)\n    wok(\"awal\")\n    ptr::update(p, |x| { c <- x; x + 1 })\n    wok(ptr::get(p))\n}\n";
    let p = tulis(&kerja, "ganti.jrg", src);
    let Some(w) = wasm(&p, &kerja) else { return };
    let _ = std::fs::remove_dir_all(&kerja);
    assert_ne!(w.kode, 0, "{}{}", w.stdout, w.stderr);
    assert_eq!(w.stdout, "awal\n");
    assert!(w.stderr.contains("goroutine yang berganti di dalam fungsi yang dipanggil builtin jembatan"), "{}", w.stderr);
}

/// Stack: larik lokal besar (stack bayangan wasm 8 MiB, bawaan `wasm-ld` 64 KiB meluap) dan rekursi 20 ribu tingkat
/// (`node --stack-size=3000`). Dibandingkan dengan jawaban tertulis, bukan interpreter: di runner CI interpreter
/// sendiri menolak 20 ribu tingkat (penjaga stack-nya), dan itu dulu terbaca sebagai kegagalan wasm.
const STACK_DALAM: &str = r#"fn dalam(n: int) -> int {
    if n == 0 { return 0 }
    return 1 + dalam(n - 1)
}
fn main() {
    var a: [2000]int = [7; 2000]
    var b: [2000]int = [7; 2000]
    b[1999] = 8
    wok(a == b, dalam(20000))
}
"#;

#[test]
fn stack_dalam_lewat_wasm() {
    let kerja = folder("stack");
    let p = tulis(&kerja, "stack.jrg", STACK_DALAM);
    let Some(w) = wasm(&p, &kerja) else { return };
    let _ = std::fs::remove_dir_all(&kerja);
    assert_eq!((w.kode, w.stdout.as_str()), (0, "false 20000\n"), "{}", w.stderr);
}

/// `JRG_EKSPOR_PARITAS=<dir> cargo test --test wasm_parity -- --ignored ekspor_paritas_llvm`: program `PARITAS_LLVM`
/// ke berkas, untuk sapuan wasm di luar suite (tiap kasus ±8 detik penyusunan).
#[test]
#[ignore]
fn ekspor_paritas_llvm() {
    let Ok(dir) = std::env::var("JRG_EKSPOR_PARITAS") else { return };
    std::fs::create_dir_all(&dir).expect("buat direktori ekspor");
    let mut daftar = String::new();
    for (i, (nama, src)) in PARITAS_LLVM.iter().enumerate() {
        std::fs::write(format!("{}/{:03}.jrg", dir, i), src).expect("tulis program");
        daftar.push_str(&format!("{:03}\t{}\n", i, nama));
    }
    std::fs::write(format!("{}/daftar.txt", dir), daftar).expect("tulis daftar");
}
