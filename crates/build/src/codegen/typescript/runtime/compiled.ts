import type { Cursor, Obj, ReadTable, Version } from "./types.js";
import {
  emptyTable,
  invalid,
  limitCollection,
  readByte,
  readBytes,
  readLength,
  readVarint,
  readUint,
  tableLength,
  takeBytes,
  textEncoder,
  varint32Size,
  writeVarint32,
} from "./wire.js";
import {
  type WriteCursor,
  type WriteInterner,
  beginWrite,
  prependInternTable,
  takeWrite,
  writeByte,
  writeBytes,
  writeLength,
  writeU32At,
  writeUint,
} from "./writer.js";
import { integerKind, littleEndian, resized, sequence } from "./scalars.js";

type WriteTable = WriteInterner | undefined;
type ReadItem<T = unknown> = (r: Cursor, table: ReadTable) => T;
type WriteItem = (w: WriteCursor, value: unknown, table: WriteTable) => void;

function object(value: unknown): Obj {
  if (typeof value !== "object" || value === null)
    throw new TypeError("Expected a Pojoc record");
  return value as Obj;
}

function setField(target: Obj, name: string, value: unknown): void {
  if (name === "__proto__") {
    Object.defineProperty(target, name, {
      value,
      enumerable: true,
      writable: true,
      configurable: true,
    });
  } else target[name] = value;
}

function versionNumber(version: Version): bigint {
  if (typeof version === "number" && !Number.isSafeInteger(version))
    invalid("Use bigint for a large Pojoc version");
  const number = BigInt(version);
  if (number < 0n || number > 0xffff_ffff_ffff_ffffn)
    invalid("Expected unsigned 64-bit varint");
  return number;
}

function unsupported(version: bigint): never {
  return invalid(`Unsupported Pojoc version ${version}`);
}

function finishDecode<T>(r: Cursor, value: Obj): T {
  if (r.pos !== r.end) invalid("Unexpected trailing Pojoc payload");
  return value as T;
}

function writePayloadWithTable(
  w: WriteCursor,
  value: Obj,
  intern: boolean,
  write: (w: WriteCursor, value: Obj, table: WriteTable) => void,
): void {
  if (intern) {
    const table: WriteInterner = {
      first: undefined,
      second: undefined,
      entries: undefined,
    };
    const start = w.pos;
    write(w, value, table);
    prependInternTable(w, start, table);
  } else write(w, value, undefined);
}

function encodeCompiledPayload(
  value: unknown,
  version: number | readonly number[],
  intern: boolean,
  write: (w: WriteCursor, value: Obj, table: WriteTable) => void,
): Uint8Array<ArrayBuffer> {
  const w = beginWrite(true);
  if (typeof version === "number") writeByte(w, version);
  else {
    const start = takeWrite(w, version.length);
    w.data.set(version, start);
  }
  const length = takeWrite(w, 4);
  writePayloadWithTable(w, object(value), intern, write);
  const size = w.pos - length - 4;
  if (size > 0xffff_ffff) invalid("Pojoc payload exceeds its envelope");
  writeU32At(w.data, length, size);
  return w.data.subarray(0, w.pos);
}

function encodeFixedPayload(
  value: unknown,
  header: Uint8Array<ArrayBuffer>,
  frameSize: number,
  write: (w: WriteCursor, value: Obj, table: WriteTable) => void,
): Uint8Array<ArrayBuffer> {
  const bytes = new Uint8Array(frameSize);
  bytes.set(header);
  const w: WriteCursor = {
    data: bytes,
    pos: header.length,
    view: undefined,
    fillPresenceInPlace: true,
  };
  write(w, object(value), undefined);
  if (w.pos !== frameSize) invalid("Unexpected Pojoc fixed payload length");
  return bytes;
}

function readFixedRecords<T>(
  r: Cursor,
  count: number,
  width: number,
  read: (r: Cursor, pos: number) => T,
): T[] {
  limitCollection(count);
  let pos = takeBytes(r, count * width);
  const rows: T[] = [];
  for (let i = 0; i < count; i++, pos += width) rows.push(read(r, pos));
  return rows;
}

type FixedRecordView<Row> = {
  readonly length: number;
  get<Key extends keyof Row>(index: number, key: Key): Row[Key];
};

