// jaga_stack.c — penjaga stack untuk biner hasil `--emit-llvm`. Nol biaya per panggilan:
// stack yang habis ditangkap SEKALI lewat sinyal di stack cadangan, lalu dilaporkan.
#if defined(__unix__) || defined(__APPLE__)
#include <signal.h>
#include <stdint.h>
#include <string.h>
#include <sys/resource.h>

void jrg_panic(const unsigned char *ptr, int64_t len);
void jrg_tunda_buang(void);
int jrg_ko_di_stack(uintptr_t alamat);

static char jrg_stack_cadangan[64 * 1024];
static uintptr_t jrg_stack_atas, jrg_stack_bawah;

static const char JRG_PESAN_STACK[] =
    "rekursi terlalu dalam — stack program habis sebelum rekursinya berhenti.\n"
    "  bantuan: pastikan ada kondisi berhenti (base case) yang PASTI tercapai, "
    "dan tiap panggilan rekursif bergerak mendekatinya\n"
    "  catatan: kalau memang butuh kedalaman sebesar ini, ubah jadi perulangan "
    "(`while`/`for`), atau perbesar stack (`ulimit -s`)";

static void jrg_tangani_stack(int sinyal, siginfo_t *info, void *konteks) {
    (void)konteks;
    uintptr_t alamat = (uintptr_t)info->si_addr;
    // Cuma galat di wilayah stack (plus halaman penjaganya) yang berarti stack habis.
    if ((alamat < jrg_stack_atas && alamat >= jrg_stack_bawah) || jrg_ko_di_stack(alamat)) {
        jrg_tunda_buang();
        jrg_panic((const unsigned char *)JRG_PESAN_STACK, (int64_t)(sizeof JRG_PESAN_STACK - 1));
    }
    // Selain itu bukan urusan penjaga ini: kembalikan perilaku bawaan dan biarkan terulang.
    signal(sinyal, SIG_DFL);
}

__attribute__((constructor)) static void jrg_pasang_jaga_stack(void) {
    char penanda;
    struct rlimit batas;
    uintptr_t besar = (uintptr_t)8 << 20;
    if (getrlimit(RLIMIT_STACK, &batas) == 0 && batas.rlim_cur != RLIM_INFINITY) {
        besar = (uintptr_t)batas.rlim_cur;
    } else {
        besar = (uintptr_t)1 << 30;
    }
    // Atas dibulatkan longgar (argumen dan lingkungan ada di atas penanda); bawah diberi
    // kelonggaran satu megabyte untuk halaman penjaga dan bingkai yang melompatinya.
    jrg_stack_atas = (uintptr_t)&penanda + ((uintptr_t)1 << 20);
    uintptr_t turun = besar + ((uintptr_t)1 << 20);
    jrg_stack_bawah = (uintptr_t)&penanda > turun ? (uintptr_t)&penanda - turun : 0;

    stack_t cadangan;
    memset(&cadangan, 0, sizeof cadangan);
    cadangan.ss_sp = jrg_stack_cadangan;
    cadangan.ss_size = sizeof jrg_stack_cadangan;
    if (sigaltstack(&cadangan, 0) != 0) return;

    struct sigaction aksi;
    memset(&aksi, 0, sizeof aksi);
    aksi.sa_sigaction = jrg_tangani_stack;
    aksi.sa_flags = SA_SIGINFO | SA_ONSTACK;
    sigemptyset(&aksi.sa_mask);
    sigaction(SIGSEGV, &aksi, 0);
    sigaction(SIGBUS, &aksi, 0);
}
#endif
