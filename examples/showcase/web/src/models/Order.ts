import { Entity } from './Entity';
import { Money, ZERO } from './Money';
import { Product } from './Product';

export interface Line {
  product: Product;
  quantity: number;
}

export class Order extends Entity {
  private lines: Line[] = [];

  add(product: Product, quantity = 1): this {
    this.lines.push({ product, quantity });
    return this;
  }

  total(): Money {
    return this.lines.reduce((sum, l) => sum.plus(l.product.price.times(l.quantity)), ZERO);
  }

  describe(): string {
    return `order ${this.id}: ${this.lines.length} lines`;
  }
}