function checkRecordIndex(index: number, count: number): void {
  if (!Number.isInteger(index) || index < 0 || index >= count)
    invalid("Pojoc row index out of bounds");
}

function fixedRecordView(
  r: Cursor,
  count: number,
  width: number,
  read: (r: Cursor, pos: number, key: PropertyKey) => unknown,
): FixedRecordView<Obj> {
  limitCollection(count);
  const start = takeBytes(r, count * width);
  return {
    length: count,
    get(index, key) {
      checkRecordIndex(index, count);
      return read(r, start + index * width, key);
    },
  };
}

function materializedRecordView<Row extends object>(
  rows: readonly Row[],
): FixedRecordView<Row> {
  return {
    length: rows.length,
    get(index, key) {
      checkRecordIndex(index, rows.length);
      const row = rows[index];
      if (!Object.hasOwn(row, key)) invalid("Unknown Pojoc row field");
      return row[key];
    },
  };
}

function readItems<T>(
  r: Cursor,
  count: number,
  table: ReadTable,
  read: ReadItem<T>,
): T[] {
  limitCollection(count);
  const rows: T[] = [];
  for (let i = 0; i < count; i++) rows.push(read(r, table));
  return rows;
}

function readPairs(
  r: Cursor,
  size: number | null,
  table: ReadTable,
  key: ReadItem,
  item: ReadItem,
): Map<unknown, unknown> | [unknown, unknown][] {
  const count = size ?? readLength(r);
  limitCollection(count);
  const pairs: [unknown, unknown][] = [];
  for (let i = 0; i < count; i++) pairs.push([key(r, table), item(r, table)]);
  return size === null ? new Map(pairs) : pairs;
}

function readVariant(
  r: Cursor,
  size: number,
  table: ReadTable,
  kind: string,
  read: ReadItem,
) {
  if (size > r.end - r.pos) invalid("Unexpected end of Pojoc payload");
  const outerEnd = r.end;
  r.end = r.pos + size;
  try {
    const value = read(r, table);
    if (r.pos !== r.end) invalid("Invalid Pojoc union payload length");
    return { kind, value };
  } finally {
    r.end = outerEnd;
  }
}

function writeVariant(
  w: WriteCursor,
  value: unknown,
  table: WriteTable,
  width: number,
  discriminant: unknown,
  write?: WriteItem,
): void {
  if (
    typeof discriminant !== "number" ||
    !Number.isInteger(discriminant) ||
    discriminant < 0
  )
    invalid("Invalid Pojoc union kind");
  if (discriminant >= 2 ** (width * 8))
    invalid("Pojoc union discriminant overflow");
  writeUint(w, width, discriminant);
  if (write) {
    const lengthPos = takeWrite(w, 1);
    const start = w.pos;
    write(w, value, table);
    const end = w.pos;
    const length = end - start;
    if (length > 0xffff_ffff) invalid("Invalid Pojoc length");
    // A one-byte length needs no payload move.
    const extra = varint32Size(length) - 1;
    if (extra !== 0) {
      takeWrite(w, extra);
      w.data.copyWithin(start + extra, start, end);
    }
    writeVarint32(w.data, lengthPos, length);
  } else {
    if (!(value instanceof Uint8Array))
      throw new TypeError("Expected bytes for unknown union variant");
    writeLength(w, value.length);
    writeBytes(w, value);
  }
}

function writeFixedBytes(
  w: WriteCursor,
  value: unknown,
  width: number,
  bitsetWidth?: number,
): void {
  if (!(value instanceof Uint8Array) || value.length !== (bitsetWidth ?? width))
    invalid(
      bitsetWidth === undefined
        ? "Invalid fixed bytes length"
        : "Invalid bitset width",
    );
  writeBytes(w, bitsetWidth === undefined ? value : resized(value, width));
}

function writeVFloat(
  w: WriteCursor,
  value: unknown,
  width: number,
  min: number,
  step: number,
): void {
  if (typeof value !== "number") throw new TypeError("Expected a Pojoc vfloat");
  const steps = Math.round((Math.fround(value) - min) / step);
  writeUint(w, width, Math.min(2 ** (width * 8) - 1, Math.max(0, steps || 0)));
}

function arrayItems(
  w: WriteCursor,
  value: unknown,
  count: number | null,
): ArrayLike<unknown> {
  const items = sequence(value);
  if (count === null) writeLength(w, items.length);
  else if (items.length !== count) invalid("Invalid fixed array length");
  return items;
}

