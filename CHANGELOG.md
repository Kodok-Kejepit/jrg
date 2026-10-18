# Riwayat perubahan

Format mengikuti [Keep a Changelog](https://keepachangelog.com/id/1.1.0/), dan penomorannya mengikuti
[Semantic Versioning](https://semver.org/lang/id/). Selama versi `0.x`, perubahan yang memutus program lama masih
mungkin terjadi; semuanya dicatat di bagian **Memutus**.

## [Belum dirilis]

## [0.2.0] — 2026-10-18

Rilis kedua. Backend LLVM sekarang menjalankan konkurensi, modul, dan sebagian besar pustaka; `jrgm` jadi pengelola
proyek yang lengkap; program JRG bisa dibungkus jadi aplikasi Android; dan banyak aturan bahasa diperketat supaya
tidak ada lagi jawaban salah yang diam.

### Memutus

Program yang ditulis untuk 0.1.0 mungkin perlu diubah di tempat-tempat ini.

- **Penugasan butuh nama yang sudah dideklarasikan.** `x = 5` atas nama yang belum ada dulu diam-diam membuat
  variabel; sekarang galat. Tulis `var x = 5` atau `x := 5`. Menugasi fungsi atau fungsi bawaan (`len = 5`) juga
  ditolak.
- **Ruang nama modul per berkas.** Sebuah berkas cuma melihat modul yang ia import sendiri, `var` tingkat modul tidak
  lagi diekspor (sediakan fungsi pembaca), dan nama berawalan `_` privat.
- **Pencarian menjawab `Option`, tidak lagi `-1` atau `nil`:** `arr::find`, `arr::find_idx`, `arr::index_of`,
  `arr::binary_search`, `str::find`, `str::index_of`, `re::find`, `json::get`, dan `env::get`. Tambahkan `?? cadangan`,
  `.unwrap_or(...)`, atau `match`.
- **`http::get`, `post`, `put`, `delete`, `request` menjawab `Result`.** Dulu jawaban berhasil berupa dict telanjang.
- **Tidak ada pemotongan diam-diam.** Pecahan atau `bool` di tempat bilangan bulat (`range(0, 2.5)`,
  `math::gcd(1.1, 7)`) ditolak; tulis `x as int`. Konversi yang tidak muat (`300 as u8`, `-5 as uint`) ditolak.
  `65 as byte` sekarang `65`, bukan rune `A`.
- **`as` lebih longgar dari minus**, seperti Rust: `-128 as i8` berarti `(-128) as i8`. Sebaliknya `**` lebih erat:
  `-2 ** 2` sekarang `-4`.
- **Argumen pertama `str::` harus teks.** `str::upper(nil)` dan `str::is_empty([])` ditolak; tulis `str(x)` dulu.
  Pola di method teks (`"a1".contains(1)`) juga harus teks.
- **Jumlah argumen diperiksa** di semua fungsi dan method bawaan: kelebihan maupun kekurangan ditolak.
- **`a.each(f)` menjawab `()`, `a.group_by(f)` menjawab dict**, sama dengan bentuk `arr::`-nya.
- **JSON untuk enum** memakai variannya sebagai kunci: `Some(3)` jadi `{"Some":3}`, `None` jadi `"None"`. Bentuk yang
  sama dipakai saat enum dikirim ke `py::`, `js::`, dan `php::`.
- **`sleep(n)` berarti `n` detik**, sama dengan `time::sleep`.
- **`exit(n)` cuma menerima 0–255.**
- **Field struct tanpa nilai bawaan wajib diisi** di literal.
- **`Mutex::new()` tidak menerima nilai**; ia kunci murni. Pakai `mutex::with_lock(m, f)`.
- **Fungsi yang tidak menjawab apa pun** (`fs::write`, `ptr::set`, `dok::log_*`, …) menjawab `()`, bukan `nil`.
- **Reduksi `wokwok::` tanpa sumbu** (`sum`, `mean`, `max`, …) menjawab angka biasa, bukan larik 0-dimensi.
- **`fn f(a, b = 1, c)` ditolak**: parameter wajib tidak boleh sesudah parameter bernilai bawaan.
- **`_` tidak bisa dibaca**, dan sisi kiri `=` harus sebuah tempat (`f() = 2` ditolak saat parse).
- **Literal bilangan bulat di atas 128 bit** ditolak saat parse.
- **`Warna { .. }`** tanpa deklarasi sendiri sekarang struct `r`, `g`, `b`, `a` (bawaan 255). Field lain ditolak di
  tempatnya; dulu baru gagal di `canvas::`.
- **`dok::json_set` atas handle pustaka** (`dok::router_new()`, `Future`, …) ditolak.

### Baru

**Bahasa**

- `a, b = e` dan `a, b := e` membongkar tuple ke beberapa nama.
- Argumen bernama (`f(b: 2, a: 1)`), sebar di pemanggilan (`f(...v)`) dan di literal array (`[1, ...a]`), pola di
  parameter (`fn f((x, y))`) dan di kepala `for` (`for _, (a, b) in xs`).
- Konstruktor varian sebagai nilai: `xs.map(Some)`, `let f = E::A`.
- `var x: T` tanpa nilai awal mendapat nilai nol tipenya, dan `for i in 0..N` tidak lagi membuat array sebesar N.
- `::nama` selalu menunjuk fungsi bawaan, walau program punya fungsi bernama sama.
- Indeks negatif di `get`, `get_or`, `remove`, `insert`, `swap`, `char_at`, dan awal `str::substr`.
- `defer` tetap jalan saat fungsinya berhenti karena galat.
- Literal dict dengan kunci bilangan bulat (`{1: "a"}`).

**Pustaka**

- `Atomic` dan `Mutex` di interpreter dan VM, dengan bentuk method (`a.add(1)`, `m.with_lock(f)`).
- `std/teks`, `std/uang`, dan `std/tabel`; mesin game 2D `std/mesin` (12 modul).
- `suara::`: membuat, menyimpan (WAV), dan memutar bunyi.
- `term::`: mode mentah, membaca tombol (termasuk tetikus dan tempelan), ukuran layar, mode layar penuh.
- `lua::` dan `php::` (termasuk Composer dan templat PHP); `py::`, `js::`, `php::`, `lua::` sekarang juga jalan di
  `--vm`.
- HTTPS di klien HTTP, `http::unduh` ke berkas, dan badan `chunked`.
- Data biner sebagai array byte di jaringan, WebSocket, SQLite (BLOB), berkas, dan `crypto::`.
- Mode tampil kanvas: jendela desktop, peramban, atau berkas PNG per bingkai.
- Semua 818 fungsi bawaan punya penjelasan di `jrgc --doc`.

**Backend LLVM**

- Konkurensi (`spawn`, channel, `select`, `Future`, `Task`) sebagai korutin di satu thread; kebuntuan dilaporkan.
- `import` antar-berkas dan `std/`; mesin game `std/mesin` jalan di native.
- `dok::`, `crypto::`, `re::`, `ptr::`, `json::`, `fs::`, `Atomic`, `Mutex`, dan `extern "C"`.
- Struct generik, closure dan fungsi tanpa anotasi, fungsi sebagai nilai, nilai bertipe campur, dict sebagai tabel
  hash.
- Luapan bilangan bulat diperiksa; `math::wrap_add`, `wrap_sub`, `wrap_mul` untuk yang memang ingin membungkus.
- `.map`, `.map_err`, `.and_then` atas `Option` dan `Result`; konstruktor varian sebagai nilai; `Warna`;
  `import "std/fmt"`; `jawa::array(n, isi)`.

**Perkakas**

- `jrgm lint [--ketat]`, dependensi git (`jrgm add --git`, `jrgm.lock`, `jrgm update`), dan `jrgm publish`.
- `jrgm new --bahasa X` dan `jrgm exec`: proyek dan berkas bahasa lain (C, C++, Rust, Go, Python, …) lewat
  perkakasnya masing-masing.
- `jrgc --periksa` menemukan nama yang belum dideklarasikan tanpa menjalankan program; `JRG_GALAT_KE` menulis galat
  ke berkas untuk editor.
- Penyunting `jrgc --sunting`: banyak berkas, seleksi, cari dan ganti, jalankan dengan Ctrl+R lalu lompat ke galat.
- `jrgc --jrgm` (satu biner berperan sebagai `jrgm`).
- `jrgc --periksa` juga memeriksa jumlah argumen fungsi yang ditulis di berkas itu, dan `jrgm lint` memperingatkan
  variabel lokal yang tidak pernah dibaca.
- `jrgm.toml` boleh menulis array di beberapa baris.
- Biner siap pakai untuk Linux (x86_64, aarch64), macOS (Intel, Apple Silicon), Windows, dan Termux. `--emit-llvm`
  di biner rilis cukup butuh `clang`.

**Android dan JRG IDE**

- `jrgc --apk` dan `jrgm apk`: game kanvas (native) atau aplikasi web (dengan `jrgc` di dalamnya) jadi APK; `--rilis`
  dan `--aab` untuk Play Store; `jrgm siapkan android` memasang SDK dan NDK.
- Izin berdialog, getar, sentuhan dan tombol layar, suara lewat AAudio, ikon adaptif, layanan latar dengan notifikasi.
- Pustaka C, C++, Rust milik proyek dan penjalan Lua, QuickJS, SQLite bisa ikut di dalam APK.
- JRG IDE: lingkungan pengembangan di peramban dan Android, dengan terminal, panel Masalah, proyek `jrgm`, dan Ubuntu
  di dalam aplikasi untuk versi Terminal.

**Situs**

- Situs dokumentasi ditulis ulang, dan kelas Matematika × JRG memakai tema yang sama. Contoh kodenya bisa diubah dan
  dijalankan langsung di peramban, dan semuanya diperiksa terhadap `jrgc` sebelum terbit.

### Diperbaiki

- Puluhan kasus di mana interpreter dan VM menjawab berbeda untuk program yang sama, atau satu menerima dan yang lain
  menolak; tabel paritasnya sekarang kosong di kedua arah.
- Jawaban salah yang diam di backend LLVM: salinan dict yang berbagi isi, jenis nilai yang berganti diam-diam, luapan
  yang membungkus, potongan yang bisa ditulisi.
- `json::parse` atas angka besar, escape, dan teks tidak sah; galat berkas dan jaringan dalam bahasa Indonesia, tanpa
  kode OS.
- Server `dok::serve` tidak bisa lagi dimatikan oleh satu permintaan yang aneh.
- Pembandingan dua fungsi dengan `==` ditolak (dulu menjawab `false`).
- Di backend LLVM: `m[0] = m[0] + f()` yang mengganti `m` di dalam `f` tidak lagi kehilangan tulisannya, dan sisi
  kanan penugasan dihitung lebih dulu seperti di dua mesin lain; `main` yang punya `defer` menunggu goroutine-nya;
  nama yang tidak ada (`jawa::free`, `task::await`) ditolak seperti di interpreter.

### Kinerja

- Menambah elemen ke array atau dict tetap linear di semua mesin, termasuk lewat `var` modul, closure, dan field.
- `x = x.m(..)` dan `d = dict::set(d, k, v)` mengubah di tempat; membaca satu nilai tidak menyalin koleksinya.
- Putaran utama VM dan pemanggilan fungsi di interpreter lebih cepat.

## [0.1.0] — 2026-09-08

Rilis pertama. Bahasanya berjalan di tiga mesin eksekusi dengan keluaran yang
sama persis, diuji secara diferensial.

### Bahasa

- Tiga mesin: penerjemah pohon (acuan semantik), VM bytecode (`--vm`), dan
  kompilator native lewat LLVM (`--emit-llvm`).
- Multi-paradigma: prosedural, berorientasi objek (struct, trait, pewarisan
  tunggal, visibilitas field), fungsional (closure yang menangkap pengikatan,
  HOF, pencocokan pola dengan guard dan `nama @ pola`, `Option`/`Result`,
  operator `?` dan `??`), dan konkuren gaya CSP (`spawn`, channel, `select`).
- Semantik nilai: nilai tidak pernah beralias, penyalinan terjadi saat menulis.
  Tidak ada garbage collector.
- Operator bisa didefinisikan tipe (`add`, `cmp`, `eq`, dan sebagainya), dan
  `fn to_string(self)` dipakai oleh setiap jalur tampilan.
- Berkas tanpa `package` dijalankan sebagai skrip.

### Pustaka

- 658 builtin di 37 namespace.
- 11 modul pustaka standar yang ditulis dalam JRG: `set`, `queue`, `deque`,
  `heap`, `csv`, `path`, `functional`, `option`, `result`, `iterator`, `testing`.
- `dok::` — server HTTP dengan middleware, WebSocket, sesi bertanda tangan,
  templat yang meng-escape HTML secara bawaan, driver SQLite lewat `dlopen`.
- `canvas::` — kanvas 2D dengan anti-aliasing, transformasi, gradien, dan
  enkoder sekaligus dekoder PNG yang ditulis sendiri.
- `wokwok::` — pustaka numerik n-dimensi dengan buffer bertipe dan kontigu.

### FFI

- C dan C++ (mangling Itanium diturunkan dari keluaran `g++`, bukan dari
  spesifikasi), Rust dan Go (lewat `extern "C"`), Python, JavaScript, Java
  (JNI, indeksnya diturunkan dari `jni.h`), .NET (lewat hostfxr), dan
  WebAssembly (penerjemah yang ditulis sendiri, jawabannya diadu dengan
  `WebAssembly` milik Node).

### Perkakas

- `jrgc` — kompilator, runner, REPL, pencarian builtin (`--cari`, `--doc`).
- `jrgm` — scaffolding proyek, build, run, watch, test, fmt, dan dependensi
  lokal lewat path.
- Language server (hover, completion, definisi, simbol, diagnostik).

### Diagnostik

- Setiap galat membawa lokasi, cuplikan sumber, dan caret di kolom yang tepat —
  sama persis di penerjemah maupun VM.
- Galat runtime menyebut rantai pemanggilan yang membawanya.
- Nada diagnostik bisa dipilih (`--nada`), dan `--sopan` mematikan sapaannya
  untuk CI yang membandingkan keluaran.

### Diuji

- 471 tes Rust, nol gagal.
- Paritas diferensial penerjemah ↔ VM dan penerjemah ↔ LLVM.
- 11 berkas konformans, lulus di ketiga mesin dengan keluaran identik.
