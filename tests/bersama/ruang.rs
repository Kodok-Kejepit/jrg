// tests/bersama/ruang.rs — skenario ruang nama modul: program multi-berkas beserta
// jawaban yang BENAR, dijalankan `ruang_nama_modul_satu_arti_di_dua_mesin`.

pub enum Harap {
    /// Jalan, dan keluarannya persis ini.
    Keluaran(&'static str),
    /// Ditolak: galatnya memuat `pesan`, dan keluaran SEBELUM galat itu persis `sebelum`.
    Tolak { pesan: &'static str, sebelum: &'static str },
}

pub struct Skenario {
    pub nama: &'static str,
    /// `(nama berkas, isi)`; yang dijalankan selalu `main.jrg`.
    pub berkas: &'static [(&'static str, &'static str)],
    pub harap: Harap,
}

pub const SKENARIO: &[Skenario] = &[
    // Fase LUAR: trait milik unitnya — dulu acuan memakai default `main` untuk ketiganya.
    Skenario {
        nama: "R10 tiga trait bernama sama di tiga berkas, tiap default milik berkasnya",
        berkas: &[
            ("a.jrg", "pub trait Suara { fn bunyi(self) -> string = \"a-default\" }\npub struct A { pub x: int }\nimpl Suara for A {}\npub fn coba() { return A { x: 1 }.bunyi() }\n"),
            ("b.jrg", "pub trait Suara { fn bunyi(self) -> string = \"b-default\" }\npub struct B { pub x: int }\nimpl Suara for B {}\npub fn coba() { return B { x: 1 }.bunyi() }\n"),
            ("main.jrg", "package main\nimport \"a\"\nimport \"b\"\ntrait Suara { fn bunyi(self) -> string = \"main-default\" }\nstruct M { x: int }\nimpl Suara for M {}\npub fn main() {\n    wok(a::coba(), b::coba(), M { x: 1 }.bunyi())\n}\n"),
        ],
        harap: Harap::Keluaran("a-default b-default main-default\n"),
    },
    Skenario {
        nama: "R11 impl atas trait impor, berawalan dan telanjang, di samping trait sendiri bernama sama",
        berkas: &[
            ("a.jrg", "pub trait Suara { fn bunyi(self) -> string = \"a-default\" }\npub struct A { pub x: int }\nimpl Suara for A {}\npub fn coba() { return A { x: 1 }.bunyi() }\n"),
            ("c.jrg", "pub trait Wajib { fn harus(self) -> string\n  fn sapa(self) -> string = \"halo \" + self.harus() }\n"),
            ("main.jrg", "package main\nimport \"a\"\nimport \"c\"\ntrait Suara { fn bunyi(self) -> string = \"main\" }\nstruct K { x: int }\nstruct L { x: int }\nimpl a::Suara for K {}\nimpl Wajib for L { fn harus(self) -> string = \"L\" }\npub fn main() {\n    wok(K { x: 1 }.bunyi(), L { x: 2 }.sapa())\n}\n"),
        ],
        harap: Harap::Keluaran("a-default halo L\n"),
    },
    // Fase TELANJANG-MODUL: pola `Kotak { s }` dulu cocok di acuan dan tidak di `--vm`.
    Skenario {
        nama: "R6 varian ber-field milik modul ditulis telanjang di literal",
        berkas: &[
            ("a.jrg", "package a\npub enum Bentuk { Kotak { s: int }, Bulat(int) }\n"),
            ("main.jrg", "package main\nimport \"a\"\npub fn main() {\n    wok(Kotak { s: 1 })\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`Kotak` itu varian `Bentuk::Kotak` milik modul `a`", sebelum: "" },
    },
    Skenario {
        nama: "R7 varian ber-field milik modul ditulis telanjang di pola",
        berkas: &[
            ("a.jrg", "package a\npub enum Bentuk { Kotak { s: int }, Bulat(int) }\n"),
            ("main.jrg", "package main\nimport \"a\"\npub fn main() {\n    let k = a::Kotak { s: 5 }\n    match k { Kotak { s } => wok(s), _ => wok(0) }\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`Kotak` itu varian `Bentuk::Kotak` milik modul `a`", sebelum: "" },
    },
    Skenario {
        nama: "R8 varian tuple milik modul ditulis telanjang di pola",
        berkas: &[
            ("a.jrg", "package a\npub enum Bentuk { Kotak { s: int }, Bulat(int) }\n"),
            ("main.jrg", "package main\nimport \"a\"\npub fn main() {\n    match a::Bulat(3) { Bulat(n) => wok(n), _ => wok(0) }\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`Bulat` itu varian `Bentuk::Bulat` milik modul `a`", sebelum: "" },
    },
    Skenario {
        nama: "R9 varian ber-field milik modul lewat awalannya, literal dan pola",
        berkas: &[
            ("a.jrg", "package a\npub enum Bentuk { Kotak { s: int }, Bulat(int) }\n"),
            ("main.jrg", "package main\nimport \"a\"\npub fn main() {\n    let k = a::Kotak { s: 5 }\n    match k { a::Kotak { s } => wok(s), _ => wok(0) }\n    let j = Bentuk::Kotak { s: 6 }\n    match j { Bentuk::Kotak { s } => wok(s), _ => wok(0) }\n}\n"),
        ],
        harap: Harap::Keluaran("5\n6\n"),
    },
    Skenario {
        nama: "D1 const/var/let bernama sama di dua modul tidak saling menimpa",
        berkas: &[
            ("a.jrg", "package a\nconst NILAI = [1, 2, 3]\nvar HITUNG = 0\nlet TETAP = \"milik a\"\npub fn ambil() { return NILAI }\npub fn naik() { HITUNG = HITUNG + 1; return HITUNG }\npub fn tetap() { return TETAP }\n"),
            ("b.jrg", "package b\nvar NILAI = dict::new()\nvar HITUNG = 100\nlet TETAP = \"milik b\"\npub fn isi() { NILAI[\"x\"] = 1; return NILAI }\npub fn naik() { HITUNG = HITUNG + 10; return HITUNG }\npub fn tetap() { return TETAP }\n"),
            ("main.jrg", "package main\nimport \"a\"\nimport \"b\"\npub fn main() {\n    wok(a::ambil())\n    wok(b::isi())\n    wok(a::ambil())\n    wok(a::naik(), b::naik(), a::naik(), b::naik())\n    wok(a::tetap(), b::tetap())\n}\n"),
        ],
        harap: Harap::Keluaran("[1, 2, 3]\ndict {x: 1}\n[1, 2, 3]\n1 110 2 120\nmilik a milik b\n"),
    },
    Skenario {
        nama: "D3c var milik main tidak tersentuh fungsi modul bernama sama",
        berkas: &[
            ("a.jrg", "package a\nconst NILAI = [1, 2, 3]\nvar HITUNG = 0\npub fn ambil() { return NILAI }\npub fn naik() { HITUNG = HITUNG + 1; return HITUNG }\n"),
            ("main.jrg", "package main\nimport \"a\"\nvar HITUNG = 7\nfn ambil() { return \"milik main\" }\npub fn main() {\n    wok(HITUNG, ambil(), a::ambil(), a::naik(), HITUNG)\n}\n"),
        ],
        harap: Harap::Keluaran("7 milik main [1, 2, 3] 1 7\n"),
    },
    Skenario {
        nama: "D4 const pemakai tidak mengubah modul std (std/path)",
        berkas: &[
            ("main.jrg", "package main\nimport \"std/path\"\nconst PEMISAH = \";\"\npub fn main() {\n    wok(PEMISAH)\n    wok(path::normalize(\"a/./b/../c\"))\n    wok(path::join(\"usr\", \"lib\"))\n}\n"),
        ],
        harap: Harap::Keluaran(";\na/c\nusr/lib\n"),
    },
    Skenario {
        nama: "D3 var modul tidak terlihat telanjang dari pengimpor",
        berkas: &[
            ("a.jrg", "package a\nvar HITUNG = 0\npub fn ambil() { return 1 }\n"),
            ("main.jrg", "package main\nimport \"a\"\npub fn main() {\n    wok(\"mulai\")\n    wok(HITUNG)\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "variabel `HITUNG` belum dikenal", sebelum: "mulai\n" },
    },
    Skenario {
        nama: "D3b fn modul tidak terlihat telanjang dari pengimpor",
        berkas: &[
            ("a.jrg", "package a\npub fn ambil() { return 1 }\n"),
            ("main.jrg", "package main\nimport \"a\"\npub fn main() {\n    wok(\"mulai\")\n    wok(ambil())\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "variabel `ambil` belum dikenal", sebelum: "mulai\n" },
    },
    Skenario {
        nama: "D5 nama ber-awalan _ tidak terjangkau dari luar (fn)",
        berkas: &[
            ("a.jrg", "package a\nfn _rahasia() { return \"rahasia modul\" }\npub fn terbuka() { return _rahasia() }\n"),
            ("main.jrg", "package main\nimport \"a\"\npub fn main() {\n    wok(a::terbuka())\n    wok(a::_rahasia())\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`a::_rahasia` tidak ditemukan", sebelum: "rahasia modul\n" },
    },
    Skenario {
        nama: "D5b nama ber-awalan _ tidak terjangkau dari luar (const)",
        berkas: &[
            ("a.jrg", "package a\nconst _KUNCI = 42\npub fn kunci() { return _KUNCI }\n"),
            ("main.jrg", "package main\nimport \"a\"\npub fn main() {\n    wok(a::kunci())\n    wok(a::_KUNCI)\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`a::_KUNCI` tidak ditemukan", sebelum: "42\n" },
    },
    Skenario {
        nama: "K3 const dan let modul terbaca dari luar",
        berkas: &[
            ("a.jrg", "package a\npub const BATAS = 10\nlet NAMA = \"modul a\"\npub fn dua_kali() { return BATAS * 2 }\n"),
            ("main.jrg", "package main\nimport \"a\"\npub fn main() {\n    wok(a::BATAS, a::NAMA, a::dua_kali())\n}\n"),
        ],
        harap: Harap::Keluaran("10 modul a 20\n"),
    },
    Skenario {
        nama: "K3b var modul tidak diekspor: dibaca dari luar ditolak dengan sebabnya",
        berkas: &[
            ("a.jrg", "package a\nvar HITUNG = 0\npub fn naik() { HITUNG = HITUNG + 1; return HITUNG }\n"),
            ("main.jrg", "package main\nimport \"a\"\npub fn main() {\n    wok(a::naik())\n    wok(a::HITUNG)\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`HITUNG` adalah `var` milik modul `a`", sebelum: "1\n" },
    },
    Skenario {
        nama: "K2 import milik modul lain tidak terlihat (tanpa transitif)",
        berkas: &[
            ("bersama.jrg", "package bersama\npub fn nilai() { return 10 }\n"),
            ("kiri.jrg", "package kiri\nimport \"bersama\"\npub fn ambil() { return bersama::nilai() * 2 }\n"),
            ("main.jrg", "package main\nimport \"kiri\"\npub fn main() {\n    wok(kiri::ambil())\n    wok(bersama::nilai())\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`bersama::nilai` tidak ditemukan", sebelum: "20\n" },
    },
    Skenario {
        nama: "S1 as/range/len tidak memanggil fungsi pemakai bernama sama",
        berkas: &[
            ("main.jrg", "package main\nfn int(x) { return 99 }\nfn float(x) { return \"bukan float\" }\nfn rune(x) { return \"bukan rune\" }\nfn uint(x) { return \"bukan uint\" }\nfn bool(x) { return \"bukan bool\" }\nfn range(a, b) { return [\"hantu\"] }\nfn range_inclusive(a, b) { return [\"hantu inklusif\"] }\nfn len(x) { return 777 }\npub fn main() {\n    wok(2.5 as int, 3 as float, 65 as rune, 5 as uint, 1 as bool)\n    for _, i in 0..3 { wok(i) }\n    for _, i in 0..=1 { wok(i) }\n    wok(\"abcd\".len(), [1, 2, 3].len())\n    wok(int(1), len(\"ab\"), range(0, 2))\n}\n"),
        ],
        harap: Harap::Keluaran("2 3.0 A 5 true\n0\n1\n2\n0\n1\n4 3\n99 777 [hantu]\n"),
    },
    Skenario {
        nama: "S1b var bernama range tidak merusak a..b",
        berkas: &[
            ("main.jrg", "package main\nvar range = \"ini variabel\"\npub fn main() {\n    wok(range)\n    var n = 0\n    for _, i in 0..3 { n = n + i }\n    wok(n)\n}\n"),
        ],
        harap: Harap::Keluaran("ini variabel\n3\n"),
    },
    Skenario {
        nama: "S2 fn terap/terap_bernama pemakai tidak membajak sebar dan argumen bernama",
        berkas: &[
            ("main.jrg", "package main\nfn terap(f, a) { return \"dibajak terap\" }\nfn terap_bernama(f, p, n, v) { return \"dibajak terap_bernama\" }\nfn tambah(a, b) { return a + b }\npub fn main() {\n    wok(tambah(...[1, 2]))\n    wok(tambah(b: 2, a: 1))\n    wok(terap(1, 2))\n}\n"),
        ],
        harap: Harap::Keluaran("3\n3\ndibajak terap\n"),
    },
    Skenario {
        nama: "S3 modul pemakai bernama str/math tidak membajak method bawaan",
        berkas: &[
            ("str.jrg", "package str\npub fn upper(s) { return \"upper milik modul pemakai\" }\npub fn kebalik(s) { return str::reverse(s) }\n"),
            ("math.jrg", "package math\npub fn abs(x) { return \"abs milik modul pemakai\" }\n"),
            ("main.jrg", "package main\nimport \"str\"\nimport \"math\"\npub fn main() {\n    wok(str::upper(\"a\"), str::kebalik(\"abc\"))\n    wok(\"a\".upper(), \"abc\".lower())\n    wok(math::abs(0 - 3), (0 - 3).abs(), math::sqrt(4.0))\n}\n"),
        ],
        harap: Harap::Keluaran("upper milik modul pemakai cba\nA abc\nabs milik modul pemakai 3 2.0\n"),
    },
    Skenario {
        nama: "S4 modul std tetap memakai str:: bawaan walau pemakai punya modul str",
        berkas: &[
            ("str.jrg", "package str\npub fn join(a, b) { return \"join milik pemakai\" }\npub fn split(a, b) { return [\"split milik pemakai\"] }\npub fn starts_with(a, b) { return true }\npub fn ends_with(a, b) { return true }\n"),
            ("main.jrg", "package main\nimport \"str\"\nimport \"std/path\"\npub fn main() {\n    wok(str::join(1, 2))\n    wok(path::normalize(\"a/./b/../c\"))\n    wok(path::join(\"usr\", \"lib\"))\n}\n"),
        ],
        harap: Harap::Keluaran("join milik pemakai\na/c\nusr/lib\n"),
    },
    Skenario {
        nama: "S2b modul pemakai bernama dict tidak membajak literal dict",
        berkas: &[
            ("dict.jrg", "package dict\npub fn from_pairs(p) { return \"dibajak from_pairs\" }\n"),
            ("main.jrg", "package main\nimport \"dict\"\npub fn main() {\n    let d = {\"a\": 1}\n    wok(d)\n    wok(dict::from_pairs([]))\n}\n"),
        ],
        harap: Harap::Keluaran("dict {a: 1}\ndibajak from_pairs\n"),
    },
    Skenario {
        nama: "S5 dua modul bernama sama dari direktori berbeda",
        berkas: &[
            ("x/util.jrg", "package util\npub fn siapa() { return \"util milik x\" }\n"),
            ("y/util.jrg", "package util\npub fn siapa() { return \"util milik y\" }\n"),
            ("b.jrg", "package b\nimport \"y/util\"\npub fn dari_b() { return util::siapa() }\n"),
            ("main.jrg", "package main\nimport \"x/util\"\nimport \"b\"\npub fn main() {\n    wok(util::siapa())\n    wok(b::dari_b())\n}\n"),
        ],
        harap: Harap::Keluaran("util milik x\nutil milik y\n"),
    },
    Skenario {
        nama: "S5b sama, urutan import dibalik",
        berkas: &[
            ("x/util.jrg", "package util\npub fn siapa() { return \"util milik x\" }\n"),
            ("y/util.jrg", "package util\npub fn siapa() { return \"util milik y\" }\n"),
            ("b.jrg", "package b\nimport \"y/util\"\npub fn dari_b() { return util::siapa() }\n"),
            ("main.jrg", "package main\nimport \"b\"\nimport \"x/util\"\npub fn main() {\n    wok(util::siapa())\n    wok(b::dari_b())\n}\n"),
        ],
        harap: Harap::Keluaran("util milik x\nutil milik y\n"),
    },
    Skenario {
        nama: "S6 modul mencari import-nya di direktorinya SENDIRI lebih dulu",
        berkas: &[
            ("util.jrg", "package util\npub fn siapa() { return \"util di akar\" }\n"),
            ("x/util.jrg", "package util\npub fn siapa() { return \"util di x\" }\n"),
            ("x/pakai.jrg", "package pakai\nimport \"util\"\npub fn siapa() { return util::siapa() }\n"),
            ("main.jrg", "package main\nimport \"x/pakai\"\nimport \"util\"\npub fn main() {\n    wok(pakai::siapa())\n    wok(util::siapa())\n}\n"),
        ],
        harap: Harap::Keluaran("util di x\nutil di akar\n"),
    },
    Skenario {
        nama: "K4 penugasan ke nama yang belum dideklarasikan ditolak (bukan lokal diam-diam)",
        berkas: &[
            ("main.jrg", "package main\nfn hitung(k) {\n    var total = 0\n    for _, x in k { total = total + x }\n    return total\n}\npub fn main() {\n    wok(hitung([1, 2, 3]))\n    totl = 5\n    wok(totl)\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`totl` belum dideklarasikan", sebelum: "6\n" },
    },
    Skenario {
        nama: "K4b menugasi nama builtin tanpa deklarasi ditolak, bukan menimpa builtinnya",
        berkas: &[
            ("main.jrg", "package main\npub fn main() {\n    wok(len(\"ab\"))\n    len = 5\n    wok(len(\"abc\"))\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`len` bukan variabel — di sini ia fungsi", sebelum: "2\n" },
    },
    Skenario {
        nama: "K4c penugasan gabungan ke nama yang belum ada: kalimat yang sama",
        berkas: &[
            ("main.jrg", "package main\npub fn main() {\n    wok(\"mulai\")\n    jumlah += 1\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`jumlah` belum dideklarasikan", sebelum: "mulai\n" },
    },
    Skenario {
        nama: "K4d var yang dideklarasikan (lokal, modul, tangkapan closure) tetap bisa ditugasi",
        berkas: &[
            ("main.jrg", "package main\nvar g = 1\nfn ubah() { g = g + 1 }\npub fn main() {\n    var a = 1\n    a = 2\n    b := 3\n    b = 4\n    let f = || { a = a + 10 }\n    f()\n    ubah()\n    for _, i in [1, 2] { a = a + i }\n    var d = {\"k\": 1}\n    d[\"k\"] = 2\n    x, y := 1, 2\n    x, y = y, x\n    wok(a, b, g, d, x, y)\n}\n"),
        ],
        harap: Harap::Keluaran("15 4 2 dict {k: 2} 2 1\n"),
    },
    Skenario {
        nama: "K1 satu berkas dua alias = satu modul (keadaannya satu)",
        berkas: &[
            ("c.jrg", "package c\nvar N = 0\npub fn naik() { N = N + 1; return N }\n"),
            ("main.jrg", "package main\nimport \"c\" as satu\nimport \"c\" as dua\npub fn main() {\n    wok(satu::naik(), satu::naik(), dua::naik())\n}\n"),
        ],
        harap: Harap::Keluaran("1 2 3\n"),
    },
    Skenario {
        nama: "K1b berlian: keadaan modul bersama satu, lewat alias berbeda",
        berkas: &[
            ("c.jrg", "package c\nvar N = 0\npub fn naik() { N = N + 1; return N }\n"),
            ("a.jrg", "package a\nimport \"c\"\npub fn maju() { return c::naik() }\n"),
            ("b.jrg", "package b\nimport \"c\" as hitung\npub fn maju() { return hitung::naik() }\n"),
            ("main.jrg", "package main\nimport \"a\"\nimport \"b\"\nimport \"c\"\npub fn main() {\n    wok(a::maju(), b::maju(), c::naik(), a::maju())\n}\n"),
        ],
        harap: Harap::Keluaran("1 2 3 4\n"),
    },
    Skenario {
        nama: "P1 modul memakai fungsi, const, var, dan closure miliknya sendiri",
        berkas: &[
            ("lib.jrg", "package lib\nconst FAKTOR = 3\nvar panggilan = 0\nfn _kali(n) { return n * FAKTOR }\npub fn hitung(xs) {\n    panggilan = panggilan + 1\n    return arr::map(xs, |x| _kali(x) + panggilan)\n}\npub fn berapa() { return panggilan }\n"),
            ("main.jrg", "package main\nimport \"lib\"\npub fn main() {\n    wok(lib::hitung([1, 2]))\n    wok(lib::hitung([1, 2]))\n    wok(lib::berapa())\n}\n"),
        ],
        harap: Harap::Keluaran("[4, 7]\n[5, 8]\n2\n"),
    },
    Skenario {
        nama: "P2 struct, method, enum dan konstruktor modul",
        berkas: &[
            ("bentuk.jrg", "package bentuk\npub struct Kotak { pub l: int = 2, pub t: int = 3 }\nimpl Kotak { pub fn luas(self) { return self.l * self.t } }\npub enum Warna { Merah, Rgb(int, int, int) }\npub fn bawaan() { return Kotak {} }\npub fn merah() { return Merah }\n"),
            ("main.jrg", "package main\nimport \"bentuk\"\npub fn main() {\n    let k = bentuk::bawaan()\n    wok(k.luas(), Kotak { l: 5 }.luas())\n    wok(bentuk::merah(), Warna::Rgb(1, 2, 3), bentuk::Merah)\n    match bentuk::merah() { Warna::Merah => wok(\"merah\"), _ => wok(\"lain\") }\n}\n"),
        ],
        harap: Harap::Keluaran("6 15\nWarna::Merah Warna::Rgb((1, 2, 3)) Warna::Merah\nmerah\n"),
    },
    Skenario {
        nama: "P3 modul memakai import-nya sendiri, dan pengimpor memakai keduanya",
        berkas: &[
            ("bersama.jrg", "package bersama\npub fn nilai() { return 10 }\n"),
            ("kiri.jrg", "package kiri\nimport \"bersama\"\npub fn ambil() { return bersama::nilai() * 2 }\n"),
            ("main.jrg", "package main\nimport \"kiri\"\nimport \"bersama\"\npub fn main() {\n    wok(kiri::ambil(), bersama::nilai())\n}\n"),
        ],
        harap: Harap::Keluaran("20 10\n"),
    },
    Skenario {
        nama: "P4 fungsi modul sebagai nilai, dan nama main yang membayangi builtin",
        berkas: &[
            ("alat.jrg", "package alat\npub fn dua_kali(x) { return x * 2 }\npub fn pakai(f, x) { return f(x) }\n"),
            ("main.jrg", "package main\nimport \"alat\"\nfn abs(x) { return \"abs milik main\" }\npub fn main() {\n    wok(arr::map([1, 2], alat::dua_kali))\n    wok(alat::pakai(alat::dua_kali, 5))\n    wok(abs(0 - 1), math::abs(0 - 1), (0 - 1).abs())\n    let f = alat::dua_kali\n    wok(f(4))\n}\n"),
        ],
        harap: Harap::Keluaran("[2, 4]\n10\nabs milik main 1 1\n8\n"),
    },
    Skenario {
        nama: "P5 goroutine di dalam modul memakai var modulnya",
        berkas: &[
            ("kerja.jrg", "package kerja\nvar hasil = 0\npub fn jalan(n) {\n    let c = Chan::new(1)\n    spawn { c <- n * 2 }\n    hasil = <-c\n    return hasil\n}\npub fn terakhir() { return hasil }\n"),
            ("main.jrg", "package main\nimport \"kerja\"\npub fn main() {\n    wok(kerja::jalan(21))\n    wok(kerja::terakhir())\n}\n"),
        ],
        harap: Harap::Keluaran("42\n42\n"),
    },
    Skenario {
        nama: "P6 modul std bersarang dan std biasa tetap jalan",
        berkas: &[
            ("main.jrg", "package main\nimport \"std/set\"\nimport \"std/mesin/vek\"\npub fn main() {\n    var s = set::new()\n    s = s.add(3)\n    s = s.add(3)\n    wok(s.len())\n    let v = vek::baru(3.0, 4.0)\n    wok(v.panjang())\n}\n"),
        ],
        harap: Harap::Keluaran("1\n5.0\n"),
    },
    Skenario {
        nama: "G1 goroutine menulis var tingkat atas: pemiliknya melihat",
        berkas: &[
            ("main.jrg", "package main\nvar HITUNG = 0\npub fn main() {\n    let c = make_chan(int, 1)\n    spawn {\n        HITUNG = 5\n        c <- 1\n    }\n    <-c\n    wok(HITUNG)\n}\n"),
        ],
        harap: Harap::Keluaran("5\n"),
    },
    Skenario {
        nama: "G2 goroutine melihat perubahan var sesudah ia lahir",
        berkas: &[
            ("main.jrg", "package main\nvar BENDERA = false\npub fn main() {\n    let c = make_chan(int, 1)\n    let d = make_chan(int, 1)\n    spawn {\n        <-d\n        wok(\"anak melihat\", BENDERA)\n        c <- 1\n    }\n    BENDERA = true\n    d <- 1\n    <-c\n}\n"),
        ],
        harap: Harap::Keluaran("anak melihat true\n"),
    },
    Skenario {
        nama: "G3 var modul dibagi goroutine yang lahir di modul lain",
        berkas: &[
            ("hitung.jrg", "package hitung\nvar N = [0]\npub fn naik() { N[0] = N[0] + 1 }\npub fn baca() { return N[0] }\n"),
            ("main.jrg", "package main\nimport \"hitung\"\npub fn main() {\n    let c = make_chan(int, 1)\n    spawn {\n        hitung::naik()\n        hitung::naik()\n        c <- 1\n    }\n    <-c\n    hitung::naik()\n    wok(hitung::baca())\n}\n"),
        ],
        harap: Harap::Keluaran("3\n"),
    },
    Skenario {
        nama: "E2 fn pemakai bernama Some menang atas konstruktor bawaan",
        berkas: &[
            ("main.jrg", "package main\nfn Some(x) { return x + 100 }\npub fn main() {\n    wok(Some(1))\n    let f = Some\n    wok(f(2))\n    wok(::Some(3))\n}\n"),
        ],
        harap: Harap::Keluaran("101\n102\nOption::Some(3)\n"),
    },
    Skenario {
        nama: "F1 menugasi nama fungsi ditolak: bukan variabel",
        berkas: &[
            ("main.jrg", "package main\nfn f() { return 1 }\npub fn main() {\n    wok(\"mulai\")\n    f = 5\n    wok(f)\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`f` bukan variabel — di sini ia fungsi", sebelum: "mulai\n" },
    },
    Skenario {
        nama: "F2 menugasi varian enum ditolak: bukan variabel",
        berkas: &[
            ("main.jrg", "package main\nenum E { A, B }\npub fn main() {\n    wok(\"mulai\")\n    A = 5\n    wok(A)\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`A` bukan variabel — di sini ia nilai tetap", sebelum: "mulai\n" },
    },
    Skenario {
        nama: "H1 x[0] = 1 atas x yang tidak ada: kalimat penugasan, sebelum index dihitung",
        berkas: &[
            ("main.jrg", "package main\nfn f() { wok(\"f jalan\"); return 0 }\npub fn main() {\n    wok(\"mulai\")\n    x[f()] = 1\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`x` belum dideklarasikan", sebelum: "mulai\n" },
    },
    Skenario {
        nama: "H2 x.a = 1 atas x yang tidak ada",
        berkas: &[
            ("main.jrg", "package main\npub fn main() {\n    wok(\"mulai\")\n    x.a = 1\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`x` belum dideklarasikan", sebelum: "mulai\n" },
    },
    Skenario {
        nama: "T1 fungsi modul tampil tanpa awalan, dan alias::Tipe::metode jalan",
        berkas: &[
            ("alat.jrg", "package alat\npub struct Titik { pub x: int }\nimpl Titik {\n    pub fn baru(x) { return Titik { x: x } }\n    pub fn dobel(self) { return self.x * 2 }\n}\npub fn dua_kali(x) { return x * 2 }\n"),
            ("main.jrg", "package main\nimport \"alat\"\npub fn main() {\n    wok(alat::dua_kali)\n    wok(str(alat::dua_kali))\n    wok({\"f\": alat::dua_kali})\n    wok(alat::Titik::baru(3).dobel())\n    wok(Titik::baru(4).dobel())\n    wok(alat::Titik::dobel(Titik { x: 5 }))\n}\n"),
        ],
        harap: Harap::Keluaran("<fn dua_kali>\n<fn dua_kali>\ndict {f: <fn dua_kali>}\n6\n8\n10\n"),
    },
    Skenario {
        nama: "B1 ::nama menjangkau bawaan yang dibayangi",
        berkas: &[
            ("main.jrg", "package main\nfn len(a, b, c) { return 99 }\npub fn main() {\n    wok(len(1, 2, 3), ::len(\"ab\"), ::str::upper(\"a\"), ::math::PI > 3.0)\n    wok(::arr::map([1, 2], |x| x * 2))\n    let f = ::len\n    wok(f([1, 2, 3]))\n}\n"),
        ],
        harap: Harap::Keluaran("99 2 A true\n[2, 4]\n3\n"),
    },
    Skenario {
        nama: "B2 ::nama yang bukan bawaan ditolak dengan sebabnya",
        berkas: &[
            ("main.jrg", "package main\nfn milikku() { return 1 }\npub fn main() {\n    wok(\"mulai\")\n    wok(::milikku())\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`::milikku` tidak ditemukan: `::` di depan nama berarti bawaan bahasa", sebelum: "mulai\n" },
    },
    Skenario {
        nama: "C1 import melingkar ditolak dengan rantainya",
        berkas: &[
            ("a.jrg", "package a\nimport \"b\"\npub fn fa() { return 1 }\n"),
            ("b.jrg", "package b\nimport \"a\"\npub fn fb() { return 2 }\n"),
            ("main.jrg", "package main\nimport \"a\"\npub fn main() {\n    wok(a::fa())\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "import muter-muter: a -> b -> a", sebelum: "" },
    },
    Skenario {
        nama: "I3 impl atas dict milik pemakai terjangkau sebagai method",
        berkas: &[
            ("main.jrg", "package main\nimpl dict {\n    fn dobel(self) { return 7 }\n}\npub fn main() {\n    let d = {\"a\": 1}\n    wok(d.dobel(), d.len())\n}\n"),
        ],
        harap: Harap::Keluaran("7 1\n"),
    },
    Skenario {
        nama: "I4 struct pemakai bernama arr tidak membajak method array",
        berkas: &[
            ("main.jrg", "package main\nstruct arr { pub n: int }\nimpl arr {\n    fn len(self) { return 77 }\n    fn sum(a) { return \"sum pemakai\" }\n}\npub fn main() {\n    wok([1, 2].len())\n    wok([1, 2].sum())\n    wok(arr { n: 1 }.len())\n}\n"),
        ],
        harap: Harap::Keluaran("2\n3\n77\n"),
    },
    Skenario {
        nama: "SEL select menugasi var modul dan tangkapan closure",
        berkas: &[
            ("main.jrg", "package main\nvar hasil = 0\npub fn main() {\n    let c = make_chan(int, 2)\n    c <- 7\n    c <- 8\n    select {\n        case hasil = <-c: { wok(\"dapat\", hasil) }\n    }\n    var lokal = 0\n    let f = || {\n        select {\n            case lokal = <-c: { wok(\"dalam\", lokal) }\n        }\n    }\n    f()\n    wok(hasil, lokal)\n}\n"),
        ],
        harap: Harap::Keluaran("dapat 7\ndalam 8\n7 8\n"),
    },
    Skenario {
        nama: "SEL2 select menugasi nama yang belum dideklarasikan ditolak saat jalan",
        berkas: &[
            ("main.jrg", "package main\npub fn main() {\n    let c = make_chan(int, 1)\n    c <- 7\n    wok(\"mulai\")\n    select {\n        case belum = <-c: { wok(\"x\") }\n    }\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`belum` belum dideklarasikan", sebelum: "mulai\n" },
    },
    Skenario {
        nama: "W1 method induk dari modul lain terjangkau lewat anak di main",
        berkas: &[
            ("dasar.jrg", "package dasar\npub struct Hewan { pub nama: string }\nimpl Hewan {\n    pub fn sapa(self) { return \"halo \" + self.nama }\n}\n"),
            ("main.jrg", "package main\nimport \"dasar\"\nstruct Anjing: Hewan { pub ras: string }\npub fn main() {\n    let a = Anjing { nama: \"Bimo\", ras: \"kampung\" }\n    wok(a.sapa())\n}\n"),
        ],
        harap: Harap::Keluaran("halo Bimo\n"),
    },
    Skenario {
        nama: "N1 nama modul yang sama dengan nama builtin tetap bergabung per nama",
        berkas: &[
            ("math.jrg", "package math\npub fn tambah_dua(x) { return x + 2 }\npub fn sqrt(x) { return \"sqrt pemakai\" }\n"),
            ("main.jrg", "package main\nimport \"math\"\npub fn main() {\n    wok(math::tambah_dua(1), math::sqrt(4.0), math::abs(0 - 3))\n    wok((4.0).sqrt())\n}\n"),
        ],
        harap: Harap::Keluaran("3 sqrt pemakai 3\n2.0\n"),
    },
    Skenario {
        nama: "U1 salah ketik nama ekspor modul dapat saran dari modul itu",
        berkas: &[
            ("alat.jrg", "package alat\npub fn dua_kali(x) { return x * 2 }\n"),
            ("main.jrg", "package main\nimport \"alat\"\npub fn main() {\n    wok(\"mulai\")\n    wok(alat::dua_kalli(2))\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`alat::dua_kalli` tidak ditemukan — maksudnya `alat::dua_kali`?", sebelum: "mulai\n" },
    },
    Skenario {
        nama: "U2 salah ketik nama milik modul di dalam modulnya dapat saran",
        berkas: &[
            ("alat.jrg", "package alat\nfn bantu_hitung(x) { return x * 2 }\npub fn pakai(x) { return bantu_hitun(x) }\n"),
            ("main.jrg", "package main\nimport \"alat\"\npub fn main() {\n    wok(\"mulai\")\n    wok(alat::pakai(2))\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "variabel `bantu_hitun` belum dikenal — maksudnya `bantu_hitung`?", sebelum: "mulai\n" },
    },
    Skenario {
        nama: "E1 fn bernama sama dengan varian enum di berkas yang sama ditolak parser",
        berkas: &[
            ("main.jrg", "package main\nenum E { A, B }\nfn A() { return 5 }\npub fn main() {\n    wok(A)\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`A` sudah jadi nama varian `E::A` di berkas ini", sebelum: "" },
    },
    Skenario {
        nama: "W1b method induk dari modul lain memakai nama privat modulnya sendiri",
        berkas: &[
            ("dasar.jrg", "package dasar\nconst AWALAN = \"saya \"\nfn hias(s) { return AWALAN + s }\npub struct Dasar { pub n: string }\nimpl Dasar {\n    pub fn perkenalan(self) { return hias(self.n) }\n    pub fn to_string(self) -> string { return \"<\" + self.n + \">\" }\n}\n"),
            ("main.jrg", "package main\nimport \"dasar\"\nconst AWALAN = \"bukan ini \"\nfn hias(s) { return \"bukan ini juga\" }\nstruct Anak: Dasar { pub umur: int }\nstruct Cucu: Anak { pub x: int }\npub fn main() {\n    let a = Anak { n: \"Bimo\", umur: 3 }\n    wok(a.perkenalan())\n    wok(a)\n    let c = Cucu { n: \"Caca\", umur: 1, x: 0 }\n    wok(c.perkenalan())\n}\n"),
        ],
        harap: Harap::Keluaran("saya Bimo\n<Bimo>\nsaya Caca\n"),
    },
    Skenario {
        nama: "V1 var tingkat atas dipakai sebelum diisi: kalimatnya menyebut urutan, bukan salah ketik",
        berkas: &[
            ("main.jrg", "package main\nvar A = f()\nfn f() { return B[0] }\nvar B = [2]\npub fn main() {\n    wok(A)\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`B` dipakai sebelum nilainya diisi", sebelum: "" },
    },
    Skenario {
        nama: "V2 sama, di dalam modul (VM dulu kehilangan sarannya)",
        berkas: &[
            ("m.jrg", "package m\nvar A = B + 1\nconst B = 2\npub fn a() { return A }\n"),
            ("main.jrg", "package main\nimport \"m\"\npub fn main() {\n    wok(m::a())\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`B` dipakai sebelum nilainya diisi", sebelum: "" },
    },
    Skenario {
        nama: "V3 nama tingkat atas yang SUDAH diisi tetap terbaca dari fungsi yang dideklarasikan lebih dulu",
        berkas: &[
            ("main.jrg", "package main\nfn f() { return B + K }\nvar B = 2\nconst K = 10\npub fn main() {\n    wok(f())\n}\n"),
        ],
        harap: Harap::Keluaran("12\n"),
    },
    Skenario {
        nama: "J1 import berakhiran .jrg dan berawalan ./ dimuat seperti namanya",
        berkas: &[
            ("bantu.jrg", "package bantu\npub fn dua() { return 2 }\n"),
            ("sub/alat.jrg", "package alat\npub fn tiga() { return 3 }\n"),
            ("main.jrg", "package main\nimport \"bantu.jrg\"\nimport \"./sub/alat.jrg\"\npub fn main() {\n    wok(bantu::dua(), alat::tiga())\n}\n"),
        ],
        harap: Harap::Keluaran("2 3\n"),
    },
    Skenario {
        nama: "J2 import ber-:: dan bertitik memberi alias segmen terakhirnya",
        berkas: &[
            ("sub/alat.jrg", "package alat\npub fn tiga() { return 3 }\n"),
            ("sub/kakas.jrg", "package kakas\npub fn empat() { return 4 }\n"),
            ("main.jrg", "package main\nimport \"sub::alat\"\nimport \"sub.kakas\"\npub fn main() {\n    wok(alat::tiga(), kakas::empat())\n}\n"),
        ],
        harap: Harap::Keluaran("3 4\n"),
    },
    Skenario {
        nama: "J3 jalur import bukan nama modul: sub::alat::f ditolak di dua mesin",
        berkas: &[
            ("sub/alat.jrg", "package alat\npub fn tiga() { return 3 }\n"),
            ("main.jrg", "package main\nimport \"sub::alat\"\npub fn main() {\n    wok(\"mulai\")\n    wok(sub::alat::tiga())\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`sub::alat::tiga` tidak ditemukan", sebelum: "mulai\n" },
    },
    Skenario {
        nama: "J4 nama asli modul yang diimpor ber-alias menyebut aliasnya",
        berkas: &[
            ("main.jrg", "package main\nimport \"std/set\" as s\npub fn main() {\n    wok(\"mulai\")\n    let a = set::new()\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "modul `set` diimpor dengan nama `s` di berkas ini, jadi tulis `s::new`", sebelum: "mulai\n" },
    },
    Skenario {
        nama: "J5 modul yang tidak ada: bantuannya menyebut nama berkas yang benar",
        berkas: &[
            ("main.jrg", "package main\nimport \"hantu.jrg\"\npub fn main() {\n    wok(1)\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "nama file-nya harus `hantu.jrg`", sebelum: "" },
    },
    Skenario {
        nama: "J6 alias atas namespace bawaan tidak memuat apa pun, jadi tidak disebut",
        berkas: &[
            ("main.jrg", "package main\nimport \"std/math\" as m\npub fn main() {\n    wok(math::sqrt(4.0))\n    wok(math::hantu(4.0))\n}\n"),
        ],
        harap: Harap::Tolak { pesan: "`math::hantu` tidak ditemukan — cek lagi penulisan", sebelum: "2.0\n" },
    },
];
