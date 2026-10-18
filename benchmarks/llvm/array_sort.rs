const N: usize = 2000;
const ROUNDS: i64 = 3000;

fn main() {
    let last: i64 = N as i64 - 1;
    let mut hash: u64 = 1469598103934665603;
    let mut seed: u64 = 88172645463325252;
    let mut any_failed = false;
    let mut data = [0i64; N];

    for _ in 0..ROUNDS {
        for i in 0..N {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            data[i] = (seed >> 1) as i64;
        }

        // ---- build heap ----
        let mut start: i64 = (N as i64 - 2) / 2;
        while start >= 0 {
            let mut root = start;
            loop {
                let mut child = root * 2 + 1;
                if child > last {
                    break;
                }
                if child + 1 <= last && data[child as usize] < data[child as usize + 1] {
                    child += 1;
                }
                if data[root as usize] < data[child as usize] {
                    data.swap(root as usize, child as usize);
                    root = child;
                } else {
                    break;
                }
            }
            start -= 1;
        }

        // ---- drain heap ----
        let mut end: i64 = last;
        while end > 0 {
            data.swap(0, end as usize);
            end -= 1;

            let mut root: i64 = 0;
            loop {
                let mut child = root * 2 + 1;
                if child > end {
                    break;
                }
                if child + 1 <= end && data[child as usize] < data[child as usize + 1] {
                    child += 1;
                }
                if data[root as usize] < data[child as usize] {
                    data.swap(root as usize, child as usize);
                    root = child;
                } else {
                    break;
                }
            }
        }

        let mut prev = data[0];
        for i in 0..N {
            if data[i] < prev {
                any_failed = true;
            }
            prev = data[i];
            hash = (hash ^ data[i] as u64).wrapping_mul(11400714819323198485);
        }
    }

    if any_failed {
        println!("SORT_FAILED");
    } else {
        println!("{}", hash);
    }
}
