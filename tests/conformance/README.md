# Suite konformansi JRG

Setiap file `.jrg` di direktori ini adalah **spesifikasi semantik bahasa
yang bisa dieksekusi**. Isinya program JRG yang memakai `assert`/`assert_eq`
untuk menyatakan apa arti tiap konstruksi. Kalau sebuah program di sini
selesai tanpa error, berarti implementasinya sesuai spesifikasi.

Yang membuatnya berguna: file-file ini bukan test untuk interpreter saja.
Interpreter adalah sumber kebenaran semantik JRG, dan suite yang sama nanti
dijalankan terhadap VM dan backend LLVM untuk menyeret ketiganya ke
semantik yang sama. Jadi begitu sebuah aturan ditulis di sini, aturan itu
mengikat semua backend — bukan cuma yang kebetulan sudah mengimplementasi.

    sumber .jrg
        v
    perilaku yang diharapkan (assert di dalam file)
        v
    interpreter  ->  VM  ->  LLVM
        v
    hasil

## Menjalankan

Dijalankan otomatis oleh test Rust `conformance_suite_passes` di
`tests/conformance.rs`:

```bash
cargo test --release conformance
```

Untuk menjalankan satu file secara manual saat mengembangkan:

```bash
jrgc tests/conformance/control_flow.jrg
```

## Aturan menulis file baru

- Satu file per area bahasa; nama file = nama areanya.
- Pakai `assert_eq(aktual, harapan)` supaya kegagalan menyebutkan kedua
  nilainya, bukan cuma "false".
- Setiap assert menguji SATU aturan, dengan komentar yang menyatakan
  aturannya — file ini dibaca sebagai dokumentasi juga.
- Sertakan kasus batas (kosong, satu elemen, bersarang), bukan cuma
  jalur bahagia.
- Program harus berakhir sendiri (tidak boleh ada server/loop tak
  berujung) dan tidak boleh bergantung pada file eksternal, jaringan,
  waktu, atau keacakan.

## Yang SENGAJA tidak diuji di sini

Perilaku yang belum jadi bagian semantik resmi JRG, atau yang berbeda
antar-backend dan belum diputuskan mana yang benar, tidak ditulis di sini
supaya suite ini tidak mengunci sesuatu yang masih bisa berubah.
