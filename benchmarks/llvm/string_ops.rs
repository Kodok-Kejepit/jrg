fn fnv(h: u64, s: &str) -> u64 {
    let mut acc = h;
    for &b in s.as_bytes() {
        acc = (acc ^ b as u64).wrapping_mul(1099511628211);
    }
    acc
}

fn main() {
    let rounds: i64 = 600000;
    let mut hash: u64 = 1469598103934665603;

    for r in 0..rounds {
        let base: String = format!("item-{}:{}", r % 997, r % 31);
        let upper: String = base.to_uppercase();
        let flipped: String = upper.chars().rev().collect();
        let swapped: String = flipped.replace('-', "_");

        hash = fnv(hash, &base);
        hash = fnv(hash, &swapped);
        hash = hash.wrapping_add(swapped.chars().count() as u64);
    }

    println!("{}", hash);
}
