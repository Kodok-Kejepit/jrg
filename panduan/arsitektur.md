# Arsitektur JRG

Peta kode sumber untuk yang ingin membaca atau mengubah `jrgc`. Penjelasan bahasanya sendiri ada di
[situs dokumentasi](https://jawarustgoplusplus.web.id/web_dokumentasi/index.html).

## Jalur sebuah program

```
berkas .jrg
   │  src/lexer/     token, termasuk penyisipan titik koma otomatis
   ▼
   │  src/parser/    pohon sintaks (AST) dan pemeriksaan yang berlaku di semua mesin
   ▼
 ┌─────────────────┬──────────────────────┬──────────────────────────┐
 │ src/interpreter │ src/vm               │ src/llvm                  │
 │ menjalankan AST │ bytecode + mesin     │ LLVM IR teks → clang      │
 │ langsung        │ virtual (--vm)       │ → program native          │
 └─────────────────┴──────────────────────┴──────────────────────────┘
```

Ketiga mesin memakai pemindai dan pengurai yang sama, jadi galat sintaks dan aturan yang diperiksa parser (misalnya
`let` yang tidak bisa ditugasi ulang) berlaku sama di semuanya. `src/pipa.rs` adalah pintu bersama untuk mengurai
berkas beserta pesan galatnya.

## Mesin

- **Interpreter** (`src/interpreter/`): mesin bawaan. Fungsi bawaan didaftarkan lewat `NativeFnKind`. Goroutine adalah
  thread sistem operasi.
- **VM** (`src/vm/`): kompilator ke bytecode dan mesin virtualnya. Tiap instruksi membawa lokasi sumber, jadi galatnya
  menunjuk baris dan kolom yang sama dengan interpreter. Fungsi bawaan VM terdaftar di `src/stdlib/` dalam tabel statis
  yang dibangun sekali per proses.
- **LLVM** (`src/llvm/`): menulis LLVM IR sebagai teks lalu memanggil `clang`. Programnya ditautkan ke `runtime/`
  (pustaka statis Rust tanpa `std`) dan, untuk `dok::`/`crypto::`/`re::`/`ptr::`, ke `jembatan/`. Rinciannya di
  [backend-llvm.md](backend-llvm.md).

Aturan yang dipakai lebih dari satu mesin (pesan galat, konversi, urutan nilai, batas aritas) ditulis sekali, biasanya
di `src/interpreter/`, lalu dipanggil dari mesin lain.

## Pustaka

| Folder | Isi |
|---|---|
| `src/stdlib/` | fungsi bawaan VM per area: koleksi, sistem, berkas, JSON, regex, HTTP, future |
| `src/dok/` | jaringan, HTTP, server web, JSON, log, kripto, SQLite |
| `src/canvas/` | rasterizer, transformasi, PNG (enkoder dan dekoder sendiri), jendela |
| `src/wokwok/` | array numerik n-dimensi dengan buffer bertipe |
| `src/suara/` | pembangkit bunyi, WAV, pemutar (PulseAudio, AAudio) |
| `src/ffi/` | C/C++ (`native.rs`), Python, JavaScript, PHP, Lua lewat proses pekerja (`pekerja.rs`), JVM, .NET, WebAssembly |
| `stdlib/` | modul `std/` yang ditulis dalam JRG; ikut tertanam di biner |

## Perkakas

| Folder | Isi |
|---|---|
| `src/jrgm/` | `jrgm`: proyek, dependensi, tes, format, lint, rilis, APK |
| `src/lsp/` | server bahasa |
| `src/repl.rs`, `src/term.rs` | REPL dan kendali terminal (mode mentah, pembaca tombol) |
| `editor/` | penyunting `jrgc --sunting`, ditulis dalam JRG dan tertanam di biner |
| `src/host.rs` | satu pintu ke lingkungan: keluaran, masukan, jam, acak, keluar |
| `web/` | mesin JRG untuk peramban (WebAssembly), dipakai tombol Jalankan di situs |
| `ide/` | JRG IDE: pelayan dalam JRG, halaman web, dan pembungkus APK Android |
| `rilis/` | pemasang dan pembungkus rilis |
| `perkakas/` | skrip pemeriksa: sapuan penuh, pembangun situs, uji pemasang |

## Tes

| Berkas | Yang dijaga |
|---|---|
| `tests/backend_parity.rs` | program yang sama harus mencetak hal yang sama di interpreter dan VM, atau ditolak keduanya |
| `tests/llvm_parity.rs` | hal yang sama antara interpreter dan LLVM, plus daftar yang memang belum didukung LLVM |
| `tests/conformance/` | berkas uji semantik lengkap yang dijalankan di ketiga mesin |
| `tests/memory_model.rs`, `tests/vm_complexity.rs` | biaya: penyalinan dan pertumbuhan koleksi tetap linear |
| `tests/error_behavior.rs` | isi pesan galat |
| `tests/lsp.rs`, `tests/fmt.rs`, `tests/nada.rs` | server bahasa, formatter, nada pesan galat |

Tabel paritas bekerja dua arah: ada daftar program yang harus sama, dan daftar program yang harus ditolak dengan pesan
yang benar. Tanpa yang kedua, sebuah mesin bisa diam-diam mulai menerima program yang seharusnya ditolak tanpa satu tes
pun gagal.
