// tests/llvm_parity.rs

mod bersama;
use std::process::Command;

fn jrgc() -> &'static str { env!("CARGO_BIN_EXE_jrgc") }

/// Apakah `clang` tersedia? Backend LLVM memanggilnya untuk menghasilkan
/// native code, jadi tanpanya tidak ada yang bisa diukur.
fn ada_clang() -> bool {
    let nama = std::env::var("JRGC_CLANG").unwrap_or_else(|_| "clang".to_string());
    Command::new(&nama).arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

struct Hasil {
    keluaran: String,
    galat: String,
    sukses: bool,
    /// Kode keluar apa adanya; lewat LLVM itu kode keluar biner hasil kompilasinya.
    kode: Option<i32>,
}

/// Direktori sementara unik per pemanggilan — tes berjalan paralel.
fn dir_uji(tag: &str) -> std::path::PathBuf {
    let d = crate::bersama::dir_unik(&format!("jrg-llvm-{}", tag));
    std::fs::create_dir_all(&d).expect("bikin dir sementara");
    d
}

fn jalankan_interp(src: &str) -> Hasil {
    let dir = dir_uji("interp");
    let file = dir.join("uji.jrg");
    std::fs::write(&file, src).expect("tulis sumber");
    let out = Command::new(jrgc()).arg(&file).env("JRG_SOPAN", "1")
        .env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY")
        .output().expect("jalankan jrgc");
    let _ = std::fs::remove_dir_all(&dir);
    Hasil {
        keluaran: String::from_utf8_lossy(&out.stdout).trim().to_string(),
        galat: String::from_utf8_lossy(&out.stderr).to_string(),
        sukses: out.status.success(),
        kode: out.status.code(),
    }
}

/// Kompilasi lewat `--emit-llvm` lalu jalankan binernya.
fn jalankan_llvm(src: &str) -> Hasil {
    let dir = dir_uji("emit");
    let file = dir.join("uji.jrg");
    std::fs::write(&file, src).expect("tulis sumber");
    let out = Command::new(jrgc())
        .arg(&file).arg("--emit-llvm").arg("-o").arg(dir.join("uji.bin"))
        .env("JRG_SOPAN", "1").env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY")
        .output().expect("jalankan jrgc --emit-llvm");
    let _ = std::fs::remove_dir_all(&dir);
    Hasil {
        keluaran: String::from_utf8_lossy(&out.stdout).trim().to_string(),
        galat: String::from_utf8_lossy(&out.stderr).to_string(),
        sukses: out.status.success(),
        kode: out.status.code(),
    }
}

/// Jalankan lewat `--vm`. Hanya dipakai sapuan tiga-mesin di bawah.
fn jalankan_vm(src: &str) -> Hasil {
    let dir = dir_uji("vm");
    let file = dir.join("uji.jrg");
    std::fs::write(&file, src).expect("tulis sumber");
    let out = Command::new(jrgc()).arg(&file).arg("--vm").env("JRG_SOPAN", "1")
        .output().expect("jalankan jrgc --vm");
    let _ = std::fs::remove_dir_all(&dir);
    Hasil {
        keluaran: String::from_utf8_lossy(&out.stdout).trim().to_string(),
        galat: String::from_utf8_lossy(&out.stderr).to_string(),
        sukses: out.status.success(),
        kode: out.status.code(),
    }
}

/// Nama-nama dari bahasa lain, **dibaca dari sumbernya** alih-alih ditulis
/// ulang.
fn nama_bahasa_lain() -> Vec<String> {
    let src = std::fs::read_to_string(
        concat!(env!("CARGO_MANIFEST_DIR"), "/src/interpreter/value.rs"))
        .expect("baca value.rs");
    let mulai = src.find("pub fn padanan_bahasa_lain").expect("fungsi padanan ada");
    let badan = &src[mulai..];
    let habis = badan.find("\n}").expect("akhir fungsi");
    let badan = &badan[..habis];

    let mut nama = Vec::new();
    for baris in badan.lines() {
        // Tiap lengan berbentuk `"a" | "b" => "penjelasan",` — yang diambil cuma
        // sisi KIRI `=>`, jadi teks penjelasannya tidak ikut terpanen.
        let Some(kiri) = baris.split("=>").next() else { continue };
        if !baris.contains("=>") { continue; }
        let mut sisa = kiri;
        while let Some(a) = sisa.find('"') {
            let sisa2 = &sisa[a + 1..];
            let Some(b) = sisa2.find('"') else { break };
            nama.push(sisa2[..b].to_string());
            sisa = &sisa2[b + 1..];
        }
    }
    nama
}

/// **Sebuah kata dari bahasa lain dijawab kalimat yang SAMA di tiga mesin.**
/// **Bulat lawan pecahan menjawab sama di tiga mesin** (Fase LLVM-16): tiap …
#[test]
fn bulat_lawan_pecahan_sama_di_tiga_mesin() {
    if !ada_clang() { eprintln!("lewati: clang tidak ada"); return; }
    let operand = ["3", "2.5", "3.0", "bi", "pf", "fb()", "ff()"];
    let operator = ["+", "-", "*", "/", "%", "**", "==", "!=", "<", "<=", ">", ">="];
    let mut badan = String::new();
    for op in operator { for a in operand { for b in operand {
        badan.push_str(&format!("    wok(\"{} {} {} =\", {} {} {})\n", a, op, b, a, op, b));
    } } }
    let src = format!("package main\nfn fb() -> int {{ return 3 }}\nfn ff() -> float {{ return 2.5 }}\n\
                       pub fn main() {{\n    let bi = 3\n    let pf = 2.5\n{}}}\n", badan);
    let (i, v, l) = (jalankan_interp(&src), jalankan_vm(&src), jalankan_llvm(&src));
    assert!(i.sukses, "acuan gagal:\n{}", i.galat);
    assert_eq!(i.keluaran, v.keluaran, "acuan dan --vm beda");
    assert!(l.sukses, "LLVM menolak:\n{}", l.galat);
    let beda: Vec<String> = i.keluaran.lines().zip(l.keluaran.lines())
        .filter(|(a, b)| a != b).map(|(a, b)| format!("  acuan {}  |  llvm {}", a, b)).collect();
    assert!(beda.is_empty(), "{} baris beda:\n{}", beda.len(), beda.join("\n"));
    assert_eq!(i.keluaran.lines().count(), operand.len() * operand.len() * operator.len());
}

include!("bersama/paritas.rs");

/// Program `PARITAS` yang LLVM JALANKAN tapi menjawab lain — dicatat dengan sebabnya sampai
/// ditutup. Daftar ini swa-retraksi: entri yang sudah sama memerahkan tesnya sebagai kabar baik.
const PARITAS_BEDA_DI_LLVM: &[(&str, &str)] = &[
    // Kosong sejak Fase URUTAN-LLVM (entri terakhirnya, tulisan yang hilang di `m[0] = m[0] + g()`,
    // ditutup). Entri berikutnya: ("nama kasus PARITAS", "sebabnya").
];

/// Modul yang di-`import` jalan lewat LLVM dan menjawab sama dengan dua mesin lain (Fase
/// MODUL-LLVM): alias ganda atas satu berkas berbagi satu `var`, modul yang mengimpor modul,
/// nama sama di dua berkas, method dan `fn` bersarang milik modul; yang privat ditolak sama.
#[test]
fn modul_pemakai_sama_di_tiga_mesin() {
    if !ada_clang() { eprintln!("lewati: clang tidak ada"); return; }
    let dir = dir_uji("modul");
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let tulis = |n: &str, isi: &str| std::fs::write(dir.join(n), isi).unwrap();
    tulis("bantu.jrg", "package bantu\nvar HITUNG = 0\nconst BATAS = 10\nlet SALAM = \"halo\"\n\
        fn _rahasia() { return 1 }\nfn naik() { HITUNG = HITUNG + 1; return HITUNG }\n\
        fn dua(x) { return x * 2 + _rahasia() }\nfn batas() { return BATAS }\n");
    tulis("sub/alat.jrg", "package alat\nimport \"bantu\"\nstruct Kotak { pub isi: int = 0 }\n\
        impl Kotak { fn dobel(self) { return self.isi * 2 } }\n\
        fn pakai(n) { let k = Kotak { isi: n }\n return k.dobel() + bantu::dua(n) }\n\
        fn kali(a, b) { fn dalam(x, y) { return x * y }\n return dalam(a, b) }\n\
        fn rusak() { let a = [1]\n return a[5] }\n");
    tulis("utama.jrg", "package main\nimport \"bantu\"\nimport \"bantu\" as b2\nimport \"sub/alat\"\n\
        fn dua(x) { return x + 1000 }\npub fn main() {\n\
        wok(bantu::dua(4), bantu::naik(), b2::naik(), bantu::batas(), bantu::BATAS, bantu::SALAM)\n\
        wok(dua(1), alat::pakai(3), alat::kali(2, 5)) }\n");
    tulis("privat.jrg", "package main\nimport \"bantu\"\npub fn main() { wok(bantu::_rahasia()) }\n");
    tulis("var.jrg", "package main\nimport \"bantu\"\npub fn main() { wok(bantu::HITUNG) }\n");
    tulis("lingkar_a.jrg", "package lingkar_a\nimport \"lingkar_b\"\nfn a() { return 1 }\n");
    tulis("lingkar_b.jrg", "package lingkar_b\nimport \"lingkar_a\"\nfn b() { return 2 }\n");
    tulis("lingkar.jrg", "package main\nimport \"lingkar_a\"\npub fn main() { wok(lingkar_a::a()) }\n");
    let jalan = |berkas: &str, mode: &[&str]| {
        let out = Command::new(jrgc()).arg(dir.join(berkas)).args(mode)
            .env("JRG_SOPAN", "1").current_dir(&dir).output().expect("jalankan jrgc");
        (String::from_utf8_lossy(&out.stdout).trim().to_string(),
         String::from_utf8_lossy(&out.stderr).to_string(), out.status.success())
    };
    let bin = dir.join("uji.bin");
    let llvm: Vec<&str> = vec!["--emit-llvm", "-o", bin.to_str().unwrap()];
    let mut masalah = Vec::new();
    let (i, _, oki) = jalan("utama.jrg", &[]);
    let (v, _, _) = jalan("utama.jrg", &["--vm"]);
    let (l, galat, okl) = jalan("utama.jrg", &llvm);
    if !oki || i != "9 1 2 10 10 halo\n1001 13 10" { masalah.push(format!("interpreter: {:?}", i)); }
    if v != i { masalah.push(format!("--vm beda: {:?}", v)); }
    if !okl || l != i { masalah.push(format!("llvm beda: {:?}\n{}", l, galat)); }
    for (berkas, kunci) in [("privat.jrg", "privat milik modul `bantu`"),
                            ("var.jrg", "adalah `var` milik modul `bantu`"),
                            ("lingkar.jrg", "lingkar_a")] {
        for mode in [&[][..], &llvm[..]] {
            let (_, g, ok) = jalan(berkas, mode);
            if ok || !g.contains(kunci) {
                masalah.push(format!("{} ({}): harus ditolak memuat {:?}, dapat: {}", berkas,
                    if mode.is_empty() { "interpreter" } else { "llvm" }, kunci, g.lines().next().unwrap_or("")));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(masalah.is_empty(), "{} masalah modul lewat LLVM:\n{}", masalah.len(), masalah.join("\n"));
}

/// Nama tipe dan trait milik berkasnya di TIGA mesin (Fase TRAIT-RUANG): LLVM dulu memegang
/// satu ruang tipe, jadi `Titik` milik dua berkas ditolak dan `impl a::Suara` memakai trait lain.
#[test]
fn tipe_dan_trait_bernama_sama_di_dua_berkas_sama_di_tiga_mesin() {
    if !ada_clang() { eprintln!("lewati: clang tidak ada"); return; }
    let dir = dir_uji("ruang");
    std::fs::create_dir_all(&dir).unwrap();
    let tulis = |n: &str, isi: &str| std::fs::write(dir.join(n), isi).unwrap();
    tulis("ta.jrg", "pub struct Titik { pub x: int }\npub enum Warna { Merah, Rgb(int) }\npub fn buat() { return Titik { x: 1 } }\npub fn w() { return Warna::Rgb(7) }\npub fn nama(t: Titik) -> string { return typeof(t) }\n");
    tulis("tipe.jrg", "package main\nimport \"ta\"\nstruct Titik { y: int }\nenum Warna { Biru }\npub fn main() {\n    let p = ta::buat()\n    wok(p, p.x, typeof(p), ta::nama(p))\n    let q = Titik { y: 2 }\n    wok(q, typeof(q))\n    wok(ta::w(), Warna::Biru)\n    wok(ta::Titik { x: 9 })\n    wok(str(p))\n}\n");
    tulis("a.jrg", "pub trait Suara { fn bunyi(self) -> string = \"a-default\" }\npub struct A { pub x: int }\nimpl Suara for A {}\npub fn coba() { return A { x: 1 }.bunyi() }\n");
    tulis("b.jrg", "pub trait Suara { fn bunyi(self) -> string = \"b-default\" }\npub struct B { pub x: int }\nimpl Suara for B {}\npub fn coba() { return B { x: 1 }.bunyi() }\n");
    tulis("c.jrg", "pub trait Wajib { fn harus(self) -> string\n  fn sapa(self) -> string = \"halo \" + self.harus() }\n");
    tulis("trait.jrg", "package main\nimport \"a\"\nimport \"b\"\ntrait Suara { fn bunyi(self) -> string = \"main-default\" }\nstruct M { x: int }\nimpl Suara for M {}\npub fn main() {\n    wok(a::coba(), b::coba(), M { x: 1 }.bunyi())\n}\n");
    tulis("impl.jrg", "package main\nimport \"a\"\nimport \"c\"\ntrait Suara { fn bunyi(self) -> string = \"main\" }\nstruct K { x: int }\nstruct L { x: int }\nimpl a::Suara for K {}\nimpl Wajib for L { fn harus(self) -> string = \"L\" }\npub fn main() {\n    wok(K { x: 1 }.bunyi(), L { x: 2 }.sapa())\n}\n");
    let jalan = |berkas: &str, mode: &[&str]| {
        let out = Command::new(jrgc()).arg(dir.join(berkas)).args(mode)
            .env("JRG_SOPAN", "1").current_dir(&dir).output().expect("jalankan jrgc");
        (String::from_utf8_lossy(&out.stdout).trim().to_string(),
         String::from_utf8_lossy(&out.stderr).to_string())
    };
    let bin = dir.join("uji.bin");
    let llvm: Vec<&str> = vec!["--emit-llvm", "-o", bin.to_str().unwrap()];
    let mut masalah = Vec::new();
    for (berkas, harap) in [("tipe.jrg", "Titik {x: 1} 1 struct:Titik struct:Titik"),
                            ("trait.jrg", "a-default b-default main-default"),
                            ("impl.jrg", "a-default halo L")] {
        let (i, _) = jalan(berkas, &[]);
        let (v, _) = jalan(berkas, &["--vm"]);
        let (l, g) = jalan(berkas, &llvm);
        if !i.starts_with(harap) { masalah.push(format!("{}: acuan {:?}", berkas, i)); }
        if v != i { masalah.push(format!("{}: --vm {:?}", berkas, v)); }
        if l != i { masalah.push(format!("{}: llvm {:?}\n{}", berkas, l, g)); }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(masalah.is_empty(), "{} masalah:\n{}", masalah.len(), masalah.join("\n"));
}

/// `Option` bersarang menjawab sama di TIAP kompilasi. Dijalankan berulang karena cacatnya
/// (Fase SIMPUL-2, #559) ada di urutan `HashMap`: `Option<Option<int>>` sesekali didaftarkan
/// sebelum `Option<int>`, dan satu jalan yang kebetulan benar meloloskannya dua pipeline.
#[test]
fn opsi_bersarang_deterministik() {
    if !ada_clang() { eprintln!("lewati: clang tidak ada"); return; }
    let src = "package main\nfn bungkus(x) { return Some(Some(x)) }\npub fn main() {\n\
        let dua = Some(Some(5))\n if let Some(dalam) = dua { if let Some(v) = dalam { wok(v) } }\n\
        let tiga = Some(Some(Some(\"x\")))\n match tiga { Some(Some(Some(s))) => wok(s), _ => wok(\"tidak\") }\n\
        match bungkus(7) { Some(Some(n)) => wok(n + 1), _ => wok(0) }\n\
        let f = |x| x > 0 $ Some(Some(x * 2)) : None\n wok(f(4), f(-1)) }";
    let harap = jalankan_interp(src);
    assert!(harap.sukses, "acuan gagal:\n{}", harap.galat);
    for putaran in 0..10 {
        let l = jalankan_llvm(src);
        assert!(l.sukses, "putaran {}: LLVM gagal:\n{}", putaran, l.galat);
        assert_eq!(l.keluaran, harap.keluaran, "putaran {}: jawaban LLVM berubah antar-kompilasi", putaran);
    }
}

/// JRG engine (`std/mesin`) di LLVM: ketiga contoh game diputar tanpa jendela dan keluarannya
/// harus sama persis dengan interpreter (Fase MESIN-LLVM).
#[test]
fn contoh_game_mesin_sama_di_llvm() {
    if !ada_clang() { eprintln!("lewati: clang tidak ada"); return; }
    let akar = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut salah = Vec::new();
    for nama in ["game_tangkap", "game_lompat", "game_menu"] {
        let src = std::fs::read_to_string(akar.join("examples").join(format!("{}.jrg", nama))).expect("baca contoh");
        let (i, l) = (jalankan_interp(&src), jalankan_llvm(&src));
        if !i.sukses || i.keluaran.is_empty() { salah.push(format!("{}: interpreter gagal\n{}", nama, i.galat)); continue; }
        if l.keluaran != i.keluaran || !l.sukses {
            salah.push(format!("{}:\n  acuan: {:?}\n  llvm:  {:?}\n  galat: {}", nama, i.keluaran, l.keluaran,
                l.galat.lines().take(6).collect::<Vec<_>>().join("\n         ")));
        }
    }
    assert!(salah.is_empty(), "contoh game berbeda di LLVM:\n{}", salah.join("\n"));
}

/// Perkakas, bukan tes: `JRG_EKSPOR_PARITAS=<dir> cargo test --test llvm_parity -- --ignored
/// ekspor_paritas` menulis tiap program `PARITAS` ke `<dir>/NNN.jrg` plus `daftar.txt`.
#[test]
#[ignore]
fn ekspor_paritas() {
    let Ok(dir) = std::env::var("JRG_EKSPOR_PARITAS") else { return };
    std::fs::create_dir_all(&dir).expect("buat direktori ekspor");
    let mut daftar = String::new();
    for (i, (nama, src)) in PARITAS.iter().enumerate() {
        std::fs::write(format!("{}/{:03}.jrg", dir, i), src).expect("tulis program");
        daftar.push_str(&format!("{:03}\t{}\n", i, nama));
    }
    std::fs::write(format!("{}/daftar.txt", dir), daftar).expect("tulis daftar");
}

/// Rekursi yang DALAM tapi berhenti dijawab LLVM dengan benar — penjaga stack tidak boleh
/// berupa penghitung kedalaman. Rekursi ekor jalan berapa pun dalamnya; yang bukan ekor
/// memakai stack sungguhan sejak `+` diperiksa (±500 ribu tingkat di stack 8 MiB).
#[test]
fn rekursi_dalam_yang_berhenti_tetap_dijawab_llvm() {
    if !ada_clang() { eprintln!("lewati: clang tidak ada"); return; }
    let l = jalankan_llvm("package main\nfn d(n: int) -> int { if n == 0 { return 0 }\n return d(n - 1) + 1 }\n\
        fn genap(n: int) -> bool { if n == 0 { return true }\n return ganjil(n - 1) }\n\
        fn ganjil(n: int) -> bool { if n == 0 { return false }\n return genap(n - 1) }\n\
        pub fn main() { wok(d(300000), genap(5000000)) }");
    assert!(l.sukses, "LLVM gagal:\n{}", l.galat);
    assert_eq!(l.keluaran.trim(), "300000 true");
}

/// **Tiap program `PARITAS` yang LLVM jalankan menjawab sama dengan acuan**
/// (Fase LLVM-16).
/// Bangun biner `--emit-llvm` tanpa menjalankannya. `None` kalau kompilasinya gagal.
fn bangun_llvm(dir: &std::path::Path, nama: &str, src: &str) -> Option<std::path::PathBuf> {
    let file = dir.join(format!("{}.jrg", nama));
    std::fs::write(&file, src).expect("tulis sumber");
    let bin = dir.join(format!("{}.bin", nama));
    let out = Command::new(jrgc()).arg(&file).arg("--emit-llvm").arg("-o").arg(&bin).arg("--no-run")
        .env("JRG_SOPAN", "1").env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY")
        .output().expect("jalankan jrgc --emit-llvm");
    out.status.success().then_some(bin)
}

/// Waktu tercepat dari `kali` putaran, dengan batas memori: bentuk kuadratik di backend ini
/// juga kuadratik dalam MEMORI, dan tanpa batas ia menghabiskan mesinnya sebelum gagal.
fn tercepat_biner(bin: &std::path::Path, kali: usize) -> Option<f64> {
    let mut terbaik: Option<f64> = None;
    for _ in 0..kali {
        let t = std::time::Instant::now();
        let out = Command::new("sh").arg("-c").arg("ulimit -v 3000000; exec \"$0\"").arg(bin)
            .output().expect("jalankan biner");
        if !out.status.success() { return None; }
        let d = t.elapsed().as_secs_f64();
        terbaik = Some(terbaik.map_or(d, |x: f64| x.min(d)));
    }
    terbaik
}

/// (nama, kepala, badan `main` dengan `N` di tempat ukurannya, N kecil, batas rasio untuk 8x N).
/// Linear berarti sekitar 8x, kuadratik 64x; yang O(1) terhadap N dibatasi 4x.
const BIAYA_LLVM: &[(&str, &str, &str, usize, f64)] = &[
    ("a = a + [x]", "", "var a: []int = []\n var i = 0\n while i < N { a = a + [i]\n i += 1 }\n wok(len(a))", 200_000, 24.0),
    ("a = a + [[x, x]]", "", "var a: [][]int = []\n var i = 0\n while i < N { a = a + [[i, i]]\n i += 1 }\n wok(len(a))", 100_000, 24.0),
    ("A = A + [x] atas var modul", "var A: []int = []\n", "var i = 0\n while i < N { A = A + [i]\n i += 1 }\n wok(len(A))", 200_000, 24.0),
    ("a = a + [x] lewat tangkapan closure", "",
     "var a: []int = []\n let f = |x: int| { a = a + [x] }\n var i = 0\n while i < N { f(i)\n i += 1 }\n wok(len(a))", 100_000, 24.0),
    ("p.xs = p.xs + [x]", "struct P { pub xs: []int }\n",
     "var p = P { xs: [] }\n var i = 0\n while i < N { p.xs = p.xs + [i]\n i += 1 }\n wok(len(p.xs))", 200_000, 24.0),
    ("q = q.push(x)", "struct Q { pub xs: []int }\nimpl Q { pub fn push(self, x: int) -> Q { self.xs += [x]\n return self } }\n",
     "var q = Q { xs: [] }\n var i = 0\n while i < N { q = q.push(i)\n i += 1 }\n wok(len(q.xs))", 200_000, 24.0),
    ("s = s + \"x\" melewati satu chunk arena", "", "var s = \"\"\n var i = 0\n while i < N { s = s + \"x\"\n i += 1 }\n wok(len(s))", 500_000, 24.0),
    ("d[k] = v", "", "var d: dict<int> = dict::new()\n var i = 0\n while i < N { d[str(i)] = i\n i += 1 }\n wok(len(d))", 50_000, 24.0),
    ("array sebagai argumen ke parameter yang cuma dibaca", "fn pjg(x: []int) -> int { return len(x) + x[0] }\n",
     "var a = arr::fill(0, N)\n var i = 0\n var s = 0\n while i < 200000 { s += pjg(a)\n i += 1 }\n wok(s)", 2_000, 4.0),
    ("struct berisi array sebagai argumen", "struct P { pub xs: []int }\nfn pjg(p: P) -> int { return len(p.xs) }\n",
     "var p = P { xs: arr::fill(0, N) }\n var i = 0\n var s = 0\n while i < 200000 { s += pjg(p)\n i += 1 }\n wok(s)", 2_000, 4.0),
    ("array bersarang sebagai argumen, satu elemennya dibaca", "fn satu(x: [][]int, i: int) -> int { return x[i][0] }\n",
     "var a: [][]int = []\n var i = 0\n while i < N { a = a + [[i]]\n i += 1 }\n var s = 0\n i = 0\n while i < 100000 { s += satu(a, i % 1000)\n i += 1 }\n wok(s)", 1_000, 4.0),
];

/// Padanan `memory_model.rs`/`vm_complexity.rs` untuk backend LLVM: ukuran dikali 8, lalu
/// rasio waktunya dibandingkan dengan batasnya.
#[test]
fn biaya_di_llvm_tetap_linear() {
    if !ada_clang() { eprintln!("lewati: clang tidak ada"); return; }
    let dir = dir_uji("biaya");
    let mut temuan = Vec::new();
    for (i, (nama, kepala, badan, n, batas)) in BIAYA_LLVM.iter().enumerate() {
        let sumber = |n: usize| format!("package main\n{}pub fn main() {{\n {}\n}}\n", kepala, badan.replace('N', &n.to_string()));
        let (Some(kecil), Some(besar)) = (bangun_llvm(&dir, &format!("k{}", i), &sumber(*n)),
                                          bangun_llvm(&dir, &format!("b{}", i), &sumber(*n * 8))) else {
            temuan.push(format!("  {}: tidak terkompilasi", nama));
            continue;
        };
        let Some(a) = tercepat_biner(&kecil, 3) else { temuan.push(format!("  {}: gagal di N = {} (batas memori 3 GB)", nama, n)); continue };
        let Some(b) = tercepat_biner(&besar, 3) else {
            temuan.push(format!("  {}: gagal di N = {} (kehabisan memori berarti menyalin tiap langkah)", nama, n * 8));
            continue;
        };
        let rasio = b / a;
        if rasio > *batas {
            temuan.push(format!("  {}: {:.1}x untuk 8x N (batas {}x; {:.1} ms -> {:.1} ms)", nama, rasio, batas, a * 1e3, b * 1e3));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(temuan.is_empty(), "biaya di backend LLVM tumbuh lebih cepat dari ukurannya:\n{}", temuan.join("\n"));
}

#[test]
fn tiap_program_paritas_yang_jalan_di_llvm_menjawab_sama() {
    if !ada_clang() { eprintln!("lewati: clang tidak ada"); return; }
    let kerja = std::sync::Mutex::new(PARITAS.iter().collect::<Vec<_>>());
    let temuan = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| loop {
                let Some((nama, src)) = kerja.lock().unwrap().pop() else { break };
                let l = jalankan_llvm(src);
                let tercatat = PARITAS_BEDA_DI_LLVM.iter().any(|(n, _)| n == nama);
                // Ditolak LLVM: celah backend yang keras, boleh — tapi entri tercatatnya basi.
                if !l.sukses {
                    if tercatat {
                        temuan.lock().unwrap().push(format!(
                            "  KABAR BAIK, sekarang DITOLAK LLVM — hapus dari PARITAS_BEDA_DI_LLVM: {}", nama));
                    }
                    continue;
                }
                let i = jalankan_interp(src);
                let sama = i.sukses && i.keluaran == l.keluaran;
                if sama == tercatat {
                    temuan.lock().unwrap().push(if sama {
                        format!("  KABAR BAIK, sekarang sama — hapus dari PARITAS_BEDA_DI_LLVM: {}", nama)
                    } else {
                        format!("  {}\n    acuan: {:?}\n    llvm:  {:?}", nama, i.keluaran, l.keluaran)
                    });
                }
            });
        }
    });
    let mut temuan = temuan.into_inner().unwrap();
    temuan.sort();
    assert!(temuan.is_empty(), "{} program PARITAS lewat LLVM:\n{}", temuan.len(), temuan.join("\n"));
}

#[test]
fn kata_bahasa_lain_dijawab_sama_di_tiga_mesin() {
    if !ada_clang() {
        eprintln!("LEWAT: `clang` tidak ada, sapuan tiga mesin tidak bisa dijalankan");
        return;
    }
    let nama = nama_bahasa_lain();
    assert!(nama.len() >= 20,
        "pemanenan nama dari `padanan_bahasa_lain` mengecil ({} nama) — \
         sebuah sapuan yang tidak menyapu apa pun lulus dengan sempurna", nama.len());

    let mut beda = Vec::new();
    for n in &nama {
        // Nama yang berakhiran `_` adalah ejaan yang memang tidak bisa ditulis
        // sebagai kata kunci JRG di posisi ini; dilewati dengan sebab itu.
        if n.ends_with('_') { continue; }
        let src = format!("package main\npub fn main() {{ wok({}) }}\n", n);
        let i = jalankan_interp(&src);
        let v = jalankan_vm(&src);
        let l = jalankan_llvm(&src);
        let baris = |h: &Hasil| h.galat.lines().next().unwrap_or("").to_string();
        let (bi, bv, bl) = (baris(&i), baris(&v), baris(&l));
        if i.sukses || v.sukses || l.sukses {
            beda.push(format!("  `{}` DITERIMA di salah satu mesin", n));
        } else if bi != bv || bi != bl {
            beda.push(format!("  `{}`\n    int : {}\n    vm  : {}\n    llvm: {}", n, bi, bv, bl));
        }
    }
    assert!(beda.is_empty(),
        "{} kata dari bahasa lain dijawab berbeda antar-mesin:\n{}",
        beda.len(), beda.join("\n"));
}

// ─── Tabel 1: harus identik
// ────────────────────────────────────────────────

const PARITAS_LLVM: &[(&str, &str)] = &[
    // Fase KEPUTUSAN-1: `-2 ** 2` = -4, indeks negatif di builtin dari belakang.
    ("pangkat lawan minus, indeks negatif di builtin", r#"package main
pub fn main() {
    let x = 3
    wok(-2 ** 2, -x ** 2, (-2) ** 2, ~2 ** 2, -2 ** 3 ** 2, -128 as i8, - 2 ** 2 + 1, -2.0 ** 2)
    let a = [10, 20, 30]
    wok(a.get(-1), a.get(-4), a.get_or(-1, 0), a.get_or(-9, 0))
    wok(a.remove(-1), a.insert(-1, 9), arr::insert(a, -3, 5), a.swap(-1, 0), "abc".char_at(-1))
}
"#),
    // Fase JAWA-LLVM: `jawa::array(n, isi)` dengan isi awal.
    ("jawa::array dengan isi awal", r#"package main
pub fn main() {
    let a = jawa::array(3, 7)
    jawa::array_set(a, 1, 9)
    wok(jawa::array_get(a, 0), jawa::array_get(a, 1), jawa::array_len(a), jawa::array_to_list(a))
    jawir::free(a)
    wok(jawa::is_valid(a))
}
"#),
    // Fase PETA-OPSI: `map`/`map_err`/`and_then` atas Option dan Result, termasuk yang mengganti tipe isinya.
    ("peta atas Option dan Result", r##"package main
fn bagi(a: float, b: float) -> Result<float, string> {
    if b == 0.0 { return Err("nol") }
    return Ok(a / b)
}
fn cari(xs: []int, x: int) -> Option<int> {
    for i, v in xs { if v == x { return Some(i) } }
    return None
}
fn ke_teks(n: int) -> string { return "#" + str(n) }
pub fn main() {
    wok(Some(2).map(|x| x + 1), cari([1, 2], 9).map(|i| i * 10))
    wok(bagi(20.0, 4.0).map(fn(v) => v * 2.0), bagi(1.0, 0.0).map(|v| v * 2.0))
    wok(bagi(1.0, 0.0).map_err(|s| s + "!"), bagi(2.0, 1.0).map_err(|s| s + "!"))
    wok(cari([5, 6], 6).and_then(|i| cari([0, 1, 2], i)), cari([5], 6).and_then(|i| Some(i)))
    wok(bagi(8.0, 2.0).and_then(|v| bagi(v, 2.0)), bagi(8.0, 0.0).and_then(|v| bagi(v, 2.0)))
    wok(cari([3, 4], 4).map(ke_teks), Some(1).map(|x| x + 1).map(|x| x * 5))
    let urutan = Some(1).map(|x| { wok("di dalam f"); x })
    wok(urutan)
}
"##),
    // Fase KTOR-LLVM: konstruktor enum non-generik sebagai nilai — tampilan, `typeof`, HOF, `|>`.
    ("konstruktor sebagai nilai", r#"package main
enum E { A(int), B, C(int, string) }
enum W { K(string), L }
fn terap(f: fn(string) -> W, x: string) -> W { return f(x) }
fn dua(x: int) -> int { return x * 2 }
pub fn main() {
    let f = E::A
    wok(f, typeof(f), [f], str(f))
    wok(f(3), arr::map([1, 2], E::A))
    let g = E::C
    wok(g(1, "x"))
    let k = K
    wok(k("a"), "b" |> W::K, terap(W::K, "c"))
    let fs = [W::K, W::K]
    wok(fs[1]("d"))
    wok(dua, typeof(dua), typeof(|x| x))
}
"#),
    // Fase SUBSTR: awal `str::substr` negatif, dan eksponen negatif TERTULIS menjawab pecahan.
    ("substr awal negatif, pangkat negatif tertulis", r#"package main
fn separuh(x: int) { return x ** -1 }
pub fn main() {
    let s = "halo dunia"
    wok(str::substr(s, -5, 3), s.substr(-5), str::substr(s, -10, 4), "[" + str::substr(s, 10) + "]", str::substr("héllo", -4, 2))
    wok(2 ** -1, 3 ** -2, typeof(2 ** -1), (-2) ** -3, 10 ** -3 * 1000.0, separuh(4))
    let x = 5
    let h = x ** -2
    wok(h, typeof(h))
}
"#),
    // Fase JRGM-2: `http::` menjawab `Result` (dulu galat saat jalan di tiga mesin).
    ("http: gagal sambung dan URL tak sah jadi Err", r#"package main
pub fn main() {
    match http::get("http://127.0.0.1:1/x") { Ok(j) => wok("ok"), Err(e) => wok("err", str::contains(e, "sambungan ditolak")) }
    wok(http::get("http://127.0.0.1:abc/"))
}
"#),
    // Fase PEGANGAN-LLVM: dulu entri `BELUM_DIDUKUNG_LLVM` ("`i64` sebagai `Atomic`").
    ("pegangan dibandingkan dengan jenis lain menjawab false", r#"package main
pub fn main() {
 let a = Atomic::new(1)
 let m = Mutex::new()
 let c = make_chan(int)
 wok(a == 1, a != 1, m == "x", c == nil, 1 == a, a == m)
}
"#),
    // Fase KALIMAT: `jawa::sizeof` atas nilai biasa menaksir ukurannya, dulu ditolak LLVM.
    ("jawa::sizeof atas nilai yang bukan pegangan", r#"package main
pub fn main() {
 wok(jawa::sizeof(5), jawa::sizeof("abc"), jawa::sizeof([1,2]), jawa::sizeof(2.5), jawa::sizeof(true))
 let b = jawa::buffer(4)
 wok(jawa::sizeof(b))
 wok(jawa::sizeof({"a": 1}))
}
"#),
    // Fase HEKS: buffer dari teks heksa lahir di runtime LLVM, bukan di registri jembatan.
    ("jawa::buffer_from_hex dibaca buffer_get dan buffer_to_hex", r#"package main
pub fn main() {
 let b = jawa::buffer_from_hex("00ff41")
 wok(jawa::buffer_get(b, 1))
 wok(jawa::buffer_len(b))
 let c = jawa::buffer(3)
 wok(jawa::buffer_get(c, 0))
}
"#),
    ("jawa::buffer_to_hex atas buffer dari teks heksa", r#"package main
pub fn main() {
 let c = jawa::buffer(2)
 jawa::buffer_set(c, 0, 171)
 wok(jawa::buffer_to_hex(c))
 wok(jawa::buffer_to_hex(jawa::buffer_from_hex("ABcd")))
 wok(jawa::buffer_len(jawa::buffer_from_hex("")))
}
"#),
    // Fase SALIN-MODUL: nilai milik `var` modul yang dibaca lewat penunjuk disalin kalau yang
    // dipanggil sesudahnya bisa menulis `var` modul. Yang pertama dulu entri `DIVERGENSI_LLVM`.
    ("operan array var modul ditulis operan berikutnya",
     "package main\nvar A = [1, 2]\nfn ubah() -> []int { A[0] = 4; return [5] }\npub fn main() { let x = A + ubah()\n wok(x) }"),
    ("self milik var modul tidak ikut tulisan fungsi lain, operan kiri dibaca lebih dulu", r#"package main
struct K { pub n: int, pub xs: []int }
var G = K { n: 1, xs: [1] }
fn ubah() { G = K { n: 99, xs: [9, 9] } }
impl K {
    fn f(self) -> int {
        let a = self.n
        ubah()
        return a + self.n + len(self.xs)
    }
    fn baca(self) -> int { return self.n }
}
var A = [1, 2, 3]
fn tulis() -> int { A[0] = 4; return 5 }
fn ubah2() -> []int { A[0] = 7; return [5] }
pub fn main() {
    wok(G.f())
    wok(G.baca())
    wok(A + [tulis()])
    wok(A + ubah2())
    let x = A + [1]
    wok(x)
}
"#),
    // Fase RUANG-LLVM: pola varian atas nilai yang pasti bukan enum tidak cocok, bukan ditolak.
    ("pola varian telanjang dan berawalan atas struct jatuh ke lengan lain", r#"package main
enum E { X(int), Y }
struct P { a: int }
pub fn main() {
 let p = P { a: 1 }
 wok(match p { Y => "y", _ => "lain" })
 wok(match p { E::Y => "y", _ => "lain" })
}
"#),
    // Fase GENAP: pintu byte dan `fs::` yang dulu tidak ada di sini, lewat jembatan.
    ("crypto: array byte dan aes_decrypt_bytes", r#"package main
pub fn main() {
    let k = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"
    let iv = "000102030405060708090a0b0c0d0e0f"
    let s = crypto::aes_encrypt([0, 255, 65], k, iv).unwrap()
    wok(crypto::aes_decrypt(s, k, iv))
    wok(crypto::aes_decrypt_bytes(s, k, iv))
    wok(crypto::aes_decrypt_bytes(crypto::aes_encrypt("hé", k, iv).unwrap(), k, iv))
    wok(crypto::aes_decrypt_bytes(s, "00", iv))
    wok(crypto::base64_encode([0, 255, 65]), crypto::base64_encode("hé"))
    wok(crypto::hex_encode([0, 255]), crypto::hex_encode("A"))
    wok(crypto::sha256([104, 105]) == crypto::sha256("hi"), crypto::md5([]) == crypto::md5(""))
    wok(crypto::crc32([97]), crypto::crc32("a"), crypto::hmac_sha256([104], "x") == crypto::hmac_sha256("h", "x"))
    wok(crypto::base64_encode(crypto::base64_decode_bytes("AP9B").unwrap()))
}
"#),
    ("fs: try_read dan pintu base64 lewat jembatan", r#"package main
pub fn main() {
    let d = fs::join(fs::temp_dir(), "jrg-l3")
    fs::mkdir(d)
    let f = fs::join(d, "b.bin")
    wok(fs::write_base64(f, "AP9B"))
    wok(fs::read_base64(f))
    wok(fs::try_read(f))
    wok(fs::try_read(fs::join(d, "hilang")))
    wok(fs::append_base64(f, "Qg=="))
    wok(fs::size(f))
    fs::write(fs::join(d, "t.txt"), "hé")
    wok(fs::try_read(fs::join(d, "t.txt")))
    fs::delete(d)
    wok(fs::exists(d))
}
"#),
    ("dok: BLOB SQLite jadi array byte, TEXT bukan UTF-8 jadi Err", r#"package main
pub fn main() {
    if !dok::db_tersedia() { wok("tanpa sqlite")  return }
    let db = dok::db_open(":memory:").unwrap()
    wok(dok::db_query(db, "SELECT x'00ff41' AS b, 'é' AS t, 3 AS n"))
    wok(dok::db_query(db, "SELECT CAST(x'ff41' AS TEXT) AS rusak"))
    wok(dok::db_query(db, "SELECT CAST(CAST(x'ff41' AS TEXT) AS BLOB) AS rusak"))
    dok::db_close(db)
}
"#),
    ("dok: tcp_recv tidak membelah karakter, pintu byte TCP dan UDP", r#"package main
fn port(l) {
    let a = dok::tcp_local_addr(l)
    let p = str::split(a, ":")
    return str::to_int(p[p.len() - 1])
}
fn port_udp(u) {
    let p = str::split(dok::udp_local_addr(u), ":")
    return str::to_int(p[p.len() - 1])
}
pub fn main() {
    let l = dok::tcp_listen("127.0.0.1", 0).unwrap()
    let k = dok::tcp_connect("127.0.0.1", port(l), 2000).unwrap()
    let c = dok::tcp_accept(l).unwrap()
    dok::tcp_send(k, "aaaé€b").unwrap()
    wok(dok::tcp_recv(c, 4))
    wok(dok::tcp_recv(c, 4))
    wok(dok::tcp_recv(c, 1))
    wok(dok::tcp_recv(c, 4))
    dok::tcp_send(k, [97, 255, 98]).unwrap()
    wok(dok::tcp_recv(c, 8))
    wok(dok::tcp_recv(c, 8))
    wok(dok::tcp_recv_bytes(c, 8))
    dok::tcp_send(k, [0, 200, 10]).unwrap()
    wok(dok::tcp_recv_line(c))
    dok::tcp_send(k, [226, 130]).unwrap()
    dok::tcp_close(k)
    wok(dok::tcp_recv(c, 8))
    wok(dok::tcp_recv(c, 8))
    dok::tcp_close(c)
    dok::tcp_close_listener(l)
    let u = dok::udp_bind("127.0.0.1", 0).unwrap()
    let v = dok::udp_bind("127.0.0.1", 0).unwrap()
    let pu = port_udp(u)
    dok::udp_send_to(v, "127.0.0.1", pu, "héé").unwrap()
    wok(dok::udp_recv_from(u, 64).unwrap().data)
    dok::udp_send_to(v, "127.0.0.1", pu, "héé").unwrap()
    wok(dok::udp_recv_from(u, 2).unwrap().data)
    dok::udp_send_to(v, "127.0.0.1", pu, [1, 255]).unwrap()
    wok(dok::udp_recv_from(u, 64))
    wok(dok::udp_recv_bytes_from(u, 64).unwrap().data)
}
"#),
    ("crypto: dekode byte bukan teks", r#"package main
pub fn main() {
    wok(crypto::base64_decode_bytes("AP9B"))
    wok(crypto::base64_decode("AP9B"))
    wok(crypto::hex_decode_bytes("00ff"))
}
"#),
    // Fase SEBAR: basis bertipe lain sah selama field-nya bagian dari sasaran.
    ("basis sebar bertipe struct lain yang field-nya bagian dari sasaran", r#"package main
struct B { pub x: int, pub y: int = 7 }
struct H { pub x: int }
struct K: H { pub k: int }
struct N { pub nama: string, pub x: int }
struct M { pub x: int, pub nama: string, pub z: int = 3 }
pub fn main() {
    let h = H { x: 4 }
    wok(K { k: 1, ...h })
    wok(B { ...h })
    let n = N { nama: "nn", x: 8 }
    wok(M { ...n })
    wok(M { x: 1, ...n })
    let b = B { x: 1, y: 2 }
    wok(B { x: 5, ...b })
}"#),
    // Fase BINER: LLVM dulu menjawab "tipe `V` tidak dikenal" untuk varian yang ditulis telanjang.
    ("literal varian ber-field yang ditulis telanjang", r#"package main
enum E { V { a: int, b: int }, K }
fn lihat(e: E) -> int {
    match e { E::V { a, b } => a + b, E::K => 0 }
}
pub fn main() {
    let e = V { a: 1, b: 2 }
    wok(lihat(e))
    wok(lihat(V { b: 5, a: 4 }))
    wok(V { a: 7, b: 8 })
}"#),
    // Fase PULIH: `A { ...a }` dulu ditolak parser di tiga mesin.
    ("literal struct yang isinya cuma basis sebar",
     "package main\nstruct A { pub x: int, pub y: int }\npub fn main() {\n let a = A { x: 1, y: 2 }\n wok(A { ...a })\n let b = A { ...a }\n wok(b.x + b.y)\n}"),
    // Fase WAJIB: urutan evaluasi default field sama di tiga mesin.
    ("default field struct: hanya yang tidak diisi, sesudah field tertulis", r#"package main
fn catat(s: string) -> int { wok(s); return 1 }
struct A { pub x: int = catat("default A.x"), pub y: int = catat("default A.y"), pub z: int = catat("default A.z") }
struct B: A { pub w: int = catat("default B.w"), pub y: int = catat("default B.y") }
pub fn main() {
    wok(A { y: catat("tulis y") })
    let a = A { x: 5, y: 6, z: 7 }
    wok(A { y: catat("tulis y2"), ...a })
    wok(B { z: catat("tulis z") })
    wok(B {})
}
"#),
    // Fase BONGKAR: `q, v = q.pop()` di biner berjalan di tempat `q`. 200 ribu elemen —
    // bentuk dua pernyataan kehabisan memori di ukuran ini.
    ("std/queue, deque, heap: q, v = q.pop() atas 200 ribu elemen", r#"package main
import "std/queue"
import "std/deque"
import "std/heap"
pub fn main() {
    var q: queue::Queue<int> = queue::new()
    var d: deque::Deque<int> = deque::new()
    var h: heap::Heap<int, int> = heap::new()
    var s = 0
    var v: Option<int> = None
    for i in 0..200000 { q = q.push(i)
      d = d.push_front(i) }
    for i in 0..20000 { h = h.push((i * 7919) % 10007) }
    for i in 0..200000 { q, v = q.pop()
      s = s + (v ?? 0)
      d, v = d.pop_back()
      s = s + (v ?? 0) }
    for i in 0..20000 { h, v = h.pop()
      s = s + (v ?? 0) }
    wok(s); wok(q.len()); wok(d.len()); wok(h.len()); wok(q.pop()[1])
}
"#),
    // Fase TUTUP: array literal yang meluruh jadi slice DISALIN ke heap. Dulu slice-nya
    // menunjuk stack fungsinya, dan `return [v, v + 1]` menjawab sampah.
    ("array literal jadi slice lalu keluar dari fungsi: return, field, Some", r#"package main
fn f(v: int) -> []int { return [v, v + 1] }
fn g(v: int) -> []int { let a = [v, v * 2]
  return a }
fn kotor(n: int) -> int { var t = [9, 9, 9, 9, 9, 9, 9, 9]
  return t[n] + n }
struct K { pub xs: []int }
fn h(v: int) -> K { return K { xs: [v, 7] } }
fn o(v: int) -> Option<[]int> { return Some([v]) }
pub fn main() {
    let a = f(3)
    let b = g(5)
    let c = h(4)
    let d = o(6)
    wok(kotor(1))
    wok(a); wok(b); wok(c); wok(d)
}
"#),
    ("match di ekor fungsi dengan lengan array beda panjang; return [v] lawan return []", r#"package main
fn ta(o) {
    match o {
        Some(v) => [v],
        None    => [],
    }
}
fn tb(o) {
    match o {
        Some(v) => return [v, v],
        None => return [],
    }
}
pub fn main() { wok(ta(Some(3))); wok(typeof(ta(Some(3)))); let k: Option<int> = None
  wok(ta(k)); wok(tb(Some(4))); wok(tb(k)) }
"#),
    ("separuh tipe tak terlihat jadi nilai bertag: None, Ok, Err, var x = nil", r#"package main
pub fn main() {
    var kosong = nil
    kosong = 5
    wok(kosong); kosong = "teks"; wok(kosong)
    var x = None
    wok(x); x = Some(3); wok(x); x = Some("teks"); wok(x)
    var r = Ok(4)
    wok(r); r = Err("rusak"); wok(r)
    let e = Err(5)
    wok(e); wok(typeof(e)); wok(e == Err(5)); wok(None == None)
    match x { Some(v) => wok(v), None => wok("kosong") }
    let daftar = [None, Some(1)]
    wok(daftar); wok(len(daftar))
    wok(x ?? "bawaan"); let y = None
    wok(y ?? 7)
}
"#),
    ("x = f(x, ..) dan nama tertangkap: closure melihat nilai LAMA selagi fungsinya jalan", r#"package main
struct K { pub xs: []int }
impl K {
    pub fn terap(self, f: fn() -> int) -> K { self.xs += [1]
      self.xs += [f()]
      return self }
}
fn pakai(a: []int, f: fn() -> int) -> []int { a = arr::push(a, 1)
  a = arr::push(a, f())
  return a }
pub fn main() {
    var p = K { xs: [5] }
    let g = || len(p.xs)
    p = p.terap(g)
    wok(p.xs)
    var a = [5]
    let h = || len(a)
    a = pakai(a, h)
    wok(a)
    var b = [1]
    let b0 = b
    b = pakai(b, || 0)
    wok(b); wok(b0)
}
"#),
    ("x = f(x, ..): dict::set, dict::remove, fungsi pemakai — tanpa alias", r#"package main
struct K { pub xs: []int }
fn tambah(k, x) { k.xs += [x]
  return k }
fn isi(d, k, v) { d[k] = v
  return d }
fn dorong(a, x) { a = arr::push(a, x)
  return a }
fn rek(d, n) { if n == 0 { return d }
  d = isi(d, n, n * n)
  d = rek(d, n - 1)
  return d }
fn banyak(a, xs...) { for _, x in xs { a = a + [x] }
  return a }
fn bawaan(a, x = 7) { a = a + [x]
  return a }
fn gagal(d, i) { let a = [1]
  d["x"] = a[i]
  return d }
pub fn main() {
    var k = K { xs: [1] }
    let k0 = k
    var k1 = k
    k = tambah(k, 2)
    k = tambah(tambah(k, 3), 4)
    wok(k); wok(k0); wok(k1)
    var d = {"a": 1}
    let d0 = d
    d = dict::set(d, "b", 2)
    d = isi(d, "c", 3)
    d = dict::remove(d, "a")
    d = dict::remove(d, "tak ada")
    wok(d); wok(d0)
    let f = || { d = dict::set(d, "f", 0) }
    f()
    wok(d)
    var a = [1]
    let a0 = a
    a = dorong(a, 2)
    a = banyak(a, 3, 4)
    a = bawaan(a)
    a = bawaan(a, 9)
    wok(a); wok(a0)
    wok(rek({"0": 0}, 4))
    var e = {"z": 1}
    e = rek(e, 2)
    wok(e)
    var s = "ab"
    s = str::upper(s)
    wok(s)
    var t = {"k": 1}
    wok("sebelum")
}
"#),
    // Fase BERTIPE: penyalin-dalam menembus struct bersarang dan payload enum; nilai yang
    // dikembalikan dari `self` pinjaman, dari pengikat pola, dan dari `var` modul disalin.
    // Dua puluh dari 34 baris ini dulu dijawab salah tanpa pesan.
    ("alias: kembalian dari self, struct bersarang, enum, pola, var modul", r#"package main
struct K { pub a: []int, pub b: []int }
impl K {
    pub fn sama(self) -> K { return K { a: self.a, b: self.b } }
    pub fn dorong(self, x: int) -> K { return K { a: arr::push(self.a, x), b: self.b } }
    pub fn isi(self) -> []int { return self.a }
}
fn lewat(k: K) -> K { return k }
fn ambil(k: K) -> []int { return k.b }
pub fn main() {
    var k = K { a: [1, 2], b: [7, 8] }
    var m = k.sama()
    m.a[0] = 9; m.b[1] = 0
    wok(k); wok(m)
    var n = k.dorong(3)
    n.b[0] = 100; n.a[0] = 50
    wok(k); wok(n)
    var x = k.isi()
    x[1] = 77
    wok(k); wok(x)
    var l = lewat(k)
    l.a[0] = -1
    var y = ambil(k)
    y[0] = -2
    wok(k); wok(l); wok(y)
    let p = k.dorong(4)
    let r = k.dorong(5)
    wok(p.a); wok(r.a)
    var t = (k.a, k.b)
    t[0][0] = 31
    wok(k); wok(t)
    kedua()
    ketiga()
}
struct D { pub d: dict<int>, pub n: K, pub o: Option<[]int> }
impl D {
    pub fn diri(self) -> D { return self }
    pub fn dik(self) -> dict<int> { self.d }
    pub fn dalam(self) -> []int = self.n.a
    pub fn pasang(self) -> (K, []int) { return (self.n, self.n.b) }
    pub fn pilih(self, c: bool) -> []int { if c { return self.n.a }
      return self.n.b }
    pub fn opsi(self) -> []int { match self.o { Some(v) => return v, None => return [] } }
}
pub fn kedua() {
    var d = D { d: {"a": 1}, n: K { a: [1, 2], b: [3] }, o: Some([5, 6]) }
    var e = d.diri()
    e.n.a[0] = 9; e.d["a"] = 2
    var f = d.dik()
    f["a"] = 3; f["z"] = 0
    var g = d.dalam()
    g[1] = 8
    var h = d.pasang()
    h[0].a[0] = 7; h[1][0] = 6
    var i = d.pilih(true)
    i[0] = 5
    var j = d.opsi()
    j[0] = 4
    wok(d); wok(e); wok(f); wok(g); wok(h); wok(i); wok(j)
}
var G = [1, 2]
enum Bentuk { Daftar([]int), Kosong }
fn ambil_g() { return G }
fn isi_bentuk(b: Bentuk) -> []int { match b { Bentuk::Daftar(xs) => return xs, Bentuk::Kosong => return [] } }
pub fn ketiga() {
    var x = ambil_g()
    x[0] = 9
    wok(G); wok(x)
    var o = Some([1, 2])
    var p = o
    match p { Some(v) => { v[0] = 7
        wok(v) } None => {} }
    wok(o); wok(p)
    var b = Bentuk::Daftar([3, 4])
    var c = b
    var z = isi_bentuk(c)
    z[0] = 0
    wok(isi_bentuk(b)); wok(isi_bentuk(c)); wok(z)
    var d1 = D { d: {"a": 1}, n: K { a: [1, 2], b: [3] }, o: Some([5, 6]) }
    var d2 = d1
    d2.n.a[0] = 99
    let ds = [d1, d2]
    var d3 = ds[0]
    d3.n.b[0] = 55
    wok(d1.n); wok(d2.n); wok(ds[0].n); wok(d3.n)
    let tp = (K { a: [1], b: [2] }, Some([3]))
    var tq = tp
    tq[0].a[0] = 4
    wok(tp); wok(tq)
}
"#),
    ("std/queue, deque, heap bertipe; parameter tipe tak terlihat jadi nilai bertag", r#"package main
import "std/queue"
import "std/deque"
import "std/heap"
struct Titik { pub x: int }
pub fn main() {
    var q: queue::Queue<Titik> = queue::new()
    q = q.push(Titik { x: 1 }).push(Titik { x: 2 })
    wok(q.len()); wok(q.pop()[1]); wok(q.peek())
    let s = queue::from(["a", "b"]).push("c")
    wok(s); wok(s.to_array()); s.each(|x| wok(x + "!"))
    var d: deque::Deque<float> = deque::new()
    d = d.push_back(1.5).push_front(0.5)
    wok(d); wok(d.pop_back()[1]); wok(d.peek_front())
    var h = heap::new()
    h = h.push(3).push(1).push(2)
    wok(h); wok(h.pop()[1]); wok(h.to_sorted_array())
    var bebas = queue::new()
    bebas = bebas.push(1).push("dua").push(3.5)
    wok(bebas); wok(bebas.pop()[1])
}
"#),
    ("method tanpa anotasi atas struct generik: closure tanpa hasil, struct baru dari penduga", r#"package main
struct S<T> { pub d: []T }
impl<T> S<T> {
    pub fn ubah(self, f) {
        var k = []
        for _, v in self.d { k = arr::push(k, f(v)) }
        return S { d: k }
    }
    pub fn tiap(self, f) { for _, v in self.d { f(v) } }
}
struct S2<T> { pub d: dict<T> }
impl<T> S2<T> {
    pub fn ubah(self, f) {
        var d = dict::new()
        for _, v in dict::values(self.d) {
            let y = f(v)
            d = dict::set(d, y, y)
        }
        return S2 { d: d }
    }
}
pub fn main() { let a = S { d: [1, 2] }
  a.tiap(|x| wok(x))
  wok(a.ubah(|x| x * 2).d)
  let b = S2 { d: {"1": 1, "2": 2} }
  wok(b.ubah(|x| x * 2).d) }
"#),
    // Fase GENERIK: struct generik dimonomorfisasi per susunan argumen tipe.
    ("struct generik: literal, method, fungsi generik, tipe dari tempat hasil", r#"package main
struct Kotak<T> { pub isi: []T, pub nama: string }
impl<T> Kotak<T> {
    pub fn tambah(self, x: T) -> Kotak<T> { return Kotak { isi: arr::push(self.isi, x), nama: self.nama } }
    pub fn banyak(self) -> int { return len(self.isi) }
    pub fn pertama(self) -> Option<T> { return arr::first(self.isi) }
}
fn baru<T>(nama: string) -> Kotak<T> { return Kotak { isi: [], nama: nama } }
fn id<T>(x: T) -> T { return x }
pub fn main() {
    var k: Kotak<int> = baru("angka")
    k = k.tambah(3)
    k = k.tambah(4)
    wok(k.banyak()); wok(k.pertama()); wok(k); wok(typeof(k))
    var t: Kotak<string> = baru("teks")
    t = t.tambah("a")
    wok(t); wok(id(5)); wok(id("x"))
    let l = Kotak { isi: [1.5], nama: "pecahan" }
    wok(l.tambah(2.5))
}
"#),
    ("struct generik: dua parameter, impl<U>, operator, hook, field fn, bersarang", r#"package main
struct Pasang<A, B> { pub kiri: A, pub kanan: B }
struct Tumpuk<T> { isi: []T, pub nama: string = "tumpukan" }
struct Titik { pub x: int, pub y: int }
struct Pilih<T> { pub f: fn(T) -> int, pub bawaan: Option<T> }
impl<A, B> Pasang<A, B> {
    pub fn tukar(self) -> Pasang<B, A> { return Pasang { kiri: self.kanan, kanan: self.kiri } }
    pub fn to_string(self) -> string { return "<" + str(self.kiri) + " | " + str(self.kanan) + ">" }
}
impl<U> Tumpuk<U> {
    pub fn dorong(self, x: U) -> Tumpuk<U> { return Tumpuk { isi: arr::push(self.isi, x), nama: self.nama } }
    pub fn ambil(self) -> (Tumpuk<U>, Option<U>) {
        let h = arr::pop(self.isi)
        return (Tumpuk { isi: h[0], nama: self.nama }, h[1])
    }
    pub fn tinggi(self) -> int { return len(self.isi) }
    pub fn eq(self, lain: Tumpuk<U>) -> bool { return self.isi == lain.isi }
}
impl<T> Pilih<T> {
    pub fn nilai(self, x: T) -> int { let f = self.f
      return f(x) }
}
fn kosong<T>() -> Tumpuk<T> { return Tumpuk { isi: [] } }
fn pasang<A, B>(a: A, b: B) -> Pasang<A, B> { return Pasang { kiri: a, kanan: b } }
fn pertama<T>(xs: []T) -> Option<T> { return arr::first(xs) }
fn jumlah(t: Tumpuk<int>) -> int { var s = 0
  var k = t
  for i in 0..t.tinggi() { let r = k.ambil()
    k = r[0]
    s = s + (r[1] ?? 0) }
  return s }
pub fn main() {
    let p = pasang(1, "satu")
    wok(p); wok(p.tukar()); wok(p.tukar().kiri); wok(typeof(p))
    var t: Tumpuk<Titik> = kosong()
    t = t.dorong(Titik { x: 1, y: 2 }).dorong(Titik { x: 3, y: 4 })
    wok(t.tinggi()); wok(t.nama)
    let r = t.ambil()
    wok(r[1]); wok(r[0].tinggi())
    var a: Tumpuk<int> = kosong()
    for i in 0..5 { a = a.dorong(i * i) }
    wok(jumlah(a)); wok(a == a.dorong(1)); wok(a == a)
    let daftar = [pasang(1, 2.5), pasang(3, 4.5)]
    wok(daftar[1].kanan); wok(len(daftar)); wok(daftar)
    let pl = Pilih { f: |s: string| len(s), bawaan: Some("xy") }
    wok(pl.nilai("halo")); wok(pl.bawaan)
    wok(pertama([7, 8])); wok(pertama(["a"]))
    let dalam = pasang(pasang(1, 2), [pasang("a", true)])
    wok(dalam.kiri.kanan); wok(dalam.kanan[0].kiri)
}
"#),
    ("std/heap bertipe: Heap<T, K> lewat tiga mesin", r#"package main
import "std/heap"
pub fn main() { let h = heap::from([5,3,8,1,9,2]); wok(h.len()); wok(h.peek()); wok(h.to_sorted_array()); let r = h.pop(); wok(r[1]); wok(r[0].to_sorted_array()); wok(heap::from_by(["ccc","a","bb"], |s| len(s)).to_sorted_array())
  var k: heap::Heap<string, string> = heap::new()
  k = k.push("b").push("a")
  wok(k); wok(k.pop()[1]); wok(typeof(k))
  let kosong: heap::Heap<int, int> = heap::new()
  wok(kosong.pop()[1]); wok(kosong.peek()) }
"#),
    ("std/mesin/tween: closure dan fungsi bernama ke field fn(float) -> float", r#"package main
import "std/mesin/tween"
pub fn main() {
    var t = tween::baru(0, 10, 2, tween::masuk_kuad)
    t = t.maju(1); wok(t.nilai())
    let u = tween::baru(1.5, 3, 4.0, tween::pantul).maju(3)
    wok(u.nilai()); wok(tween::antara(0, 8, 0.25, tween::keluar_kubik)); wok(tween::linear(2)); wok(tween::sinus(0.5))
}
"#),
    ("math::sign: NaN tetap NaN, tanda mengikuti jenis argumennya", r#"package main
pub fn main() { wok(math::sign(math::NAN), math::sign(-2.5), math::sign(0.0), math::sign(-3), math::sign(7), math::sign(0)) }
"#),
    // Fase SERPIH-3: tiga bentuk yang dulu ditolak atau mematikan program di LLVM.
    ("bilangan bulat beda lebar dan tanda: dibandingkan dan dihitung menurut nilainya", r#"package main
pub fn main() {
    let x: i32 = 5
    let y: i64 = 5
    wok(x == y, x < y, x + y, typeof(x + y))
    let a: u8 = 200
    let b: i8 = -100
    wok(a + b, a - b, a * b, a > b, a == b)
    let c: u64 = 18446744073709551615
    let d: i64 = -1
    wok(c == d, c > d, d < c, d != c)
    let e: i16 = 300
    let f: i64 = 9000000000
    wok(e * f, f / e, f % e)
}
"#),
    ("defer di badan closure: jalan saat closure pulang, lewat return maupun ekor", r#"package main
fn pakai(f: fn(int) -> int) -> int { return f(3) }
pub fn main() {
    var jml = 0
    let g = |a: int| -> int {
        defer wok("keluar", a)
        if a > 2 { return a * 10 }
        jml += a
        a
    }
    wok(g(1), g(5), jml)
    wok(pakai(|x| { defer wok("dalam"); x + 1 }))
}
"#),
    ("goroutine yang gagal berhenti sendirian: defer-nya jalan, main lanjut", r#"package main
fn gagal() {
    defer wok("defer goroutine")
    let a = [1]
    wok(a[5])
}
pub fn main() {
    spawn { gagal() }
    time::sleep(0.05)
    wok("main lanjut")
}
"#),
    // Fase SAPU-29: temuan dua sapuan builtin (contoh benar, dan argumen angka berganti jenis).
    ("env::set tidak menjawab apa-apa; handle jawa:: tampil dengan kind-nya", r#"package main
pub fn main() {
    wok(typeof(env::set("JRG_SAPU29", 1)), env::get("JRG_SAPU29"))
    let p = jawa::alloc(5)
    let a = jawa::array(2)
    let b = jawa::buffer(2)
    wok(p, a, b)
    wok(p.id, a.kind, typeof(b))
    jawa::array_set(a, 0, 9)
    jawa::array_push(a, 4)
    wok(jawa::array_get(a, 0), jawa::array_get(a, 2), jawa::array_len(a))
    wok(math::degrees(180), math::radians(3.5), math::is_finite(1), math::log(8, 2))
}
"#),
    // Fase PINJAM-LLVM: argumen ke parameter yang cuma dibaca tidak disalin lagi, jadi yang
    // DIKEMBALIKAN dari pinjaman (self, parameter, elemen `for`) harus nilai sendiri.
    ("tugas yang gagal dan tidak pernah diambil hasilnya tidak menghentikan program", r#"package main
fn bagi(a: int, b: int) -> int { return a / b }
pub fn main() {
    let f = Future::new(bagi, 6, 0)
    let g = Task::spawn(bagi, 6, 2)
    wok("jalan terus")
    wok(task::join(g))
    time::sleep(0.02)
    wok(future::is_done(f))
}
"#),
    ("`Self` di tanda tangan dan badan method adalah tipe penerimanya", r#"package main
struct T { pub x: int }
impl T { fn baru(x: int) -> Self { return Self { x: x } }
 fn tambah(self, n: int) -> Self { return Self { x: self.x + n } }
 fn sama(self, o: Self) -> bool { return self.x == o.x }
 fn daftar(self) -> []Self { return [self, Self { x: 0 }] } }
struct A { pub x: int }
struct B: A { pub y: int }
impl A { fn buat(self) { return Self { x: self.x + 1 } } }
struct K<T> { pub v: T }
impl K<T> { fn lagi(self) -> Self { return Self { v: self.v } } }
pub fn main() { let t = T::baru(3)
 wok(t.tambah(4).x, t.sama(T::baru(3)), t.daftar())
 let a = A { x: 1 }
 wok(a.buat(), typeof(a.buat()))
 let k = K { v: 2 }
 wok(k.lagi())
 }
"#),
    ("Option::ok, Result::ok dan Result::err menjawab Option; isinya nilai sendiri", r#"package main
pub fn main() { let o: Option<int> = Some(3)
 let n: Option<int> = None
 let r: Result<int, string> = Ok(4)
 let e: Result<int, string> = Err("x")
 wok(Option::ok(o), Option::ok(n), o.ok(), n.ok())
 wok(r.ok(), r.err(), e.ok(), e.err(), typeof(e.err()))
 wok(e.err().unwrap() + "y", r.ok().unwrap() + 1)
 wok(Result::ok(r), Result::err(e))
 let ra: Result<[]int, string> = Ok([1, 2])
 var x = ra.ok().unwrap()
 x[0] = 9
 wok(x, ra) }
"#),
    ("predikat HOF memakai truthiness; enumerate/zip/list telanjang; str::sub satu argumen", r#"package main
pub fn main() { wok(str::sub("a,b"), str::slice("a,b"), str::substring("a,b"))
 wok(list("ab"), list([1, 2]), typeof(list("ab")))
 wok(arr::count([0, 1, 2], |x| x), arr::count_if([0, 1, 2], |x| x), arr::find([0, 1, 2], |x| x), arr::find_idx([0, 3], |x| x), arr::partition([0, 1, 2], |x| x))
 wok(arr::filter([0, 1, 2, 0, 3], |x| x), arr::filter(["", "a"], |s| s), [0.0, 1.5].filter(|x| x))
 wok(arr::any(["", "a"], |s| s), arr::all(["", "a"], |s| s), [1, 2].any(|x| x - 1), [[], [1]].filter(|a| a))
 wok(enumerate([10, 20]), zip([1, 2], ["a", "b"]))
 wok(arr::count([1, 2, 3], |x| x > 1), arr::partition([1, 2, 3], |x| x % 2 == 0)) }
"#),
    ("jawa::array_pop, array_to_list, buffer_fill, buffer_to_hex, sizeof", r#"package main
pub fn main() { let a = jawa::array(2)
 jawa::array_set(a, 0, 5)
 jawa::array_set(a, 1, 6)
 jawa::array_push(a, 7)
 wok(jawa::array_to_list(a), typeof(jawa::array_to_list(a)))
 let p = jawa::array_pop(a)
 wok(p, typeof(p), jawa::array_len(a))
 wok(jawa::array_pop(a), jawa::array_pop(a))
 let b = jawa::buffer(3)
 let r = jawa::buffer_fill(b, 171)
 wok(r, typeof(r), jawa::buffer_to_hex(b), jawa::buffer_get(b, 2))
 wok(jawa::sizeof(a), jawa::sizeof(b), jawa::sizeof(jawa::alloc(1)), typeof(jawa::sizeof(b)))
 }
"#),
    ("json::set dan json::merge menjawab objek baru", r#"package main
pub fn main() { let o = {"a": 1, "b": 2}
 let p = json::set(o, "c", 3)
 wok(p, o, json::set(o, "a", 9))
 wok(json::merge(o, {"b": 7, "z": 1}), json::merge({"a": 1}, {"a": 1}))
 let j = json::parse("{\"k\": [1, 2], \"s\": \"x\"}")
 wok(json::set(j, "n", 5))
 wok(json::stringify(json::merge(j, json::parse("{\"s\": 2}"))))
 }
"#),
    ("pinjaman: yang dikembalikan dari self atau parameter yang cuma dibaca adalah nilai sendiri", r#"package main
struct Q { pub xs: []int, pub ys: [][]int }
struct K { pub xs: []int }
impl Q {
    pub fn a(self) -> Option<[]int> { return Some(self.xs) }
    pub fn b(self) -> ([]int, int) { return (self.xs, 1) }
    pub fn c(self, k: bool) -> []int { return k $ self.xs : self.xs }
    pub fn d(self) -> []int { self.xs }
    pub fn e(self) -> [][]int { return [self.xs] }
    pub fn g(self) -> []int { return match 1 { _ => self.xs } }
    pub fn h(self) -> int { for i, y in self.ys { y[0] = 5 }
        return 0 }
    pub fn k(self) -> fn() -> int { return || self.xs[0] }
    pub fn m(self) -> ([]int, []int) { return self.xs, self.ys[0] }
}
fn pa(p: []int) -> Option<[]int> { return Some(p) }
fn pb(p: []int) -> K { return K { xs: p } }
fn pc(p: [][]int) -> []int { return p[0] }
fn pd(p: []int) -> []int { p }
fn pe(p: [][]int, i: int) -> []int { if i > 0 { return p[1] }
    return arr::first(p).unwrap() }
fn pf(p: [][]int) -> []int { for _, y in p { return y }
    return [] }
fn pg(p: []int) -> int { return p[0] + len(p) }
fn ph(k: K) -> []int { return k.xs }
fn baca(p: [][]int) -> int { return pg(p[1]) + pg(pc(p)) }
pub fn main() {
    var q = Q { xs: [1, 2], ys: [[1]] }
    var a = q.a().unwrap()
    a[0] = 9
    var b = q.b()
    b.0[0] = 9
    var c = q.c(true)
    c[0] = 9
    var d = q.d()
    d[0] = 9
    var e = q.e()
    e[0][0] = 9
    var g = q.g()
    g[0] = 9
    let _ = q.h()
    let k = q.k()
    q.xs[0] = 40
    var m = q.m()
    m.0[1] = 9
    m.1[0] = 9
    wok(q.xs, q.ys, k())
    var x: []int = [1, 2]
    var xx: [][]int = [[1, 2], [3]]
    var ra = pa(x).unwrap()
    ra[0] = 9
    var rb = pb(x)
    rb.xs[0] = 9
    var rc = pc(xx)
    rc[0] = 9
    var rd = pd(x)
    rd[0] = 9
    var re = pe(xx, 0)
    re[0] = 9
    var re2 = pe(xx, 1)
    re2[0] = 9
    var rf = pf(xx)
    rf[0] = 9
    var kk = K { xs: [7, 8] }
    var rh = ph(kk)
    rh[0] = 9
    wok(x, xx, kk.xs, pg(x), baca(xx))
}
"#),
    ("elemen yang diambil bawaan dan elemen for adalah salinan; tumbuh lewat tangkapan closure", r#"package main
struct K { pub xs: []int }
pub fn main() {
    var ys: [][]int = [[1], [2]]
    var a = ys.first().unwrap()
    a[0] = 10
    var c = arr::get(ys, 1).unwrap()
    c[0] = 12
    var d = arr::last(ys).unwrap()
    d[0] = 13
    var e = arr::find(ys, |v: []int| true).unwrap()
    e[0] = 14
    var g = arr::get_or(ys, 0, [0])
    g[0] = 16
    var dflt: []int = [0]
    var g2 = arr::get_or(ys, 9, dflt)
    g2[0] = 16
    var o = Some(ys[0])
    var u = o.unwrap()
    u[0] = 17
    var u2 = o.unwrap_or([0])
    u2[0] = 18
    var kosong: Option<[]int> = None
    var u3 = kosong.unwrap_or(dflt)
    u3[0] = 18
    var dd: dict<[]int> = dict::new()
    dd["a"] = [1]
    var v = dict::get(dd, "a").unwrap()
    v[0] = 19
    for i, y in ys { y[0] = 5 }
    for _, y in ys { var z = y
        z[0] = 6 }
    var jumlah = 0
    for _, y in ys { jumlah += y[0] }
    wok(ys, o, dd, dflt, jumlah)
    var a2: []int = []
    let tambah = |x: int| { a2 = a2 + [x] }
    for i in 0..5 { tambah(i) }
    a2 += [9]
    tambah(7)
    let beku = move || len(a2)
    let awal = move || a2[0]
    let tulis = move |v: int| { a2[0] = v
        return a2[0] }
    a2 = a2 + [1]
    a2[0] = 44
    wok(a2, beku(), awal(), tulis(3), a2[0])
}
"#),
    ("arr::swap: salinan dengan dua elemen ditukar", r#"package main
pub fn main() { let a = [1, 2, 3]
  wok(arr::swap(a, 0, 2)); wok(arr::swap(a, 1, 1)); wok(a); wok(arr::swap(["x", "y"], 1, 0)) }
"#),
    // Fase PETA: dict LLVM jadi tabel hash (primitif C). Yang dikunci: kepemilikan (salinan,
    // parameter, elemen array, iterasi sambil menyisip), urutan tampil, dan 60 ribu kunci acak —
    // ukuran yang dulu menghabiskan memori.
    ("dict tabel hash: alias, urutan, hapus, gabung, banding, 60 ribu kunci", r#"package main
fn isi(d, n) { for i in 0..n { d[str(i * 3 % 17)] = i }
  return d }
fn tambah(d) { d["zz"] = 1
  return len(d) }
pub fn main() {
    var d = {"m": 0}
    for i in 0..30 { d[str((i * 7) % 31)] = i }
    wok(len(d)); wok(d["21"]); wok(dict::has(d, "30")); wok(dict::has(d, "31")); wok(dict::get(d, "7")); wok(dict::get(d, "x"))
    wok(dict::keys(d))
    d["0"] = 100; d["a"] = 5
    wok(d)
    var e = d
    e["baru"] = 1
    for i in 0..40 { e[str(i + 100)] = i }
    wok(len(d)); wok(len(e)); wok(dict::has(d, "baru")); wok(dict::has(e, "139")); wok(dict::has(d, "139"))
    wok(tambah(d)); wok(len(d)); wok(dict::has(d, "zz"))
    let f = isi(d, 20)
    wok(len(f)); wok(len(d)); wok(f["16"]); wok(dict::has(d, "16"))
    var n = 0
    for k, v in d { d[k + "!"] = v
      n = n + 1 }
    wok(n); wok(len(d))
    for k in d { if len(k) > 2 { d = dict::remove(d, k) } }
    wok(dict::keys(d))
    var g = dict::merge(d, {"q": 9, "0": 1})
    g["k"] = 2
    wok(g == d); wok(len(g)); wok(dict::values(g)[0])
    var h = {"x": 1}
    var i2 = {"x": 1}
    h["b"] = 2; h["a"] = 3
    i2["a"] = 3; i2["b"] = 2
    wok(h == i2); wok(h); wok(i2)
    let xs = [h, i2]
    var y = xs[1]
    y["c"] = 0
    wok(xs); wok(y)
    var kosong = dict::remove({"a": 1}, "a")
    kosong["p"] = 1
    wok(kosong)
    var besar = {"s": 0}
    for i in 0..60000 { besar[str(i * 7919 % 100003)] = i }
    var s = 0
    for i in 0..60000 { s = s + besar[str(i * 7919 % 100003)] }
    wok(len(besar)); wok(s); wok(dict::keys(besar)[1]); wok(besar[dict::keys(besar)[4000]])
    besar["zzz"] = 1
    wok(dict::has(besar, "zzz")); wok(dict::has(besar, "99999x")); wok(besar[str(59999 * 7919 % 100003)])
}
"#),
    ("dict: alias lewat fungsi, closure, iterasi, salinan", r#"package main

fn ubah(d) { d["a"] = 99
  d["z"] = 5
  return d["a"] }
fn buat() { var d = {"a": 1}
  return d }
pub fn main() {
    var d = {"a": 1, "b": 2}
    wok(ubah(d)); wok(d)
    var e = d
    e["a"] = 7; e["c"] = 3
    wok(d); wok(e)
    let xs = [d, d]
    var y = xs[0]
    y["b"] = 50
    wok(xs); wok(y)
    var f = buat()
    f["q"] = 1
    wok(f); wok(buat())
    let g = || { d["a"] = 1000 }
    g()
    wok(d)
    for k, v in d { d[k] = v + 1 }
    wok(d)
    var h = dict::remove(d, "a")
    h["a"] = 0
    wok(d); wok(h); wok(dict::merge(d, h)); wok(dict::keys(h)); wok(dict::values(h)); wok(d == e); wok(dict::has(h, "b")); wok(dict::get(h, "zz"))
}
"#),
    ("dict: anotasi `dict`/`dict<V>`, kunci bukan teks, dict::set", r#"package main
pub fn main() { var d: dict<string, int> = {"a": 1}
 var e: dict<int> = {"b": 2}
 var f: dict = {"c": 3}
 wok(d, e, f)
 var g = {1: "x", 20: "y"}
 wok(g); wok(g[1]); wok(g["20"]); wok(dict::has(g, 1)); g[2.5] = "p"; g[true] = "q"
 let r = 'k'
 g[r] = "rune"
 wok(g); wok(dict::get(g, 2.5)); g = dict::set(g, 7, "tujuh"); wok(g); wok(dict::keys(g)) }
"#),
    ("dict telanjang di field struct, dict::set di tempat, dict::get_or", r#"package main
struct Set { pub d: dict }
fn dari(a) { var d = dict::new()
  for _, x in a { d = dict::set(d, x, x) }
  return Set { d: d } }
fn hitung(teks) { var c = dict::new()
  for _, k in str::split(teks, " ") { c = dict::set(c, k, dict::get_or(c, k, 0) + 1) }
  return c }
pub fn main() {
    let s = dari([3, 1, 2, 3, 1])
    wok(dict::len(s.d)); wok(dict::values(s.d)); wok(dict::has(s.d, 2)); wok(dict::has(s.d, 9))
    let t = dari(["b", "a"])
    wok(dict::keys(t.d)); wok(t.d)
    var d = {"a": 1}
    let e = dict::set(d, "b", 2)
    wok(d); wok(e)
    wok(hitung("a b a c a b"))
    var besar = dict::new()
    for i in 0..3000 { besar = dict::set(besar, i * 7 % 3001, i) }
    wok(len(besar)); wok(besar[7])
}
"#),
    ("argumen method berparameter tanpa anotasi dievaluasi SEKALI", r#"package main
struct P { pub n: int }
impl P { pub fn m(self, x) { return x + self.n } }
fn efek() { wok("efek")
 return 1 }
pub fn main() { let p = P { n: 1 }
 wok(p.m(efek())) }
"#),
    ("arr::sort atas elemen bertag: angka campur, teks, stabil", r#"package main
struct K { pub isi: array }
pub fn main() {
    wok(arr::sort(K { isi: [3, 1.5, 2, 2.0, 1] }.isi)); wok(arr::sort(K { isi: ["b", "a", "c"] }.isi))
    wok(arr::sort(K { isi: [true, false] }.isi)); let e: []int = []
    wok(arr::sort(K { isi: e }.isi))
}
"#),
    // Fase BUNGKUS: `array<T>` yang masuk field `array` telanjang dikotak per elemen, dan
    // nilai bertag yang menuju variabel bertipe dibuka dengan pemeriksaan tag.
    ("array bertipe masuk field `array` telanjang, lalu dibaca kembali", r#"package main
struct Kotak { pub isi: array }
fn jumlah(k) { var s = 0
  for _, x in k.isi { s = s + x }
  return s }
pub fn main() {
    let a = [1, 2, 3]
    let k = Kotak { isi: a }
    wok(k.isi); wok(len(k.isi)); wok(k.isi[1]); wok(jumlah(k))
    let t = Kotak { isi: ["a", "b"] }
    wok(t.isi); wok(typeof(t.isi[0])); wok(typeof(k.isi[0]))
    var b = [4.5]
    b = arr::push(b, 2.5)
    let f = Kotak { isi: b }
    wok(f.isi); wok(a)
    var s = 0
    s = k.isi[2]
    wok(s + 1)
    var u = 7
    u = t.isi[0]
    wok(u)
    let e: []int = []
    wok(Kotak { isi: e }.isi)
}
"#),
    // Fase SEPARUH: `Ok`/`Err`/`None` yang separuh tipenya datang dari `return` LAIN di fungsi
    // tanpa anotasi, termasuk di dalam tuple.
    ("separuh tipe: Ok dan Err, Some dan None, tuple berisi None", r#"package main
fn bagi(a, b) {
    if b == 0 { return Err("bagi nol") }
    return Ok(a / b)
}
fn cari(xs, x) {
    if len(xs) == 0 { return None }
    for i, v in xs { if v == x { return Some(i) } }
    return None
}
fn pasang(n) {
    if n < 0 { return (n, Option::None) }
    return (n, Some("ada"))
}
fn ekst(s) {
    match str::index_of(s, ".") { None => return Option::None, Some(i) => if i < 1 { return Option::None }, }
    return Option::Some(str::sub(s, 1, len(s)))
}
pub fn main() {
    wok(bagi(6, 3)); wok(bagi(1, 0))
    match bagi(9, 3) { Ok(v) => wok(v), Err(e) => wok(e) }
    wok(cari([4, 5, 6], 6)); wok(cari([1], 7)); wok(cari([], 1))
    wok(pasang(-1)); wok(pasang(2)); wok(ekst("a.b")); wok(ekst("ab"))
}
"#),
    ("arr:: bentuk namespace: pop, first, last, get, get_or, take, skip", r#"package main
fn pecah(daftar, n) {
    var keluar = []
    var bagian = []
    for i in daftar {
        bagian = arr::push(bagian, daftar[i])
        if bagian.len() == n { keluar = arr::push(keluar, bagian)
            bagian = [] }
    }
    return keluar
}
fn kiri(ps) { var k = []
    for i in ps { let p = ps[i]
        k = arr::push(k, p.0) }
    return k }
pub fn main() {
    let a = [3, 4, 5]
    wok(arr::pop(a)); wok(arr::first(a)); wok(arr::last(a)); wok(arr::get(a, 1)); wok(arr::get(a, 9)); wok(arr::get_or(a, 9, 7))
    let e: []int = []
    wok(arr::pop(e)); wok(arr::last(e))
    wok(arr::take(a, 2)); wok(arr::take(a, 9)); wok(arr::take(a, 0)); wok(arr::skip(a, 1)); wok(arr::skip(a, 9))
    wok(pecah([1, 2, 3, 4], 2)); wok(kiri([(1, "a"), (2, "b")]))
}
"#),
    ("str::join, str::sub, dan `for j, c in teks`", r#"package main
fn potong(p) { var k = []
    for _, b in str::split(p, "/") { if len(b) > 0 { k = arr::push(k, b) } }
    return k }
pub fn main() {
    let n = "abé.cd"
    wok(str::join(["a", "b"], "-")); wok(str::join([], "-")); wok(["x", "y"].join("+")); wok(str::join(potong("/usr//lib/"), "/"))
    wok(str::sub(n, 1, 3)); wok(n.sub(4, len(n))); wok(str::sub(n, 2, 2)); wok(str::substring(n, 0, 2))
    for j, c in n { wok_(j); wok_(" "); wok_(c); wok_(" "); wok(c == '.') }
}
"#),
    // Fase SIMPUL-1: closure tanpa anotasi yang diserahkan ke fungsi tanpa anotasi — tipe
    // parameternya dibaca dari cara BADAN fungsi itu memakainya, termasuk di modul `std/`.
    ("closure tanpa anotasi ke fungsi pemakai: map, filter, lipat, panggil dua kali", r#"package main
fn petakan(daftar, f) { var keluar = []
  for _, x in daftar { keluar = arr::push(keluar, f(x)) }
  return keluar }
fn saring(daftar, pred) { var keluar = []
  for _, x in daftar { if pred(x) { keluar = arr::push(keluar, x) } }
  return keluar }
fn lipat(daftar, awal, f) { var a = awal
  for _, x in daftar { a = f(a, x) }
  return a }
fn dua_kali(f, x) { return f(f(x)) }
pub fn main() {
  let k = 10
  wok(petakan([1, 2, 3], |x| x * 2 + k))
  wok(petakan(["a", "b"], |s| s + "!"))
  wok(saring([1, 2, 3, 4], |x| x % 2 == 0))
  wok(lipat([1, 2, 3], 0, |a, x| a + x))
  wok(dua_kali(|x| x + 3, 4))
}"#),
    // Fase TUMBUH-LLVM: pertumbuhan di tempat tidak boleh membocorkan alias ke nama lain,
    // ke pemanggil, ke global yang sudah disalin, atau antar-putaran loop.
    ("tumbuh di tempat: salinan, parameter, global, loop, closure, a + a", r#"package main
var G = []
fn tumbuh(d, x) { d = d + [x]
  d = arr::push(d, x * 2)
  return d }
fn isi_global(n) { for i in 0..n { G = arr::push(G, i) } }
fn bikin(n) { var k = []
  for i in 0..n { k = arr::push(k, i * i) }
  return k }
pub fn main() {
  var a = [1, 2]
  var b = a
  a = a + [3]
  a += [4, 5]
  b = arr::push(b, 99)
  wok(a, b)
  a = a + a
  wok(a, len(a))
  let c = tumbuh(a, 7)
  wok(len(a), len(c), c[len(c) - 1])
  var d = bikin(5)
  var e = d
  d = arr::push(d, -1)
  e[0] = 42
  wok(d, e)
  for r in 0..3 { var k = []
    for i in 0..r + 2 { k = arr::push(k, r * 10 + i) }
    k = arr::concat(k, [r])
    wok(k) }
  isi_global(4)
  let h = G
  isi_global(2)
  wok(G, h)
  var t = ["a"]
  for i in 0..3 { t = arr::push(t, "s" + str(i)) }
  let tambah = |x| { a = arr::push(a, x) }
  tambah(1000)
  wok(t, a[len(a) - 1], len(a))
  var kosong = []
  kosong = kosong + []
  kosong = arr::push(kosong, true)
  wok(kosong)
}"#),
    // Fase SIMPUL-3: nama yang diikat pola `match` punya tipe di penyimpul (`Some(v) => v`), dan
    // field `array` tanpa tipe elemen disimpan bertag — satu tata letak untuk isi apa pun.
    ("pola match berpengikat di penyimpul, iterator::filter_map, field array telanjang", r#"package main
import "std/iterator"
struct Kotak { pub isi: array, pub n: int = 0 }
impl Kotak {
  fn tambah(self, x) { return Kotak { isi: arr::push(self.isi, x), n: self.n + 1 } }
  fn panjang(self) { return len(self.isi) }
}
fn pilih(d, f) { var k = []
  for _, x in d { match f(x) { Some(v) => { k = arr::push(k, v) }
    None => {} } }
  return k }
fn atau_nol(d, f) { var k = []
  for _, x in d { let r = f(x)
    k = arr::push(k, r ?? 0) }
  return k }
pub fn main() {
  let a = [3, 1, 2]
  wok(pilih(a, |x| x > 1 $ Some(x * 10) : None), atau_nol(a, |x| x > 1 $ Some(x) : None))
  wok(iterator::filter_map(a, |x| x > 1 $ Option::Some(x * 10) : Option::None))
  var k = Kotak { isi: [] }
  k = k.tambah(1)
  k = k.tambah(2)
  wok(k.panjang(), k.n, k.isi, k.isi[0], k)
}"#),
    // Fase SIMPUL-2: fungsi yang mengembalikan `Some`/`None` tanpa anotasi (tata letaknya dibawa
    // pulang dari salinan penduga), `for … in range`, dan parameter-fungsi yang DITERUSKAN.
    ("closure tanpa anotasi: Option dari penduga, range, penerusan, std/iterator fold/find", r#"package main
import "std/iterator"
fn cari(daftar, pred) { for i in daftar { if pred(daftar[i]) { return Some(daftar[i]) } }
  return None }
fn isi(n, f) { var keluar = []
  for _, i in range(0, n) { keluar = arr::push(keluar, f(i)) }
  return keluar }
fn lipat(d, f, awal) { var a = awal
  for _, x in d { a = f(a, x) }
  return a }
fn lipat2(d, f, awal) { return lipat(d, f, awal) }
pub fn main() {
  let a = [3, 1, 2]
  wok(cari(a, |x| x < 3), cari(a, |x| x > 9))
  wok(isi(4, |i| i * i))
  wok(lipat2(a, |s, x| s + x, 0), iterator::fold(a, |s, x| s * x, 1), iterator::reduce(a, |s, x| s + x, 0))
  wok(iterator::find(a, |x| x > 1), iterator::any(a, |x| x == 2), iterator::all(a, |x| x > 0))
}"#),
    ("std/iterator lewat LLVM: map, filter, each dengan closure tanpa anotasi", r#"package main
import "std/iterator"
pub fn main() {
    wok(iterator::map([1, 2], |x| x * 2))
    wok(iterator::filter([1, 2], |x| x > 1))
    wok(typeof(iterator::each([1], |x| x)))
}"#),
    ("panik: pesan dirakit, keluaran sebelumnya utuh — lihat GAGAL_SAAT_JALAN", r#"package main
fn aman(x) { if x < 0 { panik("negatif: " + str::to_string(x)) }
  return x * 2 }
pub fn main() { wok(aman(4)) }"#),
    // Fase LEBAR-LLVM: tepat di batas masih jalan, dan yang sengaja membungkus ditulis.
    ("int 64 bit: tepi yang muat, dan math::wrap_*", r#"package main
pub fn main() {
    var x = 9223372036854775806
    x += 1
    var m = -9223372036854775807
    m = m - 1
    wok(x, m, -x, math::abs(-x), [9223372036854775806, 1].sum())
    var h: uint = 1469598103934665603
    for i in 0..5 { h = math::wrap_mul(h ^ (i as uint), 1099511628211) }
    wok(h, math::wrap_sub(0, 1), math::wrap_add(18446744073709551615, 2))
}"#),
    // Fase DUGA: tipe kembalian fungsi tanpa anotasi dibaca dari penurunan badannya.
    ("kembalian tanpa anotasi: match berpola pengikat, ??, fungsi pustaka", r#"package main
enum E { A(int), B }
fn g(e) { return match e { E::A(n) => n, E::B => -1 } }
fn m(o: Option<int>) { return o ?? 5 }
fn p(x: int) { return math::abs(x) }
pub fn main() { wok(g(E::A(3)), g(E::B), m(Option::Some(1)), m(Option::None), p(-5), typeof(p(-5))) }"#),
    ("kembalian tanpa anotasi: fn bersarang, method, default, tuple, rekursi", r#"package main
var G = 7
struct K { pub v: int = 0 }
impl K {
    fn ambil(self) { return self.v }
    fn pasangan(self) { return (self.v, "k") }
}
fn h(a, b = a + 1) { return b }
fn k(a = G) { return a }
fn luar() { let r = kedua(1); fn kedua(x) { return x + 100 }; return r }
fn fakt(n) { if n < 2 { return 1 }
    return n * fakt(n - 1) }
pub fn main() {
    wok(K { v: 5 }.ambil(), K { v: 2 }.pasangan(), h(3), k(), k(99), luar(), fakt(10))
}"#),
    // Fase PACU-3: literal diperiksa lebarnya, dan `-2^63` tetap sah sebagai operand `-`.
    ("literal tepi 64 bit", "package main\npub fn main() { let x: u64 = 18446744073709551615\n let y: i8 = -128\n wok(-9223372036854775808, 9223372036854775807, x, y) }"),
    // Fase LLVM-17: `self` dulu penunjuk ke milik pemanggil, `wok` mencetak di tengah evaluasi
    // argumennya, tiap nilai fungsi `function`, dan geseran >= lebar adalah poison.
    ("penerima disalin, urutan cetak, typeof closure, geseran", r#"package main
struct P { pub n: int, pub xs: []int }
impl P {
    fn naik(self) -> int { self.n += 1; return self.n }
    fn dua(self) -> int { return self.naik() + self.n }
    fn isi(self) -> P { self.xs[0] = 7; self.n = 3; return self }
    fn baca(self) -> int { return self.n + self.xs[0] }
    fn lewat_closure(self) -> int { let f = || { self.n = 50; return self.n }; return f() + self.n }
}
fn tambah(a: int, b: int) -> int { return a + b }
fn f(n: int) -> int { wok_("[", n, "]"); return n }
pub fn main() {
    var p = P { n: 1, xs: [1, 2] }
    wok(p.naik(), p.n, p.dua(), p.n)
    let q = p.isi()
    wok(p, q, p.baca(), q.baca())
    wok(p.lewat_closure(), p.n)
    wok(f(1), f(2))
    let g = |x: int| x + 1
    let k = 5
    let h = |x: int| x + k
    let t = tambah
    wok(typeof(g), typeof(h), typeof(tambah), typeof(t), g(1), h(1), t(1, 2))
    let n = 70
    let z = 0
    wok(-8 >> n, 8 >> n, z << n, 1 << 3, 256 >> 4, -256 >> 4, 5 << 0)
}"#),
    ("method default trait, hook tampilan darinya, dan teks + agregat", r#"package main
struct B { pub n: int }
impl Bentuk for B { fn luas(self) -> int { return self.n * self.n } }
trait Bentuk {
    fn luas(self) -> int
    fn sapa(self) -> string = "dari trait"
    fn to_string(self) -> string { return "Bentuk luas " + str(self.luas()) }
    fn dobel(self) -> int { return self.luas() * 2 }
}
struct A { pub n: int }
impl Bentuk for A {
    fn luas(self) -> int { return self.n }
    fn sapa(self) -> string { return "milik A" }
}
struct P { pub a: int }
enum E { X(int), Y }
struct V { pub variant: string }
pub fn main() {
    let a = A { n: 3 }
    let b = B { n: 4 }
    wok(a.sapa(), b.sapa(), a.dobel(), b.dobel())
    wok(a, b, [a], "x" + b, a.to_string())
    let p = P { a: 1 }
    wok("x" + p, "x" + [1, 2], "x" + (1, "a"), "x" + E::X(2), "x" + E::Y, [1] + "y", p + "z")
    let v = V { variant: "None" }
    wok(match v { None => "none", _ => "lain" }, match p { Y => "y", _ => "lain" })
}"#),
    // Fase LLVM-16: tanpa anotasi, kembalian ternary/`if … else`/`match` dulu ditebak `()`
    // dan nilainya dibuang tanpa kata — `wok(f(0), f(1))` mencetak `() ()`.
    ("kembalian ternary, if, dan match tanpa anotasi", r#"package main
fn f(n) { return n == 0 $ true : false }
fn g(e) { return match e { 1 => 10, _ => 20 } }
fn h(n) { if n > 0 { "pos" } else { "neg" } }
fn k(n) { return if n > 0 { 1.5 } else { 2.5 } }
pub fn main() {
    wok(f(0), f(1), g(1), g(2), h(1), h(-1), k(1))
}"#),
    // `math::pow` selalu pecahan, dan min/max memakai rumus acuan `a < b $ a : b`.
    ("math: pow selalu pecahan, min dan max seperti acuan", r#"package main
pub fn main() {
    wok(math::pow(2, 10), pow(2, 3), math::pow(2, -1), math::pow(2.0, 0.5))
    wok(math::min(math::NAN, 1.0), math::min(1.0, math::NAN), math::max(math::NAN, 1.0))
    wok(math::min(-0.0, 0.0), math::min(0.0, -0.0), math::max(3, 2), math::min(2.5, 1.5))
}"#),
    // Fase SERPIH: `a, _ = …` dulu ditolak LLVM ("`_` bukan variabel yang bisa dialamati").
    ("pembuang: `_ = f()` dan `a, _ = …` membuang nilainya", r#"package main
fn f() -> int {
    wok("f")
    return 5
}
pub fn main() {
    _ = f()
    var a = 1
    a, _ = 7, f()
    wok(a)
}"#),
    // Fase RUANG: fungsi milik program menang atas bawaan bernama sama, di tiga mesin —
    // dulu LLVM menjawab `abs(-5)` dengan bawaannya dan menolak `len(1, 2, 3)`.
    (
        "fungsi milik program menang atas bawaan bernama sama",
        "package main\nfn len(a, b, c) { return a + b + c }\nfn abs(x) { return 7 }\npub fn main() {\n    wok(len(1, 2, 3), abs(-5), ::len(\"ab\"), ::abs(-5))\n}\n",
    ),
    // Fase TERM: `exit` satu aturan di tiga mesin.
    (
        "exit: kode 0 sesudah keluaran, sisanya tidak jalan",
        "package main\npub fn main() {\n    wok(\"sebelum\")\n    exit(0)\n    wok(\"tidak sampai\")\n}\n",
    ),
    // `()` yang ditulis adalah unit, di tiga mesin (dulu tuple kosong).
    (
        "unit: () yang ditulis sama dengan unit yang dijawab fungsi",
        "package main\nfn f() { }\npub fn main() {\n    wok(typeof(()), f() == (), () == ())\n}\n",
    ),
    // Fase KEMBALIAN: yang tidak menjawab apa-apa menjawab `unit` — LLVM sudah
    // begitu sejak dulu, dua mesin acuan menjawab `nil`.
    (
        "jawa::set dan jawir::free menjawab unit",
        r#"package main
pub fn main() {
    let p = jawa::alloc(1)
    wok(typeof(jawa::set(p, 2)), jawa::get(p))
    let a = jawa::array(2)
    wok(typeof(jawa::array_set(a, 0, 7)), typeof(jawir::free(p)))
    let u = jawa::array_push(a, 1)
    wok(1, u, typeof(u))
}
"#,
    ),
    // Fase KURUNG: penjaganya menyentuh pintu yang dilewati SETIAP pemanggilan
    // method, jadi sisi positifnya dikunci berdampingan.
    (
        "pemanggilan method dan pseudo-field tetap jalan",
        r#"package main
struct P { pub a: int }
impl P { fn dua(self) { return self.a * 2 } }
pub fn main() {
    let p = P { a: 5 }
    wok(p.dua())
    let s = "halo"
    wok(s.len, s.upper())
}
"#,
    ),
    // Fase POLA-FIELD: sisi POSITIF penjaga pola field. Penjaga yang menolak
    // semuanya juga menghijaukan tabel penolakan, jadi batasnya dikunci di sini.
    (
        "pola sebagian field tetap cocok",
        r#"package main
struct P { pub a: int, pub b: int }
pub fn main() {
    let p = P { a: 7, b: 2 }
    match p { P { a } => wok(a), _ => wok("lain") }
    let q = P { a: 8, b: 3 }
    match q { P { a, .. } => wok(a), _ => wok("lain") }
}
"#,
    ),
    // `byte` bilangan 8 bit tak bertanda di ketiga mesin (Fase KONVERSI) —
    // kasus ini dulu satu-satunya isi `DIVERGENSI_LLVM`.
    (
        "byte: bilangan 8 bit, bukan karakter",
        r#"package main
pub fn main() {
    wok(65 as byte)
    wok((65 as byte) == 65)
}"#,
    ),
    // Tipe hasil dari ANOTASI method struct, di fungsi tanpa anotasi dan di
    // closure. Dulu `f` dipancarkan `void`: nilainya dibuang dan `typeof`
    // menjawab `unit` — hasil salah yang diam.
    (
        "method struct: tipe hasil dari anotasinya",
        r#"package main
struct P { pub n: int }
impl P {
    fn dua(self) -> int { return self.n * 2 }
    fn besar(self) -> bool { return self.n > 1 }
}
fn f(t: P) { return t.dua() }
fn g(t: P) { return P::dua(t) }
pub fn main() {
    let xs = [P { n: 1 }, P { n: 2 }]
    wok(f(P { n: 3 }), typeof(f(P { n: 3 })), g(P { n: 4 }))
    wok(xs.map(|t| t.dua()), xs.map(|t: P| t.dua()), xs.filter(|t| t.besar()))
    wok(arr::map(xs, |t| t.dua()), xs.map(|t| P::dua(t)))
}"#,
    ),
    // Nilai FUNGSI di parameter: tipe hasilnya dari tipenya. Dulu `jalankan`
    // ditebak `()` dan `assert_eq(jalankan(..), 3)` di `closures.jrg` LULUS
    // karena `() == 3` dikompilasi jadi `true` — dua cacat saling menutupi.
    (
        "nilai fungsi di parameter: tipe hasil dari tipenya",
        r#"package main
fn jalankan(f) { return f() }
pub fn main() {
    var arg = 1
    wok(jalankan(|| 3), typeof(jalankan(|| 3)))
    assert_eq(jalankan(|| { arg = arg * 3; return arg }), 3)
    wok(arg)
}"#,
    ),
    // `()` lawan nilai lain: `false`, seperti acuan. Dulu literal `6` menyesuaikan
    // diri ke `()` dan `kosong() == 6` jadi konstanta `true`.
    (
        "unit lawan nilai lain: == lintas tipe",
        r#"package main
fn kosong() { }
pub fn main() {
    let u = kosong()
    wok(u == 6, u != 6, kosong() == 6, kosong() == "teks", kosong() != 0)
}"#,
    ),
    // Bentuk path atas method ber-`self`. Di sini penerimanya diketahui saat
    // KOMPILASI, jadi pemeriksaannya gratis dan tepat.
    (
        "path method di llvm",
        r#"package main
struct P { pub n: int }
impl P { pub fn tambah(self, k: int) -> int { return self.n + k } }
pub fn main() {
    let p = P{n: 1}
    wok(P::tambah(p, 4))
    wok(p.tambah(4))
}
"#,
    ),
    (
        "sebar array di llvm",
        r#"package main
pub fn main() {
    let a = [2, 3]
    let b = [1, ...a, 4]
    wok(b)
    wok(len(b))
}
"#,
    ),
    // `"""` — pembatas ASCII untuk template, di backend ketiga.
    (
        "tiga kutip di llvm",
        r#"package main
pub fn main() {
    let n = 7
    wok("""nilai ${n}
baris dua""")
    wok(«lama ${n * 2}»)
    wok("biasa", "")
}"#,
    ),
    // ── `str::format`: aturan template yang sama dengan interpreter ──────
    ("format: dua desimal",       "package main\npub fn main() { wok(str::format(\"{:.2}\", 3.14159)) }"),
    ("format: nol desimal",       "package main\npub fn main() { wok(str::format(\"{:.0}\", 2.7)) }"),
    ("format: int dapat desimal", "package main\npub fn main() { wok(str::format(\"{:.2}\", 5)) }"),
    ("format: campur",           "package main\npub fn main() { wok(str::format(\"{} = {:.3}\", \"pi\", 3.14159265)) }"),
    ("format: {x} tetap teks",   "package main\npub fn main() { wok(str::format(\"{x} {}\", 1)) }"),
    // Closure `move`. Di backend ini tangkapan berupa kotak heap dan `env`
    // menyimpan PENUNJUK ke tiap kotak; `move` menyimpan penunjuk ke kotak
    // BARU berisi salinan. Kedua arah diuji terpisah.
    ("move llvm: berbagi (biasa)",  "package main\npub fn main() { var n = 1\n let f = || n\n n = 9\n wok(f()) }"),
    ("move llvm: snapshot",         "package main\npub fn main() { var n = 1\n let f = move || n\n n = 9\n wok(f()) }"),
    ("move llvm: dalam->luar putus","package main\npub fn main() { var n = 1\n let f = move || { n = 5\n return n }\n wok(f())\n wok(n) }"),
    ("move llvm: mutasi bertahan",  "package main\npub fn main() { var n = 0\n let f = move || { n = n + 1\n return n }\n wok(f())\n wok(f())\n wok(n) }"),
    ("move llvm: dua nama",         "package main\npub fn main() { var a = 1\n var b = 2\n let f = move || a + b\n a = 10\n b = 20\n wok(f()) }"),
    ("move llvm: tanpa tangkapan",  "package main\npub fn main() { let f = move || 7\n wok(f()) }"),

    // Operator yang didefinisikan pengguna.
    ("operator llvm: a + b",   "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:1} + V{x:2}) }"),
    ("operator llvm: a - b",   "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:5} - V{x:2}) }"),
    ("operator llvm: -a",      "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(-V{x:7}) }"),
    ("operator llvm: a == b",  "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:1} == V{x:1}); wok(V{x:1} == V{x:2}) }"),
    ("operator llvm: a != b",  "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:1} != V{x:2}) }"),
    ("operator llvm: a < b",   "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:1} < V{x:2}); wok(V{x:3} < V{x:2}) }"),
    ("operator llvm: a >= b",  "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:3} >= V{x:3}) }"),
    ("operator llvm: rantai",  "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(V{x:1} + V{x:2} + V{x:3}) }"),
    ("operator llvm: bawaan tak tersentuh", "package main\nstruct V { pub x: int }\nimpl V {\n  fn add(self, o) -> V { return V{x: self.x + o.x} }\n  fn sub(self, o) -> V { return V{x: self.x - o.x} }\n  fn mul(self, o) -> V { return V{x: self.x * o.x} }\n  fn neg(self) -> V { return V{x: 0 - self.x} }\n  fn eq(self, o) -> bool { return self.x == o.x }\n  fn cmp(self, o) -> int { return self.x - o.x }\n  fn to_string(self) -> string { return \"V\" + fmt::sprint(self.x) }\n}\npub fn main() { wok(1 + 2); wok(-5); wok(1 < 2) }"),
    ("mut llvm: var",          "package main\npub fn main() { var x = 1\n x = 2\n wok(x) }"),
    ("mut llvm: let mut",      "package main\npub fn main() { let mut x = 1\n x = 2\n wok(x) }"),

    // Hook tampilan `to_string`.
    ("hook llvm: wok",            "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok(P{n:1}) }"),
    ("hook llvm: wok_",           "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok_(P{n:1}); wok(\"\") }"),
    ("hook llvm: fmt::sprint",    "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok(fmt::sprint(P{n:1})) }"),
    ("hook llvm: dalam array",    "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok([P{n:1}, P{n:2}]) }"),
    ("hook llvm: dalam tuple",    "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok((P{n:1}, 7)) }"),
    ("hook llvm: di field",       "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\nstruct Q { pub p: P }\npub fn main() { wok(Q{p: P{n:1}}) }"),
    ("hook llvm: eksplisit",      "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok(P{n:1}.to_string()) }"),
    ("hook llvm: tanpa hook",     "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\nstruct R { pub m: int }\npub fn main() { wok(R{m:2}) }"),
    ("hook llvm: typeof tetap",   "package main\nstruct P { pub n: int }\nimpl P { fn to_string(self) -> string { return \"P#\" + fmt::sprint(self.n) } }\npub fn main() { wok(typeof(P{n:1})) }"),

    // `let … else` dan parameter pengumpul.
    ("let else dan parameter pengumpul", r#"package main
fn satu(o: Option<int>) -> int {
    let Some(x) = o else { return -1 }
    return x * 10
}
fn banyak(o: Option<(int, int)>) -> int {
    let Some((a, b)) = o else { return -1 }
    return a + b
}
fn jumlah(xs...) -> int {
    var t = 0
    for i in xs { t = t + xs[i] }
    return t
}
pub fn main() {
    wok(satu(Some(3)), satu(None))
    wok(banyak(Some((3, 4))), banyak(None))
    wok(jumlah(1, 2, 3), jumlah())
    wok(match 5 { x => x }, match 9 { n @ _ => n })
}"#),

    // ── pola pengikat, koalesensi, dan varian unit telanjang ────────────
    ("pola pengikat dan koalesensi", r#"package main
enum E { A(int), B }
var n = 0
fn mahal() -> int { n = n + 1; return 9 }
pub fn main() {
    wok(match 5 { m @ 1..=9 => m * 10, _ => 0 })
    wok(match 50 { m @ 1..=9 => m * 10, _ => 0 })
    let o: Option<int> = Some(3)
    wok(match o { None => 0, Some(v) => v })
    let e = E::A(3)
    wok(match e { B => 0, E::A(v) => v })

    let k: Option<int> = None
    let b: Option<int> = None
    let c: Option<int> = Some(7)
    wok(o ?? 9, k ?? 9, k ?? b ?? c ?? 0)

    wok(o ?? mahal(), n)
    wok(o.unwrap_or(mahal()), n)
}"#),

    // ── sintaks yang diturunkan di parser ───────────────────────────────
    ("if let dan while let", r#"package main
pub fn main() {
    let o = Some(3)
    if let Some(x) = o { wok("ada", x) }
    let k: Option<int> = None
    if let Some(x) = k { wok(x) } else { wok("kosong") }
    wok(if let Some(v) = o { v } else { 0 })
    if let [a, b] = [1, 2] { wok(a, b) }

    var n = 0
    while let Some(x) = Some(n) {
        n = n + 1
        if x > 2 { break }
        wok("n", x)
    }
}"#),
    ("penugasan jamak dan for klasik `:=`", r#"package main
pub fn main() {
    var a = 1
    var b = 2
    a, b = b, a
    wok(a, b)
    c, d := 5, 6
    wok(c, d)
    for i := 0; i < 3; i = i + 1 { wok(i) }
}"#),
    ("else boleh memimpin barisnya sendiri", r#"package main
pub fn main() {
    if 1 > 2 { wok("a") }
    else if 1 > 0 { wok("b") }
    else { wok("c") }
}"#),

    // ── closure ─────────────────────────────────────────────────────────
    ("closure: tangkapan bermutasi terlihat dua arah", r#"package main
pub fn main() {
    var x = 1
    let ubah = || { x = 2 }
    wok(x)
    ubah()
    wok(x)
}"#),
    ("closure: dua closure berbagi satu pengikatan", r#"package main
pub fn main() {
    var n = 0
    let naik = || { n = n + 1 }
    let lihat = || n
    naik(); naik(); naik()
    wok(n, lihat())
}"#),
    ("closure: lolos keluar dari bingkai asalnya", r#"package main
fn bikin() -> fn() -> int {
    var n = 0
    return || { n = n + 1; return n }
}
pub fn main() {
    let c = bikin()
    wok(c(), c(), c())
    let d = bikin()
    wok(d())
}"#),
    ("closure: tiap putaran loop mengikat ulang", r#"package main
pub fn main() {
    var fs = []
    for i in 0..3 { fs = fs + [|| i] }
    wok(fs[0](), fs[1](), fs[2]())
}"#),
    ("closure: tipe parameter dibaca dari situs pemanggilan", r#"package main
pub fn main() {
    var p = 1
    let byparam = |p| p
    wok(byparam(5), p)
    let dua = |a, b| a + b
    wok(dua(3, 4))
}"#),
    ("closure: menangkap var tingkat modul", r#"package main
var g = 0
pub fn main() {
    let gn = || { g = g + 1; return g }
    wok(gn(), gn(), g)
}"#),
    // Fungsi BERSARANG bukan closure: ia tidak melihat lokal fungsi luarnya
    // (diukur — acuannya menolak dengan "variabel belum dikenal") dan di-hoist …
    ("fungsi bersarang: hoist per blok dan saling panggil", r#"package main
fn luar() -> int {
    wok(a(2))
    fn a(x: int) -> int { return b(x) + 1 }
    fn b(x: int) -> int { return x * 10 }
    return a(3)
}
pub fn main() {
    wok(luar())
    fn dalam(x: int) -> int { return x * 3 }
    wok(dalam(2))
    if true {
        fn blok(x: int) -> int { return x + 100 }
        wok(blok(1))
    }
}"#),
    ("fungsi bersarang: dimonomorfisasi per tipe argumen", r#"package main
pub fn main() {
    fn ganda(x) { return x + x }
    wok(ganda(3), ganda("ab"))
}"#),

    // ── jawa:: / jawir:: ────────────────────────────────────────────────
    ("jawa: sel tunggal, array, buffer, dan handle salinan", r#"package main
pub fn main() {
    let p = jawa::alloc(42)
    wok(jawa::get(p), jawa::is_valid(p), typeof(p))
    jawa::set(p, 99)
    wok(jawa::get(p))
    jawir::free(p)
    wok(jawa::is_valid(p))

    let a = jawa::array(3)
    jawa::array_set(a, 0, 10)
    let a2 = jawa::array_push(a, 30)
    wok(jawa::array_len(a), jawa::array_get(a, 0), jawa::array_get(a, 3), a2)

    let b = jawa::buffer(4)
    jawa::buffer_set(b, 0, 255)
    wok(jawa::buffer_len(b), jawa::buffer_get(b, 0))

    let asli = jawa::alloc(1)
    let salinan = asli
    jawa::set(salinan, 123)
    wok(jawa::get(asli))
    jawir::free(salinan)
    wok(jawa::is_valid(asli), jawa::is_valid(salinan))
}"#),

    ("fungsi bernama sebagai nilai", r#"package main
fn tambah(a: int, b: int) -> int { return a + b }
fn kali(a: int, b: int) -> int { return a * b }
fn pakai(f: fn(int, int) -> int, x: int, y: int) -> int { return f(x, y) }
pub fn main() {
    let f = tambah
    var g = tambah
    g = kali
    wok(f(2, 3), g(2, 3))
    wok(pakai(tambah, 4, 5), pakai(kali, 4, 5))
}"#),

    // ── defer ───────────────────────────────────────────────────────────
    ("defer: urutan LIFO dan pembekuan argumen", r#"package main
fn urutan() {
    defer wok("A")
    defer wok("B")
    defer wok("C")
    wok("badan")
}
fn beku() {
    var x = 1
    defer wok("beku:", x)
    x = 99
    wok("akhir:", x)
}
pub fn main() { urutan(); beku() }"#),
    ("defer: lingkup fungsi, bukan blok", r#"package main
fn dalam_blok() {
    if true {
        defer wok("dalam if")
        wok("di if")
    }
    wok("sesudah if")
}
fn dalam_loop() {
    for i in 0..3 { defer wok("loop", i) }
    wok("sesudah loop")
}
pub fn main() { dalam_blok(); dalam_loop() }"#),
    ("defer: fungsi yang dipanggil dibekukan saat didaftarkan",
     "package main\nfn a() { wok(\"a\") }\nfn b() { wok(\"b\") }\npub fn main() { var f = a\n defer f()\n f = b\n defer f()\n wok(\"badan\") }"),
    ("defer: tetap jalan lewat return lebih awal", r#"package main
fn dg_return() -> int {
    defer wok("defer jalan")
    return 7
}
fn kondisional(c: bool) {
    if c { defer wok("hanya kalau true") }
    wok("selesai", c)
}
pub fn main() {
    wok(dg_return())
    kondisional(true)
    kondisional(false)
}"#),

    // ── method tingkat-tinggi ───────────────────────────────────────────
    ("hof: map filter reduce find any all", r#"package main
pub fn main() {
    let a = [1, 2, 3]
    wok(a.map(|x| x * 2))
    wok(a.filter(|x| x > 1))
    wok(a.reduce(|acc, x| acc + x, 0))
    wok(a.any(|x| x > 2), a.all(|x| x > 0))
    wok(a.find(|x| x > 1))
    wok(a.find(|x| x > 9))
    wok(a.map(|x| x * 2).filter(|x| x > 2))
    let s = ["a", "bb"]
    wok(s.map(|x| x.len()))
}"#),

    // ── parameter default ───────────────────────────────────────────────
    ("parameter default dievaluasi di bingkai callee", r#"package main
fn a(x: int, y: int = 10) -> int { return x + y }
fn b(x: int, y: int = x * 2) -> int { return x + y }
var hit = 0
fn efek() -> int { hit = hit + 1; return 100 }
fn c(x: int, y: int = efek()) -> int { return x + y }
pub fn main() {
    wok(a(5), a(5, 1))
    wok(b(3), b(3, 1))
    wok(c(1), c(1, 2), c(1))
    wok(hit)
}"#),

    // ── var tingkat modul ───────────────────────────────────────────────
    ("var tingkat modul: urutan inisialisasi dan mutasi", r#"package main
var n = 0
var pesan = "halo"
var xs = [1, 2, 3]
var turunan = n + 5
fn naik() { n = n + 1 }
pub fn main() {
    wok(n, pesan, xs, turunan)
    naik(); naik()
    wok(n)
    pesan = pesan + " dunia"
    xs = xs + [4]
    wok(pesan, xs)
}"#),

    // ── range ───────────────────────────────────────────────────────────
    ("range: satu, dua, dan tiga argumen", r#"package main
pub fn main() {
    wok(range(3))
    wok(range(1, 4))
    wok(range(0, 10, 3))
    wok(range(0))
    wok(typeof(range(3)))
}"#),

    // ── aliasing: penunjuk DI DALAM agregat ─────────────────────────────
    ("aliasing: slice di dalam struct ikut disalin", r#"package main
struct Kotak { pub isi: []int }
pub fn main() {
    var k1 = Kotak { isi: [9, 9] }
    var k2 = k1
    k2.isi[0] = 88
    wok(k1.isi[0], k2.isi[0])
}"#),
    ("aliasing: array di dalam array ikut disalin", r#"package main
pub fn main() {
    var a1 = [[1], [2]]
    var a2 = a1
    a2[0] = a2[0] + [3]
    wok(a1[0], a2[0])
}"#),

    // ── method string ───────────────────────────────────────────────────
    ("method string: split, contains, ubah huruf", r#"package main
pub fn main() {
    wok("a,b,c".split(","))
    wok("abc".split(""))
    wok("a,,b".split(","))
    wok("a,b,c".split(",").len())
    wok("halo".to_uppercase(), "HALO".to_lowercase())
    wok("halo".contains("al"), "halo".contains("z"))
    wok("halo".starts_with("ha"), "halo".ends_with("lo"))
    wok("a-b".replace("-", "+"))
    wok("  x  ".trim())
}"#),

    // ── nama internal tidak boleh bocor ─────────────────────────────────
    ("nama enum termangel tidak bocor ke keluaran", r#"package main
enum Warna { Merah, Biru }
pub fn main() {
    let o: Option<int> = Some(3)
    wok(o, typeof(o))
    let w = Warna::Merah
    wok(w, typeof(w))
    let r: Result<int, string> = Ok(1)
    wok(r, typeof(r))
}"#),

    // ── dict ────────────────────────────────────────────────────────────
    ("dict: literal, baca, dan cetak", r#"package main
pub fn main() {
    let d = {"b": 2, "a": 1, "c": 3}
    wok(d)
    wok(d["a"], d["b"], d["c"])
    wok(typeof(d), len(d))
}"#),
    ("dict: kunci kembar, yang terakhir menang", r#"package main
pub fn main() {
    wok({"a": 1, "b": 2, "a": 9})
}"#),
    ("dict: namespace dict::", r#"package main
pub fn main() {
    let d = {"a": 1, "b": 2}
    wok(dict::len(d), dict::is_empty(d))
    wok(dict::has(d, "a"), dict::has(d, "z"))
    wok(dict::keys(d), dict::values(d))
    wok(dict::get(d, "a").unwrap())
    wok(dict::remove(d, "a"))
    wok(dict::merge(d, {"z": 9}))
}"#),
    ("dict: tulis lewat indeks, tumbuh dan menimpa", r#"package main
pub fn main() {
    var d = {"a": 1}
    d["b"] = 2
    d["a"] = 9
    wok(d, len(d))
}"#),
    ("dict: iterasi menyebut KUNCI, bukan indeks", r#"package main
pub fn main() {
    let d = {"x": 10, "y": 20}
    for k in d { wok(k, d[k]) }
    for k, v in d { wok(k, v) }
}"#),
    ("dict: kesamaan tidak bergantung urutan penulisan", r#"package main
pub fn main() {
    wok({"a": 1, "b": 2} == {"b": 2, "a": 1})
    wok({"a": 1} == {"a": 2}, {"a": 1} == {"a": 1, "b": 2})
    wok({"a": 1} != {"a": 2})
}"#),
    ("dict: nilai array, panjang boleh beda", r#"package main
pub fn main() {
    let d = {"a": [1, 2], "b": [3]}
    wok(d)
    wok(d["a"], d["b"])
}"#),
    ("dict: penyerahan menyalin, bukan berbagi", r#"package main
pub fn main() {
    var a = {"k": 1}
    var b = a
    b["k"] = 99
    b["baru"] = 5
    wok(a, b)
}"#),
    ("aritmatika int", r#"package main
pub fn main() {
    var a: int = 17
    var b: int = 5
    wok(a + b, a - b, a * b, a / b, a % b)
}"#),

    ("presedensi dan tanda", r#"package main
pub fn main() {
    wok(2 + 3 * 4, (2 + 3) * 4, -7 / 2, -7 % 2)
}"#),

    ("float dibagi dan dicetak", r#"package main
pub fn main() {
    var x: f64 = 7.0
    var y: f64 = 2.0
    wok(x / y, x * y, x - y)
}"#),

    ("bool dan operator logika", r#"package main
pub fn main() {
    var t: bool = true
    var f: bool = false
    wok(t && f, t || f, !t)
}"#),

    ("perbandingan", r#"package main
pub fn main() {
    var a: int = 3
    var b: int = 9
    wok(a < b, a > b, a <= b, a >= b, a == b, a != b)
}"#),

    ("if else berantai", r#"package main
fn kelas(n: int) -> int {
    if n < 10 { return 1 }
    if n < 100 { return 2 }
    return 3
}
pub fn main() {
    wok(kelas(5), kelas(50), kelas(500))
}"#),

    ("while dan akumulator", r#"package main
pub fn main() {
    var i: int = 0
    var s: int = 0
    while i < 10 { s = s + i; i = i + 1 }
    wok(s)
}"#),

    ("for range", r#"package main
pub fn main() {
    var s: int = 0
    for i in 0..5 { s = s + i }
    wok(s)
}"#),

    ("rekursi fib", r#"package main
fn fib(n: int) -> int {
    if n < 2 { return n }
    return fib(n - 1) + fib(n - 2)
}
pub fn main() { wok(fib(15)) }"#),

    ("break dan continue", r#"package main
pub fn main() {
    var s: int = 0
    var i: int = 0
    while i < 20 {
        i = i + 1
        if i % 2 == 0 { continue }
        if i > 11 { break }
        s = s + i
    }
    wok(s, i)
}"#),

    ("array indeks dan len", r#"package main
pub fn main() {
    var a: [4]int = [10, 20, 30, 40]
    wok(a[0], a[3], len(a))
}"#),

    ("array ditulis di tempat", r#"package main
pub fn main() {
    var a: [3]int = [1, 2, 3]
    a[1] = 99
    wok(a[0], a[1], a[2])
}"#),

    ("string dan cetak", r#"package main
pub fn main() {
    var s: string = "halo"
    wok(s, len(s))
}"#),

    ("struct dan field", r#"package main
struct Titik { pub x: int, pub y: int }
pub fn main() {
    var p: Titik = Titik { x: 3, y: 4 }
    wok(p.x, p.y)
}"#),

    ("method dengan self", r#"package main
struct Kotak { pub w: int, pub h: int }
impl Kotak {
    fn luas(self) -> int { return self.w * self.h }
}
pub fn main() {
    var k: Kotak = Kotak { w: 3, h: 5 }
    wok(k.luas())
}"#),

    ("enum dan match", r#"package main
enum Warna { Merah, Hijau, Biru }
fn kode(w: Warna) -> int {
    match w {
        Warna::Merah => { return 1 }
        Warna::Hijau => { return 2 }
        Warna::Biru  => { return 3 }
    }
}
pub fn main() {
    wok(kode(Warna::Merah), kode(Warna::Hijau), kode(Warna::Biru))
}"#),

    ("match pada int dengan default", r#"package main
fn nama(n: int) -> int {
    match n {
        0 => { return 100 }
        1 => { return 200 }
        _ => { return 900 }
    }
}
pub fn main() { wok(nama(0), nama(1), nama(7)) }"#),

    ("fungsi banyak argumen", r#"package main
fn campur(a: int, b: int, c: int) -> int { return a * 100 + b * 10 + c }
pub fn main() { wok(campur(1, 2, 3)) }"#),

    ("cast int ke float dan balik", r#"package main
pub fn main() {
    var i: int = 7
    var f: f64 = i as f64 / 2.0
    wok(f, f as int)
}"#),

    ("operator bit", r#"package main
pub fn main() {
    var a: int = 12
    var b: int = 10
    wok(a & b, a | b, a ^ b, a << 2, a >> 2)
}"#),

    ("ternary dolar", r#"package main
pub fn main() {
    var n: int = 7
    wok(n > 5 $ 111 : 222)
}"#),

    ("loop bersarang", r#"package main
pub fn main() {
    var s: int = 0
    var i: int = 0
    while i < 4 {
        var j: int = 0
        while j < 4 { s = s + i * j; j = j + 1 }
        i = i + 1
    }
    wok(s)
}"#),

    // ── truthiness ─────────────────────────────────────────────────────
    ("truthiness sebagai kondisi", r#"package main
pub fn main() {
    if 2 { wok("dua") } else { wok("SALAH") }
    var n: int = 5
    if n { wok("n") } else { wok("SALAH") }
    var z: int = 0
    if z { wok("SALAH") } else { wok("nol") }
    if "x" { wok("isi") } else { wok("SALAH") }
    if "" { wok("SALAH") } else { wok("kosong") }
}"#),

    ("konversi bool eksplisit", r#"package main
pub fn main() {
    wok(bool(0), bool(1), bool(-5), bool(0.0), bool(1.5))
    wok(bool(""), bool("x"), bool("0"), bool(true), bool(false))
}"#),

    // NaN falsy — `fcmp one`, bukan `une`. Perbedaan satu huruf yang cuma
    // kelihatan kalau NaN benar-benar diuji, dan sudah pernah menyimpang
    // antara interpreter dan VM.
    ("NaN falsy, inf truthy", r#"package main
pub fn main() {
    var n: f64 = math::NAN
    var i: f64 = math::INF
    wok(bool(n), bool(i))
    if n { wok("SALAH") } else { wok("NaN falsy") }
}"#),

    ("konstanta math lengkap", r#"package main
pub fn main() {
    wok(math::PI, math::E, math::TAU, math::INF, math::NAN)
}"#),

    // ── aturan indeks ──────────────────────────────────────────────────
    ("indeks negatif pada array, baca dan tulis", r#"package main
pub fn main() {
    var a: [4]int = [10, 20, 30, 40]
    wok(a[-1], a[-4], a[0], a[3])
    a[-1] = 99
    a[-4] = 11
    wok(a[3], a[0])
}"#),

    ("indeks negatif pada slice", r#"package main
fn ambil(a: array, i: int) -> int { return a[i] }
pub fn main() {
    var a: [5]int = [1, 2, 3, 4, 5]
    wok(ambil(a[0:4], -1), ambil(a[1:4], -2))
}"#),

    // Elemen tuple bertipe beda-beda, jadi indeksnya wajib konstan di
    // backend statis. Yang diuji di sini bahwa yang konstan BEKERJA dan
    // jawabannya sama.
    ("indeks tuple termasuk negatif", r#"package main
pub fn main() {
    var t: (int, string, bool) = (1, "dua", true)
    wok(t[0], t[1], t[2], t[-1], t[-3])
}"#),

    // Indeks string menghitung KARAKTER, bukan byte. Kalau per byte, `u[1]`
    // akan mengembalikan separuh karakter — sebuah rune yang bukan karakter
    // apa pun, tanpa error.
    ("indeks string per karakter, bukan byte", r#"package main
pub fn main() {
    var s: string = "halo"
    wok(s[0], s[3], s[-1])
    var u: string = "aé漢z"
    wok(len(u), u[0], u[1], u[2], u[3], u[-1])
}"#),

    // ── rune sebagai tipe tersendiri ───────────────────────────────────
    ("rune dicetak sebagai karakter", r#"package main
pub fn main() {
    var c: rune = 'h'
    wok(c, typeof(c), c as int)
    wok(65 as rune, typeof(65 as rune))
    wok('a' < 'b', 'a' == 'a', 'b' > 'a')
    var u: rune = '漢'
    wok(u, u as int)
}"#),

    // ── string: konkatenasi, pengulangan, slicing ──────────────────────
    ("konkatenasi string dua arah", r#"package main
pub fn main() {
    wok("n=" + 5, "f=" + 1.5, "b=" + true, "r=" + 'h')
    wok(5 + "=n", "a" + "b")
}"#),

    ("pengulangan string dua arah", r#"package main
pub fn main() {
    wok("ab" * 3, "ab" * 0, 3 * "ab")
}"#),

    // Slicing string menghitung KARAKTER. Kalau per byte, `u[1:3]` akan
    // memotong di tengah karakter dan menghasilkan byte yang bukan UTF-8
    // sah — string rusak, tanpa error.
    ("slicing string per karakter", r#"package main
pub fn main() {
    var s: string = "halo dunia"
    wok(s[0:4], s[5:], s[:4], s[-5:])
    var u: string = "aé漢z"
    wok(u[1:3], len(u[1:3]), u[:2], u[2:])
}"#),

    // Batas slice negatif — aturan yang sama dengan indeks, dan sebelumnya
    // dipanikkan di backend ini persis seperti indeks dulu.
    ("batas slice negatif pada array", r#"package main
fn cetak(a: array) { for i in 0..len(a) { wok_(a[i]) }; wok("") }
pub fn main() {
    var a: [5]int = [1, 2, 3, 4, 5]
    cetak(a[-3:])
    cetak(a[:-2])
    cetak(a[1:-1])
}"#),

    ("konkatenasi array", r#"package main
pub fn main() {
    var a: [2]int = [1, 2]
    var b: [2]int = [3, 4]
    var c = a + b
    wok(len(c), c[0], c[1], c[2], c[3])
    var s1: [2]string = ["a", "b"]
    var s2: [1]string = ["c"]
    var s3 = s1 + s2
    wok(len(s3), s3[0], s3[2])
}"#),

    // ── inferensi tipe ─────────────────────────────────────────────────
    ("parameter tanpa anotasi, banyak tipe", r#"package main
fn cetak(v) { wok(v) }
fn gandakan(x) -> int { return x * 2 }
pub fn main() {
    cetak(7)
    cetak("teks")
    cetak(1.5)
    cetak(true)
    wok(gandakan(21))
}"#),

    ("tipe return disimpulkan dari badan", r#"package main
fn bikin_array() {
    var lokal = [1, 2, 3]
    return lokal
}
fn jumlah(a: int, b: int) { return a + b }
fn nama() { return "jrg" }
fn ubah_dalam(a) { a[0][0] = 777; return a }
pub fn main() {
    var u = bikin_array()
    u[0] = 999
    wok(u[0], u[1], jumlah(2, 3), nama())
    var c: [2][2]int = [[1, 2], [3, 4]]
    let hasil = ubah_dalam(c)
    wok(hasil[0][0], c[0][0])
}"#),

    ("parameter method tanpa anotasi", r#"package main
struct K { pub n: int }
impl K {
    fn tambah(self, x) -> int { return self.n + x }
    fn gabung(self, s) -> string { return s + "!" }
}
pub fn main() {
    var k: K = K { n: 1 }
    wok(k.tambah(2), k.gabung("hai"))
}"#),

    // `match` tanpa konteks tipe: sebelumnya dianggap tak bernilai, jadi
    // `let z = if c { match .. } else { 5 }` ditolak dengan menuduh `if`-nya.
    ("match menyimpulkan tipe dari arm-nya", r#"package main
pub fn main() {
    let z = if true { match 1 { 1 => 111, _ => 0 } } else { 5 }
    wok(z)
    let w = match 2 { 1 => "satu", _ => "lain" }
    wok(w)
}"#),

    // ── pola array ─────────────────────────────────────────────────────
    ("pola array di match", r#"package main
fn klas(a: array) -> string {
    return match a {
        []           => "kosong",
        [x]          => "satu:" + x,
        [x, y]       => "dua:" + x + "," + y,
        [k, ...sisa] => "kepala:" + k + " sisa:" + len(sisa),
    }
}
pub fn main() {
    var e: [0]int = []
    var s: [1]int = [7]
    var d: [2]int = [1, 2]
    var b: [4]int = [1, 2, 3, 4]
    wok(klas(e), klas(s), klas(d), klas(b))
    match b { [p, ..., z] => wok("ujung:", p, z), _ => wok("?") }
    wok(match [1, 2, 3] { [k, ...] => k, _ => -1 })
    wok(match [[1, 2], [3, 4]] { [[a, b], [c, d]] => a + b + c + d, _ => -1 })
    wok(match [1, 99, 3] { [1, _, 3] => "cocok", _ => "tidak" })
    wok(match [2, 99, 3] { [1, _, 3] => "cocok", _ => "tidak" })
}"#),

    ("pola array di let", r#"package main
pub fn main() {
    let [m, n] = [10, 20]
    wok(m, n)
    let [h, ...t] = [1, 2, 3]
    wok(h, len(t), t)
}"#),

    // ── `nil` ──────────────────────────────────────────────────────────
    ("nil: cetak, banding, truthiness", r#"package main
pub fn main() {
    wok(typeof(nil), «${nil}», sprint(nil), nil)
    wok(nil == nil, nil != nil, nil == 1, 1 == nil)
    wok(bool(nil))
}"#),

    // ── template literal ───────────────────────────────────────────────
    ("template literal untuk semua tipe", r#"package main
struct P { pub x: int, pub y: string }
enum W { A, B }
pub fn main() {
    var n: int = 42
    var f: f64 = 1.5
    var b: bool = true
    var s: string = "dunia"
    var r: rune = 'z'
    var a: [3]int = [1, 2, 3]
    var t: (int, string) = (7, "x")
    var p: P = P { x: 1, y: "q" }
    wok(«halo ${s}, n=${n}»)
    wok(«float ${f} bool ${b} rune ${r}»)
    wok(«array ${a} slice ${a[0:2]} tuple ${t}»)
    wok(«struct ${p} enum ${W::A}»)
    wok(«hitung ${n * 2 + 1} dan ${n > 10}»)
    wok(«bersarang ${«dalam ${n}»}»)
    wok(«»)
}"#),

    // ── namespace `arr::` ──────────────────────────────────────────────
    ("arr:: mengembalikan nilai baru", r#"package main
pub fn main() {
    var a: [5]int = [3, 1, 4, 1, 5]
    wok(arr::push(a, 9))
    wok(arr::sum(a), arr::min(a), arr::max(a))
    wok(arr::reverse(a), arr::sort(a))
    wok(arr::contains(a, 4), arr::contains(a, 7))
    wok("asli:", a)
    var k: [0]int = []
    wok(arr::sum(k), arr::reverse(k), arr::contains(k, 1))
}"#),

    ("arr:: pada float dan string", r#"package main
pub fn main() {
    var f: [3]f64 = [2.5, 1.5, 3.0]
    wok(arr::sum(f), arr::min(f), arr::max(f), arr::sort(f))
    var s: [3]string = ["c", "a", "b"]
    wok(arr::sort(s), arr::contains(s, "a"), arr::reverse(s))
}"#),

    ("arr::push menumbuhkan di dalam loop", r#"package main
pub fn main() {
    var t = []
    for i in 0..5 { t = arr::push(t, i * i) }
    wok(len(t), t, arr::sum(t))
}"#),

    // ── array bertumbuh ────────────────────────────────────────────────
    ("kumpulkan hasil di dalam loop", r#"package main
pub fn main() {
    var t = []
    for i in 0..5 { t = t + [i] }
    wok(len(t), t.len(), t[0], t[4], t == [0, 1, 2, 3, 4])
}"#),

    ("kumpulkan tuple, dan di dalam if/match", r#"package main
pub fn main() {
    var a = []
    for i in 0..3 { a = a + [(i, i * 2)] }
    var b = []
    for i in 0..5 { if i > 1 { b = b + [i] } }
    var c = []
    for i in 0..5 { match i { 3 => break, _ => { c = c + [i] } } }
    wok(len(a), len(b), len(c))
}"#),

    // Lokal BERANOTASI tidak ikut berubah — kalau seseorang menulis
    // `var a: [2000]int`, ia meminta array berukuran tetap, dan mengubahnya
    // diam-diam jadi heap akan memperlambat kode yang sudah benar.
    ("array beranotasi tetap berukuran tetap", r#"package main
pub fn main() {
    var a: [3]int = [1, 2, 3]
    wok(len(a), a.len(), a[0])
}"#),

    // ── break/continue berlabel ────────────────────────────────────────
    ("break dan continue berlabel", r#"package main
pub fn main() {
    var a = []
    'luar: for i in 0..3 {
        for j in 0..3 {
            if i == 1 && j == 1 { break 'luar }
            a = a + [(i, j)]
        }
    }
    var b = []
    'lain: for i in 0..3 {
        for j in 0..3 {
            if j == 1 { continue 'lain }
            b = b + [(i, j)]
        }
    }
    var n = 0
    'w: while true { n = n + 1; if n > 2 { break 'w } }
    var m = 0
    'l: loop { m = m + 1; if m > 4 { break 'l } }
    wok(len(a), len(b), n, m)
}"#),

    // ── `typeof` menjawab nama JRG, bukan nama internal ────────────────
    ("typeof memakai nama JRG", r#"package main
struct P { pub x: int }
enum W { A }
fn kosong() {}
pub fn main() {
    var i: int = 1
    var f: f64 = 1.5
    var r: rune = 'a'
    var a: [2]int = [1, 2]
    var t: (int, string) = (1, "a")
    var p: P = P { x: 1 }
    var u: u32 = 7
    wok(typeof(i), typeof(f), typeof(r), typeof(u))
    wok(typeof(a), typeof(a[0:1]), typeof(t), typeof(p))
    wok(typeof(W::A), typeof(kosong()), typeof("x"), typeof(true))
}"#),

    // ── memindahkan slice juga menyalin ────────────────────────────────
    ("memindahkan slice ke nama lain menyalin", r#"package main
fn ambil(a: array) -> array { return a }
pub fn main() {
    var a: [4]int = [1, 2, 3, 4]
    var s = a[0:3]
    var t = s
    t[0] = 99
    wok(s[0], t[0])
    var g = []
    for i in 0..3 { g = g + [i] }
    var h = g
    h[0] = 77
    wok(g[0], h[0])
    var u = ambil(s)
    u[1] = 55
    wok(s[1], u[1])
}"#),

    // ── slice adalah NILAI, bukan pandangan ────────────────────────────
    ("menulis lewat slice tidak menyentuh asalnya", r#"package main
pub fn main() {
    var a: [4]int = [1, 2, 3, 4]
    var s = a[0:2]
    s[0] = 99
    wok(a[0], a[1], s[0], s[1])
}"#),

    ("slice dari slice juga terpisah", r#"package main
pub fn main() {
    var a: [4]int = [1, 2, 3, 4]
    var s = a[0:3]
    var t = s[0:2]
    t[0] = 55
    wok(a[0], s[0], t[0])
}"#),

    ("fungsi yang menulis slice kiriman tidak menyentuh asalnya", r#"package main
fn ubah(s: array) { s[0] = 900 }
pub fn main() {
    var a: [4]int = [1, 2, 3, 4]
    ubah(a[0:2])
    ubah(a)
    wok(a[0], a[1])
}"#),

    // Sebuah slice bisa MELEWATI hidup frame yang membuatnya.
    ("slice dikembalikan dari fungsi tetap utuh", r#"package main
fn bikin() -> array {
    var lokal: [4]int = [11, 22, 33, 44]
    return lokal[0:3]
}
fn ganggu() -> int {
    var sampah: [8]int = [7, 7, 7, 7, 7, 7, 7, 7]
    return sampah[0]
}
pub fn main() {
    var s = bikin()
    var _ = ganggu()
    wok(len(s), s[0], s[1], s[2])
}"#),

    // ── `a += b` berarti `a = a + b` ───────────────────────────────────
    ("penugasan majemuk pada string", r#"package main
pub fn main() {
    var s: string = "a"
    s += "b"
    s += 5
    wok(s)
    var t: string = "ab"
    t *= 3
    wok(t)
}"#),

    // ── array kosong sebagai literal lentur ────────────────────────────
    ("array kosong di kedua sisi penyambungan", r#"package main
pub fn main() {
    wok(len([1] + []), len([] + [1]))
    var s: [2]string = ["a", "b"]
    wok(len(s + []), len([] + s))
}"#),

    ("array kosong sebagai argumen dan nilai", r#"package main
fn ambil(a: array) -> int { return len(a) }
pub fn main() {
    wok(ambil([]))
    var t = []
    wok(len(t))
}"#),

    // Array berukuran tetap dibandingkan dengan slice: yang dibandingkan ISINYA.
    ("array tetap dibandingkan dengan slice", r#"package main
pub fn main() {
    var xs: [4]int = [1, 2, 3, 4]
    wok(xs[2:2] == [], xs[0:2] == [])
    wok(xs == xs[0:4], xs[0:2] == xs[0:2])
    wok([] == [])
}"#),

    // ── operasi string di level byte ───────────────────────────────────
    ("operasi string dengan karakter multi-byte", r#"package main
pub fn main() {
    var s: string = "Halo Dunia aé漢z"
    wok(str::upper(s))
    wok(str::lower(s))
    wok(str::reverse(s))
    wok(str::replace(s, "a", "_"))
    wok(str::replace(s, "漢", "X"))
    wok(str::contains(s, "Dunia"), str::contains(s, "漢"), str::contains(s, "zz"))
    wok(str::starts_with(s, "Halo"), str::ends_with(s, "z"))
    wok(len(s), str::trim("  x  "))
}"#),

    // Pola kosong menyisipkan penggantinya di setiap BATAS KARAKTER,
    // termasuk kedua ujung. Runtime LLVM dulu mengembalikan stringnya apa
    // adanya — divergensi yang sudah ada sebelum pekerjaan level-byte.
    ("replace dengan pola kosong", r#"package main
pub fn main() {
    wok(str::replace("aé漢", "", "-"))
    wok(str::replace("", "", "-"))
    wok(str::replace("abc", "", "."))
}"#),

    // ── kesetaraan struktural agregat ──────────────────────────────────
    ("eq array tetap", r#"package main
pub fn main() {
    var a: [2]int = [1, 2]
    var b: [2]int = [1, 2]
    var c: [2]int = [1, 3]
    wok(a == b, a == c, a != c, a != b)
}"#),

    ("eq tuple campur tipe", r#"package main
pub fn main() {
    var a: (int, string) = (1, "a")
    var b: (int, string) = (1, "a")
    var c: (int, string) = (2, "a")
    wok(a == b, a == c)
}"#),

    ("eq unit", r#"package main
fn kosong() {}
pub fn main() {
    wok(kosong() == kosong())
}"#),

    ("eq struct field demi field", r#"package main
struct P { pub x: int, pub y: int }
pub fn main() {
    wok(P { x: 1, y: 2 } == P { x: 1, y: 2 }, P { x: 1, y: 2 } == P { x: 9, y: 2 })
}"#),

    ("eq array bersarang", r#"package main
pub fn main() {
    var a: [2][2]int = [[1, 2], [3, 4]]
    var b: [2][2]int = [[1, 2], [3, 4]]
    var c: [2][2]int = [[1, 2], [3, 9]]
    wok(a == b, a == c)
}"#),

    ("eq float dan string di dalam array", r#"package main
pub fn main() {
    var f1: [2]f64 = [1.5, 2.5]
    var f2: [2]f64 = [1.5, 2.5]
    var s1: [2]string = ["a", "b"]
    var s2: [2]string = ["a", "c"]
    wok(f1 == f2, s1 == s2)
}"#),

    // Slice: panjang harus dibandingkan LEBIH DULU. Tanpa itu loopnya
    // membaca melewati ujung yang lebih pendek.
    ("eq slice termasuk beda panjang", r#"package main
fn sama(a: array, b: array) -> bool { return a == b }
pub fn main() {
    var x: [5]int = [1, 2, 3, 4, 5]
    var y: [5]int = [1, 2, 9, 4, 5]
    wok(sama(x[0:3], y[0:2]), sama(x[0:3], y[0:3]), sama(x[0:2], y[0:2]))
}"#),

    // Jalur LOOP, bukan unroll: interpreter menerima array besar, jadi
    // meng-unroll perbandingannya akan meledakkan IR. Kasus "beda di elemen
    // 0" ikut menguji keluar-lebih-awal.
    ("eq array besar lewat loop", r#"package main
pub fn main() {
    var a: [2000]int = [7; 2000]
    var b: [2000]int = [7; 2000]
    wok(a == b)
    b[1999] = 8
    wok(a == b)
    b[1999] = 7
    b[0] = 8
    wok(a == b)
}"#),

    // Sisi POSITIF dari penegakan visibilitas: field privat harus tetap bisa
    // diakses dari dalam method tipenya sendiri, baca maupun tulis.
    ("field privat boleh diakses dari method sendiri", r#"package main
struct Akun { saldo: int = 100, pub nama: string = "a" }
impl Akun {
    fn lihat(self) -> int { return self.saldo }
    fn tambah(self) -> int {
        self.saldo = self.saldo + 5
        return self.saldo
    }
}
pub fn main() {
    var a: Akun = Akun {}
    wok(a.lihat(), a.tambah(), a.nama)
}"#),

    ("shadowing di blok", r#"package main
pub fn main() {
    var x: int = 1
    { var x: int = 2; wok(x) }
    wok(x)
}"#),

    // ── Closure yang dipanggil langsung, dan `move` di posisi HOF ────────
    ("closure langsung dipanggil",        "package main\npub fn main() { wok((|x| x * 2)(4)) }"),
    ("closure langsung, dua parameter",   "package main\npub fn main() { wok((|a, b| a + b)(3, 5)) }"),
    ("closure langsung, argumen string",  "package main\npub fn main() { wok((|s| s + \"!\")(\"halo\")) }"),
    ("closure langsung, tanpa parameter", "package main\npub fn main() { wok((|| 7)()) }"),
    ("closure langsung, anotasi menang",  "package main\npub fn main() { wok((|x: f64| x * 2.0)(1.5)) }"),
    ("closure langsung, move",            "package main\npub fn main() { var n = 1\n wok((move |x| x + n)(10)) }"),
    ("move di HOF memutus tautan",        "package main\npub fn main() { var n = 0\n let b = [1, 2, 3].map(move |x| { n = n + 1\n return x })\n wok(n, b) }"),

    // ── Tangkapan atas PARAMETER closure yang melingkupi ─────────────────
    ("tangkap: fn -> closure atas param fn",     "package main\nfn bikin(a: int) -> fn(int) -> int { return |b: int| { return a + b } }\npub fn main() { wok(bikin(3)(4)) }"),
    ("tangkap: closure atas lokal luar",         "package main\npub fn main() { let n = 3\n let c = |b: int| { return n + b }\n wok(c(4)) }"),
    ("tangkap: closure->closure atas lokal",     "package main\npub fn main() { let d = |a: int| { let m = a\n return |b: int| { return m + b } }\n wok(d(3)(4)) }"),
    ("tangkap: closure->closure atas PARAMETER", "package main\npub fn main() { let e = |a: int| { return |b: int| { return a + b } }\n wok(e(3)(4)) }"),

    // ── Closure berantai (currying) ──────────────────────────────────────
    ("currying: dua tingkat",  "package main\npub fn main() { let luar = |a| { return |b| { return a + b } }\n wok(luar(3)(4)) }"),
    ("currying: tiga tingkat", "package main\npub fn main() { let t = |a| { return |b| { return |c| { return a + b + c } } }\n wok(t(1)(2)(3)) }"),
    ("currying: tipe campur",  "package main\npub fn main() { let c = |s| { return |n| { return s + (n as string) } }\n wok(c(\"x\")(5)) }"),

    // ── Paruh `Result` dari situs panggilan LAIN ─────────────────────────
    ("result: paruh dari situs panggilan lain",
     "package main\nfn lewat(r) { let v = r?\n return Ok(v * 2) }\npub fn main() { wok(lewat(Ok(21)))\n wok(lewat(Err(\"gagal\"))) }"),

    // ── Kesetaraan enum ──────────────────────────────────────────────────
    ("enum ==: Result sama dan beda",
     "package main\npub fn main() { let a: Result<int, string> = Ok(1)\n let b: Result<int, string> = Ok(1)\n let c: Result<int, string> = Ok(2)\n wok(a == b, a == c, a != c) }"),
    ("enum ==: lintas variant",
     "package main\npub fn main() { let a: Result<int, string> = Ok(1)\n let d: Result<int, string> = Err(\"x\")\n wok(a == d, d == d) }"),
    ("enum ==: Option dengan variant unit",
     "package main\npub fn main() { let s: Option<int> = Some(3)\n let t: Option<int> = Some(4)\n let u: Option<int> = None\n wok(s == s, s == t, s == u, u == u) }"),
    ("enum ==: enum pengguna, payload jamak",
     "package main\nenum B { Lingkaran(float), Kotak(float, float), Titik }\npub fn main() { wok(B::Titik == B::Titik, B::Lingkaran(2.0) == B::Lingkaran(2.0), B::Lingkaran(2.0) == B::Lingkaran(3.0)) }"),
    ("enum ==: payload dua field",
     "package main\nenum B { Lingkaran(float), Kotak(float, float), Titik }\npub fn main() { wok(B::Kotak(1.0, 2.0) == B::Kotak(1.0, 2.0), B::Kotak(1.0, 2.0) == B::Kotak(1.0, 9.0), B::Lingkaran(2.0) == B::Titik) }"),

    // ── `d.kunci` pada sebuah dict ───────────────────────────────────────
    ("dict: baca lewat .kunci",  "package main\npub fn main() { let d = {\"a\": 1, \"b\": 2}\n wok(d[\"a\"], d.a, d.b) }"),
    ("dict: tulis lewat .kunci", "package main\npub fn main() { var d = {\"a\": 1}\n d.a = 9\n wok(d.a, d[\"a\"]) }"),
    ("dict: .kunci baru menumbuhkan", "package main\npub fn main() { var d = {\"a\": 1}\n d.c = 30\n wok(d.c, dict::len(d), d) }"),
    ("dict: .kunci dengan +=",   "package main\npub fn main() { var d = {\"a\": 1}\n d.a += 5\n wok(d.a) }"),
    // ── `dict::new()` dan method gaya `d.len()` ──────────────────────────
    ("dict::new lalu diisi",     "package main\npub fn main() { var b = dict::new()\n b[\"x\"] = \"halo\"\n wok(b, dict::len(b)) }"),
    ("dict::new tak pernah diisi","package main\npub fn main() { let k = dict::new()\n wok(dict::len(k), dict::is_empty(k), k) }"),
    ("dict method: len/is_empty/has", "package main\npub fn main() { let d = {\"a\": 1, \"b\": 2}\n wok(d.len(), d.is_empty(), d.has(\"a\"), d.has(\"z\")) }"),
    ("dict method: keys/values",  "package main\npub fn main() { let d = {\"a\": 1, \"b\": 2}\n wok(d.keys(), d.values()) }"),
    ("dict method: remove/merge", "package main\npub fn main() { let d = {\"a\": 1, \"b\": 2}\n wok(d.remove(\"a\"), d.merge({\"c\": 3})) }"),
    ("dict method: get",          "package main\npub fn main() { let d = {\"a\": 1}\n wok(d.get(\"a\"), d.get(\"z\")) }"),

    // ── Nilai bertag: dict bernilai CAMPUR TIPE ──────────────────────────
    ("any: dict campur, baca kunci tertulis",
     "package main\npub fn main() { let d = {\"nama\": \"budi\", \"umur\": 17}\n wok(d[\"nama\"], d[\"umur\"], dict::len(d)) }"),
    ("any: dict campur lewat .kunci",
     "package main\npub fn main() { let d = {nama: \"siti\", umur: 20}\n wok(d[\"nama\"], d.umur) }"),
    ("any: dict campur bersarang",
     "package main\npub fn main() { let b = {\"angka\": [1, 2], \"dalam\": {\"k\": \"v\"}}\n wok(b[\"angka\"][1], b[\"dalam\"][\"k\"]) }"),
    ("any: dict campur dengan nil",
     "package main\npub fn main() { let g = {\"ada\": 1, \"kosong\": nil}\n wok(g.get(\"ada\"), g.get(\"kosong\"), g.get(\"tak_ada\")) }"),
    ("any: Option berpayload nol",
     "package main\npub fn main() { let g = {\"ada\": 1, \"kosong\": nil}\n wok(g.get(\"kosong\") == Some(nil), g.get(\"tak_ada\").is_none()) }"),

    // ── Nilai bertag: `var` yang BERGANTI tipe elemen ────────────────────
    ("any: var berganti tipe elemen",
     "package main\nvar jejak = []\nfn catat(s) { jejak = arr::push(jejak, s) }\npub fn main() { jejak = []\n catat(\"a\")\n wok(jejak)\n jejak = []\n catat(1)\n wok(jejak) }"),
    ("any: konflik lewat defer",
     "package main\nvar jejak = []\nfn catat(s) { jejak = arr::push(jejak, s) }\nfn pakai() { defer catat(7)\n catat(\"x\") }\npub fn main() { pakai()\n wok(jejak) }"),

    // Yang TIDAK boleh ikut berubah: koleksi bertipe seragam tetap telanjang.
    // Kalau kotak bocor ke sini, keluarannya masih benar tapi jalannya sudah
    // beda — jadi yang dijaga di sini bukan sekadar jawabannya.
    ("any: dict seragam tetap seragam",
     "package main\npub fn main() { let d = {\"a\": 1, \"b\": 2}\n wok(d[\"a\"] + d[\"b\"], dict::len(d)) }"),
    ("any: var tipe elemen konsisten",
     "package main\nvar t = []\nfn tambah(x) { t = arr::push(t, x) }\npub fn main() { tambah(1)\n tambah(2)\n wok(t) }"),

    // ── Method koleksi yang mengembalikan `Option` ───────────────────────
    ("koleksi: first/last/get",
     "package main\npub fn main() { let a = [10, 20, 30]\n wok(a.first(), a.last(), a.get(1), a.get(99)) }"),
    ("koleksi: first pada kosong",
     "package main\npub fn main() { let k = []\n wok(k.first(), k.last()) }"),
    ("koleksi: Some(nil) beda dari None",
     "package main\npub fn main() { let a = [nil]\n wok(a.first(), a.first().is_some()) }"),
    ("koleksi: get_or",
     "package main\npub fn main() { let a = [10, 20]\n wok(a.get_or(1, -1), a.get_or(9, -1)) }"),
    ("koleksi: pop",
     "package main\npub fn main() { let a = [10, 20, 30]\n let (s, d) = a.pop()\n wok(s, d, a) }"),
    ("koleksi: pop pada kosong",
     "package main\npub fn main() { let k = []\n let (s, d) = k.pop()\n wok(s, d) }"),
    ("koleksi: min/max sbg method",
     "package main\npub fn main() { wok([3, 1, 2].min(), [3, 1, 2].max(), [1, 2].sum()) }"),

    // ── Namespace `str::` ────────────────────────────────────────────────
    ("str:: bentuk namespace",
     "package main\npub fn main() { wok(str::contains(\"halo\", \"al\"), str::upper(\"ab\"), str::trim(\"  x  \")) }"),
    ("str:: is_empty/char_at/repeat",
     "package main\npub fn main() { wok(str::is_empty(\"\"), str::is_empty(\"a\"), str::char_at(\"halo\", 1), str::repeat(\"ab\", 3)) }"),

    // ── Iterasi dict memberi KUNCI ───────────────────────────────────────
    ("dict: iterasi memberi kunci",
     "package main\npub fn main() { let d = {\"b\": 2, \"a\": 1}\n var t = []\n for k in d { t = t + [k] }\n wok(t) }"),
    ("dict: iterasi dua variabel",
     "package main\npub fn main() { let d = {\"b\": 2, \"a\": 1}\n var t = []\n for k, v in d { t = t + [«${k}=${v}»] }\n wok(t) }"),

    // ── Lokal SKALAR yang berganti tipe ──────────────────────────────────
    ("dinamis: int lalu float",   "package main\npub fn main() { var x = 1\n x = 2.5\n wok(x, typeof(x)) }"),
    ("dinamis: int lalu bool",    "package main\npub fn main() { var y = 1\n y = true\n wok(y, typeof(y)) }"),
    ("dinamis: float lalu int",   "package main\npub fn main() { var z = 3.9\n z = 1\n wok(z, typeof(z)) }"),
    ("dinamis: berganti di dalam if", "package main\npub fn main() { var x = 1\n if true { x = \"teks\" }\n wok(x, typeof(x)) }"),
    ("dinamis: int lalu string",  "package main\npub fn main() { var x = 1\n x = \"teks\"\n wok(x, typeof(x)) }"),
    ("dinamis: dibaca sebelum berganti", "package main\npub fn main() { var x = 1\n wok(x, typeof(x))\n x = \"teks\"\n wok(x, typeof(x)) }"),
    ("dinamis: global skalar",    "package main\nvar g = 1\npub fn main() { g = \"teks\"\n wok(g, typeof(g)) }"),

    // Aritmetika dan perbandingan atas nilai yang golongannya baru diketahui
    // saat jalan.
    ("dinamis: aritmetika setelah berganti", "package main\npub fn main() { var x = 1\n x = \"a\"\n x = 2\n wok(x + 1) }"),
    ("dinamis: int + float bertag", "package main\npub fn main() { var y = 1\n y = 2.5\n wok(y + 1, y * 2) }"),
    ("dinamis: gabung string dua arah", "package main\npub fn main() { var s = 1\n s = \"n=\"\n var t = 1\n t = \"x\"\n wok(s + 5, 5 + t) }"),
    ("dinamis: kesetaraan",       "package main\npub fn main() { var x = 1\n x = 2\n var y = 1\n y = \"teks\"\n wok(x == 2, x != 2, y == \"teks\", y == \"lain\") }"),
    ("dinamis: urutan angka",     "package main\npub fn main() { var z = 1\n z = 2.5\n wok(z > 2.0, z < 2.0, z >= 2.5) }"),
    ("dinamis: typeof dari tag",  "package main\npub fn main() { let d = {\"a\": 1, \"b\": \"t\"}\n let k = \"a\"\n wok(typeof(d[k])) }"),

    // Yang TIDAK boleh ikut berubah: lokal yang tipenya konsisten tetap
    // telanjang. Kalau kotaknya bocor ke sini, jawabannya masih benar tapi
    // jalannya sudah beda — jadi yang dijaga bukan sekadar keluarannya.
    ("dinamis: lokal konsisten tetap telanjang", "package main\npub fn main() { var x = 1\n x = 2\n x = 3\n wok(x + 1, typeof(x)) }"),
    ("dinamis: float konsisten tetap telanjang", "package main\npub fn main() { var f = 1.5\n f = 2.5\n wok(f * 2.0, typeof(f)) }"),
    // Posisi per KARAKTER, sama seperti dua backend lain — dan `find`/
    // `index_of` sebelumnya tidak ada di sini sama sekali.
    ("posisi: find per karakter",    "package main\npub fn main() { wok(str::find(\"héllo\", \"l\"), \"héllo\".index_of(\"l\")) }"),
    // Bentuk pulang-perginya lewat `Option` sejak Fase MUDAH. Kasus lamanya
    // berbunyi `s[str::find(s, "k")]` — yang persis jebakan yang ditutup: `-1`
    // adalah indeks yang SAH, jadi pencarian gagal membaca karakter terakhir.
    ("posisi: pulang-pergi s[find]",
     "package main\npub fn main() { let s = \"日本語kode\"\n       match str::find(s, \"k\") { Some(i) => wok(s[i]), None => wok(\"tidak ada\") }\n       match str::find(s, \"z\") { Some(i) => wok(s[i]), None => wok(\"tidak ada\") } }"),
    ("posisi: find tidak ketemu",    "package main\npub fn main() { wok(str::find(\"halo\", \"z\")) }"),

    // `**=` dan struct tuple keduanya diturunkan di PARSER, jadi backend ini
    // mewarisinya tanpa disentuh — dan itu justru yang perlu dibuktikan.
    ("operator **=",
     "package main\npub fn main() { var a = 2\n a **= 10\n wok(a) }"),
    ("struct tuple: konstruksi dan field",
     "package main\nstruct Titik(int, int)\n\
      pub fn main() { let t = Titik(3, 4)\n wok(t._0 + t._1) }"),

    // Pola di posisi parameter, diturunkan di parser jadi parameter biasa plus
    // `let` — jadi backend ini mewarisinya tanpa disentuh.
    ("pola di parameter",
     "package main\nfn jumlah((a, b)) -> int { return a + b }\n\
      pub fn main() { wok(jumlah((3, 4))) }"),

    // Bentuk namespace HOF dialihkan ke bentuk method yang sudah ada di backend ini, dan `fold`
    // memakai mesin `reduce` dengan letak nilai awal yang bertukar.
    ("hof namespace dan fold",
     "package main\npub fn main() { wok(arr::map([1, 2, 3], |x| x * 2))\n\
      wok(arr::filter([1, 2, 3], |x| x > 1))\n wok(arr::reduce([1, 2, 3], |a, b| a + b, 0))\n\
      wok(arr::any([1, 2], |x| x > 1))\n wok(arr::find([1, 2], |x| x > 1))\n\
      wok([1, 2, 3].fold(10, |a, b| a + b))\n wok(arr::fold([1, 2, 3], 10, |a, b| a + b)) }"),

    // ── Warisan struct (Fase LLVM-14) ──────────────────────────────────────
    (
        "warisan: field induk terbaca, dan tampilannya sama",
        r#"package main
struct Hewan { pub nama: string }
struct Anjing: Hewan { pub ras: string }
pub fn main() {
    let a = Anjing { nama: "Bo", ras: "beagle" }
    wok(a.nama, a.ras)
    wok(a)
}"#),
    (
        "warisan: nilai default milik induk ikut berlaku",
        r#"package main
struct Hewan { pub nama: string = "tanpa nama", pub kaki: int = 4 }
struct Anjing: Hewan { pub ras: string = "campuran" }
pub fn main() { let a = Anjing {}  wok(a.nama, a.kaki, a.ras) }"#),
    (
        "warisan: method induk dipanggil atas anak",
        r#"package main
struct Hewan { pub nama: string }
impl Hewan { pub fn sapa(self) -> string { return "halo " + self.nama } }
struct Anjing: Hewan { pub ras: string }
pub fn main() { wok(Anjing { nama: "Bo", ras: "beagle" }.sapa()) }"#),
    (
        "warisan: rantai tiga tingkat",
        r#"package main
struct A { pub a: int }
struct B: A { pub b: int }
struct C: B { pub c: int }
pub fn main() { let x = C { a: 1, b: 2, c: 3 }  wok(x.a + x.b + x.c) }"#),
    // Bayangan yang MENGGANTI TIPE menentukan rancangannya.
    (
        "warisan: anak membayangi field induk, tipenya ikut berganti",
        r#"package main
struct P { pub n: int }
impl P { pub fn lihat(self) -> string { return "n=" + self.n } }
struct Q: P { pub n: string }
pub fn main() { let q = Q { n: "teks" }  wok(q.n)  wok(q.lihat()) }"#),
    (
        "warisan: field privat induk lewat method warisan",
        r#"package main
struct P { rahasia: int = 5 }
impl P { pub fn baca(self) -> int { return self.rahasia } }
struct Q: P { pub x: int }
pub fn main() { wok(Q { x: 1 }.baca()) }"#),
    (
        "warisan: pola, sebar, dan hook to_string",
        r#"package main
struct Hewan { pub nama: string = "x", pub kaki: int = 4 }
impl Hewan { pub fn to_string(self) -> string { return "<" + self.nama + ">" } }
struct Anjing: Hewan { pub ras: string = "y" }
pub fn main() {
    let a = Anjing { nama: "Bo", ras: "beagle" }
    match a { Anjing { nama, ras } => wok(nama, ras) }
    let b = Anjing { nama: "Rex", ...a }
    wok(b.nama, b.kaki, b.ras)
    wok(a)
}"#),

    // Fase SINKRON-LLVM: `Atomic` dan `Mutex` adalah pegangan di backend ini juga.
    ("Atomic: baca, tulis, tambah, cas, dan salinan yang menunjuk keadaan yang sama", r#"package main
pub fn main() {
    let a = Atomic::new(5)
    wok(a, typeof(a))
    wok(atomic::add(a, 3), atomic::sub(a, 1), atomic::get(a))
    wok(atomic::set(a, 10), atomic::get(a))
    wok(atomic::cas(a, 10, 20), atomic::cas(a, 10, 30), atomic::get(a))
    let b = a
    atomic::add(b, 1)
    wok(atomic::get(a), a == b, a == Atomic::new(21), a != b)
    wok([a, b], (a, 1), "x" + str(a))
    let z = Atomic::new()
    wok(atomic::get(z))
}"#),
    ("Atomic dan Mutex sebagai field, lewat spawn dan closure", r#"package main
struct K { pub hit: Atomic, pub m: Mutex }
fn naik(a: Atomic, n: int) { for i in 0..n { atomic::add(a, 1) } }
pub fn main() {
    let k = K { hit: Atomic::new(0), m: Mutex::new() }
    let ch = make_chan(int, 4)
    for i in 0..4 {
        spawn { naik(k.hit, 100); ch <- 1 }
    }
    for i in 0..4 { <-ch }
    wok(atomic::get(k.hit), k)
    let f = |x| { atomic::add(k.hit, x) }
    wok(f(5))
    var total = 0
    let m = k.m
    wok(m, typeof(m), m == k.m, m == Mutex::new())
    wok(mutex::lock(m))
    wok(mutex::unlock(m))
    let r = mutex::with_lock(m, || { 7 * 6 })
    wok(r)
    wok(mutex::with_lock(m, || "teks"))
    wok("beres")
}"#),
    ("mutex::with_lock menjaga baca-tidur-tulis antar goroutine", r#"package main
pub fn main() {
    let m = Mutex::new()
    let p = Atomic::new(0)
    let ch = make_chan(int, 8)
    for i in 0..8 {
        spawn {
            mutex::with_lock(m, || {
                let v = atomic::get(p)
                time::sleep(0.001)
                atomic::set(p, v + 1)
            })
            ch <- i
        }
    }
    for i in 0..8 { <-ch }
    wok(atomic::get(p))
}"#),
    ("ekor badan perulangan bukan nilai kembalian fungsi tanpa anotasi", r#"package main
fn hitung(n: int) { for i in 0..n { math::abs(i) } }
fn putar(n: int) { var i = 0
    while i < n { i += 1; str::upper("a") } }
fn ulang(xs: []int) { for _, x in xs { x * 2 } }
fn cari(xs: []int) { for _, x in xs { if x > 1 { return x } }
    return -1 }
pub fn main() {
    wok(hitung(3), putar(2), ulang([1, 2]), cari([1, 5]))
}"#),
    ("Atomic dan Mutex lewat method, fungsi tanpa anotasi, Future, Task, Option dan dict", r#"package main
struct Pencacah { pub n: Atomic, pub m: Mutex }
impl Pencacah {
    fn baru() -> Pencacah { Pencacah { n: Atomic::new(0), m: Mutex::new() } }
    fn naik(self) { atomic::add(self.n, 1) }
    fn baca(self) -> int { atomic::get(self.n) }
}
fn buat() { Atomic::new(7) }
fn kunci_baru() { Mutex::new() }
fn tambah(a, n) { atomic::add(a, n) }
fn tukar(a: Atomic) { atomic::cas(a, 7, 70) }
pub fn main() {
    let p = Pencacah::baru()
    p.naik(); p.naik()
    let q = p
    q.naik()
    wok(p.baca(), q.baca(), p == q)
    let a = buat()
    wok(tambah(a, 3), tukar(a), tukar(buat()), atomic::get(a))
    let m = kunci_baru()
    let hasil = mutex::with_lock(m, || { atomic::add(a, 1); [atomic::get(a), 2] })
    wok(hasil, str(a), str(m), """${a} ${m}""")
    let f = Future::new(|| { mutex::with_lock(m, || atomic::add(a, 5)) })
    wok(future::await(f))
    var hit = Atomic::new(0)
    let t = Task::spawn(|| { for i in 0..1000 { atomic::add(hit, 2) }; atomic::get(hit) })
    wok(task::join(t), atomic::get(hit))
    hit = Atomic::new(-4)
    wok(atomic::get(hit), atomic::sub(hit, -9223372036854775807))
    let o: Option<Atomic> = Some(a)
    wok(o, typeof(o), [m] == [m], (a, m) == (a, m), (a, m) == (buat(), m))
    let d = {"x": a, "y": buat()}
    wok(d, atomic::add(d["x"], 1), atomic::get(a))
    let g = Future::new(|| { mutex::with_lock(m, || { let z = [1]; z[4] }) })
    time::sleep(0.02)
    mutex::lock(m); wok("kunci bebas lagi"); mutex::unlock(m)
}"#),

];

// ─── Tabel 2: gap yang diketahui
// ───────────────────────────────────────────

const BELUM_DIDUKUNG_LLVM: &[(&str, &str, &str)] = &[
    // Fase KEPUTUSAN-1: dulu literalnya diam-diam jadi `2.0` (dua mesin lain menyimpan `2`).
    ("literal bulat ditambahkan ke array pecahan",
     "package main\npub fn main() { var f = [1.5]\n f = arr::push(f, 2)\n wok(f) }",
     "masuk ke tempat yang bertipe `float`"),
    ("literal bulat di let beranotasi float",
     "package main\npub fn main() { let x: float = 7\n wok(x / 2) }",
     "ditulis `7.0`"),
    // Fase SUBSTR: eksponen negatif yang baru diketahui saat jalan; `int ** int` bertipe `int`
    // di LLVM, dua mesin lain menjawab `0.5`.
    ("pangkat bulat dengan eksponen negatif dari variabel",
     "package main\npub fn main() { let n = -1\n wok(2 ** n) }",
     "`2 ** -1`: pangkat negatif menjawab pecahan"),
    // Fase SAPU-29: acuan menjawab `nil`; LLVM dulu menjawab `0`, sekarang menolak saat jalan.
    ("jawa::array: elemen yang belum pernah diisi",
     "package main\npub fn main() { let a = jawa::array(3)\n wok(jawa::array_get(a, 1)) }",
     "belum pernah diisi"),
    ("jawa::set: rune disimpan sebagai bilangan",
     "package main\npub fn main() { let p = jawa::alloc(1)\n jawa::set(p, 'a')\n wok(jawa::get(p)) }",
     "cuma menyimpan bilangan bulat, diberi `rune`"),
    // Dua kotak berisi tuple sama panjang tapi beda tipe elemen: acuan membandingkan per elemen
    // (`true`); LLVM dulu menjawab `false`, sekarang menolak saat jalan.
    ("nilai bertag: == dua tuple beda tipe elemen",
     "package main\npub fn main() { var u = nil\n var v = nil\n u = (1, 2)\n v = (1.0, 2.0)\n wok(u == v) }",
     "membandingkan dua tuple yang tipe elemennya berbeda di tempat campur tipe belum didukung backend"),
    // Fase UMUM: struct di dalam nilai bertag dibuka sebagai struct LAIN — gagal saat jalan,
    // menyebut kedua tipenya (acuan mengganti tipe variabelnya).
    ("nilai bertag: struct dibuka sebagai struct lain",
     "package main\nstruct P { pub x: int }\nstruct Q { pub y: int }\npub fn main() { var y = nil\n y = P { x: 7 }\n let q: Q = y\n wok(q.y) }",
     "nilai bertipe `struct:P` tidak bisa mengisi tempat yang bertipe `struct:Q`"),
    // Fase KORUTIN: kebuntuan DILAPORKAN di LLVM (korutin satu utas bisa melihatnya); dua mesin
    // lain menggantung selamanya, jadi kasus ini tidak pernah dijalankan di sana.
    ("kebuntuan: menerima dari channel yang tidak punya pengirim",
     "package main\npub fn main() { let c = make_chan(int)\n wok(<-c) }",
     "kebuntuan: semua goroutine sedang menunggu channel"),
    // Fase ISI: kotak bertag mencatat isi array/dict-nya. Dulu dua program ini mencetak sampah
    // tanpa pesan (pemeriksaan tagnya cuma tahu "ini array").
    ("nilai bertag: array<int> dibuka sebagai []string", "package main\npub fn main() {\n    var x = nil\n    x = [1, 2, 3]\n    let a: []string = x\n    wok(len(a))\n    wok(a[0])\n}\n",
     "nilai bertipe `array<int>` tidak bisa mengisi tempat yang bertipe `array<string>`"),
    ("nilai bertag: isi dict campur dibuka sebagai []float", "package main\npub fn main() {\n    var d = {\"a\": 1, \"b\": \"teks\"}\n    d[\"c\"] = [1, 2, 3]\n    let a: []float = d[\"c\"]\n    wok(a)\n    let k = \"c\"\n    let b: [][]int = d[k]\n    wok(len(b))\n    wok(b)\n}\n",
     "nilai bertipe `array<int>` tidak bisa mengisi tempat yang bertipe `array<float>`"),
    // Fase BUNGKUS: nilai bertag yang tagnya tidak cocok dengan tipe variabelnya gagal saat
    // jalan menyebut tipe aslinya — acuan mengganti tipe variabelnya, backend statis tidak.
    ("nilai bertag ke parameter beranotasi tipe lain", "package main\nstruct Kotak { pub isi: array }\nfn f(n: int) { wok(n) }\npub fn main() { let t = Kotak { isi: [\"a\"] }\n f(t.isi[0]) }",
     "nilai bertipe `string` tidak bisa mengisi tempat yang bertipe `int`"),
    // Fase LEBAR-LLVM: `+`/`-`/`*`, negasi, `arr::sum` dan `math::abs` dulu MEMBUNGKUS di
    // lebar tipenya; sekarang gagal saat jalan sambil menyebut hasil yang sebenarnya.
    ("int 64 bit: penjumlahan yang meluap dilaporkan", "package main\npub fn main() { var x = 9223372036854775807\n wok(x + 1) }",
     "= 9223372036854775808 tidak muat di bilangan bulat 64 bit bertanda"),
    ("int 64 bit: perkalian dan negasi yang meluap dilaporkan", "package main\npub fn main() { var x = 4000000000\n var m = -9223372036854775807 - 1\n if x > 0 { wok(-m) }\n wok(x * x) }",
     "tidak muat di bilangan bulat 64 bit bertanda"),
    ("uint: pengurangan di bawah nol dilaporkan", "package main\npub fn main() { var x: uint = 5\n wok(x - 6) }",
     "= -1 tidak muat di bilangan bulat 64 bit tak bertanda"),
    ("arr::sum dan math::abs yang meluap dilaporkan", "package main\npub fn main() { let a = [9223372036854775807, 1]\n wok(arr::sum(a)) }",
     "tidak muat di bilangan bulat 64 bit bertanda"),
    // Fase PACU-3: dulu dipotong diam-diam (`-1`, `44`); literalnya sekarang ditolak.
    ("literal di atas 64 bit", "package main\npub fn main() { let m = 170141183460469231731687303715884105727\n wok(m - 1) }",
     "tidak muat di `int` backend LLVM"),
    ("literal tidak muat di anotasinya", "package main\npub fn main() { let x: u8 = 300\n wok(x) }",
     "tidak muat di `u8`"),
    // Tahap 5: dulu DIVERGENSI — LLVM mengarang `D {x: 0}`, `Some(0)`, varian pertama.
    ("var struct tanpa nilai awal", "package main\nstruct D { x: int = 5 }\npub fn main() { var a: D\n wok(a) }",
     "tidak punya nilai nol"),
    ("var Option tanpa nilai awal", "package main\npub fn main() { var a: Option<int>\n wok(a) }",
     "tidak punya nilai nol"),
    ("var enum tanpa nilai awal", "package main\nenum Warna { Merah, Hijau }\npub fn main() { var a: Warna\n wok(a) }",
     "tidak punya nilai nol"),
    ("math::max bulat dan pecahan: tipenya baru diketahui saat jalan", r#"package main
pub fn main() { wok(math::max(1, 2.5)) }"#, "atas bilangan bulat dan pecahan sekaligus"),
    // `extern "C"` jalan sejak Fase EKSTERN-LLVM (lewat jembatan ke `ffi::native`); yang tersisa
    // nilai FUNGSI sebagai argumen: acuan menjawab `Err` dari lapisan FFI, LLVM menolak di kompilasi.
    ("panggilan extern C: fungsi sebagai argumen tidak bisa menyeberang", r#"package main
extern "C" { fn atexit(f: fn()) -> i32 }
pub fn main() { let f = || 1; wok(atexit(f)) }"#, "tidak bisa diserahkan ke `atexit`"),
    // "urutan pada array" dulu di sini; sejak Fase UMUM array dan tuple punya urutan di LLVM.
    // "variabel berganti tipe" DULU di sini, dan ia satu-satunya entri di tabel
    // ini yang tidak selalu menolak: `var x = 1; x = 2.5` dikompilasi dan …

        ("channel", r#"package main
pub fn main() {
    var c: chan int = make(chan int, 1)
    wok(1)
}"#, "chan"),

    // "goroutine spawn" dulu di sini; sejak Fase KORUTIN `spawn` jalan sebagai korutin.

    // Kesetaraan enum DULU di sini, dengan alasan yang benar: payload berbagi
    // tempat antar variant, jadi membandingkan blobnya langsung menjawab SALAH.

    // Literal dict diturunkan jadi panggilan `dict::from_pairs`, jadi pesannya
    // harus mengakui `dict::` sebagai namespace stdlib yang nyata — bukan …

    // Indeks tuple non-konstan: bukan kemalasan, tapi konsekuensi tipe.
    ("indeks tuple non-konstan", r#"package main
pub fn main() {
    var t: (int, int) = (1, 2)
    var i: int = 0
    wok(t[i])
}"#, "sudah pasti saat kompilasi"),

    // Dict bernilai campur tipe DULU di sini, dengan alasan yang benar: satu
    // buffer pasangan `{ %JrgStr, V }` cuma punya satu `V`, jadi nilai campur …
    ("spread di pemanggilan",
     "package main\nfn tambah(a, b) { return a + b }\n\
      pub fn main() { wok(tambah(...[1, 2])) }",
     "spread di pemanggilan"),

    // Argumen bernama: urutannya ditentukan dari nama parameter fungsinya saat
    // JALAN. Yang dikunci di sini bahwa penolakannya menyebut sebabnya, bukan
    // nama turunan `terap_bernama` yang pemakainya tidak pernah tulis.
    ("argumen bernama",
     "package main\nfn bagi(a, b) { return a / b }\n\
      pub fn main() { wok(bagi(a: 10, b: 2)) }",
     "argumen bernama"),

    // Kasus "method arr:: yang belum ada" dulu di sini; sejak Fase KECIL tidak ada lagi method
    // array beraritas benar yang ditolak, jadi kasusnya dicabut.
    ("fungsi pustaka sebagai nilai",
     "package main\npub fn main() { wok([1.0, 4.0].map(math::sqrt)) }",
     "dipakai sebagai NILAI"),

    // Konstruktor sebagai NILAI (Fase KONSTRUKTOR): jalan di dua mesin acuan, dan di sini
    // dulu dijawab "path `E::A` tidak dikenal" / "`Some` tidak ditemukan".
    // Fase KTOR-LLVM: enum NON-generik sudah jalan (PARITAS_LLVM); yang generik belum.
    ("konstruktor enum generik sebagai nilai: celah backend", r#"package main
enum G<T> { V(T), C }
pub fn main() {
    let f = G::V
    wok(f(3))
}"#, "belum mendukung konstruktor sebagai nilai"),
    ("Some sebagai nilai: celah backend", r#"package main
pub fn main() {
    let f = Some
    wok(f(3))
}"#, "belum mendukung konstruktor sebagai nilai"),
    // Fase SINKRON-LLVM: `Atomic` di backend ini 64 bit, dan luapannya ditolak saat jalan.
    ("atomic::add melewati 64 bit", "package main\npub fn main() { let a = Atomic::new(9223372036854775807)\n wok(atomic::add(a, 1)) }",
     "= 9223372036854775808 tidak muat di bilangan bulat 64 bit"),
];

// ─── Tabel 3: harus DITOLAK interpreter DAN LLVM
// ───────────────────────────

/// `(nama, program, kata-kunci-interpreter, kata-kunci-llvm)`
const DITOLAK_INTERP_DAN_LLVM: &[(&str, &str, &str, &str)] = &[
    // Fase KALIMAT: nilai tanpa padanan JSON, kalimat penyandi yang sama di tiga mesin.
    ("json::stringify atas atomic", "package main\npub fn main() {\n let a = Atomic::new(1)\n wok(json::stringify(a))\n}",
     "`atomic` tidak punya padanan di JSON, jadi ia tidak bisa disandikan", "`atomic` tidak punya padanan di JSON, jadi ia tidak bisa disandikan"),
    ("json::stringify atas mutex", "package main\npub fn main() {\n let a = Mutex::new()\n wok(json::stringify(a))\n}",
     "`mutex` tidak punya padanan di JSON, jadi ia tidak bisa disandikan", "`mutex` tidak punya padanan di JSON, jadi ia tidak bisa disandikan"),
    ("json::stringify atas channel", "package main\npub fn main() {\n let a = make_chan(int)\n wok(json::stringify(a))\n}",
     "`channel` tidak punya padanan di JSON, jadi ia tidak bisa disandikan", "`channel` tidak punya padanan di JSON, jadi ia tidak bisa disandikan"),
    ("json::stringify atas fungsi", "package main\npub fn main() {\n let a = |x| x + 1\n wok(json::stringify(a))\n}",
     "`fungsi` tidak punya padanan di JSON, jadi ia tidak bisa disandikan", "`fungsi` tidak punya padanan di JSON, jadi ia tidak bisa disandikan"),
    // Fase SEBAR: kalimat yang sama di tiga mesin.
    ("basis sebar membawa field liar ke struct",
     "package main\nstruct A { pub x: int, pub w: int }\nstruct B { pub x: int, pub y: int = 7 }\npub fn main() {\n let a = A { x: 1, w: 2 }\n wok(B { ...a })\n}", "basis `...` membawa field `w` yang tidak dideklarasikan struct `B`", "basis `...` membawa field `w` yang tidak dideklarasikan struct `B`"),
    ("varian unit telanjang milik dua enum", "package main\nenum E { A(int), K, V { a: int } }\nenum F { A(int), K, V { a: int } }\npub fn main() {\n wok(K)\n}",
     "`K` ambigu — varian bernama itu ada di E dan F", "`K` ambigu — varian bernama itu ada di E dan F"),
    // Fase WAJIB: kalimat field wajib sama di tiga mesin (`galat_index::field_wajib*`).
    ("literal struct tanpa field wajib",
     "package main\nstruct B { x: int, pub y: int }\npub fn main() { wok(1)\n let b = B { x: 1 }\n wok(b) }",
     "struct `B`: field `y` wajib diisi (tidak ada nilai default)", "struct `B`: field `y` wajib diisi (tidak ada nilai default)"),
    ("literal struct tanpa field wajib milik induk",
     "package main\nstruct A { pub x: int }\nstruct B: A { pub y: int }\npub fn main() { wok(B { y: 2 }) }",
     "struct `B`: field `x` wajib diisi (tidak ada nilai default)", "struct `B`: field `x` wajib diisi (tidak ada nilai default)"),
    ("varian ber-field tanpa salah satu field",
     "package main\nenum E { V { a: int, b: int }, W }\npub fn main() { wok(E::V { a: 1 }) }",
     "varian `E::V` butuh field `b`", "varian `E::V` butuh field `b`"),
    ("literal varian telanjang yang melewatkan field",
     "package main\nenum E { V { a: int, b: int }, K }\npub fn main() {\n wok(V { a: 1 })\n}",
     "varian `E::V` butuh field `b`", "varian `E::V` butuh field `b`"),
    // Fase SINKRON-LLVM: kalimat `Atomic`/`Mutex` sama dengan acuan (`sinkron::pesan_bukan*`).
    ("Atomic::new atas pecahan", "package main\npub fn main() { let a = Atomic::new(1.5) }",
     "`Atomic::new` butuh bilangan bulat, dapat `float` — Atomic cuma menyimpan `int`", "`Atomic::new` butuh bilangan bulat, dapat `float` — Atomic cuma menyimpan `int`"),
    ("atomic::add dengan pecahan", "package main\npub fn main() { let a = Atomic::new(1)\n atomic::add(a, 1.5) }",
     "`atomic::add` butuh bilangan bulat, dapat `float` — Atomic cuma menyimpan `int`", "`atomic::add` butuh bilangan bulat, dapat `float` — Atomic cuma menyimpan `int`"),
    ("atomic::get atas bukan Atomic", "package main\npub fn main() { wok(atomic::get(3)) }",
     "`atomic::get` butuh sebuah Atomic (buat dengan `Atomic::new`), dapat `int`", "`atomic::get` butuh sebuah Atomic (buat dengan `Atomic::new`), dapat `int`"),
    ("mutex::lock atas Atomic", "package main\npub fn main() { let a = Atomic::new(1)\n mutex::lock(a) }",
     "`mutex::lock` butuh sebuah Mutex (buat dengan `Mutex::new`), dapat `atomic`", "`mutex::lock` butuh sebuah Mutex (buat dengan `Mutex::new`), dapat `atomic`"),
    ("mutex::with_lock dengan bukan fungsi", "package main\npub fn main() { let m = Mutex::new()\n mutex::with_lock(m, 5) }",
     "nilai bertipe `int` bukan fungsi, jadi tidak bisa dipanggil", "nilai bertipe `int` bukan fungsi, jadi tidak bisa dipanggil"),
    ("Atomic tidak punya urutan", "package main\npub fn main() { let a = Atomic::new(1)\n wok(a < a) }",
     "tidak bisa membandingkan `atomic` dengan `atomic`", "tidak bisa membandingkan `atomic` dengan `atomic`"),
    // Fase SAPU-29: `rune`, `bool` dan teks dulu dibaca LLVM sebagai angkanya (21 dari 1090).
    ("jenis lain di tempat angka: arr::get (llvm)", "package main\npub fn main() { wok(arr::get([1, 2], 'a')) }",
     "arr::get: butuh bilangan bulat, dapat `rune`", "arr::get: butuh bilangan bulat, dapat `rune`"),
    ("jenis lain di tempat angka: math::gcd (llvm)", "package main\npub fn main() { wok(math::gcd('a', 1)) }",
     "butuh bilangan bulat, dapat `rune`", "butuh bilangan bulat, dapat `rune`"),
    ("jenis lain di tempat angka: math::lcm (llvm)", "package main\npub fn main() { wok(math::lcm(2, 'a')) }",
     "butuh bilangan bulat, dapat `rune`", "butuh bilangan bulat, dapat `rune`"),
    ("jenis lain di tempat angka: str::repeat (llvm)", "package main\npub fn main() { wok(str::repeat(\"ab\", 'a')) }",
     "butuh bilangan bulat, dapat `rune`", "butuh bilangan bulat, dapat `rune`"),
    ("jenis lain di tempat angka: future::timeout (llvm)", "package main\npub fn main() { wok(future::timeout(Future::new(|| 1), 'a')) }",
     "dapat `rune`", "dapat `rune`"),
    ("jenis lain di tempat angka: math::degrees bool (llvm)", "package main\npub fn main() { wok(math::degrees(true)) }",
     "math::degrees: butuh angka, dapat `bool`", "math::degrees: butuh angka, dapat `bool`"),
    ("jenis lain di tempat angka: math::log teks (llvm)", "package main\npub fn main() { wok(math::log(2, \"7\")) }",
     "math::log: butuh angka, dapat `string`", "math::log: butuh angka, dapat `string`"),
    ("jenis lain di tempat angka: math::is_finite teks (llvm)", "package main\npub fn main() { wok(math::is_finite(\"7\")) }",
     "math::is_finite: butuh angka, dapat `string`", "math::is_finite: butuh angka, dapat `string`"),
    // Tahap 5: keenam kasus ini dulu DITERIMA LLVM dengan jawaban karangan (`LLVM_LEBIH_PERMISIF`).
    ("pangkat bulat yang meluap (llvm)", "wok(2 ** 4294967296)\n",
     "kelewat besar buat integer", "kelewat besar buat integer"),
    ("alamat pointer negatif (llvm)", "package main\npub fn main() { wok((-1) as *const int) }",
     "angka negatif tidak punya padanan", "cast ke tipe pointer belum didukung"),
    ("import std/ yang tidak ada (llvm)", "package main\nimport \"std/hantu\"\npub fn main() { wok(1) }",
     "modul standar 'std/hantu' tidak ada", "modul standar 'std/hantu' tidak ada"),
    ("factorial yang meluap (llvm)", "package main\npub fn main() { wok(math::factorial(40)) }",
     "kelewat besar buat integer 128 bit", "kelewat besar buat integer 128 bit"),
    ("let atas varian unit yang tidak cocok (llvm)", "let None = Option::Some(3)\nwok(\"lolos\")\n",
     "tidak cocok dengan polanya", "tidak cocok dengan polanya"),
    ("tulis lewat key dict yang belum ada (llvm)",
     "package main\npub fn main() { var d = {\"a\": {\"b\": 1}}\n d.zz.b = 5\n wok(d) }",
     "key `zz` belum ada", "key `zz` belum ada"),
    // Fase LLVM-16: `coerce` dulu memotong pecahan dan membaca `bool` sebagai angka.
    ("indeks bool (llvm)", "package main\npub fn main() {\n  wok([1, 2][true])\n}",
     "tidak bisa index `array` dengan `bool`", "tidak bisa index `array` dengan `bool`"),
    ("indeks pecahan pada string (llvm)", "package main\npub fn main() {\n  wok(\"ab\"[1.0])\n}",
     "tidak bisa index `string` dengan `float`", "tidak bisa index `string` dengan `float`"),
    ("pecahan sebagai jumlah ulang (llvm)", "package main\npub fn main() {\n  wok(str::repeat(\"a\", 2.5))\n}",
     "butuh bilangan bulat, dapat pecahan", "tidak bisa dipakai sebagai `int` tanpa ditulis"),
    ("math::abs atas bool (llvm)", "package main\npub fn main() {\n  wok(math::abs(true))\n}",
     "math::abs: butuh angka, dapat `bool`", "math::abs: butuh angka, dapat `bool`"),
    // Fase SERPIH: `a[0:2] = …` dulu DITERIMA LLVM dan menulis ke salinan — `a` tetap.
    (
        "sasaran: potongan ditolak (llvm)",
        "package main\npub fn main() {\n  var a = [1, 2, 3]\n  a[0:2] = [9, 9]\n  wok(a)\n}",
        "potongan (`x[a:b]`) bukan sesuatu yang bisa di-assign",
        "potongan (`x[a:b]`) bukan sesuatu yang bisa di-assign",
    ),
    (
        "pembuang dibaca (llvm)",
        "package main\npub fn main() {\n  var _ = 1\n  _ = 2\n  wok(_)\n}",
        "`_` cuma untuk membuang nilai",
        "`_` cuma untuk membuang nilai",
    ),
    // Fase TERM: dulu `exit(2.5)` keluar 0 di acuan dan dikoersi ke i32 di LLVM.
    (
        "exit: kode bukan bilangan bulat ditolak (llvm)",
        "package main\npub fn main() {\n  exit(2.5)\n}",
        "kode keluar harus bilangan bulat 0 sampai 255, dapat `float`",
        "kode keluar harus bilangan bulat 0 sampai 255, dapat `float`",
    ),
    // Fase LITERAL: literal di atas `i128::MAX` ditolak PARSER, jadi tiga mesin satu
    // kalimat — dulu acuan membungkusnya jadi angka negatif tanpa sepatah kata.
    (
        "literal integer di luar 128 bit ditolak (llvm)",
        "package main\npub fn main() {\n  wok(170141183460469231731687303715884105728)\n}",
        "di luar jangkauan integer JRG",
        "di luar jangkauan integer JRG",
    ),
    // Fase BANDING-FN: dua nilai yang bisa dipanggil tidak bisa dibandingkan.
    (
        "banding dua fungsi ditolak (llvm)",
        "package main\nfn f(x: int) -> int { return x }\npub fn main() {\n  let a: fn(int) -> int = f\n  let b: fn(int) -> int = f\n  wok(a == b)\n}",
        "tidak bisa membandingkan",
        "tidak bisa membandingkan",
    ),
    (
        "banding dua fungsi: sebabnya disebut (llvm)",
        "package main\nfn f(x: int) -> int { return x }\npub fn main() {\n  let a: fn(int) -> int = f\n  let b: fn(int) -> int = f\n  wok(a != b)\n}",
        "tidak punya identitas",
        "tidak punya identitas",
    ),
    // Fase KURUNG: sebuah callable tanpa `()`. Backend ini dulu DIAM untuk
    // bentuk pernyataan dan menolak referensi method dengan kalimatnya sendiri.
    // Sekarang satu definisi untuk tiga mesin.
    (
        "fungsi sebagai pernyataan ditolak (llvm)",
        "package main\nfn k() { wok(\"EFEK\") }\npub fn main() {\n  k\n  wok(\"akhir\")\n}",
        "berdiri sendiri sebagai satu pernyataan",
        "berdiri sendiri sebagai satu pernyataan",
    ),
    (
        "referensi method tanpa kurung ditolak (llvm)",
        "package main\nstruct P { pub a: int }\nimpl P { fn dua(self) { return self.a * 2 } }\npub fn main() {\n  let p = P { a: 5 }\n  wok(typeof(p.dua))\n}",
        "itu method milik `P`, dan di sini ditulis tanpa `()`",
        "itu method milik `P`, dan di sini ditulis tanpa `()`",
    ),
    // Fase POLA-FIELD: backend ini sudah menolaknya sejak dulu, dengan kalimat
    // MILIKNYA sendiri — dua kalimat untuk satu kesalahan, di tabel yang tidak
    // bisa melihatnya. Sekarang satu definisi untuk tiga mesin.
    (
        "pola field yang tidak ada dijawab kalimat yang sama (llvm)",
        "package main\nstruct P { pub a: int, pub b: int }\npub fn main() {\n  let p = P { a: 1, b: 2 }\n  match p { P { a, zz } => wok(a), _ => wok(\"lain\") }\n}",
        "pola `P` menyebut field `zz` yang tidak ada",
        "pola `P` menyebut field `zz` yang tidak ada",
    ),
    (
        "salah ketik satu huruf disarankan di tiga mesin (llvm)",
        "package main\nstruct P { pub a: int, pub b: int }\npub fn main() {\n  let p = P { a: 1, b: 2 }\n  match p { P { ab } => wok(1), _ => wok(\"lain\") }\n}",
        "maksudnya `a`?", "maksudnya `a`?",
    ),
    // ── Fase MUDAH-2 sampai ke mesin KETIGA (Fase SAPU) ───────────────────
    (
        "null dijawab padanannya, bukan tebakan ejaan (llvm)",
        "package main\npub fn main() { wok(null) }",
        "di JRG namanya `nil`", "di JRG namanya `nil`",
    ),
    (
        "println dijawab padanannya (llvm)",
        "package main\npub fn main() { println(\"x\") }",
        "mencetak pakai `wok", "mencetak pakai `wok",
    ),
    (
        "console dijawab padanannya (llvm)",
        "package main\npub fn main() { console.log(1) }",
        "itu JavaScript", "itu JavaScript",
    ),
    // `"%d" % 5` — kebiasaan Python.
    (
        "modulo dengan string menyebut `%`, bukan koersi (llvm)",
        "package main\npub fn main() { wok(\"%d\" % 5) }",
        "di JRG `%` cuma sisa bagi", "di JRG `%` cuma sisa bagi",
    ),
    // Nama induk warisan yang tidak ada.
    (
        "induk warisan yang namanya tidak ada",
        "package main\nstruct Hewan { pub nama: string }\n\
         struct Anjing: Hewann { pub ras: string }\n\
         pub fn main() { wok(Anjing { nama: \"Bo\", ras: \"b\" }.ras) }",
        "mewarisi `Hewann`",
        "mewarisi `Hewann`",
    ),
    (
        "path method: penerima tipe lain (llvm)",
        "package main\nstruct P { pub n: int }\nstruct Q { pub n: int }\nimpl P { pub fn tambah(self, k: int) -> int { return self.n + k } }\npub fn main() { wok(P::tambah(Q{n:9}, 4)) }",
        "method milik",
        "method milik",
    ),
    // ── `str::format`: dua hasil salah yang diam, ditutup ─────────────────
    (
        "format: {0} ditolak dua-duanya",
        "package main\npub fn main() { wok(str::format(\"{0}\", 1)) }",
        "placeholder berposisi",
        "placeholder berposisi",
    ),
    (
        "format: argumen berlebih ditolak dua-duanya",
        "package main\npub fn main() { wok(str::format(\"{}\", 1, 2)) }",
        "argumen",
        "argumen",
    ),
    (
        "format: spec tak dikenal ditolak dua-duanya",
        "package main\npub fn main() { wok(str::format(\"{:>8}\", 1)) }",
        "tidak dikenal",
        "tidak dikenal",
    ),
    (
        "format: spec di nilai bukan angka ditolak dua-duanya",
        "package main\npub fn main() { wok(str::format(\"{:.2}\", \"teks\")) }",
        "cuma untuk angka",
        "cuma untuk angka",
    ),

    // `rune` sengaja BUKAN `is_int` di backend ini, supaya aritmetika rune
    // tetap ditolak seperti di acuannya. Kalau ia dijadikan int demi
    // memudahkan cast, `'a' + 1` akan diam-diam sah — dan mencetak karakter.
    (
        "aritmetika pada rune ditolak dua-duanya",
        r#"package main
pub fn main() {
    wok('a' + 1)
}"#,
        "tidak bisa `+`",
        "tidak berlaku untuk tipe `rune`",
    ),
    (
        "baca field private dari luar",
        r#"package main
struct Rahasia { nilai: int = 7 }
pub fn main() {
    var r: Rahasia = Rahasia {}
    wok(r.nilai)
}"#,
        "private",
        "private",
    ),
    // Kesetaraan agregat ditambahkan di fase ini; URUTAN sengaja TIDAK.
    (
        "tulis field private dari luar",
        r#"package main
struct Rahasia { nilai: int = 1 }
pub fn main() {
    var r: Rahasia = Rahasia {}
    r.nilai = 9
    wok(1)
}"#,
        "private",
        "private",
    ),
    // `[...5]` dulu dijawab LLVM dengan "`arr::concat` butuh array, diberi `i64`": nama turunan
    // yang tidak ditulis pemakai, dan nama tipe internal.
    (
        "sebar bukan array",
        "package main\npub fn main() { wok([...5]) }",
        "cuma bisa menyebar array — dapat `int`",
        "cuma bisa menyebar array — dapat `int`",
    ),
    // ── Nama yang bukan nilai, dan konstruktor (Fase KONSTRUKTOR) ─────────
    // `E` dan `PI` telanjang dulu konstanta matematika di backend ini saja, jadi
    // `enum E` yang dipakai sebagai nilai mencetak 2.718281828459045.
    (
        "nama enum sebagai nilai dijelaskan, bukan bilangan Euler (llvm)",
        "package main\nenum E { A(int), C }\npub fn main() { wok(E) }",
        "`E` itu nama enum (tipe)", "`E` itu nama enum (tipe)",
    ),
    (
        "PI telanjang ditolak seperti acuannya (llvm)",
        "package main\npub fn main() { wok(PI) }",
        "`PI` belum dikenal", "`PI` tidak ditemukan",
    ),
    (
        "nama struct sebagai nilai dijelaskan (llvm)",
        "package main\nstruct P { pub x: int }\npub fn main() { wok(P) }",
        "`P` itu nama struct (tipe)", "`P` itu nama struct (tipe)",
    ),
    (
        "varian ber-field sebagai nilai dijelaskan, bukan nil (llvm)",
        "package main\nenum E { V { x: int } }\npub fn main() { wok(E::V) }",
        "`E::V` itu varian ber-field", "`E::V` itu varian ber-field",
    ),
    (
        "aritas konstruktor satu kalimat (llvm)",
        "package main\nenum E { A(int), C }\npub fn main() { wok(E::A(1, 2)) }",
        "konstruktor `E::A` butuh 1 argumen, dapat 2", "konstruktor `E::A` butuh 1 argumen, dapat 2",
    ),
    // Fase JAWA-LLVM: LLVM dulu menerima kedua awalan untuk semua nama `jawa::`/`jawir::`.
    (
        "jawa::free bukan nama acuan (llvm)",
        "package main\npub fn main() { let p = jawa::alloc(1)\n jawa::free(p) }",
        "`jawa::free` tidak ditemukan", "`jawa::free` tidak ditemukan",
    ),
    (
        "task::await bukan nama acuan (llvm)",
        "package main\npub fn main() { let t = Task::spawn(|| 5)\n wok(task::await(t)) }",
        "`task::await` tidak ditemukan", "`task::await` tidak ditemukan",
    ),
    (
        "future::join bukan nama acuan (llvm)",
        "package main\npub fn main() { let f = Future::new(|| 5)\n wok(future::join(f)) }",
        "`future::join` tidak ditemukan", "`future::join` tidak ditemukan",
    ),
    (
        "jawir::alloc bukan nama acuan (llvm)",
        "package main\npub fn main() { let p = jawir::alloc(1) }",
        "`jawir::alloc` tidak ditemukan", "`jawir::alloc` tidak ditemukan",
    ),
    // Fase EJAAN-LLVM: dulu "`len` butuh 1 argumen", "konversi tipe butuh 1 argumen".
    (
        "builtin kelebihan argumen satu kalimat (llvm)",
        "package main\npub fn main() { wok(int(1, 9)) }",
        "`int` kebanyakan argumen: paling banyak 1, diberi 2",
        "`int` kebanyakan argumen: paling banyak 1, diberi 2",
    ),
    (
        "builtin kurang argumen satu kalimat (llvm)",
        "package main\npub fn main() { wok(str::upper()) }",
        "`str::upper` butuh 1 argumen, dapat 0", "`str::upper` butuh 1 argumen, dapat 0",
    ),
];

// ─── Tabel 4: divergensi yang DIKETAHUI dan tidak bisa ditutup
// ─────────────

// Program yang jalan di DUA mesin dengan jawaban berbeda, ditemukan lewat
// sapuan. Yang salah LLVM-nya (acuan sumber kebenaran); ditutup di fase LLVM.
const DIVERGENSI_LLVM: &[(&str, &str, &str, &str)] = &[
    // (`int` 64 bit yang membungkus, #346, ditutup di Fase LEBAR-LLVM.)
    // (Literal bulat yang menyesuaikan diri di tempat pecahan, ditutup di Fase KEPUTUSAN-1:
    // LLVM sekarang menolaknya, entrinya di `BELUM_DIDUKUNG_LLVM`.)
];

/// Ditolak acuan, DITERIMA LLVM dengan jawaban salah — cerminan `VM_LEBIH_PERMISIF`,
/// dari sapuan `DITOLAK_DUA_BACKEND` lewat LLVM. Kolom: nama, program, kunci pesan
/// acuan, keluaran LLVM.
const LLVM_LEBIH_PERMISIF: &[(&str, &str, &str, &str)] = &[
    // Kosong sejak tahap 5. Tabelnya dipertahankan untuk kasus berikutnya.
];

#[test]
fn llvm_lebih_permisif_masih_tercatat_akurat() {
    if lewati_tanpa_clang("llvm_lebih_permisif_masih_tercatat_akurat") { return; }
    let mut kabar = Vec::new();
    for (nama, src, kunci_acuan, keluaran_llvm) in LLVM_LEBIH_PERMISIF {
        let i = jalankan_interp(src);
        let l = jalankan_llvm(src);
        if i.sukses {
            kabar.push(format!("[{}] interpreter sekarang MENERIMA — premisnya hilang.", nama));
        } else if !i.galat.contains(kunci_acuan) {
            kabar.push(format!("[{}] interpreter menolak, tapi bukan karena `{}`:\n  {}",
                nama, kunci_acuan, i.galat.lines().next().unwrap_or("")));
        }
        if !l.sukses {
            kabar.push(format!("[{}] KABAR BAIK: LLVM sekarang menolak. Pindahkan kasusnya \
                ke `DITOLAK_INTERP_DAN_LLVM`.", nama));
        } else if l.keluaran != *keluaran_llvm {
            kabar.push(format!("[{}] LLVM sekarang menjawab {:?}, bukan {:?} — ukur ulang.",
                nama, l.keluaran, keluaran_llvm));
        }
    }
    assert!(kabar.is_empty(), "daftar `LLVM_LEBIH_PERMISIF` sudah tidak akurat:\n\n{}", kabar.join("\n\n"));
}

// ─── Tabel 5: pesan gagal SAAT JALAN harus sama ───────────────────────────

const GAGAL_SAAT_JALAN: &[(&str, &str, &str, &str)] = &[
    ("jawa::buffer_from_hex jumlah digit ganjil", "package main\npub fn main() { wok(\"sebelum\")\n wok(jawa::buffer_from_hex(\"abc\")) }",
     "sebelum", "jawa::buffer_from_hex: hex_decode: jumlah digitnya harus genap, dapat 3"),
    // Fase SEBAB: jalur yang tidak ada dulu menjawab `[]`, sama dengan direktori kosong.
    ("fs::walk atas jalur yang tidak ada", "package main\npub fn main() { wok(\"sebelum\")\n wok(fs::walk(\"/tidak/ada/jrg\")) }",
     "sebelum", "fs::walk '/tidak/ada/jrg': berkas atau direktorinya tidak ada"),
    ("fs::walk atas berkas", "package main\npub fn main() { fs::write(\"/tmp/jrg-walk-berkas.txt\", \"a\")\n wok(\"sebelum\")\n wok(fs::walk(\"/tmp/jrg-walk-berkas.txt\")) }",
     "sebelum", "fs::walk '/tmp/jrg-walk-berkas.txt': bukan direktori"),
    // Fase SINKRON-LLVM: kunci berpemilik, kalimatnya dari `src/sinkron.rs`.
    ("mutex: mengunci dua kali di goroutine yang sama", "package main\npub fn main() { wok(\"sebelum\")\n let m = Mutex::new()\n mutex::lock(m)\n mutex::lock(m) }",
     "sebelum", "mutex::lock: kunci ini sudah kamu pegang di goroutine yang sama"),
    ("mutex: with_lock lalu lock di dalamnya", "package main\npub fn main() { wok(\"sebelum\")\n let m = Mutex::new()\n mutex::with_lock(m, || { mutex::lock(m) }) }",
     "sebelum", "mutex::lock: kunci ini sudah kamu pegang di goroutine yang sama"),
    ("mutex: melepas yang tidak terkunci", "package main\npub fn main() { wok(\"sebelum\")\n let m = Mutex::new()\n mutex::unlock(m) }",
     "sebelum", "mutex::unlock: kunci ini tidak sedang terkunci — tiap `mutex::unlock` butuh satu `mutex::lock` sebelumnya"),
    ("mutex: ditinggal terkunci oleh goroutine yang selesai", "package main\npub fn main() { wok(\"sebelum\")\n let m = Mutex::new()\n let c = make_chan(int, 1)\n spawn { mutex::lock(m)\n c <- 1 }\n <-c\n time::sleep(0.05)\n mutex::lock(m) }",
     "sebelum", "mutex::lock: kunci ini ditinggal terkunci oleh goroutine yang sudah selesai"),
    ("mutex: melepas kunci milik goroutine lain", "package main\npub fn main() { wok(\"sebelum\")\n let m = Mutex::new()\n let c = make_chan(int, 1)\n spawn { mutex::lock(m)\n c <- 1\n time::sleep(0.2)\n mutex::unlock(m) }\n <-c\n mutex::unlock(m) }",
     "sebelum", "mutex::unlock: kunci ini dipegang goroutine lain — cuma yang mengunci yang boleh melepas"),
    // Fase TUNDA-LLVM: `defer` jalan saat galat membawa fungsinya keluar — bingkai terdalam
    // dulu, LIFO — dan keluarannya mendahului kalimat galat. Dulu LLVM langsung keluar.
    (
        "defer: jalan saat panik, bingkai terdalam dulu",
        "package main\nfn catat(s: string) { wok(\"tunda \" + s) }\n\
         fn dalam(a: []int, i: int) -> int { defer catat(\"d1\")\n defer catat(\"d2\")\n return a[i] }\n\
         fn luar() -> int { defer catat(\"luar\")\n let r = dalam([1, 2], 5)\n wok(\"tak sampai\")\n return r }\n\
         pub fn main() { defer catat(\"main\")\n wok(\"mulai\")\n wok(luar()) }",
        "mulai\ntunda d2\ntunda d1\ntunda luar\ntunda main",
        "index [5] di luar batas",
    ),
    (
        "defer: yang ikut gagal saat panik jadi catatan, sisanya tetap jalan",
        "package main\nfn gagal(s: string) { let a = [1]\n wok(\"coba \" + s)\n wok(a[3]) }\n\
         fn catat(s: string) { wok(\"tunda \" + s) }\n\
         fn g() -> int { defer catat(\"x\")\n defer gagal(\"y\")\n let a = [1]\n return a[9] }\n\
         pub fn main() { defer catat(\"main\")\n wok(g()) }",
        "coba y\ntunda x\ntunda main",
        "catatan: sebuah `defer` juga gagal saat membereskan: index [3] di luar batas",
    ),
    (
        "defer: yang gagal di jalur pulang biasa tidak menghentikan sisanya",
        "package main\nfn gagal(s: string) { let a = [1]\n wok(\"coba \" + s)\n wok(a[3]) }\n\
         fn catat(s: string) { wok(\"tunda \" + s) }\n\
         fn f() -> int { defer catat(\"a\")\n defer gagal(\"b\")\n defer catat(\"c\")\n return 7 }\n\
         pub fn main() { defer catat(\"main\")\n wok(f())\n wok(\"tak sampai\") }",
        "tunda c\ncoba b\ntunda a\ntunda main",
        "index [3] di luar batas",
    ),
    (
        "defer: fungsi yang dipanggil dibekukan saat didaftarkan, juga saat panik",
        "package main\nfn a() { wok(\"a\") }\nfn b() { wok(\"b\") }\n\
         pub fn main() { var f = a\n defer f()\n f = b\n let g = |s: string| wok(\"clo \" + s)\n defer g(\"1\")\n panik(\"sengaja\") }",
        "clo 1\na",
        "sengaja",
    ),
    (
        "defer: method, loop, dan assert yang gagal",
        "package main\nstruct B { pub nama: string }\nimpl B { fn tutup(self) { wok(\"tutup \" + self.nama) } }\n\
         fn catat(s: string, n: int) { wok(s + \" \" + str(n)) }\n\
         pub fn main() { let x = B { nama: \"a\" }\n defer x.tutup()\n for i in 0..2 { defer catat(\"putaran\", i) }\n assert(1 == 2, \"salah\") }",
        "putaran 1\nputaran 0\ntutup a",
        "salah",
    ),
    // Fase LAPIS: kalimat gagal builtin yang ditulis dalam JRG sama dengan kalimat acuannya.
    (
        "arr::chunks: ukuran nol",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(arr::chunks([1, 2, 3], 0)) }",
        "sebelum",
        "arr::chunks: ukuran harus lebih dari 0",
    ),
    (
        "arr::windows: ukuran negatif",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(arr::windows([1, 2, 3], 0 - 2)) }",
        "sebelum",
        "arr::windows: ukuran tidak boleh negatif, dapat -2",
    ),
    (
        "arr::insert: di luar batas",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(arr::insert([1, 2, 3], 5, 9)) }",
        "sebelum",
        "arr::insert: index 5 di luar batas (panjang 3)",
    ),
    (
        "arr::remove: di luar batas",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(arr::remove([1, 2, 3], 5)) }",
        "sebelum",
        "arr::remove: index 5 di luar batas (panjang 3)",
    ),
    (
        "arr::repeat: jumlah negatif",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(arr::repeat([1, 2], 0 - 1)) }",
        "sebelum",
        "arr::repeat: jumlah tidak boleh negatif, dapat -1",
    ),
    (
        "arr::max_by: array kosong",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(arr::max_by([1][1:], |x| x)) }",
        "sebelum",
        "arr::max_by: array kosong tidak punya nilai terbesar",
    ),
    (
        "arr::mean: array kosong",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(arr::mean([1][1:])) }",
        "sebelum",
        "arr::mean: array kosong tidak punya rata-rata",
    ),
    (
        "arr::max_by: kunci NaN",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(arr::max_by([1.0, math::NAN, 2.0], |x| x)) }",
        "sebelum",
        "arr::max_by: kunci bertipe `float` dan `float` tidak bisa dibandingkan, jadi urutannya tidak terdefinisi",
    ),
    (
        "arr::sort_by_key: kunci NaN",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(arr::sort_by_key([2.0, math::NAN, 1.0], |x| x)) }",
        "sebelum",
        "arr::sort_by_key: kunci bertipe `float` dan `float` tidak bisa dibandingkan, jadi urutannya tidak terdefinisi",
    ),
    (
        "arr::binary_search: mencari NaN",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(arr::binary_search([1.0, 2.0, 3.0], math::NAN)) }",
        "sebelum",
        "binary_search: `2.0` (tipe float) tidak bisa dibandingkan dengan yang dicari `NaN` (tipe float)",
    ),
    (
        "arr::median: memuat NaN",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(arr::median([1.0, math::NAN])) }",
        "sebelum",
        "arr::median: array mengandung NaN, urutannya tidak terdefinisi",
    ),
    (
        "arr::sort_desc: memuat NaN",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(arr::sort_desc([1.0, math::NAN])) }",
        "sebelum",
        "arr::sort_desc: tidak bisa membandingkan `float` dengan `float`, jadi urutannya tidak terdefinisi",
    ),
    // Fase EKOR: pad di lapis ketiga, dan bentuk method chunks/windows berkalimat sama.
    (
        "str::pad_left: isi dua karakter",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(str::pad_left(\"ab\", 5, \"xy\")) }",
        "sebelum",
        "str::pad_left: isinya harus tepat satu karakter, dapat 2 karakter",
    ),
    (
        "str::center: lebar negatif",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(str::center(\"ab\", 0 - 1)) }",
        "sebelum",
        "index tidak boleh negatif: -1",
    ),
    (
        "method chunks(0)",
        "package main\npub fn main() { wok(\"sebelum\")\n wok([1, 2, 3].chunks(0)) }",
        "sebelum",
        "arr::chunks: ukuran harus lebih dari 0",
    ),
    (
        "method windows(0)",
        "package main\npub fn main() { wok(\"sebelum\")\n wok([1, 2, 3].windows(0)) }",
        "sebelum",
        "arr::windows: ukuran harus lebih dari 0",
    ),
    // Fase KECIL: `rand::` dan `str::substr` di lapis ketiga.
    (
        "rand::int: batas terbalik",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(rand::int(5, 3)) }",
        "sebelum",
        "rand::int: hi (3) harus >= lo (5)",
    ),
    (
        "rand::choice: array kosong",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(rand::choice([1][1:])) }",
        "sebelum",
        "rand::choice: array kosong, tidak ada yang bisa dipilih",
    ),
    (
        "str::substr: awal di luar jangkauan",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(str::substr(\"halo\", 5)) }",
        "sebelum",
        "str::substr: posisi awal 5 di luar jangkauan — string ini cuma 4 karakter",
    ),
    (
        "str::substr: panjang negatif",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(str::substr(\"halo\", 1, 0 - 1)) }",
        "sebelum",
        "index tidak boleh negatif: -1",
    ),
    // Fase BERKAS: kalimat gagal `fs::`/`time::` dari lapis ketiga sama dengan acuannya.
    (
        "fs::read: berkas tidak ada",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(fs::read(\"/tidak/ada/jrg.txt\")) }",
        "sebelum",
        "fs::read '/tidak/ada/jrg.txt': berkas atau direktorinya tidak ada",
    ),
    (
        "fs::size: berkas tidak ada",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(fs::size(\"/tidak/ada/jrg.txt\")) }",
        "sebelum",
        "fs::size '/tidak/ada/jrg.txt': berkas atau direktorinya tidak ada",
    ),
    (
        "fs::write: folder tidak ada",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(fs::write(\"/tidak/ada/jrg.txt\", \"x\")) }",
        "sebelum",
        "fs::write '/tidak/ada/jrg.txt': berkas atau direktorinya tidak ada",
    ),
    (
        "time::format: arahan tak dikenal",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(time::format(0, \"%q\")) }",
        "sebelum",
        "time::format: arahan `%q` tidak dikenal.",
    ),
    (
        "time::sleep: durasi tak masuk akal",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(time::sleep(99999999999)) }",
        "sebelum",
        "time::sleep: durasi `99999999999` detik tidak masuk akal",
    ),
    // Fase JSON-LLVM: pengurai dan penyandi yang sama (`json_inti`) di runtime LLVM.
    (
        "json::parse: `tidak` bukan `true`",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(json::parse(\"tidak\")) }",
        "sebelum",
        "json::parse: harusnya `true` di posisi 0, yang tertulis `tidak`",
    ),
    (
        "json::parse: objek tak tertutup",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(json::parse(\"{\")) }",
        "sebelum",
        "json::parse: objek tidak ditutup dengan `}`",
    ),
    (
        "json::stringify: NaN",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(json::stringify([1.5, math::NAN])) }",
        "sebelum",
        "`NaN` tidak punya padanan di JSON, jadi ia tidak bisa disandikan",
    ),
    (
        "json::pretty: lebar di luar jangkauan",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(json::pretty([1], 0 - 1)) }",
        "sebelum",
        "json::pretty: lebar indentasi -1 di luar jangkauan 0 sampai 16",
    ),
    // Fase KORUTIN: galat channel dan serah-sekali Future/Task, kalimat yang sama dengan acuan.
    (
        "channel: kirim ke yang sudah ditutup",
        "package main\npub fn main() { wok(\"sebelum\")\n let c = make_chan(int, 1)\n c.close()\n c <- 3 }",
        "sebelum",
        "channel sudah ditutup: tidak bisa kirim lagi",
    ),
    (
        "channel: terima dari yang tertutup dan kosong",
        "package main\npub fn main() { wok(\"sebelum\")\n let c = make_chan(int, 1)\n c.close()\n wok(<-c) }",
        "sebelum",
        "channel sudah ditutup dan kosong: tidak akan ada nilai lagi",
    ),
    (
        "task::join dua kali",
        "package main\nfn kerja() -> int { return 42 }\npub fn main() { let t = Task::spawn(kerja)\n wok(\"sebelum\")\n let a = task::join(t)\n wok(task::join(t) + a) }",
        "sebelum",
        "task::join: hasilnya sudah diambil sebelumnya",
    ),
    (
        "future::try_get sesudah await",
        "package main\nfn kerja() -> int { return 42 }\npub fn main() { let f = Future::new(kerja)\n wok(\"sebelum\")\n let a = future::await(f)\n wok(future::try_get(f), a) }",
        "sebelum",
        "future::try_get: hasilnya sudah diambil sebelumnya",
    ),
    (
        "jawa::array_pop atas larik kosong",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(jawa::array_pop(jawa::array(0))) }",
        "sebelum",
        "jawa::array_pop: array kosong",
    ),
    (
        "jawa::buffer_fill di luar byte",
        "package main\npub fn main() { wok(\"sebelum\")\n jawa::buffer_fill(jawa::buffer(2), 256) }",
        "sebelum",
        "jawa::buffer_fill: nilai 256 di luar jangkauan byte (0..255)",
    ),
    (
        "jawa:: dengan handle jenis lain",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(jawa::buffer_to_hex(jawa::array(1))) }",
        "sebelum",
        "jawa::buffer_to_hex: handle ini hasil `jawa::array(...)`, bukan `jawa::buffer(...)`",
    ),
    // Badan Future/Task yang gagal: galatnya muncul di titik pengambilan hasil, bukan mematikan program.
    (
        "future::await atas tugas yang gagal",
        "package main\nfn bagi(a: int, b: int) -> int {\n defer wok(\"defer bagi\")\n return a / b }\npub fn main() { let g = Future::new(bagi, 6, 3)\n wok(future::await(g))\n let f = Future::new(bagi, 6, 0)\n time::sleep(0.05)\n wok(\"sebelum\")\n wok(future::await(f)) }",
        "defer bagi\n2\ndefer bagi\nsebelum",
        "Future gagal: `6 / 0`",
    ),
    (
        "task::join atas tugas yang gagal",
        "package main\nfn bagi(a: int, b: int) -> int {\n defer wok(\"defer bagi\")\n return a / b }\npub fn main() { let t = Task::spawn(bagi, 6, 0)\n time::sleep(0.05)\n wok(\"sebelum\", future::is_done(t))\n wok(task::join(t)) }",
        "defer bagi\nsebelum true",
        "Task gagal: `6 / 0`",
    ),
    (
        "future::all memuat tugas yang gagal",
        "package main\nfn bagi(a: int, b: int) -> int {\n defer wok(\"defer bagi\")\n return a / b }\npub fn main() { wok(\"sebelum\")\n wok(future::all([Future::new(bagi, 6, 2), Future::new(bagi, 6, 0)])) }",
        "sebelum\ndefer bagi\ndefer bagi",
        "Future gagal: `6 / 0`",
    ),
    (
        "future::timeout atas tugas yang gagal",
        "package main\nfn bagi(a: int, b: int) -> int {\n defer wok(\"defer bagi\")\n return a / b }\npub fn main() { let f = Future::new(bagi, 6, 0)\n time::sleep(0.05)\n wok(\"sebelum\")\n wok(future::timeout(f, 100)) }",
        "defer bagi\nsebelum",
        "Future gagal: `6 / 0`",
    ),
    (
        "future::race atas tugas yang gagal",
        "package main\nfn bagi(a: int, b: int) -> int {\n defer wok(\"defer bagi\")\n return a / b }\npub fn main() { wok(\"sebelum\")\n wok(future::race([Future::new(bagi, 6, 0)])) }",
        "sebelum\ndefer bagi",
        "Future gagal: `6 / 0`",
    ),
    // Fase UMUM: NaN di dalam array membatalkan pembandingan urutannya, seperti di dua mesin lain.
    (
        "urutan array: elemen NaN tidak terbanding",
        "package main\npub fn main() { wok(\"sebelum\")\n wok([math::NAN] < [1.0]) }",
        "sebelum",
        "tidak bisa membandingkan `float` dengan `float`",
    ),
    // `str::char_at` di luar batas dulu DIJAWAB LLVM dengan karakter nol, tanpa pesan.
    (
        "str::char_at: indeks di luar batas",
        "package main\npub fn main() { let s = \"h\u{e9}\u{e9}-abc\"\n let n = len(s) + 3\n wok(str::char_at(s, 2), s.char_at(4))\n wok(str::char_at(s, n)) }",
        "\u{e9} a",
        "str::char_at: index 10 di luar batas (panjang 7)",
    ),
    (
        "str::char_at: indeks negatif dari belakang, lalu di luar batas",
        "package main\npub fn main() { let s = \"abc\"\n let n = 0 - len(s) - 1\n wok(str::char_at(s, n + 1))\n wok(str::char_at(s, n)) }",
        "a",
        "str::char_at: index -4 di luar batas (panjang 3)",
    ),
    // Fase GENERIK: `arr::swap` di LLVM, dengan kalimat acuan untuk dua kegagalannya.
    (
        "arr::swap: indeks di luar batas",
        "package main\npub fn main() { let a = [1, 2, 3]\n let n = len(a) * 3\n wok(\"sebelum\")\n wok(arr::swap(a, 0, n)) }",
        "sebelum",
        "arr::swap: index 9 di luar batas (panjang 3)",
    ),
    (
        "arr::swap: indeks negatif dari belakang, lalu di luar batas",
        "package main\npub fn main() { let a = [1, 2, 3]\n let n = 0 - len(a) - 1\n wok(arr::swap(a, n + 1, -1))\n wok(arr::swap(a, n, 0)) }",
        "[3, 2, 1]",
        "arr::swap: index -4 di luar batas (panjang 3)",
    ),
    // Fase SISA: LLVM dulu menjawab `NaN` (max) dan `1.0` (min) tanpa pesan.
    (
        "arr::max atas pecahan yang memuat NaN",
        "package main\npub fn main() { let n = math::NAN\n wok(arr::max([n]))\n wok(arr::max([1.0, n])) }",
        "NaN",
        "arr::max: `float` dan `float` tidak bisa dibandingkan, jadi tidak ada yang bisa disebut paling kecil atau paling besar",
    ),
    (
        "arr::min atas pecahan yang memuat NaN",
        "package main\npub fn main() { let n = math::NAN\n wok(arr::min([2.0, 1.0]))\n wok(arr::min([n, 1.0, 2.5])) }",
        "1.0",
        "arr::min: `float` dan `float` tidak bisa dibandingkan, jadi tidak ada yang bisa disebut paling kecil atau paling besar",
    ),
    // Fase PETA: LLVM dulu MENERIMA dan menjawab `[1.0, 2.5, NaN]`.
    (
        "arr::sort atas pecahan yang memuat NaN",
        "package main\npub fn main() { let n = 0.0 / 0.0\n wok(\"sebelum\")\n wok(arr::sort([2.5, n, 1.0])) }",
        "sebelum",
        "arr::sort: tidak bisa membandingkan `float` dengan `float`, jadi urutannya tidak terdefinisi",
    ),
    // Fase PETA: campuran golongan di array bertag ditolak dengan kalimat acuan.
    (
        "arr::sort atas elemen bertag yang tidak saling terbanding",
        "package main\nstruct K { pub isi: array }\npub fn main() { wok(\"sebelum\")\n wok(arr::sort(K { isi: [1, \"a\"] }.isi)) }",
        "sebelum",
        "arr::sort: tidak bisa membandingkan `string` dengan `int`, jadi urutannya tidak terdefinisi",
    ),
    // Fase SEPARUH: jumlah negatif ditolak saat jalan, dengan kalimat acuan.
    (
        "arr::take: jumlah negatif, sesudah keluaran sebelumnya",
        "package main\npub fn main() { let a = [1, 2]\n wok(arr::take(a, 1))\n let n = 0 - len(a)\n wok(arr::take(a, n)) }",
        "[1]",
        "arr::take: jumlah tidak boleh negatif, dapat -2",
    ),
    // Fase SIMPUL-1: `panik` dulu "fungsi tidak ditemukan" di LLVM.
    (
        "panik: menghentikan program dengan pesannya, sesudah keluaran sebelumnya",
        "package main\npub fn main() { wok(\"sebelum\")\n panik(\"rusak: \" + str(3)) }",
        "sebelum",
        "rusak: 3",
    ),
    // Fase JAGA-STACK: dulu SIGSEGV dan core dump tanpa sepatah kata. Dua cabang, supaya
    // pengoptimal tidak bisa mengubah rekursinya jadi perulangan.
    (
        "rekursi tak berujung: stack habis dilaporkan, bukan SIGSEGV",
        "package main\nfn f(n: int) -> int { return f(n + 1) + f(n + 2) }\npub fn main() { wok(\"mulai\")\n wok(f(0)) }",
        "mulai",
        "rekursi terlalu dalam",
    ),
    // Fase TERM: di luar 0–255 cuma 8 bit terbawahnya yang sampai — dulu 256 jadi 0.
    (
        "exit: kode di luar 0-255 gagal saat jalan, bukan dipotong",
        r#"package main
fn kode() -> int { return 256 }
pub fn main() {
    wok("sebelum")
    exit(kode())
}"#,
        "sebelum",
        "kode keluar harus 0 sampai 255, dapat 256",
    ),
    // `jawa::` mensimulasikan malloc/free, jadi use-after-free dan double-free
    // adalah KEGAGALAN yang harus dilaporkan — bukan perilaku tak terdefinisi.
    (
        "jawa: use-after-free",
        r#"package main
pub fn main() {
    let p = jawa::alloc(1)
    wok("sebelum")
    jawir::free(p)
    wok(jawa::get(p))
}"#,
        "sebelum",
        "use-after-free — pointer ini sudah di-`jawir::free`",
    ),
    (
        "jawa: double free",
        r#"package main
pub fn main() {
    let p = jawa::alloc(1)
    wok("sebelum")
    jawir::free(p)
    jawir::free(p)
}"#,
        "sebelum",
        "double free — pointer ini sudah pernah di-`jawir::free` sebelumnya",
    ),

    (
        "index array di luar batas",
        r#"package main
pub fn main() {
    wok("sebelum")
    var a: [4]int = [1, 2, 3, 4]
    wok(a[9])
}"#,
        "sebelum",
        "index [9] di luar batas — array ini isinya 4 elemen (index valid: 0..3, atau -1..-4",
    ),
    (
        "index array negatif di luar batas",
        r#"package main
pub fn main() {
    wok("sebelum")
    var a: [4]int = [1, 2, 3, 4]
    wok(a[-9])
}"#,
        "sebelum",
        "index [-9] di luar batas — array ini isinya 4 elemen",
    ),
    // Satuan yang disebut untuk string harus `karakter`, bukan `elemen` —
    // yang dihitung memang karakter, bukan byte, dan pembaca yang bingung
    // soal `s[9]` justru perlu tahu itu.
    (
        "index string di luar batas menyebut karakter",
        r#"package main
pub fn main() {
    wok("sebelum")
    var s: string = "halo"
    wok(s[9])
}"#,
        "sebelum",
        "index [9] di luar batas — string ini isinya 4 karakter",
    ),
    (
        "bagi nol",
        r#"package main
pub fn main() {
    wok("sebelum")
    var n: int = 0
    wok(1 / n)
}"#,
        "sebelum",
        "`1 / 0` — nggak bisa bagi dengan nol",
    ),
    (
        "sisa bagi nol punya kalimatnya sendiri",
        r#"package main
pub fn main() {
    wok("sebelum")
    var n: int = 0
    wok(-7 % n)
}"#,
        "sebelum",
        "`-7 % 0` — nggak bisa cari sisa bagi dengan nol",
    ),
    (
        "geser: jumlah negatif",
        r#"package main
pub fn main() {
    wok("sebelum")
    var n: int = -1
    wok(8 >> n)
}"#,
        "sebelum",
        "`8 >> -1` — jumlah geseran tidak boleh negatif",
    ),
    (
        "geser: 128 kali atau lebih",
        r#"package main
pub fn main() {
    wok("sebelum")
    var n: int = 200
    wok(8 << n)
}"#,
        "sebelum",
        "`8 << 200` — integer JRG cuma 128 bit",
    ),
];

// ─── Tabel 6: berkas konformans yang LULUS lewat LLVM ─────────────────────

const KONFORMANS_LLVM: &[&str] = &[
    "closures",
    // Dua yang terakhir masuk, dan keduanya butuh NILAI BERTAG — satu-satunya
    // hal yang benar-benar menghalangi sesudah delapan celah lain ditutup.
    "collections",
    "control_flow",
    "functions",
    "indexing",
    "memori_manual",
    "pattern_matching",
    "semantik_cow",
    "semantik_nilai",
    "strings",
    "values_and_operators",
];

// ─── Tes ───────────────────────────────────────────────────────────────────

/// Pesan lewat kalau `clang` tidak ada — dicetak, tidak disembunyikan.
fn lewati_tanpa_clang(nama: &str) -> bool {
    if ada_clang() { return false; }
    eprintln!(
        "LEWAT: {} — `clang` tidak ada di PATH, jadi backend LLVM tidak bisa \
         menghasilkan native code. Set JRGC_CLANG kalau namanya lain.",
        nama
    );
    true
}

/// Divergensi yang diketahui harus MASIH ada.
#[test]
fn divergensi_llvm_yang_diketahui_masih_ada() {
    if lewati_tanpa_clang("divergensi_llvm_yang_diketahui_masih_ada") { return; }

    let mut kabar = Vec::new();
    for (nama, src, keluaran_acuan, keluaran_llvm) in DIVERGENSI_LLVM {
        let i = jalankan_interp(src);
        let l = jalankan_llvm(src);
        if !i.sukses || !l.sukses {
            kabar.push(format!(
                "[{}] salah satu backend sekarang GAGAL, bukan sekadar berbeda — \
                 kasusnya berubah sifat, periksa lagi.",
                nama
            ));
            continue;
        }
        if i.keluaran != *keluaran_acuan {
            kabar.push(format!(
                "[{}] interpreter sekarang menjawab {:?}, bukan {:?} — semantik \
                 acuannya berubah, jadi catatan di tabel ini sudah usang.",
                nama, i.keluaran, keluaran_acuan
            ));
        }
        if l.keluaran == i.keluaran {
            kabar.push(format!(
                "[{}] KABAR BAIK: dua-duanya menjawab {:?} sekarang. Pindahkan \
                 kasusnya ke `PARITAS_LLVM` dan hapus dari sini.",
                nama, i.keluaran
            ));
        } else if l.keluaran != *keluaran_llvm {
            kabar.push(format!(
                "[{}] LLVM sekarang menjawab {:?}, bukan {:?} — masih berbeda, \
                 tapi berbeda dengan cara lain. Ukur ulang sebelum mempercayai \
                 catatannya.",
                nama, l.keluaran, keluaran_llvm
            ));
        }
    }
    assert!(kabar.is_empty(), "daftar `DIVERGENSI_LLVM` sudah tidak akurat:\n\n{}", kabar.join("\n\n"));

}

/// Program yang gagal SAAT JALAN: keluaran sebelum gagal sama, dan pesan
/// gagalnya menyebut hal yang sama.
#[test]
fn pesan_gagal_saat_jalan_sama() {
    if lewati_tanpa_clang("pesan_gagal_saat_jalan_sama") { return; }

    let mut gagal = Vec::new();
    for (nama, src, keluaran, memuat) in GAGAL_SAAT_JALAN {
        let i = jalankan_interp(src);
        let l = jalankan_llvm(src);

        for (label, h) in [("interpreter", &i), ("llvm", &l)] {
            if h.sukses {
                gagal.push(format!("[{}] {} TIDAK gagal — kasusnya sudah usang?", nama, label));
                continue;
            }
            if h.keluaran != *keluaran {
                gagal.push(format!(
                    "[{}] {} mencetak {:?} sebelum gagal, diharap {:?} — waktu gagalnya beda",
                    nama, label, h.keluaran, keluaran
                ));
            }
            if !h.galat.contains(memuat) {
                gagal.push(format!(
                    "[{}] pesan {} tidak memuat `{}`\n  dapat: {}",
                    nama, label, memuat,
                    h.galat.lines().take(2).collect::<Vec<_>>().join(" | ")
                ));
            }
        }
    }
    assert!(gagal.is_empty(), "{} masalah pada pesan gagal saat jalan:\n\n{}", gagal.len(), gagal.join("\n\n"));
}

// ─── Pintu konversi: satu aturan di tiga mesin ────────────────────────────

/// (pintu, nilai) yang GAGAL di acuan — tiap kelas kegagalan dari tiap keluarga
/// pintu. Dulu LLVM menjawab `1` untuk `NaN as int`, `44` untuk `300 as u8`,
/// dan baris kosong untuk `(-1) as rune`.
const KONVERSI_GAGAL: &[(&str, &str)] = &[
    ("x as int", "0.0 / 0.0"), ("x as int", "1.0 / 0.0"), ("x as i8", "-1.0 / 0.0"),
    ("x as u8", "1e40"), ("x as int", "-1e40"), ("x as u8", "-5.9"), ("x as i8", "300.5"),
    ("x as u64", "18446744073709551616.0"), ("x as i64", "9223372036854775808.0"),
    ("x as u8", "300"), ("x as byte", "-1"), ("x as i32", "5000000000"), ("x as uint", "-1"),
    ("x as i16", "-32769"), ("x as u8", "'Ā'"), ("x as rune", "-1"), ("x as rune", "55296"),
    ("x as rune", "1114112"), ("x as rune", "5.9"), ("x as rune", "\"a\""),
    ("x as rune", "true"), ("x as int", "nil"), ("x as float", "nil"), ("x as float", "'a'"),
    ("x as int", "[1, 2]"), ("x as int", "\"abc\""), ("x as int", "\"\""),
    ("x as int", "\"1.5\""), ("x as u8", "\"300\""), ("x as u16", "\" -3 \""),
    ("x as int", "\"999999999999999999999999999999999999999999\""),
    ("x as float", "\"abc\""), ("x as float", "\"  \""),
    ("int(x)", "0.0 / 0.0"), ("uint(x)", "-5"), ("rune(x)", "-1"), ("float(x)", "\"x\""),
    ("parse_int(x)", "5.9"), ("str::to_int(x)", "true"), ("str::to_int(x)", "\"1.5\""),
    ("str::to_int(x)", "\"\""), ("str::to_int(x)", "'A'"), ("str::to_float(x)", "\"abc\""),
    ("parse_float(x)", "'A'"), ("x.to_int()", "\"x\""), ("x.to_float()", "\"\""),
];

/// Program yang BERHASIL di tiap pintu, termasuk nilai bertag — sisi positifnya,
/// karena penolakan atas segalanya juga membuat daftar di atas hijau.
const KONVERSI_BERHASIL: &str = r#"package main
pub fn main() {
    let f = 5.9
    let n = -5.9
    wok(f as int, n as int, f as u8, n as i8, int(f), uint(f))
    wok(300 as i32, 65 as u8, 65 as byte, 70000 as rune, rune(300), -128 as i8, 255 as u8)
    wok("5" as int, " 42 " as u8, "  -7" as i8, "1.5" as float, float("2"), float(3), float(true))
    wok(true as u8, false as byte, true as i8, true as float, int(true), uint(true))
    wok('A' as u8, 'A' as int, 'A' as i16, 9223372036854775807 as i64)
    wok(-9223372036854775808.0 as i64, -0.9 as u8, -0.0 as u64, 255.9 as u8)
    wok(parse_int("12"), str::to_int(" -3 "), parse_float("1.5"), str::to_float(2))
    wok("12".to_int(), " 1.5 ".to_float(), str::to_int(7))
    let u: u64 = 9223372036854775807
    wok(u as int)
    let b: i128 = 100000000000000000000
    wok(b as i128, b as u128, "99999999999999999999" as i128)
    var x = 1
    x = 2.5
    wok(x as int, int(x))
    x = "7"
    wok(x as int, x as float, str::to_int(x))
    wok((1.1 as f32) == 1.1, 16777217 as f32)
}"#;

/// Baris galat pertama tanpa awalan `error: ` — LLVM mencetak kalimatnya saja.
fn baris_galat(galat: &str) -> String {
    let b = galat.lines().next().unwrap_or("").trim();
    b.strip_prefix("error: ").unwrap_or(b).to_string()
}

/// Tiap pintu konversi menjawab SAMA dengan acuan di backend LLVM, dan yang
/// gagal gagal dengan kalimat yang sama, sesudah keluaran yang sama.
#[test]
fn pintu_konversi_sama_di_llvm() {
    if lewati_tanpa_clang("pintu_konversi_sama_di_llvm") { return; }

    let mut gagal = Vec::new();
    for (pintu, nilai) in KONVERSI_GAGAL {
        let src = format!(
            "package main\npub fn main() {{\n    var x = {}\n    wok(\"mulai\")\n    wok({})\n}}\n",
            nilai, pintu);
        let i = jalankan_interp(&src);
        let l = jalankan_llvm(&src);
        let nama = format!("{} dengan x = {}", pintu, nilai);
        if i.sukses {
            gagal.push(format!("[{}] acuan TIDAK gagal — kasusnya usang?", nama));
            continue;
        }
        if l.sukses {
            gagal.push(format!("[{}] LLVM menjawab {:?}, acuan menolak: {}",
                nama, l.keluaran, baris_galat(&i.galat)));
            continue;
        }
        if i.keluaran != "mulai" || l.keluaran != "mulai" {
            gagal.push(format!("[{}] waktu gagalnya beda: acuan {:?}, llvm {:?}",
                nama, i.keluaran, l.keluaran));
        }
        let (bi, bl) = (baris_galat(&i.galat), baris_galat(&l.galat));
        if bi != bl {
            gagal.push(format!("[{}] kalimatnya beda\n  acuan: {}\n  llvm : {}", nama, bi, bl));
        }
    }

    let i = jalankan_interp(KONVERSI_BERHASIL);
    let l = jalankan_llvm(KONVERSI_BERHASIL);
    if !i.sukses || !l.sukses || i.keluaran != l.keluaran {
        gagal.push(format!(
            "[yang berhasil] keluarannya beda\n  acuan: {:?} {}\n  llvm : {:?} {}",
            i.keluaran, baris_galat(&i.galat), l.keluaran, baris_galat(&l.galat)));
    }
    assert!(gagal.is_empty(), "{} masalah di pintu konversi LLVM:\n\n{}",
        gagal.len(), gagal.join("\n\n"));
}

/// **`exit` di LLVM: kode 0–255 sampai ke pemanggil apa adanya, dan di luar itu gagal saat
/// jalan dengan kalimat acuannya.** Dulu `exit(256)` keluar 0 — sukses — di sana.
#[test]
fn kode_keluar_sama_di_llvm() {
    if lewati_tanpa_clang("kode_keluar_sama_di_llvm") { return; }
    let mut salah = Vec::new();
    for kode in [0, 7, 255] {
        let src = format!(
            "package main\npub fn main() {{\n    wok(\"mulai\")\n    exit({})\n    wok(\"lewat\")\n}}\n", kode);
        let (i, l) = (jalankan_interp(&src), jalankan_llvm(&src));
        if i.kode != Some(kode) || l.kode != Some(kode) || i.keluaran != "mulai" || l.keluaran != "mulai" {
            salah.push(format!("[exit({})] acuan {:?} {:?}, llvm {:?} {:?} {}",
                kode, i.kode, i.keluaran, l.kode, l.keluaran, baris_galat(&l.galat)));
        }
    }
    for nilai in ["256", "0 - 1", "70000", "4294967296"] {
        let src = format!(
            "package main\nfn kode() -> int {{ return {} }}\npub fn main() {{\n    wok(\"mulai\")\n    exit(kode())\n}}\n",
            nilai);
        let (i, l) = (jalankan_interp(&src), jalankan_llvm(&src));
        let (bi, bl) = (baris_galat(&i.galat), baris_galat(&l.galat));
        if i.sukses || l.sukses || i.keluaran != "mulai" || l.keluaran != "mulai" || bi != bl
            || !bi.contains("kode keluar harus 0 sampai 255") {
            salah.push(format!("[exit({})]\n  acuan: {:?} {}\n  llvm : {:?} {}",
                nilai, i.keluaran, bi, l.keluaran, bl));
        }
    }
    assert!(salah.is_empty(), "{} masalah kode keluar di LLVM:\n\n{}", salah.len(), salah.join("\n\n"));
}

/// `int`/`uint` di backend ini 64 bit, di acuan 128 bit. Angka di antaranya
/// DITOLAK dengan sebabnya — dulu dipotong diam-diam.
#[test]
fn int_64_bit_menolak_bukan_memotong() {
    if lewati_tanpa_clang("int_64_bit_menolak_bukan_memotong") { return; }

    let kasus: &[(&str, &str, &str)] = &[
        ("x as int", "1e20", "tulis `i128`"),
        ("int(x)", "\"99999999999999999999\"", "tulis `i128`"),
        ("x as uint", "1e20", "tulis `u128`"),
        ("parse_int(x)", "\"99999999999999999999\"", "str::to_int: "),
    ];
    let mut gagal = Vec::new();
    for (pintu, nilai, memuat) in kasus {
        let src = format!(
            "package main\npub fn main() {{\n    var x = {}\n    wok(\"mulai\")\n    wok({})\n}}\n",
            nilai, pintu);
        let i = jalankan_interp(&src);
        let l = jalankan_llvm(&src);
        let nama = format!("{} dengan x = {}", pintu, nilai);
        if !i.sukses { gagal.push(format!("[{}] acuan gagal: {}", nama, baris_galat(&i.galat))); }
        if l.sukses {
            gagal.push(format!("[{}] LLVM menjawab {:?} — angka di luar 64 bit dijawab, \
                                bukan ditolak", nama, l.keluaran));
            continue;
        }
        let bl = baris_galat(&l.galat);
        if l.keluaran != "mulai" || !bl.contains("backend LLVM") || !bl.contains(memuat) {
            gagal.push(format!("[{}] penolakannya tidak menyebut sebabnya: {:?} | {}",
                nama, l.keluaran, bl));
        }
    }
    assert!(gagal.is_empty(), "{}", gagal.join("\n\n"));
}

/// Berkas konformans yang lulus lewat LLVM harus TETAP lulus.
#[test]
fn konformans_yang_lulus_lewat_llvm_tetap_lulus() {
    if lewati_tanpa_clang("konformans_yang_lulus_lewat_llvm_tetap_lulus") { return; }

    let akar = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conformance");
    let mut gagal = Vec::new();
    let mut kabar_baik = Vec::new();

    let mut semua: Vec<String> = std::fs::read_dir(&akar)
        .expect("baca tests/conformance")
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("jrg") {
                p.file_stem().and_then(|x| x.to_str()).map(|s| s.to_string())
            } else { None }
        })
        .collect();
    semua.sort();

    for nama in semua {
        let src = std::fs::read_to_string(akar.join(format!("{}.jrg", nama)))
            .expect("baca berkas konformans");
        let i = jalankan_interp(&src);
        let l = jalankan_llvm(&src);
        let lulus = l.sukses && i.sukses && i.keluaran == l.keluaran;

        if KONFORMANS_LLVM.contains(&nama.as_str()) {
            if !lulus {
                gagal.push(format!(
                    "[{}] TERDAFTAR lulus tapi sekarang tidak.\n  interp: {}\n  llvm  : {}",
                    nama,
                    i.keluaran.lines().last().unwrap_or("(kosong)"),
                    if l.sukses { l.keluaran.lines().last().unwrap_or("(kosong)").to_string() }
                    else { l.galat.lines().next().unwrap_or("").to_string() }
                ));
            }
        } else if lulus {
            kabar_baik.push(nama.clone());
        }
    }

    assert!(
        gagal.is_empty(),
        "{} berkas konformans mundur:\n\n{}",
        gagal.len(), gagal.join("\n\n")
    );
    assert!(
        kabar_baik.is_empty(),
        "KABAR BAIK: {} berkas konformans sekarang lulus lewat LLVM dan belum \
         terdaftar — tambahkan ke `KONFORMANS_LLVM` supaya tidak bisa mundur \
         diam-diam: {}",
        kabar_baik.len(), kabar_baik.join(", ")
    );
}

#[test]
fn paritas_interpreter_dan_llvm() {
    if lewati_tanpa_clang("paritas_interpreter_dan_llvm") { return; }

    let mut gagal = Vec::new();
    for (nama, src) in PARITAS_LLVM {
        let i = jalankan_interp(src);
        let l = jalankan_llvm(src);

        if !i.sukses {
            gagal.push(format!(
                "[{}] program uji GAGAL di interpreter — kasusnya salah tulis, \
                 bukan divergensi backend.\n  stderr: {}",
                nama, i.galat.lines().take(3).collect::<Vec<_>>().join(" | ")
            ));
            continue;
        }
        if !l.sukses {
            gagal.push(format!(
                "[{}] gagal di --emit-llvm sementara interpreter berhasil.\n  \
                 interpreter: {:?}\n  stderr llvm: {}",
                nama, i.keluaran,
                l.galat.lines().take(4).collect::<Vec<_>>().join(" | ")
            ));
            continue;
        }
        if i.keluaran != l.keluaran {
            gagal.push(format!(
                "[{}] keluaran BERBEDA — ini divergensi semantik, bukan gap fitur\n  \
                 interpreter: {:?}\n  llvm       : {:?}",
                nama, i.keluaran, l.keluaran
            ));
        }
    }

    assert!(
        gagal.is_empty(),
        "{} dari {} kasus paritas LLVM tidak cocok:\n\n{}\n\n\
         Interpreter adalah acuan semantik: kalau LLVM berbeda, LLVM yang salah.",
        gagal.len(), PARITAS_LLVM.len(), gagal.join("\n\n")
    );
}

/// Tiap `pub fn ns_nama` di `lapis.jrg` harus melayani builtin `ns::nama` yang memang ada.
#[test]
fn tiap_fungsi_lapis_bernama_builtin_yang_ada() {
    let daftar = Command::new(jrgc()).arg("--daftar-builtin").output().expect("jrgc jalan");
    let daftar = String::from_utf8_lossy(&daftar.stdout).to_string();
    let ada: Vec<&str> = daftar.lines().collect();
    let mut salah = Vec::new();
    let mut jumlah = 0;
    for baris in include_str!("../src/llvm/lapis.jrg").lines() {
        let Some(sisa) = baris.strip_prefix("pub fn ") else { continue };
        let nama: String = sisa.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        let Some((ns, item)) = nama.split_once('_') else { salah.push(nama); continue };
        jumlah += 1;
        if !ada.contains(&format!("{}::{}", ns, item).as_str()) { salah.push(nama); }
    }
    assert!(jumlah >= 40, "lapis.jrg cuma memuat {} fungsi publik — berkasnya terbaca?", jumlah);
    assert!(salah.is_empty(), "fungsi lapis tanpa builtin bernama sama: {:?}", salah);
}

/// Runtime LLVM memakai SALINAN `json_inti.rs`; dua berkas itu harus sama bita demi bita.
#[test]
fn salinan_json_inti_di_runtime_identik() {
    assert!(include_str!("../src/json_inti.rs") == include_str!("../runtime/src/json_inti.rs"),
        "runtime/src/json_inti.rs berbeda dari src/json_inti.rs — salin ulang: cp src/json_inti.rs runtime/src/");
}

#[test]
fn gap_llvm_ditolak_dengan_alasan_sebenarnya() {
    let mut gagal = Vec::new();
    for (nama, src, alasan) in BELUM_DIDUKUNG_LLVM {
        // Tidak perlu `clang`: yang diuji penolakan di tahap codegen, sebelum
        // ada IR yang diserahkan ke clang.
        let l = jalankan_llvm(src);
        if l.sukses {
            gagal.push(format!(
                "[{}] KABAR BAIK: sekarang berhasil di LLVM. Pindahkan kasusnya \
                 ke `PARITAS_LLVM` supaya keluarannya ikut dibandingkan.",
                nama
            ));
            continue;
        }
        if !l.galat.contains(alasan) {
            gagal.push(format!(
                "[{}] ditolak, tapi pesannya tidak menyebut `{}` — penolakan yang \
                 tidak menjelaskan sebabnya sama membingungkannya dengan salah \
                 jawab.\n  stderr: {}",
                nama, alasan,
                l.galat.lines().take(4).collect::<Vec<_>>().join(" | ")
            ));
        }
    }
    assert!(
        gagal.is_empty(),
        "{} dari {} gap LLVM tidak terlaporkan sebagaimana mestinya:\n\n{}",
        gagal.len(), BELUM_DIDUKUNG_LLVM.len(), gagal.join("\n\n")
    );
}

/// Program yang HARUS ditolak dua-duanya, tetap ditolak dua-duanya.
#[test]
fn penolakan_bersama_interpreter_dan_llvm() {
    let mut gagal = Vec::new();
    for (nama, src, alasan_i, alasan_l) in DITOLAK_INTERP_DAN_LLVM {
        let i = jalankan_interp(src);
        let l = jalankan_llvm(src);

        if i.sukses {
            gagal.push(format!(
                "[{}] interpreter MENERIMANYA — premis kasusnya hilang; acuan \
                 semantiknya yang berubah, jadi periksa dulu apakah itu memang \
                 disengaja.",
                nama
            ));
        } else if !i.galat.contains(alasan_i) {
            gagal.push(format!(
                "[{}] interpreter menolak tapi bukan karena `{}`\n  stderr: {}",
                nama, alasan_i, i.galat.lines().take(3).collect::<Vec<_>>().join(" | ")
            ));
        }

        if l.sukses {
            gagal.push(format!(
                "[{}] LLVM MENERIMANYA sementara interpreter menolak — divergensi \
                 semantik, bukan gap fitur.",
                nama
            ));
        } else if !l.galat.contains(alasan_l) {
            gagal.push(format!(
                "[{}] LLVM menolak tapi pesannya tidak menyebut `{}` — penolakan \
                 yang tidak menjelaskan sebabnya sama membingungkannya dengan \
                 salah jawab.\n  stderr: {}",
                nama, alasan_l, l.galat.lines().take(3).collect::<Vec<_>>().join(" | ")
            ));
        }
    }
    assert!(
        gagal.is_empty(),
        "{} masalah pada penolakan bersama:\n\n{}",
        gagal.len(), gagal.join("\n\n")
    );
}

// ─── Jenis tidak berganti diam-diam (Fase JENIS-LLVM) ─────────────────────

/// Pasangan (jenis tempat, jenis nilai) yang dulu dijawab LLVM dengan jenis LAIN tanpa pesan:
/// rune jadi angka, angka jadi `true`, `nil` jadi `false`, bulat jadi pecahan.
const PASANGAN_JENIS: &[(&str, &str, &str, &str)] = &[
    ("int", "7", "rune", "'r'"), ("rune", "'r'", "int", "7"),
    ("bool", "true", "int", "7"), ("bool", "true", "float", "2.5"),
    ("bool", "true", "string", "\"s\""), ("bool", "true", "rune", "'r'"),
    ("bool", "true", "nil", "nil"), ("float", "2.5", "int", "7"),
];

fn posisi_jenis(ta: &str, a: &str, b: &str) -> Vec<(&'static str, String)> {
    vec![
        ("literal array", format!("pub fn main() {{ let x = [{a}, {b}]\n wok(x, typeof(x[0]), typeof(x[1])) }}")),
        ("ekor if", format!("pub fn main() {{ let c = false\n let x = if c {{ {a} }} else {{ {b} }}\n wok(x, typeof(x)) }}")),
        ("ternary", format!("pub fn main() {{ let c = false\n let x = c $ {a} : {b}\n wok(x, typeof(x)) }}")),
        ("lengan match", format!("pub fn main() {{ let n = 2\n let x = match n {{ 1 => {a}, _ => {b} }}\n wok(x, typeof(x)) }}")),
        ("arr::push", format!("pub fn main() {{ let x = arr::push([{a}], {b})\n wok(x, typeof(x[1])) }}")),
        ("tulis elemen", format!("pub fn main() {{ var x = [{a}]\n x[0] = {b}\n wok(x, typeof(x[0])) }}")),
        ("closure", format!("pub fn main() {{ let f = |c: bool| if c {{ {a} }} else {{ {b} }}\n let x = f(false)\n wok(x, typeof(x)) }}")),
        ("get_or", format!("pub fn main() {{ let x = [{a}].get_or(5, {b})\n wok(x, typeof(x)) }}")),
        ("let beranotasi", format!("pub fn main() {{ let x: {ta} = {b}\n wok(x, typeof(x)) }}")),
        ("parameter beranotasi", format!("fn f(x: {ta}) {{ wok(x, typeof(x)) }}\npub fn main() {{ f({b}) }}")),
        ("return beranotasi", format!("fn f() -> {ta} {{ return {b} }}\npub fn main() {{ let x = f()\n wok(x, typeof(x)) }}")),
        ("field beranotasi", format!("struct S {{ pub x: {ta} }}\npub fn main() {{ let s = S {{ x: {b} }}\n wok(s.x, typeof(s.x)) }}")),
    ]
}

/// Nilai yang jenisnya lain dari tempatnya: LLVM boleh menolak dengan lantang atau menjawab
/// persis seperti acuan — tidak pernah menjawab dengan jenis lain. Dulu 136 dari 870 begitu.
#[test]
fn jenis_tidak_berganti_diam_diam_di_llvm() {
    if lewati_tanpa_clang("jenis_tidak_berganti_diam_diam_di_llvm") { return; }
    let mut masalah = Vec::new();
    for (ta, a, tb, b) in PASANGAN_JENIS {
        for (posisi, badan) in posisi_jenis(ta, a, b) {
            let src = format!("package main\n{}", badan);
            let i = jalankan_interp(&src);
            let l = jalankan_llvm(&src);
            if !l.sukses {
                if i.sukses && !l.galat.contains("masuk ke tempat yang bertipe") && !l.galat.contains("tidak bisa") {
                    masalah.push(format!("[{posisi}: {tb} ke {ta}] LLVM menolak tanpa menyebut jenisnya:\n  {}",
                        l.galat.lines().next().unwrap_or("")));
                }
                continue;
            }
            if !i.sukses {
                masalah.push(format!("[{posisi}: {tb} ke {ta}] acuan menolak, LLVM menjawab {:?}", l.keluaran));
            } else if i.keluaran != l.keluaran {
                masalah.push(format!("[{posisi}: {tb} ke {ta}] jenis berganti diam-diam\n  acuan: {:?}\n  llvm:  {:?}",
                    i.keluaran, l.keluaran));
            }
        }
    }
    assert!(masalah.is_empty(), "{} masalah jenis di LLVM:\n\n{}", masalah.len(), masalah.join("\n\n"));
}

