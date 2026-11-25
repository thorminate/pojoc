import { type WriteCursor, writeU32Varint, writeVarint } from "./writer.js";
import type { Cursor, FixedScalar, Primitive, ReadTable } from "./types.js";
import {
  cursorView,
  invalid,
  limitCollection,
  readByte,
  readIntern,
  readString,
  readUint,
  readVarint,
  readVarint32,
  takeBytes,
} from "./wire.js";

type NumericArray =
  | Uint8Array
  | Int8Array
  | Uint16Array
  | Int16Array
  | Uint32Array
  | Int32Array
  | BigUint64Array
  | BigInt64Array
  | Float32Array
  | Float64Array;
type NumericConstructor = {
  readonly BYTES_PER_ELEMENT: number;
  new (length: number): NumericArray;
  new (buffer: ArrayBufferLike, offset: number, length: number): NumericArray;
};

const packed: Partial<Record<Primitive, NumericConstructor>> = {
  u8: Uint8Array,
  i8: Int8Array,
  u16: Uint16Array,
  i16: Int16Array,
  u32: Uint32Array,
  i32: Int32Array,
  u64: BigUint64Array,
  i64: BigInt64Array,
  f32: Float32Array,
  f64: Float64Array,
};
const littleEndian = new Uint8Array(new Uint16Array([1]).buffer)[0] === 1;

function arrayOf(ty: unknown): NumericConstructor | undefined {
  return typeof ty === "string" ? packed[ty as Primitive] : undefined;
}

function readPacked(
  r: Cursor,
  ctor: NumericConstructor,
  count: number,
): NumericArray {
  const width = ctor.BYTES_PER_ELEMENT;
  const start = takeBytes(r, count * width);
  const data = r.data;
  const offset = data.byteOffset + start;
  if ((littleEndian || width === 1) && offset % width === 0) {
    return new ctor(data.buffer, offset, count);
  }
  const array = new ctor(count);
  const copy = new Uint8Array(array.buffer);
  copy.set(data.subarray(start, r.pos));
  if (!littleEndian && width > 1) {
    for (let i = 0; i < copy.length; i += width)
      copy.subarray(i, i + width).reverse();
  }
  return array;
}

function integerKind(ty: unknown): [number, boolean] | undefined {
  if (ty === "varint32") return [32, false];
  if (ty === "varint64") return [64, false];
  if (typeof ty !== "string" || (ty[0] !== "u" && ty[0] !== "i")) return;
  if (ty === "intern") return;
  return [Number(ty.slice(1)), ty[0] === "i"];
}

function integer(ty: unknown, value: unknown): bigint {
  const [bits, signed] = integerKind(ty)!;
  if (
    bits === 64
      ? typeof value !== "bigint"
      : typeof value !== "number" || !Number.isInteger(value)
  ) {
    throw new TypeError(
      `Expected ${bits === 64 ? "bigint" : "integer"} for ${ty}`,
    );
  }
  const n = BigInt(value as number | bigint);
  const wrapped = signed ? BigInt.asIntN(bits, n) : BigInt.asUintN(bits, n);
  if (n !== wrapped) invalid(`Integer out of range for ${ty}`);
  return n;
}

const fixedWidths: Record<FixedScalar, number> = {
  u8: 1,
  i8: 1,
  u16: 2,
  i16: 2,
  u32: 4,
  i32: 4,
  u64: 8,
  i64: 8,
  f32: 4,
  f64: 8,
  bool: 1,
};

function isFixedScalar(ty: unknown): ty is FixedScalar {
  return typeof ty === "string" && Object.hasOwn(fixedWidths, ty);
}

function readFixedScalar(
  view: DataView,
  start: number,
  ty: FixedScalar,
): number | bigint | boolean {
  switch (ty) {
    case "u8":
      return view.getUint8(start);
    case "i8":
      return view.getInt8(start);
    case "u16":
      return view.getUint16(start, true);
    case "i16":
      return view.getInt16(start, true);
    case "u32":
      return view.getUint32(start, true);
    case "i32":
      return view.getInt32(start, true);
    case "u64":
      return view.getBigUint64(start, true);
    case "i64":
      return view.getBigInt64(start, true);
    case "f32":
      return view.getFloat32(start, true);
    case "f64":
      return view.getFloat64(start, true);
    case "bool":
      return view.getUint8(start) !== 0;
  }
}

function readScalar(r: Cursor, ty: Primitive, table: ReadTable): unknown {
  switch (ty) {
    case "string":
      return readString(r);
    case "varint32":
      return readVarint32(r);
    case "varint64":
      return readVarint(r);
    case "intern":
      return readIntern(r, table);
    case "u8":
      return readByte(r);
    case "i8":
      return (readByte(r) << 24) >> 24;
    case "bool":
      return readByte(r) !== 0;
    case "u16":
      return readUint(r, 2);
    case "i16":
      return (readUint(r, 2) << 16) >> 16;
    case "u32":
      return readUint(r, 4);
    case "i32":
      return readUint(r, 4) | 0;
    default:
      return readFixedScalar(cursorView(r), takeBytes(r, fixedWidths[ty]), ty);
  }
}

function sequence(value: unknown): ArrayLike<unknown> {
  if (
    Array.isArray(value) ||
    (ArrayBuffer.isView(value) && !(value instanceof DataView))
  ) {
    return value as ArrayLike<unknown>;
  }
  throw new TypeError("Expected a Pojoc array");
}