function writePacked(
  w: WriteCursor,
  value: unknown,
  ctor: { new (length: number): Exclude<ArrayBufferView, DataView> },
): boolean {
  if (!littleEndian || !(value instanceof ctor)) return false;
  writeBytes(
    w,
    new Uint8Array(value.buffer, value.byteOffset, value.byteLength),
  );
  return true;
}

function writePairs(
  w: WriteCursor,
  value: unknown,
  table: WriteTable,
  size: number | null,
  key: WriteItem,
  item: WriteItem,
): void {
  let entries: Iterable<[unknown, unknown]>;
  if (size === null) {
    if (!(value instanceof Map)) throw new TypeError("Expected a Pojoc Map");
    writeLength(w, value.size);
    entries = value;
  } else {
    if (!Array.isArray(value) || value.length !== size)
      invalid("Invalid fixed map length");
    entries = value;
  }
  for (const [k, v] of entries) {
    key(w, k, table);
    item(w, v, table);
  }
}

function skipTable(r: Cursor): void {
  const count = tableLength(r);
  for (let i = 0; i < count; i++) takeBytes(r, readLength(r));
}

function skipDelta(r: Cursor, count: number): void {
  limitCollection(count);
  if (count > r.end - r.pos) invalid("Truncated Pojoc delta array");
  for (let i = 0; i < count; i++) readVarint(r);
}

function validateRange(
  value: unknown,
  min: number | null,
  max: number | null,
  field: string,
): void {
  const n =
    typeof value === "string"
      ? textEncoder.encode(value).length
      : value instanceof Map
        ? value.size
        : Array.isArray(value) || ArrayBuffer.isView(value)
          ? sequence(value).length
          : Number(value);
  if ((min !== null && n < min) || (max !== null && n > max))
    invalid(`Constraint violation in ${field}`);
}

function castScalar(value: unknown, fromFloat: boolean, to: string): unknown {
  if (to === "bool") return Boolean(value);
  if (to === "f32") return Math.fround(Number(value));
  if (to === "f64") return Number(value);
  const target = integerKind(to);
  if (!target) return value;
  const [bits, signed] = target;
  let n: bigint;
  if (fromFloat) {
    const minimum = signed ? -(1n << BigInt(bits - 1)) : 0n;
    const maximum = (1n << BigInt(signed ? bits - 1 : bits)) - 1n;
    const number = Number(value);
    if (Number.isNaN(number)) n = 0n;
    else if (number <= Number(minimum)) n = minimum;
    else if (number >= Number(maximum)) n = maximum;
    else n = BigInt(Math.trunc(number));
  } else {
    n = BigInt(value as number | bigint | boolean);
    n = signed ? BigInt.asIntN(bits, n) : BigInt.asUintN(bits, n);
  }
  return bits === 64 ? n : Number(n);
}

function arrayDefault(
  count: number,
  make: (index: number) => unknown,
): unknown[] {
  return Array.from({ length: count }, (_, i) => make(i));
}

function adaptArray(
  value: unknown,
  count: number | null,
  make: () => unknown,
): unknown[] {
  const items =
    value instanceof Map ? Array.from(value.values()) : sequence(value);
  return arrayDefault(count ?? items.length, (i) =>
    i < items.length ? items[i] : make(),
  );
}

function adaptMap(
  value: unknown,
  count: number,
  pad: boolean,
  make: () => [unknown, unknown],
): [unknown, unknown][] {
  const items = (value as [unknown, unknown][]).slice(0, count);
  if (pad) while (items.length < count) items.push(make());
  return items;
}

export type { FixedRecordView, ReadItem, WriteItem, WriteTable };
export {
  object,
  setField,
  versionNumber,
  unsupported,
  finishDecode,
  writePayloadWithTable,
  encodeCompiledPayload,
  encodeFixedPayload,
  readFixedRecords,
  fixedRecordView,
  materializedRecordView,
  readItems,
  readPairs,
  readVariant,
  writeVariant,
  writeFixedBytes,
  writeVFloat,
  arrayItems,
  writePacked,
  writePairs,
  skipTable,
  skipDelta,
  validateRange,
  castScalar,
  arrayDefault,
  adaptArray,
  adaptMap,
};
