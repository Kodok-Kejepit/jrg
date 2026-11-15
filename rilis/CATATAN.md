JRG 0.3.0 membawa mesin eksekusi keempat, `--jit`, dua puluh tiga modul `std/` baru, dan pengetatan di pustaka web,
arsip, dan pengurai. Ada beberapa perubahan yang bisa membuat program lama berperilaku lain; daftarnya di bawah.

## Yang baru

- **`jrgc program.jrg --jit`**: bagian program yang sering jalan dikompilasi jadi kode mesin saat program berjalan,
  sisanya tetap di VM. Hasilnya sama persis dengan `--vm`, termasuk galat dan lokasinya. Bilangan bulat, struct,
  enum, larik dan teks semuanya ikut. Contoh: kernel teks `string_ops` turun dari 9,3 detik di `--vm` ke 0,7 detik.
- **Dua puluh tiga modul `std/` baru**, ditulis dalam JRG: `cli`, `tanggal`, `glob`, `diff`, `shlex`, `uuid`, `ini`,
  `dotenv`, `log`, `graf`, `base32`, `ip`, `mirip`, `lru`, `os`, `templat`, `bungkus`, `cron`, `sandi`, `markdown`,
  `batas`, `hitung`, `izin`.
- **Pustaka baru**: `zip::`, `tar::`, `gzip::`, `xml::`, `toml::`, `url::`, `semver::`, `jwt::`, token CSRF
  (`dok::csrf_token`), `dok::form`/`dok::multipart`, `fs::safe_join`, `crypto::compare_digest`, `env::os`/`env::arch`.
- **`http::hanya_publik(true)`** menolak alamat internal (loopback, jaringan privat, metadata awan) di klien HTTP,
  diperiksa dari alamat yang benar-benar disambungi.
- `main -> int` jadi kode keluar program di semua mesin, dan rekursi tidak lagi dibatasi 10 000 tingkat.

## Bisa memutus program lama

- `dok::serve` tanpa alamat sekarang mendengar di `127.0.0.1` (dulu `0.0.0.0`).
- WebSocket dari halaman situs lain (`Origin` beda host) dijawab 403.
- `select` dengan beberapa lengan yang siap memilih acak di semua mesin.
- Base64 cuma menerima satu ejaan untuk satu isi; `=` di tengah dan bit sisa bukan nol jadi `Err`.
- `dok::serve` lebih ketat membaca permintaan: jalur ganda ejaan, baris dan kepala cacat dijawab 400.
- `dok::db_exec`/`dok::db_query` menjalankan tepat satu pernyataan, dan jumlah parameter harus sama dengan `?`.
- `dok::render` menolak nilai di tempat yang tidak bisa diloloskan (di dalam `<script>`, atribut `on*`, dan lain-lain).
- `jrgm.toml`/`jrgm.lock` dibaca sebagai TOML 1.0 ketat, dan versi dependensi harus SemVer persis.
- `http::unduh` tidak lagi mengikuti pengalihan dari `https://` ke `http://`.

Rincian tiap perubahan, termasuk empat puluh perbaikan, ada di
[CHANGELOG.md](https://github.com/Kodok-Kejepit/jrg/blob/main/CHANGELOG.md).

## Pasang

```sh
curl -fsSL https://raw.githubusercontent.com/Kodok-Kejepit/jrg/main/rilis/pasang.sh | sh
```

Atau unduh arsip untuk sistem kamu di bawah, lalu periksa dengan berkas `.sha256`-nya. Dokumentasi:
https://jrg-lang.org
