package main

import (
	"fmt"
	"time"
)

//go:noinline
func runCollectionsAndMemory(iterations int64) int64 {
	var vecKeys [16]int64
	for k := int64(0); k < 16; k++ {
		vecKeys[k] = (k * 2654435761) & 1048575
	}

	buckets := [32]int64{
		101, 203, 307, 409, 503, 607, 701, 809,
		907, 1009, 1103, 1201, 1301, 1409, 1511, 1601,
		1709, 1801, 1901, 2003, 2111, 2203, 2309, 2411,
		2503, 2609, 2707, 2801, 2903, 3001, 3109, 3203,
	}

	var acc int64 = 0
	for i := int64(0); i < iterations; i++ {
		keyIdx := i & 15
		keyVal := vecKeys[keyIdx]
		slot := (keyVal + i) & 31
		prev := buckets[slot]
		updated := ((prev * 33) + keyVal + (i & 255)) & 16777215
		buckets[slot] = updated
		acc += updated & 4095
	}

	for i := 1; i < 32; i++ {
		cur := buckets[i]
		j := i - 1
		for j >= 0 && buckets[j] > cur {
			buckets[j+1] = buckets[j]
			j--
		}
		buckets[j+1] = cur
	}

	searchQueries := int64(100000)
	var foundHits int64 = 0
	for q := int64(0); q < searchQueries; q++ {
		target := buckets[q&31]
		lo := 0
		hi := 31
		for lo <= hi {
			mid := (lo + hi) / 2
			midVal := buckets[mid]
			if midVal == target {
				foundHits++
				break
			} else if midVal < target {
				lo = mid + 1
			} else {
				hi = mid - 1
			}
		}
	}

	return acc + foundHits + buckets[0] + buckets[31]
}

//go:noinline
func runStringWorkload(scanIters, concatIters int64) int64 {
	hdrGet := "GET /api/v1/orders/dispatch HTTP/1.1"
	hdrPost := "POST /api/v1/settlement/commit HTTP/1.1"
	hdrProbe := "GET /api/v1/orders/dispatch HTTP/1.1"

	var strAcc int64 = 0
	for i := int64(0); i < scanIters; i++ {
		n1 := int64(len(hdrGet))
		n2 := int64(len(hdrPost))
		if (i & 1) == 0 {
			if hdrGet == hdrProbe {
				strAcc += n1
			}
		} else {
			if hdrGet != hdrPost {
				strAcc += n2
			}
		}
	}

	prefix := "aps_tx_"
	suffix := "_ok"
	for c := int64(0); c < concatIters; c++ {
		out := prefix + suffix
		strAcc += int64(len(out)) + (c & 3)
	}

	return strAcc
}

func main() {
	t0 := time.Now()
	collSum := runCollectionsAndMemory(2000000)
	t1 := time.Now()
	strSum := runStringWorkload(500000, 20000)
	t2 := time.Now()

	collUs := t1.Sub(t0).Microseconds()
	strUs := t2.Sub(t1).Microseconds()
	totalUs := t2.Sub(t0).Microseconds()
	checksum := collSum + strSum

	fmt.Printf("BENCH_03_MEM_COLL elapsed_us=%d collections_us=%d strings_us=%d checksum=%d\n",
		totalUs, collUs, strUs, checksum)
}
