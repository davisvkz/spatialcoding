import { PricingService } from '../services/PricingService';
import * as UI from './index';

const pricing = new PricingService();

export function App() {
  return (
    <main>
      <UI.ProductList products={pricing.catalog()} />
      <UI.Button label="Checkout" />
    </main>
  );
}
