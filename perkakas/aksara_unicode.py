#!/usr/bin/env python3
# perkakas/aksara_unicode.py — bangkitkan `src/lexer/aksara_tabel.rs` (aksara + NFC untuk identifier).
#   python3 perkakas/aksara_unicode.py            tulis ulang tabelnya
#   python3 perkakas/aksara_unicode.py --periksa  bandingkan saja: 0 sama, 1 beda, 3 data Unicode lain/tidak ada
import os
import sys
import unicodedata as u

DATA = os.environ.get("JRG_DATA_UNICODE", "/usr/share/unicode")
AKAR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TUJUAN = os.path.join(AKAR, "src", "lexer", "aksara_tabel.rs")
SEMU = ["Jpan", "Kore", "Hanb"]  # himpunan tambahan UTS #39 §5.1


def baris(nama):
    with open(os.path.join(DATA, nama), encoding="utf-8") as f:
        for b in f:
            b = b.split("#", 1)[0].strip()
            if b:
                yield [x.strip() for x in b.split(";")]


def rentang(t):
    a, _, z = t.partition("..")
    return int(a, 16), int(z or a, 16)


def versi():
    with open(os.path.join(DATA, "Scripts.txt"), encoding="utf-8") as f:
        return f.readline().strip().lstrip("# ").replace("Scripts-", "").replace(".txt", "")


def bangun():
    pendek = {}  # nama panjang -> kode empat huruf
    for b in baris("PropertyValueAliases.txt"):
        if b[0] == "sc":
            pendek[b[2]] = b[1]
    skrip = {}
    for b in baris("Scripts.txt"):
        a, z = rentang(b[0])
        for cp in range(a, z + 1):
            skrip[cp] = pendek[b[1]]
    ext = {}
    for b in baris("ScriptExtensions.txt"):
        a, z = rentang(b[0])
        for cp in range(a, z + 1):
            ext[cp] = tuple(sorted(b[1].split()))
    kode = sorted({s for s in skrip.values() if s not in ("Zyyy", "Zinh", "Zzzz")}
                  | {s for t in ext.values() for s in t}) + SEMU
    assert len(kode) <= 192, len(kode)
    nomor = {k: i for i, k in enumerate(kode)}
    nama = {}
    for b in baris("PropertyValueAliases.txt"):
        if b[0] == "sc" and b[1] in nomor:
            nama[b[1]] = b[2].replace("_", " ")
    nama.update({"Jpan": "Jepang", "Kore": "Korea", "Hanb": "Han-Bopomofo", "Latn": "Latin",
                 "Cyrl": "Kiril", "Grek": "Yunani", "Armn": "Armenia", "Arab": "Arab", "Hani": "Han"})

    # Aksara per kode: skrip tunggal, kecuali yang punya Script_Extensions.
    satu, banyak = [], []
    for cp in range(0x80, 0x110000):
        if cp in ext:
            banyak.append((cp, tuple(nomor[s] for s in ext[cp])))
        else:
            s = skrip.get(cp, "Zzzz")
            if s not in ("Zyyy", "Zinh", "Zzzz"):
                satu.append((cp, nomor[s]))
    satu_r = gabung(satu)
    banyak_r = gabung(banyak)

    # NFC: kombinasi kanonik (bukan pengecualian), kelas gabung, dan yang tak pernah muncul di NFC.
    pasang, ccc, qc_tidak = [], [], []
    for cp in range(0x110000):
        c = chr(cp)
        if 0xD800 <= cp <= 0xDFFF:
            continue
        k = u.combining(c)
        if k:
            ccc.append((cp, k))
        if u.normalize("NFC", c) != c:
            qc_tidak.append((cp, u.normalize("NFC", c)))
        d = u.decomposition(c)
        if d and not d.startswith("<"):
            bag = [int(x, 16) for x in d.split()]
            if len(bag) == 2 and u.normalize("NFC", chr(bag[0]) + chr(bag[1])) == c:
                pasang.append((bag[0], bag[1], cp))
    pasang.sort()
    return {"versi": versi(), "nfc": u.unidata_version, "kode": kode, "nama": [nama.get(k, k) for k in kode],
            "satu": satu_r, "banyak": banyak_r, "ccc": gabung(ccc), "qc": qc_tidak, "pasang": pasang}


