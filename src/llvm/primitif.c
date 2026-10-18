// primitif.c — primitif memori biner `--emit-llvm` yang ditulis dalam C (lapis kedua dari
// arsitektur tiga lapis: Rust kompiler, C primitif, pustaka standar dalam JRG).
#include <stdint.h>
#include <string.h>

void *jrg_alloc(int64_t ukuran, int64_t jajar);

typedef struct { void *ptr; int64_t len; } JrgSlice;
// Kapasitas penyangga yang sedang dipegang sebuah variabel, dan penyangga MANA yang dimaksud.
typedef struct { void *pemilik; int64_t kap; } JrgKap;

// `a = a + isi` di tempat: menulis ke sisa kapasitas, atau pindah ke penyangga dua kali
// lebih besar. Kapasitas cuma berlaku kalau `s->ptr` masih penyangga yang dicatat `k`.
void jrg_slice_tumbuh(JrgSlice *s, JrgKap *k, const void *isi, int64_t n, int64_t ukuran, int64_t jajar) {
    int64_t len = s->len, baru = len + n;
    if (k->pemilik == 0 || s->ptr != k->pemilik || baru > k->kap) {
        int64_t kap = baru * 2;
        if (kap < 8) kap = 8;
        void *p = jrg_alloc(kap * ukuran, jajar);
        if (len > 0) memcpy(p, s->ptr, (size_t)(len * ukuran));
        s->ptr = p;
        k->pemilik = p;
        k->kap = kap;
    }
    if (n > 0) memcpy((char *)s->ptr + len * ukuran, isi, (size_t)(n * ukuran));
    s->len = baru;
}

typedef struct { const char *ptr; int64_t len; } JrgStr;

// `str::join(isi, pemisah)`: satu alokasi sebesar hasilnya, lalu disalin berurutan.
void jrg_str_join(const JrgStr *isi, int64_t n, const char *sp, int64_t sl, const char **op, int64_t *ol) {
    int64_t total = n > 1 ? (n - 1) * sl : 0;
    for (int64_t i = 0; i < n; i++) total += isi[i].len;
    char *p = total > 0 ? jrg_alloc(total, 1) : (char *)"";
    char *t = p;
    for (int64_t i = 0; i < n; i++) {
        if (i > 0 && sl > 0) { memcpy(t, sp, (size_t)sl); t += sl; }
        if (isi[i].len > 0) { memcpy(t, isi[i].ptr, (size_t)isi[i].len); t += isi[i].len; }
    }
    *op = p;
    *ol = total;
}

// ── dict: pasangan `{kunci, nilai}` berurutan sisip + indeks hash di kepala penyangga ──
#include <stdlib.h>

typedef struct { void *ptr; int64_t len; } JrgDict;
// Duduk tepat SEBELUM pasangan pertama; 64 bita supaya perataan pasangannya tetap.
typedef struct {
    int64_t kap;        // kapasitas penyangga, dalam pasangan
    int32_t *indeks;    // slot hash: nomor pasangan + 1, 0 = kosong
    int64_t n_indeks;   // pangkat dua, atau 0 kalau belum ada
    int64_t terindeks;  // berapa pasangan pertama yang sudah masuk indeks
    int64_t terurut;    // 1 kalau pasangannya urut menurut kunci
    int64_t cadangan[3];
} JrgPetaKepala;

static JrgPetaKepala *jrg_peta_kepala(const void *p) {
    return (JrgPetaKepala *)((char *)p - sizeof(JrgPetaKepala));
}

static const JrgStr *jrg_peta_kunci(const void *p, int64_t i, int64_t langkah) {
    return (const JrgStr *)((const char *)p + i * langkah);
}

// Urutan bita lalu panjang — sama dengan urutan teks di dua mesin lain.
static int jrg_peta_banding(const char *ap, int64_t an, const char *bp, int64_t bn) {
    int64_t n = an < bn ? an : bn;
    int c = n > 0 ? memcmp(ap, bp, (size_t)n) : 0;
    if (c != 0) return c;
    return an < bn ? -1 : (an > bn ? 1 : 0);
}

static int jrg_peta_banding_pasangan(const void *a, const void *b) {
    const JrgStr *x = a, *y = b;
    return jrg_peta_banding(x->ptr, x->len, y->ptr, y->len);
}

static uint64_t jrg_peta_hash(const char *p, int64_t n) {
    uint64_t h = 1469598103934665603ULL;
    for (int64_t i = 0; i < n; i++) { h ^= (unsigned char)p[i]; h *= 1099511628211ULL; }
    return h ^ (h >> 29);
}

// Penyangga baru untuk `n` pasangan, kepalanya sudah terisi.
void *jrg_dict_baru(int64_t n, int64_t langkah, int64_t jajar) {
    if (n < 0) n = 0;
    char *mentah = jrg_alloc((int64_t)sizeof(JrgPetaKepala) + n * langkah, jajar < 16 ? 16 : jajar);
    JrgPetaKepala *h = (JrgPetaKepala *)mentah;
    memset(h, 0, sizeof *h);
    h->kap = n;
    h->terurut = 1;
    return mentah + sizeof(JrgPetaKepala);
}

