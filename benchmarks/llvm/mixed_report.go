package main

import (
	"fmt"
	"strconv"
)

type Record struct {
	id     int64
	amount float64
	weight int64
	tier   int64
}

func (self Record) score() float64 {
	return self.amount * (1.0 + float64(self.weight)*0.01)
}

func tierOf(weight int64) int64 {
	switch weight % 7 {
	case 0:
		return 0
	case 1, 2:
		return 1
	case 3, 4, 5:
		return 2
	default:
		return 3
	}
}

func fnv(h uint64, s string) uint64 {
	acc := h
	for i := 0; i < len(s); i++ {
		acc = (acc ^ uint64(s[i])) * 1099511628211
	}
	return acc
}

func main() {
	const records int64 = 12000000
	const buckets int64 = 64

	var counts [64]int64
	var sums [64]float64

	var seed uint64 = 88172645463325252
	var hash uint64 = 1469598103934665603
	var tierTotal int64 = 0

	for r := int64(0); r < records; r++ {
		seed = seed*6364136223846793005 + 1442695040888963407
		w := int64((seed >> 33) % 1000)
		seed = seed*6364136223846793005 + 1442695040888963407
		amt := float64((seed>>33)%100000) * 0.01

		rec := Record{
			id:     r,
			amount: amt,
			weight: w,
			tier:   tierOf(w),
		}

		b := rec.id % buckets
		counts[b]++
		sums[b] += rec.score()
		tierTotal += rec.tier

		if r%16 == 0 {
			label := "rec" + strconv.FormatInt(rec.id%1000, 10) + "/t" + strconv.FormatInt(rec.tier, 10)
			hash = fnv(hash, label)
		}
	}

	var totalCount int64 = 0
	var totalSum float64 = 0.0
	for b := int64(0); b < buckets; b++ {
		totalCount += counts[b]
		totalSum += sums[b]
		hash = (hash ^ uint64(counts[b])) * 1099511628211
	}

	fmt.Println(totalCount, tierTotal, hash)
	fmt.Println(strconv.FormatFloat(totalSum, 'f', -1, 64))
}
