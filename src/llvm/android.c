// android.c — pintu masuk APK: NativeActivity menjalankan `main` program JRG di utasnya.
// stdout/stderr diteruskan ke logcat; mode `"tab"` digambar ke layar lewat `jrg_android_pasang`.
#include <android/log.h>
#include <android/native_window.h>
#include <android_native_app_glue.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(int, char **);

typedef struct {
    int32_t (*tampil)(const uint32_t *, int32_t, int32_t);
    void (*masukan)(float *, float *, int32_t *, int32_t *);
    int32_t (*sentuhan)(float *, int32_t);
} JrgAndroid;
// Ada hanya kalau programnya memakai jembatan (mis. `Canvas::`).
extern void jrg_android_pasang(const JrgAndroid *) __attribute__((weak));

#define JRG_MAKS_JARI 10
static struct android_app *jrg_app;
static int32_t jrg_layar_w, jrg_layar_h;          // ukuran permukaan jendela, piksel layar
static int32_t jrg_buf_w, jrg_buf_h, jrg_ox, jrg_oy; // penyangga yang diskalakan sistem
static int32_t jrg_k = 1;                         // pembesaran bulat kanvas di dalam penyangga
static ANativeWindow *jrg_geometri_untuk;
static float jrg_sx, jrg_sy;
static int32_t jrg_sentuh, jrg_kembali;
static float jrg_jari[JRG_MAKS_JARI][2];
static int32_t jrg_jari_n;

static void jrg_cmd(struct android_app *app, int32_t cmd) {
    if (cmd == APP_CMD_INIT_WINDOW && app->window) {
        jrg_layar_w = ANativeWindow_getWidth(app->window);
        jrg_layar_h = ANativeWindow_getHeight(app->window);
        jrg_geometri_untuk = 0;
    }
}

static int32_t jrg_input(struct android_app *app, AInputEvent *e) {
    (void)app;
    if (AInputEvent_getType(e) == AINPUT_EVENT_TYPE_MOTION) {
        int32_t penuh = AMotionEvent_getAction(e);
        int32_t aksi = penuh & AMOTION_EVENT_ACTION_MASK;
        int32_t lepas = -1;   // jari yang baru diangkat masih ikut di kejadian ini
        if (aksi == AMOTION_EVENT_ACTION_POINTER_UP)
            lepas = (penuh & AMOTION_EVENT_ACTION_POINTER_INDEX_MASK) >> AMOTION_EVENT_ACTION_POINTER_INDEX_SHIFT;
        int32_t habis = aksi == AMOTION_EVENT_ACTION_UP || aksi == AMOTION_EVENT_ACTION_CANCEL;
        jrg_jari_n = 0;
        size_t n = habis ? 0 : AMotionEvent_getPointerCount(e);
        for (size_t i = 0; i < n && jrg_jari_n < JRG_MAKS_JARI; i++) {
            if ((int32_t)i == lepas) continue;
            jrg_jari[jrg_jari_n][0] = AMotionEvent_getX(e, i);
            jrg_jari[jrg_jari_n][1] = AMotionEvent_getY(e, i);
            jrg_jari_n++;
        }
        jrg_sx = AMotionEvent_getX(e, 0);
        jrg_sy = AMotionEvent_getY(e, 0);
        jrg_sentuh = !habis;
        return 1;
    }
    if (AInputEvent_getType(e) == AINPUT_EVENT_TYPE_KEY && AKeyEvent_getKeyCode(e) == AKEYCODE_BACK) {
        if (AKeyEvent_getAction(e) == AKEY_EVENT_ACTION_UP) jrg_kembali = 1;
        return 1;
    }
    return 0;
}

// Proses kejadian yang menunggu; `ms` < 0 menunggu sampai ada satu.
static void jrg_pompa(int ms) {
    int ev;
    struct android_poll_source *src;
    while (ALooper_pollAll(ms, 0, &ev, (void **)&src) >= 0) {
        if (src) src->process(jrg_app, src);
        if (jrg_app->destroyRequested || ms != 0) return;
    }
}

// Kanvas dibesarkan bulat (k kali, piksel tetap tajam) lalu dilebarkan ke rasio layar;
// sisa skala yang kecil dikerjakan sistem, dan pinggirnya hitam. `-DJRG_GAMBAR_HALUS`: k = 1.
static void jrg_atur_geometri(ANativeWindow *w, int32_t cw, int32_t ch) {
    int32_t k = 1;
#ifndef JRG_GAMBAR_HALUS
    if (jrg_layar_w > 0 && jrg_layar_h > 0) {
        k = jrg_layar_w / cw < jrg_layar_h / ch ? jrg_layar_w / cw : jrg_layar_h / ch;
        if (k < 1) k = 1;
    }
#endif
    int32_t sw = cw * k, sh = ch * k;
    jrg_buf_w = sw; jrg_buf_h = sh;
    if (jrg_layar_w > 0 && jrg_layar_h > 0) {
        if ((int64_t)jrg_layar_w * sh > (int64_t)jrg_layar_h * sw)
            jrg_buf_w = (int32_t)((int64_t)sh * jrg_layar_w / jrg_layar_h);
        else
            jrg_buf_h = (int32_t)((int64_t)sw * jrg_layar_h / jrg_layar_w);
    }
    jrg_k = k;
    jrg_ox = (jrg_buf_w - sw) / 2;
    jrg_oy = (jrg_buf_h - sh) / 2;
    ANativeWindow_setBuffersGeometry(w, jrg_buf_w, jrg_buf_h, WINDOW_FORMAT_RGBX_8888);
    jrg_geometri_untuk = w;
}

