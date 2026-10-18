#!/bin/sh
# Bangun, bungkus, dan hitung checksum untuk platform MESIN INI.
set -eu

akar=$(cd "$(dirname "$0")/.." && pwd)
cd "$akar"

versi=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
os=$(uname -s | tr 'A-Z' 'a-z')
case "$os" in
  linux)  plat_os=linux ;;
  darwin) plat_os=macos ;;
  *)      plat_os=$os ;;
esac
arch=$(uname -m)
case "$arch" in
  arm64) arch=aarch64 ;;
esac
# Termux/Android: `uname -s` menjawab Linux, tapi binernya bionic, bukan glibc.
[ "$(uname -o 2>/dev/null)" = "Android" ] && plat_os=android
plat="$plat_os-$arch"
nama="jrg-$versi-$plat"

echo "versi    : $versi"
echo "platform : $plat"

cargo build --release
keluar="$akar/rilis/keluaran"
rm -rf "$keluar/$nama"
mkdir -p "$keluar/$nama"

for b in jrgc jrgm; do
  cp "target/release/$b" "$keluar/$nama/$b"
done
# Yang ikut: lisensi dan satu halaman cara mulai. Biner tanpa lisensi di
# dalamnya memaksa orang mencarinya, dan sebagian tidak akan mencari.
cp LICENSE "$keluar/$nama/" 2>/dev/null || true
cp README.md "$keluar/$nama/" 2>/dev/null || true

cd "$keluar"
tar czf "$nama.tar.gz" "$nama"
sha=$(sha256sum "$nama.tar.gz" | cut -d' ' -f1)
ukuran=$(wc -c < "$nama.tar.gz" | tr -d ' ')

printf '%s  %s\n' "$sha" "$nama.tar.gz" > "$nama.tar.gz.sha256"

cat <<JSON > "$nama.json"
{
  "platform": "$plat",
  "nama": "$nama.tar.gz",
  "sha256": "$sha",
  "ukuran": $ukuran,
  "isi": ["jrgc", "jrgm"]
}
JSON

echo "berkas   : $keluar/$nama.tar.gz"
echo "sha256   : $sha"
echo "ukuran   : $ukuran byte"
echo
echo "Periksa sendiri:  sha256sum -c $nama.tar.gz.sha256"
