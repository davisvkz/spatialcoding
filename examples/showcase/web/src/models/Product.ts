import { Describable, Entity } from './Entity';
import { Money } from './Money';

export class Product extends Entity implements Describable {
  constructor(id: string, readonly name: string, readonly price: Money) {
    super(id);
  }

  describe(): string {
    return `${this.name} (${this.price.cents / 100})`;
  }
}

export class DigitalProduct extends Product {
  constructor(id: string, name: string, price: Money, readonly url: string) {
    super(id, name, price);
  }
}
