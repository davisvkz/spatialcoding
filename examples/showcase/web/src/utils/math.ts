export const clamp = (value: number, min: number, max: number): number =>
  Math.min(max, Math.max(min, value));

export function percent(part: number, whole: number): number {
  return whole === 0 ? 0 : (part / whole) * 100;
}
