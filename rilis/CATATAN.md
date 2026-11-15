JRG 0.5.0 membuat `--jit` lebih cepat di teks dan bilangan besar, memperketat `dok::serve`, dan menambal beberapa
celah di `--emit-llvm` dan pemeriksa. Dua perubahan bisa membuat program lama berperilaku lain; daftarnya di bawah.

## Lebih cepat

`--jit`, jam penuh lawan 0.4.0: `numeric_kernel` 0,62 → 0,47 detik (setara biner `--emit-llvm`), `string_ops`
0,66 → 0,61, `mixed_report` 2,18 → 2,10. Yang tidak lagi lewat jalur umum: `teks + teks`, `str(n)`, geser bit konstan
atas bilangan di atas 64 bit, dan closure pendek yang membaca tangkapannya.

## Lebih aman

- `dok::serve` melepas klien yang tidak membaca jawabannya (tenggat tulis 30 detik ditambah 1 detik per 16 KiB), jadi
  satu klien lambat tidak bisa menahan pekerja server.
- Permintaan HTTP/1.1 tanpa kepala `Host` dijawab 400 (RFC 9112).

## Bisa memutus program lama

- Klien HTTP/1.1 yang tidak mengirim `Host` ke `dok::serve` sekarang ditolak; HTTP/1.0 tetap boleh.
- Klien yang membaca jawaban sangat lambat diputus sesudah tenggat tulisnya habis.

## Lainnya

- `jrgc --periksa` dan `jrgm lint` melaporkan method dan field yang tidak ada atas `let x = T { .. }`, dengan
  kalimat yang sama seperti saat baris itu dijalankan.
- `--emit-llvm`: `arr::insert` dengan nilai berjenis lain membuat larik campur seperti interpreter, dan sebar `...x`
  di literal varian enum generik jalan.

Rincian lengkapnya di [CHANGELOG.md](https://github.com/Kodok-Kejepit/jrg/blob/main/CHANGELOG.md).

## Pasang

```sh
curl -fsSL https://raw.githubusercontent.com/Kodok-Kejepit/jrg/main/rilis/pasang.sh | sh
```

Atau unduh arsip untuk sistem kamu di bawah, lalu periksa dengan berkas `.sha256`-nya. Dokumentasi:
https://jrg-lang.org