static void jrg_peta_catat(JrgPetaKepala *h, const void *p, int64_t i, int64_t langkah) {
    const JrgStr *k = jrg_peta_kunci(p, i, langkah);
    uint64_t s = jrg_peta_hash(k->ptr, k->len) & (uint64_t)(h->n_indeks - 1);
    while (h->indeks[s] != 0) s = (s + 1) & (uint64_t)(h->n_indeks - 1);
    h->indeks[s] = (int32_t)(i + 1);
}

// Indeks mencakup tepat `len` pasangan pertama; disusul sedikit demi sedikit kalau bisa.
static void jrg_peta_indeks(JrgPetaKepala *h, const void *p, int64_t len, int64_t langkah) {
    if (h->indeks == 0 || h->terindeks > len || h->n_indeks < 2 * len) {
        int64_t n = 16;
        int64_t sasaran = 2 * (h->kap > len ? h->kap : len);
        while (n < sasaran) n *= 2;
        h->indeks = jrg_alloc(n * (int64_t)sizeof(int32_t), 4);
        memset(h->indeks, 0, (size_t)n * sizeof(int32_t));
        h->n_indeks = n;
        h->terindeks = 0;
    }
    for (int64_t i = h->terindeks; i < len; i++) jrg_peta_catat(h, p, i, langkah);
    h->terindeks = len;
}

// Nomor pasangan berkunci ini, atau -1. Dict kecil dipindai; sisanya lewat indeks hash.
int64_t jrg_dict_find(const void *p, int64_t len, int64_t langkah, const char *kp, int64_t kl) {
    if (p == 0 || len <= 0) return -1;
    if (len <= 8) {
        for (int64_t i = 0; i < len; i++) {
            const JrgStr *k = jrg_peta_kunci(p, i, langkah);
            if (k->len == kl && (kl == 0 || memcmp(k->ptr, kp, (size_t)kl) == 0)) return i;
        }
        return -1;
    }
    JrgPetaKepala *h = jrg_peta_kepala(p);
    if (h->indeks == 0 || h->terindeks != len) jrg_peta_indeks(h, p, len, langkah);
    uint64_t topeng = (uint64_t)(h->n_indeks - 1);
    uint64_t s = jrg_peta_hash(kp, kl) & topeng;
    while (h->indeks[s] != 0) {
        int64_t i = h->indeks[s] - 1;
        const JrgStr *k = jrg_peta_kunci(p, i, langkah);
        if (k->len == kl && (kl == 0 || memcmp(k->ptr, kp, (size_t)kl) == 0)) return i;
        s = (s + 1) & topeng;
    }
    return -1;
}

// Pasangan berkunci ini; kalau belum ada, ditambahkan di ujung (kapasitas menggandakan diri).
void *jrg_dict_tempat(JrgDict *d, int64_t langkah, int64_t jajar, const char *kp, int64_t kl) {
    int64_t i = jrg_dict_find(d->ptr, d->len, langkah, kp, kl);
    if (i >= 0) return (char *)d->ptr + i * langkah;
    int64_t len = d->len;
    JrgPetaKepala *h = d->ptr ? jrg_peta_kepala(d->ptr) : 0;
    if (h == 0 || len >= h->kap) {
        void *baru = jrg_dict_baru(len < 4 ? 8 : len * 2, langkah, jajar);
        if (len > 0) memcpy(baru, d->ptr, (size_t)(len * langkah));
        jrg_peta_kepala(baru)->terurut = h ? h->terurut : 1;
        d->ptr = baru;
        h = jrg_peta_kepala(baru);
    }
    char *pas = (char *)d->ptr + len * langkah;
    if (h->terurut && len > 0) {
        const JrgStr *akhir = jrg_peta_kunci(d->ptr, len - 1, langkah);
        if (jrg_peta_banding(akhir->ptr, akhir->len, kp, kl) > 0) h->terurut = 0;
    }
    memset(pas, 0, (size_t)langkah);
    ((JrgStr *)pas)->ptr = kp;
    ((JrgStr *)pas)->len = kl;
    if (h->indeks != 0 && h->terindeks == len && 2 * (len + 1) <= h->n_indeks) {
        jrg_peta_catat(h, d->ptr, len, langkah);
        h->terindeks = len + 1;
    }
    d->len = len + 1;
    return pas;
}

// Sebelum pasangannya dibaca BERURUTAN (cetak, kunci, nilai, iterasi, salin, banding).
void jrg_dict_rapikan(void *p, int64_t len, int64_t langkah) {
    if (p == 0 || len <= 1) return;
    JrgPetaKepala *h = jrg_peta_kepala(p);
    if (h->terurut) return;
    qsort(p, (size_t)len, (size_t)langkah, jrg_peta_banding_pasangan);
    h->terurut = 1;
    h->terindeks = 0;
    h->indeks = 0;
}

