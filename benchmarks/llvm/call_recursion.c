#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>

static int64_t fib(int64_t n) {
    if (n < 2) return n;
    return fib(n - 1) + fib(n - 2);
}

static int64_t ack_lite(int64_t m, int64_t n) {
    if (m == 0) return n + 1;
    if (n == 0) return ack_lite(m - 1, 1);
    return ack_lite(m - 1, ack_lite(m, n - 1));
}

static int64_t step_a(int64_t v) {
    return v * 3 + 1;
}

static int64_t step_b(int64_t v) {
    return step_a(v) ^ (v >> 2);
}

static int64_t step_c(int64_t v) {
    return step_b(v) + step_a(v >> 1);
}

static int64_t chain(int64_t v) {
    return step_c(step_b(step_a(v)));
}

int main(void) {
    const int64_t iterations = 20000000;
    int64_t acc = 0;

    for (int64_t i = 0; i < iterations; ++i) {
        acc = (acc + chain(i)) % 1000000007;
    }

    acc = acc + fib(39);
    acc = acc + ack_lite(2, 8);

    printf("%" PRId64 "\n", acc);
    return 0;
}
