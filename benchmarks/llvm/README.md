# Benchmark LLVM lintas bahasa

Kernel-kernel kecil yang ditulis dengan algoritma yang sama dalam JRG, C, Rust, dan Go, untuk membandingkan kode
yang dihasilkan `--emit-llvm` dengan kompilator lain.

## Kernel

| Kernel | Yang diuji |
|---|---|
| `numeric_kernel` | rekurensi `u64` 200 juta langkah; kualitas kode mesin murni |
| `array_sort` | heapsort berulang di tempat, pengindeksan dan pemeriksaan batas |
| `struct_particles` | integrator partikel: struct sebagai nilai, method per langkah |
| `string_ops` | gabung, `upper`, balik, ganti teks; berat di alokasi |
| `call_recursion` | fib naif, rekursi ganda, rantai fungsi kecil |
| `match_dispatch` | mesin tumpukan dengan enum berisi dan pola rentang |
| `mixed_report` | campuran struct, array, teks, pecahan, dan `match` |

Tiap kernel mencetak checksum, dan runner berhenti kalau keluaran salah satu bahasa berbeda dari JRG.

Di luar suite:

- `string_scan.jrg`: memindai teks per indeks dengan panjang berbeda tapi jumlah karakter tetap. Waktu yang datar
  berarti `str::char_at` tetap O(1) untuk pemindaian maju.
- `output_throughput.jrg`: mengukur pencetakan. Arahkan keluarannya ke `/dev/null`.

## Menjalankan

```sh
benchmarks/llvm/run_suite.sh                         # semua kernel
ROUNDS=5 CPU=0 benchmarks/llvm/run_suite.sh          # 5 putaran, dipaku ke CPU 0
KERNELS="array_sort string_ops" benchmarks/llvm/run_suite.sh
benchmarks/llvm/run.sh                               # numeric_kernel saja, plus Python
JRGC=/jalur/jrgc JRG_FULL_POWER=1 benchmarks/llvm/run.sh
```

Runner membangun semua bahasa yang tersedia dengan penyetelan CPU setempat, memeriksa keluarannya sama, melakukan satu
pemanasan, lalu mengukur bergantian dan melaporkan rata-rata, waktu terbaik, dan ukuran biner. Rust dan Go dilewati
kalau tidak terpasang.

Agar hasil pecahan sama persis, C dikompilasi dengan `-ffp-contract=off` (tanpa FMA otomatis), dan C serta Go
mencetak `double` dengan format terpendek yang sama dengan Rust dan JRG.

## Membaca hasilnya

- Selisih waktu di bawah sekitar 6–10% tidak bisa dibedakan dari derau. Jalankan beberapa putaran dan bandingkan
  waktu terbaik.
- Rasio bisa memburuk karena pembandingnya membaik. Baca kedua kolom, bukan rasionya saja.
- Sebagian kernel memakai bilangan `u64` yang membungkus, yang ditolak interpreter dan VM. Kernel-kernel itu hanya
  berjalan lewat `--emit-llvm`.

Angka terbaru ada di halaman [Benchmark](https://jawarustgoplusplus.web.id/web_dokumentasi/benchmark.html).
