JRG 0.6.1 meningkatkan keamanan: delapan celah yang ditemukan lewat audit ditutup, ditambah pesan galat yang menuntun
pendatang dari bahasa lain, deteksi kebuntuan, dan tiga puluh method baru atas `Option`/`Result`. Dua perubahan bisa
membuat program lama berperilaku lain, dan pesan galat sekarang berbahasa Inggris secara bawaan; daftarnya di bawah.

## Keamanan

- **`zip::extract` dan `tar::extract` tidak lagi bisa menulis ke luar folder tujuan lewat race tautan simbolik.** Pihak
  lain yang bisa menulis ke folder tujuan dulu dapat mengganti direktori yang sudah diperiksa menjadi tautan di sela
  pemeriksaan dan penulisan; isi arsip lalu ditulis mengikuti tautan itu. Di Linux dan macOS tiap bagian jalur sekarang
  dibuka relatif ke folder tujuan tanpa mengikuti tautan.
- **`--emit-llvm` tidak lagi membocorkan isi memori** saat kunci dict campur tipe ditimpa nilai berjenis lain lalu
  dibaca lewat `d["a"]`, `d.a` atau `dict::get`. Jenisnya sekarang diperiksa; jenis lain gagal dengan pesan.
- **Anotasi tipe dan pola yang bersarang sangat dalam ditolak** di 256 tingkat, tidak lagi mematikan `jrgc` karena
  stack habis (`[][]...int`, `Option<Option<...>>`, `(((...)))`).
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
- **`http::hanya_publik(true)` menolak lebih banyak bentuk alamat internal IPv6**: SIIT (`::ffff:0:a.b.c.d`), NAT64
  lokal, Teredo, `2001:db8::/32` dan `100::/64`.

Audit keamanan masih berlanjut. Temuan yang belum selesai diperbaiki masuk 0.6.2.

## Bisa memutus program lama

- **Pesan JRG tampil dalam bahasa Inggris secara bawaan.** Bahasa Indonesia tetap ada: `jrgm language id`,
  `jrgc --bahasa id`, atau `JRG_BAHASA=id`. Keluaran program tidak berubah; skrip yang membandingkan teks galat perlu
  memasang bahasanya.
- **`?` tidak lagi mengubah `None` menjadi `Err` diam-diam**, dan **`var x: T` tanpa nilai awal untuk struct, enum
  atau `Option` gagal kalau dibaca sebelum diisi** (dulu `nil`). Keduanya dulu menghasilkan nilai yang masuk akal tapi
  salah; sekarang gagal dengan pesan yang menyebut jalan keluarnya.
- Batas sumber daya lebih ketat: `proses::jalankan`/`keluaran`, `tok()` dan `dok::tcp_recv_all` berhenti di 256 MiB
  dan menjawab `Err` (dulu kehabisan memori); klien `http::` menolak jawaban yang terpotong; `jrgm new` menolak folder
  yang sudah ada; dependensi git `jrgm` cuma lewat https, http, ssh, git dan berkas lokal.

## Baru

- **Tiga puluh method atas `Option` dan `Result`** di semua mesin: `unwrap_or_else`, `ok_or`, `filter`, `or_else`,
  `map_or`, `zip`, `flatten`, `transpose`, `is_some_and` dan kawan-kawannya.
- **Kebuntuan dilaporkan.** Kalau semua goroutine menunggu channel dan tidak ada yang bisa maju, programnya berhenti
  dengan pesan, bukan menggantung selamanya.
- **Pesan yang menuntun** untuk `elif`, `def`, `for (;;)`, `a ? b : c`, `class`, `public`, `===` dan badan bergaya
  titik dua: dijawab padanan JRG-nya, dan satu salah tulis tetap satu galat.
- **`tangkap(f)`** menangkap galat saat jalan sebagai `Result`; **`fs::open`/`read_line`/`read_chunk`** membaca berkas
  besar bertahap; **`http::tenggat`**, **`wasm::batas_langkah`** dan **`dok::password_needs_rehash`**.
- **`--emit-llvm`** kini menjalankan potongan tuple berbatas variabel, argumen bernama dan `...` ke nilai fungsi
  dinamis, dan `json::merge` struct dengan dict. **`--emit-wasm`** membawa builtin `jrgc` ke WebAssembly.

## Lebih cepat

`--jit` lebih cepat lagi: IR milik JRGJIT jadi bawaan, callee disisipkan ke pemanggilnya, dan payload tuple serta
struct dilipat tanpa dibangun. Biner `--emit-llvm` membebaskan memori sementara tiap putaran loop, dan `match` atas
bilangan bulat jadi lompatan bertabel. Jawabannya tetap sama persis dengan interpreter dan VM.

Rincian lengkapnya di [CHANGELOG.md](https://github.com/Kodok-Kejepit/jrg/blob/main/CHANGELOG.md).

## Pasang

```sh
curl -fsSL https://raw.githubusercontent.com/Kodok-Kejepit/jrg/main/rilis/pasang.sh | sh
```

Atau unduh arsip untuk sistem kamu di bawah, lalu periksa dengan berkas `.sha256`-nya. Dokumentasi:
https://jrg-lang.org
