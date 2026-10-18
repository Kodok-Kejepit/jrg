#!/bin/sh
# Pemasang JRG.
set -eu

REPO="${JRG_REPO:-Kodok-Kejepit/jrg}"
VERSI="terbaru"
KE="${JRG_KE:-$HOME/.local/bin}"
KE_DIPILIH="${JRG_KE:-}"
SUMBER="${JRG_SUMBER:-https://github.com/$REPO/releases}"

while [ $# -gt 0 ]; do
  case "$1" in
    --versi) VERSI="$2"; shift 2 ;;
    --ke)    KE="$2"; KE_DIPILIH=1; shift 2 ;;
    --bantuan|-h)
      sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'
      exit 0 ;;
    *) echo "pilihan tidak dikenal: $1" >&2; exit 2 ;;
  esac
done

butuh() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "butuh \`$1\`, tapi tidak ada di PATH." >&2
    exit 1
  }
}
butuh tar

unduh() {
  if command -v curl >/dev/null 2>&1; then curl -fsSL "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then wget -qO "$2" "$1"
  else echo "butuh \`curl\` atau \`wget\`." >&2; exit 1
  fi
}

# ── Platform ────────────────────────────────────────────────────────────
os=$(uname -s | tr 'A-Z' 'a-z')
case "$os" in
  linux)  os=linux ;;
  darwin) os=macos ;;
  msys*|mingw*|cygwin*) os=windows ;;
  *) echo "sistem \`$os\` belum punya biner siap pakai." >&2
     echo "Bangun dari sumber: cargo build --release" >&2
     exit 1 ;;
esac
arch=$(uname -m)
case "$arch" in
  x86_64|amd64) arch=x86_64 ;;
  aarch64|arm64) arch=aarch64 ;;
  *) echo "arsitektur \`$arch\` belum punya biner siap pakai." >&2
     echo "Bangun dari sumber: cargo build --release" >&2
     exit 1 ;;
esac
plat="$os-$arch"

# Termux bukan glibc: biner Linux biasa di sana menjawab "not found" yang menyesatkan tentang
# berkas yang jelas ada, jadi Termux memakai biner Android (bionic) dan memasang ke `$PREFIX/bin`.
if [ -n "${PREFIX:-}" ] && [ -d "$PREFIX/bin" ] && echo "$PREFIX" | grep -q com.termux; then
  plat="android-$arch"
  [ -n "$KE_DIPILIH" ] || KE="$PREFIX/bin"
fi

kerja=$(mktemp -d)
trap 'rm -rf "$kerja"' EXIT INT TERM

# ── Manifes ─────────────────────────────────────────────────────────────
if [ "$VERSI" = "terbaru" ]; then
  url_manifes="$SUMBER/latest/download/manifes.json"
else
  url_manifes="$SUMBER/download/v$VERSI/manifes.json"
fi
echo "-> mengambil manifes ($VERSI)"
unduh "$url_manifes" "$kerja/manifes.json" || {
  echo "manifes tidak bisa diambil dari $url_manifes" >&2
  exit 1
}

# Satu field dari manifes untuk platform ini.
baca() {
  if command -v python3 >/dev/null 2>&1 && python3 -c "" 2>/dev/null; then
    python3 - "$kerja/manifes.json" "$plat" "$1" <<'PY' 2>/dev/null || true
import json, sys
m = json.load(open(sys.argv[1]))
for b in m["berkas"]:
    if b["platform"] == sys.argv[2]:
        print(b[sys.argv[3]]); break
PY
  else
    tr -d ' \t\n' < "$kerja/manifes.json" \
      | tr '{}' '\n\n' \
      | grep "\"platform\":\"$plat\"" \
      | sed 's/.*"'"$1"'":"\{0,1\}\([^",]*\)"\{0,1\}.*/\1/' \
      | head -1 || true
  fi
}

berkas=$(baca nama)
sha=$(baca sha256)
versi_nyata=$(grep -o '"versi": *"[^"]*"' "$kerja/manifes.json" | head -1 | sed 's/.*"\([^"]*\)"$/\1/')

# Gagal di sini harus BERBICARA. Versi pertama skrip ini mati diam-diam
# waktu `python3` tidak ada — `set -e` menghentikannya di tengah substitusi
# perintah, jadi yang terlihat cuma "mengambil manifes" lalu tidak ada apa-apa.
if [ -z "$berkas" ] || [ -z "$sha" ]; then
  echo "manifes tidak memuat berkas untuk \`$plat\`, atau tidak bisa dibaca." >&2
  echo "Platform yang ada di manifes:" >&2
  tr -d ' \t\n' < "$kerja/manifes.json" | tr '{}' '\n\n' \
    | sed -n 's/.*"platform":"\([^"]*\)".*/  \1/p' >&2 || true
  exit 1
fi

echo "-> mengunduh $berkas"
unduh "${url_manifes%/*}/$berkas" "$kerja/$berkas"

# ── Periksa DULU, pasang kemudian ───────────────────────────────────────
if command -v sha256sum >/dev/null 2>&1; then
  nyata=$(sha256sum "$kerja/$berkas" | cut -d' ' -f1)
elif command -v shasum >/dev/null 2>&1; then
  nyata=$(shasum -a 256 "$kerja/$berkas" | cut -d' ' -f1)
else
  nyata=""
fi

if [ -z "$nyata" ]; then
  echo "PERINGATAN: tidak ada \`sha256sum\` maupun \`shasum\`, jadi berkasnya" >&2
  echo "            TIDAK diperiksa. Pasang manual kalau ini penting." >&2
elif [ "$nyata" != "$sha" ]; then
  echo "CHECKSUM TIDAK COCOK — pemasangan dibatalkan." >&2
  echo "  manifes : $sha" >&2
  echo "  berkas  : $nyata" >&2
  exit 1
else
  echo "-> sha256 cocok"
fi

# ── Pasang ──────────────────────────────────────────────────────────────
mkdir -p "$KE"
tar xzf "$kerja/$berkas" -C "$kerja"
isi=$(find "$kerja" -maxdepth 1 -type d -name 'jrg-*' | head -1)
for b in jrgc jrgm; do
  [ -f "$isi/$b" ] || continue
  cp "$isi/$b" "$KE/$b"
  chmod +x "$KE/$b"
done
# Pustaka `--emit-llvm` (runtime dan jembatan) ke `<tujuan>/../lib/jrg`, tempat jrgc mencarinya.
pustaka="$(cd "$KE/.." && pwd)/lib/jrg"
for p in "$isi"/libjrgrt.a "$isi"/jrgrt.lib "$isi"/libjrgjembatan.so "$isi"/libjrgjembatan.dylib; do
  [ -f "$p" ] || continue
  mkdir -p "$pustaka"
  cp "$p" "$pustaka/"
done

echo
echo "JRG $versi_nyata terpasang di $KE"
case ":$PATH:" in
  *":$KE:"*) ;;
  *)
    echo
    echo "$KE belum ada di PATH. Tambahkan ke ~/.profile atau ~/.zshrc:"
    echo "    export PATH=\"\$PATH:$KE\""
    ;;
esac
echo
echo "Coba:  jrgc --version   dan   jrgc   (REPL)"
