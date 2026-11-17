import { formatMoney } from './format';
import { Money } from '../models/Money';

// 🎉🎉🎉 UTF-16 columns: the references below sit after astral characters.
export const party = (m: Money): string => `🎉🎉🎉 ${formatMoney(m)} 🎉`;
