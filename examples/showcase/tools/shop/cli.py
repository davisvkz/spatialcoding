from . import models
from .pricing import total
from .plugins.discount import Discount


def main() -> None:
    items = [models.Item("pen", 150), models.Bundle("pack", [models.Item("ink", 300)])]
    print(total(items, [Discount(10)]))


if __name__ == "__main__":
    main()
