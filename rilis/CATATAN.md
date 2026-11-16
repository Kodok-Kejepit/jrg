JRG 0.6.0 membuat `--jit` jauh lebih cepat untuk kode yang hidup dari struct dan `map`/`filter`/`reduce`, membawa
`--jit` ke `jrgm`, dan memperketat beberapa batas sumber daya. Tiga perubahan bisa membuat program lama berperilaku
lain; daftarnya di bawah.

## Lebih cepat

`--jit` sekarang menghitung method kecil berumus langsung di tempat pemanggilnya. `x = x.m(dt)` yang membangun `Self`
baru dari rumus pecahan atas field-nya tidak lagi memanggil method dan membuat struct baru: field dibaca, dihitung di
register, lalu ditulis ke struct yang sama. Method yang cuma mengembalikan rumus pecahan atas field-nya
(`rec.score()`) juga dihitung di tempat. Callback `map`/`filter`/`reduce` dan kawan-kawannya dipanggil tanpa
menyiapkan bingkai baru tiap elemen. Jam penuh lawan 0.5.0:

| program | 0.5.0 | 0.6.0 |
|---|---|---|
| `struct_particles` | 12,9 s | 3,3 s |
| `mixed_report` | 2,2 s | 1,4 s |
| `hof_closure` | 0,66 s | 0,44 s |
| `string_scan` | 0,44 s | 0,32 s |

Jawabannya tetap sama persis dengan interpreter dan VM, termasuk saat struct-nya dibagi dengan variabel lain, field-nya
berganti jenis, atau callback-nya gagal di tengah larik.

## Baru

- `jrgm run --jit`, `jrgm exec --jit` dan `jrgm test --jit`: proyek bisa dijalankan dan diuji lewat mesin keempat, sama
  dengan `jrgc --jit`. `--watch` ikut meneruskannya.

## Bisa memutus program lama

- `json::stringify`, `json::pretty` dan `dok::json_encode` menolak pegangan pustaka (`Ptr`, `Future`, `DokDb`, …),
  seperti `Mutex`. Dulu hasilnya `{"id":1}`.
- WebSocket `dok::serve` melepas klien yang diam dan tidak menjawab `ping` (60 + 30 detik), menutup pesan yang tidak
  utuh dalam 30 detik ditambah 1 detik per 16 KiB, dan menghitung pesan setengah jadi di anggaran 256 MiB yang sama
  dengan badan HTTP.
- `wokwok::` menolak hasil broadcast dan `matmul` di atas 256 MiB, seperti pembangun lariknya. Dulu dua larik kecil
  bisa meminta puluhan GB dan mematikan prosesnya.

## Diperbaiki

- `jrgm siapkan android` melewati entri katalog yang nama berkas atau foldernya bukan nama polos (`..`, `/`).

Rincian lengkapnya di [CHANGELOG.md](https://github.com/Kodok-Kejepit/jrg/blob/main/CHANGELOG.md).

## Pasang

```sh
curl -fsSL https://raw.githubusercontent.com/Kodok-Kejepit/jrg/main/rilis/pasang.sh | sh
```

Atau unduh arsip untuk sistem kamu di bawah, lalu periksa dengan berkas `.sha256`-nya. Dokumentasi:
https://jrg-lang.org
