// The same work as `strings.skuld`, in Go. See `README.md`.
package main

import (
	"fmt"
	"os"
	"strconv"
	"unicode/utf8"
)

func pushNumber(buffer []byte, value int) []byte {
	digits := []byte{}
	rest := value
	if rest == 0 {
		digits = append(digits, 48)
	}
	for rest > 0 {
		digits = append(digits, byte(48+rest%10))
		rest /= 10
	}
	for index := len(digits) - 1; index >= 0; index-- {
		buffer = append(buffer, digits[index])
	}
	return buffer
}

func main() {
	count := 0
	if len(os.Args) > 1 {
		count, _ = strconv.Atoi(os.Args[1])
	}
	buffer := []byte{}
	for index := 0; index < count; index++ {
		buffer = append(buffer, []byte("item-")...)
		buffer = pushNumber(buffer, index)
		buffer = append(buffer, 59)
	}
	// Validated, like Skuld's `bytes_to_string`.
	if !utf8.Valid(buffer) {
		fmt.Println("not utf-8")
		os.Exit(1)
	}
	text := string(buffer)
	found := 0
	for at := 0; at+1 < len(text); at++ {
		if text[at] == 57 && text[at+1] == 59 {
			found++
		}
	}
	fmt.Println(len(text), found)
}
