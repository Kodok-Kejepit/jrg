# Merilis JRG

Folder ini berisi perkakas untuk membuat rilis. Rilisnya sendiri dibuat lewat tag git.

| Berkas | Gunanya |
|---|---|
| [`VERSI.md`](VERSI.md) | aturan penomoran versi dan apa yang dihitung memutus |
| `bungkus.sh` | bangun, bungkus, dan hitung checksum untuk platform mesin ini |
| `uji_asap.sh` | uji cepat atas biner jadi: versi, uji konformans di dua mesin, satu proyek `jrgm`, `--emit-llvm` |
| `susun_manifes.py` | susun `manifes.json` dari berkas-berkas bungkusan |
| `periksa_manifes.py` | pastikan manifes cocok dengan berkasnya |
| `pasang.sh` | pemasang untuk pemakai, tanpa toolchain Rust |
| `manifes.contoh.json` | bentuk manifesnya |
| `../.github/workflows/rilis.yml` | membangun semua platform saat tag didorong |
| `../.github/workflows/uji.yml` | tes tiap ada dorongan |

## Langkah merilis

```sh
# 1. Pastikan semua tes hijau
cargo test --release -j 1 --no-fail-fast

# 2. Naikkan versi di Cargo.toml, lalu ubah `## [Belum dirilis]` di CHANGELOG.md
#    jadi `## [0.3.0] — <tanggal>`

# 3. Coba bungkusannya di mesin sendiri
sh rilis/bungkus.sh
sh rilis/uji_asap.sh target/release 0.3.0

# 4. Buat tag dan dorong; workflow rilis membangun semua platform
git tag -a v0.3.0 -m "v0.3.0"
git push origin v0.3.0
```

Langkah 3 menangkap kesalahan isi arsip sebelum sampai ke runner.

## Platform

| Platform | Dibangun di | Diuji di sana |
|---|---|---|
| `linux-x86_64` | `ubuntu-latest` | ya |
| `linux-aarch64` | `ubuntu-24.04-arm` | ya |
| `macos-aarch64` | `macos-latest` | ya |
| `macos-x86_64` | `macos-latest` (target Intel) | kalau runnernya punya Rosetta |
| `windows-x86_64` | `windows-latest` | ya |
| `android-aarch64` (Termux) | `ubuntu-latest` dengan NDK | tidak, biner Android cuma jalan di HP |

Yang diuji di runner adalah biner jadinya, lewat `uji_asap.sh`. Arsip tiap platform membawa `jrgc`, `jrgm`, dan
pustaka untuk `--emit-llvm` (runtime dan jembatan builtin), jadi pemakai cukup memasang `clang` untuk mengompilasi
program jadi native. `pasang.sh` menaruh pustakanya di `../lib/jrg` dari folder binernya, dan di Termux memasang ke
`$PREFIX/bin`. Di Windows `--emit-llvm` belum diuji, jadi belum dijanjikan.
