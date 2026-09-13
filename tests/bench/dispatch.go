// The same work as `dispatch.skuld`, in Go. See `README.md`.
package main

import (
	"fmt"
	"os"
	"strconv"
)

type Shape interface {
	Area() int64
}

type Square struct{ side int64 }
type Rectangle struct{ width, height int64 }
type Triangle struct{ base, height int64 }

func (s *Square) Area() int64    { return s.side * s.side }
func (r *Rectangle) Area() int64 { return r.width * r.height }
func (t *Triangle) Area() int64  { return t.base * t.height / 2 }

func main() {
	count := 0
	if len(os.Args) > 1 {
		count, _ = strconv.Atoi(os.Args[1])
	}
	shapes := []Shape{}
	for index := 0; index < count; index++ {
		i := int64(index)
		switch index % 3 {
		case 0:
			shapes = append(shapes, &Square{side: i % 100})
		case 1:
			shapes = append(shapes, &Rectangle{width: i % 50, height: i % 20})
		default:
			shapes = append(shapes, &Triangle{base: i % 40, height: i % 30})
		}
	}
	var total int64 = 0
	for _, shape := range shapes {
		total = (total + shape.Area()) % 1000000007
	}
	fmt.Println(total)
}
