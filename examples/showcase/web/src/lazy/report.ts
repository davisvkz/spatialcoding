import { Order } from '../models';
import { formatMoney } from '../utils/format';

export function report(order: Order): string {
  return `${order.describe()} = ${formatMoney(order.total())}`;
}