// ── nilai bertag: pengurutan ──
void jrg_panic(const unsigned char *ptr, int64_t len);

typedef struct {
    int32_t tag;
    int32_t sela;
    union { int64_t i; double f; uint8_t b; int32_t r; JrgStr s; } isi;
} JrgAny;

static const char *const JRG_NAMA_TAG[] = { "nil", "bool", "int", "float", "rune", "string", "array", "dict", "unit" };

// Golongan yang saling terbanding: 1 angka, 2 bool, 3 rune, 4 teks; 0 tidak terbanding.
static int jrg_any_golongan(const JrgAny *a) {
    switch (a->tag) {
        case 2: return 1;
        case 3: return a->isi.f != a->isi.f ? 0 : 1;
        case 1: return 2;
        case 4: return 3;
        case 5: return 4;
        default: return 0;
    }
}

static int jrg_any_banding(const JrgAny *a, const JrgAny *b) {
    switch (a->tag) {
        case 1: return (a->isi.b > b->isi.b) - (a->isi.b < b->isi.b);
        case 4: return (a->isi.r > b->isi.r) - (a->isi.r < b->isi.r);
        case 5: return jrg_peta_banding(a->isi.s.ptr, a->isi.s.len, b->isi.s.ptr, b->isi.s.len);
        default: break;
    }
    if (a->tag == 2 && b->tag == 2) return (a->isi.i > b->isi.i) - (a->isi.i < b->isi.i);
    double x = a->tag == 2 ? (double)a->isi.i : a->isi.f;
    double y = b->tag == 2 ? (double)b->isi.i : b->isi.f;
    return (x > y) - (x < y);
}

// Gagal dengan `pola`, bita 1 dan 2 di dalamnya diganti nama tipe kiri dan kanan.
static void jrg_any_tak_sebanding(const char *pola, int64_t pl, const JrgAny *kiri, const JrgAny *kanan) {
    char pesan[512];
    int64_t n = 0;
    for (int64_t i = 0; i < pl && n < 480; i++) {
        if (pola[i] == 1 || pola[i] == 2) {
            int32_t t = (pola[i] == 1 ? kiri->tag : kanan->tag) & 255;
            const char *nama = t <= 8 ? JRG_NAMA_TAG[t] : "nilai";
            for (; *nama && n < 500; nama++) pesan[n++] = *nama;
        } else {
            pesan[n++] = pola[i];
        }
    }
    jrg_panic((const unsigned char *)pesan, n);
}

// Urut naik yang STABIL (2 dan 2.0 tetap di urutan aslinya); campuran golongan ditolak.
void jrg_sort_any(JrgAny *p, int64_t n, const char *pola, int64_t pl) {
    if (p == 0 || n <= 1) return;
    for (int64_t i = 0; i + 1 < n; i++) {
        int ga = jrg_any_golongan(&p[i]), gb = jrg_any_golongan(&p[i + 1]);
        if (ga == 0 || gb == 0 || ga != gb) jrg_any_tak_sebanding(pola, pl, &p[i + 1], &p[i]);
    }
    JrgAny *sementara = jrg_alloc(n * (int64_t)sizeof(JrgAny), 8);
    for (int64_t lebar = 1; lebar < n; lebar *= 2) {
        for (int64_t awal = 0; awal < n; awal += 2 * lebar) {
            int64_t tengah = awal + lebar < n ? awal + lebar : n;
            int64_t akhir = awal + 2 * lebar < n ? awal + 2 * lebar : n;
            int64_t a = awal, b = tengah, k = awal;
            while (a < tengah && b < akhir) sementara[k++] = jrg_any_banding(&p[b], &p[a]) < 0 ? p[b++] : p[a++];
            while (a < tengah) sementara[k++] = p[a++];
            while (b < akhir) sementara[k++] = p[b++];
        }
        memcpy(p, sementara, (size_t)n * sizeof(JrgAny));
    }
}

// `arr::sort` atas pecahan: NaN tidak punya urutan, jadi ditolak — tidak ditaruh di ujung.
void jrg_sort_f64_ketat(double *p, int64_t n, const char *pesan, int64_t pl) {
    for (int64_t i = 0; i < n; i++) {
        if (p[i] != p[i]) jrg_panic((const unsigned char *)pesan, pl);
    }
}

// Gagal dengan `pola`; bita 1 dan 2 di dalamnya diganti bilangan `a` dan `b`.
static void jrg_panik_pola(const char *pola, int64_t pl, int64_t a, int64_t b) {
    char pesan[512];
    int64_t n = 0;
    for (int64_t i = 0; i < pl && n < 440; i++) {
        if (pola[i] != 1 && pola[i] != 2) { pesan[n++] = pola[i]; continue; }
        int64_t v = pola[i] == 1 ? a : b;
        char angka[24];
        int k = 0;
        uint64_t u = v < 0 ? 0 - (uint64_t)v : (uint64_t)v;
        do { angka[k++] = (char)('0' + u % 10); u /= 10; } while (u > 0);
        if (v < 0) pesan[n++] = '-';
        while (k > 0) pesan[n++] = angka[--k];
    }
    jrg_panic((const unsigned char *)pesan, n);
}

