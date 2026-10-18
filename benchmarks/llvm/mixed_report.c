#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Shortest representation that still round-trips back to the exact same
   double — this is what Rust/Go/JRG all print by default, and C has no
   printf specifier for it, so reproduce it explicitly. Without this the
   four implementations disagree on digits while agreeing on the value. */
static void print_shortest(double v) {
    char buf[64];
    for (int prec = 1; prec <= 17; ++prec) {
        snprintf(buf, sizeof buf, "%.*g", prec, v);
        if (strtod(buf, NULL) == v) break;
    }
    printf("%s\n", buf);
}

typedef struct {
    int64_t id;
    double amount;
    int64_t weight;
    int64_t tier;
} Record;

static double score(Record self) {
    return self.amount * (1.0 + (double)self.weight * 0.01);
}

static int64_t tier_of(int64_t weight) {
    switch (weight % 7) {
        case 0: return 0;
        case 1:
        case 2: return 1;
        case 3:
        case 4:
        case 5: return 2;
        default: return 3;
    }
}

static uint64_t fnv(uint64_t h, const char *s, size_t len) {
    uint64_t acc = h;
    for (size_t i = 0; i < len; ++i) {
        acc = (acc ^ (uint64_t)(unsigned char)s[i]) * UINT64_C(1099511628211);
    }
    return acc;
}

int main(void) {
    const int64_t records = 12000000;
    const int64_t buckets = 64;

    static int64_t counts[64];
    static double sums[64];
    for (int64_t b = 0; b < buckets; ++b) {
        counts[b] = 0;
        sums[b] = 0.0;
    }

    uint64_t seed = UINT64_C(88172645463325252);
    uint64_t hash = UINT64_C(1469598103934665603);
    int64_t tier_total = 0;

    for (int64_t r = 0; r < records; ++r) {
        seed = seed * UINT64_C(6364136223846793005) + UINT64_C(1442695040888963407);
        int64_t w = (int64_t)((seed >> 33) % 1000);
        seed = seed * UINT64_C(6364136223846793005) + UINT64_C(1442695040888963407);
        double amt = (double)((seed >> 33) % 100000) * 0.01;

        Record rec;
        rec.id = r;
        rec.amount = amt;
        rec.weight = w;
        rec.tier = tier_of(w);

        int64_t b = rec.id % buckets;
        counts[b] = counts[b] + 1;
        sums[b] = sums[b] + score(rec);
        tier_total = tier_total + rec.tier;

        if (r % 16 == 0) {
            char label[64];
            int n = snprintf(label, sizeof label, "rec%" PRId64 "/t%" PRId64,
                             rec.id % 1000, rec.tier);
            hash = fnv(hash, label, (size_t)n);
        }
    }

    int64_t total_count = 0;
    double total_sum = 0.0;
    for (int64_t b = 0; b < buckets; ++b) {
        total_count = total_count + counts[b];
        total_sum = total_sum + sums[b];
        hash = (hash ^ (uint64_t)counts[b]) * UINT64_C(1099511628211);
    }

    printf("%" PRId64 " %" PRId64 " %" PRIu64 "\n", total_count, tier_total, hash);
    print_shortest(total_sum);
    return 0;
}
