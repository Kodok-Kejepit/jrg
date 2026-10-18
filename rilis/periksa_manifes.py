#!/usr/bin/env python3
"""Periksa bahwa tiap entri manifes cocok dengan berkasnya.

Dijalankan SEBELUM rilis terbit. Sebuah manifes yang checksumnya meleset tidak
menghasilkan galat di mana pun sampai seseorang memasangnya, dan pesan yang ia
lihat menuduh unduhannya — bukan manifesnya.
"""
import hashlib, json, os, sys

def sha256(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for blok in iter(lambda: f.read(1 << 20), b""):
            h.update(blok)
    return h.hexdigest()

def main():
    if len(sys.argv) != 3:
        sys.stderr.write("pakai: periksa_manifes.py <manifes.json> <direktori>\n")
        return 2
    manifes, direktori = sys.argv[1], sys.argv[2]
    m = json.load(open(manifes, encoding="utf-8"))

    salah = []
    for b in m["berkas"]:
        jalur = os.path.join(direktori, b["nama"])
        if not os.path.exists(jalur):
            salah.append("%s: berkasnya tidak ada" % b["nama"]); continue
        nyata = sha256(jalur)
        if nyata != b["sha256"]:
            salah.append("%s: sha256 manifes %s, berkas %s"
                         % (b["nama"], b["sha256"], nyata))
        uk = os.path.getsize(jalur)
        if uk != b["ukuran"]:
            salah.append("%s: ukuran manifes %d, berkas %d" % (b["nama"], b["ukuran"], uk))

    if salah:
        sys.stderr.write("manifes tidak cocok dengan berkasnya:\n")
        for s in salah:
            sys.stderr.write("  " + s + "\n")
        return 1

    print("manifes cocok: %d berkas, versi %s" % (len(m["berkas"]), m["versi"]))
    return 0

if __name__ == "__main__":
    sys.exit(main())
