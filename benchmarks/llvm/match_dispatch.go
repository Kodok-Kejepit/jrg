package main

import "fmt"

type opTag int

const (
	opPush opTag = iota
	opAddK
	opMulK
	opXor
	opRot
	opRange
)

type Op struct {
	tag  opTag
	a, b int64
}

func classify(v int64) int64 {
	switch v % 11 {
	case 0:
		return 5
	case 1, 2, 3:
		return 7
	case 4, 5, 6, 7, 8:
		return 11
	default:
		return 13
	}
}

func exec(o Op, acc int64) int64 {
	switch o.tag {
	case opPush:
		return o.a
	case opAddK:
		return acc + o.a
	case opMulK:
		return (acc * o.a) % 1000003
	case opXor:
		return acc ^ 1431655765
	case opRot:
		return ((acc << 7) | (acc >> 25)) % 1000003
	case opRange:
		out := acc
		if acc < o.a {
			out = o.a
		}
		if acc > o.b {
			out = o.b
		}
		return out
	}
	return acc
}

func main() {
	const iterations int64 = 45000000
	var acc int64 = 1
	var total int64 = 0

	for i := int64(0); i < iterations; i++ {
		sel := i % 6
		var op Op
		if sel == 0 {
			op = Op{tag: opPush, a: i % 997}
		} else if sel == 1 {
			op = Op{tag: opAddK, a: i % 31}
		} else if sel == 2 {
			op = Op{tag: opMulK, a: 3 + i%7}
		} else if sel == 3 {
			op = Op{tag: opXor}
		} else if sel == 4 {
			op = Op{tag: opRot}
		} else {
			op = Op{tag: opRange, a: 10, b: 900000}
		}

		acc = exec(op, acc)
		total = (total + classify(acc)) % 1000000007
	}

	fmt.Println(acc, total)
}
