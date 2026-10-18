# Berkontribusi ke JRG

Terima kasih sudah mau ikut membantu. Laporan bug, perbaikan dokumentasi, dan kode semuanya diterima.

## Melaporkan bug

Buka [Issue](https://github.com/Kodok-Kejepit/jrg/issues) dan sertakan:

- program `.jrg` sekecil mungkin yang memperlihatkan masalahnya;
- keluaran yang kamu dapat dan yang kamu harapkan;
- mesin yang dipakai (`jrgc`, `--vm`, atau `--emit-llvm`) dan keluaran `jrgc --version`.

Jawaban yang salah tanpa pesan galat dianggap bug serius. Begitu juga dua mesin yang mencetak hal berbeda untuk program
yang sama.

## Menyiapkan

```sh
git clone https://github.com/Kodok-Kejepit/jrg
cd jrg
cargo build --release
```

Di mesin dengan memori kecil (di bawah 8 GB), pakai `-j 1`: profil release memakai LTO dan bisa kehabisan memori kalau
dikompilasi paralel.

## Menguji

```sh
cargo test --release -j 1 --no-fail-fast   # seluruh tes
cargo test --test backend_parity           # satu berkas tes
sh rilis/uji_asap.sh target/release 0.2.0  # uji asap atas biner jadi
```

`--no-fail-fast` penting: tanpanya `cargo test` berhenti di berkas tes pertama yang gagal. Tes yang memakai Python, Node,
PHP, Lua, atau JDK dilewati dengan pesan kalau perkakasnya tidak terpasang.

## Aturan kode

- **Tidak ada jawaban salah yang diam.** Sesuatu yang tidak bisa dihitung dengan benar harus menghasilkan galat yang
  menyebut sebabnya, tidak pernah `0`, `nil`, atau nilai kosong yang tampak masuk akal.
- **Satu program, satu arti.** Perubahan perilaku harus berlaku di interpreter dan VM sekaligus. Kalau backend LLVM
  belum bisa, ia menolak dengan alasan yang jelas. Tambahkan kasusnya ke `tests/backend_parity.rs` atau
  `tests/llvm_parity.rs`.
- **Satu definisi.** Aturan yang dipakai beberapa mesin (pesan galat, konversi, urutan) ditulis sekali lalu dipanggil,
  bukan disalin.
- **Pesan galat** berbahasa Indonesia santai, tanpa emoji, menyebut letak dan jalan keluarnya.
- **Komentar** pendek, paling banyak tiga baris. Penjelasan panjang masuk ke `panduan/` atau ke situs.
- Ikuti gaya kode di sekitar bagian yang kamu ubah. Untuk kode JRG, jalankan `jrgm fmt`.

## Dokumentasi

Situs dokumentasinya ([jawarustgoplusplus.web.id](https://jawarustgoplusplus.web.id)) dikelola terpisah dari repo ini.
Kalau ada contoh yang salah atau penjelasan yang kurang, buka issue dan sebut halamannya. Contoh kode di sana
dijalankan dengan `jrgc` sebelum terbit, jadi contoh yang keluarannya tidak cocok juga dihitung bug.

## Pull request

- Satu PR untuk satu perubahan.
- Jelaskan apa yang berubah dan bagaimana kamu memeriksanya.
- Tambahkan tes yang gagal sebelum perbaikanmu dan lulus sesudahnya.
- Catat perubahan yang terlihat oleh pemakai di bagian `[Belum dirilis]` di `CHANGELOG.md`.

Kontribusimu dirilis di bawah lisensi MIT yang sama dengan proyek ini.
