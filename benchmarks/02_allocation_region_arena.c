/*
 * Benchmark 02 (C / Clang): Allocation-Intensive Workload
 * Phase A: 2,000,000 individual heap allocations (`calloc(1, 32)` + `free`)
 *          across 200 batches of 10,000 objects (standard CRT heap allocator).
 * Phase B: 200,000 `ArenaNode` struct instantiations on the stack.
 */
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#ifdef _WIN32
#include <windows.h>
#else
#include <time.h>
#endif

static int64_t time_now_ns(void) {
#ifdef _WIN32
    LARGE_INTEGER freq, counter;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&counter);
    return (int64_t)((counter.QuadPart * 1000000000LL) / freq.QuadPart);
#else
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)(ts.tv_sec * 1000000000LL + ts.tv_nsec);
#endif
}

typedef struct ArenaNode {
    int64_t id;
    int64_t weight;
    int64_t tag;
    int64_t checksum;
} ArenaNode;

__attribute__((noinline))
int64_t run_standard_heap_batches(int64_t batches, int64_t items_per_batch) {
    int64_t total_acc = 0;
    void** ptr_table = (void**)malloc(sizeof(void*) * (size_t)items_per_batch);
    for (int64_t b = 0; b < batches; b++) {
        int64_t batch_sum = 0;
        for (int64_t i = 0; i < items_per_batch; i++) {
            int64_t raw = ((b * items_per_batch + i) * 1664525LL + 1013904223LL) & 2147483647LL;
            int64_t* blk = (int64_t*)calloc(1, 32);
            blk[0] = raw;
            batch_sum += blk[0] & 65535LL;
            ptr_table[i] = blk;
        }
        for (int64_t i = 0; i < items_per_batch; i++) {
            free(ptr_table[i]);
        }
        total_acc += batch_sum;
    }
    free(ptr_table);
    return total_acc;
}

__attribute__((noinline))
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

int main(void) {
    int64_t t0 = time_now_ns();
    int64_t arena_sum = run_standard_heap_batches(200, 10000);
    int64_t t1 = time_now_ns();
    int64_t struct_sum = run_struct_allocations(200000);
    int64_t t2 = time_now_ns();

    int64_t arena_us = (t1 - t0) / 1000;
    int64_t struct_us = (t2 - t1) / 1000;
    int64_t total_us = (t2 - t0) / 1000;
    int64_t checksum = arena_sum + struct_sum;

    printf("BENCH_02_ALLOC elapsed_us=%lld arena_2M_us=%lld struct_200k_us=%lld checksum=%lld\n",
           (long long)total_us,
           (long long)arena_us,
           (long long)struct_us,
           (long long)checksum);
    return 0;
}
