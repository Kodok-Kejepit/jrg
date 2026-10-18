
// korutin.c — konkurensi biner `--emit-llvm`: korutin SATU utas dan channel di atasnya.
// Tanpa kunci di mana pun; kebuntuan dilaporkan.
#if defined(__unix__) || defined(__APPLE__)
#include <sys/mman.h>
#if defined(__ANDROID__)
// bionic tidak punya swapcontext/makecontext: stack ditukar sendiri (cuma register callee-saved).
typedef struct { void *sp; } JrgKonteks;
void jrg_ko_tukar(void **simpan, void *muat);
#if defined(__aarch64__)
__asm__(".text\n.globl jrg_ko_tukar\n.hidden jrg_ko_tukar\n.type jrg_ko_tukar,%function\n.p2align 2\n"
        "jrg_ko_tukar:\n"
        "sub sp, sp, #176\n"
        "stp x19, x20, [sp, #0]\nstp x21, x22, [sp, #16]\nstp x23, x24, [sp, #32]\n"
        "stp x25, x26, [sp, #48]\nstp x27, x28, [sp, #64]\nstp x29, x30, [sp, #80]\n"
        "stp d8, d9, [sp, #96]\nstp d10, d11, [sp, #112]\nstp d12, d13, [sp, #128]\nstp d14, d15, [sp, #144]\n"
        "mov x2, sp\nstr x2, [x0]\nmov sp, x1\n"
        "ldp x19, x20, [sp, #0]\nldp x21, x22, [sp, #16]\nldp x23, x24, [sp, #32]\n"
        "ldp x25, x26, [sp, #48]\nldp x27, x28, [sp, #64]\nldp x29, x30, [sp, #80]\n"
        "ldp d8, d9, [sp, #96]\nldp d10, d11, [sp, #112]\nldp d12, d13, [sp, #128]\nldp d14, d15, [sp, #144]\n"
        "add sp, sp, #176\nret\n.size jrg_ko_tukar, .-jrg_ko_tukar\n");
#define JRG_KO_BINGKAI 176
#define JRG_KO_KEMBALI (88 / 8)
#elif defined(__x86_64__)
__asm__(".text\n.globl jrg_ko_tukar\n.hidden jrg_ko_tukar\n.type jrg_ko_tukar,@function\n.p2align 4\n"
        "jrg_ko_tukar:\n"
        "pushq %rbp\npushq %rbx\npushq %r12\npushq %r13\npushq %r14\npushq %r15\n"
        "movq %rsp, (%rdi)\nmovq %rsi, %rsp\n"
        "popq %r15\npopq %r14\npopq %r13\npopq %r12\npopq %rbx\npopq %rbp\nret\n"
        ".size jrg_ko_tukar, .-jrg_ko_tukar\n");
#define JRG_KO_BINGKAI 64
#define JRG_KO_KEMBALI (48 / 8)
#else
#error "korutin Android: arsitektur ini belum punya penukar stack"
#endif
#else
#include <ucontext.h>
typedef ucontext_t JrgKonteks;
#endif

void jrg_flush(int32_t);

#define JRG_KO_STACK ((size_t)1 << 20)

// Sama dengan `DaftarTunda` di runtime: tiap korutin punya tumpukan `defer` sendiri.
typedef struct { void *ptr; int64_t len, kap; } JrgTunda;
extern JrgTunda jrg_tunda_kini;

typedef struct JrgKo {
    JrgKonteks ctx;
    void (*fn)(void *);
    void *arg;
    char *stack;
    int64_t bangun;          // tenggat tidur (mikrodetik sejak epoch), 0 = tidak tidur
    JrgTunda tunda;          // tumpukan `defer` miliknya selagi ia tidak jalan
    int masa;                // badan sebuah Future/Task: gagalnya milik yang menunggu hasilnya
    struct JrgKo *next;
} JrgKo;

