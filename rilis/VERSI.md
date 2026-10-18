# Penomoran versi

JRG mengikuti [Semantic Versioning 2.0.0](https://semver.org/lang/id/).

```
MAYOR.MINOR.TAMBALAN        contoh: 0.2.0
```

## Apa yang dihitung "memutus"

Selama **0.x**, menurut SemVer sendiri, apa pun boleh berubah di MINOR. Proyek
ini tetap mencatat tiap perubahan yang memutus di `CHANGELOG.md` dengan judul
**MEMUTUS**, karena "boleh berubah" bukan alasan untuk membuat orang menebak.

Yang dihitung memutus:

- Sebuah fungsi bawaan berganti **tipe kembalian** atau **urutan argumen**.
- Sebuah program yang tadinya jalan jadi ditolak.
- Sebuah keluaran berubah bentuknya (`wok` mencetak sesuatu dengan cara lain).

Yang **tidak** dihitung memutus:

- Program yang tadinya **ditolak** jadi diterima.
- Pesan galat berubah kata-katanya. Isinya tetap menyebut hal yang sama, dan
  membekukan kalimat galat berarti tidak bisa memperbaikinya lagi.
- Sebuah nama bawaan baru ditambahkan.

## Di mana versinya hidup

Satu tempat: `Cargo.toml`. `jrgc --version` dan `jrgm --version` membacanya
lewat `env!("CARGO_PKG_VERSION")`, jadi tidak ada angka kedua yang bisa
melenceng.

## Sebelum 1.0.0

`0.x` berarti bahasanya masih bisa berubah bentuk. Yang sudah tidak akan
berubah lagi sebelum 1.0:

- semantik nilai (nilai tidak pernah beralias),
- tiga mesin menjawab sama,
- galat tidak pernah diam.

Yang masih bisa berubah: nama dan tanda tangan di pustaka standar, dan sintaks
yang belum punya tes paritas.

## Yang menahan 1.0.0

1. Backend LLVM belum menjalankan seluruh bahasa: sebar di pemanggilan, argumen bernama, konstruktor sebagai nilai, dan
   pewarisan struct generik masih ditolak.
2. Belum ada registri paket pusat; dependensi diambil dari folder lokal atau repositori git.
3. Urutan argumen di pustaka standar belum seragam antar-namespace (`re::` memakai `(pola, teks)`, sementara `str::` dan
   `arr::` memakai `(subjek, …)`).
4. Beberapa aturan bahasa masih dibahas, misalnya `for i in a..b` yang memberi indeks, dan diskriminan enum.
