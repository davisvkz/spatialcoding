export interface Identifiable {
  readonly id: string;
}

export interface Describable extends Identifiable {
  describe(): string;
}

export abstract class Entity implements Identifiable {
  constructor(public readonly id: string) {}

  abstract describe(): string;

  equals(other: Entity): boolean {
    return this.id === other.id;
  }
}
