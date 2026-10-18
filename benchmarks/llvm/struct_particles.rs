#[derive(Clone, Copy)]
struct Particle {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    mass: f64,
}

impl Particle {
    fn advance(self, dt: f64) -> Particle {
        Particle {
            x: self.x + self.vx * dt,
            y: self.y + self.vy * dt,
            vx: self.vx - self.x * dt * 0.001,
            vy: self.vy - self.y * dt * 0.001,
            mass: self.mass,
        }
    }

    fn energy(self) -> f64 {
        0.5 * self.mass * (self.vx * self.vx + self.vy * self.vy)
    }
}

fn main() {
    let count: i64 = 50000;
    let steps: i64 = 3000;
    let dt: f64 = 0.01;

    let mut total: f64 = 0.0;
    let mut seed: u64 = 88172645463325252;

    for _ in 0..count {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let sx = ((seed >> 33) % 1000) as f64 * 0.001;
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let sy = ((seed >> 33) % 1000) as f64 * 0.001;

        let mut part = Particle {
            x: sx,
            y: sy,
            vx: 1.0 - sy,
            vy: 1.0 - sx,
            mass: 1.0 + sx,
        };

        for _ in 0..steps {
            part = part.advance(dt);
        }
        total = total + part.energy() + part.x - part.y;
    }

    println!("{}", total);
}
