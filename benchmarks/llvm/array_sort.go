package main

import "fmt"

const (
	N      = 2000
	ROUNDS = 3000
)

func main() {
	const last int64 = N - 1
	var hash uint64 = 1469598103934665603
	var seed uint64 = 88172645463325252
	anyFailed := false
	var data [N]int64

	for r := int64(0); r < ROUNDS; r++ {
		for i := 0; i < N; i++ {
			seed = seed*6364136223846793005 + 1442695040888963407
			data[i] = int64(seed >> 1)
		}

		// ---- build heap ----
		for start := int64(N-2) / 2; start >= 0; start-- {
			root := start
			for {
				child := root*2 + 1
				if child > last {
					break
				}
				if child+1 <= last && data[child] < data[child+1] {
					child++
				}
				if data[root] < data[child] {
					data[root], data[child] = data[child], data[root]
					root = child
				} else {
					break
				}
			}
		}

		// ---- drain heap ----
		for end := last; end > 0; {
			data[0], data[end] = data[end], data[0]
			end--

			root := int64(0)
			for {
				child := root*2 + 1
				if child > end {
					break
				}
				if child+1 <= end && data[child] < data[child+1] {
					child++
				}
				if data[root] < data[child] {
					data[root], data[child] = data[child], data[root]
					root = child
				} else {
					break
				}
			}
		}

		prev := data[0]
		for i := 0; i < N; i++ {
			if data[i] < prev {
				anyFailed = true
			}
			prev = data[i]
			hash = (hash ^ uint64(data[i])) * 11400714819323198485
		}
	}

	if anyFailed {
		fmt.Println("SORT_FAILED")
	} else {
		fmt.Println(hash)
	}
}