// `arr::swap(a, i, j)`: salinan dengan dua elemen ditukar; indeks negatif dihitung dari belakang
// (aturan `a[i]`), di luar batas ditolak dengan indeks yang ditulis.
void *jrg_slice_tukar(const void *p, int64_t len, int64_t i0, int64_t j0, int64_t ukuran, int64_t jajar,
                      const char *luar, int64_t ll) {
    int64_t i = i0 < 0 ? i0 + len : i0, j = j0 < 0 ? j0 + len : j0;
    if (i < 0 || i >= len) jrg_panik_pola(luar, ll, i0, len);
    if (j < 0 || j >= len) jrg_panik_pola(luar, ll, j0, len);
    char *baru = jrg_alloc(len * ukuran, jajar);
    memcpy(baru, p, (size_t)(len * ukuran));
    if (i != j) {
        memcpy(baru + i * ukuran, (const char *)p + j * ukuran, (size_t)ukuran);
        memcpy(baru + j * ukuran, (const char *)p + i * ukuran, (size_t)ukuran);
    }
    return baru;
}

// ── pertumbuhan di tempat untuk tempat mana pun (field, elemen) ──
// Kapasitas dicatat di tabel kecil berkunci alamat penyangga. `terisi` adalah batas yang sudah
// ditulisi: menyambung di tempat hanya kalau slice ini berakhir TEPAT di sana, jadi dua
// pemegang penyangga yang sama tidak pernah saling menimpa.
typedef struct { void *ptr; int64_t kap; int64_t terisi; } JrgKapUmum;
#define JRG_KAP_UMUM 4096
static JrgKapUmum jrg_kap_umum[JRG_KAP_UMUM];

static JrgKapUmum *jrg_kap_cari(const void *p) {
    uintptr_t h = (uintptr_t)p;
    h = (h >> 4) ^ (h >> 16);
    return &jrg_kap_umum[h & (JRG_KAP_UMUM - 1)];
}

void jrg_slice_tumbuh_umum(JrgSlice *s, const void *isi, int64_t n, int64_t ukuran, int64_t jajar) {
    int64_t len = s->len, baru = len + n;
    JrgKapUmum *k = s->ptr ? jrg_kap_cari(s->ptr) : 0;
    if (k == 0 || k->ptr != s->ptr || k->terisi != len || baru > k->kap) {
        int64_t kap = baru * 2;
        if (kap < 8) kap = 8;
        void *p = jrg_alloc(kap * ukuran, jajar);
        if (len > 0) memcpy(p, s->ptr, (size_t)(len * ukuran));
        s->ptr = p;
        k = jrg_kap_cari(p);
        k->ptr = p;
        k->kap = kap;
    }
    if (n > 0) memcpy((char *)s->ptr + len * ukuran, isi, (size_t)(n * ukuran));
    k->terisi = baru;
    s->len = baru;
}

// ── nilai bertag: isi array/dict dikenali dari lukisan di bit atas tag ──
enum { JRG_L_ANY = 1, JRG_L_INT, JRG_L_FLOAT, JRG_L_BOOL, JRG_L_RUNE, JRG_L_STR, JRG_L_SLICE, JRG_L_DICT, JRG_L_BURAM = 15 };

static int64_t jrg_lukis_ukuran(uint32_t l) {
    switch (l & 15) {
        case JRG_L_ANY: return (int64_t)sizeof(JrgAny);
        case JRG_L_BOOL: return 1;
        case JRG_L_RUNE: return 4;
        case JRG_L_INT: case JRG_L_FLOAT: return 8;
        default: return 16;
    }
}

// Langkah pasangan dict `{kunci, nilai}`: kunci 16 bita, lalu nilainya, dibulatkan ke 8.
static int64_t jrg_lukis_langkah(uint32_t l) { return (16 + jrg_lukis_ukuran(l) + 7) / 8 * 8; }

// Lukisan NILAI di dalam sebuah kotak; 0 untuk nil dan unit.
static uint32_t jrg_any_lukis(const JrgAny *a) {
    uint32_t isi = (uint32_t)a->tag >> 8;
    switch (a->tag & 255) {
        case 1: return JRG_L_BOOL;
        case 2: return JRG_L_INT;
        case 3: return JRG_L_FLOAT;
        case 4: return JRG_L_RUNE;
        case 5: return JRG_L_STR;
        case 6: return JRG_L_SLICE | isi << 4;
        case 7: return JRG_L_DICT | isi << 4;
        default: return 0;
    }
}

