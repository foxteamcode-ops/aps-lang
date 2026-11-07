#include <iostream>
#include <chrono>
#include <cstdint>

#ifdef _WIN32
#include <windows.h>
static int64_t get_time_ns() {
    LARGE_INTEGER freq, counter;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&counter);
    return static_cast<int64_t>((counter.QuadPart * 1000000000LL) / freq.QuadPart);
}
#else
static int64_t get_time_ns() {
    auto now = std::chrono::steady_clock::now().time_since_epoch();
    return std::chrono::duration_cast<std::chrono::nanoseconds>(now).count();
}
#endif

struct BankLedger {
    int64_t total_transactions = 0;
    int64_t settled_count = 0;
    int64_t rejected_count = 0;
    int64_t total_fees_collected = 0;
    int64_t total_volume_kzt = 0;
};

static int64_t process_transaction(int64_t tx_id, BankLedger& ledger) {
    int64_t amount = (tx_id * 104729) % 50000 + 100;
    constexpr int64_t fee_bps = 15;
    int64_t fee = (amount * fee_bps) / 10000;

    ledger.total_transactions++;

    if (amount > 49000) {
        ledger.rejected_count++;
        return 0;
    } else {
        int64_t net_payout = amount - fee;
        ledger.settled_count++;
        ledger.total_fees_collected += fee;
        ledger.total_volume_kzt += net_payout;
        return net_payout;
    }
}

int main() {
    std::cout << "==================================================================\n";
    std::cout << "      C++20 (CLANG++ -O3) FINTECH HIGHLOAD: 1,000,000 TRANSACTIONS\n";
    std::cout << "==================================================================\n";

    constexpr int64_t total_tx = 1000000;
    BankLedger ledger{};

    int64_t start_ns = get_time_ns();

    for (int64_t i = 0; i < total_tx; i++) {
        process_transaction(i, ledger);
    }

    int64_t end_ns = get_time_ns();
    int64_t elapsed_ns = end_ns - start_ns;
    int64_t elapsed_ms = elapsed_ns / 1000000;
    int64_t latency_ns_per_tx = total_tx > 0 ? (elapsed_ns / total_tx) : 0;
    int64_t tps = elapsed_ms > 0 ? ((total_tx * 1000) / elapsed_ms) : 99999999;

    std::cout << "==================================================================\n";
    std::cout << "              ИТОГОВЫЙ ОТЧЁТ ТРАНЗАКЦИОННОЙ НАГРУЗКИ (C++20)      \n";
    std::cout << "==================================================================\n";
    std::cout << "  Всего обработано транзакций : " << ledger.total_transactions << "\n";
    std::cout << "  Успешно проведено (Settled)  : " << ledger.settled_count << "\n";
    std::cout << "  Отклонено антифродом         : " << ledger.rejected_count << "\n";
    std::cout << "  Общее время выполнения (мс)  : " << elapsed_ms << "\n";
    std::cout << "  Средняя задержка на 1 tx     : " << latency_ns_per_tx << " нс (наносекунд!)\n";
    std::cout << "  ПРОПУСКНАЯ СПОСОБНОСТЬ (TPS) : " << tps << " транзакций / сек!\n";
    std::cout << "  Суммарная комиссия (KZT)     : " << ledger.total_fees_collected << "\n";
    std::cout << "  Контрольная сумма объёма KZT : " << ledger.total_volume_kzt << "\n";
    std::cout << "==================================================================\n";
    return 0;
}