function typedValues(ty: unknown, items: unknown[]): unknown[] | NumericArray {
  const ctor = arrayOf(ty);
  if (!ctor) return items;
  const target = new ctor(items.length);
  const writable = target as { [index: number]: unknown };
  items.forEach((item, i) => {
    writable[i] = item;
  });
  return target;
}

function unsignedBytes(value: bigint, width: number): Uint8Array {
  const bytes = new Uint8Array(width);
  for (let n = 0; n < width; n++, value >>= 8n) bytes[n] = Number(value & 255n);
  return bytes;
}

function bytesAsInt(bytes: Uint8Array): bigint {
  let value = 0n;
  for (let i = Math.min(bytes.length, 16) - 1; i >= 0; i--)
    value = (value << 8n) | BigInt(bytes[i]);
  return value;
}

function signedVarint(r: Cursor): bigint {
  const value = readVarint(r);
  return (value >> 1n) ^ -(value & 1n);
}

function readWideDelta32(r: Cursor, first: number): number {
  const { data, end } = r;
  let pos = r.pos;
  // The first bit is the sign. Shifting each later group into place
  // keeps only the 32 magnitude bits needed by the wrapping accumulator.
  const sign = -(first & 1);
  let value = (first & 127) >>> 1;
  let byte: number;
  for (let shift = 6; shift <= 27; shift += 7) {
    if (pos >= end) invalid("Unexpected end of Pojoc payload");
    byte = data[pos++];
    value |= (byte & 127) << shift;
    if (byte < 128) {
      r.pos = pos;
      return value ^ sign;
    }
  }

  for (let index = 5; index < 10; index++) {
    if (pos >= end) invalid("Unexpected end of Pojoc payload");
    byte = data[pos++];
    if (index === 9 && byte > 1) invalid("Pojoc varint overflow");
    if (byte < 128) {
      r.pos = pos;
      return value ^ sign;
    }
  }
  return invalid("Pojoc varint overflow");
}

function writeSignedVarint(w: WriteCursor, value: bigint): void {
  const wrapped = BigInt.asIntN(64, value);
  writeVarint(w, (wrapped << 1n) ^ (wrapped >> 63n));
}

function decodeDelta(
  r: Cursor,
  ty: unknown,
  count: number,
): unknown[] | NumericArray {
  limitCollection(count);
  if (count > r.end - r.pos) invalid("Truncated Pojoc delta array");
  const [bits, signed] = integerKind(ty)!;
  if (bits < 64) {
    const items = new (arrayOf(ty)!)(count);
    if (count === 0) return items;
    let previous = 0;
    let start = 0;
    if (!signed) {
      previous = readVarint32(r);
      items[0] = previous;
      start = 1;
    }
    const { data, end } = r;
    let pos = r.pos;
    for (let i = start; i < count; i++) {
      if (pos >= end) invalid("Unexpected end of Pojoc payload");
      const byte = data[pos++];
      let delta;
      if (byte < 128) {
        delta = (byte >>> 1) ^ -(byte & 1);
      } else {
        r.pos = pos;
        delta = readWideDelta32(r, byte);
        pos = r.pos;
      }
      previous = (previous + delta) | 0;
      items[i] = previous;
    }
    r.pos = pos;
    return items;
  }
  const wrap = signed ? BigInt.asIntN : BigInt.asUintN;
  const items = new (signed ? BigInt64Array : BigUint64Array)(count);
  let previous = 0n;
  for (let i = 0; i < count; i++) {
    const next = i
      ? previous + signedVarint(r)
      : signed
        ? signedVarint(r)
        : readVarint(r);
    previous = wrap(64, next);
    items[i] = previous;
  }
  return items;
}

function encodeDelta(
  w: WriteCursor,
  ty: unknown,
  value: ArrayLike<unknown>,
): void {
  const [bits, signed] = integerKind(ty)!;
  if (bits < 64) {
    const bound = 2 ** (signed ? bits - 1 : bits);
    const min = signed ? -bound : 0;
    let previous = 0;
    for (let i = 0; i < value.length; i++) {
      const next = value[i];
      if (typeof next !== "number" || !Number.isInteger(next))
        throw new TypeError(`Expected integer for ${ty}`);
      if (next < min || next >= bound)
        invalid(`Integer out of range for ${ty}`);
      const delta = next - previous;
      const zigzag = delta < 0 ? -2 * delta - 1 : 2 * delta;
      const encoded = i === 0 && !signed ? next : zigzag;
      // A jump across the full 32-bit range can need 33 ZigZag bits.
      if (encoded <= 0xffff_ffff) writeU32Varint(w, encoded);
      else writeVarint(w, BigInt(encoded));
      previous = next;
    }
    return;
  }
  let previous = 0n;
  for (let i = 0; i < value.length; i++) {
    const next = integer(ty, value[i]);
    if (i || signed) writeSignedVarint(w, next - previous);
    else writeVarint(w, next);
    previous = next;
  }
}

function resized(bytes: Uint8Array, width: number): Uint8Array {
  if (bytes.length === width) return bytes;
  const result = new Uint8Array(width);
  result.set(bytes.subarray(0, width));
  return result;
}

export {
  littleEndian,
  arrayOf,
  readPacked,
  integerKind,
  fixedWidths,
  isFixedScalar,
  readFixedScalar,
  readScalar,
  sequence,
  typedValues,
  unsignedBytes,
  bytesAsInt,
  decodeDelta,
  encodeDelta,
  resized,
};
