# Model memori JRG

Aturan tentang siapa memiliki sebuah nilai, sampai kapan, dan kapan ia dibebaskan. Aturan ini berlaku di ketiga mesin;
cara mewujudkannya di dalam `jrgc` boleh berubah selama aturannya tetap.

## Nilai tidak pernah berbagi

Setiap nilai dimiliki tepat satu nama. Menugaskan, mengoper ke fungsi, mengembalikan, dan membaca dari koleksi
menghasilkan nilai yang berdiri sendiri: mengubah yang satu tidak pernah mengubah yang lain.

```jrg
var a = [1, 2, 3]
var b = a
b[0] = 99
wok(a, b)        // [1, 2, 3] [99, 2, 3]
```

Akibatnya:

- umur sebuah nilai sama dengan umur nama pemiliknya, dan ia dibebaskan tepat saat nama itu keluar dari lingkupnya;
- tidak ada pengumpul sampah, dan tidak dibutuhkan;
- *use-after-free*, *double free*, dan pointer menggantung tidak bisa terjadi pada nilai biasa;
- `self` di dalam method adalah salinan. Method yang mengubah sesuatu mengembalikan nilai baru: `a = a.setor(5)`.

Pemilik sebuah penyimpanan juga tidak pernah teramati: `==`, `<`, dan pengurutan selalu membandingkan isi.

## Menyalin itu murah

Salinan sebenarnya baru dibuat saat ada yang menulis (*copy-on-write*), dan setiap penulisan melepas perkongsian di
setiap tingkat jalur tulisnya, bukan cuma di akarnya.

| Operasi | Biaya |
|---|---|
| `var b = a`, mengoper, mengembalikan | tetap, berapa pun ukuran `a` |
| `a[i] = v`, `p.x = v` | sebanding kedalaman jalur, bukan ukuran |
| `a = a.push(x)`, `a = a + [x]`, `d = dict::set(d, k, v)` | linear untuk n kali tambah |
| `x = x.m(..)` | penerima dipindahkan ke method, jadi method yang mengubah `self` bekerja di tempat |
| membaca satu nilai (`a.get(i)`, `dict::get`) | tidak menyalin koleksi |
| `a = [x] + a` | kuadratik untuk n kali tambah; pakai `std/deque` |

Biaya ini dikunci dengan tes berbasis rasio di `tests/memory_model.rs` dan `tests/vm_complexity.rs`: inputnya
diperbesar delapan kali, lalu waktunya harus tumbuh sesuai kelas biayanya.

## Yang sengaja berbagi

| Apa | Kenapa |
|---|---|
| variabel yang ditangkap closure | closure dan lingkup asalnya memiliki variabel itu bersama; perubahan terlihat dua arah. `move \|\| …` menangkap salinan |
| `Atomic`, `Mutex`, channel | gunanya memang berbagi keadaan antar-goroutine |
| `ptr::new(v)` | pegangan eksplisit ke satu nilai bersama |
| `jawa::alloc` / `jawir::free` | alokasi manual yang diperiksa |
| handle `py::`, `js::`, `php::`, `jvm::`, kanvas | menunjuk sesuatu yang hidup di luar memori JRG |

Untuk semuanya, yang disalin adalah **pegangannya**, bukan penyimpanannya. Umurnya tetap mengikuti aturan di atas:
penyimpanan dibebaskan saat pegangan terakhir hilang, dan objek Python dilepas di sisi Python tanpa perlu
memanggil apa pun.

## Alokasi manual

`jawa::` meniru `malloc`/`free` tanpa pointer mentah: "alamatnya" adalah nomor di sebuah registri, jadi salah pakai
menghasilkan galat, bukan kerusakan memori.

```jrg
let p = jawa::alloc(42)
let q = p
jawa::set(q, 99)
wok(jawa::get(p))        // 99: p dan q menunjuk tempat yang sama
jawir::free(q)
wok(jawa::is_valid(p))   // false
jawa::get(p)             // galat: use-after-free
```

## Batas yang diketahui

Closure yang memanggil dirinya sendiri lewat variabel yang ia tangkap membentuk lingkaran kepemilikan yang tidak pernah
dibebaskan. Pada 200 ribu closure semacam itu, sekitar 460 MB tertahan di interpreter dan 46 MB di VM. Untuk rekursi,
pakai `fn` bernama.

## Ringkasan

1. Nilai dimiliki satu nama; memberikannya ke tempat lain menghasilkan salinan.
2. Salinan baru dibuat saat ada yang menulis, jadi memberikan nilai itu murah.
3. Umur nilai sama dengan umur namanya.
4. Yang berbagi hanya tangkapan closure dan pegangan yang memang dibuat untuk itu.
5. Tidak ada pengumpul sampah.
