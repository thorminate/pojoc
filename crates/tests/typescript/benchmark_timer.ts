import process from "node:process";

export const engine = process.versions.bun
  ? `Bun ${process.versions.bun}`
  : `Node ${process.version} (V8 ${process.versions.v8})`;

type Work = (index: number) => void;

function measure(work: Work, count: number): number {
  const start = performance.now();
  for (let i = 0; i < count; i++) work(i & 15);
  return performance.now() - start;
}

function calibrate(work: Work): number {
  let count = 32;
  while (measure(work, count) < 40 && count < 1_048_576) count *= 2;
  return count;
}

export function timeCases(cases: readonly Work[]): number[] {
  const counts = cases.map(calibrate);
  const samples = cases.map(() => [] as number[]);
  for (let round = 0; round < 9; round++) {
    for (let step = 0; step < cases.length; step++) {
      const index = (round + step) % cases.length;
      const count = counts[index];
      samples[index].push((measure(cases[index], count) * 1_000_000) / count);
    }
  }
  return samples.map((results) => results.sort((a, b) => a - b)[4]);
}

export function time(work: Work): number {
  return timeCases([work])[0];
}
