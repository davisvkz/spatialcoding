import { Product } from '../models';
import { formatMoney } from '../utils/format';
import { Button } from './Button';
import { Card } from './Card';

export function ProductList({ products }: { products: Product[] }) {
  return (
    <div>
      {products.map((p) => (
        <Card key={p.id} title={p.name}>
          <span>{formatMoney(p.price)}</span>
          <Button label="Add" />
        </Card>
      ))}
    </div>
  );
}
