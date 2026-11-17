from ..pricing import Rule


class Discount(Rule):
    def __init__(self, percent: int) -> None:
        self.percent = percent

    def apply(self, cents: int) -> int:
        return cents * (100 - self.percent) // 100
