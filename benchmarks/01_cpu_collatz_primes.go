package main

import (
	"fmt"
	"time"
)

//go:noinline
func collatzSteps(startN int64) int64 {
	n := startN
	var steps int64 = 0
	for n > 1 {
		if (n % 2) == 0 {
			n = n / 2
		} else {
			n = 3*n + 1
		}
		steps++
	}
	return steps
}

//go:noinline
func isPrimeFast(n int64) bool {
	if n < 2 {
		return false
	}
	if n == 2 || n == 3 {
		return true
	}
	if (n%2) == 0 || (n%3) == 0 {
		return false
	}
	var d int64 = 5
	for (d * d) <= n {
		if (n%d) == 0 || (n%(d+2)) == 0 {
			return false
		}
		d += 6
	}
	return true
}

//go:noinline
func matrixPolyKernel(iterations int64) int64 {
	var acc0 int64 = 17
	var acc1 int64 = 31
	var acc2 int64 = 47
	var acc3 int64 = 73
	for i := int64(1); i < iterations; i++ {
		acc0 = int64((uint64(acc0)*1103515245 + 12345 + uint64(i)) & 2147483647)
		acc1 = int64((uint64(acc1 ^ (acc0 >> 3))) + uint64(i)*7)
		acc2 = int64((uint64(acc2) + uint64(acc1)) & 1073741823)
		acc3 = acc3 ^ int64(uint64(acc2)+uint64(i)*13)
	}
	return int64((uint64(acc0) + uint64(acc1) + uint64(acc2) + uint64(acc3)) & 2147483647)
}

func main() {
	t0 := time.Now()

	collatzLimit := int64(500001)
	var totalCollatzSteps int64 = 0
	for n := int64(1); n < collatzLimit; n++ {
		totalCollatzSteps += collatzSteps(n)
	}

	primeLimit := int64(200001)
	var primeCount int64 = 0
	for p := int64(2); p < primeLimit; p++ {
		if isPrimeFast(p) {
			primeCount++
		}
	}

	polyIters := int64(5000001)
	polyChecksum := matrixPolyKernel(polyIters)

	elapsedUs := time.Since(t0).Microseconds()
	checksum := totalCollatzSteps + primeCount + polyChecksum

	fmt.Printf("BENCH_01_CPU elapsed_us=%d collatz=%d primes=%d checksum=%d\n",
		elapsedUs, totalCollatzSteps, primeCount, checksum)
}
