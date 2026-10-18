#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

/* Shortest representation that still round-trips back to the exact same
   double — what Rust/Go/JRG print by default; C has no such specifier. */
static void print_shortest(double v) {
    char buf[64];
    for (int prec = 1; prec <= 17; ++prec) {
        snprintf(buf, sizeof buf, "%.*g", prec, v);
        if (strtod(buf, NULL) == v) break;
    }
    printf("%s\n", buf);
}

typedef struct {
    double x;
    double y;
    double vx;
    double vy;
    double mass;
} Particle;

static Particle advance(Particle self, double dt) {
    Particle out;
    out.x = self.x + self.vx * dt;
    out.y = self.y + self.vy * dt;
    out.vx = self.vx - self.x * dt * 0.001;
    out.vy = self.vy - self.y * dt * 0.001;
    out.mass = self.mass;
    return out;
}

static double energy(Particle self) {
    return 0.5 * self.mass * (self.vx * self.vx + self.vy * self.vy);
}

int main(void) {
    const int64_t count = 50000;
    const int64_t steps = 3000;
    const double dt = 0.01;

    double total = 0.0;
    uint64_t seed = UINT64_C(88172645463325252);

    for (int64_t p = 0; p < count; ++p) {
        seed = seed * UINT64_C(6364136223846793005) + UINT64_C(1442695040888963407);
        double sx = (double)((seed >> 33) % 1000) * 0.001;
        seed = seed * UINT64_C(6364136223846793005) + UINT64_C(1442695040888963407);
        double sy = (double)((seed >> 33) % 1000) * 0.001;

        Particle part;
        part.x = sx;
        part.y = sy;
        part.vx = 1.0 - sy;
        part.vy = 1.0 - sx;
        part.mass = 1.0 + sx;

        for (int64_t s = 0; s < steps; ++s) {
            part = advance(part, dt);
        }
        total = total + energy(part) + part.x - part.y;
    }

    print_shortest(total);
    return 0;
}