static JrgKo jrg_ko_utama;
static JrgKo *jrg_ko_kini = &jrg_ko_utama;
static JrgKo *jrg_ko_siap, *jrg_ko_siap_ekor;   // antrean yang bisa jalan
static JrgKo *jrg_ko_tidur_daftar;              // yang sedang tidur
static JrgKo *jrg_ko_mayat;                     // selesai, stack-nya belum dilepas
static int64_t jrg_ko_hidup;                    // goroutine yang belum selesai
static int jrg_ko_utama_menunggu;               // `main` sudah habis dan menunggu sisanya

static int64_t jrg_ko_jam(void) {
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    return (int64_t)ts.tv_sec * 1000000 + ts.tv_nsec / 1000;
}

static void jrg_ko_antre(JrgKo *k) {
    k->next = 0;
    if (jrg_ko_siap_ekor) jrg_ko_siap_ekor->next = k; else jrg_ko_siap = k;
    jrg_ko_siap_ekor = k;
}

static void jrg_ko_pindah(JrgKo *ke) {
    JrgKo *dari = jrg_ko_kini;
    if (ke == dari) return;
    dari->tunda = jrg_tunda_kini;
    jrg_tunda_kini = ke->tunda;
    jrg_ko_kini = ke;
#if defined(__ANDROID__)
    jrg_ko_tukar(&dari->ctx.sp, ke->ctx.sp);
#else
    swapcontext(&dari->ctx, &ke->ctx);
#endif
}

static void jrg_ko_buntu(void) {
    static const char pesan[] =
        "kebuntuan: semua goroutine sedang menunggu channel dan tidak ada yang bisa melanjutkan\n"
        "  catatan: di interpreter dan `--vm` program ini menggantung selamanya; backend "
        "`--emit-llvm` menjalankan goroutine sebagai korutin satu utas, jadi ia bisa melihatnya";
    jrg_panic((const unsigned char *)pesan, (int64_t)(sizeof pesan - 1));
}

// Yang memanggil sudah TIDAK ada di antrean siap (diparkir, tidur, atau selesai).
static void jrg_ko_jadwal(void) {
    for (;;) {
        if (jrg_ko_siap) {
            JrgKo *k = jrg_ko_siap;
            jrg_ko_siap = k->next;
            if (jrg_ko_siap == 0) jrg_ko_siap_ekor = 0;
            jrg_ko_pindah(k);
            return;
        }
        if (jrg_ko_tidur_daftar == 0) jrg_ko_buntu();
        int64_t awal = jrg_ko_tidur_daftar->bangun;
        for (JrgKo *k = jrg_ko_tidur_daftar; k; k = k->next) if (k->bangun < awal) awal = k->bangun;
        int64_t sisa = awal - jrg_ko_jam();
        if (sisa > 0) {
            struct timespec ts = { (time_t)(sisa / 1000000), (long)(sisa % 1000000) * 1000 };
            while (nanosleep(&ts, &ts) != 0 && errno == EINTR) {}
        }
        int64_t kini = jrg_ko_jam();
        JrgKo **p = &jrg_ko_tidur_daftar;
        while (*p) {
            JrgKo *k = *p;
            if (k->bangun <= kini) { *p = k->next; k->bangun = 0; jrg_ko_antre(k); } else p = &k->next;
        }
    }
}

// Akhir hidup sebuah goroutine: bangunkan `main` kalau ia yang terakhir, lalu serahkan giliran.
static void jrg_kunci_tinggal(JrgKo *ko);

static void jrg_ko_tamat(JrgKo *k) {
    jrg_kunci_tinggal(k);
    jrg_ko_hidup--;
    if (jrg_ko_hidup == 0 && jrg_ko_utama_menunggu) { jrg_ko_utama_menunggu = 0; jrg_ko_antre(&jrg_ko_utama); }
    jrg_ko_mayat = k;
    jrg_ko_jadwal();
}

static void jrg_ko_masuk(void) {
    JrgKo *k = jrg_ko_kini;
    k->fn(k->arg);
    jrg_ko_tamat(k);
}

