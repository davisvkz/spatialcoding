import { Money } from '../models/Money';
import { clamp } from './math';

export function formatMoney(money: Money): string {
  const major = clamp(money.cents, 0, Number.MAX_SAFE_INTEGER) / 100;
  return `${major.toFixed(2)} ${money.currency}`;
}
