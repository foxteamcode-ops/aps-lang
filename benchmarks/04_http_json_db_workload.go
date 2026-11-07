package main

import (
	"fmt"
	"time"
)

type ServerConfig struct {
	port         int64
	maxBodyBytes int64
	feeBps       int64
}

type DbMetrics struct {
	commits   int64
	rollbacks int64
	netVolume int64
}

//go:noinline
func processHttpRequests(count int64, cfg *ServerConfig) int64 {
	var statusAcc int64 = 0
	for reqId := int64(0); reqId < count; reqId++ {
		methodCode := (reqId & 3) + 1
		bodyLen := (reqId * 37) & 131071
		authOk := (reqId & 15) != 0

		scratchPtr := make([]int64, 4)
		scratchPtr[0] = reqId ^ 255
		scratch := scratchPtr[0]
		if !authOk {
			statusAcc += 401
		} else if bodyLen > cfg.maxBodyBytes {
			statusAcc += 413
		} else if methodCode == 1 || methodCode == 2 {
			statusAcc += 200 + (scratch & 7)
		} else {
			statusAcc += 404
		}
	}
	return statusAcc
}

//go:noinline
func processJsonDocuments(count int64) int64 {
	var jsonAcc int64 = 0
	for docId := int64(0); docId < count; docId++ {
		astNodes := make([]int64, 5)
		astNodes[0] = 1
		astNodes[1] = 100000 + docId
		astNodes[2] = 1500 + ((docId * 19) & 4095)
		astNodes[3] = 100 + ((docId * 7) & 63)
		astNodes[4] = 2

		if astNodes[0] == 1 && astNodes[4] == 2 {
			totalFare := (astNodes[2] * astNodes[3]) / 100
			jsonAcc += (astNodes[1] & 1023) + totalFare
		}
	}
	return jsonAcc
}

//go:noinline
func executeDbAndWorkerPipeline(
	txCount int64,
	cfg *ServerConfig,
	metrics *DbMetrics,
) int64 {
	var balanceA int64 = 500000000
	var balanceB int64 = 500000000
	var workerChecksum int64 = 0

	for txId := int64(0); txId < txCount; txId++ {
		amount := 250 + ((txId * 13) & 1023)
		forceRollback := (txId & 7) == 7

		undoA := balanceA
		undoB := balanceB

		balanceA -= amount
		balanceB += amount

		if forceRollback || balanceA < 0 {
			balanceA = undoA
			balanceB = undoB
			metrics.rollbacks++
		} else {
			fee := (amount * cfg.feeBps) / 10000
			net := amount - fee
			metrics.commits++
			metrics.netVolume += net
			workerChecksum += (net ^ (txId & 255))
		}
	}

	return workerChecksum + (balanceA / 1000) + (balanceB / 1000)
}

func main() {
	sharedCfg := ServerConfig{port: 8080, maxBodyBytes: 65536, feeBps: 800}
	dbMetrics := DbMetrics{}

	t0 := time.Now()
	httpSum := processHttpRequests(100000, &sharedCfg)
	t1 := time.Now()
	jsonSum := processJsonDocuments(100000)
	t2 := time.Now()
	dbSum := executeDbAndWorkerPipeline(100000, &sharedCfg, &dbMetrics)
	t3 := time.Now()

	httpUs := t1.Sub(t0).Microseconds()
	jsonUs := t2.Sub(t1).Microseconds()
	dbUs := t3.Sub(t2).Microseconds()
	totalUs := t3.Sub(t0).Microseconds()
	checksum := httpSum + jsonSum + dbSum + dbMetrics.commits + dbMetrics.rollbacks + dbMetrics.netVolume

	fmt.Printf("BENCH_04_SERVER elapsed_us=%d http_us=%d json_us=%d db_async_us=%d checksum=%d\n",
		totalUs, httpUs, jsonUs, dbUs, checksum)
}