// Salin nilai berlukisan `dari` di `src` menjadi berlukisan `ke` di `dst`; 0 kalau mustahil.
static int jrg_lukis_ubah(const void *src, uint32_t dari, void *dst, uint32_t ke) {
    if (dari == ke) { memcpy(dst, src, (size_t)jrg_lukis_ukuran(ke)); return 1; }
    uint32_t jd = dari & 15, jk = ke & 15;
    if (jk == JRG_L_ANY) {
        JrgAny *k = dst;
        memset(k, 0, sizeof *k);
        switch (jd) {
            case JRG_L_BOOL: k->tag = 1; break;
            case JRG_L_INT: k->tag = 2; break;
            case JRG_L_FLOAT: k->tag = 3; break;
            case JRG_L_RUNE: k->tag = 4; break;
            case JRG_L_STR: k->tag = 5; break;
            case JRG_L_SLICE: k->tag = (int32_t)(6u | (dari >> 4) << 8); break;
            case JRG_L_DICT: k->tag = (int32_t)(7u | (dari >> 4) << 8); break;
            default: return 0;
        }
        memcpy(&k->isi, src, (size_t)jrg_lukis_ukuran(dari));
        return 1;
    }
    if (jd == JRG_L_ANY) {
        const JrgAny *k = src;
        uint32_t l = jrg_any_lukis(k);
        return l != 0 && jrg_lukis_ubah(&k->isi, l, dst, ke);
    }
    if (jd != jk) return 0;
    uint32_t ed = dari >> 4, ek = ke >> 4;
    if ((ed & 15) == 0 || (ek & 15) == 0 || (ed & 15) == JRG_L_BURAM || (ek & 15) == JRG_L_BURAM) return 0;
    if (jd == JRG_L_SLICE) {
        const JrgSlice *s = src;
        int64_t ud = jrg_lukis_ukuran(ed), uk = jrg_lukis_ukuran(ek);
        char *buf = jrg_alloc(s->len > 0 ? s->len * uk : 1, 8);
        for (int64_t i = 0; i < s->len; i++)
            if (!jrg_lukis_ubah((const char *)s->ptr + i * ud, ed, buf + i * uk, ek)) return 0;
        JrgSlice h = { buf, s->len };
        memcpy(dst, &h, sizeof h);
        return 1;
    }
    if (jd == JRG_L_DICT) {
        const JrgDict *d = src;
        int64_t ld = jrg_lukis_langkah(ed), lk = jrg_lukis_langkah(ek);
        char *buf = jrg_dict_baru(d->len, lk, 8);
        for (int64_t i = 0; i < d->len; i++) {
            const char *ps = (const char *)d->ptr + i * ld;
            char *pd = buf + i * lk;
            memset(pd, 0, (size_t)lk);
            memcpy(pd, ps, sizeof(JrgStr));
            if (!jrg_lukis_ubah(ps + 16, ed, pd + 16, ek)) return 0;
        }
        if (d->ptr != 0 && d->len > 0) jrg_peta_kepala(buf)->terurut = jrg_peta_kepala(d->ptr)->terurut;
        JrgDict h = { buf, d->len };
        memcpy(dst, &h, sizeof h);
        return 1;
    }
    return 0;
}

// Salin-dalam DI TEMPAT nilai berlukisan `l`: penyangga array/dict di dalamnya diganti salinannya,
// supaya salinan sebuah kotak tidak berbagi isi dengan aslinya.
static void jrg_lukis_salin(void *p, uint32_t l) {
    uint32_t j = l & 15, e = l >> 4;
    if (j == JRG_L_ANY) {
        JrgAny *k = p;
        uint32_t dalam = jrg_any_lukis(k);
        if ((dalam & 15) == JRG_L_SLICE || (dalam & 15) == JRG_L_DICT) jrg_lukis_salin(&k->isi, dalam);
        return;
    }
    if ((e & 15) == 0 || (e & 15) == JRG_L_BURAM) return;
    int bersarang = (e & 15) == JRG_L_ANY || (e & 15) == JRG_L_SLICE || (e & 15) == JRG_L_DICT;
    if (j == JRG_L_SLICE) {
        JrgSlice *s = p;
        int64_t uk = jrg_lukis_ukuran(e);
        char *buf = jrg_alloc(s->len > 0 ? s->len * uk : 1, 8);
        if (s->len > 0) memcpy(buf, s->ptr, (size_t)(s->len * uk));
        if (bersarang) for (int64_t i = 0; i < s->len; i++) jrg_lukis_salin(buf + i * uk, e);
        s->ptr = (void *)buf;
    } else if (j == JRG_L_DICT) {
        JrgDict *d = p;
        if (d->ptr == 0 || d->len == 0) return;
        int64_t lk = jrg_lukis_langkah(e);
        char *buf = jrg_dict_baru(d->len, lk, 8);
        memcpy(buf, d->ptr, (size_t)(d->len * lk));
        jrg_peta_kepala(buf)->terurut = jrg_peta_kepala(d->ptr)->terurut;
        if (bersarang) for (int64_t i = 0; i < d->len; i++) jrg_lukis_salin(buf + i * lk + 16, e);
        d->ptr = (void *)buf;
    }
}

