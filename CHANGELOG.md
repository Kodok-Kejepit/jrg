# Riwayat perubahan

Format mengikuti [Keep a Changelog](https://keepachangelog.com/id/1.1.0/), dan penomorannya mengikuti
[Semantic Versioning](https://semver.org/lang/id/). Selama versi `0.x`, perubahan yang memutus program lama masih
mungkin terjadi; semuanya dicatat di bagian **Memutus**.

## [Belum dirilis]

## [0.6.1] — 2026-11-18

### Memutus

- **Pesan JRG tampil dalam bahasa Inggris secara bawaan** (galat, peringatan, laporan uji, REPL, `jrgm`, `--help`).
  Bahasa Indonesia tetap ada: `jrgm language id` (untuk semua jalan berikutnya), `jrgc --bahasa id`, atau
  `JRG_BAHASA=id`. Keluaran program tidak berubah; skrip yang membandingkan teks galat perlu memasang bahasanya.
  Biner `--emit-llvm` memilih bahasa galatnya sendiri saat jalan dengan aturan yang sama, dan `jrgm new` menulis
  kerangka proyek dalam bahasa pesan.
- **`proses::jalankan` dan `proses::keluaran` menghentikan program yang keluarannya melewati 256 MiB** dan menjawab
  `Err` (dulu `jrgc` mati kehabisan memori); penampung `proses::mulai` yang penuh menunggu dibaca.
- **`jrgm new` menolak folder yang sudah ada** (dulu `src/main.jrg` di dalamnya ditimpa templat) **dan nama di luar
  huruf, angka, `_`, `-`** (dengan jalur folder di depannya).
- **Dependensi git `jrgm` cuma lewat https, http, ssh, git dan berkas lokal**: URL `<nama>::...` dulu memanggil
  `git-remote-<nama>` dari PATH.
- **`tok()` gagal berkalimat untuk baris masukan di atas 256 MiB** (dulu `jrgc` mati kehabisan memori), dan di
  `--emit-llvm` menolak masukan bukan UTF-8 seperti dua mesin lain (dulu teks kosong).
- **`dok::tcp_recv_all` berhenti di 256 MiB** dan menjawab `Err` (dulu menampung sampai memori habis).
- **Klien `http::` menolak jawaban yang terpotong** (badan lebih pendek dari `Content-Length`) dan `Content-Length`
  yang tidak sah atau berbeda-beda; byte sesudah badan dibuang. HTTPS menuntut paling sedikit TLS 1.2.
- **`?` tidak lagi mengubah `None` menjadi `Err` (atau sebaliknya) diam-diam.** Di fungsi yang kembalian beranotasi
  `Result`, `x?` atas `Option` kosong dulu pulang dengan `None` apa adanya; sekarang gagal dengan pesan yang menyebut
  kedua jenisnya. Tulis `x.ok_or("alasan")?` untuk mengubahnya.
- **`var x: T` tanpa nilai awal untuk struct, enum atau `Option` gagal kalau dibaca sebelum diisi** (dulu `nil` diam-diam).
  Jenis yang punya nol jelas (`int`, `float`, `bool`, teks, larik, tuple) tetap berawal dari nolnya.

### Baru

- **`tangkap(f)` menangkap galat saat jalan** di tiga mesin: `Ok(nilai)`, atau `Err(Galat)` berisi `pesan`, `berkas`,
  `baris`, `kolom`. Semua galat ditangkap (juga stack habis) kecuali `exit()` dan memori habis; `defer` jalan dulu;
  `panik(g)` melempar ulang dengan tempat asalnya.

- **Pembaca berkas bertahap `fs::open`, `fs::read_line`, `fs::read_chunk`, `fs::close`**: untuk berkas di atas 256 MiB
  yang ditolak `fs::read`.
- **`http::tenggat(detik)`**: tenggat total tiap permintaan klien HTTP (bawaan mati), supaya server yang menetes
  jawabannya tidak bisa menahan pemanggil tanpa batas.
- **`wasm::batas_langkah(h, n)`**: `wasm::panggil` berhenti dengan `Err` sesudah `n` instruksi (bawaan tanpa batas),
  supaya modul yang tidak dipercaya tidak bisa menggantung pemanggilnya.
- **`dok::password_needs_rehash(hash, putaran?)`**: `true` kalau hash sandi dibuat dengan putaran lain dari bawaan
  (600000), supaya hash lama diringkas ulang sesudah login berhasil.
- **Situs dokumentasi dua bahasa**: bahasa Inggris jadi bawaan, versi Indonesia di `/id/`, dengan tombol EN/ID di
  tiap halaman. Halaman mesin menjelaskan mesin kelima (WebAssembly) yang sedang dibuat.
- **Saluran WhatsApp resmi "JRG — Official Channel"** untuk kabar rilis dan komunitas, ditautkan dari tiap halaman
  situs.
- **Tiga puluh method baru atas `Option` dan `Result`**, jalan di semua mesin: `unwrap_or_else`, `ok_or`, `ok_or_else`,
  `filter`, `or`, `or_else`, `and`, `xor`, `map_or`, `map_or_else`, `zip`, `flatten`, `transpose`, `is_some_and`,
  `is_none_or`, `inspect`, `contains` dan kawan-kawannya.
- **Kebuntuan dilaporkan, tidak lagi menggantung selamanya.** Kalau semua goroutine menunggu channel dan tidak ada yang
  bisa maju, programnya berhenti dengan pesan di interpreter, `--vm`, `--jit` dan `--emit-llvm`. Program yang cuma
  lambat (tidur, kerja lama) tidak dituduh buntu.
- **Pesan untuk kebiasaan dari bahasa lain**: `elif`, `def`/`function`, `for (;;)` gaya C, `a ? b : c`, `if (x = 5)`,
  `class`/`interface`, `public`/`private`, `===`/`!==` dan badan bergaya titik dua kini dijawab padanan JRG-nya, dan
  satu salah tulis tetap satu galat (dulu bisa beruntun sampai lima).
- **`--emit-llvm`**: potongan tuple dengan batas dari variabel (`t[i:]`), `len()` atas tuple, argumen bernama dan `...`
  ke nilai fungsi yang baru diketahui saat jalan (`var f = ..`, parameter, `T::m`), dan `json::merge` antara struct
  dan dict kini jalan; dulu ditolak saat kompilasi.
- **`--emit-wasm` membawa builtin `jrgc`** (`crypto::`, `re::`, `url::` dan kawan-kawannya) ke WebAssembly.

### Kinerja

- **`--jit` lebih cepat lagi**: IR milik JRGJIT jadi bawaan, callee yang berhasil disisipkan ke pemanggilnya, dan
  payload tuple serta struct dilipat tanpa dibangun. Jawabannya tetap sama persis dengan `--vm`.
- **Biner `--emit-llvm` membebaskan memori sementara tiap putaran loop** yang tidak menyimpan nilainya ke luar putaran
  (teks yang dirakit lalu cuma dibaca): `string_ops` 24 MB → 1,5 MB dan `mixed_report` 7,2 → 1,6 MB, setara C, tanpa
  melambat. Sambil itu `let t = [a]` dan
  `var n = m` atas larik bersarang juga tidak lagi menyalin sampai ada yang menulis.

- **`var b = a` di biner `--emit-llvm` tidak lagi menyalin array**: dua nama berbagi isinya sampai salah satunya
  menulis (16 ribu salinan 16 ribu elemen: 1,2 detik dan 1,9 GB, sekarang 4 ms).

- **`--emit-llvm`: `match` atas bilangan bulat berlengan literal dan rentang jadi satu lompatan bertabel**, bukan
  perbandingan berurutan — `match_dispatch` 710 → 580 ms, setara C.
- **`--emit-llvm`: `str::char_at` di dalam loop tidak lagi memeriksa ulang jumlah karakter tiap putaran** —
  `string_ops` 123 → 109 ms (C 100).
- **`--emit-llvm`: `let r = arr::pop(a)` lalu `a = r[0]` tidak lagi menyalin larik** — 160 ribu pop 9 ms (dulu kehabisan
  memori di 20 ribu).

### Diperbaiki

- **`--emit-llvm`: `str::join` atas larik bukan teks jalan** seperti `arr::join` (dulu ditolak saat kompilasi).
- **Berkas statis `dok::serve` dialirkan dari disk**, tidak lagi dimuat utuh per permintaan. Dengan middleware
  `sesudah` berkasnya tetap dimuat, di bawah anggaran 256 MiB (lewat: 503).

### Keamanan

- **`zip::extract` dan `tar::extract` tidak lagi bisa menulis ke luar folder tujuan lewat race tautan simbolik.** Pihak
  lain yang bisa menulis ke folder tujuan dulu dapat mengganti direktori yang sudah diperiksa menjadi tautan di sela
  pemeriksaan dan penulisan, lalu isi arsip ditulis mengikuti tautan itu ke luar. Di Linux dan macOS tiap bagian jalur
  sekarang dibuka relatif ke folder tujuan tanpa mengikuti tautan, jadi tautan yang disisipkan kemudian ditolak.
- **`--emit-llvm` tidak lagi membocorkan isi memori saat kunci dict campur tipe ditimpa nilai berjenis lain.** Dulu
  `d["a"]`, `d.a` dan `dict::get` atas kunci yang tadinya angka lalu ditimpa teks atau larik membaca alamat memori atau
  byte mentah sebagai nilai. Sekarang jenisnya diperiksa: jenis yang sama tetap terbaca, jenis lain gagal dengan pesan
  yang menyebut jenis sebenarnya.
- **Anotasi tipe dan pola yang bersarang sangat dalam ditolak, tidak lagi mematikan `jrgc`.** `[][]...int`,
  `Option<Option<...>>` dan pola `(((...)))` puluhan ribu tingkat dulu menghabiskan stack pengurai; sekarang ditolak di
  256 tingkat dengan pesan yang sama di semua mesin, seperti ekspresi.
- **`wasm::muat` menolak berkas `.wasm` rusak dengan pesan**, tidak lagi berhenti mendadak.
- **`wokwok::` tidak lagi berhenti mendadak pada beberapa bentuk array.** `where` dengan kondisi yang di-broadcast
  membaca tiap operand di posisi yang benar (dulu berhenti, atau membaca posisi yang salah), `reverse` dan reduksi per
  sumbu atas sumbu berukuran 0 menjawab array kosong, dan `reshape` menolak bentuk yang hasil kalinya meluap atau lebih
  dari 32 dimensi.
- **`jvm::` memeriksa kelas objek sebelum diserahkan ke Java.** Objek kelas lain ditolak dengan pesan, bukan jawaban
  karangan atau JVM yang mati; objek yang dilepas `jvm::lepas` dari goroutine lain tidak lagi bisa terhapus selagi
  sebuah panggilan memakainya.
- **`jrgm lsp` tidak lagi mati oleh pesan rusak**: JSON yang bersarang sangat dalam ditolak, pratinjau galat tidak
  memotong karakter UTF-8, dan literal seperti `true` diperiksa hurufnya.
- **`http::hanya_publik(true)` menolak lebih banyak bentuk alamat internal IPv6**: alamat yang membawa IPv4 lewat SIIT
  (`::ffff:0:a.b.c.d`), NAT64 lokal (`64:ff9b:1::/48`), Teredo (`2001::/32`), `2001:db8::/32` dan `100::/64`.

## [0.6.0] — 2026-11-16

### Memutus

- **`json::stringify`, `json::pretty` dan `dok::json_encode` menolak pegangan pustaka** (`Ptr`, `Future`, `DokDb`, …),
  seperti `Mutex` — dulu tertulis `{"id":1}`.
- **WebSocket `dok::serve` melepas klien yang diam dan tidak menjawab `ping`** (60 + 30 detik), menutup pesan yang
  tidak utuh dalam 30 detik ditambah 1 detik per 16 KiB, dan menghitung pesan setengah jadi di anggaran 256 MiB yang
  sama dengan badan HTTP.
- **`wokwok::` menolak hasil broadcast dan `matmul` di atas 256 MiB**, seperti pembangun lariknya — dulu dua larik
  kecil bisa meminta puluhan GB dan mematikan prosesnya.

### Baru

- **`jrgm run --jit`, `jrgm exec --jit` dan `jrgm test --jit`**: proyek bisa dijalankan dan diuji lewat mesin keempat
  (VM yang mengompilasi bagian panas jadi kode mesin), sama dengan `jrgc --jit`; `--watch` ikut meneruskannya.

### Kinerja

- **`--jit` menghitung method berumus di tempat**: `x = x.m(dt)` dengan `m` yang membangun `Self` dari rumus pecahan
  atas field-nya tidak lagi memanggil method dan membangun struct baru — field dibaca, dihitung di register, lalu
  ditulis ke struct yang sama; method yang cuma mengembalikan rumus pecahan atas field-nya (`rec.score()`) juga
  dihitung di tempat. `struct_particles` 12,9 → 3,3 s, `mixed_report` 2,2 → 1,4 s.
- **`--jit` memanggil callback `map`/`filter`/`reduce` dan kawan-kawannya langsung**: bingkai callback disiapkan sekali
  untuk seluruh larik, bukan per elemen. `hof_closure` 0,66 → 0,44 s.
- **`--jit` tidak lagi jatuh ke nilai VM di loop sesudah titik masuknya**: panggilan di loop yang belum pernah dijalankan
  saat fungsi pemanggilnya dikompilasi meminjam jenis hasil panggilan lain ke fungsi yang sama. `string_scan`
  0,44 → 0,32 s.

### Diperbaiki

- **`jrgm siapkan android` melewati entri katalog yang nama berkas atau foldernya bukan nama polos** (`..`, `/`).

## [0.5.0] — 2026-11-16

### Memutus

- **`dok::serve` menjawab 400 untuk permintaan HTTP/1.1 tanpa kepala `Host`** (RFC 9112); HTTP/1.0 tetap boleh.
- **`dok::serve` melepas klien yang tidak membaca jawabannya**: tenggat tulis 30 detik ditambah 1 detik per 16 KiB.

### Baru

- **`jrgc --periksa` (dan `jrgm lint`) melaporkan method dan field yang tidak ada** atas `let x = T { .. }`, dengan
  kalimat yang sama seperti saat baris itu dijalankan.

### Kinerja

- **`--jit` lebih cepat di teks, bilangan di atas 64 bit, dan closure** (jam penuh lawan 0.4.0):
  `numeric_kernel` 0,62 → 0,47 s, `string_ops` 0,66 → 0,61 s, `mixed_report` 2,18 → 2,10 s. `teks + teks`,
  `str(n)`, geser konstan atas bilangan 128 bit, dan closure pendek yang membaca tangkapan tidak lagi lewat jalur umum.

### Diperbaiki

- **`--emit-llvm`: sebar `...x` di literal varian enum generik jalan** kalau basisnya varian enum yang sama.
- **`--emit-llvm`: `arr::insert` dengan nilai berjenis lain membuat larik campur** seperti interpreter (dulu ditolak).

## [0.4.0] — 2026-11-15

### Memutus

- **`json::parse` dan `dok::json_decode` menolak kunci kembar di satu objek** (juga yang kembar lewat escape,
  `"adm\u0069n"` lawan `"admin"`). Dulu yang terakhir menang diam-diam, padahal pengurai lain memilih yang pertama.
- **Bacaan berkas utuh berbatas 256 MiB** (`fs::read` dan kawan-kawannya, `file::read`, `zip::`/`tar::` dari jalur,
  `wasm::muat`, berkas statis `dok::serve`): yang lebih besar, atau perangkat tanpa ujung seperti `/dev/zero`, jadi
  galat berkalimat alih-alih menghabiskan memori mesin.
- **`jrgm build` tidak memasang paket Python lagi** — cuma melaporkan modul yang belum ada; `jrgm run` tetap
  memasangnya. Memeriksa modul tidak lagi menjalankan `__init__.py` milik proyek.
- **Sesi `dok::session_*` punya masa berlaku**: cap waktunya ikut ditandatangani, dan `dok::session_decode` menolak
  sesi yang lebih tua dari 14 hari (atau dari argumen ketiga, dalam detik). Cookie sesi lama perlu masuk ulang sekali.
- **Kanvas mode `web` cuma melayani halamannya sendiri**: `Host` lain (DNS rebinding) dan permintaan dari situs lain
  ditolak 403, jadi halaman web lain tidak bisa membaca layar program atau mengirim tombol ke sana.
- **`dok::router_static` tidak menyajikan berkas atau folder bertitik** (`.env`, `.git/`, `.htpasswd`) — dijawab
  seperti tidak ada; `.well-known/` tetap disajikan.
- **`dok::log_*` menulis satu baris per panggilan**: baris baru, ESC dan pengubah arah di pesan dan field ditulis
  sebagai `\u{..}`, dan `"` di nilai field teks di-escape — teks dari luar tidak bisa lagi memalsukan baris log.
- **`str::replace`, `re::replace`/`re::replace_all` dan `str::join`/`arr::join` menolak hasil di atas 256 MiB**,
  seperti `str::repeat`: dulu teks 1 MB dengan pengganti 10 KB mematikan prosesnya.

### Kinerja

- **`--jit` lebih cepat di kode yang memegang enum, struct, larik, dan panggilan fungsi kecil** (jam penuh lawan
  0.3.0): `match_dispatch` 9,9 → 7,0 s, `call_recursion` 1,01 → 0,87 s, `array_sort` 2,5 → 2,2 s,
  `struct_particles` 13,6 → 13,1 s. Lengan `match` atas varian berfield banyak, konstanta varian tanpa payload,
  konversi `int`/`uint` atas bilangan di atas 64 bit, dan fungsi bulat kecil yang dipanggil kode mesin tidak lagi
  lewat jalur umum.

### Diperbaiki

- **Respons `dok::serve` membawa `X-Content-Type-Options: nosniff`** (kecuali aplikasi memasangnya sendiri).
- **Runner `py::`/`js::`/`php::` pindah ke `~/.cache/jrgc/runner`**, dan berkas kerja `--emit-llvm`, `jrgm exec`
  dan `--apk` di folder sementara baru bermode 0700 — dulu nama tebakan di `/tmp` bisa disiapkan pemakai lain.
- **`dok::password_hash` bawaannya 600 000 putaran** (dulu 210 000, angka OWASP untuk SHA-512, bukan SHA-256), dan
  HMAC-SHA256 jauh lebih cepat (SHA-NI bila ada). Hash lama tetap bisa diperiksa.
- **AES (`crypto::seal`, `crypto::aes_*`) tidak lagi membocorkan kunci lewat waktu**: AES-NI di x86_64, dan hitungan
  waktu-tetap tanpa tabel di platform lain. `crypto::random_int` tanpa bias modulo.
- **`--emit-llvm`: `==` atas dua fungsi gagal saat jalan seperti interpreter** (`arr::unique([f])` menjawab
  `[<closure>]`), `f == 1`, `arr::index_of(["a"], 1)` dan `arr::contains(["a"], 1)` menjawab `false`/`None`, dan galat
  saat kompilasi dari dalam pustaka bawaan menunjuk baris program, bukan `<lapis>:baris`.
- **`--emit-llvm`: `f(...v)` dan `f(a: 1)` ke closure yang diikat `let` jalan**, seperti interpreter dan `--vm`
  (dulu ditolak saat kompilasi). Spread berjumlah salah gagal saat jalan dengan kalimat yang sama dengan interpreter.

## [0.3.0] — 2026-11-15

### Memutus

- **`dok::serve` mendengar di `127.0.0.1` kalau alamatnya tidak ditulis** (dulu `0.0.0.0`). Server di belakang
  proxy di mesin yang sama tetap jalan; untuk membukanya ke jaringan tulis `dok::serve(r, port, "0.0.0.0")`.
- **WebSocket dari peramban ditolak 403 kalau host `Origin`-nya lain dari `Host`** (port boleh beda), supaya halaman
  dari situs lain tidak bisa membuka koneksi atas nama pengunjung. Klien tanpa `Origin` tetap diterima.
- **`select` dengan beberapa lengan yang siap sekaligus memilih secara acak** di semua mesin (dulu `--vm` dan
  `--emit-llvm` selalu memilih yang pertama).
- **`let`/`const` lokal yang bernama sama dengan varian enum milik berkasnya ditolak**, dengan kalimat yang sama
  seperti di tingkat atas — nama telanjangnya selalu berarti varian.

- **Klien HTTP (`http::`, `dok::http_request`) menolak kepala `Content-Length`, `Transfer-Encoding` dan
  `Connection` dari pemakai**, dan `Host` yang ditulis dua kali — keduanya dulu terkirim di samping milik klien
  sendiri, sehingga server dan proxy bisa membaca batas permintaan berbeda. `Host` yang ditulis sekali sekarang
  menggantikan yang otomatis.
- **`jrgm` tidak lagi menyerahkan nama berkas yang diawali `-` ke kompilator sebagai opsi**: jalurnya diberi `./`.
- **`main -> int` sekarang kode keluar program** di interpreter, `--vm`, `jrgm run` dan `--emit-llvm`:
  `pub fn main() -> int { 3 }` keluar dengan 3 (dulu 0 di interpreter dan `--vm`). Aturannya sama dengan `exit(n)`:
  di luar 0–255 jadi galat. `main` beranotasi tipe lain (`-> string`) ditolak sebelum jalan. `main` tanpa anotasi
  tidak berubah: nilai baris terakhirnya dibuang — dan di `--emit-llvm` tidak lagi jadi kode keluar.
- **Rekursi tidak lagi dibatasi 10 000 tingkat.** Interpreter dan `--vm` sekarang menolak rekursi saat stack-nya
  benar-benar hampir habis (interpreter ±20 ribu tingkat, `--vm` sampai 1 juta), dan pesannya tidak lagi menyebut
  angka 10000.

- **`dok::serve` menjawab 400 untuk jalur yang punya lebih dari satu ejaan:** segmen kosong (`//`), segmen `.` atau
  `..`, `\`, dan garis miring yang di-encode (`%2F`, `%5C`). Router dan middleware dulu membaca jalur seperti itu
  berbeda, sehingga penjaga izin di middleware bisa dilewati tanpa sesi.

- **`crypto::base64_decode` dan semua pintu base64 menerima SATU ejaan per isi.** `=` di tengah data (`aGk=aGk=`
  dulu terbaca `hihi`), bit sisa yang bukan nol (`aGl=` dulu sama dengan `aGk=`), dan spasi atau tab di tengah data
  sekarang jadi `Err` yang menyebut posisinya. Ganti baris di tengah (base64 MIME) dan spasi di ujung tetap boleh.

- **Versi dependensi git dan versi paket yang diterbitkan harus SemVer 2.0.0 persis.** `version = "v0.1.0"`, `"^1"`
  dan `"0.01.0"` sekarang galat yang menyebut alasannya; `"^1"` dulu jadi tag `v^1` yang tidak pernah ada.
  `jrgm publish` menerima versi pra-rilis (`1.0.0-rc.1`).
- **`jrgm.toml` dan `jrgm.lock` dibaca sebagai TOML 1.0 yang ketat** (pengurai yang sama dengan `toml::parse`).
  Kunci atau tabel kembar, teks tanpa kutip (`name = x`), dan nilai berjenis salah (`version = 1.0`,
  `abi = ["a", 1]`) sekarang galat yang menyebut baris dan kuncinya; dulu kutipnya dibuang begitu saja dan isi tabel
  sebaris dipecah di tiap koma, jadi `path = "../a,b"` terbaca salah. Dependensi berkunci asing (`pth`), atau yang
  menulis `path` dan `git` sekaligus, ditolak. `[dependencies.nama]` sekarang terbaca. `jrgm remove name` tidak lagi
  membuang `name` milik `[package]`, dan `jrgm add` meloloskan `"`/`\` di jalur dan URL.
- **`dok::serve` membaca baris permintaan dan kepala dengan ketat.** Baris permintaan harus tepat `METODE /jalur
  HTTP/1.x` dipisah satu spasi; tab, spasi ganda, bagian keempat, target bentuk absolut (`GET http://…`) dan `*`
  dijawab 400, versi selain HTTP/1.0 dan HTTP/1.1 dijawab 505. Baris yang diakhiri LF tanpa CR, CR atau NUL di tengah
  baris, karakter kendali di nilai kepala, `Host` kembar, dan `Host` yang bukan `host[:port]` sah dijawab 400.
- **`dok::redirect` cuma menerima kode 301, 302, 303, 307, 308**, dan URL tujuan berkarakter kendali jadi galat.
  Query `dok::serve` berisi lebih dari 10 000 pasangan dijawab 400.
- **`dok::db_exec`/`dok::db_query` menjalankan tepat satu pernyataan.** Teks SQL berisi pernyataan kedua
  (`"DELETE …; DROP TABLE t"`) sekarang `Err`; dulu yang kedua dibuang tanpa kabar. Komentar sesudahnya tetap boleh.
- **Jumlah nilai parameter harus sama dengan jumlah `?`.** Dulu yang kurang diisi `nil` diam-diam.
- **`dok::render` menempatkan nilai menurut konteks HTML.** `{{ x }}` di dalam `<script>`/`<style>`, di atribut
  `on*`/`style`/`srcdoc`, di atribut tanpa tanda kutip, atau di tempat nama tag/atribut sekarang `Err` — meloloskan
  HTML tidak melindungi tempat-tempat itu. Nilai di atribut URL (`href`, `src`, `action`, …) yang berskema selain
  `http`/`https`/`mailto`/`tel` (mis. `javascript:`) diganti `#jrg-url-ditolak`. `{{{ x }}}` tetap mentah.
- **`http::unduh` tidak lagi mengikuti pengalihan dari `https://` ke `http://`.** Isi unduhan dan jalurnya dulu
  berpindah ke sambungan tanpa enkripsi tanpa kabar; sekarang `Err`.

### Baru

- **`jrgc file.jrg --jit`**: mesin eksekusi keempat. Fungsi yang hanya menghitung bilangan bulat (seperti `fib`,
  faktorial, `gcd`) dikompilasi jadi kode mesin saat program berjalan — `fib(25)` ±13x lebih sedikit instruksi
  daripada `--vm`; sisanya berjalan di VM sambil dicatat profilnya. Hasilnya, termasuk luapan ke 128 bit dan
  kalimat galat beserta lokasinya, sama persis dengan dua mesin lain. Kode mesin yang menemui kasus di luar
  tebakannya (luapan 64 bit, `% 0`, rekursi sangat dalam) diserahkan ke VM tepat di instruksi itu, tidak diulang
  dari awal. Fungsi yang juga memegang teks, larik atau nilai lain tetap dikompilasi sampai instruksi pertama yang
  menyentuh nilai itu, dan loop panas di `main` naik ke kode mesin di tengah jalan — `for i in 0..n` dan `while`
  atas bilangan bulat ±30x lebih sedikit instruksi daripada `--vm`. `JRG_JIT_LOG=ringkas` melaporkan fungsi panas,
  yang dikompilasi, yang ditolak beserta alasannya, dan jumlah deopt.

- `JRGC_BATAS_MEMORI=<MiB>` membatasi memori biner yang dijalankan `jrgc --emit-llvm`; yang melewatinya berhenti
  dengan "memori habis" alih-alih menghabiskan memori mesin.

- **Dua puluh tiga modul `std/` baru, ditulis dalam JRG**, masing-masing menolak bentuk yang dibaca lain oleh pustaka bahasa
  lain:
  - `std/cli`: argumen baris perintah. Tanpa singkatan opsi, opsi yang ditulis dua kali jadi `Err`, `--` menutup
    opsi, dan nilai berawalan `-` wajib ditulis `--opsi=-x`.
  - `std/tanggal`: tanggal dan waktu tahun 1 sampai 9999 dengan offset tetap. RFC 3339 ketat: waktu tanpa zona,
    detik 60 dan digit non-ASCII ditolak. `tambah_bulan` menjawab `Err` kalau tanggal tujuannya tidak ada.
  - `std/glob`: pola berkas dengan waktu linear (tanpa ReDoS). `[` yang tak ditutup jadi `Err`, dan berkas
    tersembunyi tidak ikut kecuali polanya menulis titik.
  - `std/diff`: selisih Myers terpendek dan format unified yang dibaca `patch`.
  - `std/shlex`: memecah dan mengutip perintah shell menurut aturan POSIX.
  - `std/uuid`: v4 dan v7 dari sumber acak OS. Pengurainya cuma menerima bentuk kanonik.
  - `std/ini`: kunci atau bagian kembar dan baris sambungan jadi `Err`. Nilai diambil apa adanya.
  - `std/dotenv`: `${X}` tidak diekspansi, kunci kembar jadi `Err`, dan `muat` tidak menimpa lingkungan.
  - `std/log`: log terstruktur (logfmt atau JSON per baris). Satu panggilan selalu satu baris karena `\n` dan
    karakter kendali di-escape. Pesan tidak pernah ditafsirkan (tanpa `%s` dan `${..}`).
  - `std/graf`: BFS, jalur terpendek Dijkstra, urutan topologi yang menyebut siklusnya, dan komponen terhubung.
    Bobot negatif ditolak.
  - `std/base32`: RFC 4648 ketat. Huruf kecil, padding salah dan bit sisa bukan nol jadi `Err`.
  - `std/ip`: alamat IPv4/IPv6 dan CIDR. Nol di depan (`010.0.0.1`), heksa, bentuk pendek `127.1`, zona `%eth0` dan
    CIDR yang bit host-nya terisi ditolak. `::ffff:127.0.0.1` digolongkan `loopback` (penangkal bypass SSRF).
  - `std/mirip`: jarak Levenshtein per karakter, rasio kemiripan dan saran ejaan. Teks di atas 2000 karakter jadi
    `Err` alih-alih dihitung berjam-jam.
  - `std/lru`: cache berkapasitas tetap yang membuang entri paling lama tidak dipakai, O(1) per operasi. Kuncinya
    wajib teks, karena `1` dan `"1"` akan jadi satu kunci.
  - `std/os`: nama OS, arsitektur, folder rumah dan sementara, CPU, pengguna, deteksi Termux. `os::path()` membuang
    entri `PATH` yang kosong atau relatif (jalan membajak program), dan `os::path_berbahaya()` menyebutnya.
  - `std/templat`: templat teks `{{ jalur | filter }}` tanpa ekspresi, pemanggilan atau perulangan, jadi templat dari
    luar tidak bisa menjalankan apa pun (kelas SSTI). Kunci yang hilang jadi `Err` yang menyebut barisnya.
  - `std/bungkus`: membungkus dan memotong teks menurut lebar kolom layar, sehingga huruf Asia Timur dihitung dua
    kolom dan tanda gabung tidak terpisah dari hurufnya.
  - `std/cron`: ekspresi jadwal lima kolom dan waktu berikutnya. Langkah `/0`, rentang terbalik dan angka di luar
    batas jadi `Err`; jadwal yang tidak pernah cocok (31 Februari) dijawab `Err`, tidak memutar tanpa akhir.
  - `std/sandi`: pemeriksa kata sandi menurut NIST SP 800-63B, yang menilai panjang dan menolak sandi pasaran,
    deret, pengulangan dan nama pengguna. Aturan "wajib huruf besar/simbol" sengaja tidak ada.
  - `std/markdown`: Markdown ke HTML yang aman untuk masukan dari luar. HTML mentah selalu di-escape, dan tautan
    cuma `http`/`https`/`mailto`/relatif (`JaVaScRiPt:` dan `java\tscript:` ikut ditolak). Biayanya linear, juga
    untuk masukan jahat.
  - `std/batas`: pembatas laju token bucket per kunci (penangkal tebak sandi dan banjir permintaan); jumlah kunci
    yang diingat berbatas, jadi kunci palsu tanpa ujung tidak menghabiskan memori.
  - `std/hitung`: kalkulator rumus dari luar, pengganti `eval()`; cuma aritmetika, variabel dan fungsi dari daftar
    tetap, berbatas panjang dan kedalaman.
  - `std/izin`: kontrol akses berbasis peran yang menolak secara bawaan; peran atau aksi yang tidak dikenal tidak
    pernah diizinkan, dan `produk.*` tidak mencakup `produkx`.
- **`crypto::base64url_encode`, `crypto::base64url_decode` dan `crypto::base64url_decode_bytes`** di tiga mesin:
  base64 untuk URL, cookie dan token, tanpa padding dan dengan satu ejaan per isi. `jwt::` dan sesi `dok::` memakai
  aturan yang sama.
- **`env::os()` dan `env::arch()`** di tiga mesin: nama sistem operasi (`linux`, `android`, `macos`, `windows`,
  `freebsd`) dan arsitektur (`x86_64`, `aarch64`, `arm`, `x86`, `riscv64`) dari biner itu sendiri. Yang tidak
  dikenali dijawab `tak-dikenal`, tidak pernah ditebak.

- **Pesan galat tidak lagi mengutip masukan utuh.** Teks dari luar yang disebut pesan galat `str::to_int`/`int()`/
  `float()`, `json::parse`, `toml::parse`, `url::parse` dan `re::compile` dipotong kalau lebih dari 64 karakter
  (dengan panjang aslinya), dan karakter kendali serta pengubah arah ditulis sebagai `\u{..}`. Dulu angka 2 juta digit
  jadi pesan 2 MB, dan `"12\nPALSU"` menyisipkan baris palsu ke log.
- `fs::safe_join(dasar, relatif)`: jalur di bawah sebuah folder yang tidak bisa keluar darinya. `..`, jalur absolut
  (yang di `fs::join` menimpa dasarnya), `\`, huruf drive, bagian kosong dan karakter kendali jadi `Err`, juga
  tautan simbolik yang menunjuk ke luar folder dasar atau ke tempat yang tidak ada. Aturan namanya sama dengan
  penjaga `zip::`/`tar::`.
- `semver::parse`, `semver::compare`, `semver::satisfies`: SemVer 2.0.0 dan rentangnya (`^1.2, <1.5 || =2.0.0`).
  Bentuk yang dibaca lain oleh pengurai lain jadi `Err`: awalan `v`/`=`, nol di depan, angka di luar 64 bit, versi
  tanpa operator di rentang, spasi atau tanda hubung sebagai pemisah, wildcard `1.x`. Versi pra-rilis cuma cocok
  dengan rentang yang menulis pra-rilis berinti sama. Pengurainya linear, tanpa regex.
- `toml::parse`, `toml::stringify`: TOML 1.0. Bentuk yang dibaca lain oleh pengurai lain jadi `Err`: kunci atau
  tabel kembar, tabel diperluas lewat kunci bertitik atau header, tabel sebaris diperluas, bilangan di luar 64 bit,
  nol di depan, escape di luar TOML 1.0, CR telanjang, tanggal mustahil; sarang paling dalam 256.
- `dok::host_check(req, izin)`: kepala `Host` dicocokkan dengan daftar izin (`contoh.id`, `.contoh.id` beserta
  subdomainnya, `host:port`) — penangkal DNS rebinding dan tautan reset sandi yang memakai `Host` palsu.
- `dok::form`, `dok::multipart`: badan formulir dan unggahan diurai ketat — bentuk yang dibaca lain oleh pengurai
  lain (kepala atau parameter kembar, `filename*`, `\"`, baris tanpa CR) jadi `Err`, paling banyak 1 000 bagian.
- `dok::berkas_aman(nama)`: nama berkas unggahan yang aman disimpan (tanpa folder, `..`, titik depan, nama perangkat).
- `dok::safe_redirect(tujuan, cadangan)`: pengalihan ke `tujuan` hanya kalau jalur lokal, penangkal pengalihan terbuka.
- `url::parse`, `url::build`, `url::join`, `url::query_parse`, `url::query_build`: satu pengurai URL yang juga
  dipakai klien `http::`. URL yang dibaca lain oleh pengurai lain jadi `Err`: spasi dan karakter kendali, `\`,
  `@` ganda, IPv4 oktal/heksa/bulat, host ber-`%XX`, `http:` tanpa `//`. `..` diselesaikan saat mengurai, kunci
  query kembar tetap dua pasangan, `;` bukan pemisah. Klien `http::` menolak kredensial di URL dan menyelesaikan
  pengalihan relatif menurut RFC 3986.
- `tar::list`, `tar::read`, `tar::read_text`, `tar::extract`, `tar::create`: arsip tar dan `.tar.gz` tanpa
  dependensi. Nama yang keluar folder tujuan, tautan simbolik dan keras, entri berukuran ganda, sparse, nama ganda
  dan data sesudah penanda akhir ditolak; `extract` memeriksa seluruh arsip sebelum menulis dan tidak pernah menimpa.
- `gzip::compress`, `gzip::decompress`, `gzip::decompress_text`: format gzip tanpa dependensi. Hasil bongkaran di atas
  256 MiB ditolak sebagai bom, CRC32 dan ukuran tiap anggota diperiksa sebelum isinya diserahkan, kepala yang
  terpotong atau ber-bit cadangan dan byte sisa sesudah anggota terakhir jadi `Err`.
- `http::hanya_publik(true)`: sesudahnya klien `http::` dan `dok::http_*` menolak alamat loopback, privat,
  link-local, CGNAT, multicast dan metadata awan (`169.254.169.254`), juga yang dibungkus IPv6. Yang diperiksa alamat
  hasil DNS yang benar-benar disambungi, di tiap pengalihan — pakai saat URL datang dari pemakai (SSRF).
- `dok::csrf_token`, `dok::csrf_verify`: token CSRF yang terikat ke sesi dan bercap waktu (bawaan 12 jam), dibandingkan
  dalam waktu tetap; token sesi lain, token kedaluwarsa, dan token yang diubah ditolak.
- `jwt::sign`, `jwt::verify`: JSON Web Token HS256. Algoritmanya dipatok (token `alg: none` atau algoritma lain ditolak),
  rahasia minimal 32 byte, `exp`/`nbf` diperiksa, dan token dengan kunci JSON kembar ditolak.
- `xml::parse`, `xml::stringify`, `xml::escape`: XML tanpa dependensi. DTD ditolak utuh, jadi entitas eksternal (XXE)
  dan ledakan entitas tidak mungkin; sarang dibatasi, atribut ganda ditolak, dan penulisnya menolak nama elemen atau
  atribut yang bisa menyisipkan markup.
- `zip::list`, `zip::read`, `zip::read_text`, `zip::extract`, `zip::create`: membaca, membongkar dan membuat arsip
  ZIP. Arsip yang mencoba keluar dari folder tujuan (`../`, jalur absolut), berisi tautan simbolik, nama ganda,
  bom zip, atau isi yang tidak cocok dengan CRC-nya ditolak sebelum satu berkas pun ditulis, dan berkas yang sudah
  ada tidak pernah ditimpa.
- `crypto::compare_digest(a, b)`: membandingkan tanda tangan atau token (teks atau array byte) dalam waktu tetap,
  padanan `hmac.compare_digest` di Python. Pakai ini, bukan `==`, saat memeriksa HMAC webhook atau token rahasia.

### Kinerja

- **`--jit` sekarang juga mengompilasi kode yang memegang struct, enum, larik dan teks**, bukan cuma bilangan
  bulat: field dibaca lewat tembolok bentuk, `match` atas enum diputuskan sekali per rantai lengan, fungsi kecil
  dipanggil tanpa bingkai VM, dan `len`/`str::char_at` atas teks lokal tanpa salinan. `string_ops` penuh
  `--vm` 9,3 s, `--jit` 0,70 s.
- **`let r = arr::pop(a)` lalu `a = r[0]`, dan `a, x = a.pop()`, sekarang linear** di interpreter dan `--vm`
  (dulu menyalin seluruh larik tiap langkah: 16 000 pop 2,2 s → 38 ms).
- `xml::parse` linear dalam jumlah atribut (160 ribu atribut 34 s → 0,24 s).

- **`--vm` 2–28% lebih cepat** di kernel acuan: membaca dan menulis `a[i]` atas larik yang dimiliki satu variabel
  tidak lagi mengunci, `x + 1`/`i % k`/`a * b` atas bilangan bulat dan langkah maju `for` dikerjakan sebagai satu
  instruksi, dan memanggil fungsi tanpa `defer` tidak lagi membayar pemeriksaannya.

- Biner `--emit-llvm`: `str(x % n)` dan `str(rekaman.field)` di tengah sambungan teks tidak lagi membuat teks
  sementara.
- Biner `--emit-llvm`: rantai `a + b + c` atas teks disambung dalam satu alokasi, `len(s)` dan `s[i]` atas teks
  ASCII tidak memanggil runtime, `str::reverse`/`str::replace` lebih cepat — `string_ops` 282 → 156 ms.

### Diperbaiki

- `wasm::muat`/`wasm::panggil` atas modul yang menyebut jumlah parameter, lokal atau cabang `br_table` raksasa
  menjawab `Err`; dulu berkas 17 byte mematikan proses (alokasi 4 GB). Batasnya sama dengan Wasmtime.
- Biner `--emit-llvm`: `json::merge` atas struct ditolak di baris panggilannya dengan jalan keluarnya
  (`dict { ...p }`); dulu kalimatnya salah alamat.
- Biner `--emit-llvm`: `const` lokal dan `fn` di dalam blok polos tidak lagi dibuang atau salah dibaca.
- Pesan galat klien HTTP dan `url::parse` tidak lagi memuat sandi dari URL (`user:sandi@host` jadi `***@host`).

- `struct Ptr`, `struct Task`, `struct DokRouter` (dan nama handle pustaka lain) yang dideklarasikan program sendiri
  tidak lagi bisa memalsukan handle pustaka atau membuka tulis `.id` pada handle asli; `Task::spawn` dan `Future::new`
  tetap bisa dipanggil dari berkas yang mendeklarasikan struct bernama sama.
- Biner `--emit-llvm`: `var` modul array yang didorong elemen berjenis lain (`A = A.push(ubah())`), variabel `Result`
  yang berganti jenis muatan (`r = Ok(1)` lalu `r = Ok("s")` atau `r = Ok(r)`), dan kunci dict bertipe `nil`, `array`,
  `tuple`, `struct`, `enum`, atau `dict` sekarang berjalan sama dengan interpreter dan `--vm`; penerima method array
  dan dict (`f().push(x)`, `A.push(ubah())`) dievaluasi tepat sekali di atas salinan yang aman dari mutasi argumen.
- Keamanan jaringan dan pelayan HTTP: `dok::router_static` menolak tautan simbolik yang keluar dari folder dasar
  (`fs::safe_join`, 403); `http::unduh` membatasi panjang dan jumlah baris kepala jawaban serta memeriksa penutup CRLF
  tiap potongan `Transfer-Encoding: chunked`; `dok::ws_recv` menolak frame klien tanpa mask dan urutan fragmen yang
  melanggar RFC 6455 (kode 1002).
- `--vm`: `f.floor` atas bilangan (tanpa `()`) menjawab nilainya seperti interpreter, dulu `<native fn>`; method
  yang tidak ada (`x.uper(f())`) ditolak SESUDAH argumennya dievaluasi; memanggil field yang bukan fungsi
  (`p.x(2)`) menyebut field yang ada, dulu "`int` bukan fungsi".
- Biner `--emit-llvm` menerima sebar di literal varian ber-field (`E::V { a: 5, ...x }`), dari struct atau dari
  varian enum yang sama; dulu ditolak saat kompilasi.
- Biner `--emit-llvm`: method `Option`/`Result` (`x.is_some()`, `x.unwrap_or(0)`) atas variabel yang bisa berisi
  jenis apa pun berhenti dengan pesan bahwa method itu "ditulis tanpa `()`" — sekarang dijawab seperti di
  interpreter.
- Biner `--emit-llvm` menerima variabel `Option` yang diisi ulang dengan isi berjenis lain (`o = Some(o)`, atau
  `None` lalu `Some("t")`); dulu ditolak saat kompilasi.
- Biner `--emit-llvm`: fungsi yang menumbuhkan array dari array lain yang juga sedang tumbuh (`std/csv::from_dicts`
  atas hasil `csv::to_dicts`) bisa dikompilasi dengan tipe kembalian yang berbeda antara pemanggil dan fungsinya,
  tergantung urutan acak di dalam kompilator — sebagian kompilasi berhenti dengan "tag nilai tidak dikenal" atau
  "memori habis". Sumber yang sama sekarang selalu menghasilkan biner yang sama.
- `x ** n` dengan `x` pecahan dan `n` bulat dibulatkan tepat di interpreter dan `--vm` — dulu digit terakhirnya
  bisa beda dari `--emit-llvm` (`1.1 ** 10` menjawab `2.593742460100002`, yang tepat `2.5937424601000023`).
- Biner `--emit-llvm`: `**` atas nilai yang jenisnya baru diketahui saat jalan (`n[0] **= 2` atas `[1, 2.5]`).
- Biner `--emit-llvm`: `+` yang mengubah lokal jadi teks (`c = c + "z"` atas array, `n = n + "x"` atas bulat),
  `+` atas dua nilai bertag berisi array, dan `d = dict::set(d, k, v)` dengan `v` berjenis lain jalan — dulu
  ditolak.
- Biner `--emit-llvm`: elemen array `var` yang ditulis berjenis lain (`a[1] = "q"` atas `[1, 2]`) dan literal yang
  mencampur array dengan teks atau dict (`[[1, 2], "z"]` yang elemennya ditulis) jalan — dulu ditolak saat
  kompilasi.
- Biner `--emit-llvm`: handle `py::`/`js::`/`php::`/`lua::` yang cuma dipakai di dalam satu blok (badan loop,
  fungsi) dilepas di ujung blok itu — dulu worker bahasa lain menyimpan tiap objek sampai program selesai.
- Biner `--emit-llvm`: literal array berisi array yang panjangnya berbeda (`[[t], [t, t]]` atas tuple, `Option`
  atau variabel) jalan — dulu ditolak saat kompilasi.
- Nilai bersarang sangat dalam (jutaan tingkat) bisa ditampilkan, dibandingkan, diurutkan dan disandikan JSON di
  interpreter dan `--vm` — dulu prosesnya mati tanpa pesan di atas ±1 juta tingkat. `str()` dan `json::stringify`
  atas nilai bersarang tidak lagi melambat kuadratik di `--vm` (500 ribu tingkat: 29 detik → 0,4 detik).
- Biner `--emit-llvm`: `match x { None => .., _ => .. }` atas elemen array atau isi dict campur tipe yang berisi
  `None` (atau varian tanpa isi milik programmu) memilih lengan yang benar — dulu selalu jatuh ke `_`. Pola
  `Some(v)`, `Ok(x)`, `Err(e)` dan varian enum generik atas nilai campur tipe jalan (dulu ditolak saat kompilasi),
  `for` atas nilai campur tipe yang berisi dict, teks atau bilangan jalan, dan `match` tanpa lengan yang cocok
  menyebut nilainya seperti interpreter.
- Biner `--emit-llvm`: menulis lewat indeks ke dalam nilai campur tipe (`e[1] = x` atas elemen array campur,
  `d["k"][1] = x`, `a[0][1] = y`) jalan seperti di interpreter — dulu ditolak saat kompilasi. `t.0` atas tuple di
  dalam nilai campur tipe juga jalan. `--vm` menyebut `t.5` di luar batas dengan kalimat yang sama dengan interpreter.
- `jrgc f.jrg --uji --emit-llvm` menjalankan tes di biner native, dengan laporan dan kode keluar yang sama dengan
  interpreter (dulu "fungsi `main` tidak ditemukan"). Laporan `--uji` tidak lagi menyebut nomor baris berkas uji
  untuk galat yang lahir di modul lain — baris itu dulu karangan.
- Biner `--emit-llvm`: ke-23 modul `std/` baru (dulu 7) jalan di sana — `base32`, `batas`, `cli`, `cron`, `diff`,
  `dotenv`, `glob`, `graf`, `hitung`, `ini`, `ip`, `lru`, `markdown`, `shlex`, `tanggal`, `templat` bertambah. `arr::min`/`max` menerima array
  yang panjangnya baru diketahui saat jalan. Cabang `typeof` yang mustahil tidak lagi
  diturunkan, closure di dalam modul bisa memanggil fungsi modulnya, `r.map_err(..)` dan `Ok(None)` bertipe benar,
  `math::min`/`max`, `str::join` dan operator bit menerima nilai campur jenis, dan `x = m::f(x, ..)` serta
  `x, y = f(x, ..)` tidak lagi menyalin `x`.
- Biner `--emit-llvm`: `arr::sort_desc` menukar elemen yang setara (`[2, 2.0]` → `[2.0, 2]`) — sekarang stabil
  seperti dua mesin lain. `<` dan pengurutan atas array berisi nilai campur jenis jalan (dulu ditolak), literal
  array campur dari variabel (`[n, i, nama]`) dan indeks konstanta modul (`NAMA[i]`) diterima; `std/mirip` dan
  `std/uuid` jalan di sana. Akses karakter teks lebih cepat: `string_ops` 822 → 276 ms, `mixed_report` 585 → 301 ms.
- Biner `--emit-llvm`: perulangan atas teks (`for c in s`, `str::chars`) dan `s = s + rune` sekarang linear (dulu
  480 ribu karakter ±5 s), `x = arr::pop(x)[0]` tidak lagi menyalin `x`, dan `x = arr::push(x, ..)` di fungsi yang
  memakai closure tidak lagi menyalin `x` — `std/csv`, `std/shlex`, `std/path` dan `std/tabel` jadi linear di sana.
- `arr::contains`, `arr::index_of`, `.starts_with`/`.ends_with` atas array di `--vm` tidak lagi menyalin seluruh array.
- `std/tabel` (`baris_baru`, `dari`, `rata_kanan`) linear di tiga mesin.
- Teks tidak lagi disalin tiap kali dibaca: argumen `str::`, `s[i]`, `str::char_at` dan `str::starts_with` atas
  teks panjang sekarang tidak bergantung pada panjangnya (dulu 1,6 MB teks ±1 ms per panggilan di `--vm`), dan
  `s[a:b]`/`str::substr` tidak lagi membuat daftar karakter seluruh teks.
- `std/csv::parse`, `std/path::normalize`, `std/graf` (membangun graf dan `urut_topologi`), `std/iterator::group_by`
  dan `std/lru` sekarang linear — dulu kuadrat (12 ribu baris CSV 4,7 s → 85 ms); antrean `std/lru` dulu tumbuh
  tanpa batas.
- Di `--emit-llvm`, array/dict yang tersimpan di tempat campur tipe sekarang bisa dibandingkan dengan `==`/`!=`
  (dulu ditolak) dan ditampilkan sedalam apa pun (dulu 20 ribu tingkat kehabisan stack).
- `x = [.., x, ..]` (mis. `a = [a, i]` di perulangan) di `--emit-llvm` sekarang linear; dulu seluruh `x` disalin
  tiap langkah, jadi 20 ribu langkah memakan 3 GB.
- `--vm` yang tidak bisa membuat thread programnya (batas memori proses) sekarang menjalankan program di thread
  pemanggil; dulu ia panik.

- `obj.d = dict::set(obj.d, k, v)` dan `obj.d = dict::remove(obj.d, k)` sekarang mengubah dict di tempat; dulu menyalin
  seluruh dict tiap kali, jadi menghapus 16 000 kunci satu per satu makan 20 detik.

- `obj.daftar = arr::push(obj.daftar, x)` sekarang tumbuh di tempat seperti `obj.daftar.push(x)`; dulu menyalin seluruh
  array tiap kali, jadi 32 000 elemen makan 52 detik.

- `int("777…7x")` dan `str::to_int` atas teks panjang yang bukan bilangan dulu disebut "di luar jangkauan integer
  128 bit". Sekarang disebut "bukan bilangan bulat", karena luapan cuma dilaporkan kalau teksnya seluruhnya digit.

- Klien `http::`/`dok::http_*`: menyambung dibatasi 10 detik per alamat (dulu bisa menggantung beberapa menit), dan
  URL berhost IPv6 (`http://[::1]:8080/`) dulu ditolak sebagai port tidak sah.
- `dok::password_hash`/`password_verify`: sandi yang sangat panjang dulu membuat satu login memakan menit CPU (sandi
  di-hash ulang tiap putaran); sekarang biayanya sama dengan sandi biasa. Hash lama tetap cocok.
- `--vm`: `let`/`const`/`var` tingkat modul yang initializer-nya membuat variabel lokal (`a ?? b`, `match` yang
  mengikat nama, blok berisi `let`) dulu gagal dengan galat internal `local slot 0 OOB`.
- `--emit-llvm`: `str::repeat(s, n)` dan `s * n` dengan `n` negatif dulu menjawab teks kosong; sekarang galat seperti
  dua mesin lain, juga untuk hasil di atas 256 MiB. `arr::fill`/`arr::repeat` memegang batas 16 777 216 elemen yang
  sama.
- Contoh web tidak lagi menulis rahasia sesi di sumbernya; mereka membaca `SESSION_KEY`, atau membuat kunci acak.

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
