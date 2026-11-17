package pricing

import "example.com/shop/internal/store"

func Total(items []store.Item) int {
	sum := 0
	for _, item := range items {
		sum += item.Cents
	}
	return sum
}
