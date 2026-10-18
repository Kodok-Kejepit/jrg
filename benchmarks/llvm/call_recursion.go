package main

import "fmt"

func fib(n int64) int64 {
	if n < 2 {
		return n
	}
	return fib(n-1) + fib(n-2)
}

func ackLite(m, n int64) int64 {
	if m == 0 {
		return n + 1
	}
	if n == 0 {
		return ackLite(m-1, 1)
	}
	return ackLite(m-1, ackLite(m, n-1))
}

func stepA(v int64) int64 {
	return v*3 + 1
}

func stepB(v int64) int64 {
	return stepA(v) ^ (v >> 2)
}

func stepC(v int64) int64 {
	return stepB(v) + stepA(v>>1)
}

func chain(v int64) int64 {
	return stepC(stepB(stepA(v)))
}

func main() {
	const iterations int64 = 20000000
	var acc int64 = 0

	for i := int64(0); i < iterations; i++ {
		acc = (acc + chain(i)) % 1000000007
	}

	acc += fib(39)
	acc += ackLite(2, 8)

	fmt.Println(acc)
}
