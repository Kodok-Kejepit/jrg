struct Record {
    id: i64,
    amount: f64,
    weight: i64,
    tier: i64,
}

impl Record {
    fn score(&self) -> f64 {
        self.amount * (1.0 + self.weight as f64 * 0.01)
    }
}

fn tier_of(weight: i64) -> i64 {
    match weight % 7 {
        0 => 0,
        1 | 2 => 1,
        3..=5 => 2,
        _ => 3,
    }
}

fn fnv(h: u64, s: &str) -> u64 {
    let mut acc = h;
    for &b in s.as_bytes() {
        acc = (acc ^ b as u64).wrapping_mul(1099511628211);
    }
    acc
}

fn main() {
    let records: i64 = 12000000;
    let buckets: i64 = 64;

    let mut counts = [0i64; 64];
    let mut sums = [0.0f64; 64];

    let mut seed: u64 = 88172645463325252;
    let mut hash: u64 = 1469598103934665603;
    let mut tier_total: i64 = 0;

    for r in 0..records {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let w = ((seed >> 33) % 1000) as i64;
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let amt = ((seed >> 33) % 100000) as f64 * 0.01;

        let rec = Record {
            id: r,
            amount: amt,
            weight: w,
            tier: tier_of(w),
        };

        let b = (rec.id % buckets) as usize;
        counts[b] += 1;
        sums[b] += rec.score();
        tier_total += rec.tier;

        if r % 16 == 0 {
            let label = format!("rec{}/t{}", rec.id % 1000, rec.tier);
            hash = fnv(hash, &label);
        }
    }

    let mut total_count: i64 = 0;
    let mut total_sum: f64 = 0.0;
    for b in 0..buckets as usize {
        total_count += counts[b];
        total_sum += sums[b];
        hash = (hash ^ counts[b] as u64).wrapping_mul(1099511628211);
    }

    println!("{} {} {}", total_count, tier_total, hash);
    println!("{}", total_sum);
}
