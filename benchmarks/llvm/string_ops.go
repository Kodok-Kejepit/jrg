package main

import (
	"fmt"
	"strconv"
	"strings"
)

func fnv(h uint64, s string) uint64 {
	acc := h
	for i := 0; i < len(s); i++ {
		acc = (acc ^ uint64(s[i])) * 1099511628211
	}
	return acc
}

func reverse(s string) string {
	r := []rune(s)
	for i, j := 0, len(r)-1; i < j; i, j = i+1, j-1 {
		r[i], r[j] = r[j], r[i]
	}
	return string(r)
}

func main() {
	const rounds int64 = 600000
	var hash uint64 = 1469598103934665603

	for r := int64(0); r < rounds; r++ {
		base := "item-" + strconv.FormatInt(r%997, 10) + ":" + strconv.FormatInt(r%31, 10)
		upper := strings.ToUpper(base)
		flipped := reverse(upper)
		swapped := strings.ReplaceAll(flipped, "-", "_")

		hash = fnv(hash, base)
		hash = fnv(hash, swapped)
		hash = hash + uint64(len([]rune(swapped)))
	}

	fmt.Println(hash)
}
