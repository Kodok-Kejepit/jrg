#!/usr/bin/env python3
"""Pemindai kelas "mekanisme lengkap, kabelnya tidak ada".

Batasnya: cuma struct multi-baris berkoma penutup (gaya rustfmt pohon ini) —
struct satu baris TIDAK terlihat, diukur lewat sabotase di Fase KABEL."""

import re, os, sys, collections

# Kandidat: field ber-tipe Vec<..> / Option<..> / bool yang (a) selalu
# dibangun dengan nilai kosong (Vec::new(), None, false), DAN (b) tidak …

src = {}
for root, dirs, files in os.walk('src'):
    dirs[:] = [d for d in dirs if d not in ('target',)]
    for fn in files:
        if fn.endswith('.rs'):
            p = os.path.join(root, fn)
            src[p] = open(p, encoding='utf-8').read()
semua = "\n".join(src.values())

# Korpus untuk mencari PENULISAN dibuat dengan membuang badan deklarasi
# struct.
_struct_badan = re.compile(r'(^(?:pub[^\n]*?)?struct\s+[A-Z][A-Za-z0-9_]*[^\n]*\{)(.*?)(^\})', re.S | re.M)
tulis_korpus = _struct_badan.sub(lambda m: m.group(1) + m.group(3), semua)

# Kumpulkan field dari deklarasi struct
FIELD = re.compile(r'^\s*(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z0-9_]*)\s*:\s*([^,\n]+),\s*$', re.M)
struct_re = re.compile(r'^(?:pub[^\n]*?)?struct\s+([A-Z][A-Za-z0-9_]*)[^\n]*\{(.*?)^\}', re.S | re.M)

DERIVE_ISI = ('Parser', 'Args', 'Subcommand', 'Deserialize', 'Default')

kandidat = []
for path, s in src.items():
    for m in struct_re.finditer(s):
        nama_struct, badan = m.group(1), m.group(2)
        # Struct yang diisi sebuah macro derive tidak punya situs pengisian yang
        # bisa dilihat pemindai teks — clap `#[derive(Parser)]` mengisi tiap field …
        batas = s.rfind('\n}\n', 0, m.start())
        kepala = s[batas + 3 if batas >= 0 else 0:m.start()]
        if any(f'derive' in kepala and d in kepala for d in DERIVE_ISI):
            continue
        for f in FIELD.finditer(badan):
            fld, ty = f.group(1), f.group(2).strip()
            kosong = None
            if ty.startswith('Vec<'):      kosong = 'vec'
            elif ty.startswith('Option<'): kosong = 'opt'
            elif ty == 'bool':             kosong = 'bool'
            if kosong:
                kandidat.append((path, nama_struct, fld, ty, kosong))

def ditulis(fld, kind):
    """Adakah situs yang MENGISI field ini dengan sesuatu yang bukan kosong?"""
    pola = [
        rf'\.{fld}\s*\.\s*push\s*\(',
        rf'\.{fld}\s*\.\s*insert\s*\(',
        rf'\.{fld}\s*\.\s*extend\s*\(',
        rf'\.{fld}\s*\.\s*append\s*\(',
        rf'\.{fld}\s*\.\s*push_str\s*\(',
        rf'\.{fld}\s*=\s*(?!Vec::new\(\)|None|false|Default::default\(\))',
        rf'\bself\.{fld}\s*=',
        rf'\.{fld}\s*\.\s*get_or_insert',
        rf'\.{fld}\s*\.\s*take\s*\(\)\s*\.',
        # Inisialisasi bentuk singkat: `AssignStmt { targets, values, .. }`.
        # Tidak ada `targets:` di situ sama sekali, jadi tanpa pola ini
        # setiap field yang diisi bentuk singkat terbaca tidak pernah diisi.
        rf'\{{[^{{}};]*\b{fld}\s*[,}}]',
    ]
    # inisialisasi literal dengan nilai bukan-kosong Spasinya ada DI DALAM
    # lookahead, bukan di depannya.
    pola.append(rf'\b{fld}\s*:(?!\s*(?:Vec::new\(\)|None|false|Vec::with_capacity|Default::default\(\))\s*[,}}\n])')
    for p in pola:
        if re.search(p, tulis_korpus):
            return True
    return False

def dibaca(fld):
    return bool(re.search(rf'\.{fld}\b(?!\s*[:=][^=])', semua))

lapor = []
for path, st, fld, ty, kind in kandidat:
    if ditulis(fld, kind):
        continue
    if not dibaca(fld):
        continue          # tidak dibaca DAN tidak ditulis: itu dead_code, sudah ada lint-nya
    lapor.append((path, st, fld, ty))

print(f"kandidat diperiksa: {len(kandidat)}")
print(f"DIBACA tapi TIDAK PERNAH DIISI: {len(lapor)}")
for p, st, f, t in sorted(lapor):
    print(f"  {p}: {st}.{f}: {t}")

# ── `--periksa`: daftar swa-retraksi, dipanggil `tests/fmt.rs` ─────────────
# Tiap entri punya SEBABNYA. Kandidat baru memerahkan dengan menyebut namanya;
# entri yang tidak muncul lagi memerahkan juga, supaya daftarnya tidak basi.
DIKETAHUI = {
    'src/ffi/mod.rs: BuildProfile.debug_info': 'sketsa desain AOT yang sengaja belum tersambung',
    'src/ffi/mod.rs: CppHeaderInfo.classes': 'sketsa desain AOT yang sengaja belum tersambung',
    'src/ffi/mod.rs: CppMethod.vtable_index': 'sketsa desain AOT yang sengaja belum tersambung',
    'src/ffi/mod.rs: JrgManifest.cpp_sources': 'sketsa desain AOT yang sengaja belum tersambung',
}
if '--periksa' in sys.argv:
    ketemu = {f"{p}: {st}.{f}" for p, st, f, t in lapor}
    baru = sorted(ketemu - DIKETAHUI.keys())
    basi = sorted(DIKETAHUI.keys() - ketemu)
    gagal = False
    # Pemindai yang buta selalu lulus: lantainya dari 229 kandidat saat ditulis.
    if len(kandidat) < 150:
        print(f"GAGAL: cuma {len(kandidat)} kandidat diperiksa — pemindainya sepertinya buta")
        gagal = True
    for k in baru:
        print(f"GAGAL: {k} DIBACA tapi tidak pernah DIISI — mekanisme lengkap, kabelnya tidak ada?")
        gagal = True
    for k in basi:
        print(f"KABAR BAIK: {k} sekarang terisi atau hilang — hapus entrinya dari DIKETAHUI")
        gagal = True
    sys.exit(1 if gagal else 0)