// Goroutine `spawn` yang gagal berhenti sendirian, seperti di dua mesin lain; `main` dan
// badan Future/Task tidak (jawab 0). `tanya` bukan nol: cuma bertanya, tidak menghentikan.
int jrg_ko_gugur(int tanya) {
    JrgKo *k = jrg_ko_kini;
    if (k == &jrg_ko_utama || k->masa) return 0;
    if (!tanya) jrg_ko_tamat(k);
    return 1;
}

static int jrg_ko_baru_masa;

// `fn(arg)` jadi goroutine baru; ia mulai jalan saat yang sekarang menunggu atau selesai.
void jrg_ko_spawn(void (*fn)(void *), void *arg) {
    if (jrg_ko_mayat && jrg_ko_mayat != jrg_ko_kini) {
        munmap(jrg_ko_mayat->stack, JRG_KO_STACK);
        jrg_ko_mayat = 0;
    }
    JrgKo *k = jrg_alloc((int64_t)sizeof(JrgKo), 16);
    memset(k, 0, sizeof *k);
    k->fn = fn;
    k->arg = arg;
    k->masa = jrg_ko_baru_masa;
    jrg_ko_baru_masa = 0;
    k->stack = mmap(0, JRG_KO_STACK, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (k->stack == MAP_FAILED) {
        static const char pesan[] = "spawn: tidak bisa memesan stack untuk goroutine baru (memori habis)";
        jrg_panic((const unsigned char *)pesan, (int64_t)(sizeof pesan - 1));
    }
    mprotect(k->stack, 4096, PROT_NONE);
#if defined(__ANDROID__)
    // Bingkai awal: register nol, alamat kembali `jrg_ko_masuk`, puncaknya rata 16 byte.
    uintptr_t puncak = ((uintptr_t)k->stack + JRG_KO_STACK) & ~(uintptr_t)15;
    void **bingkai = (void **)(puncak - JRG_KO_BINGKAI);
    memset(bingkai, 0, JRG_KO_BINGKAI);
    bingkai[JRG_KO_KEMBALI] = (void *)jrg_ko_masuk;
    k->ctx.sp = bingkai;
#else
    getcontext(&k->ctx);
    k->ctx.uc_stack.ss_sp = k->stack;
    k->ctx.uc_stack.ss_size = JRG_KO_STACK;
    k->ctx.uc_link = 0;
    makecontext(&k->ctx, jrg_ko_masuk, 0);
#endif
    jrg_ko_hidup++;
    jrg_ko_antre(k);
}

// Beri giliran ke yang lain, lalu lanjut.
void jrg_ko_serah(void) {
    if (jrg_ko_siap == 0) return;
    jrg_ko_antre(jrg_ko_kini);
    jrg_ko_jadwal();
}

// Tidur `mikro` mikrodetik; goroutine lain jalan selama itu.
void jrg_ko_tidur(int64_t mikro) {
    if (mikro <= 0) return;
    if (jrg_ko_hidup == 0 && jrg_ko_kini == &jrg_ko_utama) {
        struct timespec ts = { (time_t)(mikro / 1000000), (long)(mikro % 1000000) * 1000 };
        while (nanosleep(&ts, &ts) != 0 && errno == EINTR) {}
        return;
    }
    JrgKo *k = jrg_ko_kini;
    k->bangun = jrg_ko_jam() + mikro;
    k->next = jrg_ko_tidur_daftar;
    jrg_ko_tidur_daftar = k;
    jrg_ko_jadwal();
}

// Di ujung `main`: goroutine yang belum selesai ditunggu, seperti di dua mesin lain.
void jrg_ko_tunggu_semua(void) {
    if (jrg_ko_hidup > 0) {
        jrg_ko_utama_menunggu = 1;
        jrg_ko_jadwal();
    }
    jrg_flush(0);
}

// Wilayah stack goroutine yang sedang jalan, untuk penjaga stack.
int jrg_ko_di_stack(uintptr_t alamat) {
    JrgKo *k = jrg_ko_kini;
    return k->stack != 0 && alamat >= (uintptr_t)k->stack && alamat < (uintptr_t)k->stack + JRG_KO_STACK;
}

// ── channel ──
typedef struct JrgTunggu { JrgKo *k; void *data; int beres; struct JrgTunggu *next; } JrgTunggu;

typedef struct {
    int64_t kap, esize, n, kepala;
    char *buf;
    int tutup;
    int sekali;      // channel milik sebuah Future (1) atau Task (2): hasilnya diserahkan satu kali
    int diambil;
    char *gagal;     // kalimat galat badan tugasnya; diteruskan ke yang mengambil hasilnya
    int64_t gagal_n;
    JrgTunggu *penerima, *penerima_ekor, *pengirim, *pengirim_ekor;
} JrgChan;

static void jrg_ch_panik(const char *s) { jrg_panic((const unsigned char *)s, (int64_t)strlen(s)); }

void *jrg_chan_new(int64_t kap, int64_t esize) {
    if (kap < 0) kap = 0;
    JrgChan *c = jrg_alloc((int64_t)sizeof(JrgChan), 8);
    memset(c, 0, sizeof *c);
    c->kap = kap;
    c->esize = esize;
    c->buf = jrg_alloc((kap > 0 ? kap : 1) * (esize > 0 ? esize : 1), 8);
    return c;
}

static void jrg_ch_antre(JrgTunggu **kepala, JrgTunggu **ekor, JrgTunggu *t) {
    t->next = 0;
    if (*ekor) (*ekor)->next = t; else *kepala = t;
    *ekor = t;
}

static JrgTunggu *jrg_ch_ambil(JrgTunggu **kepala, JrgTunggu **ekor) {
    JrgTunggu *t = *kepala;
    if (t) { *kepala = t->next; if (*kepala == 0) *ekor = 0; }
    return t;
}

static void jrg_ch_masuk_buf(JrgChan *c, const void *v) {
    memcpy(c->buf + ((c->kepala + c->n) % c->kap) * c->esize, v, (size_t)c->esize);
    c->n++;
}

// 1 terkirim tanpa menunggu, 0 harus menunggu, 2 channelnya sudah ditutup.
static int32_t jrg_ch_coba_kirim(JrgChan *c, const void *v) {
    if (c->tutup) return 2;
    JrgTunggu *t = jrg_ch_ambil(&c->penerima, &c->penerima_ekor);
    if (t) {
        memcpy(t->data, v, (size_t)c->esize);
        t->beres = 1;
        jrg_ko_antre(t->k);
        return 1;
    }
    if (c->n < c->kap) { jrg_ch_masuk_buf(c, v); return 1; }
    return 0;
}

// Bentuk yang dipanggil program: jawaban "belum" memberi giliran dulu, lalu dicoba sekali lagi.
int32_t jrg_chan_try_send(JrgChan *c, const void *v, int64_t esize) {
    (void)esize;
    int32_t r = jrg_ch_coba_kirim(c, v);
    if (r != 0) return r;
    jrg_ko_serah();
    return jrg_ch_coba_kirim(c, v);
}

void jrg_chan_send(JrgChan *c, const void *v, int64_t esize) {
    if (c->tutup) jrg_ch_panik("channel sudah ditutup: tidak bisa kirim lagi");
    JrgTunggu *t = jrg_ch_ambil(&c->penerima, &c->penerima_ekor);
    if (t) {
        // Penerimanya sudah menunggu: nilainya diserahkan dan IA yang jalan lebih dulu.
        memcpy(t->data, v, (size_t)c->esize);
        t->beres = 1;
        jrg_ko_antre(jrg_ko_kini);
        JrgKo *k = t->k;
        jrg_ko_pindah(k);
        return;
    }
    if (c->n < c->kap) { jrg_ch_masuk_buf(c, v); return; }
    (void)esize;
    JrgTunggu saya = { jrg_ko_kini, (void *)v, 0, 0 };
    jrg_ch_antre(&c->pengirim, &c->pengirim_ekor, &saya);
    jrg_ko_jadwal();
    if (!saya.beres) jrg_ch_panik("channel sudah ditutup: tidak bisa kirim lagi");
}

// 1 ada nilai yang langsung diambil, 0 harus menunggu, 2 tertutup dan kosong.
static int32_t jrg_ch_coba_terima(JrgChan *c, void *out) {
    if (c->n > 0) {
        memcpy(out, c->buf + c->kepala * c->esize, (size_t)c->esize);
        c->kepala = (c->kepala + 1) % c->kap;
        c->n--;
        JrgTunggu *t = jrg_ch_ambil(&c->pengirim, &c->pengirim_ekor);
        if (t) { jrg_ch_masuk_buf(c, t->data); t->beres = 1; jrg_ko_antre(t->k); }
        return 1;
    }
    JrgTunggu *t = jrg_ch_ambil(&c->pengirim, &c->pengirim_ekor);
    if (t) {
        memcpy(out, t->data, (size_t)c->esize);
        t->beres = 1;
        jrg_ko_antre(t->k);
        return 1;
    }
    return c->tutup ? 2 : 0;
}

int32_t jrg_chan_try_recv(JrgChan *c, void *out, int64_t esize) {
    (void)esize;
    int32_t r = jrg_ch_coba_terima(c, out);
    if (r != 0) return r;
    jrg_ko_serah();
    return jrg_ch_coba_terima(c, out);
}

void jrg_chan_recv(JrgChan *c, void *out, int64_t esize) {
    (void)esize;
    int32_t r = jrg_ch_coba_terima(c, out);
    if (r == 1) return;
    if (r == 2) jrg_ch_panik("channel sudah ditutup dan kosong: tidak akan ada nilai lagi");
    JrgTunggu saya = { jrg_ko_kini, out, 0, 0 };
    jrg_ch_antre(&c->penerima, &c->penerima_ekor, &saya);
    jrg_ko_jadwal();
    if (!saya.beres) jrg_ch_panik("channel sudah ditutup dan kosong: tidak akan ada nilai lagi");
}

void jrg_chan_close(JrgChan *c) {
    c->tutup = 1;
    JrgTunggu *t;
    while ((t = jrg_ch_ambil(&c->penerima, &c->penerima_ekor)) != 0) jrg_ko_antre(t->k);
    while ((t = jrg_ch_ambil(&c->pengirim, &c->pengirim_ekor)) != 0) jrg_ko_antre(t->k);
}

// 0 panjang, 1 kapasitas, 2 sudah ditutup, 3 ada nilai yang bisa diambil.
int64_t jrg_chan_info(const JrgChan *c, int32_t apa) {
    // Pertanyaan yang jawabannya "belum" memberi giliran dulu: loop yang cuma BERTANYA
    // (`while !c.is_closed() { .. }`) tidak pernah menunggu, jadi tanpa ini pengirimnya tak jalan.
    if (apa == 1) return c->kap;
    int64_t r = apa == 0 ? c->n : apa == 2 ? c->tutup : (c->n > 0 || c->pengirim != 0);
    if (r != 0) return r;
    jrg_ko_serah();
    return apa == 0 ? c->n : apa == 2 ? c->tutup : (c->n > 0 || c->pengirim != 0);
}
// ── Future/Task: channel berkapasitas satu yang hasilnya diserahkan SEKALI ──
void *jrg_masa_baru(void (*jalan)(void *), void *env, void *kode, int64_t esize, int32_t tugas) {
    JrgChan *c = jrg_chan_new(1, esize);
    c->sekali = tugas ? 2 : 1;
    void **paket = jrg_alloc(24, 8);
    paket[0] = env;
    paket[1] = c;
    paket[2] = kode;
    jrg_ko_baru_masa = 1;
    jrg_ko_spawn(jalan, paket);
    return c;
}

static void jrg_masa_sudah(const JrgChan *c, const char *nama, int64_t nl) {
    if (!c->diambil) return;
    static const char ekor[] = ": hasilnya sudah diambil sebelumnya — sebuah Future/Task menyerahkan hasilnya "
        "SEKALI. Simpan nilainya di sebuah variabel kalau perlu dipakai berkali-kali, atau bikin tugas baru";
    char pesan[320];
    int64_t n = nl < 60 ? nl : 60;
    memcpy(pesan, nama, (size_t)n);
    memcpy(pesan + n, ekor, sizeof ekor - 1);
    jrg_panic((const unsigned char *)pesan, n + (int64_t)(sizeof ekor - 1));
}

// Badan Future/Task yang gagal: galatnya dititipkan di channel-nya, goroutine-nya berhenti.
// Jawab 0 kalau yang sedang jalan bukan badan sebuah tugas.
int jrg_masa_gagal(const char *s, int64_t n) {
    JrgKo *k = jrg_ko_kini;
    if (k == &jrg_ko_utama || !k->masa) return 0;
    JrgChan *c = ((void **)k->arg)[1];
    c->gagal = jrg_alloc(n > 0 ? n : 1, 1);
    memcpy(c->gagal, s, (size_t)n);
    c->gagal_n = n;
    char *nol = jrg_alloc(c->esize > 0 ? c->esize : 1, 8);
    memset(nol, 0, (size_t)(c->esize > 0 ? c->esize : 1));
    jrg_chan_send(c, nol, c->esize);
    jrg_ko_tamat(k);
    return 1;
}

// Hasil yang baru diambil ternyata galat: gagal di titik pengambilannya, seperti dua mesin lain.
static void jrg_masa_periksa(JrgChan *c) {
    c->diambil = 1;
    if (!c->gagal) return;
    const char *awal = c->sekali == 2 ? "Task gagal: " : "Future gagal: ";
    int64_t na = (int64_t)strlen(awal);
    char *pesan = jrg_alloc(na + c->gagal_n + 1, 1);
    memcpy(pesan, awal, (size_t)na);
    memcpy(pesan + na, c->gagal, (size_t)c->gagal_n);
    jrg_panic((const unsigned char *)pesan, na + c->gagal_n);
}

// Tunggu hasilnya (`future::await`, `task::join`).
void jrg_masa_ambil(JrgChan *c, void *out, const char *nama, int64_t nl) {
    jrg_masa_sudah(c, nama, nl);
    jrg_chan_recv(c, out, c->esize);
    jrg_masa_periksa(c);
}

// Hasilnya kalau sudah ada dalam `mikro` mikrodetik (0 = tanpa menunggu): 1 diambil, 0 belum.
int32_t jrg_masa_coba(JrgChan *c, void *out, int64_t mikro, const char *nama, int64_t nl) {
    jrg_masa_sudah(c, nama, nl);
    int64_t tenggat = jrg_ko_jam() + mikro;
    for (;;) {
        if (c->n > 0) { jrg_chan_recv(c, out, c->esize); jrg_masa_periksa(c); return 1; }
        if (mikro <= 0 || jrg_ko_jam() >= tenggat) {
            // Beri goroutine lain satu giliran dulu: tugasnya mungkin tinggal menyerahkan hasil.
            if (mikro <= 0) { jrg_ko_serah(); if (c->n > 0) continue; }
            return 0;
        }
        jrg_ko_tidur(200);
    }
}

int32_t jrg_masa_selesai(JrgChan *c) {
    if (c->n == 0 && !c->diambil) jrg_ko_serah();
    return c->n > 0 || c->diambil;
}

// Indeks tugas pertama yang hasilnya siap, dari `n` penunjuk channel.
int64_t jrg_masa_balap(JrgChan **daftar, int64_t n) {
    for (;;) {
        for (int64_t i = 0; i < n; i++) if (daftar[i]->n > 0) return i;
        jrg_ko_tidur(200);
    }
}
#endif

// ── Atomic: satu `int64_t` di heap; satu utas, jadi baca-ubah-tulisnya tidak bisa disela ──
void *jrg_atom_baru(int64_t awal) {
    int64_t *a = jrg_alloc(8, 8);
    *a = awal;
    return a;
}

// ── Mutex: kunci berpemilik; yang menunggu diparkir sampai kuncinya lepas ──
typedef struct JrgKunci { JrgKo *pemilik; JrgKo *antre; int ditinggal; int dijaga; struct JrgKunci *next; } JrgKunci;
static JrgKunci *jrg_kunci_semua;

void *jrg_kunci_baru(void) {
    JrgKunci *k = jrg_alloc((int64_t)sizeof(JrgKunci), 8);
    memset(k, 0, sizeof *k);
    k->next = jrg_kunci_semua;
    jrg_kunci_semua = k;
    return k;
}

static void jrg_kunci_panik(const char *nama, int64_t nl, const char *ekor) {
    char pesan[400];
    int64_t n = nl < 60 ? nl : 60;
    size_t ne = strlen(ekor);
    memcpy(pesan, nama, (size_t)n);
    memcpy(pesan + n, ekor, ne);
    jrg_panic((const unsigned char *)pesan, n + (int64_t)ne);
}

static void jrg_kunci_bangun(JrgKunci *k) {
    JrgKo *w = k->antre;
    k->antre = 0;
    while (w) { JrgKo *n = w->next; jrg_ko_antre(w); w = n; }
}

static const char jrg_kunci_ditinggal[] = ": kunci ini ditinggal terkunci oleh goroutine yang sudah selesai — "
    "yang mengunci harus melepasnya (`mutex::unlock`), atau pakai `mutex::with_lock(m, f)` yang melepas sendiri";

// `dijaga` bukan nol: dikunci `mutex::with_lock`, jadi dilepas juga kalau goroutine-nya gagal.
void jrg_kunci_kunci(JrgKunci *k, int32_t dijaga, const char *nama, int64_t nl) {
    for (;;) {
        if (k->ditinggal) jrg_kunci_panik(nama, nl, jrg_kunci_ditinggal);
        if (!k->pemilik) break;
        if (k->pemilik == jrg_ko_kini) jrg_kunci_panik(nama, nl, ": kunci ini sudah kamu pegang di goroutine yang sama — "
            "mengunci lagi berarti menunggu dirimu sendiri selamanya. Lepas dulu dengan `mutex::unlock`");
        jrg_ko_kini->next = k->antre;
        k->antre = jrg_ko_kini;
        jrg_ko_jadwal();
    }
    k->pemilik = jrg_ko_kini;
    k->dijaga = dijaga;
}

void jrg_kunci_lepas(JrgKunci *k, const char *nama, int64_t nl) {
    if (k->ditinggal) jrg_kunci_panik(nama, nl, jrg_kunci_ditinggal);
    if (!k->pemilik) jrg_kunci_panik(nama, nl, ": kunci ini tidak sedang terkunci — tiap `mutex::unlock` butuh satu "
        "`mutex::lock` sebelumnya");
    if (k->pemilik != jrg_ko_kini) jrg_kunci_panik(nama, nl, ": kunci ini dipegang goroutine lain — cuma yang mengunci "
        "yang boleh melepas");
    k->pemilik = 0;
    jrg_kunci_bangun(k);
}

// Goroutine yang selesai sambil memegang kunci: yang dijaga dilepas, sisanya ditandai ditinggal.
static void jrg_kunci_tinggal(JrgKo *ko) {
    for (JrgKunci *k = jrg_kunci_semua; k; k = k->next) {
        if (k->pemilik != ko) continue;
        k->pemilik = 0;
        if (!k->dijaga) k->ditinggal = 1;
        jrg_kunci_bangun(k);
    }
}
