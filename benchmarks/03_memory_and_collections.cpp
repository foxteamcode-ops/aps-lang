/*
 * Benchmark 03 (C++20 / Clang++): Memory, Collections, Algorithms & String Processing
 * Identical algorithm and input parameters as 03_memory_and_collections.aps
 */
#include <iostream>
#include <string>
#include <cstring>
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
int64_t run_collections_and_memory(int64_t iterations) {
    int64_t vec_keys[16];
    for (int64_t k = 0; k < 16; k++) {
        vec_keys[k] = (k * 2654435761LL) & 1048575LL;
    }

    int64_t buckets[32] = {
        101, 203, 307, 409, 503, 607, 701, 809,
        907, 1009, 1103, 1201, 1301, 1409, 1511, 1601,
        1709, 1801, 1901, 2003, 2111, 2203, 2309, 2411,
        2503, 2609, 2707, 2801, 2903, 3001, 3109, 3203
    };

    int64_t acc = 0;
    for (int64_t i = 0; i < iterations; i++) {
        int64_t key_idx = i & 15;
        int64_t key_val = vec_keys[key_idx];
        int64_t slot = (key_val + i) & 31;
        int64_t prev = buckets[slot];
        int64_t updated = ((prev * 33) + key_val + (i & 255)) & 16777215;
        buckets[slot] = updated;
        acc += updated & 4095;
    }

    for (int64_t i = 1; i < 32; i++) {
        int64_t cur = buckets[i];
        int64_t j = i - 1;
        while (j >= 0 && buckets[j] > cur) {
            buckets[j + 1] = buckets[j];
            j -= 1;
        }
        buckets[j + 1] = cur;
    }

    volatile int64_t search_queries = 100000;
    int64_t found_hits = 0;
    for (int64_t q = 0; q < search_queries; q++) {
        int64_t target = buckets[q & 31];
        int64_t lo = 0;
        int64_t hi = 31;
        while (lo <= hi) {
            int64_t mid = (lo + hi) / 2;
            int64_t mid_val = buckets[mid];
            if (mid_val == target) {
                found_hits += 1;
                break;
            } else if (mid_val < target) {
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        }
    }

    return acc + found_hits + buckets[0] + buckets[31];
}

[[gnu::noinline]]
int64_t run_string_workload(int64_t scan_iters, int64_t concat_iters) {
    const std::string hdr_get = "GET /api/v1/orders/dispatch HTTP/1.1";
    const std::string hdr_post = "POST /api/v1/settlement/commit HTTP/1.1";
    const std::string hdr_probe = "GET /api/v1/orders/dispatch HTTP/1.1";

    int64_t str_acc = 0;
    for (int64_t i = 0; i < scan_iters; i++) {
        int64_t n1 = static_cast<int64_t>(hdr_get.length());
        int64_t n2 = static_cast<int64_t>(hdr_post.length());
        if ((i & 1) == 0) {
            if (hdr_get == hdr_probe) {
                str_acc += n1;
            }
        } else {
            if (hdr_get != hdr_post) {
                str_acc += n2;
            }
        }
    }

    const std::string prefix = "aps_tx_";
    const std::string suffix = "_ok";
    for (int64_t c = 0; c < concat_iters; c++) {
        std::string out = prefix + suffix;
        str_acc += static_cast<int64_t>(out.length()) + (c & 3);
    }

    return str_acc;
}

int main() {
    int64_t t0 = time_now_ns();
    int64_t coll_sum = run_collections_and_memory(2000000);
    int64_t t1 = time_now_ns();
    int64_t str_sum = run_string_workload(500000, 20000);
    int64_t t2 = time_now_ns();

    int64_t coll_us = (t1 - t0) / 1000;
    int64_t str_us = (t2 - t1) / 1000;
    int64_t total_us = (t2 - t0) / 1000;
    int64_t checksum = coll_sum + str_sum;

    std::cout << "BENCH_03_MEM_COLL elapsed_us=" << total_us
              << " collections_us=" << coll_us
              << " strings_us=" << str_us
              << " checksum=" << checksum << "\n";
    return 0;
}
