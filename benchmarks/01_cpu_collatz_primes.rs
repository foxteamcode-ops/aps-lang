// ============================================================================
// Benchmark 01 (Rust): CPU-Intensive Workload (Collatz Conjecture & Prime / ALU)
// Identical algorithm and input range as 01_cpu_collatz_primes.aps
// ============================================================================

use std::hint::black_box;
use std::time::Instant;

#[inline(never)]
fn collatz_steps(start_n: i64) -> i64 {
    let mut n: i64 = start_n;
    let mut steps: i64 = 0;
    while n > 1 {
        if (n % 2) == 0 {
            n /= 2;
        } else {
            n = 3 * n + 1;
        }
        steps += 1;
    }
    steps
}

#[inline(never)]
fn is_prime_fast(n: i64) -> bool {
    if n < 2 {
        return false;
    }
    if n == 2 || n == 3 {
        return true;
    }
    if (n % 2) == 0 || (n % 3) == 0 {
        return false;
    }
    let mut d: i64 = 5;
    while (d * d) <= n {
        if (n % d) == 0 || (n % (d + 2)) == 0 {
            return false;
        }
        d += 6;
    }
    true
}

#[inline(never)]
fn matrix_poly_kernel(iterations: i64) -> i64 {
    let mut acc0: i64 = 17;
    let mut acc1: i64 = 31;
    let mut acc2: i64 = 47;
    let mut acc3: i64 = 73;
    for i in 1..iterations {
        acc0 = (acc0.wrapping_mul(1103515245).wrapping_add(12345).wrapping_add(i)) & 2147483647;
        acc1 = (acc1 ^ (acc0 >> 3)).wrapping_add(i.wrapping_mul(7));
        acc2 = acc2.wrapping_add(acc1) & 1073741823;
        acc3 ^= acc2.wrapping_add(i.wrapping_mul(13));
    }
    acc0.wrapping_add(acc1).wrapping_add(acc2).wrapping_add(acc3) & 2147483647
}

fn main() {
    let t0 = Instant::now();

    let mut total_collatz_steps: i64 = 0;
    for n in 1..black_box(500_001) {
        total_collatz_steps += collatz_steps(n);
    }

    let mut prime_count: i64 = 0;
    for p in 2..black_box(200_001) {
        if is_prime_fast(p) {
            prime_count += 1;
        }
    }

    let poly_checksum = matrix_poly_kernel(black_box(5_000_001));

    let elapsed_us = t0.elapsed().as_micros() as i64;
    let checksum = total_collatz_steps + prime_count + poly_checksum;
    println!(
        "BENCH_01_CPU elapsed_us={} collatz={} primes={} checksum={}",
        elapsed_us, total_collatz_steps, prime_count, checksum
    );
}
