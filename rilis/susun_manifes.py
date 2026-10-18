#!/usr/bin/env python3
"""Susun `manifes.json` dari berkas bungkusan yang ada di sebuah direktori.

Diturunkan dari berkasnya, bukan diketik: sebuah manifes yang ditulis tangan
akan melenceng pada rilis pertama yang platformnya bertambah, dan yang
melenceng membuat tiap pemasangan gagal dengan pesan yang menuduh unduhannya.
"""
import hashlib, json, os, re, sys

PLAT = re.compile(r"^jrg-(?P<versi>[^-]+)-(?P<plat>.+?)\.(tar\.gz|zip)$")

def sha256(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for blok in iter(lambda: f.read(1 << 20), b""):
            h.update(blok)
    return h.hexdigest()

def main():
    if len(sys.argv) != 3:
        sys.stderr.write("pakai: susun_manifes.py <versi> <direktori>\n")
        return 2
    versi, direktori = sys.argv[1], sys.argv[2]

    berkas = []
    for nama in sorted(os.listdir(direktori)):
        m = PLAT.match(nama)
        if not m or m.group("versi") != versi:
            continue
        jalur = os.path.join(direktori, nama)
        biner = ["jrgc.exe", "jrgm.exe"] if nama.endswith(".zip") else ["jrgc", "jrgm"]
        berkas.append({
            "platform": m.group("plat"),
            "nama": nama,
            "sha256": sha256(jalur),
            "ukuran": os.path.getsize(jalur),
            "isi": biner,
        })

    if not berkas:
        sys.stderr.write(
            "tidak ada bungkusan untuk versi %s di %s — manifes kosong lebih "
            "buruk daripada tidak ada manifes, jadi ini dihentikan\n" % (versi, direktori))
        return 1

    json.dump({
        "skema": 1,
        "versi": versi,
        "berkas": berkas,
    }, sys.stdout, indent=2, ensure_ascii=False)
    sys.stdout.write("\n")
    return 0

if __name__ == "__main__":
    sys.exit(main())
