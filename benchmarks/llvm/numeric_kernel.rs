fn kernel(iterations: i64) -> u64 {
    let mut x = 305_419_896_u64;
    for _ in 0..iterations {
        x = x
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223)
            ^ (x >> 13);
    }
    x
}

fn main() {
    println!("{}", kernel(200_000_000));
}