def gabung(isi):
    """[(cp, nilai)] berurut -> [(awal, akhir, nilai)] untuk kode berurutan bernilai sama."""
    hasil = []
    for cp, v in isi:
        if hasil and hasil[-1][1] == cp - 1 and hasil[-1][2] == v:
            hasil[-1][1] = cp
        else:
            hasil.append([cp, cp, v])
    return hasil


def tulis(t):
    o = ["// src/lexer/aksara_tabel.rs — DIBANGKITKAN `perkakas/aksara_unicode.py` dari Scripts.txt,",
         "// ScriptExtensions.txt dan `unicodedata` Python. Jangan disunting tangan: jalankan ulang skripnya.",
         "",
         'pub const VERSI_AKSARA: &str = "%s";' % t["versi"],
         'pub const VERSI_NFC: &str = "%s";' % t["nfc"],
         "",
         "pub static NAMA: &[&str] = &[%s];" % ", ".join('"%s"' % n for n in t["nama"]),
         "pub static KODE: &[&str] = &[%s];" % ", ".join('"%s"' % n for n in t["kode"]),
         "",
         "pub static SATU: &[(u32, u32, u8)] = &["]
    o += ["    (0x%04X, 0x%04X, %d)," % (a, z, v) for a, z, v in t["satu"]]
    o += ["];", "", "pub static BANYAK: &[(u32, u32, &[u8])] = &["]
    o += ["    (0x%04X, 0x%04X, &[%s])," % (a, z, ", ".join(map(str, v))) for a, z, v in t["banyak"]]
    o += ["];", "", "pub static CCC: &[(u32, u32, u8)] = &["]
    o += ["    (0x%04X, 0x%04X, %d)," % (a, z, v) for a, z, v in t["ccc"]]
    o += ["];", "", "/// Kode yang tidak pernah muncul di NFC, dengan bentuk NFC-nya.",
          "pub static BUKAN_NFC: &[(u32, &str)] = &["]
    o += ['    (0x%04X, "%s"),' % (cp, "".join("\\u{%X}" % ord(x) for x in s)) for cp, s in t["qc"]]
    o += ["];", "", "/// Dua kode yang NFC gabungkan jadi satu (pertama, kedua, hasil), urut.",
          "pub static PASANG: &[(u32, u32, u32)] = &["]
    o += ["    (0x%04X, 0x%04X, 0x%04X)," % p for p in t["pasang"]]
    o += ["];", ""]
    return "\n".join(o)


def main():
    try:
        t = bangun()
    except FileNotFoundError as e:
        print("data Unicode tidak ada: %s" % e.filename)
        return 3
    teks = tulis(t)
    if "--periksa" in sys.argv:
        try:
            lama = open(TUJUAN, encoding="utf-8").read()
        except FileNotFoundError:
            lama = ""
        if lama == teks:
            print("tabel aksara cocok dengan Unicode %s / unicodedata %s" % (t["versi"], t["nfc"]))
            return 0
        tertulis = [b for b in lama.splitlines() if b.startswith("pub const VERSI")]
        if tertulis != [b for b in teks.splitlines() if b.startswith("pub const VERSI")]:
            print("data Unicode di sini lain versi (%s / %s)" % (t["versi"], t["nfc"]))
            return 3
        print("tabel aksara TIDAK cocok dengan pembangkitnya")
        return 1
    with open(TUJUAN, "w", encoding="utf-8") as f:
        f.write(teks)
    print("ditulis %s: %d rentang aksara, %d berekstensi, %d ccc, %d bukan-NFC, %d pasang"
          % (TUJUAN, len(t["satu"]), len(t["banyak"]), len(t["ccc"]), len(t["qc"]), len(t["pasang"])))
    return 0


if __name__ == "__main__":
    sys.exit(main())
