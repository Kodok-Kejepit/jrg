# Contoh untuk backend LLVM

Program-program kecil untuk mencoba `--emit-llvm`. Semuanya juga jalan di interpreter dan `--vm`.

```sh
jrgc examples_llvm/04_enum_match_option_result.jrg --emit-llvm
```

| Berkas | Isi |
|---|---|
| `01_hello.jrg` | `wok`/`wok_`, tipe dasar |
| `02_control_flow.jrg` | `if`, `while`, `loop`, `for`, rekursi |
| `03_struct_methods.jrg` | struct, `impl`, method, sebar `...` |
| `04_enum_match_option_result.jrg` | enum berisi, `match` (rentang, atau, penjaga), `Result` dan `?`, `Option` |
| `05_format_strings_arrays.jrg` | `str::format`, array, fungsi teks |
| `06_benchmark_fib.jrg` | `fib(32)` rekursif, untuk membandingkan tiga mesin |
| `07_goroutines_channels.jrg` | goroutine dan channel |
| `08_select.jrg` | `select` atas beberapa channel |
| `10_full_power_math.jrg` | fungsi matematika LLVM, `f32`, preset `--full-power` |
| `11_layout_slice_safety.jrg` | `Option<Struct>`, potongan yang diperiksa, parameter potongan generik |

Membandingkan kecepatan:

```sh
jrgc examples_llvm/06_benchmark_fib.jrg
jrgc examples_llvm/06_benchmark_fib.jrg --vm
jrgc examples_llvm/06_benchmark_fib.jrg --emit-llvm -O3 --native-cpu -o fib --no-run && ./fib
```

Di LLVM, goroutine berjalan sebagai korutin di satu thread; lihat [panduan/backend-llvm.md](../panduan/backend-llvm.md).
