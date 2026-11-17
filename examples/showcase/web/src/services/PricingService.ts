import { Money, Order, Product } from '../models';
import { catalog } from '../data/catalog';
import { percent } from '../utils/math';
import { InMemoryRepository, Repository } from './Repository';

export class PricingService {
  constructor(private readonly products: Repository<Product> = new InMemoryRepository<Product>()) {}

  discount(order: Order, rate: number): Money {
    return order.total().times(1 - percent(rate, 100) / 100);
  }

  catalog(): Product[] {
    return this.products.all().length > 0 ? this.products.all() : catalog;
  }
}
