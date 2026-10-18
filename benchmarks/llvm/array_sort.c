#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>

#define N 2000
#define ROUNDS 3000

int main(void) {
    const int64_t last = N - 1;
    uint64_t hash = UINT64_C(1469598103934665603);
    uint64_t seed = UINT64_C(88172645463325252);
    int any_failed = 0;
    static int64_t data[N];

    for (int64_t r = 0; r < ROUNDS; ++r) {
        for (int64_t i = 0; i < N; ++i) {
            seed = seed * UINT64_C(6364136223846793005) + UINT64_C(1442695040888963407);
            data[i] = (int64_t)(seed >> 1);
        }

        /* ---- build heap ---- */
        for (int64_t start = (N - 2) / 2; start >= 0; --start) {
            int64_t root = start;
            for (;;) {
                int64_t child = root * 2 + 1;
                if (child > last) break;
                if (child + 1 <= last) {
                    if (data[child] < data[child + 1]) child = child + 1;
                }
                if (data[root] < data[child]) {
                    int64_t tmp = data[root];
                    data[root] = data[child];
                    data[child] = tmp;
                    root = child;
                } else {
                    break;
                }
            }
        }

        /* ---- drain heap ---- */
        for (int64_t end = last; end > 0;) {
            int64_t tmp0 = data[0];
            data[0] = data[end];
            data[end] = tmp0;
            --end;

            int64_t root = 0;
            for (;;) {
                int64_t child = root * 2 + 1;
                if (child > end) break;
                if (child + 1 <= end) {
                    if (data[child] < data[child + 1]) child = child + 1;
                }
                if (data[root] < data[child]) {
                    int64_t tmp = data[root];
                    data[root] = data[child];
                    data[child] = tmp;
                    root = child;
                } else {
                    break;
                }
            }
        }

        int64_t prev = data[0];
        for (int64_t i = 0; i < N; ++i) {
            if (data[i] < prev) any_failed = 1;
            prev = data[i];
            hash = (hash ^ (uint64_t)data[i]) * UINT64_C(11400714819323198485);
        }
    }

    if (any_failed) {
        printf("SORT_FAILED\n");
    } else {
        printf("%" PRIu64 "\n", hash);
    }
    return 0;
}
