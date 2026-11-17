import { App } from './ui/App';
import { party } from './utils/emoji';
import { Money } from './models/Money';
import limit from './legacy/cjs';

export async function main(): Promise<void> {
  const { report } = await import('./lazy/report');
  console.log(party(new Money(limit(150))), App, report);
}
