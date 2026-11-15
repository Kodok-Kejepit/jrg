// tests/llvm_parity.rs

mod bersama;
use std::process::Command;

/// Tiap biner `--emit-llvm` dari tes ini berbatas `JRGC_BATAS_MEMORI` (bawaan 2048 MiB; `=` kosong mematikannya),
/// supaya kasus yang rakus gagal "memori habis" alih-alih menghabiskan memori mesin.
fn jrgc() -> &'static str {
    static PASANG: std::sync::Once = std::sync::Once::new();
    PASANG.call_once(|| if std::env::var_os("JRGC_BATAS_MEMORI").is_none() { std::env::set_var("JRGC_BATAS_MEMORI", "2048") });
    env!("CARGO_BIN_EXE_jrgc")
}

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

/// `zip::` (Fase AMAN-15) sama di tiga mesin: buat, daftar, baca, bongkar; bongkar ulang tidak menimpa,
/// dan arsip zip-slip buatan `zipfile` Python ditolak tanpa satu berkas pun tertulis — juga yang sah di dalamnya.
#[test]
fn zip_sama_di_tiga_mesin_dan_tidak_menulis_ke_luar() {
    if !ada_clang() { eprintln!("lewati: clang tidak ada"); return; }
    const SLIP: &str = "UEsDBBQAAAAAAAAAIQBH3dx5AgAAAAIAAAAIAAAAYW1hbi50eHRva1BLAwQUAAAAAAAAACEAzH1w+wUAAAAFAAAAEAAAAC4uLy4uL2tlbHVhci50eHRqYWhhdFBLAQIUAxQAAAAAAAAAIQBH3dx5AgAAAAIAAAAIAAAAAAAAAAAAAACkgQAAAABhbWFuLnR4dFBLAQIUAxQAAAAAAAAAIQDMfXD7BQAAAAUAAAAQAAAAAAAAAAAAAACkgSgAAAAuLi8uLi9rZWx1YXIudHh0UEsFBgAAAAACAAIAdAAAAFsAAAAAAA==";
    let jalan = |mesin: &str, f: fn(&str) -> Hasil| {
        let d = dir_uji(&format!("zip-{}", mesin));
        let src = format!(r#"package main
pub fn main() {{
    let d = "{d}"
    wok(zip::create(d + "/a.zip", {{"baca.md": "halo\n" + str::repeat("ulang ", 50), "data/": "", "data/b.bin": [0, 1, 255]}}))
    for _, e in zip::list(d + "/a.zip")? {{ wok(e.nama, e.ukuran, e.folder, e.mampat <= e.ukuran) }}
    wok(zip::read(d + "/a.zip", "data/b.bin"), str::len(zip::read_text(d + "/a.zip", "baca.md")?))
    wok(zip::read_text(d + "/a.zip", "data/b.bin"))
    wok(zip::extract(d + "/a.zip", d + "/k"), fs::read(d + "/k/baca.md") == zip::read_text(d + "/a.zip", "baca.md")?)
    wok(zip::extract(d + "/a.zip", d + "/k"))
    wok(zip::create(d + "/b.zip", {{"../luar": "x"}}))
    fs::write_base64(d + "/slip.zip", "{SLIP}")?
    wok(zip::extract(d + "/slip.zip", d + "/s"), fs::exists(d + "/s/aman.txt"))
}}
"#, d = d.display());
        let h = f(&src);
        let tak_tertulis = !d.join("../keluar.txt").exists() && !d.join("keluar.txt").exists();
        let _ = std::fs::remove_dir_all(&d);
        (h.sukses, h.galat, h.keluaran.replace(&d.display().to_string(), "D"), tak_tertulis)
    };
    let (i, v, l) = (jalan("i", jalankan_interp), jalan("v", jalankan_vm), jalan("l", jalankan_llvm));
    assert!(i.0, "acuan gagal:\n{}", i.1);
    for kata in ["Result::Ok(3)", "data/b.bin 3 false true", "[0, 1, 255]", "bukan teks UTF-8",
                 "`baca.md`: sudah ada (0 berkas", "zip-slip", "Result::Ok(2) true"] {
        assert!(i.2.contains(kata), "keluaran acuan tidak memuat `{}`:\n{}", kata, i.2);
    }
    assert!(i.2.contains("../../keluar.txt") && i.2.contains("false"), "{}", i.2);
    assert_eq!(i.2, v.2, "acuan dan --vm beda");
    assert!(l.0, "LLVM gagal:\n{}", l.1);
    assert_eq!(i.2, l.2, "acuan dan LLVM beda");
    assert!(i.3 && v.3 && l.3, "arsip zip-slip menulis ke luar foldernya");
}

/// `tar::` (Fase AMAN-22) sama di tiga mesin: buat `.tar.gz`, daftar, baca, bongkar; arsip tar-slip dan arsip
/// bertautan simbolik buatan `tarfile` Python ditolak tanpa satu berkas pun tertulis; `tar` sistem membaca buatan kita.
#[test]
fn tar_sama_di_tiga_mesin_dan_tidak_menulis_ke_luar() {
    if !ada_clang() { eprintln!("lewati: clang tidak ada"); return; }
    const SLIP: &str = "H4sIAAAAAAAC/+3UPQrDMAyGYR2lJ3Cc4LTn0RAodX/AlaHHr5Kp7dIphpD3QSChRdMnvek92MtkRdEdU1q6++1u+Jjn/Sn1gxyiNFCfpsVPyj49smDHQui88nStWtb6A//zP37nv4/kv5GLntWIAQAAAAAAAAAAAAAAwGa9AeFNbOwAKAAA";
    const SYM: &str = "H4sIAAAAAAAC/+3RMQrCQBCG0TlKTqBxWdzzLGIhSArdQI7vYhlso0jea/7ppvhanVudYlNjd875vd16P9ylpBJDOl7bJTY2P1t99JexT/d6mw5tab/tf1r3z2mM4StNdt5/CQAAAAAAAAAA4F+9AAlcC88AKAAA";
    let jalan = |mesin: &str, f: fn(&str) -> Hasil| {
        let d = dir_uji(&format!("tar-{}", mesin));
        let src = format!(r#"package main
pub fn main() {{
    let d = "{d}"
    let a = d + "/a.tar.gz"
    wok(tar::create(a, {{"baca.md": "halo\n" + str::repeat("ulang ", 50), "data/": "", "data/b.bin": [0, 1, 255]}}))
    for _, e in tar::list(a)? {{ wok(e.nama, e.ukuran, e.folder, e.jenis) }}
    wok(tar::read(a, "data/b.bin"), str::len(tar::read_text(a, "baca.md")?))
    wok(tar::read_text(a, "data/b.bin"))
    wok(tar::extract(a, d + "/k"), fs::read(d + "/k/baca.md") == tar::read_text(a, "baca.md")?)
    wok(tar::extract(a, d + "/k"))
    wok(tar::create(d + "/b.tar", {{"../luar": "x"}}))
    fs::write_base64(d + "/slip.tgz", "{SLIP}")?
    wok(tar::extract(d + "/slip.tgz", d + "/s"), fs::exists(d + "/s/aman.txt"))
    fs::write_base64(d + "/sym.tgz", "{SYM}")?
    for _, e in tar::list(d + "/sym.tgz")? {{ wok(e) }}
    wok(tar::extract(d + "/sym.tgz", d + "/t"), fs::exists(d + "/t/lain.txt"))
}}
"#, d = d.display());
        let h = f(&src);
        let tak_tertulis = !d.join("../keluar.txt").exists() && !d.join("keluar.txt").exists();
        // `tar` sistem membaca arsip buatan kita dengan isi yang sama.
        let sistem = std::fs::create_dir_all(d.join("x")).is_ok() && Command::new("tar")
            .args(["-xzf", "a.tar.gz", "-C", "x"]).current_dir(&d).status().map_or(true, |s| s.success()
                && std::fs::read(d.join("x/data/b.bin")).ok() == Some(vec![0, 1, 255]));
        let _ = std::fs::remove_dir_all(&d);
        (h.sukses, h.galat, h.keluaran.replace(&d.display().to_string(), "D"), tak_tertulis && sistem)
    };
    let (i, v, l) = (jalan("i", jalankan_interp), jalan("v", jalankan_vm), jalan("l", jalankan_llvm));
    assert!(i.0, "acuan gagal:\n{}", i.1);
    for kata in ["Result::Ok(3)", "data/b.bin 3 false berkas", "data/ 0 true folder", "[0, 1, 255]", "bukan teks UTF-8",
                 "`baca.md`: sudah ada (0 berkas", "Result::Ok(2) true", "../../keluar.txt", "symlink ke `/etc`",
                 "tautan: /etc"] {
        assert!(i.2.contains(kata), "keluaran acuan tidak memuat `{}`:\n{}", kata, i.2);
    }
    assert_eq!(i.2.matches(") false").count(), 2, "arsip jahat menulis sebagian:\n{}", i.2);
    assert_eq!(i.2, v.2, "acuan dan --vm beda");
    assert!(l.0, "LLVM gagal:\n{}", l.1);
    assert_eq!(i.2, l.2, "acuan dan LLVM beda");
    assert!(i.3 && v.3 && l.3, "arsip tar-slip menulis ke luar foldernya, atau `tar` sistem menolak buatan kita");
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

/// Galat saat jalan keluar dengan kode yang sama di interpreter dan LLVM (dulu 101 di LLVM).
#[test]
fn kode_keluar_galat_sama_dengan_acuan() {
    if !ada_clang() { eprintln!("lewati: clang tidak ada"); return; }
    for src in [
        "package main\npub fn main() { let a = [1]\n wok(a[5]) }",
        "package main\npub fn main() { panik(\"x\") }",
        "package main\nfn f(n: int) -> int { return f(n + 1) + 1 }\npub fn main() { wok(f(0)) }",
        "package main\npub fn main() { let x: Option<int> = None\n wok(x.unwrap()) }",
        "package main\npub fn main() { assert(1 == 2) }",
    ] {
        let (i, l) = (jalankan_interp(src), jalankan_llvm(src));
        assert!(i.kode == Some(1) && l.kode == i.kode, "{}: interpreter {:?}, LLVM {:?}\n{}", src, i.kode, l.kode, l.galat);
    }
}

/// `main -> int`: nilainya kode keluar di TIGA mesin (Fase KODE-MAIN), dengan aturan `exit` (di luar 0–255 gagal
/// sesudah `defer`); anotasi lain ditolak satu kalimat; tanpa anotasi nilainya dibuang, tapi `Err`/`None` dilaporkan.
#[test]
fn main_bulat_jadi_kode_keluar_di_tiga_mesin() {
    if !ada_clang() { eprintln!("lewati: clang tidak ada"); return; }
    for (src, kode, keluar, galat) in [
        ("package main\npub fn main() -> int { wok(\"a\")\n 3 }", 3, "a", ""),
        // Tanpa anotasi nilai ekornya dibuang — `fs::delete(f)` di baris terakhir bukan kode keluar.
        ("package main\npub fn main() { let x = 5\n x }", 0, "", ""),
        ("package main\npub fn main() { wok(1)\n [1, 2] }", 0, "1", ""),
        ("package main\npub fn main() { let r: Result<int, string> = Err(\"x\")\n r }", 1, "", "`Result::Err` merambat keluar dari `main`"),
        ("package main\npub fn main() -> u8 { return 9 }", 9, "", ""),
        ("package main\npub fn main() -> Option<int> { Some(3) }", 0, "", ""),
        ("package main\npub fn main() -> int { defer wok(\"tunda\")\n 256 }", 1, "tunda", "`main` mengembalikan 256 — kode keluar harus 0 sampai 255"),
        ("package main\npub fn main() -> int { -1 }", 1, "", "`main` mengembalikan -1"),
        ("package main\npub fn main() -> [2]int { [1, 2] }", 1, "", "`main` cuma boleh mengembalikan bilangan bulat 0 sampai 255"),
        ("package main\npub fn main() -> string { \"x\" }", 1, "", "bukan `string`"),
    ] {
        let hasil = [("interpreter", jalankan_interp(src)), ("--vm", jalankan_vm(src)), ("LLVM", jalankan_llvm(src))];
        for (mesin, h) in &hasil {
            assert!(h.kode == Some(kode) && h.keluaran == keluar && h.galat.contains(galat),
                "{} ({}): kode {:?}, keluaran {:?}\n{}", src, mesin, h.kode, h.keluaran, h.galat);
        }
        assert_eq!(hasil[0].1.galat, hasil[1].1.galat, "{}: stderr interpreter lawan --vm", src);
    }
}

/// Rekursi tak berujung lewat callback HOF (rekursi RUST, bukan bingkai JRG) dilaporkan oleh penjaga sisa stack
/// di dua mesin acuan, bukan mematikan proses (Fase REKURSI-LEGA: batas hitungan 10 000 dibuka).
#[test]
fn rekursi_lewat_callback_dilaporkan_bukan_crash() {
    let src = "package main\nfn g(n: int) -> int { return [n].map(|x| g(x + 1))[0] }\npub fn main() { wok(\"m\")\n wok(g(0)) }";
    for (mesin, h) in [("interpreter", jalankan_interp(src)), ("--vm", jalankan_vm(src))] {
        assert!(h.kode == Some(1) && h.keluaran == "m" && h.galat.contains("rekursi terlalu dalam di fungsi"),
            "{}: kode {:?}, keluaran {:?}\n{}", mesin, h.kode, h.keluaran, h.galat);
    }
}

/// Biner yang mati oleh sinyal disebut sinyalnya dan keluar 128+N (#876); dulu kode 1 tanpa pesan.
#[test]
fn biner_yang_mati_oleh_sinyal_disebut() {
    if !ada_clang() { eprintln!("lewati: clang tidak ada"); return; }
    let l = jalankan_llvm("package main\nextern \"C\" {\n fn abort()\n}\npub fn main() { wok(\"sebelum\")\n abort() }");
    assert_eq!((l.keluaran.as_str(), l.kode), ("sebelum", Some(134)), "{}", l.galat);
    assert!(l.galat.contains("berhenti oleh sinyal 6 (SIGABRT)"), "{}", l.galat);
}

/// `JRGC_BATAS_MEMORI` membatasi biner yang dijalankan `--emit-llvm`: program rakus berhenti "memori habis"
/// sesudah keluarannya, dan nilai yang bukan bilangan MiB positif ditolak sebelum apa pun jalan.
#[test]
fn batas_memori_biner_llvm_ditegakkan() {
    if !ada_clang() { eprintln!("lewati: clang tidak ada"); return; }
    let dir = dir_uji("batas");
    let file = dir.join("uji.jrg");
    std::fs::write(&file, "package main\npub fn main() { wok(\"mulai\")\n var s = \"x\"\n for i in 0..40 { s = s + s }\n wok(len(s)) }")
        .expect("tulis sumber");
    let jalan = |batas: &str| {
        let out = Command::new(jrgc()).arg(&file).arg("--emit-llvm").arg("-o").arg(dir.join("uji.bin"))
            .env("JRG_SOPAN", "1").env("JRGC_BATAS_MEMORI", batas).output().expect("jalankan jrgc");
        (String::from_utf8_lossy(&out.stdout).trim().to_string(), String::from_utf8_lossy(&out.stderr).to_string(),
         out.status.code())
    };
    let (k, g, kode) = jalan("300");
    assert!(k == "mulai" && kode == Some(1) && g.contains("memori habis"), "{:?} {:?}\n{}", k, kode, g);
    for salah in ["0", "abc", "-5", "1.5"] {
        let (k, g, kode) = jalan(salah);
        assert!(k.is_empty() && kode != Some(0) && g.contains("JRGC_BATAS_MEMORI harus bilangan MiB"), "{}: {:?}\n{}", salah, k, g);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--uji` di `--emit-llvm` melapor persis seperti interpreter: baris hasil, `(baris N)` dari situs galat (tanpa
/// baris untuk galat di modul lain), `defer` sebelum laporannya, filter, berkas tanpa tes, kode keluar.
#[test]
fn uji_di_llvm_seperti_acuan() {
    if !ada_clang() { eprintln!("lewati: clang tidak ada"); return; }
    let dir = dir_uji("ujillvm");
    let tes = "package main\nimport \"std/base32\"\nfn dua(x) { return x * 2 }\nfn main() { wok(\"main tidak jalan\") }\n\
        fn test_lulus() { assert_eq(dua(2), 4) }\nfn test_gagal() {\n wok(\"sebelum\")\n assert_eq(dua(2), 5)\n}\n\
        fn uji_panik() { panik(\"meledak\") }\nfn test_indeks() {\n let a = [1, 2]\n wok(a[5])\n}\n\
        fn test_defer() {\n defer wok(\"defer jalan\")\n assert(false)\n}\n\
        fn test_goroutine() {\n let c = make_chan(int, 1)\n spawn { c <- 42 }\n assert_eq(<-c, 42)\n}\n\
        fn test_modul() { base32::encode(3.5) }\nfn test_kembali() { return 7 }\nfn test_akhir() { wok(\"akhir\") }\n";
    std::fs::write(dir.join("t.jrg"), tes).expect("tulis sumber");
    std::fs::write(dir.join("kosong.jrg"), "package main\nfn main() { wok(1) }\n").expect("tulis sumber");
    let jalan = |berkas: &str, llvm: bool, ekstra: &[&str]| {
        let mut c = Command::new(jrgc());
        c.arg(dir.join(berkas)).arg("--uji").args(ekstra).env("JRG_SOPAN", "1");
        if llvm { c.arg("--emit-llvm").arg("-o").arg(dir.join("u.bin")); }
        let o = c.output().expect("jalankan jrgc");
        (String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string(), o.status.code())
    };
    for (berkas, ekstra) in [("t.jrg", &[][..]), ("t.jrg", &["--uji-filter", "lulus"][..]), ("kosong.jrg", &[][..])] {
        let (a, l) = (jalan(berkas, false, ekstra), jalan(berkas, true, ekstra));
        assert_eq!(a, l, "{} {:?}: interpreter (kiri) lawan --emit-llvm (kanan)", berkas, ekstra);
    }
    let (k, _, kode) = jalan("t.jrg", false, &[]);
    assert!(kode == Some(1) && k.contains("  GAGAL test_gagal (baris 8) — assert_eq gagal")
        && k.contains("defer jalan\n  GAGAL test_defer (baris 17)") && k.contains("  GAGAL test_modul — base32::encode")
        && !k.contains("main tidak jalan") && k.ends_with("--- 9 test: 4 lulus, 5 gagal ---\n"), "{}", k);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Kebuntuan yang terlihat dari goroutine `spawn` atau badan Future mengakhiri programnya SEKALI.
/// Dulu goroutine itu saja yang tamat, lalu kebuntuan yang sama ditemukan lagi sampai stack habis.
#[test]
fn kebuntuan_dari_goroutine_dilaporkan_sekali() {
    if !ada_clang() { eprintln!("lewati: clang tidak ada"); return; }
    for src in [
        "package main\npub fn main() { let m = Mutex::new()\n let c = make_chan(int)\n m.lock()\n spawn { m.lock()\n c <- 1 }\n wok(\"mulai\")\n wok(<-c) }",
        "package main\npub fn main() { let m = Mutex::new()\n m.lock()\n let f = Future::new(|| { m.lock()\n 1 })\n wok(\"mulai\")\n wok(future::await(f)) }",
    ] {
        let l = jalankan_llvm(src);
        assert!(!l.sukses && l.keluaran == "mulai", "{}\n{}", src, l.galat);
        assert_eq!(l.galat.matches("kebuntuan: semua goroutine sedang menunggu channel atau kunci").count(), 1, "{}\n{}", src, l.galat);
        assert!(!l.galat.contains("goroutine error") && !l.galat.contains("rekursi"), "{}\n{}", src, l.galat);
    }
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
    // Fase AMAN-34 (#918): `x = [.., x, ..]` memindahkan `x` — dulu disalin utuh tiap langkah (20 ribu langkah 3 GB).
    ("a = [a, x] membungkus dirinya", "", "var a = []\n var i = 0\n while i < N { a = [a, i]\n i += 1 }\n wok(len(a))", 50_000, 24.0),
    // Fase AMAN-35 (#922): rune ditempel lewat penyangga stack — dulu tiap rune teks arena baru, `s` disalin.
    ("s = s + rune", "", "let cs = str::chars(str::repeat(\"ab,\", N))\n var s = \"\"\n for _, c in cs { s = s + c }\n wok(len(s))", 50_000, 24.0),
    ("x = arr::pop(x)[0] di antara push", "", "var a: []string = []\n for _, i in range(N) { a = arr::push(a, \"a\")\n a = arr::pop(a)[0]\n a = arr::push(a, \"b\") }\n wok(len(a))", 50_000, 24.0),
    // Fase AMAN-35 (#923): `jrg_str_next_char` dulu memvalidasi SELURUH teks tiap langkah.
    ("for c in s atas teks", "", "let s = str::repeat(\"ab,\", N)\n var k = 0\n for _, c in s { if c == 'b' { k += 1 } }\n wok(k)", 100_000, 24.0),
    // Fase AMAN-35 (#923): fungsi yang punya closure dulu menyalin `x` untuk sisi kanan yang memanggil apa pun.
    ("x = arr::push(x, arr::map(.., closure))", "", "var x = [[0]]\n for _, i in range(N) { x = arr::push(x, arr::map([i], |y| y + 1)) }\n wok(len(x))", 20_000, 24.0),
    ("std/csv::parse", "import \"std/csv\"\n", "var b: []string = []\n for _, i in range(N) { b = arr::push(b, «${i},\\\"a,b\\\",x») }\n wok(len(csv::parse(str::join(b, \"\\n\"))))", 5_000, 24.0),
    ("d[k] = v", "", "var d: dict<int> = dict::new()\n var i = 0\n while i < N { d[str(i)] = i\n i += 1 }\n wok(len(d))", 50_000, 24.0),
    // Fase AMAN-38: `x = f(x, ..)` yang cuma mengembalikan `x` tidak lagi menyalin struct berisi dict, juga
    // lewat bongkar dan fungsi modul — dulu tiap panggilan menyalin dict-nya, dan arena tidak membebaskannya.
    ("x = f(x, ..) atas struct berisi dict", "struct P { pub d: dict = dict::new() }\nfn tulis(p, k) {\n p.d[k] = 1\n return p\n}\n",
     "var p = P {}\n for _, i in range(N) { p = tulis(p, str(i)) }\n wok(len(p.d))", 5_000, 24.0),
    // Fase AMAN-38 (#938): lokal di fungsi yang punya closure, tapi tidak disebut closure itu, dipinjamkan.
    ("argumen dari fungsi ber-closure ke parameter yang cuma dibaca", "fn satu(x: []int) -> int { return x[0] }\n",
     "let a = arr::map(arr::fill(0, N), |y| y + 1)\n var i = 0\n var s = 0\n while i < 200000 { s += satu(a)\n i += 1 }\n wok(s)", 2_000, 4.0),
    ("x, _ = m::f(x, ..): std/batas::coba", "import \"std/batas\"\n",
     "var p = batas::baru(5, 0.001, N)\n for _, i in range(N) { p, _ = batas::coba(p, «k${i}», i) }\n wok(batas::jumlah_kunci(p))", 2_000, 24.0),
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

/// Sumber yang sama menghasilkan IR yang sama. Dulu penyimpul lokal bertumbuh membaca urutan `HashMap`,
/// jadi pemanggil dan badan `std/csv::from_dicts` bisa sepakat beda tipe: 7 dari 12 kompilasi rusak (#972).
#[test]
fn ir_sama_tiap_kompilasi() {
    if !ada_clang() { eprintln!("lewati: clang tidak ada"); return; }
    let dir = dir_uji("ir-tetap");
    let bungkus = dir.join("clang");
    std::fs::write(&bungkus, "#!/bin/sh\nfor a; do case $a in *.ll) cp \"$a\" \"$JRG_SALIN_IR\";; esac; done\nexit 1\n")
        .expect("tulis pembungkus");
    let _ = Command::new("chmod").arg("+x").arg(&bungkus).status();
    let mut korpus: Vec<(String, String)> = vec![("csv to_dicts/from_dicts".into(),
        "package main\nimport \"std/csv\"\npub fn main() {\n    let d = csv::to_dicts([[\"nama\"], [\"ani\"]])\n    \
         wok(csv::from_dicts([\"nama\"], d))\n}\n".into())];
    for nama in ["collections", "functions", "semantik_nilai"] {
        let p = format!("{}/tests/conformance/{}.jrg", env!("CARGO_MANIFEST_DIR"), nama);
        korpus.push((nama.into(), std::fs::read_to_string(&p).expect("baca konformans")));
    }
    let mut temuan = Vec::new();
    for (nama, src) in &korpus {
        let file = dir.join("p.jrg");
        std::fs::write(&file, src).expect("tulis sumber");
        let ir: Vec<Vec<u8>> = (0..3).map(|k| {
            let salin = dir.join(format!("ir{}.ll", k));
            let _ = Command::new(jrgc()).arg(&file).arg("--emit-llvm").arg("-o").arg(dir.join("p.bin")).arg("--no-run")
                .env("JRGC_CLANG", &bungkus).env("JRG_SALIN_IR", &salin).env("JRG_SOPAN", "1").output();
            std::fs::read(&salin).unwrap_or_default()
        }).collect();
        if ir[0].is_empty() { temuan.push(format!("  {}: IR tidak tertulis", nama)); }
        else if ir[0] != ir[1] || ir[0] != ir[2] { temuan.push(format!("  {}: IR berbeda antar kompilasi", nama)); }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(temuan.is_empty(), "IR backend LLVM tidak tetap:\n{}", temuan.join("\n"));
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
                // Ditolak LLVM: celah backend yang keras, boleh — tapi entri tercatatnya basi. Biner yang MATI
                // karena cacat jrgc (tag sampah, ukuran karangan) bukan penolakan: itu jawaban rusak.
                if !l.sukses {
                    let rusak = l.galat.contains("bug di jrgc")
                        || (l.galat.contains("memori habis") && jalankan_interp(src).sukses);
                    if rusak {
                        temuan.lock().unwrap().push(format!("  RUSAK di LLVM: {}\n    {}", nama,
                            l.galat.lines().find(|b| b.contains("error")).unwrap_or("").trim()));
                    }
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

include!("bersama/paritas_llvm.rs");

// ─── Tabel 2: gap yang diketahui
// ───────────────────────────────────────────

const BELUM_DIDUKUNG_LLVM: &[(&str, &str, &str)] = &[
    // Fase AMAN-43: acuan menjawab struct ber-field tambahan; dulu kalimat pertamanya "tipe `unit`
    // tidak punya method `y`" dan kalimat yang benar menunjuk `<lapis>`, bukan panggilan pemakai.
    ("json::merge atas struct ditolak di situs panggil",
     "package main\nstruct P { pub x: int, pub y: int }\npub fn main() { let q = json::merge(dict { y: 2 }, P { x: 1, y: 0 })\n wok(q.y) }",
     "`json::merge` atas struct `P` belum bisa di backend LLVM"),
    // Fase POTONG-BERTAG (#848): tipe elemennya tidak diketahui dari tag; dulu ditolak saat kompilasi.
    ("potong: array di dalam nilai bertag",
     "package main\npub fn main() { let v = len(env::args()) > 99 $ 5 : [1, 2, 3]\n wok(v[0:2]) }",
     "di dalam nilai bertag belum didukung backend LLVM"),
    // Fase POTONG: tipe hasil `t[a:b]` bergantung pada batasnya.
    ("potong: tuple berbatas variabel", "package main\npub fn main() { let t = (1, 2, 3)\n let i = 1\n wok(t[i:]) }",
     "batas potongan tuple di backend LLVM harus angka yang sudah pasti saat kompilasi"),
    // Fase SEBAR-TIPE: dua mesin lain menyimpan `2` di field `string`; di LLVM tipe field tetap.
    ("sebar membawa field berjenis lain dari sasarannya",
     "package main\nstruct A { pub x: int, pub y: string }\nstruct B { pub x: int, pub y: int }\npub fn main() { let b = B { x: 1, y: 2 }\n let a = A { x: 5, ...b }\n wok(a.y) }",
     "field `y` dari basis sebar bertipe `int`"),
    // Fase KEPUTUSAN-1: dulu literalnya diam-diam jadi `2.0` (dua mesin lain menyimpan `2`).
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
    // Fase BAGI-LLVM: dulu membungkus diam-diam (`math::lcm` menjawab angka karangan).
    ("math::lcm dan gcd di luar 64 bit dilaporkan", "package main\npub fn main() { var a = 3037000500\n var b = 3037000501\n wok(math::lcm(a, b)) }",
     "math::lcm: hasilnya 9223372040037250500 tidak muat di bilangan bulat 64 bit bertanda"),
    // Fase BAGI-LLVM: `sdiv` atas MIN dan -1 tak terdefinisi; dulu menjawab `1`.
    // Fase GESER-LLVM (#831): dulu `1 << 63` menjawab negatif dan `3 << 62` membuang bitnya diam-diam.
    ("int 64 bit: geseran yang membuang bit dilaporkan", "package main\npub fn main() { var k = 62\n wok(1 << k)\n wok(3 << k) }",
     "hasil `3 << 62` = 13835058055282163712 tidak muat di bilangan bulat 64 bit bertanda"),
    ("int 64 bit: MIN / -1 dilaporkan", "package main\npub fn main() { var m = -9223372036854775807 - 1\n var n = -1\n wok(m / n) }",
     "= 9223372036854775808 tidak muat di bilangan bulat 64 bit bertanda"),
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
    // "extern C: fungsi sebagai argumen" (sampai Fase FUNGSI-JEMBATAN) dan
    // "urutan pada array" (sampai Fase UMUM) dulu di sini.

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

    // Dict bernilai campur tipe DULU di sini, dengan alasan yang benar: satu
    // buffer pasangan `{ %JrgStr, V }` cuma punya satu `V`, jadi nilai campur …
    // Fase SEBAR-LLVM: ke fungsi bernama milik program sudah jalan; ke NILAI fungsi belum.
    ("spread di pemanggilan nilai fungsi",
     "package main\npub fn main() { let tambah = |a, b| a + b\n wok(tambah(...[1, 2])) }",
     "spread di pemanggilan"),

    // Argumen bernama: urutannya ditentukan dari nama parameter fungsinya saat
    // JALAN. Yang dikunci di sini bahwa penolakannya menyebut sebabnya, bukan
    // nama turunan `terap_bernama` yang pemakainya tidak pernah tulis.
    ("argumen bernama ke nilai fungsi",
     "package main\npub fn main() { let bagi = |a, b| a / b\n wok(bagi(a: 10, b: 2)) }",
     "argumen bernama"),


    // Fase SINKRON-LLVM: `Atomic` di backend ini 64 bit, dan luapannya ditolak saat jalan.
    ("atomic::add melewati 64 bit", "package main\npub fn main() { let a = Atomic::new(9223372036854775807)\n wok(atomic::add(a, 1)) }",
     "atomic::add: hasil `9223372036854775807 + 1` = 9223372036854775808 tidak muat di bilangan bulat 64 bit"),
];

// ─── Tabel 3: harus DITOLAK interpreter DAN LLVM
// ───────────────────────────

/// `(nama, program, kata-kunci-interpreter, kata-kunci-llvm)`
const DITOLAK_INTERP_DAN_LLVM: &[(&str, &str, &str, &str)] = &[
    // Keputusan pemilik 2026-11-14: `let`/`const` LOKAL bernama varian telanjang milik berkas ditolak parser,
    // seperti `fn`/`const`/`var` tingkat atas — dulu `let A = E::A` lolos tanpa mengikat, `const` mengikat.
    ("let lokal bernama varian unit milik berkas", "enum E { A, B }\nlet A = E::A\nwok(\"ok\")\n",
     "`A` sudah jadi nama varian `E::A` di berkas ini, sekarang sebagai `let`", "`A` sudah jadi nama varian `E::A` di berkas ini, sekarang sebagai `let`"),
    ("const lokal bernama varian unit milik berkas", "package main\nenum W { Merah, Isi(int) }\npub fn main() { const Merah = 5; wok(Merah) }\n",
     "`Merah` sudah jadi nama varian `W::Merah` di berkas ini, sekarang sebagai `const`", "`Merah` sudah jadi nama varian `W::Merah` di berkas ini, sekarang sebagai `const`"),
    ("let dan const lokal bernama varian ber-payload, di closure dan fungsi bersarang", "package main\nenum W { Merah, Isi(int) }\nfn f() { fn g() { let Isi = 2; Isi }  g() }\npub fn main() { let h = || { const Isi = 1; Isi }\n wok(h(), f()) }\n",
     "`Isi` sudah jadi nama varian `W::Isi` di berkas ini, sekarang sebagai `let`", "`Isi` sudah jadi nama varian `W::Isi` di berkas ini, sekarang sebagai `const`"),
    ("var dan := lokal bernama varian milik berkas (keputusan 2026-11-15, satu aturan untuk semua deklarasi lokal)",
     "package main\nenum E { A, B(int) }\npub fn main() { var A = 5\n B := 7\n wok(A, B) }\n",
     "`A` sudah jadi nama varian `E::A` di berkas ini, sekarang sebagai `var`", "`B` sudah jadi nama varian `E::B` di berkas ini, sekarang sebagai `:=`"),
    // Fase KUNCI-LLVM (#821): path yang tidak ada, dipanggil maupun sebagai nilai, kalimat acuan beserta sarannya.
    ("path: salah ketik builtin disarankan", "package main\npub fn main() {\n wok(math::sqr(4.0))\n}",
     "`math::sqr` tidak ditemukan — maksudnya `math::sqrt`?", "`math::sqr` tidak ditemukan — maksudnya `math::sqrt`?"),
    ("path: salah ketik namespace sebagai nilai", "package main\npub fn main() {\n let x = mathh::PI\n wok(x)\n}",
     "`mathh::PI` tidak ditemukan — maksudnya `math::PI`?", "`mathh::PI` tidak ditemukan — maksudnya `math::PI`?"),
    ("path: nama telanjang milik program", "package main\nfn f() { 1 }\npub fn main() {\n wok(arr::f)\n}",
     "`f` memang ada, tapi bukan sebagai isi `arr`", "`f` memang ada, tapi bukan sebagai isi `arr`"),
    ("path: nama ada di namespace lain", "package main\npub fn main() {\n wok(math::upper)\n}",
     "`math::upper` tidak ditemukan — `upper` memang ada, tapi sebagai `str::upper`", "`math::upper` tidak ditemukan — `upper` memang ada, tapi sebagai `str::upper`"),
    // Fase KUNCI-LLVM (#826): field privat lewat indeks teks.
    ("field privat dibaca lewat indeks teks (llvm)",
     "package main\nstruct P { pub x: int, y: int }\npub fn main() { let p = P { x: 1, y: 2 }\n wok(p[\"y\"]) }",
     "field `y` di `P` bersifat private", "field `y` di `P` bersifat private"),
    // Fase KUNCI-LLVM: field yang tidak ada (baca dan tulis), kalimat acuan; LLVM menolaknya saat kompilasi.
    ("struct: membaca field yang tidak ada", "package main\nstruct T { x: int }\npub fn main() {\n let t = T { x: 1 }\n wok(t.y)\n}",
     "`struct T` tidak punya method atau field bernama `y` (kalau maksudnya field/key, yang ada: x) — cek lagi penulisannya ya", "`struct T` tidak punya method atau field bernama `y` (kalau maksudnya field/key, yang ada: x) — cek lagi penulisannya ya"),
    ("struct: saran dari method warisan", "package main\nstruct A { x: int }\nimpl A { fn hitung(self) -> int { self.x } }\nstruct B: A { y: int }\npub fn main() {\n let b = B { x: 1, y: 2 }\n wok(b.hitng)\n}",
     "`struct B` tidak punya method atau field bernama `hitng` (kalau maksudnya field/key, yang ada: x, y) — maksudnya `hitung`?", "`struct B` tidak punya method atau field bernama `hitng` (kalau maksudnya field/key, yang ada: x, y) — maksudnya `hitung`?"),
    ("struct: menulis field yang tidak ada", "package main\nstruct T { x: int }\npub fn main() {\n var t = T { x: 1 }\n t.z = 3\n wok(t.x)\n}",
     "struct `T` tidak punya field `z` — maksudnya `x`?", "struct `T` tidak punya field `z` — maksudnya `x`?"),
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
    // Fase SINKRON-LLVM: kalimat `Atomic`/`Mutex` sama dengan acuan; jenis argumen yang salah pindah ke
    // `GAGAL_SAAT_JALAN` di Fase ATOM-LLVM.
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
    // Indeks bool/pecahan pindah ke `GAGAL_SAAT_JALAN` (#825).
    ("pecahan sebagai jumlah ulang (llvm)", "package main\npub fn main() {\n  wok(str::repeat(\"a\", 2.5))\n}",
     "butuh bilangan bulat, dapat pecahan", "str::repeat: butuh bilangan bulat, dapat pecahan `2.5`"),
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

include!("bersama/gagal_saat_jalan.rs");

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
    let mut berlokasi = 0;
    let mut tanpa: Vec<&str> = Vec::new();
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
        // Lokasi LLVM boleh tidak ada (lahir di runtime), tapi yang ada harus milik acuan (Fase LOKASI-LLVM).
        let lokasi = |g: &str| g.lines().find(|b| b.trim_start().starts_with("--> "))
            .map(|b| b.rsplit('/').next().unwrap_or(b).to_string());
        if lokasi(&l.galat).is_none() && lokasi(&i.galat).is_some() { tanpa.push(*nama); }
        if let Some(ll) = lokasi(&l.galat) {
            berlokasi += 1;
            if lokasi(&i.galat).as_deref() != Some(ll.as_str()) {
                gagal.push(format!("[{}] lokasi LLVM `{}`, acuan {:?}", nama, ll, lokasi(&i.galat)));
            }
        }
    }
    eprintln!("{} dari {} kasus berlokasi di LLVM; berlokasi di acuan saja: {:?}", berlokasi, GAGAL_SAAT_JALAN.len(), tanpa);
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
    assert!(include_str!("../src/kutip_inti.rs") == include_str!("../runtime/src/kutip_inti.rs"),
        "runtime/src/kutip_inti.rs berbeda dari src/kutip_inti.rs — salin ulang: cp src/kutip_inti.rs runtime/src/");
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


// ─── Cabang dan tempat yang jenisnya berbeda (Fase CAMPUR) ────────────────

/// Nilai berjenis berbeda yang SAH di acuan: dulu ditolak LLVM di `if`, ternary, `match`,
/// `return`, `arr::push`, dan `var` yang berganti golongan. Sekarang nilainya bertag.
const JENIS_CAMPUR: &[&str] = &["1", "2.5", "\"a\"", "true", "nil", "[1, 2]", "({\"k\": 1})", "[\"x\"]"];

fn posisi_campur(k: usize, a: &str, b: &str) -> String {
    let pakai = |x: &str| format!("wok({x}, typeof({x}), str({x}) + \"!\")");
    match k {
        0 => format!("fn kasus() {{ for _, c in [true, false] {{ let x = if c {{ {a} }} else {{ {b} }}\n {} }} }}", pakai("x")),
        1 => format!("fn kasus() {{ for _, c in [true, false] {{ let x = c $ {a} : {b}\n {} }} }}", pakai("x")),
        2 => format!("fn kasus() {{ for _, n in [0, 1] {{ let x = match n {{ 0 => {a}, _ => {b} }}\n {} }} }}", pakai("x")),
        3 => format!("fn pilih(c) {{ if c {{ return {a} }}\n return {b} }}\n\
                      fn kasus() {{ for _, c in [true, false] {{ let x = pilih(c)\n {} }} }}", pakai("x")),
        4 => format!("fn ekor(c) {{ if c {{ {a} }} else {{ {b} }} }}\n\
                      fn kasus() {{ for _, c in [true, false] {{ let x = ekor(c)\n {} }} }}", pakai("x")),
        5 => format!("fn ekor(n) {{ match n {{ 0 => {a}, _ => {b} }} }}\n\
                      fn kasus() {{ for _, n in [0, 1] {{ let x = ekor(n)\n {} }} }}", pakai("x")),
        6 => format!("fn kasus() {{ var xs = [{a}]\n xs = arr::push(xs, {b})\n {}\n {} }}", pakai("xs"), pakai("xs[1]")),
        _ => format!("fn kasus() {{ var x = {a}\n {}\n x = {b}\n {} }}", pakai("x"), pakai("x")),
    }
}

/// Tiap posisi satu program berisi semua pasangan jenis, jadi delapan kompilasi, bukan 224.
#[test]
fn campur_jenis_dijawab_seperti_acuan() {
    if lewati_tanpa_clang("campur_jenis_dijawab_seperti_acuan") { return; }
    let nama = ["if", "ternary", "match", "return", "ekor if", "ekor match", "arr::push", "var"];
    let mut masalah = Vec::new();
    for (k, posisi) in nama.iter().enumerate() {
        let mut src = String::from("package main\n");
        let mut main = String::from("pub fn main() {\n");
        let mut n = 0;
        for (i, a) in JENIS_CAMPUR.iter().enumerate() {
            for b in &JENIS_CAMPUR[i + 1..] {
                let badan = posisi_campur(k, a, b);
                // Fungsi tiap pasangan diberi nomor supaya tidak bertabrakan.
                src += &badan.replace("kasus", &format!("kasus{n}")).replace("pilih", &format!("pilih{n}"))
                    .replace("ekor(", &format!("ekor{n}("));
                src += "\n";
                main += &format!("    wok(\"-- kasus {n}\")\n    kasus{n}()\n");
                n += 1;
            }
        }
        src += &main;
        src += "}\n";
        let i = jalankan_interp(&src);
        let l = jalankan_llvm(&src);
        if !i.sukses {
            masalah.push(format!("[{posisi}] acuan menolak programnya sendiri:\n{}", i.galat));
        } else if !l.sukses {
            masalah.push(format!("[{posisi}] LLVM menolak:\n{}", l.galat.lines().take(8).collect::<Vec<_>>().join("\n")));
        } else if i.keluaran != l.keluaran {
            let beda = i.keluaran.lines().zip(l.keluaran.lines()).find(|(p, q)| p != q);
            masalah.push(format!("[{posisi}] keluaran beda, pertama: {:?}", beda));
        }
    }
    assert!(masalah.is_empty(), "{} posisi campur jenis tidak sama dengan acuan:\n\n{}",
        masalah.len(), masalah.join("\n\n"));
}

// ─── FFI lewat jembatan (Fase FFI-LLVM) ────────────────────────────────────

fn ada_perintah(p: &str) -> bool {
    std::process::Command::new(p).arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// Handle FFI yang tidak keluar dari bloknya dilepas di biner LLVM (Fase FFI-LEPAS) — worker Python tidak lagi
/// menumpuk objeknya — dan handle yang disimpan, dikembalikan, ditangkap atau dialiaskan TIDAK dilepas.
#[test]
fn handle_ffi_yang_tidak_keluar_dilepas_di_llvm() {
    if lewati_tanpa_clang("handle_ffi_yang_tidak_keluar_dilepas_di_llvm") || !ada_perintah("python3") { return; }
    for (src, harus) in [(r#"package main
fn satu() -> int {
    let h = py::ref("[1, 2]").unwrap()
    wok_("")
    return 1
}
pub fn main() {
    var n = 0
    for i in 0..300 { n = n + satu() }
    wok(py::eval("len(__import__(\"__main__\")._objs)"), n)
}
"#, Some("Result::Ok(0) 300")), (r#"package main
pub fn main() {
    var xs = []
    for i in 0..500 {
        let h = py::ref("[1, 2]").unwrap()
        xs = arr::push(xs, h)
    }
    wok(py::eval("len(__import__(\"__main__\")._objs)"), len(xs))
}
"#, Some("Result::Ok(500) 500")),
                         (r#"package main
fn buat() {
    let h = py::ref("[1, 2]").unwrap()
    return h
}
fn panjang(x) { return py::ref_method(x, "__len__").unwrap() }
pub fn main() {
    var xs = []
    for i in 0..3 {
        let h = py::ref("[" + str(i) + ", 9]").unwrap()
        xs = arr::push(xs, h)
    }
    wok(xs, panjang(xs[2]))
    let r = buat()
    wok(r, panjang(r))
    for i in 0..2 {
        let h = py::ref("{'a': " + str(i) + "}").unwrap()
        let f = || py::ref_method(h, "keys").unwrap()
        wok(f())
    }
    for i in 0..2 {
        let h = py::ref("[1, 2, 3]").unwrap()
        let g = h
        wok(g, panjang(g))
    }
    for i in 0..2 {
        let h = py::ref("'teks'").unwrap()
        if i == 1 { wok(py::ref_method(h, "upper").unwrap()) }
        wok(str(h), typeof(h), py::ref_method(h, "count", "t").unwrap())
    }
    var simpan = []
    for i in 0..2 {
        let h = py::ref("[" + str(i) + "]").unwrap()
        simpan = simpan + [h]
    }
    wok(simpan)
}
"#, None)] {
        let (i, l) = (jalankan_interp(src), jalankan_llvm(src));
        assert!(i.sukses && l.sukses, "gagal:\n{}\n{}", i.galat, l.galat);
        assert_eq!(i.keluaran, l.keluaran, "LLVM menjawab lain:\n{}", src);
        if let Some(h) = harus { assert!(l.keluaran.ends_with(h), "{:?}, harusnya diakhiri {:?}", l.keluaran, h); }
    }
}

/// `py::`/`js::`/`php::`/`lua::` di LLVM lewat pintu yang sama dengan dua mesin lain, termasuk
/// method atas handle dan `Err` dari bahasa lainnya; callback ke fungsi JRG ditolak dengan alasannya.
#[test]
fn ffi_di_llvm_seperti_acuan() {
    if lewati_tanpa_clang("ffi_di_llvm_seperti_acuan") { return; }
    let mut src = String::from("package main\npub fn main() {\n");
    if ada_perintah("python3") {
        src += "    wok(py::import(\"math\"), py::call(\"math\", \"sqrt\", 16.0), py::eval(\"nama_tak_ada\"))\n";
        src += "    match py::ref(\"[3, 1, 2]\") {\n        Ok(x) => wok(typeof(x), x.count(1), x.index(99)),\n        Err(e) => wok(e),\n    }\n";
    }
    if ada_perintah("node") {
        src += "    wok(js::eval(\"[1, 2, 3].map(x => x * 2)\"))\n";
        src += "    match js::ref(\"new Map([['a', 1]])\") {\n        Ok(m) => wok(m.get(\"a\"), m.has(\"b\")),\n        Err(e) => wok(e),\n    }\n";
    }
    if ada_perintah("php") { src += "    wok(php::call(\"strtoupper\", \"halo\"))\n"; }
    if ada_perintah("lua") { src += "    wok(lua::eval(\"return 2 ^ 10\"))\n"; }
    src += "}\n";
    let i = jalankan_interp(&src);
    let l = jalankan_llvm(&src);
    assert!(i.sukses, "acuan gagal:\n{}", i.galat);
    assert!(l.sukses, "LLVM menolak FFI:\n{}", l.galat);
    assert_eq!(i.keluaran, l.keluaran, "FFI di LLVM menjawab lain");

    // Callback ke fungsi JRG (Fase FUNGSI-JEMBATAN): sama dengan acuan.
    let mut cb = String::from("package main\npub fn main() {\n");
    if ada_perintah("python3") {
        cb += "    wok(py::call(\"builtins\", \"list\", py::call(\"builtins\", \"map\", |x: int| x * 10, [1, 2, 3])?))\n";
    }
    if ada_perintah("php") { cb += "    wok(php::call(\"array_map\", |x| x * x, [1, 2, 3]))\n"; }
    if ada_perintah("lua") { cb += "    wok(lua::call(\"string.rep\", \"ab\", 3))\n"; }
    cb += "}\n";
    let (i, l) = (jalankan_interp(&cb), jalankan_llvm(&cb));
    assert!(i.sukses && l.sukses, "callback FFI gagal:\nacuan: {}\nllvm: {}", i.galat, l.galat);
    assert_eq!(i.keluaran, l.keluaran, "callback FFI di LLVM menjawab lain");

    // Yang tetap ditolak, dengan alasannya: hitungan handle (tidak pernah dilepas di LLVM).
    for (src, alasan) in [
        ("package main\npub fn main() { wok(py::refs()) }\n", "tidak pernah dilepas"),
    ] {
        let l = jalankan_llvm(src);
        assert!(!l.sukses && l.galat.contains(alasan), "harus ditolak menyebut `{}`, dapat:\n{}", alasan, l.galat);
    }
}

/// Fase SERVE-LLVM: handler `dok::serve` dari biner `--emit-llvm` menjawab seperti acuan — termasuk
/// var modul yang ditulis handler, `req.params`, handler yang GAGAL (500, server tetap hidup), dan 404.
#[test]
fn dok_serve_di_llvm_menjawab_seperti_acuan() {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::{Duration, Instant};
    if !ada_clang() { return; }
    struct Server(std::process::Child);
    impl Drop for Server { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
    let program = |port: u16| format!(r#"package main
var HIT = 0
fn salam(n) {{ return "hai " + str(n) }}
pub fn main() {{
  let r = dok::router_new()
  dok::router_get(r, "/a", |req| {{ HIT = HIT + 1; dok::text(salam(HIT)) }})
  dok::router_get(r, "/u/:id", |req: DokRequest| dok::text("id=" + req.params["id"]))
  dok::router_post(r, "/gema", |req| dok::text(str(len(req.body))))
  dok::router_get(r, "/x", |req| [1][5])
  dok::serve(r, {})
}}
"#, port);
    let minta = |port: u16, baris: &str| -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).expect("server sudah mati");
        s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        write!(s, "{}", baris).unwrap();
        let mut buf = String::new();
        let _ = s.read_to_string(&mut buf);
        let badan = buf.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        format!("{} {}", buf.split(' ').nth(1).unwrap_or("?"), badan)
    };
    let mut hasil = Vec::new();
    for llvm in [false, true] {
        let dir = dir_uji("serve");
        let mut jawab = None;
        for _ in 0..5 {
            let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
            let mut cmd = if llvm {
                let bin = bangun_llvm(&dir, "s", &program(port)).expect("program server tidak terkompilasi di LLVM");
                Command::new(bin)
            } else {
                let f = dir.join("s.jrg");
                std::fs::write(&f, program(port)).unwrap();
                let mut c = Command::new(jrgc());
                c.arg(f).env("JRG_SOPAN", "1");
                c
            };
            cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
            let mut anak = Server(cmd.spawn().unwrap());
            let batas = Instant::now() + Duration::from_secs(20);
            let mut siap = false;
            while Instant::now() < batas && anak.0.try_wait().ok().flatten().is_none() {
                if TcpStream::connect(("127.0.0.1", port)).is_ok() { siap = true; break; }
                std::thread::sleep(Duration::from_millis(50));
            }
            if !siap { continue; }
            let get = |p: &str| format!("GET {} HTTP/1.1\r\nHost: x\r\n\r\n", p);
            jawab = Some(vec![
                minta(port, &get("/a")), minta(port, &get("/a")), minta(port, &get("/u/77")),
                minta(port, &get("/x")).split(':').next().unwrap_or("").to_string(),
                minta(port, &get("/hilang")),
                minta(port, "POST /gema HTTP/1.1\r\nContent-Length: 3\r\n\r\nabc"),
            ]);
            drop(anak);
            break;
        }
        let _ = std::fs::remove_dir_all(&dir);
        hasil.push(jawab.expect("server tidak pernah siap di lima port"));
    }
    assert_eq!(hasil[0], ["200 hai 1", "200 hai 2", "200 id=77", "500 handler error",
                          "404 404 Not Found: GET /hilang", "200 3"], "jawaban acuan");
    assert_eq!(hasil[0], hasil[1], "--emit-llvm menjawab beda dari acuan");
}
