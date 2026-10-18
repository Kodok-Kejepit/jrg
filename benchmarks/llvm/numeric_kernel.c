#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>

static uint64_t kernel(int64_t iterations) {
    uint64_t x = UINT64_C(305419896);
    for (int64_t i = 0; i < iterations; ++i) {
        x = (x * UINT64_C(1664525) + UINT64_C(1013904223)) ^ (x >> 13);
    }
    return x;
}

int main(void) {
    printf("%" PRIu64 "\n", kernel(INT64_C(200000000)));
    return 0;
}
