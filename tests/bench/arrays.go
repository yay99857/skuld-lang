// The same work as `arrays.skuld`, in Go. See `README.md` for how to build.
package main

import (
	"fmt"
	"os"
	"sort"
	"strconv"
)

func next(state int64) int64 {
	return (state*1664525 + 1013904223) % 2147483648
}

func main() {
	count := 0
	if len(os.Args) > 1 {
		count, _ = strconv.Atoi(os.Args[1])
	}
	values := []int64{}
	var state int64 = 12345
	for i := 0; i < count; i++ {
		state = next(state)
		values = append(values, state%1000000)
	}
	// Stable, to match Skuld's `sort()`, which is stable by definition.
	sort.SliceStable(values, func(i, j int) bool { return values[i] < values[j] })
	var checksum int64 = 0
	for index, value := range values {
		checksum = (checksum + value*(int64(index)%7+1)) % 1000000007
	}
	fmt.Println(checksum)
}
