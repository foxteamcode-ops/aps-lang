// ============================================================================
// Benchmark 02 (Rust): Allocation-Intensive Workload
// Phase A: 2,000,000 individual heap allocations (`Box<[i64; 4]>`, 32 bytes each)
//          across 200 batches of 10,000 objects (standard system allocator).
// Phase B: 200,000 `ArenaNode` struct instantiations on the stack.
// ============================================================================

use std::hint::black_box;
use std::time::Instant;

#[repr(C)]
struct ArenaNode {
    id: i64,
    weight: i64,
    tag: i64,
    checksum: i64,
}

#[inline(never)]
fn run_standard_heap_batches(batches: i64, items_per_batch: i64) -> i64 {
    let mut total_acc: i64 = 0;
    for b in 0..batches {
        let mut batch_sum: i64 = 0;
        let mut batch_ptrs: Vec<Box<[i64; 4]>> = Vec::with_capacity(items_per_batch as usize);
        for i in 0..items_per_batch {
            let raw = ((b * items_per_batch + i) * 1664525 + 1013904223) & 2147483647;
            let blk = Box::new([raw, i, b, raw & 65535]);
            batch_sum += blk[0] & 65535;
            batch_ptrs.push(black_box(blk));
        }
        drop(batch_ptrs);
        total_acc += batch_sum;
    }
    total_acc
}

#[inline(never)]
fn run_struct_allocations(count: i64) -> i64 {
    let mut struct_acc: i64 = 0;
    for i in 0..count {
        let node = black_box(ArenaNode {
            id: i,
            weight: (i * 7 + 3) & 65535,
            tag: (i ^ 12345) & 65535,
            checksum: (i * 13) & 65535,
        });
        struct_acc += node.weight + node.tag + node.checksum;
    }
    struct_acc
}

fn main() {
    let t0 = Instant::now();
    let arena_sum = run_standard_heap_batches(black_box(200), black_box(10_000));
    let t1 = Instant::now();
    let struct_sum = run_struct_allocations(black_box(200_000));
    let t2 = Instant::now();

    let arena_us = (t1 - t0).as_micros() as i64;
    let struct_us = (t2 - t1).as_micros() as i64;
    let total_us = (t2 - t0).as_micros() as i64;
    let checksum = arena_sum + struct_sum;

    println!(
        "BENCH_02_ALLOC elapsed_us={} arena_2M_us={} struct_200k_us={} checksum={}",
        total_us, arena_us, struct_us, checksum
    );
}
