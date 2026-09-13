// The same work as `json_parse.skuld`, in Go, with `encoding/json`.
//
// Not exactly the same shape: Go decodes into `map[string]any`, Skuld into its
// own `JsonValue` tree. Both allocate a generic tree and validate the input,
// which is as close as the two get without writing a parser in Go by hand.
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"strconv"
)

func checksum(document any) int {
	total := 0
	object, ok := document.(map[string]any)
	if !ok {
		return 1
	}
	records, ok := object["records"].([]any)
	if !ok {
		return 1
	}
	total += len(records)
	for _, item := range records {
		record, ok := item.(map[string]any)
		if !ok {
			total++
			continue
		}
		if name, ok := record["name"].(string); ok {
			total += len(name)
		} else {
			total++
		}
	}
	return total
}

func main() {
	if len(os.Args) != 3 {
		fmt.Println("usage: json_parse <file.json> <rounds>")
		os.Exit(2)
	}
	text, err := os.ReadFile(os.Args[1])
	if err != nil {
		fmt.Println(err)
		os.Exit(1)
	}
	rounds, _ := strconv.Atoi(os.Args[2])
	total := 0
	for round := 0; round < rounds; round++ {
		var document any
		if err := json.Unmarshal(text, &document); err != nil {
			fmt.Println(err)
			os.Exit(1)
		}
		total = (total + checksum(document)) % 1000000007
	}
	fmt.Println(len(text), total)
}
