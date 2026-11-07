/*
 * Benchmark 04 (C++20 / Clang++): Enterprise Server Workload (HTTP + JSON + ACID DB)
 * Identical logic and input parameters as 04_http_json_db_workload.aps
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

struct ServerConfig {
    int64_t port;
    int64_t max_body_bytes;
    int64_t fee_bps;
};

struct DbMetrics {
    int64_t commits;
    int64_t rollbacks;
    int64_t net_volume;
};

[[gnu::noinline]]
int64_t process_http_requests(int64_t count, const ServerConfig& cfg) {
    int64_t status_acc = 0;
    for (int64_t req_id = 0; req_id < count; req_id++) {
        int64_t method_code = (req_id & 3) + 1;
        int64_t body_len = (req_id * 37) & 131071;
        bool auth_ok = (req_id & 15) != 0;

        int64_t* scratch_ptr = new int64_t[4]();
        scratch_ptr[0] = req_id ^ 255;
        int64_t scratch = scratch_ptr[0];
        if (!auth_ok) {
            status_acc += 401;
        } else if (body_len > cfg.max_body_bytes) {
            status_acc += 413;
        } else if (method_code == 1 || method_code == 2) {
            status_acc += 200 + (scratch & 7);
        } else {
            status_acc += 404;
        }
        delete[] scratch_ptr;
    }
    return status_acc;
}

[[gnu::noinline]]
int64_t process_json_documents(int64_t count) {
    int64_t json_acc = 0;
    for (int64_t doc_id = 0; doc_id < count; doc_id++) {
        int64_t* ast_nodes = new int64_t[5]();
        ast_nodes[0] = 1;
        ast_nodes[1] = 100000 + doc_id;
        ast_nodes[2] = 1500 + ((doc_id * 19) & 4095);
        ast_nodes[3] = 100 + ((doc_id * 7) & 63);
        ast_nodes[4] = 2;

        if (ast_nodes[0] == 1 && ast_nodes[4] == 2) {
            int64_t total_fare = (ast_nodes[2] * ast_nodes[3]) / 100;
            json_acc += (ast_nodes[1] & 1023) + total_fare;
        }
        delete[] ast_nodes;
    }
    return json_acc;
}

[[gnu::noinline]]
int64_t execute_db_and_worker_pipeline(
    int64_t tx_count,
    const ServerConfig& cfg,
    DbMetrics& metrics
) {
    int64_t balance_a = 500000000;
    int64_t balance_b = 500000000;
    int64_t worker_checksum = 0;

    for (int64_t tx_id = 0; tx_id < tx_count; tx_id++) {
        int64_t amount = 250 + ((tx_id * 13) & 1023);
        bool force_rollback = (tx_id & 7) == 7;

        int64_t undo_a = balance_a;
        int64_t undo_b = balance_b;

        balance_a -= amount;
        balance_b += amount;

        if (force_rollback || balance_a < 0) {
            balance_a = undo_a;
            balance_b = undo_b;
            metrics.rollbacks += 1;
        } else {
            int64_t fee = (amount * cfg.fee_bps) / 10000;
            int64_t net = amount - fee;
            metrics.commits += 1;
            metrics.net_volume += net;
            worker_checksum += (net ^ (tx_id & 255));
        }
    }

    return worker_checksum + (balance_a / 1000) + (balance_b / 1000);
}

int main() {
    ServerConfig shared_cfg = { 8080, 65536, 800 };
    DbMetrics db_metrics = { 0, 0, 0 };

    int64_t t0 = time_now_ns();
    int64_t http_sum = process_http_requests(100000, shared_cfg);
    int64_t t1 = time_now_ns();
    int64_t json_sum = process_json_documents(100000);
    int64_t t2 = time_now_ns();
    int64_t db_sum = execute_db_and_worker_pipeline(100000, shared_cfg, db_metrics);
    int64_t t3 = time_now_ns();

    int64_t http_us = (t1 - t0) / 1000;
    int64_t json_us = (t2 - t1) / 1000;
    int64_t db_us = (t3 - t2) / 1000;
    int64_t total_us = (t3 - t0) / 1000;
    int64_t checksum = http_sum + json_sum + db_sum + db_metrics.commits + db_metrics.rollbacks + db_metrics.net_volume;

    std::cout << "BENCH_04_SERVER elapsed_us=" << total_us
              << " http_us=" << http_us
              << " json_us=" << json_us
              << " db_async_us=" << db_us
              << " checksum=" << checksum << "\n";
    return 0;
}
