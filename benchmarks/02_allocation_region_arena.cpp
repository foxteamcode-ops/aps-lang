/*
 * Benchmark 02 (C++20 / Clang++): Allocation-Intensive Workload
 * Phase A: 2,000,000 individual heap allocations + frees across 200 batches of 10,000 objects.
 * Phase B: 200,000 ArenaNode struct instantiations.
 */
#include <iostream>
#include <vector>
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

struct ArenaNode {
    int64_t id;
    int64_t weight;
    int64_t tag;
    int64_t checksum;
};

[[gnu::noinline]]
int64_t run_standard_heap_batches(int64_t batches, int64_t items_per_batch) {
    int64_t total_acc = 0;
    std::vector<int64_t*> ptr_table(items_per_batch);
    for (int64_t b = 0; b < batches; b++) {
        int64_t batch_sum = 0;
        for (int64_t i = 0; i < items_per_batch; i++) {
            int64_t raw = ((b * items_per_batch + i) * 1664525LL + 1013904223LL) & 2147483647LL;
            int64_t* blk = new int64_t[4]();
            blk[0] = raw;
            batch_sum += blk[0] & 65535LL;
            ptr_table[i] = blk;
        }
        for (int64_t i = 0; i < items_per_batch; i++) {
            delete[] ptr_table[i];
        }
        total_acc += batch_sum;
    }
    return total_acc;
}

[[gnu::noinline]]
int64_t run_struct_allocations(int64_t count) {
    int64_t struct_acc = 0;
    for (int64_t i = 0; i < count; i++) {
        volatile ArenaNode node;
        node.id = i;
        node.weight = (i * 7 + 3) & 65535;
        node.tag = (i ^ 12345) & 65535;
        node.checksum = (i * 13) & 65535;
        struct_acc += node.weight + node.tag + node.checksum;
    }
    return struct_acc;
}

int main() {
    int64_t t0 = time_now_ns();
    int64_t arena_sum = run_standard_heap_batches(200, 10000);
    int64_t t1 = time_now_ns();
    int64_t struct_sum = run_struct_allocations(200000);
    int64_t t2 = time_now_ns();

    int64_t arena_us = (t1 - t0) / 1000;
    int64_t struct_us = (t2 - t1) / 1000;
    int64_t total_us = (t2 - t0) / 1000;
    int64_t checksum = arena_sum + struct_sum;

    std::cout << "BENCH_02_ALLOC elapsed_us=" << total_us
              << " arena_2M_us=" << arena_us
              << " struct_200k_us=" << struct_us
              << " checksum=" << checksum << "\n";
    return 0;
}
