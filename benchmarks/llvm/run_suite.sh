#!/usr/bin/env bash
# Multi-kernel cross-language benchmark suite for the JRG LLVM backend.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="${JRG_BENCH_DIR:-/tmp/jrg-llvm-suite}"
ROUNDS="${ROUNDS:-5}"
CPU="${CPU:-}"
BASELINE_OUT="${BASELINE_OUT:-}"

# numeric_kernel is the pre-existing single-kernel benchmark; the rest were
# added to cover array / struct / string+allocation / call+recursion /
# pattern-matching / mixed workloads.
DEFAULT_KERNELS="numeric_kernel array_sort struct_particles string_ops call_recursion match_dispatch mixed_report"
KERNELS="${KERNELS:-$DEFAULT_KERNELS}"

# JRG native codegen flags. Kept identical for every kernel so the numbers
# are comparable across kernels and across runs.
JRG_FLAGS="${JRG_FLAGS:---emit-llvm -O3 --native-cpu --strip --no-run}"

mkdir -p "$BUILD_DIR"

JRGC="${JRGC:-$ROOT_DIR/target/release/jrgc}"
if [[ ! -x "$JRGC" ]]; then
    echo "error: jrgc tidak ditemukan di $JRGC (build dulu: cargo build --release --bin jrgc)" >&2
    exit 1
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

HAVE_RUST=0
command -v rustc >/dev/null 2>&1 && HAVE_RUST=1 || echo "skip: rustc tidak ditemukan"
HAVE_GO=0
command -v go >/dev/null 2>&1 && HAVE_GO=1 || echo "skip: go tidak ditemukan"

GO_ENV=()
if [[ "$(uname -m)" == "x86_64" ]]; then
    GO_ENV=(env GOAMD64=v3)
fi

run_binary() {
    if [[ -n "$CPU" ]] && command -v taskset >/dev/null 2>&1; then
        taskset -c "$CPU" "$1"
    else
        "$1"
    fi
}

# ─── build ──────────────────────────────────────────────────────────────
echo "== build =="
declare -A BUILT   # "kernel|lang" -> binary path
for kernel in $KERNELS; do
    src_jrg="$SCRIPT_DIR/$kernel.jrg"
    if [[ ! -f "$src_jrg" ]]; then
        echo "error: $src_jrg tidak ada" >&2
        exit 1
    fi

    out="$BUILD_DIR/$kernel-jrg"
    # shellcheck disable=SC2086
    "$JRGC" "$src_jrg" $JRG_FLAGS -o "$out" >/dev/null
    BUILT["$kernel|JRG"]="$out"

    if [[ -f "$SCRIPT_DIR/$kernel.c" ]]; then
        out="$BUILD_DIR/$kernel-c"
        # -ffp-contract=off: clang fuses a*b+c into an FMA by default, which
        # changes float results and would make the cross-language output
        # comparison fail on value, not on correctness.
        "$CLANG" "$SCRIPT_DIR/$kernel.c" \
            -O3 -march=native -mtune=native -ffp-contract=off -std=c11 -s \
            -o "$out"
        BUILT["$kernel|C"]="$out"
    fi

    if [[ "$HAVE_RUST" == "1" && -f "$SCRIPT_DIR/$kernel.rs" ]]; then
        out="$BUILD_DIR/$kernel-rust"
        if rustc "$SCRIPT_DIR/$kernel.rs" \
            -O -C target-cpu=native -C strip=symbols -o "$out" 2>/dev/null; then
            BUILT["$kernel|Rust"]="$out"
        else
            echo "  skip: build Rust gagal untuk $kernel" >&2
        fi
    fi

    if [[ "$HAVE_GO" == "1" && -f "$SCRIPT_DIR/$kernel.go" ]]; then
        out="$BUILD_DIR/$kernel-go"
        if "${GO_ENV[@]}" go build -ldflags="-s -w" -o "$out" "$SCRIPT_DIR/$kernel.go" 2>/dev/null; then
            BUILT["$kernel|Go"]="$out"
        else
            echo "  skip: build Go gagal untuk $kernel" >&2
        fi
    fi

    printf "  %-18s ok\n" "$kernel"
done

LANGS=("JRG" "C" "Rust" "Go")

# ─── verify ─────────────────────────────────────────────────────────────
# JRG's output is the reference; every other implementation of the same
# kernel must match it byte for byte.
echo
echo "== verify =="
for kernel in $KERNELS; do
    expected="$(run_binary "${BUILT["$kernel|JRG"]}")"
    for lang in "${LANGS[@]}"; do
        bin="${BUILT["$kernel|$lang"]:-}"
        [[ -z "$bin" ]] && continue
        actual="$(run_binary "$bin")"
        if [[ "$actual" != "$expected" ]]; then
            echo "error: $kernel/$lang output berbeda" >&2
            echo "--- expected (JRG) ---" >&2
            echo "$expected" >&2
            echo "--- actual ($lang) ---" >&2
            echo "$actual" >&2
            exit 1
        fi
    done
    printf "  %-18s ok (semua implementasi identik)\n" "$kernel"
done

# ─── benchmark ──────────────────────────────────────────────────────────
echo
echo "== benchmark (${ROUNDS} rounds${CPU:+, CPU $CPU}) =="
declare -A TOTAL BEST
for kernel in $KERNELS; do
    for lang in "${LANGS[@]}"; do
        [[ -z "${BUILT["$kernel|$lang"]:-}" ]] && continue
        TOTAL["$kernel|$lang"]=0
        BEST["$kernel|$lang"]=9223372036854775807
        run_binary "${BUILT["$kernel|$lang"]}" >/dev/null   # warm-up
    done
done

for ((round = 1; round <= ROUNDS; round++)); do
    for kernel in $KERNELS; do
        for lang in "${LANGS[@]}"; do
            bin="${BUILT["$kernel|$lang"]:-}"
            [[ -z "$bin" ]] && continue
            start="$(date +%s%N)"
            run_binary "$bin" >/dev/null
            stop="$(date +%s%N)"
            elapsed=$(((stop - start) / 1000000))
            TOTAL["$kernel|$lang"]=$((TOTAL["$kernel|$lang"] + elapsed))
            if ((elapsed < BEST["$kernel|$lang"])); then
                BEST["$kernel|$lang"]=$elapsed
            fi
        done
    done
    printf "  round %d selesai\n" "$round"
done

# ─── report ─────────────────────────────────────────────────────────────
emit_report() {
    printf "%-18s %-6s %9s %9s %12s\n" "KERNEL" "LANG" "AVG(ms)" "BEST(ms)" "SIZE(bytes)"
    for kernel in $KERNELS; do
        for lang in "${LANGS[@]}"; do
            bin="${BUILT["$kernel|$lang"]:-}"
            [[ -z "$bin" ]] && continue
            avg=$((TOTAL["$kernel|$lang"] / ROUNDS))
            size="$(wc -c < "$bin")"
            printf "%-18s %-6s %9d %9d %12d\n" \
                "$kernel" "$lang" "$avg" "${BEST["$kernel|$lang"]}" "$size"
        done
    done
}

echo
echo "== summary =="
emit_report

if [[ -n "$BASELINE_OUT" ]]; then
    emit_report > "$BASELINE_OUT"
    echo
    echo "baseline disimpan ke $BASELINE_OUT"
fi
