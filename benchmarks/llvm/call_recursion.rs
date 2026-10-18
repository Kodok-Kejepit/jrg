fn fib(n: i64) -> i64 {
    if n < 2 {
        return n;
    }
    fib(n - 1) + fib(n - 2)
}

fn ack_lite(m: i64, n: i64) -> i64 {
    if m == 0 {
        return n + 1;
    }
    if n == 0 {
        return ack_lite(m - 1, 1);
    }
    ack_lite(m - 1, ack_lite(m, n - 1))
}

fn step_a(v: i64) -> i64 {
    v.wrapping_mul(3).wrapping_add(1)
}

fn step_b(v: i64) -> i64 {
    step_a(v) ^ (v >> 2)
}

fn step_c(v: i64) -> i64 {
    step_b(v).wrapping_add(step_a(v >> 1))
}

fn chain(v: i64) -> i64 {
    step_c(step_b(step_a(v)))
}

fn main() {
    let iterations: i64 = 20000000;
    let mut acc: i64 = 0;

    for i in 0..iterations {
        acc = (acc.wrapping_add(chain(i))) % 1000000007;
    }

    acc += fib(39);
    acc += ack_lite(2, 8);

    println!("{}", acc);
}
