#!/bin/sh
# Turunkan indeks slot JNI dari jni.h milik JDK. Indeks yang meleset berarti
# memanggil fungsi lain dengan tanda tangan lain — proses mati tanpa pesan.
# Jalankan dari akar repo; bandingkan keluarannya dengan `mod idx` di src/ffi/jvm.rs.
set -e
JDK=${1:-/usr/lib/jvm/java-25-openjdk-amd64}
gcc -I"$JDK/include" -I"$JDK/include/linux" -o /tmp/jni_indeks perkakas/jni_indeks.c
/tmp/jni_indeks