static int32_t jrg_tampil(const uint32_t *px, int32_t cw, int32_t ch) {
    jrg_pompa(0);
    // Di latar belakang jendelanya tidak ada: program berhenti di sini sampai kembali.
    while (!jrg_app->window && !jrg_app->destroyRequested) jrg_pompa(-1);
    if (jrg_app->destroyRequested || jrg_kembali) return 0;
    ANativeWindow *w = jrg_app->window;
    if (jrg_geometri_untuk != w || jrg_buf_w < cw * jrg_k || jrg_buf_h < ch * jrg_k) jrg_atur_geometri(w, cw, ch);
    ANativeWindow_Buffer b;
    if (ANativeWindow_lock(w, &b, 0) != 0) return 1;
    uint32_t *tuju = (uint32_t *)b.bits;
    int32_t k = jrg_k, sh = ch * k, akhir_x = jrg_ox + cw * k;
    if (akhir_x > b.width) akhir_x = b.width;
    for (int32_t y = 0; y < b.height; y++) {
        uint32_t *baris = tuju + (int64_t)y * b.stride;
        int32_t ry = y - jrg_oy;
        if (ry < 0 || ry >= sh) {
            for (int32_t x = 0; x < b.width; x++) baris[x] = 0xFF000000u;
            continue;
        }
        // Baris kanvas yang sama dipakai k baris berturut-turut: salin yang di atasnya.
        if (ry % k != 0) { memcpy(baris, baris - b.stride, (size_t)b.width * 4); continue; }
        const uint32_t *sumber = px + (int64_t)(ry / k) * cw;
        int32_t x = 0;
        for (; x < jrg_ox && x < b.width; x++) baris[x] = 0xFF000000u;
        for (int32_t sx = 0; sx < cw && x < akhir_x; sx++) {
            uint32_t a = sumber[sx];
            uint32_t p = ((a >> 16) & 0xFF) | (a & 0xFF00) | ((a & 0xFF) << 16) | 0xFF000000u;
            for (int32_t j = 0; j < k && x < akhir_x; j++) baris[x++] = p;
        }
        for (; x < b.width; x++) baris[x] = 0xFF000000u;
    }
    ANativeWindow_unlockAndPost(w);
    return 1;
}

// Titik layar ke koordinat kanvas, lewat penyangga dan pembesarannya.
static void jrg_ke_kanvas(float sx, float sy, float *x, float *y) {
    float kx = jrg_layar_w > 0 ? (float)jrg_buf_w / (float)jrg_layar_w : 1.0f;
    float ky = jrg_layar_h > 0 ? (float)jrg_buf_h / (float)jrg_layar_h : 1.0f;
    *x = (sx * kx - (float)jrg_ox) / (float)jrg_k;
    *y = (sy * ky - (float)jrg_oy) / (float)jrg_k;
}

static void jrg_masukan(float *x, float *y, int32_t *sentuh, int32_t *kembali) {
    jrg_ke_kanvas(jrg_sx, jrg_sy, x, y);
    *sentuh = jrg_sentuh;
    *kembali = jrg_kembali;
}

// Semua jari yang menempel, maksimal `maks`, sebagai pasangan x,y kanvas.
static int32_t jrg_sentuhan(float *xy, int32_t maks) {
    int32_t n = jrg_jari_n < maks ? jrg_jari_n : maks;
    for (int32_t i = 0; i < n; i++) jrg_ke_kanvas(jrg_jari[i][0], jrg_jari[i][1], &xy[2 * i], &xy[2 * i + 1]);
    return n;
}

static int jrg_pipa[2];
static void *jrg_ke_logcat(void *x) {
    (void)x;
    char buf[4096];
    size_t isi = 0;
    for (;;) {
        ssize_t n = read(jrg_pipa[0], buf + isi, sizeof buf - 1 - isi);
        if (n <= 0) break;
        isi += (size_t)n;
        size_t awal = 0;
        for (size_t i = 0; i < isi; i++) {
            if (buf[i] == '\n') {
                buf[i] = 0;
                __android_log_write(ANDROID_LOG_INFO, "jrg", buf + awal);
                awal = i + 1;
            }
        }
        memmove(buf, buf + awal, isi - awal);
        isi -= awal;
        if (isi == sizeof buf - 1) { buf[isi] = 0; __android_log_write(ANDROID_LOG_INFO, "jrg", buf); isi = 0; }
    }
    if (isi) { buf[isi] = 0; __android_log_write(ANDROID_LOG_INFO, "jrg", buf); }
    return 0;
}

void android_main(struct android_app *app) {
    jrg_app = app;
    app->onAppCmd = jrg_cmd;
    app->onInputEvent = jrg_input;
    pthread_t log = 0;
    if (pipe(jrg_pipa) == 0) {
        dup2(jrg_pipa[1], 1);
        dup2(jrg_pipa[1], 2);
        close(jrg_pipa[1]);
        pthread_create(&log, 0, jrg_ke_logcat, 0);
    }
    if (jrg_android_pasang) {
        static const JrgAndroid tabel = { jrg_tampil, jrg_masukan, jrg_sentuhan };
        jrg_android_pasang(&tabel);
    }
    // Direktori kerja yang bisa ditulisi, untuk mode `"png"` dan `fs::`.
    if (app->activity->internalDataPath) chdir(app->activity->internalDataPath);
    char *argv[] = { "jrg", 0 };
    int rc = main(1, argv);
    // Proses tidak dipakai ulang: keadaan runtime milik satu kali jalan.
    ANativeActivity_finish(app->activity);
    while (!app->destroyRequested) jrg_pompa(-1);
    close(1);
    close(2);
    if (log) pthread_join(log, 0);
    // `_exit`, bukan `exit`: destruktor statis bertabrakan dengan utas HWUI milik sistem.
    _exit(rc);
}
