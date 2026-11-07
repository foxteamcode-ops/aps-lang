// ============================================================================
// Benchmark 04 (Rust): Enterprise Server Workload (HTTP + JSON + ACID DB)
// Identical logic and input parameters as 04_http_json_db_workload.aps
// Uses standard heap (`Box::new`) for per-request & per-JSON AST allocations
// vs APS's `region request_arena` / `region json_arena`.
// ============================================================================

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

struct ServerConfig {
    port: i64,
    max_body_bytes: i64,
    fee_bps: i64,
}

struct DbMetrics {
    commits: i64,
    rollbacks: i64,
    net_volume: i64,
}

#[inline(never)]
fn process_http_requests(count: i64, cfg: &Arc<ServerConfig>) -> i64 {
    let mut status_acc: i64 = 0;
    for req_id in 0..count {
        let method_code = (req_id & 3) + 1;
        let body_len = (req_id * 37) & 131071;
        let auth_ok = (req_id & 15) != 0;

        let scratch_box = black_box(Box::new([req_id ^ 255, req_id, method_code, body_len]));
        let scratch = scratch_box[0];
        if !auth_ok {
            status_acc += 401;
        } else if body_len > cfg.max_body_bytes {
            status_acc += 413;
        } else if method_code == 1 || method_code == 2 {
            status_acc += 200 + (scratch & 7);
        } else {
            status_acc += 404;
        }
    }
    status_acc
}

#[inline(never)]
fn process_json_documents(count: i64) -> i64 {
    let mut json_acc: i64 = 0;
    for doc_id in 0..count {
        let ast_nodes = black_box(vec![
            1i64,
            100000 + doc_id,
            1500 + ((doc_id * 19) & 4095),
            100 + ((doc_id * 7) & 63),
            2i64,
        ]);
        if ast_nodes[0] == 1 && ast_nodes[4] == 2 {
            let total_fare = (ast_nodes[2] * ast_nodes[3]) / 100;
            json_acc += (ast_nodes[1] & 1023) + total_fare;
        }
    }
    json_acc
}

#[inline(never)]
fn execute_db_and_worker_pipeline(
    tx_count: i64,
    cfg: &Arc<ServerConfig>,
    metrics: &mut DbMetrics,
) -> i64 {
    let mut balance_a: i64 = 500_000_000;
    let mut balance_b: i64 = 500_000_000;
    let mut worker_checksum: i64 = 0;

    for tx_id in 0..tx_count {
        let amount = 250 + ((tx_id * 13) & 1023);
        let force_rollback = (tx_id & 7) == 7;

        let undo_a = balance_a;
        let undo_b = balance_b;

        balance_a -= amount;
        balance_b += amount;

        if force_rollback || balance_a < 0 {
            balance_a = undo_a;
            balance_b = undo_b;
            metrics.rollbacks += 1;
        } else {
            let fee = (amount * cfg.fee_bps) / 10000;
            let net = amount - fee;
            metrics.commits += 1;
            metrics.net_volume += net;
            worker_checksum += net ^ (tx_id & 255);
        }
    }

    worker_checksum + (balance_a / 1000) + (balance_b / 1000)
}

fn main() {
    let shared_cfg = Arc::new(ServerConfig {
        port: 8080,
        max_body_bytes: 65536,
        fee_bps: 800,
    });
    let _ = shared_cfg.port;

    let mut db_metrics = DbMetrics {
        commits: 0,
        rollbacks: 0,
        net_volume: 0,
    };

    let t0 = Instant::now();
    let http_sum = process_http_requests(black_box(100_000), &shared_cfg);
    let t1 = Instant::now();
    let json_sum = process_json_documents(black_box(100_000));
    let t2 = Instant::now();
    let db_sum = execute_db_and_worker_pipeline(black_box(100_000), &shared_cfg, &mut db_metrics);
    let t3 = Instant::now();

    let http_us = (t1 - t0).as_micros() as i64;
    let json_us = (t2 - t1).as_micros() as i64;
    let db_us = (t3 - t2).as_micros() as i64;
    let total_us = (t3 - t0).as_micros() as i64;
    let checksum = http_sum
        + json_sum
        + db_sum
        + db_metrics.commits
        + db_metrics.rollbacks
        + db_metrics.net_volume;

    println!(
        "BENCH_04_SERVER elapsed_us={} http_us={} json_us={} db_async_us={} checksum={}",
        total_us, http_us, json_us, db_us, checksum
    );
}