void jrg_any_salin(JrgAny *a) { jrg_lukis_salin(a, JRG_L_ANY); }

static int64_t jrg_lukis_nama(uint32_t l, char *buf, int64_t n, int64_t maks) {
    const char *t = "?";
    uint32_t dalam = 0;
    switch (l & 15) {
        case JRG_L_ANY: t = "campur"; break;
        case JRG_L_INT: t = "int"; break;
        case JRG_L_FLOAT: t = "float"; break;
        case JRG_L_BOOL: t = "bool"; break;
        case JRG_L_RUNE: t = "rune"; break;
        case JRG_L_STR: t = "string"; break;
        case JRG_L_SLICE: t = "array"; dalam = l >> 4; break;
        case JRG_L_DICT: t = "dict"; dalam = l >> 4; break;
    }
    for (; *t && n < maks; t++) buf[n++] = *t;
    if ((dalam & 15) != 0 && (dalam & 15) != JRG_L_BURAM && n < maks) {
        buf[n++] = '<';
        n = jrg_lukis_nama(dalam, buf, n, maks);
        if (n < maks) buf[n++] = '>';
    }
    return n;
}

// Isi kotak `a` dijadikan berkata-tag `kata`; yang mustahil gagal dengan `pola` (bita 1 = tipe aslinya).
void jrg_any_ubah(const JrgAny *a, int32_t kata, JrgAny *hasil, const char *pola, int64_t pl) {
    uint32_t ke = ((kata & 255) == 6 ? JRG_L_SLICE : JRG_L_DICT) | ((uint32_t)kata >> 8) << 4;
    uint32_t dari = jrg_any_lukis(a);
    memset(hasil, 0, sizeof *hasil);
    hasil->tag = kata;
    if (dari != 0 && jrg_lukis_ubah(&a->isi, dari, &hasil->isi, ke)) return;
    char pesan[640];
    int64_t n = 0;
    for (int64_t i = 0; i < pl && n < 600; i++) {
        if (pola[i] != 1) { pesan[n++] = pola[i]; continue; }
        if (dari != 0) { n = jrg_lukis_nama(dari, pesan, n, 600); continue; }
        int32_t t = a->tag & 255;
        const char *nama = t <= 8 ? JRG_NAMA_TAG[t] : "nilai";
        for (; *nama && n < 600; nama++) pesan[n++] = *nama;
    }
    jrg_panic((const unsigned char *)pesan, n);
}

// Urut naik STABIL untuk elemen berukuran `uk`, dengan pembanding milik pemanggil: gabung dari
// bawah, yang kanan diambil hanya kalau tegas lebih kecil (sama dengan `urut_stabil` dua mesin lain).
void jrg_sort_umum(char *p, int64_t n, int64_t uk, int32_t (*banding)(const void *, const void *)) {
    if (n < 2) return;
    char *sisi = jrg_alloc(n * uk, 8);
    char *kini = p;
    for (int64_t lebar = 1; lebar < n; lebar *= 2) {
        for (int64_t i = 0; i < n; i += 2 * lebar) {
            int64_t tengah = i + lebar < n ? i + lebar : n, akhir = i + 2 * lebar < n ? i + 2 * lebar : n;
            int64_t a = i, b = tengah, k = i;
            while (a < tengah && b < akhir) {
                if (banding(kini + b * uk, kini + a * uk) < 0) memcpy(sisi + k++ * uk, kini + b++ * uk, (size_t)uk);
                else memcpy(sisi + k++ * uk, kini + a++ * uk, (size_t)uk);
            }
            while (a < tengah) memcpy(sisi + k++ * uk, kini + a++ * uk, (size_t)uk);
            while (b < akhir) memcpy(sisi + k++ * uk, kini + b++ * uk, (size_t)uk);
        }
        char *t = kini; kini = sisi; sisi = t;
    }
    if (kini != p) memcpy(p, kini, (size_t)(n * uk));
}

// ── jembatan ke builtin milik jrgc (pustaka dinamis `libjrgjembatan`, ditautkan kalau dipakai) ──
typedef struct {
    void *(*alloc)(int64_t, int64_t);
    void *(*dict_tempat)(JrgDict *, int64_t, int64_t, const char *, int64_t);
    void (*any_ubah)(const JrgAny *, int32_t, JrgAny *, const char *, int64_t);
} JrgKait;

__attribute__((weak)) int32_t jrg_jembatan_panggil(const char *, int64_t, const JrgAny *, int64_t, JrgAny *, const JrgKait *);

