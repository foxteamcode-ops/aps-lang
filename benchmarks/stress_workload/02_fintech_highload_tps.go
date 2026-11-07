package main

import (
	"fmt"
	"time"
)

type BankLedger struct {
	totalTransactions  int64
	settledCount       int64
	rejectedCount      int64
	totalFeesCollected int64
	totalVolumeKzt     int64
}

func processTransaction(txId int64, ledger *BankLedger) int64 {
	amount := (txId*104729)%50000 + 100
	const feeBps int64 = 15
	fee := (amount * feeBps) / 10000

	ledger.totalTransactions++

	if amount > 49000 {
		ledger.rejectedCount++
		return 0
	} else {
		netPayout := amount - fee
		ledger.settledCount++
		ledger.totalFeesCollected += fee
		ledger.totalVolumeKzt += netPayout
		return netPayout
	}
}

func main() {
	fmt.Println("==================================================================")
	fmt.Println("      GO (GC-MANAGED) FINTECH HIGHLOAD: 1,000,000 TRANSACTIONS    ")
	fmt.Println("==================================================================")

	const totalTx int64 = 1000000
	var ledger BankLedger

	start := time.Now()

	for i := int64(0); i < totalTx; i++ {
		processTransaction(i, &ledger)
	}

	elapsed := time.Since(start)
	elapsedNs := elapsed.Nanoseconds()
	elapsedMs := elapsed.Milliseconds()
	var latencyNsPerTx int64 = 0
	if totalTx > 0 {
		latencyNsPerTx = elapsedNs / totalTx
	}
	var tps int64 = 99999999
	if elapsedMs > 0 {
		tps = (totalTx * 1000) / elapsedMs
	}

	fmt.Println("==================================================================")
	fmt.Println("              ИТОГОВЫЙ ОТЧЁТ ТРАНЗАКЦИОННОЙ НАГРУЗКИ (GO)         ")
	fmt.Println("==================================================================")
	fmt.Printf("  Всего обработано транзакций : %d\n", ledger.totalTransactions)
	fmt.Printf("  Успешно проведено (Settled)  : %d\n", ledger.settledCount)
	fmt.Printf("  Отклонено антифродом         : %d\n", ledger.rejectedCount)
	fmt.Printf("  Общее время выполнения (мс)  : %d\n", elapsedMs)
	fmt.Printf("  Средняя задержка на 1 tx     : %d нс (наносекунд!)\n", latencyNsPerTx)
	fmt.Printf("  ПРОПУСКНАЯ СПОСОБНОСТЬ (TPS) : %d транзакций / сек!\n", tps)
	fmt.Printf("  Суммарная комиссия (KZT)     : %d\n", ledger.totalFeesCollected)
	fmt.Printf("  Контрольная сумма объёма KZT : %d\n", ledger.totalVolumeKzt)
	fmt.Println("==================================================================")
}
