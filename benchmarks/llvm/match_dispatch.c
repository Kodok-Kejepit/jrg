#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>

/* Tagged union mirroring the JRG `enum Op` (tag + payload), so the
   dispatch being measured is the same shape in every language. */

enum OpTag { OP_PUSH, OP_ADDK, OP_MULK, OP_XOR, OP_ROT, OP_RANGE };

typedef struct {
    enum OpTag tag;
    int64_t a;
    int64_t b;
} Op;

static int64_t classify(int64_t v) {
    switch (v % 11) {
        case 0: return 5;
        case 1:
        case 2:
        case 3: return 7;
        case 4:
        case 5:
        case 6:
        case 7:
        case 8: return 11;
        default: return 13;
    }
}

static int64_t exec(Op o, int64_t acc) {
    switch (o.tag) {
        case OP_PUSH: return o.a;
        case OP_ADDK: return acc + o.a;
        case OP_MULK: return (acc * o.a) % 1000003;
        case OP_XOR:  return acc ^ INT64_C(1431655765);
        case OP_ROT:  return ((acc << 7) | (acc >> 25)) % 1000003;
        case OP_RANGE: {
            int64_t out = acc;
            if (acc < o.a) out = o.a;
            if (acc > o.b) out = o.b;
            return out;
        }
    }
    return acc;
}

int main(void) {
    const int64_t iterations = 45000000;
    int64_t acc = 1;
    int64_t total = 0;

    for (int64_t i = 0; i < iterations; ++i) {
        int64_t sel = i % 6;
        Op op;
        op.a = 0;
        op.b = 0;
        if (sel == 0) {
            op.tag = OP_PUSH;
            op.a = i % 997;
        } else if (sel == 1) {
            op.tag = OP_ADDK;
            op.a = i % 31;
        } else if (sel == 2) {
            op.tag = OP_MULK;
            op.a = 3 + i % 7;
        } else if (sel == 3) {
            op.tag = OP_XOR;
        } else if (sel == 4) {
            op.tag = OP_ROT;
        } else {
            op.tag = OP_RANGE;
            op.a = 10;
            op.b = 900000;
        }

        acc = exec(op, acc);
        total = (total + classify(acc)) % 1000000007;
    }

    printf("%" PRId64 " %" PRId64 "\n", acc, total);
    return 0;
}
