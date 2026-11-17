from .models import Base, Item


class Rule:
    def apply(self, cents: int) -> int:
        return cents


def total(items: list[Item], rules: list[Rule] | None = None) -> int:
    cents = sum(i.price() for i in items)
    for rule in rules or []:
        cents = rule.apply(cents)
    return cents


def cheapest(items: list[Base]) -> Base:
    return min(items, key=lambda i: i.price())
