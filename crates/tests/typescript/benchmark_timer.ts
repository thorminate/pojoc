import process from "node:process";

export const engine = process.versions.bun
  ? `Bun ${process.versions.bun}`
  : `Node ${process.version} (V8 ${process.versions.v8})`;

export function time(work: (index: number) => void): number {
  let count = 32;
  for (;;) {
    const start = performance.now();
    for (let i = 0; i < count; i++) work(i & 15);
    if (performance.now() - start >= 40 || count >= 1_048_576) break;
    count *= 2;
  }
  const results = [];
  for (let round = 0; round < 9; round++) {
    const start = performance.now();
    for (let i = 0; i < count; i++) work(i & 15);
    results.push((performance.now() - start) * 1_000_000 / count);
  }
  results.sort((a, b) => a - b);
  return results[4];
}
