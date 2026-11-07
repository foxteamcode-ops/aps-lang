use std::time::Instant;

struct BankLedger {
    total_transactions: i64,
    settled_count: i64,
    rejected_count: i64,
    total_fees_collected: i64,
    total_volume_kzt: i64,
}

fn process_transaction(tx_id: i64, ledger: &mut BankLedger) -> i64 {
    let amount = (tx_id * 104729) % 50000 + 100;
    let fee_bps: i64 = 15;
    let fee = (amount * fee_bps) / 10000;

    ledger.total_transactions += 1;

    if amount > 49000 {
        ledger.rejected_count += 1;
        0
    } else {
        let net_payout = amount - fee;
        ledger.settled_count += 1;
        ledger.total_fees_collected += fee;
        ledger.total_volume_kzt += net_payout;
        net_payout
    }
}

fn main() {
    println!("==================================================================");
    println!("      RUST FINTECH HIGHLOAD ENGINE: 1,000,000 TRANSACTIONS        ");
    println!("==================================================================");

    let total_tx: i64 = 1000000;
    let mut ledger = BankLedger {
        total_transactions: 0,
        settled_count: 0,
        rejected_count: 0,
        total_fees_collected: 0,
        total_volume_kzt: 0,
    };

    let start = Instant::now();

    for i in 0..total_tx {
        let _ = process_transaction(i, &mut ledger);
    }

    let elapsed = start.elapsed();
    let elapsed_ms = elapsed.as_millis();
    let latency_ns_per_tx = elapsed.as_nanos() / (total_tx as u128);
    let tps = if elapsed_ms > 0 {
        ((total_tx as u128) * 1000) / elapsed_ms
    } else {
        99999999
    };

    println!("==================================================================");
    println!("              ИТОГОВЫЙ ОТЧЁТ ТРАНЗАКЦИОННОЙ НАГРУЗКИ (RUST)       ");
    println!("==================================================================");
    println!("  Всего обработано транзакций : {}", ledger.total_transactions);
    println!("  Успешно проведено (Settled)  : {}", ledger.settled_count);
    println!("  Отклонено антифродом         : {}", ledger.rejected_count);
    println!("  Общее время выполнения (мс)  : {}", elapsed_ms);
    println!("  Средняя задержка на 1 tx     : {} нс (наносекунд!)", latency_ns_per_tx);
    println!("  ПРОПУСКНАЯ СПОСОБНОСТЬ (TPS) : {} транзакций / сек!", tps);
    println!("  Суммарная комиссия (KZT)     : {}", ledger.total_fees_collected);
    println!("  Контрольная сумма объёма KZT : {}", ledger.total_volume_kzt);
    println!("==================================================================");
}
