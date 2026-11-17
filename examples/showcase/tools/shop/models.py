from abc import ABC, abstractmethod


class Base(ABC):
    @abstractmethod
    def price(self) -> int: ...


class Item(Base):
    def __init__(self, name: str, cents: int) -> None:
        self.name = name
        self.cents = cents

    def price(self) -> int:
        return self.cents


class Bundle(Item):
    def __init__(self, name: str, items: list[Item]) -> None:
        super().__init__(name, sum(i.price() for i in items))
        self.items = items
