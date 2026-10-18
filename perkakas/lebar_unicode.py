#!/usr/bin/env python3
# perkakas/lebar_unicode.py — bangkitkan tabel lebar tampil (`src/lebar/tabel.rs`).
#   python3 perkakas/lebar_unicode.py            tulis ulang tabelnya
#   python3 perkakas/lebar_unicode.py --periksa  bandingkan saja: 0 sama, 1 beda, 3 versi Unicode lain
import sys
import unicodedata as u

KELUAR = "src/lebar/tabel.rs"


def lebar(cp):
    """Kolom layar satu karakter: 0, 1 atau 2 (aturan `wcwidth` yang umum dipakai terminal)."""
    c = chr(cp)
    kat = u.category(c)
    # Kendali, pengganti (surrogate), pemisah baris/paragraf: tidak punya lebar tetap.
    if kat in ("Cc", "Cs", "Zl", "Zp"):
        return 0
    # Tanda hubung lunak tampil sebagai satu kolom walau berkategori Cf.
    if cp == 0x00AD:
        return 1
    # Tanda gabung dan karakter format menumpang di kolom huruf sebelumnya.
    if kat in ("Mn", "Me", "Cf"):
        return 0
    # Vokal dan konsonan akhir Jamo Hangul menyatu dengan konsonan awalnya.
    if 0x1160 <= cp <= 0x11FF:
        return 0
    if u.east_asian_width(c) in ("W", "F"):
        return 2
    return 1


def rentang(nilai):
    """Daftar (awal, akhir) inklusif dari semua titik kode yang lebarnya `nilai`."""
    hasil = []
    awal = None
    for cp in range(0x110000):
        kena = lebar(cp) == nilai
        if kena and awal is None:
            awal = cp
        elif not kena and awal is not None:
            hasil.append((awal, cp - 1))
            awal = None
    if awal is not None:
        hasil.append((awal, 0x10FFFF))
    return hasil


def tabel(nama, isi):
    baris = ["pub static %s: &[(u32, u32)] = &[" % nama]
    for i in range(0, len(isi), 4):
        potong = isi[i:i + 4]
        baris.append("    " + " ".join("(0x%04X, 0x%04X)," % p for p in potong))
    baris.append("];")
    return "\n".join(baris)


def bangun():
    return "\n".join([
        "// src/lebar/tabel.rs — DIBANGKITKAN `perkakas/lebar_unicode.py` dari `unicodedata`",
        "// milik Python. Jangan disunting tangan: jalankan ulang skripnya.",
        "",
        'pub const VERSI_UNICODE: &str = "%s";' % u.unidata_version,
        "",
        tabel("NOL", rentang(0)),
        "",
        tabel("DUA", rentang(2)),
        "",
    ])


def versi_tercatat():
    try:
        for b in open(KELUAR, encoding="utf-8"):
            if b.startswith("pub const VERSI_UNICODE"):
                return b.split('"')[1]
    except OSError:
        pass
    return None


def main():
    if "--periksa" in sys.argv:
        tercatat = versi_tercatat()
        if tercatat != u.unidata_version:
            print("versi Unicode beda: tabel %s, python %s" % (tercatat, u.unidata_version))
            return 3
        lama = open(KELUAR, encoding="utf-8").read()
        if lama == bangun():
            print("tabel lebar cocok dengan unicodedata %s" % u.unidata_version)
            return 0
        print("tabel lebar TIDAK cocok dengan unicodedata %s" % u.unidata_version)
        return 1
    open(KELUAR, "w", encoding="utf-8").write(bangun())
    print("%s ditulis (Unicode %s)" % (KELUAR, u.unidata_version))
    return 0


if __name__ == "__main__":
    sys.exit(main())
