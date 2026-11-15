// Tabel bersama `tests/llvm_parity.rs` dan `tests/jit_parity.rs`; disisipkan lewat `include!`.

const GAGAL_SAAT_JALAN: &[(&str, &str, &str, &str)] = &[
    // #1011: kanal tertutup ditolak di semua lengan `select` sebelum mencoba, juga di LLVM.
    ("select: lengan kedua tertutup ditolak walau lengan pertama siap",
     "let t = make_chan(int, 1)\nt.close()\nlet d = make_chan(int, 1)\nd <- 1\nwok(\"sebelum\")\nselect {\n    case x := <-d: { wok(\"tak boleh\", x) }\n    case y := <-t: { wok(\"tak boleh juga\", y) }\n}\n",
     "sebelum", "channel sudah ditutup dan kosong: tidak akan ada nilai lagi"),
    // Fase SEBAR-VARIAN (#975): basis sebar yang tidak cocok dengan varian sasaran gagal saat jalan.
    ("sebar varian: field wajib tidak dibawa basis", "package main\nenum E { V { a: int, b: int }, W { a: int } }\npub fn main() {\n wok(\"mulai\")\n wok(E::V { a: 1, ...E::W { a: 3 } })\n}",
     "mulai", "varian `E::V` butuh field `b`"),
    ("sebar varian: basis membawa field liar", "package main\nenum E { V { a: int }, X { z: string } }\npub fn main() {\n wok(\"mulai\")\n wok(E::V { a: 1, ...E::X { z: \"q\" } })\n}",
     "mulai", "basis `...` membawa field `z` yang tidak dideklarasikan varian `E::V`"),
    ("sebar varian: basis varian tanpa field", "package main\nenum E { V { a: int }, U }\nfn f(e: E) -> E { E::V { ...e } }\npub fn main() {\n wok(f(E::V { a: 1 }))\n wok(f(E::U))\n}",
     "E::V(V {a: 1})", "dapat `enum:E`, yang tidak punya field untuk disalin"),
    ("sebar varian: struct membawa field liar", "package main\nenum E { V { a: int } }\nstruct P { a: int, c: int }\npub fn main() {\n wok(\"mulai\")\n wok(E::V { ...P { a: 1, c: 2 } })\n}",
     "mulai", "basis `...` membawa field `c` yang tidak dideklarasikan varian `E::V`"),
    // Fase TAMBAH-TEKS (#969): operator aritmetika atas nilai bertag yang golongannya salah — kalimat acuan.
    ("bertag: array + int", "package main\npub fn main() {\n var x = nil\n x = [1]\n wok(\"a\")\n wok(x + 5)\n}",
     "a", "tidak bisa `+` antara `array` dan `int`"),
    ("bertag: string - int", "package main\npub fn main() {\n var x = nil\n x = \"ab\"\n wok(\"a\")\n wok(x - 1)\n}",
     "a", "tidak bisa `-` antara `string` dan `int`"),
    ("bertag: string % int", "package main\npub fn main() {\n var x = nil\n x = \"ab\"\n wok(\"a\")\n wok(x % 5)\n}",
     "a", "di JRG `%` cuma sisa bagi"),
    // Fase AMAN-29 (#908): masukan berbaris baru tidak memalsukan baris pesan, yang panjang dipotong.
    ("pesan: teks berbaris baru di-escape", "package main\npub fn main() {\n wok(\"a\")\n wok(str::to_int(\"12\\nPALSU: x\"))\n}", "a", "teks `12\\u{a}PALSU: x` bukan bilangan bulat"),
    // Fase AMAN-37 (#927): array bertag yang isinya tak sebanding: `sort` menyebut `array`, `<` menyebut daunnya.
    ("sort array bertag tak sebanding", "package main\npub fn main() {\n var e = []\n e = arr::push(e, [1, \"a\"])\n e = arr::push(e, [\"x\", 1])\n wok(\"mulai\")\n wok(arr::sort(e))\n}",
     "mulai", "arr::sort: tidak bisa membandingkan `array` dengan `array`"),
    // Fase POLA-BERTAG (#950): `match` tanpa lengan yang cocok menyebut nilainya (dipotong), seperti acuan.
    ("match tanpa lengan cocok", "package main\npub fn main() {\n let s = \"hai\"\n wok(\"m\")\n match s { \"x\" => wok(1) }\n}",
     "m", "match tidak exhaustive: tidak ada arm yang cocok untuk nilai `hai`"),
    ("match bertag tanpa lengan cocok", "package main\npub fn main() {\n let c = [Some(9), \"q\"]\n wok(\"m\")\n match c[1] { Some(v) => wok(v) }\n}",
     "m", "match tidak exhaustive: tidak ada arm yang cocok untuk nilai `q`"),
    ("match nilai panjang dipotong", "package main\npub fn main() {\n wok(\"m\")\n match str::repeat(\"ab\", 50) { \"x\" => wok(1) }\n}",
     "m", "`abababababababababababababababababababababababababababababab… (100 karakter)`"),
    // Fase POLA-BERTAG (#951): `for` atas nilai bertag yang tak bisa diiterasi — struct berkalimat sendiri.
    ("for atas struct di nilai bertag", "package main\nstruct Titik { x: int }\npub fn main() {\n let a = [Titik { x: 1 }, 2]\n wok(\"m\")\n for x in a[0] { wok(x) }\n}",
     "m", "for-in: `Titik` itu struct, bukan koleksi"),
    ("for atas bool di nilai bertag", "package main\npub fn main() {\n let b = [true, \"s\"]\n wok(\"m\")\n for x in b[0] { wok(x) }\n}",
     "m", "for-in: tidak bisa iterate `bool`"),
    // Fase CELAH-BERTAG (#945–#947): yang bukan wadah, kunci antara hilang dan field tuple di luar batas, atas nilai bertag.
    ("tulis indeks atas bertag berisi int", "package main\npub fn main() {\n var a = []\n a = arr::push(a, 5)\n a = arr::push(a, \"s\")\n var e = a[0]\n wok(\"m\")\n e[0] = 1\n}",
     "m", "`int` tidak bisa ditulis lewat `[...]` — yang bisa cuma array, dict, dan struct"),
    ("kunci antara hilang di jalur tulis bertag", "package main\npub fn main() {\n var d = dict::new()\n d[\"k\"] = [1, \"dua\"]\n d[\"n\"] = 3\n wok(\"m\")\n d[\"q\"][1] = \"x\"\n}",
     "m", "key `q` belum ada — bikin dulu isinya sebelum menulis ke dalamnya"),
    ("field tuple bertag di luar batas", "package main\npub fn main() {\n var a = []\n a = arr::push(a, (\"x\", 1))\n a = arr::push(a, 5)\n let t = a[0]\n wok(\"m\")\n wok(t.5)\n}",
     "m", "index [5] di luar batas — tuple ini isinya 2 elemen"),
    // Fase AMAN-38 (#934): operator bit dan `math::max` atas nilai bertag gagal saat jalan berkalimat acuan.
    ("operator bit atas pecahan bertag", "package main\npub fn main() {\n let a = [2.5, 1]\n wok(\"m\")\n wok(a[0] >> a[1])\n}",
     "m", "tidak bisa `>>` antara `float` dan `int` — operator bit cuma untuk bilangan bulat"),
    ("math::max atas nilai bertag tak sebanding", "package main\npub fn main() {\n let a = [\"x\", 1]\n wok(\"m\")\n wok(math::max(a[0], a[1]))\n}",
     "m", "tidak bisa membandingkan `string` dengan `int`"),
    ("< array bertag tak sebanding", "package main\npub fn main() {\n var a = []\n a = [a, 1]\n var b = []\n b = [b, \"x\"]\n wok(\"m\")\n wok(a < b)\n}",
     "m", "tidak bisa membandingkan `int` dengan `string`"),
    // Fase AMAN-34 (#919): fungsi di dalam array bertag gagal saat dibandingkan, berlokasi seperti acuan.
    ("== atas array bertag berisi fungsi", "package main\nfn f(x) { return x }\npub fn main() {\n var a = []\n a = [a, 1]\n wok(\"mulai\")\n var b = []\n b = [b, f]\n var c = []\n c = [c, f]\n wok(b == c)\n}",
     "mulai", "tidak bisa membandingkan `function` dengan `function`"),
    ("pesan: teks panjang dipotong", "package main\npub fn main() {\n wok(\"a\")\n wok(int(str::repeat(\"7\", 100) + \"x\"))\n}", "a", "…77777777777x (101 karakter)` bukan bilangan bulat"),
    ("pesan: angka JSON raksasa dipotong", "package main\npub fn main() {\n wok(\"a\")\n wok(json::parse(str::repeat(\"9\", 200)))\n}", "a", "(200 karakter)` di luar jangkauan 128 bit"),
    ("let: pola tuple berliteral tidak cocok", "package main\npub fn main() {\n wok(\"a\")\n let (a, 5) = (1, 6)\n wok(a)\n}", "a", "nilai `let` bertipe `tuple` tidak cocok dengan polanya"),
    ("struct: kunci variabel ke field privat dari luar", "package main\nstruct P { pub x: int, rahasia: int = 7 }\npub fn main() { let p = P { x: 1 }\n var k = \"rahasia\"\n wok(\"a\")\n wok(p[k]) }", "a", "field `rahasia` di `P` bersifat private"),
    ("struct: kunci variabel yang tidak ada", "package main\nstruct P { pub x: int }\npub fn main() { let p = P { x: 1 }\n var k = \"zz\"\n wok(\"a\")\n wok(p[k]) }", "a", "field `zz` tidak ditemukan di struct `P` — field yang ada: x"),
    ("tuple: indeks variabel di luar batas", "package main\npub fn main() { let t = (1, 2)\n var i = 5\n wok(\"a\")\n wok(t[i]) }", "a", "index [5] di luar batas — tuple ini isinya 2 elemen"),
    ("tuple: indeks variabel pecahan", "package main\npub fn main() { let t = (1, \"x\")\n var i = 1.5\n wok(\"a\")\n wok(t[i]) }", "a", "tidak bisa index `tuple` dengan `float`"),
    ("potong: nilai bertag berisi bilangan", "package main\npub fn main() { let w = len(env::args()) < 99 $ 5 : \"x\"\n wok(\"a\")\n wok(w[0:1]) }", "a", "tidak bisa slice `int` — yang bisa dipotong"),
    // Fase AMAN-1 (#840): `env::set` di LLVM dulu membuang semua galat, dan NUL di jalur dipotong runtime C.
    ("env::set nama ber-`=`", "package main\npub fn main() { wok(\"a\")\n env::set(\"A=B\", \"1\") }", "a", "env::set: nama variabel \"A=B\" tidak sah — tidak boleh kosong, dan tidak boleh berisi `=` atau karakter NUL"),
    ("env::set nama ber-NUL", "package main\npub fn main() { wok(\"a\")\n env::set(\"A\" + str(rune(0)) + \"B\", \"1\") }", "a", "env::set: nama variabel \"A\\0B\" tidak sah"),
    ("env::set nilai ber-NUL", "package main\npub fn main() { wok(\"a\")\n env::set(\"JRG_UJI_NUL\", \"x\" + str(rune(0))) }", "a", "env::set: nilai untuk `JRG_UJI_NUL` berisi karakter NUL"),
    ("fs::read jalur ber-NUL", "package main\npub fn main() { wok(\"a\")\n wok(fs::read(\"Cargo.toml\" + str(rune(0)) + \"x\")) }", "a", "teksnya memuat karakter NUL, yang tidak bisa diserahkan ke sistem operasi"),
    // Fase BAGI-LLVM: jumlah elemen `range` dulu dihitung di 64 bit tanpa batas — `akhir - awal`
    // meluap dan `sdiv` atas MIN dan -1 menjawab panjang karangan; sekarang batas acuan.
    ("range: jumlah di atas batas elemen", "package main\npub fn main() { wok(\"a\")\n let m = -9223372036854775807 - 1\n var s = -1\n wok(len(range(0, m, s))) }", "a", "range: minta 9223372036854775808 elemen sekaligus, batasnya 16777216"),
    ("range: langkah nol dari variabel", "package main\npub fn main() { wok(\"a\")\n var s = 0\n wok(range(1, 5, s)) }", "a", "range: langkah tidak boleh 0 — urutannya tidak akan pernah sampai ke akhir"),
    // Fase POTONG: dulu ditolak LLVM saat kompilasi dengan kalimat sendiri.
    ("potong: atas bilangan", "package main\npub fn main() { let x = 5\n wok(\"a\")\n wok(x[0:1]) }", "a", "tidak bisa slice `int` — yang bisa dipotong: array, string, tuple"),
    ("potong: atas dict", "package main\npub fn main() { let d = {\"k\": 1}\n wok(\"a\")\n wok(d[0:1]) }", "a", "tidak bisa slice `dict` — yang bisa dipotong"),
    ("potong: batas awal pecahan", "package main\npub fn main() { let a = [1, 2, 3]\n wok(\"a\")\n wok(a[1.5:2]) }", "a", "batas awal potongan harus bilangan bulat, dapat `float` — tulis `x as int`"),
    ("potong: batas akhir pecahan atas teks", "package main\npub fn main() { let s = \"abc\"\n wok(\"a\")\n wok(s[0:2.0]) }", "a", "batas akhir potongan harus bilangan bulat, dapat `float`"),
    ("potong: batas teks sesudah awal di luar jangkauan", "package main\npub fn main() { let a = [1, 2, 3]\n wok(\"a\")\n wok(a[9:\"x\"]) }", "a", "batas awal potongan [9] di luar jangkauan"),
    ("potong: batas bool", "package main\npub fn main() { let a = [1, 2, 3]\n wok(\"a\")\n wok(a[true:2]) }", "a", "batas awal potongan harus bilangan bulat, dapat `bool`"),
    ("potong: tuple di luar jangkauan", "package main\npub fn main() { let t = (1, 2, 3)\n wok(\"a\")\n wok(t[1:9]) }", "a", "batas akhir potongan [9] di luar jangkauan — panjangnya 3"),
    ("potong: tuple terbalik", "package main\npub fn main() { let t = (1, 2, 3)\n wok(\"a\")\n wok(t[2:1]) }", "a", "potongan [2:1] terbalik"),
    ("indeks: tuple konstan di luar batas", "package main\npub fn main() { let t = (1, 2)\n wok(\"a\")\n wok(t[5]) }", "a", "index [5] di luar batas — tuple ini isinya 2 elemen"),
    // Fase LOKASI-LLVM: kalimat `assert`/`assert_eq` acuan — dulu "assertion failed: `left == right`",
    // argumen ketiga `assert_eq` dibuang, dan dua array beda panjang ditolak saat kompilasi.
    ("assert tanpa pesan", "package main\npub fn main() { wok(\"mulai\")\n assert(1 == 2) }",
     "mulai", "assert gagal — kondisinya tidak terpenuhi"),
    ("assert berpesan", "package main\npub fn main() { wok(\"mulai\")\n assert(1 == 2, \"harus sama\") }",
     "mulai", "assert gagal — harus sama"),
    ("assert_eq bilangan", "package main\npub fn main() { wok(\"mulai\")\n assert_eq(1, 2) }",
     "mulai", "assert_eq gagal — kiri `1` (int), kanan `2` (int)"),
    ("assert_eq argumen ketiga", "package main\npub fn main() { wok(\"mulai\")\n assert_eq(1.5, 2.5, \"catatan\") }",
     "mulai", "kanan `2.5` (float) — catatan"),
    ("assert_eq array beda panjang", "package main\npub fn main() { wok(\"mulai\")\n assert_eq([1, 2], [1]) }",
     "mulai", "assert_eq gagal — kiri `[1, 2]` (array), kanan `[1]` (array)"),
    ("assert_eq nilai panjang dipotong", "package main\npub fn main() { wok(\"mulai\")\n assert_eq(str::repeat(\"x\", 80), \"y\") }",
     "mulai", "… (80 karakter)"),
    // Fase ARGUMEN-METODE: jumlah argumen method bawaan yang salah — dulu kurang ditolak saat
    // kompilasi dengan kalimat sendiri, dan yang LEBIH dibuang diam-diam (`s.upper(1)` → `A`).
    ("method string kelebihan argumen, sesudah argumennya", "package main\nfn g() -> int { wok(\"g\"); return 1 }\npub fn main() { wok(\"mulai\")\n let s = \"a\"\n wok(s.upper(g())) }",
     "mulai\ng", "method `upper` butuh 0 argumen, dapat 1"),
    ("method string kurang argumen", "package main\npub fn main() { wok(\"x\")\n wok(\"a-b\".split()) }",
     "x", "method `split` butuh 1 argumen (pemisah), dapat 0"),
    ("method array kelebihan argumen", "package main\npub fn main() { wok(\"x\")\n let a = [1, 2]\n wok(a.len(3)) }",
     "x", "method `len` butuh 0 argumen, dapat 1"),
    ("method dict kelebihan argumen", "package main\npub fn main() { wok(\"x\")\n let d = {\"a\": 1}\n wok(d.keys(1)) }",
     "x", "method `keys` butuh 0 argumen, dapat 1"),
    ("method Option kelebihan argumen", "package main\npub fn main() { wok(\"x\")\n let o = Some(1)\n wok(o.unwrap(1)) }",
     "x", "method `unwrap` butuh 0 argumen, dapat 1"),
    ("defer method kelebihan argumen gagal saat jalan", "package main\nfn g() -> int { wok(\"g\"); return 1 }\npub fn main() {\n let s = \"a\"\n defer s.upper(g())\n wok(\"sesudah\") }",
     "g\nsesudah", "method `upper` butuh 0 argumen, dapat 1"),
    // Fase PTR-LLVM: yang bukan pegangan `jawa::` gagal saat jalan, sesudah semua argumen dievaluasi.
    ("jawa::get atas nilai bertag berisi teks", "package main\nfn pilih(n: int) { if n > 0 { return jawa::alloc(1) } return \"x\" }\npub fn main() { wok(\"sebelum\")\n jawa::get(pilih(0)) }",
     "sebelum", "jawa::get: butuh Ptr (dari jawa::alloc/array/buffer), dapat `string`"),
    ("jawa::set atas teks sesudah argumennya", "package main\nfn f() -> int { wok(\"f jalan\"); 3 }\npub fn main() { wok(\"sebelum\")\n jawa::set(\"x\", f()) }",
     "sebelum\nf jalan", "jawa::set: butuh Ptr (dari jawa::alloc/array/buffer), dapat `string`"),
    ("jawir::free atas teks", "package main\npub fn main() { wok(\"sebelum\")\n jawir(\"x\") }",
     "sebelum", "jawir::free: butuh Ptr (dari jawa::alloc/array/buffer), dapat `string`"),
    ("jawa::get atas Ptr milik ptr::", "package main\npub fn main() { wok(\"sebelum\")\n let p: Ptr = ptr::new(1)\n wok(jawa::get(p)) }",
     "sebelum", "jawa::get: handle tidak valid (field `kind` hilang)"),
    // Fase PEGANGAN-BERTAG: nilai bertag berisi jenis lain di tempat pegangan.
    ("Atomic dari nilai bertag berisi teks", "package main\nfn pilih(b: bool) { if b { return Atomic::new(5) } return \"teks\" }\npub fn main() { let x = pilih(false)\n wok(\"sebelum\")\n wok(atomic::get(x)) }",
     "sebelum", "`atomic::get` butuh sebuah Atomic (buat dengan `Atomic::new`), dapat `string`"),
    ("Mutex dari nilai bertag berisi Atomic", "package main\nfn pilih(b: bool) { if b { return Mutex::new() } return Atomic::new(1) }\npub fn main() { let x = pilih(false)\n wok(\"sebelum\")\n mutex::lock(x) }",
     "sebelum", "`mutex::lock` butuh sebuah Mutex (buat dengan `Mutex::new`), dapat `atomic`"),
    // Fase BULAT-2: pecahan di tempat bulat yang kalimat acuannya bukan `pesan_bukan_bulat`.
    ("for atas range berpecahan", "package main\npub fn main() { wok(\"sebelum\")\n for i in range(0, 2.5) { wok(i) } }",
     "sebelum", "range: argumen ke-2 butuh bilangan bulat, dapat pecahan `2.5`"),
    ("future::timeout dengan batas pecahan", "package main\npub fn main() { let f = Future::new(|| 1)\n wok(\"sebelum\")\n wok(future::timeout(f, 2.5)) }",
     "sebelum", "future::timeout: argumen 2 harus int >= 0 (milidetik), dapat `float`"),
    ("fs::chmod dengan mode pecahan", "package main\npub fn main() { wok(\"sebelum\")\n fs::chmod(\"/tmp/jrg-tidak-ada-chmod\", 4.5) }",
     "sebelum", "argumen tidak cocok untuk `fs::chmod` — dapat (string, float), bentuk yang diterima: fs::chmod(path: string, mode: int)"),
    ("builtin jembatan dengan argumen berjenis salah", "package main\npub fn main() { wok(\"sebelum\")\n wok(crypto::random_int(1.5, 3)) }",
     "sebelum", "argumen tidak cocok untuk `crypto::random_int` — dapat (float, int), bentuk yang diterima: crypto::random_int(min, max)"),
    // Fase ATOM-LLVM: jenis argumen `Atomic`/`Mutex` yang salah dulu ditolak LLVM saat KOMPILASI.
    ("Atomic::new atas pecahan", "package main\npub fn main() { wok(\"sebelum\")\n let a = Atomic::new(1.5) }",
     "sebelum", "`Atomic::new` butuh bilangan bulat, dapat `float` — Atomic cuma menyimpan `int`"),
    ("atomic::add dengan pecahan yang dievaluasi dulu", "package main\nfn bikin() -> float { wok(\"dievaluasi\")\n return 1.5 }\npub fn main() { let a = Atomic::new(1)\n wok(\"sebelum\")\n atomic::add(a, bikin()) }",
     "sebelum\ndievaluasi", "`atomic::add` butuh bilangan bulat, dapat `float` — Atomic cuma menyimpan `int`"),
    ("atomic::add atas nilai bertag berisi teks", "package main\npub fn main() { let xs = [1, \"a\"]\n let a = Atomic::new(1)\n wok(atomic::add(a, xs[0]))\n atomic::add(a, xs[1]) }",
     "2", "`atomic::add` butuh bilangan bulat, dapat `string` — Atomic cuma menyimpan `int`"),
    ("atomic::get atas bukan Atomic", "package main\npub fn main() { wok(\"sebelum\")\n wok(atomic::get(3)) }",
     "sebelum", "`atomic::get` butuh sebuah Atomic (buat dengan `Atomic::new`), dapat `int`"),
    ("mutex::lock atas Atomic", "package main\npub fn main() { let a = Atomic::new(1)\n wok(\"sebelum\")\n mutex::lock(a) }",
     "sebelum", "`mutex::lock` butuh sebuah Mutex (buat dengan `Mutex::new`), dapat `atomic`"),
    ("mutex::with_lock dengan bukan fungsi", "package main\npub fn main() { let m = Mutex::new()\n wok(\"sebelum\")\n mutex::with_lock(m, 5) }",
     "sebelum", "nilai bertipe `int` bukan fungsi, jadi tidak bisa dipanggil"),
    // Fase POLIMORF: tipe isi yang tidak punya method-nya gagal dengan kalimat acuan, field yang ada ikut disebut.
    ("polimorfisme: struct tanpa method itu", "package main\nstruct A { x: int }\nstruct B { y: int }\nimpl A { fn f(self) -> int { return self.x } }\npub fn main() { let xs = [A { x: 1 }, B { y: 2 }]\n wok(\"sebelum\")\n for _, v in xs { wok(v.f()) } }",
     "sebelum\n1", "`struct B` tidak punya method atau field bernama `f` (kalau maksudnya field/key, yang ada: y)"),
    // Fase FFI-LLVM: `?` di `main` berhenti dengan kalimat yang sama (`pesan_main_gagal`), `defer` jalan.
    ("`?` atas Err di main", "package main\npub fn main() { defer wok(\"tunda\")\n wok(\"sebelum\")\n let x = Result::Err([1, \"a\"])?\n wok(x) }",
     "sebelum\ntunda", "program berhenti karena `Result::Err` merambat keluar dari `main`: [1, a]"),
    ("`?` atas None di main", "package main\npub fn main() { wok(\"sebelum\")\n let x = [].first()?\n wok(x) }",
     "sebelum", "program berhenti karena `Option::None` merambat keluar dari `main` — tangani"),
    ("main beranotasi Result yang pulang Err", "package main\nstruct G { pub k: int }\npub fn main() -> Result<(), G> { defer wok(\"tunda\")\n wok(\"sebelum\")\n Err(G { k: 4 }) }",
     "sebelum\ntunda", "program berhenti karena `Result::Err` merambat keluar dari `main`: G {k: 4}"),
    // Fase MAIN-HASIL: `for` atas yang tak bisa diiterasi dulu ditolak saat kompilasi berkalimat sendiri.
    ("for atas channel", "package main\npub fn main() { let c = make_chan(int, 1)\n wok(\"sebelum\")\n for v in c { wok(v) } }",
     "sebelum", "for-in: tidak bisa iterate `channel` — yang bisa: array, string, dict"),
    ("for atas struct", "package main\nstruct P { x: int }\npub fn main() { wok(\"sebelum\")\n let p = P { x: 1 }\n for v in p { wok(v) } }",
     "sebelum", "for-in: `P` itu struct, bukan koleksi"),
    ("main beranotasi Option yang pulang None", "package main\npub fn main() -> Option<int> { wok(\"sebelum\")\n return None }",
     "sebelum", "program berhenti karena `Option::None` merambat keluar dari `main` — tangani"),
    ("spec desimal atas nilai bertag yang bukan angka", "package main\npub fn main() { let d = {\"a\": 1, \"b\": \"t\"}\n wok(\"sebelum\")\n let k = \"b\"\n wok(str::format(\"{:.2}\", d[k])) }",
     "sebelum", "str::format: spec `{:.2}` cuma untuk angka, dapat `string`"),
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
    // Fase AMAN-13 (#881–#882): `str::repeat`/`*` negatif dulu menjawab teks kosong di LLVM, dan
    // batas 256 MiB serta batas elemen `arr::fill`/`arr::repeat` tidak ditegakkan di sana.
    (
        "str::repeat: jumlah negatif",
        "package main\npub fn main() { wok(\"sebelum\")\n let n = 0 - 2\n wok(str::repeat(\"ab\", n)) }",
        "sebelum",
        "tidak bisa mengulang string sebanyak -2 kali — jumlah pengulangan tidak boleh negatif",
    ),
    (
        "str * n: jumlah negatif",
        "package main\npub fn main() { wok(\"sebelum\")\n let n = 0 - 3\n wok(\"ab\" * n) }",
        "sebelum",
        "tidak bisa mengulang string sebanyak -3 kali — jumlah pengulangan tidak boleh negatif",
    ),
    (
        "str::repeat: hasil di atas 256 MiB",
        "package main\npub fn main() { wok(\"sebelum\")\n let n = 200000000\n wok(len(str::repeat(\"aé\", n))) }",
        "sebelum",
        "hasil pengulangan kegedean: 2 karakter x 200000000 kali = sekitar 600000000 byte (batasnya 256 MiB)",
    ),
    (
        "arr::fill: di atas batas elemen",
        "package main\npub fn main() { wok(\"sebelum\")\n let n = 20000000\n wok(len(arr::fill(0, n))) }",
        "sebelum",
        "arr::fill: minta 20000000 elemen sekaligus, batasnya 16777216",
    ),
    (
        "arr::repeat: total di atas batas elemen",
        "package main\npub fn main() { wok(\"sebelum\")\n let n = 10000000\n wok(len(arr::repeat([1, 2], n))) }",
        "sebelum",
        "arr::repeat: minta 20000000 elemen sekaligus, batasnya 16777216",
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
        "fs::read: berkas bukan UTF-8",
        "package main\npub fn main() { let f = fs::join(fs::temp_dir(), \"jrg-l3-utf8-a.bin\")\n fs::write_base64(f, \"//4=\")\n wok(\"sebelum\")\n defer fs::delete(f)\n wok(fs::read(f)) }",
        "sebelum",
        "isinya bukan teks UTF-8 yang sah",
    ),
    (
        "fs::read_lines: berkas bukan UTF-8",
        "package main\npub fn main() { let f = fs::join(fs::temp_dir(), \"jrg-l3-utf8-b.bin\")\n fs::write_base64(f, \"QYA=\")\n wok(\"sebelum\")\n defer fs::delete(f)\n wok(fs::read_lines(f)) }",
        "sebelum",
        "isinya bukan teks UTF-8 yang sah",
    ),
    // Fase UTF8-LLVM: kalimat batas potongan sama dengan acuan — awal, akhir, lalu urutan.
    (
        "potongan: batas akhir di luar",
        "package main\npub fn main() { let a = [1, 2, 3]\n wok(\"sebelum\")\n wok(a[1:99]) }",
        "sebelum",
        "batas akhir potongan [99] di luar jangkauan — panjangnya 3, jadi batas yang sah 0..3 (atau -1..-3 kalau dihitung dari belakang)",
    ),
    (
        "potongan: batas awal negatif di luar",
        "package main\npub fn main() { let s = \"halo\"\n wok(\"sebelum\")\n wok(s[-9:99]) }",
        "sebelum",
        "batas awal potongan [-9] di luar jangkauan — panjangnya 4, jadi batas yang sah 0..4 (atau -1..-4 kalau dihitung dari belakang)",
    ),
    (
        "potongan: terbalik sesudah normalisasi",
        "package main\npub fn main() { let a = [1, 2, 3, 4]\n wok(\"sebelum\")\n wok(a[-1:1]) }",
        "sebelum",
        "potongan [3:1] terbalik — batas awal (3) lebih besar dari batas akhir (1); tulis yang kecil dulu, mis. [1:3]",
    ),
    (
        "dict: kunci yang hilang menyebut kunci yang ada",
        "package main\npub fn main() { let d = {\"b\": 1, \"a\": 2}\n wok(\"sebelum\")\n wok(d[\"zz\"]) }",
        "sebelum",
        "key `zz` tidak ada di dict ini — key yang ada: a, b",
    ),
    (
        "dict: kunci hilang di dict lebih dari 12 kunci",
        "package main\npub fn main() { var d = {\"k00\": 0}\n for i in 1..15 { d[«k${i}»] = i }\n wok(\"sebelum\")\n wok(d[\"q\"]) }",
        "sebelum",
        "dan 3 lagi",
    ),
    // Fase KUNCI-LLVM (#824): field dibaca dan ditulis atas nilai yang bukan struct.
    (
        "field: dibaca atas int",
        "package main\npub fn main() { let x = 5\n wok(\"a\")\n wok(x.foo) }",
        "a",
        "`int` tidak punya method atau field bernama `foo` — cek lagi penulisannya ya",
    ),
    (
        "field: method teks tanpa kurung",
        "package main\npub fn main() { let s = \"ab\"\n wok(s.upper) }",
        "",
        "`upper` itu method milik `string`, dan di sini ditulis tanpa `()`",
    ),
    (
        "field: indeks tuple di luar batas",
        "package main\npub fn main() { let t = (1, 2)\n wok(t.5) }",
        "",
        "index [5] di luar batas — tuple ini isinya 2 elemen",
    ),
    (
        "field: dibaca atas Option",
        "package main\npub fn main() { let o = Some(1)\n wok(o.x) }",
        "",
        "`enum:Option` tidak punya method atau field bernama `x` — cek lagi penulisannya ya",
    ),
    (
        "field: ditulis atas float sesudah sisi kanan",
        "package main\npub fn main() { var f = 1.5\n wok(\"a\")\n f.k = wok(\"rhs\") }",
        "a\nrhs",
        "`float` tidak punya field — `.k` cuma bisa ditulis ke struct atau dict",
    ),
    (
        "field: ditulis atas array",
        "package main\npub fn main() { var a = [1, 2]\n a.n = 3 }",
        "",
        "`array` tidak punya field — `.n` cuma bisa ditulis ke struct atau dict",
    ),
    (
        "indeks: teks atas struct tanpa field itu",
        "package main\nstruct P { pub x: int }\npub fn main() { let p = P { x: 1 }\n wok(\"a\")\n wok(p[\"zz\"]) }",
        "a",
        "field `zz` tidak ditemukan di struct `P` — field yang ada: x",
    ),
    // Fase KUNCI-LLVM (#825): indeks bertipe salah dan wadah yang tak bisa di-index, baca dan tulis.
    (
        "indeks: bool atas array",
        "package main\npub fn main() { wok(\"a\")\n wok([1, 2][true]) }",
        "a",
        "tidak bisa index `array` dengan `bool`",
    ),
    (
        "indeks: pecahan atas string",
        "package main\npub fn main() { wok(\"a\")\n wok(\"ab\"[1.0]) }",
        "a",
        "tidak bisa index `string` dengan `float`",
    ),
    (
        "indeks: atas int sesudah indeksnya",
        "package main\npub fn main() { let x = 5\n wok(x[wok(\"i\")]) }",
        "i",
        "tidak bisa index `int` — yang bisa di-index cuma array, string, tuple, dict dan struct",
    ),
    (
        "indeks: atas Option",
        "package main\npub fn main() { let o = Some(1)\n wok(o[0]) }",
        "",
        "tidak bisa index `enum:Option`",
    ),
    (
        "indeks: ditulis atas int sesudah sisi kanan dan indeksnya",
        "package main\npub fn main() { var x = 5\n x[wok(\"i\")] = wok(\"r\") }",
        "r\ni",
        "`int` tidak bisa ditulis lewat `[...]` — yang bisa cuma array, dict, dan struct",
    ),
    (
        "indeks: ditulis atas string",
        "package main\npub fn main() { var s = \"ab\"\n s[0] = \"x\" }",
        "",
        "`string` tidak bisa ditulis lewat `[...]`",
    ),
    (
        "indeks: ditulis ke array dengan teks",
        "package main\npub fn main() { var a = [1, 2]\n wok(\"a\")\n a[\"k\"] = 3 }",
        "a",
        "tidak bisa index `array` dengan `string`",
    ),
    (
        "indeks: ditulis bersarang atas elemen int",
        "package main\npub fn main() { var a = [1]\n a[0][1] = wok(\"r\") }",
        "r",
        "`int` tidak bisa ditulis lewat `[...]`",
    ),
    // Fase KUNCI-LLVM (#823): skalar, tuple, struct, enum, Option dan Result tanpa method itu.
    (
        "method: tidak ada atas int, sesudah argumennya",
        "package main\npub fn main() { let x = 5\n wok(x.uper(wok(\"arg\"))) }",
        "arg",
        "`int` tidak punya method atau field bernama `uper` — cek lagi penulisannya ya",
    ),
    (
        "method: math:: atas int menyebut jalan keluarnya",
        "package main\npub fn main() { let x = 5\n wok(x.sqrt()) }",
        "",
        "`int` tidak punya method atau field bernama `sqrt`",
    ),
    (
        "method: tidak ada atas tuple",
        "package main\npub fn main() { let t = (1, 2)\n wok(t.len()) }",
        "",
        "`tuple` tidak punya method atau field bernama `len` (yang ada untuk tuple cuma indeks numerik: `t.0`, `t.1`, ...)",
    ),
    (
        "method: tidak ada atas struct, saran dari method warisan",
        "package main\nstruct A { nama: string }\nimpl A { fn sapa(self) -> string { self.nama } }\nstruct B: A { umur: int }\npub fn main() { let b = B { nama: \"x\", umur: 2 }\n wok(b.sapa())\n wok(b.spa()) }",
        "x",
        "`struct B` tidak punya method atau field bernama `spa` (kalau maksudnya field/key, yang ada: nama, umur) — maksudnya `sapa`?",
    ),
    (
        "method: tidak ada atas enum",
        "package main\nenum E { A, B }\npub fn main() { let e = E::A\n wok(e.nama()) }",
        "",
        "`enum:E` tidak punya method atau field bernama `nama` — cek lagi penulisannya ya",
    ),
    (
        "method: tidak ada atas Option dan Result",
        "package main\npub fn main() { let r: Result<int, string> = Ok(1)\n wok(r.is_ok())\n let o = Some(3)\n wok(o.unwrp()) }",
        "true",
        "`enum:Option` tidak punya method atau field bernama `unwrp` — maksudnya `unwrap`?",
    ),
    // Fase KUNCI-LLVM (#822): method yang bukan method acuan gagal saat jalan, sesudah argumennya.
    (
        "method: tidak ada atas teks, sesudah argumennya",
        "package main\npub fn main() { let s = \"ab\"\n wok(s.uper(wok(\"arg\"))) }",
        "arg",
        "`string` tidak punya method atau field bernama `uper` — maksudnya `upper`?",
    ),
    (
        "method: tidak ada atas array",
        "package main\npub fn main() { let a = [1, 2]\n wok(a.lenn()) }",
        "",
        "`array` tidak punya method atau field bernama `lenn` — maksudnya `len`?",
    ),
    (
        "method: nama dict:: yang bukan method menyebut kunci yang ada",
        "package main\npub fn main() { let d = {\"new\": 1}\n wok(\"a\")\n wok(d.new(wok(\"arg\"))) }",
        "a\narg",
        "`dict` tidak punya method atau field bernama `new` (kalau maksudnya field/key, yang ada: new) — cek lagi penulisannya ya",
    ),
    (
        "method: str::join bukan method teks",
        "package main\npub fn main() { let s = \"-\"\n wok(s.join([\"a\", \"b\"])) }",
        "",
        "`string` tidak punya method atau field bernama `join` — cek lagi penulisannya ya",
    ),
    (
        "dict: field yang hilang lewat titik",
        "package main\npub fn main() { var d = {\"a\": 1}\n d[\"b\"] = \"t\"\n wok(d.b)\n wok(d.kyes) }",
        "t",
        "`dict` tidak punya method atau field bernama `kyes` (kalau maksudnya field/key, yang ada: a, b) — cek lagi penulisannya ya",
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
    // Fase SEBAR-LLVM: jumlah argumen spread diperiksa saat jalan, dengan kalimat acuan.
    (
        "spread kekurangan argumen ke fungsi bernama",
        "package main\nfn tambah(a: int, b: int) -> int { return a + b }\npub fn main() { var ys: []int = []\n ys = ys + [1]\n wok(\"sebelum\")\n wok(tambah(...ys)) }",
        "sebelum",
        "fungsi `tambah`: parameter `b` (posisi 2) wajib diisi",
    ),
    (
        "spread kebanyakan argumen ke fungsi bernama",
        "package main\nfn tambah(a: int, b: int) -> int { return a + b }\npub fn main() { var ys: []int = []\n for i in 0..3 { ys = ys + [i] }\n wok(\"sebelum\")\n wok(tambah(...ys)) }",
        "sebelum",
        "fungsi `tambah` kebanyakan argumen — dia cuma menerima 2, tapi dikasih 3",
    ),
    // Fase BULAT-LLVM: pecahan di tempat bulat builtin dulu ditolak LLVM saat kompilasi dengan kalimatnya sendiri.
    (
        "pecahan di indeks method char_at gagal saat jalan",
        "package main\npub fn main() { wok(\"sebelum\")\n wok(\"ab\".char_at(1.0)) }",
        "sebelum",
        "str::char_at: butuh bilangan bulat, dapat pecahan `1.0` — kalau memang mau dipotong, tulis eksplisit: `1.0 as int`",
    ),
    (
        "pecahan dari fungsi di argumen substr: argumennya dievaluasi dulu",
        "package main\nfn bikin() -> float { wok(\"dievaluasi\")\n return 2.0 }\npub fn main() { wok(\"sebelum\")\n wok(str::substr(\"abc\", bikin(), 1)) }",
        "sebelum\ndievaluasi",
        "str::substr: butuh bilangan bulat, dapat pecahan `2.0`",
    ),
    (
        "pecahan di range menyebut nomor argumennya",
        "package main\npub fn main() { let x = 2.5\n wok(\"sebelum\")\n wok(range(1, x)) }",
        "sebelum",
        "range: argumen ke-2 butuh bilangan bulat, dapat pecahan `2.5`",
    ),
    (
        "pecahan di indeks jawa::buffer_get bukan soal isi buffer",
        "package main\npub fn main() { let b = jawa::buffer(3)\n wok(\"sebelum\")\n wok(jawa::buffer_get(b, 1.5)) }",
        "sebelum",
        "jawa::buffer_get: butuh bilangan bulat, dapat pecahan `1.5`",
    ),
    // Fase TUMPUK: stack yang habis dulu membuang `defer` dan menggugurkan Future di LLVM.
    (
        "stack habis: defer tetap jalan, terdalam dulu",
        "package main\nfn f(n: int) -> int { return f(n + 1) + f(n + 2) }\nfn beres(x: int) { wok(x) }\npub fn main() { defer beres(1)\n defer beres(2)\n wok(\"mulai\")\n wok(f(0)) }",
        "mulai\n2\n1",
        "rekursi terlalu dalam",
    ),
    (
        "stack habis di badan Future: galatnya sampai ke yang menunggu",
        "package main\nfn f(n: int) -> int { return f(n + 1) + f(n + 2) }\npub fn main() { wok(\"mulai\")\n let t = Future::new(|| f(0))\n wok(future::await(t)) }",
        "mulai",
        "Future gagal: rekursi terlalu dalam",
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
    // Fase TAMAT (#833): dua mesin acuan dulu menunggu goroutine yang terblokir.
    (
        "main gagal tidak menunggu goroutine yang terblokir",
        "package main\npub fn main() {\n    let c = make_chan(int)\n    spawn { c <- 1 }\n    wok(\"sebelum\")\n    let a = [1]\n    wok(a[5])\n}",
        "sebelum",
        "index [5] di luar batas",
    ),
    // Fase HOF-LLVM (#832): dulu ditolak saat kompilasi dengan kalimat sendiri.
    (
        "hof: yang diserahkan bukan fungsi",
        "package main\npub fn main() {\n    wok(\"sebelum\")\n    wok([1, 2].reduce(0, 1))\n}",
        "sebelum",
        "nilai bertipe `int` bukan fungsi, jadi tidak bisa dipanggil",
    ),
    (
        "hof: closure kurang parameter",
        "package main\npub fn main() {\n    wok(\"sebelum\")\n    wok([1].map(|| 5))\n}",
        "sebelum",
        "fungsi `<closure>` kebanyakan argumen — dia cuma menerima 0, tapi dikasih 1.",
    ),
    (
        "hof: fungsi bernama kelebihan parameter",
        "package main\nfn dua(a: int, b: int) -> int { a + b }\npub fn main() {\n    wok(\"sebelum\")\n    wok([1].map(dua))\n}",
        "sebelum",
        "fungsi `dua`: parameter `b` (posisi 2) wajib diisi",
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
    (
        "handle: struct Ptr milik pengguna ditolak jawa::get",
        r#"package main
struct Ptr { pub id: int, pub kind: string }
pub fn main() {
    let asli = jawa::alloc(99)
    let palsu = Ptr { id: 1, kind: "single" }
    wok("sebelum", jawa::get(asli))
    wok(jawa::get(palsu))
}"#,
        "sebelum 99",
        "jawa::get: butuh Ptr (dari jawa::alloc/array/buffer), dapat `struct:Ptr`",
    ),
    (
        "handle: menulis field .id pada handle asli tetap ditolak walau ada struct Ptr pengguna",
        r#"package main
struct Ptr { pub id: int, pub kind: string }
pub fn main() {
    let _p = Ptr { id: 1, kind: "single" }
    var asli = jawa::alloc(99)
    wok("sebelum")
    asli.id = 2
}"#,
        "sebelum",
        "`Ptr` adalah handle buatan pustaka — field `id` tidak bisa ditulis",
    ),
];
