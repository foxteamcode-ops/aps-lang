// ============================================================================
// Benchmark 03 (Rust): Memory, Collections, Algorithms & String Processing
// Identical algorithm and input parameters as 03_memory_and_collections.aps
// ============================================================================

use std::hint::black_box;
use std::time::Instant;

#[inline(never)]
fn run_collections_and_memory(iterations: i64) -> i64 {
    let mut vec_keys: Vec<i64> = Vec::with_capacity(16);
    for k in 0..16 {
        vec_keys.push((k * 2654435761) & 1048575);
    }

    let mut buckets: Vec<i64> = vec![
        101, 203, 307, 409, 503, 607, 701, 809,
        907, 1009, 1103, 1201, 1301, 1409, 1511, 1601,
        1709, 1801, 1901, 2003, 2111, 2203, 2309, 2411,
        2503, 2609, 2707, 2801, 2903, 3001, 3109, 3203,
    ];

    let mut acc: i64 = 0;
    for i in 0..iterations {
        let key_idx = (i & 15) as usize;
        let key_val = vec_keys[key_idx];
        let slot = ((key_val + i) & 31) as usize;
        let prev = buckets[slot];
        let updated = ((prev * 33) + key_val + (i & 255)) & 16777215;
        buckets[slot] = updated;
        acc += updated & 4095;
    }

    for i in 1..32usize {
        let cur = buckets[i];
        let mut j: isize = (i as isize) - 1;
        while j >= 0 && buckets[j as usize] > cur {
            buckets[(j + 1) as usize] = buckets[j as usize];
            j -= 1;
        }
        buckets[(j + 1) as usize] = cur;
    }

    let mut found_hits: i64 = 0;
    for q in 0..black_box(100_000i64) {
        let target = buckets[(q & 31) as usize];
        let mut lo: isize = 0;
        let mut hi: isize = 31;
        while lo <= hi {
            let mid = (lo + hi) / 2;
            let mid_val = buckets[mid as usize];
            if mid_val == target {
                found_hits += 1;
                break;
            } else if mid_val < target {
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        }
    }

    acc + found_hits + buckets[0] + buckets[31]
}

#[inline(never)]
fn run_string_workload(scan_iters: i64, concat_iters: i64) -> i64 {
    let hdr_get = black_box("GET /api/v1/orders/dispatch HTTP/1.1");
    let hdr_post = black_box("POST /api/v1/settlement/commit HTTP/1.1");
    let hdr_probe = black_box("GET /api/v1/orders/dispatch HTTP/1.1");

    let mut str_acc: i64 = 0;
    for i in 0..scan_iters {
        let n1 = hdr_get.len() as i64;
        let n2 = hdr_post.len() as i64;
        if (i & 1) == 0 {
            if hdr_get == hdr_probe {
                str_acc += n1;
            }
        } else if hdr_get != hdr_post {
            str_acc += n2;
        }
    }

    let prefix = black_box("aps_tx_");
    let suffix = black_box("_ok");
    for c in 0..concat_iters {
        let mut combined = String::with_capacity(prefix.len() + suffix.len());
        combined.push_str(prefix);
        combined.push_str(suffix);
        let combined = black_box(combined);
        str_acc += (combined.len() as i64) + (c & 3);
    }

    str_acc
}

fn main() {
    let t0 = Instant::now();
    let coll_sum = run_collections_and_memory(black_box(2_000_000));
    let t1 = Instant::now();
    let str_sum = run_string_workload(black_box(500_000), black_box(20_000));
    let t2 = Instant::now();

    let coll_us = (t1 - t0).as_micros() as i64;
    let str_us = (t2 - t1).as_micros() as i64;
    let total_us = (t2 - t0).as_micros() as i64;
    let checksum = coll_sum + str_sum;

    println!(
        "BENCH_03_MEM_COLL elapsed_us={} collections_us={} strings_us={} checksum={}",
        total_us, coll_us, str_us, checksum
    );
}
