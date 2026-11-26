import type { Primitive } from "./types.js";
import { invalid } from "./wire.js";
import { type WriteCursor, writeString, writeU32Varint } from "./writer.js";
import {
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
} from "./scalar_writes.js";

function writeLegacyIntern(
  w: WriteCursor,
  value: unknown,
  table: Map<string, number> | undefined,
): void {
  if (!table) invalid("Missing Pojoc intern table");
  if (typeof value !== "string")
    throw new TypeError("Expected an interned string");
  const index = table.get(value);
  if (index === undefined) writeNewIntern(w, value, table);
  else writeU32Varint(w, index);
}

const scalarWriters = {
  u8: writeU8,
  i8: writeI8,
  u16: writeU16,
  i16: writeI16,
  u32: writeU32,
  i32: writeI32,
  u64: writeU64,
  i64: writeI64,
  f32: writeF32,
  f64: writeF64,
  bool: writeBool,
  varint32: writeVarint32Value,
  varint64: writeVarint64Value,
  string: writeString,
  intern: writeLegacyIntern,
};

function writeScalar(
  w: WriteCursor,
  ty: Primitive,
  value: unknown,
  table: Map<string, number> | undefined,
): void {
  scalarWriters[ty](w, value, table);
}

export { writeScalar };
