package main

import (
	"fmt"
	"time"
)

type ArenaNode struct {
	id       int64
	weight   int64
	tag      int64
	checksum int64
}

//go:noinline
func runStandardHeapBatches(batches, itemsPerBatch int64) int64 {
	var totalAcc int64 = 0
	ptrTable := make([][]int64, itemsPerBatch)
	for b := int64(0); b < batches; b++ {
		var batchSum int64 = 0
		for i := int64(0); i < itemsPerBatch; i++ {
			raw := ((b*itemsPerBatch + i)*1664525 + 1013904223) & 2147483647
			blk := make([]int64, 4)
			blk[0] = raw
			batchSum += blk[0] & 65535
			ptrTable[i] = blk
		}
		for i := int64(0); i < itemsPerBatch; i++ {
			ptrTable[i] = nil
		}
		totalAcc += batchSum
	}
	return totalAcc
}

//go:noinline
func runStructAllocations(count int64) int64 {
	var structAcc int64 = 0
	for i := int64(0); i < count; i++ {
		node := ArenaNode{
			id:       i,
			weight:   (i*7 + 3) & 65535,
			tag:      (i ^ 12345) & 65535,
			checksum: (i * 13) & 65535,
		}
		structAcc += node.weight + node.tag + node.checksum
	}
	return structAcc
}

func main() {
	t0 := time.Now()
	arenaSum := runStandardHeapBatches(200, 10000)
	t1 := time.Now()
	structSum := runStructAllocations(200000)
	t2 := time.Now()

	arenaUs := t1.Sub(t0).Microseconds()
	structUs := t2.Sub(t1).Microseconds()
	totalUs := t2.Sub(t0).Microseconds()
	checksum := arenaSum + structSum

	fmt.Printf("BENCH_02_ALLOC elapsed_us=%d arena_2M_us=%d struct_200k_us=%d checksum=%d\n",
		totalUs, arenaUs, structUs, checksum)
}
