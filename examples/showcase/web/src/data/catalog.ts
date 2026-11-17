import { DigitalProduct, Money, Product } from '../models';

// The manual script regenerates this file with more than 1000 lines, to try
// "Collapse long files". The committed copy stays short.
export const catalog: Product[] = [
  new Product('p-1', 'keyboard', new Money(4900)),
  new Product('p-2', 'mouse', new Money(2500)),
  new DigitalProduct('p-3', 'manual', new Money(500), 'https://example.com/manual.pdf'),
];
