JRG 0.4.0 memperketat pustaka berkas, web, dan kripto, dan membuat `--jit` lebih cepat di kode yang memegang enum,
struct, dan larik. Beberapa perubahan bisa membuat program lama berperilaku lain; daftarnya di bawah.

## Lebih aman

- Bacaan berkas utuh (`fs::read`, `file::read`, `zip::`/`tar::` dari jalur, berkas statis `dok::serve`) dibatasi
  256 MiB, jadi perangkat tanpa ujung seperti `/dev/zero` jadi galat alih-alih menghabiskan memori.
- AES (`crypto::seal`, `crypto::aes_*`) tidak lagi membocorkan kunci lewat waktu: AES-NI di x86_64, hitungan
  waktu-tetap di platform lain. `crypto::random_int` tanpa bias modulo.
- `dok::password_hash` bawaannya 600 000 putaran; hash lama tetap bisa diperiksa.
- Sesi `dok::session_*` punya masa berlaku (bawaan 14 hari).
- `dok::router_static` tidak menyajikan berkas bertitik (`.env`, `.git/`); respons `dok::serve` membawa
  `X-Content-Type-Options: nosniff`; `dok::log_*` selalu satu baris per panggilan.
- Kanvas mode `web` menolak halaman dari situs lain.
- Berkas kerja `--emit-llvm`, `jrgm exec`, dan runner `py::`/`js::`/`php::` tidak lagi bernama tebakan di `/tmp`.

## Lebih cepat

`--jit`, jam penuh lawan 0.3.0: `match_dispatch` 9,9 → 7,0 detik, `call_recursion` 1,01 → 0,87,
`array_sort` 2,5 → 2,2, `struct_particles` 13,6 → 13,1.

## Bisa memutus program lama

- `json::parse` dan `dok::json_decode` menolak kunci kembar di satu objek.
- Bacaan berkas di atas 256 MiB, dan hasil `str::replace`/`re::replace`/`join` di atas 256 MiB, jadi galat.
- `dok::session_decode` menolak sesi yang lebih tua dari 14 hari (atau dari argumen ketiganya, dalam detik).
- `jrgm build` tidak memasang paket Python lagi, cuma melaporkan yang belum ada; `jrgm run` tetap memasangnya.
- `dok::router_static` menjawab berkas bertitik seperti tidak ada.

## Diperbaiki di `--emit-llvm`

- `f(...v)` dan `f(a: 1)` ke closure yang diikat `let` jalan seperti di interpreter.
- `==` atas dua fungsi gagal saat jalan seperti interpreter, dan galat dari pustaka bawaan menunjuk baris program.

Rincian lengkapnya di [CHANGELOG.md](https://github.com/Kodok-Kejepit/jrg/blob/main/CHANGELOG.md).

## Pasang

```sh
curl -fsSL https://raw.githubusercontent.com/Kodok-Kejepit/jrg/main/rilis/pasang.sh | sh
```

Atau unduh arsip untuk sistem kamu di bawah, lalu periksa dengan berkas `.sha256`-nya. Dokumentasi:
https://jrg-lang.org
