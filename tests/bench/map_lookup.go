// The same work as `map_lookup.skuld`, in Go. See `README.md`.
package main

import (
	"fmt"
	"os"
	"strconv"
)

func keysOf(count int) []string {
	keys := make([]string, 0, count)
	for index := 0; index < count; index++ {
		keys = append(keys, "key-"+strconv.Itoa(index)+"-name")
	}
	return keys
}

func linearTotal(keys []string) int {
	names := []string{}
	values := []int{}
	for index, key := range keys {
		names = append(names, key)
		values = append(values, index)
	}
	total := 0
	for _, key := range keys {
		for position := range names {
			if names[position] == key {
				total += values[position]
				break
			}
		}
	}
	return total
}

func mapTotal(keys []string) int {
	indexOf := map[string]int{}
	for index, key := range keys {
		indexOf[key] = index
	}
	total := 0
	for _, key := range keys {
		if value, ok := indexOf[key]; ok {
			total += value
		}
	}
	return total
}

func main() {
	mode := ""
	count := 0
	if len(os.Args) > 2 {
		mode = os.Args[1]
		count, _ = strconv.Atoi(os.Args[2])
	}
	keys := keysOf(count)
	if mode == "map" {
		fmt.Println(mapTotal(keys))
		return
	}
	fmt.Println(linearTotal(keys))
}
