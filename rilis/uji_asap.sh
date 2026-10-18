#!/bin/sh
# Uji asap atas biner JADI: versi, konformans di dua mesin, siklus `jrgm`, `--emit-llvm` lewat
# pustaka jadi. Untuk runner tanpa suite penuh (suite memakai PTY, `unshare`, Xvfb).
# Pakai: sh rilis/uji_asap.sh <direktori-biner> <versi>
set -eu
dir=$(cd "$1" && pwd); versi=$2
akar=$(cd "$(dirname "$0")/.." && pwd)
ext=""; [ -f "$dir/jrgc.exe" ] && ext=".exe"
jrgc="$dir/jrgc$ext"; jrgm="$dir/jrgm$ext"
export JRG_SOPAN=1
gagal=0
cek() { if "$@" >/dev/null 2>&1; then echo "  ok: $*"; else echo "  GAGAL: $*" >&2; gagal=$((gagal + 1)); fi; }

echo "== versi =="
v=$("$jrgc" --version | tr -d '\r')
case "$v" in *"$versi"*) echo "  ok: $v" ;; *) echo "  GAGAL: jrgc --version '$v' bukan $versi" >&2; gagal=$((gagal + 1)) ;; esac
v=$("$jrgm" --version | tr -d '\r')
case "$v" in *"$versi"*) echo "  ok: $v" ;; *) echo "  GAGAL: jrgm --version '$v' bukan $versi" >&2; gagal=$((gagal + 1)) ;; esac

echo "== konformans (interpreter dan --vm) =="
for f in "$akar"/tests/conformance/*.jrg; do
  cek "$jrgc" "$f"
  cek "$jrgc" "$f" --vm
done

echo "== jrgm new / build / run / test =="
kerja=$(mktemp -d)
cd "$kerja"
cek "$jrgm" new asap
cd asap
cek "$jrgm" build
cek "$jrgm" run
cek "$jrgm" test
cd "$akar"
rm -rf "$kerja"

echo "== --emit-llvm lewat pustaka JADI (bukan sumber) =="
if [ -n "$ext" ]; then
  echo "  dilewati: --emit-llvm di Windows belum pernah diuji, jadi belum dijanjikan"
elif ! command -v clang >/dev/null 2>&1; then
  echo "  dilewati: tidak ada clang di runner ini"
elif [ ! -f "$dir/libjrgrt.a" ] && [ ! -f "$dir/jrgrt.lib" ]; then
  echo "  GAGAL: arsip ini tidak membawa runtime jrgrt" >&2; gagal=$((gagal + 1))
else
  kerja=$(mktemp -d)
  printf 'package main\npub fn main() { wok("native", 6 * 7) }\n' > "$kerja/n.jrg"
  hasil=$(cd "$kerja" && JRGC_RUNTIME_DIR=/tidak/ada JRGC_JEMBATAN_DIR=/tidak/ada "$jrgc" n.jrg --emit-llvm -o n.out 2>&1 | tr -d '\r')
  case "$hasil" in *"native 42"*) echo "  ok: --emit-llvm" ;; *) echo "  GAGAL: --emit-llvm: $hasil" >&2; gagal=$((gagal + 1)) ;; esac
  rm -rf "$kerja"
fi

if [ "$gagal" -ne 0 ]; then echo "UJI ASAP GAGAL: $gagal pemeriksaan" >&2; exit 1; fi
echo "UJI ASAP BERSIH"
