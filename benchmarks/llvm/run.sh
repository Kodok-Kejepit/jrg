#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="${JRG_BENCH_DIR:-/tmp/jrg-llvm-bench}"
ROUNDS="${ROUNDS:-5}"
CPU="${CPU:-}"
EXPECTED="5376449430840601188"
JRG_FULL_POWER="${JRG_FULL_POWER:-0}"

mkdir -p "$BUILD_DIR"

JRGC="${JRGC:-$ROOT_DIR/target/debug/jrgc}"
if [[ ! -x "$JRGC" ]]; then
    cargo build --bin jrgc --manifest-path "$ROOT_DIR/Cargo.toml"
fi

CLANG="${CC:-}"
if [[ -z "$CLANG" ]]; then
    for candidate in clang-21 clang-20 clang-19 clang-18 clang; do
        if command -v "$candidate" >/dev/null 2>&1; then
            CLANG="$candidate"
            break
        fi
    done
fi
if [[ -z "$CLANG" ]]; then
    echo "error: clang tidak ditemukan" >&2
    exit 1
fi

echo "== build =="
if [[ "$JRG_FULL_POWER" == "1" ]]; then
    "$JRGC" "$SCRIPT_DIR/numeric_kernel.jrg" \
        --emit-llvm --full-power --no-run \
        -o "$BUILD_DIR/numeric-jrg"
else
    "$JRGC" "$SCRIPT_DIR/numeric_kernel.jrg" \
        --emit-llvm -O3 --native-cpu --strip --no-run \
        -o "$BUILD_DIR/numeric-jrg"
fi
"$CLANG" "$SCRIPT_DIR/numeric_kernel.c" \
    -O3 -march=native -mtune=native -std=c11 -s \
    -o "$BUILD_DIR/numeric-c"

names=("JRG LLVM" "C clang")
bins=("$BUILD_DIR/numeric-jrg" "$BUILD_DIR/numeric-c")

if command -v rustc >/dev/null 2>&1; then
    if rustc "$SCRIPT_DIR/numeric_kernel.rs" \
        -O -C target-cpu=native -C strip=symbols \
        -o "$BUILD_DIR/numeric-rust"; then
        names+=("Rust")
        bins+=("$BUILD_DIR/numeric-rust")
    else
        echo "skip: rustc ditemukan tetapi build benchmark gagal" >&2
    fi
else
    echo "skip: rustc tidak ditemukan"
fi

if command -v go >/dev/null 2>&1; then
    if [[ "$(uname -m)" == "x86_64" ]]; then
        go_build=(env GOAMD64=v3 go build -ldflags="-s -w" -o "$BUILD_DIR/numeric-go" "$SCRIPT_DIR/numeric_kernel.go")
    else
        go_build=(go build -ldflags="-s -w" -o "$BUILD_DIR/numeric-go" "$SCRIPT_DIR/numeric_kernel.go")
    fi
    if "${go_build[@]}"; then
        names+=("Go")
        bins+=("$BUILD_DIR/numeric-go")
    else
        echo "skip: Go ditemukan tetapi build benchmark gagal" >&2
    fi
else
    echo "skip: go tidak ditemukan"
fi

run_binary() {
    local binary="$1"
    if [[ -n "$CPU" ]] && command -v taskset >/dev/null 2>&1; then
        taskset -c "$CPU" "$binary"
    else
        "$binary"
    fi
}

echo
echo "== verify =="
for index in "${!bins[@]}"; do
    output="$(run_binary "${bins[$index]}")"
    if [[ "$output" != "$EXPECTED" ]]; then
        echo "error: ${names[$index]} menghasilkan '$output', expected '$EXPECTED'" >&2
        exit 1
    fi
    printf "%-10s ok\n" "${names[$index]}"
done

echo
echo "== benchmark (${ROUNDS} rounds${CPU:+, CPU $CPU}) =="
declare -a totals bests
for index in "${!bins[@]}"; do
    totals[$index]=0
    bests[$index]=9223372036854775807
    run_binary "${bins[$index]}" >/dev/null
done

for ((round = 1; round <= ROUNDS; round++)); do
    for index in "${!bins[@]}"; do
        start="$(date +%s%N)"
        run_binary "${bins[$index]}" >/dev/null
        stop="$(date +%s%N)"
        elapsed_ms=$(((stop - start) / 1000000))
        totals[$index]=$((totals[$index] + elapsed_ms))
        if ((elapsed_ms < bests[$index])); then
            bests[$index]="$elapsed_ms"
        fi
        printf "round=%d %-10s %d ms\n" "$round" "${names[$index]}" "$elapsed_ms"
    done
done

echo
echo "== summary =="
for index in "${!bins[@]}"; do
    average_ms=$((totals[$index] / ROUNDS))
    file_bytes="$(wc -c < "${bins[$index]}")"
    printf "%-10s avg=%d ms best=%d ms file=%d bytes\n" \
        "${names[$index]}" "$average_ms" "${bests[$index]}" "$file_bytes"
done
