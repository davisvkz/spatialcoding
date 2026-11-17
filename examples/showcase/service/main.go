package main

import (
	"fmt"

	"example.com/shop/internal/pricing"
	"example.com/shop/internal/store"
)

func main() {
	s := store.New()
	s.Save(store.Item{ID: "pen", Cents: 150})
	fmt.Println(pricing.Total(s.All()))
}
