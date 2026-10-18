#!/bin/sh
# benchmarks/acuan/jalankan.sh — waktu terbaik dari N putaran tiap kernel di interpreter dan --vm.
# Pakai: sh benchmarks/acuan/jalankan.sh [jrgc] [putaran]
set -u
cd "$(dirname "$0")"
J=${1:-../../target/release/jrgc}
N=${2:-3}
printf '%-16s %9s %9s  %s\n' kernel interp vm keluaran
for f in *.jrg; do
  baris="$(basename "$f" .jrg)"
  hasil=""
  for b in "" "--vm"; do
    terbaik=""
    for _ in $(seq "$N"); do
      m=$(date +%s%N)
      out=$(JRG_SOPAN=1 "$J" "$f" $b 2>&1 | tr '\n' ' ')
      s=$(( ($(date +%s%N) - m) / 1000000 ))
      [ -z "$terbaik" ] || [ "$s" -lt "$terbaik" ] && terbaik=$s
    done
    hasil="$hasil $(printf '%7sms' "$terbaik")"
  done
  printf '%-16s%s  %s\n' "$baris" "$hasil" "$(printf '%s' "$out" | cut -c1-40)"
done
