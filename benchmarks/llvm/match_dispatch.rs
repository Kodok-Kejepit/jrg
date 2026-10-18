enum Op {
    Push(i64),
    AddK(i64),
    MulK(i64),
    Xor,
    Rot,
    Range(i64, i64),
}

fn classify(v: i64) -> i64 {
    match v % 11 {
        0 => 5,
        1 | 2 | 3 => 7,
        4..=8 => 11,
        _ => 13,
    }
}

fn exec(o: Op, acc: i64) -> i64 {
    match o {
        Op::Push(k) => k,
        Op::AddK(k) => acc.wrapping_add(k),
        Op::MulK(k) => acc.wrapping_mul(k) % 1000003,
        Op::Xor => acc ^ 1431655765,
        Op::Rot => ((acc << 7) | (acc >> 25)) % 1000003,
        Op::Range(lo, hi) => {
            let mut out = acc;
            if acc < lo {
                out = lo;
            }
            if acc > hi {
                out = hi;
            }
            out
        }
    }
}

fn main() {
    let iterations: i64 = 45000000;
    let mut acc: i64 = 1;
    let mut total: i64 = 0;

    for i in 0..iterations {
        let sel = i % 6;
        let op = if sel == 0 {
            Op::Push(i % 997)
        } else if sel == 1 {
            Op::AddK(i % 31)
        } else if sel == 2 {
            Op::MulK(3 + i % 7)
        } else if sel == 3 {
            Op::Xor
        } else if sel == 4 {
            Op::Rot
        } else {
            Op::Range(10, 900000)
        };

        acc = exec(op, acc);
        total = (total + classify(acc)) % 1000000007;
    }

    println!("{} {}", acc, total);
}