// Hasil: 0 nilai biasa, 1 `Some`/`Ok`, 2 `None`, 3 `Err`; kegagalan builtinnya menghentikan program.
int32_t jrg_jembatan(const char *nama, int64_t nl, const JrgAny *args, int64_t n, JrgAny *out) {
    static const JrgKait kait = { jrg_alloc, jrg_dict_tempat, jrg_any_ubah };
    // Pustakanya tidak ditautkan: program ini tidak pernah memanggil builtin lewat jembatan,
    // jadi nilai titipan tidak mungkin ada — yang sampai ke sini tag yang tak dikenal.
    if (jrg_jembatan_panggil == 0) {
        static const char pesan[] = "tag nilai tidak dikenal — ini bug di jrgc, bukan di programmu";
        jrg_panic((const unsigned char *)pesan, (int64_t)(sizeof pesan - 1));
    }
    int32_t r = jrg_jembatan_panggil(nama, nl, args, n, out, &kait);
    if (r < 0) jrg_panic((const unsigned char *)out->isi.s.ptr, out->isi.s.len);
    return r;
}

// ── pintu sistem operasi untuk lapis ketiga (`lapis::os`): berkas, lingkungan, jam ──
#if defined(__unix__) || defined(__APPLE__)
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

/* `num_cpus()`: inti yang terlihat proses ini, paling sedikit 1. */
int64_t jrg_num_cpus(void) {
    long n = sysconf(_SC_NPROCESSORS_ONLN);
    return n > 0 ? (int64_t)n : 1;
}

// Argumen proses, ditangkap sebelum `main`: glibc dan dyld menyerahkannya ke konstruktor.
static int jrg_argc;
static char **jrg_argv;
__attribute__((constructor)) static void jrg_tangkap_argumen(int argc, char **argv) {
    jrg_argc = argc;
    jrg_argv = argv;
}

void jrg_ko_tidur(int64_t mikro);

static char *jrg_os_cstr(const char *p, int64_t n) {
    char *s = jrg_alloc(n + 1, 1);
    if (n > 0) memcpy(s, p, (size_t)n);
    s[n] = 0;
    return s;
}

static void jrg_os_teks(JrgStr *out, const char *p, int64_t n) {
    out->ptr = n > 0 ? p : (const char *)1;
    out->len = n;
}

static int64_t jrg_os_tulis(const char *jalur, const char *isi, int64_t n, int bendera) {
    int fd = open(jalur, O_WRONLY | O_CREAT | bendera, 0666);
    if (fd < 0) return -errno;
    int64_t tulis = 0;
    while (tulis < n) {
        ssize_t k = write(fd, isi + tulis, (size_t)(n - tulis));
        if (k < 0) { int e = errno; close(fd); return -e; }
        tulis += k;
    }
    close(fd);
    return 0;
}

// Hapus berkas, atau direktori beserta isinya. Tautan simbolik dihapus, tidak diikuti.
static int64_t jrg_os_hapus(const char *jalur) {
    struct stat st;
    if (lstat(jalur, &st) != 0) return -errno;
    if (!S_ISDIR(st.st_mode)) return unlink(jalur) == 0 ? 0 : -errno;
    DIR *d = opendir(jalur);
    if (d == 0) return -errno;
    size_t pj = strlen(jalur);
    struct dirent *e;
    int64_t gagal = 0;
    while (gagal == 0 && (e = readdir(d)) != 0) {
        if (strcmp(e->d_name, ".") == 0 || strcmp(e->d_name, "..") == 0) continue;
        size_t pn = strlen(e->d_name);
        char *anak = malloc(pj + pn + 2);
        if (anak == 0) { gagal = -ENOMEM; break; }
        memcpy(anak, jalur, pj);
        anak[pj] = '/';
        memcpy(anak + pj + 1, e->d_name, pn + 1);
        gagal = jrg_os_hapus(anak);
        free(anak);
    }
    closedir(d);
    if (gagal != 0) return gagal;
    return rmdir(jalur) == 0 ? 0 : -errno;
}

static int jrg_os_banding_nama(const void *a, const void *b) {
    const JrgStr *x = a, *y = b;
    return jrg_peta_banding(x->ptr, x->len, y->ptr, y->len);
}

