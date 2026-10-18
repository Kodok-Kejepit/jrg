#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Mirrors the JRG version: every step allocates fresh buffers for the
   concatenation and for each transform, so allocation traffic is part of
   what is measured. */

static char *xalloc(size_t n) {
    char *p = (char *)malloc(n);
    if (!p) {
        fprintf(stderr, "oom\n");
        exit(1);
    }
    return p;
}

static char *concat_build(int64_t r, size_t *out_len) {
    char buf[64];
    int n = snprintf(buf, sizeof buf, "item-%" PRId64 ":%" PRId64,
                     r % 997, r % 31);
    char *s = xalloc((size_t)n + 1);
    memcpy(s, buf, (size_t)n + 1);
    *out_len = (size_t)n;
    return s;
}

static char *to_upper(const char *s, size_t len) {
    char *out = xalloc(len + 1);
    for (size_t i = 0; i < len; ++i) {
        char c = s[i];
        out[i] = (c >= 'a' && c <= 'z') ? (char)(c - 'a' + 'A') : c;
    }
    out[len] = '\0';
    return out;
}

static char *reverse(const char *s, size_t len) {
    char *out = xalloc(len + 1);
    for (size_t i = 0; i < len; ++i) {
        out[i] = s[len - 1 - i];
    }
    out[len] = '\0';
    return out;
}

static char *replace_dash(const char *s, size_t len) {
    char *out = xalloc(len + 1);
    for (size_t i = 0; i < len; ++i) {
        out[i] = (s[i] == '-') ? '_' : s[i];
    }
    out[len] = '\0';
    return out;
}

static uint64_t fnv(uint64_t h, const char *s, size_t len) {
    uint64_t acc = h;
    for (size_t i = 0; i < len; ++i) {
        acc = (acc ^ (uint64_t)(unsigned char)s[i]) * UINT64_C(1099511628211);
    }
    return acc;
}

int main(void) {
    const int64_t rounds = 600000;
    uint64_t hash = UINT64_C(1469598103934665603);

    for (int64_t r = 0; r < rounds; ++r) {
        size_t base_len;
        char *base = concat_build(r, &base_len);
        char *upper = to_upper(base, base_len);
        char *flipped = reverse(upper, base_len);
        char *swapped = replace_dash(flipped, base_len);

        hash = fnv(hash, base, base_len);
        hash = fnv(hash, swapped, base_len);
        hash = hash + (uint64_t)base_len;

        free(base);
        free(upper);
        free(flipped);
        free(swapped);
    }

    printf("%" PRIu64 "\n", hash);
    return 0;
}
