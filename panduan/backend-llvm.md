# Backend LLVM

`jrgc program.jrg --emit-llvm` menerjemahkan program JRG ke LLVM IR, lalu `clang` mengompilasinya jadi program native
yang berdiri sendiri.

## Memakai

```sh
jrgc program.jrg --emit-llvm                         # kompilasi lalu jalankan
jrgc program.jrg --emit-llvm -o program --no-run     # kompilasi saja
jrgc program.jrg --emit-llvm -O3 --native-cpu --lto thin --strip
jrgc program.jrg --emit-llvm --full-power            # preset tercepat untuk CPU ini
jrgc program.jrg --emit-llvm --target aarch64-unknown-linux-gnu
```

Yang dibutuhkan:

- `clang` (versi 15 ke atas). Bisa ditunjuk lewat `JRGC_CLANG`.
- Toolchain Rust, untuk membangun `runtime/` sekali saat pertama dipakai. Hasilnya disimpan di `runtime/target/`.
- Untuk program yang memakai `dok::`, `crypto::`, `re::`, atau `ptr::`: `jembatan/` juga dibangun sekali.
- Untuk PGO: `opt` dan `llvm-profdata` (atau `JRGC_LLVM_OPT`, `JRGC_LLVM_PROFDATA`).

### Optimisasi dengan profil (PGO)

```sh
jrgc program.jrg --emit-llvm -o latih --no-run --pgo-generate profil/
./latih < masukan-khas.txt
jrgc program.jrg --emit-llvm -o program --no-run --pgo-use profil/
```

## Cara kerjanya

```
AST
 │  src/llvm/sema.rs, types.rs, infer*.rs   kumpulkan deklarasi, simpulkan tipe
 ▼
 │  src/llvm/codegen*.rs                    tulis IR; fungsi generik dibuat per kombinasi tipe yang dipakai
 ▼
LLVM IR (.ll)
 │  clang -O<n> program.ll primitif.c libjrgrt.a [libjrgjembatan.so]
 ▼
program native
```

- IR ditulis sebagai teks, jadi `jrgc` tidak butuh pustaka LLVM saat dibangun; `clang` baru dibutuhkan saat
  `--emit-llvm` dipakai.
- `runtime/` adalah pustaka statis Rust tanpa `std` (cuma `alloc` di atas libc). Isinya pencetakan nilai, operasi teks,
  koleksi, dan pesan galat. Tanpa `std`, biner kecil: sekitar 18 KB untuk kernel angka yang di-*strip*.
- Nilai yang tipenya baru diketahui saat jalan disimpan bertag (`{ i32 tag, [2 x i64] }`); nilai yang tipenya jelas
  tetap polos, jadi kode angka tidak membayar biaya tag.
- `src/llvm/lapis.jrg` berisi sebagian fungsi `arr::`, `str::`, dan `dict::` yang ditulis dalam JRG sendiri lalu
  dikompilasi bersama program.

## Perbedaan dengan interpreter dan VM

- **Bilangan bulat 64 bit**, bukan 128. Luapan tetap diperiksa; angka yang tidak muat ditolak saat kompilasi atau
  dilaporkan saat jalan dengan menyebut nilai sebenarnya.
- **Konkurensi sebagai korutin di satu thread.** `spawn`, channel, `select`, `Future`, dan `Task` bekerja dan menjawab
  sama dengan dua mesin lain, tapi tidak berjalan paralel. Kebuntuan dilaporkan. Perulangan sibuk tanpa titik tunggu
  membuat goroutine lain tidak kebagian giliran.
- **Pesan galat** membawa lokasi dan potongan kode seperti mesin lain, tapi tanpa jejak pemanggilan.

## Yang belum didukung

Bentuk-bentuk ini ditolak saat kompilasi dengan alasannya, tidak pernah menghasilkan program yang salah:

- sebar di pemanggilan (`f(...v)`) dan argumen bernama (`f(a: 1)`), karena urutan argumennya baru diketahui saat jalan;
- konstruktor varian sebagai nilai (`let f = Some`);
- mewarisi struct generik;
- sebagian modul `std/` yang ditulis sangat dinamis;
- `var x: T` tanpa nilai awal untuk tipe yang nilai nolnya tidak jelas (struct, enum, `Option`).

Daftar lengkapnya dikunci di `tests/llvm_parity.rs` (`BELUM_DIDUKUNG_LLVM`), dan tiap entri diperiksa masih ditolak
dengan pesan yang benar.

## Kecepatan

Untuk kerja angka dan pemanggilan fungsi, hasilnya setara C. Angka lengkapnya dan cara mengukur ulang ada di halaman
[Benchmark](https://jawarustgoplusplus.web.id/web_dokumentasi/benchmark.html) dan `benchmarks/llvm/`.
