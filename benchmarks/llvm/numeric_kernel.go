package main

import "fmt"

func kernel(iterations int64) uint64 {
	var x uint64 = 305419896
	for i := int64(0); i < iterations; i++ {
		x = (x*1664525 + 1013904223) ^ (x >> 13)
	}
	return x
}

func main() {
	fmt.Println(kernel(200000000))
}
