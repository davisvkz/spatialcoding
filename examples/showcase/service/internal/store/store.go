package store

type Item struct {
	ID    string
	Cents int
}

type Store interface {
	Save(item Item)
	All() []Item
}

type memStore struct {
	items []Item
}

func New() Store {
	return &memStore{}
}

func (m *memStore) Save(item Item) {
	m.items = append(m.items, item)
}

func (m *memStore) All() []Item {
	return m.items
}
