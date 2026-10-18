#!/usr/bin/env python3
"""Pindahkan penjelasan panjang dari komentar ke `dokumentasi/`."""
import os, re, subprocess, sys, collections

BATAS = 3          # blok lebih panjang dari ini dipangkas
SIMPAN = 2         # berapa baris yang ditinggal di sumber

def berkas_proyek():
    out = subprocess.run(['git','ls-files'],capture_output=True,text=True).stdout.split('\n')
    return [f for f in out if f.endswith(('.rs','.jrg','.py','.js','.sh')) and os.path.exists(f)]

def penanda(path):
    return '#' if path.endswith(('.py','.sh')) else '//'

def keadaan_awal_baris(teks, gaya, rust=False):
    """Keadaan leksikal di AWAL tiap baris: 0 normal, 1 string, 2 raw, 3
    blok-komentar."""
    hasil = []
    st = 0; pagar = 0; i = 0; n = len(teks); baris_baru = True
    for baris in teks.split('\n'):
        hasil.append(st)
        j = 0; L = len(baris)
        while j < L:
            c = baris[j]
            if st == 0:
                if gaya == '//' and c == '/' and j+1 < L and baris[j+1] == '/':
                    break                                  # komentar baris
                if gaya == '#' and c == '#':
                    break
                if gaya == '//' and c == '/' and j+1 < L and baris[j+1] == '*':
                    st = 3; j += 2; continue
                if c == 'r' and gaya == '//':
                    k = j+1; p = 0
                    while k < L and baris[k] == '#': p += 1; k += 1
                    if k < L and baris[k] == '"':
                        st = 2; pagar = p; j = k+1; continue
                if c == '"': st = 1; j += 1; continue
                if c == "'":
                    # literal char (bukan lifetime) kalau tertutup dekat
                    m = re.match(r"'(\\.|[^\\'])'", baris[j:])
                    if m: j += m.end(); continue
                    # Di luar Rust kutip satu membuka teks: `'/*'` di JS dulu terbaca komentar blok.
                    if not rust: st = 4; j += 1; continue
                j += 1
            elif st == 4:
                if c == '\\': j += 2; continue
                if c == "'": st = 0
                j += 1
            elif st == 1:
                if c == '\\': j += 2; continue
                if c == '"': st = 0
                j += 1
            elif st == 2:
                if c == '"':
                    if baris[j+1:j+1+pagar] == '#'*pagar: st = 0; j += 1+pagar; continue
                j += 1
            else:
                if c == '*' and j+1 < L and baris[j+1] == '/': st = 0; j += 2; continue
                j += 1
        # String Rust boleh membentang beberapa baris tanpa `\`; bahasa lain tidak.
        # Dulu direset untuk Rust juga, dan `tests/*.jrg` di teks bantuan clap terbaca komentar blok.
        if st in (1, 4) and not rust and not baris.rstrip().endswith('\\'):
            st = 0
    return hasil

def baris_komentar(baris, gaya, st):
    if st == 3: return True
    if st != 0: return False
    t = baris.strip()
    return t.startswith(gaya) if gaya == '//' else t.startswith('#') and not t.startswith('#!')

def isi_komentar(baris, gaya):
    t = baris.strip()
    if gaya == '//':
        t = re.sub(r'^//[/!]?', '', t)
    else:
        t = re.sub(r'^#+', '', t)
    return t.strip()

def jenis(baris):
    t = baris.strip()
    if t.startswith('//!'): return '//!'
    if t.startswith('///'): return '///'
    return '//'

def blok_komentar(lines, gaya, keadaan):
    # Pergantian `//` <-> `///`/`//!` MEMUTUS blok: dulu keduanya digabung dan
    # dipangkas bergaya `//`, jadi kalimat dokumen (`--doc`, bantuan clap) hilang.
    blok = []; run = None
    for i, l in enumerate(lines):
        if baris_komentar(l, gaya, keadaan[i]):
            if run is not None and gaya == '//' and keadaan[i] == 0 and jenis(l) != jenis(lines[run]):
                if i - run > BATAS: blok.append((run, i))
                run = None
            if run is None: run = i
        else:
            if run is not None and i - run > BATAS: blok.append((run, i))
            run = None
    if run is not None and len(lines) - run > BATAS: blok.append((run, len(lines)))
    return blok

def jangkar(lines, akhir):
    """Baris kode pertama sesudah blok — dipakai sebagai judul di dokumentasi."""
    for l in lines[akhir:akhir+6]:
        t = l.strip()
        if t and not t.startswith(('#[', '#!')):
            return re.sub(r'\s*\{\s*$', '', t)[:110]
    return None

