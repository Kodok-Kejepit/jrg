// tests/profiler.rs — tests/profiler.rs — mengunci `--profile` dan nada diagnostik `jrgm`.

mod bersama;
use std::process::Command;

fn jrgc() -> &'static str { env!("CARGO_BIN_EXE_jrgc") }
fn jrgm() -> &'static str { env!("CARGO_BIN_EXE_jrgm") }

struct Proyek(std::path::PathBuf);

impl Proyek {
    fn baru(isi: &str) -> Proyek {
        let d = crate::bersama::dir_unik("jrg-prof");
        std::fs::create_dir_all(d.join("src")).expect("bikin dir");
        std::fs::write(d.join("jrgm.toml"),
            "[package]\nname = \"uji\"\nversion = \"0.1.0\"\n").expect("tulis jrgm.toml");
        std::fs::write(d.join("src/main.jrg"), isi).expect("tulis sumber");
        Proyek(d)
    }
    fn jalankan(&self, prog: &str, arg: &[&str]) -> String {
        let out = Command::new(prog).args(arg).current_dir(&self.0)
            .output().expect("jalankan");
        format!("{}{}", String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr))
    }
}
impl Drop for Proyek {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

/// Program uji: punya fungsi biasa, method, closure, dan `main` — empat
/// jalur pemanggilan yang BERBEDA di interpreter, dan tiga di antaranya
/// pernah luput dari profiler.
const PROGRAM: &str = r#"package main
struct P { pub x: float = 0.0 }
impl P {
    fn dua(self) -> float { return self.x * 2.0 }
}
fn buat(v) { return P{x: v} }
pub fn main() {
    let a = buat(3.0)
    wok(a.dua())
    let f = |x| x + 1
    var i = 0
    while i < 5 { i = f(i) }
    wok(i)
}
"#;

fn baris_fungsi(laporan: &str) -> Vec<String> {
    // Baris tabel = antara garis pertama dan baris TOTAL.
    let mut out = Vec::new();
    let mut di_tabel = false;
    for b in laporan.lines() {
        if b.contains("Fungsi") && b.contains("Calls") { di_tabel = true; continue; }
        if !di_tabel { continue; }
        if b.contains("TOTAL") { break; }
        let t = b.trim();
        if t.is_empty() || t.starts_with('─') { continue; }
        if let Some(nama) = t.split_whitespace().next() {
            out.push(nama.to_string());
        }
    }
    out.sort();
    out
}

/// Laporan harus MUNCUL di kedua backend, lewat `jrgc` maupun `jrgm`.
#[test]
fn laporan_profiler_muncul_di_semua_jalur() {
    let p = Proyek::baru(PROGRAM);
    for (nama, prog, arg) in [
        ("jrgc interp", jrgc(), vec!["src/main.jrg", "--profile", "--sopan"]),
        ("jrgc vm",     jrgc(), vec!["src/main.jrg", "--profile", "--vm", "--sopan"]),
        ("jrgm interp", jrgm(), vec!["run", "--profile"]),
        ("jrgm vm",     jrgm(), vec!["run", "--profile", "--vm"]),
    ] {
        let keluaran = p.jalankan(prog, &arg);
        assert!(
            keluaran.contains("JRG PROFILER REPORT"),
            "{}: laporan profiler TIDAK muncul.\nkeluaran:\n{}", nama, keluaran
        );
        assert!(
            keluaran.contains("TOTAL"),
            "{}: laporan ada tapi tabelnya tidak lengkap.\nkeluaran:\n{}", nama, keluaran
        );
    }
}

/// Kedua backend harus menghitung himpunan fungsi yang SAMA.
#[test]
fn kedua_backend_menghitung_fungsi_yang_sama() {
    let p = Proyek::baru(PROGRAM);
    let interp = baris_fungsi(&p.jalankan(jrgm(), &["run", "--profile"]));
    let vm = baris_fungsi(&p.jalankan(jrgm(), &["run", "--profile", "--vm"]));
    assert!(!interp.is_empty(), "interpreter tidak melaporkan fungsi apa pun");
    assert_eq!(
        interp, vm,
        "himpunan fungsi berbeda antar-backend:\n  interp: {:?}\n  vm    : {:?}",
        interp, vm
    );
    for wajib in ["main", "P::dua", "buat", "<closure>"] {
        assert!(
            interp.iter().any(|n| n == wajib),
            "`{}` tidak ada di laporan — jalur pemanggilannya sepertinya tidak diprofilkan.\n\
             yang terlaporkan: {:?}", wajib, interp
        );
    }
}

/// `main` harus punya waktu yang MASUK AKAL, bukan 0.
#[test]
fn disiplin_masuk_keluar_profiler_tidak_bocor() {
    let p = Proyek::baru(PROGRAM);
    for arg in [vec!["run", "--profile"], vec!["run", "--profile", "--vm"]] {
        let keluaran = p.jalankan(jrgm(), &arg);
        let baris_main = keluaran.lines()
            .find(|b| b.trim_start().starts_with("main "))
            .unwrap_or_else(|| panic!("baris `main` tidak ada:\n{}", keluaran));
        assert!(
            !baris_main.contains(" 0.0µs "),
            "`main` tercatat 0,0µs ({:?}) — frame profiler bocor.\n{}", arg, baris_main
        );
        // `↻` berarti rekursi; program uji ini tidak punya satu pun, jadi
        // kalau ia muncul berarti ada frame yang tidak pernah ditutup.
        assert!(
            !keluaran.contains('↻'),
            "ada fungsi yang terdeteksi rekursif padahal programnya tidak — \
             frame profiler sepertinya menumpuk:\n{}", keluaran
        );
    }
}

/// Laporannya harus menjelaskan satuan dan istilahnya.
#[test]
fn laporan_menjelaskan_cara_membacanya() {
    let p = Proyek::baru(PROGRAM);
    let keluaran = p.jalankan(jrgm(), &["run", "--profile"]);
    for kunci in ["Cara baca", "Self", "Cumulative", "Min / Max", "Satuan",
                  "sepersejuta detik", "sepermiliar detik"] {
        assert!(
            keluaran.contains(kunci),
            "penjelasan `{}` tidak ada di laporan — angka waktu gampang salah \
             dibaca dengan cara yang tidak kelihatan salah.\n{}", kunci, keluaran
        );
    }
}

/// `jrgm` harus memberi diagnostik selengkap `jrgc`, dan menghormati nada.
#[test]
fn jrgm_memberi_diagnostik_penuh_dan_menghormati_nada() {
    let p = Proyek::baru(
        "package main\npub fn main() { let a = [1,2,3]; wok(a[9]) }\n"
    );
    let galak = p.jalankan(jrgm(), &["run"]);
    assert!(galak.contains("di luar batas"), "pesan intinya hilang:\n{}", galak);
    assert!(galak.contains("-->"), "lokasi tidak ditampilkan:\n{}", galak);
    assert!(galak.contains('^'), "caret tidak ditampilkan:\n{}", galak);
    // Sapaan = baris sebelum baris `error:`.
    let baris_error = galak.lines().position(|b| b.starts_with("error:"))
        .unwrap_or_else(|| panic!("tidak ada baris `error:`:\n{}", galak));
    assert!(
        baris_error > 0 && !galak.lines().next().unwrap_or("").is_empty(),
        "nada galak (bawaan) seharusnya menambahkan sapaan di atas error:\n{}", galak
    );

    let sopan = p.jalankan(jrgm(), &["--sopan", "run"]);
    assert!(
        sopan.starts_with("error:"),
        "`--sopan` seharusnya membuang sapaan:\n{}", sopan
    );
    // Yang dibuang HANYA sapaannya — badan diagnostiknya wajib identik.
    for kunci in ["di luar batas", "-->", "^"] {
        assert!(sopan.contains(kunci), "`--sopan` ikut membuang `{}`:\n{}", kunci, sopan);
    }
    // `--galak` eksplisit harus setara dengan bawaan.
    let eksplisit = p.jalankan(jrgm(), &["--galak", "run"]);
    assert!(
        !eksplisit.starts_with("error:"),
        "`--galak` seharusnya menampilkan sapaan:\n{}", eksplisit
    );
}

/// Kerja DI DALAM goroutine harus ikut terhitung, di kedua backend.
#[test]
fn kerja_di_dalam_goroutine_ikut_terhitung() {
    let p = Proyek::baru(
        "package main\n\
         fn kerja(n) { var s = 0; var i = 0; while i < n { s = s + i*i; i = i + 1 }; return s }\n\
         pub fn main() {\n\
         \x20   let ch = make_chan(int)\n\
         \x20   var i = 0\n\
         \x20   while i < 3 { spawn { ch <- kerja(20000) }; i = i + 1 }\n\
         \x20   var t = 0\n\
         \x20   var k = 0\n\
         \x20   while k < 3 { t = t + <-ch; k = k + 1 }\n\
         \x20   wok(t)\n\
         }\n"
    );
    let interp = p.jalankan(jrgm(), &["run", "--profile"]);
    let vm = p.jalankan(jrgm(), &["run", "--profile", "--vm"]);
    for (nama, keluaran) in [("interp", &interp), ("vm", &vm)] {
        assert!(
            keluaran.contains("kerja"),
            "{}: kerja di dalam goroutine tidak terhitung sama sekali:\n{}", nama, keluaran
        );
        assert!(
            keluaran.contains("4 utas"),
            "{}: goroutine-nya tidak menyetor hasil profilernya (utas tidak terhitung):\n{}",
            nama, keluaran
        );
    }
    assert_eq!(
        baris_fungsi(&interp), baris_fungsi(&vm),
        "himpunan fungsi berbeda antar-backend untuk program bergoroutine"
    );
    // Dengan beberapa utas, self-time WAJAR melebihi waktu dinding — dan
    // labelnya harus berubah jadi `paralel`, bukan `terukur` yang lewat
    // 100% dan terbaca seperti kesalahan.
    for (nama, keluaran) in [("interp", &interp), ("vm", &vm)] {
        assert!(
            keluaran.contains("paralel"),
            "{}: program berutas banyak masih memakai label `terukur`:\n{}", nama, keluaran
        );
    }
}
