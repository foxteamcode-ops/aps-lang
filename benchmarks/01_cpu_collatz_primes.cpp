/*
 * Benchmark 01 (C++20 / Clang++): CPU-Intensive Workload (Collatz Conjecture & Prime / ALU)
 * Identical algorithm and input range as 01_cpu_collatz_primes.aps
 */
#include <iostream>
#include <cstdint>
#ifdef _WIN32
#include <windows.h>
#else
#include <chrono>
#endif

static int64_t time_now_ns() {
#ifdef _WIN32
    LARGE_INTEGER freq, counter;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&counter);
    return static_cast<int64_t>((counter.QuadPart * 1000000000LL) / freq.QuadPart);
#else
    auto now = std::chrono::steady_clock::now();
    return std::chrono::duration_cast<std::chrono::nanoseconds>(now.time_since_epoch()).count();
#endif
}

[[gnu::noinline]]
int64_t collatz_steps(int64_t start_n) {
    int64_t n = start_n;
    int64_t steps = 0;
    while (n > 1) {
        if ((n % 2) == 0) {
            n = n / 2;
        } else {
            n = 3 * n + 1;
        }
        steps += 1;
    }
    return steps;
}

[[gnu::noinline]]
bool is_prime_fast(int64_t n) {
    if (n < 2) return false;
    if (n == 2 || n == 3) return true;
    if ((n % 2) == 0 || (n % 3) == 0) return false;
    int64_t d = 5;
    while ((d * d) <= n) {
        if ((n % d) == 0 || (n % (d + 2)) == 0) {
            return false;
        }
        d += 6;
    }
    return true;
}

[[gnu::noinline]]
int64_t matrix_poly_kernel(int64_t iterations) {
    int64_t acc0 = 17;
    int64_t acc1 = 31;
    int64_t acc2 = 47;
    int64_t acc3 = 73;
    for (int64_t i = 1; i < iterations; i++) {
        acc0 = static_cast<int64_t>(((static_cast<uint64_t>(acc0) * 1103515245ULL + 12345ULL + static_cast<uint64_t>(i)) & 2147483647ULL));
        acc1 = static_cast<int64_t>((static_cast<uint64_t>(acc1 ^ (acc0 >> 3)) + static_cast<uint64_t>(i) * 7ULL));
        acc2 = static_cast<int64_t>(((static_cast<uint64_t>(acc2) + static_cast<uint64_t>(acc1)) & 1073741823ULL));
        acc3 = acc3 ^ static_cast<int64_t>((static_cast<uint64_t>(acc2) + static_cast<uint64_t>(i) * 13ULL));
    }
    return static_cast<int64_t>(((static_cast<uint64_t>(acc0) + static_cast<uint64_t>(acc1) + static_cast<uint64_t>(acc2) + static_cast<uint64_t>(acc3)) & 2147483647ULL));
}

int main() {
    int64_t t0 = time_now_ns();

    volatile int64_t collatz_limit = 500001;
    int64_t total_collatz_steps = 0;
    for (int64_t n = 1; n < collatz_limit; n++) {
        total_collatz_steps += collatz_steps(n);
    }

    volatile int64_t prime_limit = 200001;
    int64_t prime_count = 0;
    for (int64_t p = 2; p < prime_limit; p++) {
        if (is_prime_fast(p)) {
            prime_count += 1;
        }
    }

    volatile int64_t poly_iters = 5000001;
    int64_t poly_checksum = matrix_poly_kernel(poly_iters);

    int64_t t1 = time_now_ns();
    int64_t elapsed_us = (t1 - t0) / 1000;
    int64_t checksum = total_collatz_steps + prime_count + poly_checksum;

    std::cout << "BENCH_01_CPU elapsed_us=" << elapsed_us
              << " collatz=" << total_collatz_steps
              << " primes=" << prime_count
              << " checksum=" << checksum << "\n";
    return 0;
}
