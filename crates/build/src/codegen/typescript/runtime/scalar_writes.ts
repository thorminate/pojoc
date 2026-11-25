import { checkedVarint32, invalid } from "./wire.js";
import {
  type WriteCursor,
  type WriteInterner,
  takeWrite,
  writeByte,
  writeLength,
  writeU32Varint,
  writeUint,
  writeVarint,
  writeView,
} from "./writer.js";

function checkedNumber(
  value: unknown,
  min: number,
  max: number,
  kind: string,
): number {
  if (
    typeof value !== "number" ||
    !Number.isInteger(value) ||
    value < min ||
    value > max
  )
    invalid(`Integer out of range for ${kind}`);
  return value;
}

function writeU8(w: WriteCursor, value: unknown): void {
  writeByte(w, checkedNumber(value, 0, 0xff, "u8"));
}

function writeI8(w: WriteCursor, value: unknown): void {
  writeByte(w, checkedNumber(value, -0x80, 0x7f, "i8"));
}

function writeU16(w: WriteCursor, value: unknown): void {
  writeUint(w, 2, checkedNumber(value, 0, 0xffff, "u16"));
}

function writeI16(w: WriteCursor, value: unknown): void {
  writeUint(w, 2, checkedNumber(value, -0x8000, 0x7fff, "i16"));
}

function writeU32(w: WriteCursor, value: unknown): void {
  writeUint(w, 4, checkedNumber(value, 0, 0xffff_ffff, "u32"));
}

function writeI32(w: WriteCursor, value: unknown): void {
  writeUint(w, 4, checkedNumber(value, -0x8000_0000, 0x7fff_ffff, "i32"));
}

function checkedWide(value: unknown, signed: boolean, kind: string): bigint {
  if (typeof value !== "bigint")
    throw new TypeError(`Expected bigint for ${kind}`);
  const wrapped = signed ? BigInt.asIntN(64, value) : BigInt.asUintN(64, value);
  if (value !== wrapped) invalid(`Integer out of range for ${kind}`);
  return value;
}

function writeU64(w: WriteCursor, value: unknown): void {
  const n = checkedWide(value, false, "u64");
  const p = takeWrite(w, 8);
  writeView(w).setBigUint64(p, n, true);
}

function writeI64(w: WriteCursor, value: unknown): void {
  const n = checkedWide(value, true, "i64");
  const p = takeWrite(w, 8);
  writeView(w).setBigInt64(p, n, true);
}

function writeF32(w: WriteCursor, value: unknown): void {
  if (typeof value !== "number") throw new TypeError("Expected a Pojoc number");
  const p = takeWrite(w, 4);
  writeView(w).setFloat32(p, value, true);
}

function writeF64(w: WriteCursor, value: unknown): void {
  if (typeof value !== "number") throw new TypeError("Expected a Pojoc number");
  const p = takeWrite(w, 8);
  writeView(w).setFloat64(p, value, true);
}

function writeBool(w: WriteCursor, value: unknown): void {
  if (typeof value !== "boolean")
    throw new TypeError("Expected a Pojoc boolean");
  writeByte(w, value ? 1 : 0);
}

function writeVarint32Value(w: WriteCursor, value: unknown): void {
  writeU32Varint(w, checkedVarint32(value));
}

function writeVarint64Value(w: WriteCursor, value: unknown): void {
  writeVarint(w, checkedWide(value, false, "varint64"));
}

function writeNewIntern(
  w: WriteCursor,
  value: string,
  entries: Map<string, number>,
): void {
  const index = entries.size;
  writeLength(w, index);
  entries.set(value, index);
}

function writeIntern(
  w: WriteCursor,
  value: unknown,
  table: WriteInterner | undefined,
): void {
  if (!table) invalid("Missing Pojoc intern table");
  if (typeof value !== "string")
    throw new TypeError("Expected an interned string");
  const entries = table.entries;
  let index: number;
  if (entries) {
    const existing = entries.get(value);
    if (existing === undefined) {
      writeNewIntern(w, value, entries);
      return;
    }
    index = existing;
  } else if (table.first === value) {
    index = 0;
  } else if (table.second === value) {
    index = 1;
  } else if (table.first === undefined) {
    table.first = value;
    index = 0;
  } else if (table.second === undefined) {
    table.second = value;
    index = 1;
  } else {
    table.entries = new Map([
      [table.first, 0],
      [table.second, 1],
      [value, 2],
    ]);
    index = 2;
  }
  writeU32Varint(w, index);
}

export {
  writeU8,
  writeI8,
  writeU16,
  writeI16,
  writeU32,
  writeI32,
  writeU64,
  writeI64,
  writeF32,
  writeF64,
  writeBool,
  writeVarint32Value,
  writeVarint64Value,
  writeNewIntern,
  writeIntern,
};
