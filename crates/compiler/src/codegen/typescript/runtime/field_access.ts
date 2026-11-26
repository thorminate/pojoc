import type { Cursor, ReadTable } from "./types.js";
import { emptyTable, readTableOffsets, takeBytes } from "./wire.js";

type FieldScan = {
  r: Cursor;
  table: ReadTable;
  header: number;
  start: number;
};

function startFields(
  r: Cursor,
  table: ReadTable,
  bitmap: number,
  width: number,
): FieldScan {
  const header = takeBytes(r, bitmap);
  const start = takeBytes(r, width);
  return { r, table, header, start };
}

function hasField(r: Cursor, header: number, bit: number): boolean {
  return (r.data[header + (bit >> 3)] & (1 << (bit & 7))) !== 0;
}

type SkipField = (scan: FieldScan, index: number) => void;
type ReadField = (scan: FieldScan, index: number) => unknown;
type FieldOffset = (index: number) => number;

function seekField(
  scan: FieldScan,
  index: number,
  fixedCount: number,
  offset: FieldOffset,
  skip: SkipField,
): void {
  const fixed = offset(index);
  if (fixed >= 0) scan.r.pos = scan.start + fixed;
  else for (let i = fixedCount; i < index; i++) skip(scan, i);
}

function fieldView(
  r: Cursor,
  intern: boolean,
  bitmap: number,
  width: number,
  fixedCount: number,
  offset: FieldOffset,
  skip: SkipField,
  read: ReadField,
): (index: number) => unknown {
  const positions: number[] = [];
  const values: unknown[] = [];
  let scan: FieldScan | undefined;
  return (index) => {
    if (Object.hasOwn(values, index)) return values[index];
    if (!scan) {
      const start = r.pos;
      try {
        const table = intern ? readTableOffsets(r) : emptyTable;
        scan = startFields(r, table, bitmap, width);
        positions.push(r.pos);
      } catch (error) {
        r.pos = start;
        throw error;
      }
    }
    const fixed = offset(index);
    const tailIndex = index - fixedCount;
    if (fixed >= 0) r.pos = scan.start + fixed;
    else {
      r.pos = positions[positions.length - 1];
      for (let i = positions.length - 1; i < tailIndex; i++) {
        skip(scan, i + fixedCount);
        positions.push(r.pos);
      }
      r.pos = positions[tailIndex];
    }
    const value = read(scan, index);
    if (fixed < 0 && tailIndex + 1 === positions.length) positions.push(r.pos);
    values[index] = value;
    return value;
  };
}

export type { FieldScan };
export { fieldView, hasField, seekField, startFields };
