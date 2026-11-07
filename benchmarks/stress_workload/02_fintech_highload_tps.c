#include <stdio.h>
#include <stdint.h>

#ifdef _WIN32
#include <windows.h>
static int64_t get_time_ns(void) {
    LARGE_INTEGER freq, counter;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&counter);
    return (int64_t)((counter.QuadPart * 1000000000LL) / freq.QuadPart);
}
#else
#include <time.h>
static int64_t get_time_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)(ts.tv_sec * 1000000000LL + ts.tv_nsec);
}
#endif

typedef struct BankLedger {
    int64_t total_transactions;
    int64_t settled_count;
    int64_t rejected_count;
    int64_t total_fees_collected;
    int64_t total_volume_kzt;
} BankLedger;

static int64_t process_transaction(int64_t tx_id, BankLedger* ledger) {
    int64_t amount = (tx_id * 104729) % 50000 + 100;
    int64_t fee_bps = 15;
    int64_t fee = (amount * fee_bps) / 10000;

    ledger->total_transactions++;

    if (amount > 49000) {
        ledger->rejected_count++;
        return 0;
    } else {
        int64_t net_payout = amount - fee;
        ledger->settled_count++;
        ledger->total_fees_collected += fee;
        ledger->total_volume_kzt += net_payout;
        return net_payout;
    }
}

int main(void) {
    printf("==================================================================\n");
    printf("      C (CLANG -O3) FINTECH HIGHLOAD: 1,000,000 TRANSACTIONS      \n");
    printf("==================================================================\n");

    int64_t total_tx = 1000000;
    BankLedger ledger = {0, 0, 0, 0, 0};

    int64_t start_ns = get_time_ns();

    for (int64_t i = 0; i < total_tx; i++) {
        process_transaction(i, &ledger);
    }

    int64_t end_ns = get_time_ns();
    int64_t elapsed_ns = end_ns - start_ns;
    int64_t elapsed_ms = elapsed_ns / 1000000;
    int64_t latency_ns_per_tx = total_tx > 0 ? (elapsed_ns / total_tx) : 0;
    int64_t tps = elapsed_ms > 0 ? ((total_tx * 1000) / elapsed_ms) : 99999999;

    printf("==================================================================\n");
    printf("              ИТОГОВЫЙ ОТЧЁТ ТРАНЗАКЦИОННОЙ НАГРУЗКИ (C CLANG)    \n");
    printf("==================================================================\n");
    printf("  Всего обработано транзакций : %lld\n", (long long)ledger.total_transactions);
    printf("  Успешно проведено (Settled)  : %lld\n", (long long)ledger.settled_count);
    printf("  Отклонено антифродом         : %lld\n", (long long)ledger.rejected_count);
    printf("  Общее время выполнения (мс)  : %lld\n", (long long)elapsed_ms);
    printf("  Средняя задержка на 1 tx     : %lld нс (наносекунд!)\n", (long long)latency_ns_per_tx);
    printf("  ПРОПУСКНАЯ СПОСОБНОСТЬ (TPS) : %lld транзакций / сек!\n", (long long)tps);
    printf("  Суммарная комиссия (KZT)     : %lld\n", (long long)ledger.total_fees_collected);
    printf("  Контрольная сумма объёма KZT : %lld\n", (long long)ledger.total_volume_kzt);
    printf("==================================================================\n");
    return 0;
}