def kalimat_pertama(prosa):
    """Kalimat pertama dari sebuah blok, dilipat ke <= SIMPAN baris."""
    par = []
    for p in prosa:
        if not p: break
        par.append(p)
    teks = ' '.join(par).strip()
    if not teks:
        for p in prosa:
            if p: teks = p; break
    m = re.search(r'(?<!\bmis)(?<!\bdst)(?<!\bdll)\.(\s|$)', teks)
    if m: teks = teks[:m.end()].strip()
    baris = []
    for kata in teks.split():
        if baris and len(baris[-1]) + 1 + len(kata) <= 74: baris[-1] += ' ' + kata
        else: baris.append(kata)
        if len(baris) > SIMPAN: break
    if len(baris) > SIMPAN: baris = baris[:SIMPAN]; baris[-1] = baris[-1].rstrip(' .,;…') + ' …'
    return baris

def olah(path, tulis=True):
    gaya = penanda(path)
    teks = open(path, encoding='utf-8').read()
    lines = teks.split('\n')
    keadaan = keadaan_awal_baris(teks, gaya, path.endswith('.rs'))
    blok = blok_komentar(lines, gaya, keadaan)
    if not blok: return None
    kode_sebelum = [l for i, l in enumerate(lines) if not baris_komentar(l, gaya, keadaan[i])]

    dok = []          # (judul, prosa[])
    keluar = list(lines)
    for mulai, akhir in reversed(blok):
        asli = lines[mulai:akhir]
        prosa = [isi_komentar(l, gaya) for l in asli]
        j = jenis(asli[0]) if gaya == '//' else '#'
        indent = re.match(r'\s*', asli[0]).group(0)
        judul = jangkar(lines, akhir) or f'{path}:{mulai+1}'
        dok.append((judul, prosa, mulai + 1))
        ringkas = kalimat_pertama(prosa)
        baru = [f'{indent}{j} {b}'.rstrip() for b in ringkas]
        keluar[mulai:akhir] = baru

    if tulis:
        hasil = '\n'.join(keluar)
        k2 = keadaan_awal_baris(hasil, gaya, path.endswith('.rs'))
        l2 = hasil.split('\n')
        kode_sesudah = [l for i, l in enumerate(l2) if not baris_komentar(l, gaya, k2[i])]
        if kode_sebelum != kode_sesudah:
            print(f'  TOLAK {path}: baris non-komentar berubah', file=sys.stderr)
            return None
        open(path, 'w', encoding='utf-8').write(hasil)
    return list(reversed(dok))

KEPALA_DOK = """# {path}"""

def jalankan(tulis):
    total_blok = 0; total_baris = 0; tersentuh = []
    for p in berkas_proyek():
        d = olah(p, tulis=tulis)
        if not d: continue
        tersentuh.append(p); total_blok += len(d)
        if not tulis:
            for judul, prosa, ln in d:
                total_baris += len(prosa)
                print(f"  {p}:{ln}  {len(prosa)} baris  ({judul[:60]})")
            continue
        keluar = os.path.join('dokumentasi', os.path.splitext(p)[0] + '.md')
        os.makedirs(os.path.dirname(keluar), exist_ok=True)
        # Cermin yang sudah ada DITAMBAHI, tidak ditimpa: isinya penjelasan yang
        # dipindah lebih dulu, dan tidak ada salinan lainnya di pohon kerja.
        ada = os.path.exists(keluar)
        bagian = [] if ada else [KEPALA_DOK.format(path=p)]
        for judul, prosa, ln in d:
            total_baris += len(prosa)
            while prosa and not prosa[0]: prosa.pop(0)
            while prosa and not prosa[-1]: prosa.pop()
            bagian.append(f"\n## `{judul}`\n\n<sub>sumber: `{p}` baris {ln}</sub>\n")
            bagian.append('\n'.join(prosa) + '\n')
        open(keluar, 'a' if ada else 'w', encoding='utf-8').write(''.join(bagian))
    return tersentuh, total_blok, total_baris

PAKAI = """pakai: python3 perkakas/rapikan_komentar.py [--tulis]
  tanpa argumen : PERIKSA saja — sebut blok komentar yang lebih dari 3 baris,
                  tidak menulis apa pun; keluar 1 kalau ada.
  --tulis       : pangkas blok itu di sumbernya dan pindahkan penjelasannya ke
                  dokumentasi/ (cermin yang sudah ada ditambahi, bukan ditimpa)."""

if __name__ == '__main__':
    # Menulis ulang ratusan berkas terlacak harus DIMINTA. Dulu argumen apa pun,
    # termasuk `--help`, langsung menjalankannya.
    arg = sys.argv[1:]
    if arg not in ([], ['--tulis']):
        print(PAKAI); sys.exit(2)
    tulis = arg == ['--tulis']
    t, b, l = jalankan(tulis)
    print(f"berkas {'dirapikan' if tulis else 'melanggar'} : {len(t)}")
    print(f"blok {'dipindah' if tulis else 'kepanjangan'}    : {b}")
    print(f"baris prosa      : {l}")
    if not tulis and t: sys.exit(1)
