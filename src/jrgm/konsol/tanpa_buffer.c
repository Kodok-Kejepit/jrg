/* Ikut dikompilasi bersama program C/C++ yang dibungkus konsol APK: keluarannya pipa, jadi tanpa ini
   prompt tanpa baris baru (printf lalu scanf) tertahan di buffer sampai program selesai. */
#include <stdio.h>
__attribute__((constructor)) static void jrg_konsol_tanpa_buffer(void) {
    setvbuf(stdout, NULL, _IONBF, 0);
    setvbuf(stderr, NULL, _IONBF, 0);
}