// Hasil: >= 0 berhasil (artinya per operasi), negatif = -errno. Teks hasil di `out`.
int64_t jrg_os(int32_t op, const char *ap, int64_t an, const char *bp, int64_t bn, int64_t i, JrgStr *out) {
    jrg_os_teks(out, 0, 0);
    char *a = jrg_os_cstr(ap, an);
    struct stat st;
    switch (op) {
        case 1: {   // baca seluruh berkas
            int fd = open(a, O_RDONLY);
            if (fd < 0) return -errno;
            if (fstat(fd, &st) != 0) { int e = errno; close(fd); return -e; }
            if (S_ISDIR(st.st_mode)) { close(fd); return -EISDIR; }
            int64_t kap = st.st_size > 0 ? st.st_size + 1 : 4096, n = 0;
            char *buf = jrg_alloc(kap, 1);
            for (;;) {
                if (n == kap) {
                    char *baru = jrg_alloc(kap * 2, 1);
                    memcpy(baru, buf, (size_t)n);
                    buf = baru;
                    kap *= 2;
                }
                ssize_t k = read(fd, buf + n, (size_t)(kap - n));
                if (k < 0) { int e = errno; close(fd); return -e; }
                if (k == 0) break;
                n += k;
            }
            close(fd);
            jrg_os_teks(out, buf, n);
            return 0;
        }
        case 2: return jrg_os_tulis(a, bp, bn, O_TRUNC);
        case 3: return jrg_os_tulis(a, bp, bn, O_APPEND);
        case 4: return stat(a, &st) == 0 ? 1 : 0;
        case 5: return jrg_os_hapus(a);
        case 6: return stat(a, &st) == 0 ? (int64_t)st.st_size : -errno;
        case 7: return stat(a, &st) == 0 && S_ISDIR(st.st_mode) ? 1 : 0;
        case 8: return stat(a, &st) == 0 && S_ISREG(st.st_mode) ? 1 : 0;
        case 9: {   // mkdir beserta induknya
            for (int64_t k = 1; k <= an; k++) {
                if (k < an && a[k] != '/') continue;
                char simpan = a[k];
                a[k] = 0;
                if (mkdir(a, 0777) != 0 && errno != EEXIST) return -errno;
                a[k] = simpan;
            }
            return stat(a, &st) == 0 && S_ISDIR(st.st_mode) ? 0 : -ENOTDIR;
        }
        case 10: return rename(a, jrg_os_cstr(bp, bn)) == 0 ? 0 : -errno;
        case 11: {  // isi folder, terurut, dipisah baris baru
            DIR *d = opendir(a);
            if (d == 0) return -errno;
            int64_t kap = 16, n = 0, total = 0;
            JrgStr *nama = jrg_alloc(kap * (int64_t)sizeof(JrgStr), 8);
            struct dirent *e;
            while ((e = readdir(d)) != 0) {
                if (strcmp(e->d_name, ".") == 0 || strcmp(e->d_name, "..") == 0) continue;
                if (n == kap) {
                    JrgStr *baru = jrg_alloc(kap * 2 * (int64_t)sizeof(JrgStr), 8);
                    memcpy(baru, nama, (size_t)n * sizeof(JrgStr));
                    nama = baru;
                    kap *= 2;
                }
                int64_t l = (int64_t)strlen(e->d_name);
                nama[n].ptr = jrg_os_cstr(e->d_name, l);
                nama[n].len = l;
                total += l + 1;
                n++;
            }
            closedir(d);
            qsort(nama, (size_t)n, sizeof(JrgStr), jrg_os_banding_nama);
            char *buf = jrg_alloc(total + 1, 1);
            int64_t p = 0;
            for (int64_t k = 0; k < n; k++) {
                if (k > 0) buf[p++] = '\n';
                memcpy(buf + p, nama[k].ptr, (size_t)nama[k].len);
                p += nama[k].len;
            }
            jrg_os_teks(out, buf, p);
            return n;
        }
        case 12: {  // variabel lingkungan: 1 kalau diset
            const char *v = getenv(a);
            if (v == 0) return 0;
            jrg_os_teks(out, v, (int64_t)strlen(v));
            return 1;
        }
        case 13: return setenv(a, jrg_os_cstr(bp, bn), 1) == 0 ? 0 : -errno;
        case 14: {  // mikrodetik sejak epoch
            struct timespec ts;
            clock_gettime(CLOCK_REALTIME, &ts);
            return (int64_t)ts.tv_sec * 1000000 + ts.tv_nsec / 1000;
        }
        case 15: jrg_ko_tidur(i); return 0;   // tidur `i` mikrodetik; goroutine lain tetap jalan
        case 16: {  // teks galat untuk errno `i`
            const char *s = strerror((int)i);
            jrg_os_teks(out, s, (int64_t)strlen(s));
            return 0;
        }
        case 17: {  // folder kerja
            char *buf = jrg_alloc(4096, 1);
            if (getcwd(buf, 4096) == 0) return -errno;
            jrg_os_teks(out, buf, (int64_t)strlen(buf));
            return 0;
        }
        case 18: return lstat(a, &st) == 0 && S_ISLNK(st.st_mode) ? 1 : 0;
        case 19: {  // berkas biasa yang BISA dibuka untuk ditulis
            if (stat(a, &st) != 0 || !S_ISREG(st.st_mode)) return 0;
            int fd = open(a, O_WRONLY);
            if (fd < 0) return 0;
            close(fd);
            return 1;
        }
        case 20: return chmod(a, (mode_t)i) == 0 ? 0 : -errno;
        case 21: return jrg_argc;
        case 22: {  // argumen ke-`i`
            if (i < 0 || i >= jrg_argc) return -EINVAL;
            jrg_os_teks(out, jrg_argv[i], (int64_t)strlen(jrg_argv[i]));
            return 0;
        }
        case 23: return unlink(a) == 0 ? 0 : -errno;   // satu berkas, bukan direktori
        case 24: return rmdir(a) == 0 ? 0 : -errno;    // direktori KOSONG saja
        default: return -ENOSYS;
    }
}
#endif
