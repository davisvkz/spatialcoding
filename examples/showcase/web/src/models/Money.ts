export type Currency = 'EUR' | 'USD';

export class Money {
  constructor(readonly cents: number, readonly currency: Currency = 'EUR') {}

  plus(other: Money): Money {
    return new Money(this.cents + other.cents, this.currency);
  }

  times(factor: number): Money {
    return new Money(Math.round(this.cents * factor), this.currency);
  }
}

export const ZERO = new Money(0);
