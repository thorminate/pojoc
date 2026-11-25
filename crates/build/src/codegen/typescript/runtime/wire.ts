import type { Cursor, CursorOps, Input, ReadTable } from "./types.js";

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });

function invalid(reason: string): never {
  throw new RangeError(reason);
}

function checkedVarint32(value: unknown): number {
  if (typeof value !== "number" || value >>> 0 !== value) {
    invalid("Integer out of range for varint32");
  }
  return value;
}

function varint32Size(value: number): number {
  if (value < 0x80) return 1;
  if (value < 0x4000) return 2;
  if (value < 0x20_0000) return 3;
  if (value < 0x1000_0000) return 4;
  return 5;
}

function writeVarint32(data: Uint8Array, pos: number, value: number): number {
  while (value >= 128) {
    data[pos++] = (value & 127) | 128;
    value >>>= 7;
  }
  data[pos++] = value;
  return pos;
}

function decodeText(data: Uint8Array, start: number, end: number): string {
  if (end - start <= 32) {
    let value = "";
    for (let i = start; i < end; i++) {
      const byte = data[i];
      if (byte >= 0x80) return textDecoder.decode(data.subarray(start, end));
      value += String.fromCharCode(byte);
    }
    return value;
  }
  return textDecoder.decode(data.subarray(start, end));
}

function cursorView(r: Cursor): DataView {
  const data = r.data;
  return (r.view ??= new DataView(
    data.buffer,
    data.byteOffset,
    data.byteLength,
  ));
}

function takeBytes(r: Cursor, count: number): number {
  const start = r.pos;
  if (!Number.isSafeInteger(count) || count < 0 || count > r.end - start) {
    invalid("Unexpected end of Pojoc payload");
  }
  r.pos = start + count;
  return start;
}

function readBytes(r: Cursor, count: number): Uint8Array {
  const start = takeBytes(r, count);
  return r.data.subarray(start, r.pos);
}

function readByte(r: Cursor): number {
  return r.data[takeBytes(r, 1)];
}

function readUint(r: Cursor, width: number): number {
  const start = takeBytes(r, width);
  if (width === 1) return r.data[start];
  const data = r.data;
  const low = data[start] | (data[start + 1] << 8);
  if (width === 2) return low;
  return (low | (data[start + 2] << 16) | (data[start + 3] << 24)) >>> 0;
}

function readVarint(r: Cursor): bigint {
  let byte = readByte(r);
  if (byte < 128) return BigInt(byte);
  let value = BigInt(byte & 127);
  for (let shift = 7n; shift <= 63n; shift += 7n) {
    byte = readByte(r);
    if (shift === 63n && byte > 1) invalid("Pojoc varint overflow");
    value |= BigInt(byte & 127) << shift;
    if (byte < 128) return value;
  }
  return invalid("Pojoc varint overflow");
}

function readVarint32(r: Cursor, extendedLength = false): number {
  const { data, end } = r;
  let pos = r.pos;
  if (pos >= end) invalid("Unexpected end of Pojoc payload");
  const first = data[pos++];
  if (first < 128) {
    r.pos = pos;
    return first;
  }
  if (end - pos < 4) return readVarint32Slow(r, extendedLength);

  let value = first & 127;
  let byte = data[pos++];
  value |= (byte & 127) << 7;
  if (byte >= 128) {
    byte = data[pos++];
    value |= (byte & 127) << 14;
    if (byte >= 128) {
      byte = data[pos++];
      value |= (byte & 127) << 21;
      if (byte >= 128) {
        byte = data[pos++];
        if (byte > 15) {
          if (extendedLength) return readVarint32Slow(r, true);
          invalid("Pojoc varint32 overflow");
        }
        value |= byte << 28;
      }
    }
  }
  r.pos = pos;
  return value >>> 0;
}

function readVarint32Slow(r: Cursor, extendedLength: boolean): number {
  const first = readByte(r);
  let value = first & 127;
  for (let shift = 7; shift <= 28; shift += 7) {
    const byte = readByte(r);
    const bits = byte & 127;
    if (shift === 28 && bits > 15) invalid("Pojoc varint32 overflow");
    value |= bits << shift;
    if (byte < 128) return value >>> 0;
  }
  // Lengths are encoded as u64 on the wire, but this target limits them to u32.
  if (extendedLength) {
    for (let i = 5; i < 10; i++) {
      const byte = readByte(r);
      if (byte === 0) return value >>> 0;
      if (byte !== 128) break;
    }
  }
  return invalid("Pojoc varint32 overflow");
}

function readLength(r: Cursor): number {
  return readVarint32(r, true);
}

function readVarint32Array(r: Cursor, count: number): number[] {
  if (count > r.end - r.pos) invalid("Unexpected end of Pojoc payload");
  const result: number[] = [];
  for (let i = 0; i < count; i++) result.push(readVarint32(r));
  return result;
}

function readString(r: Cursor): string {
  const count = readLength(r);
  const start = r.pos;
  const end = start + count;
  if (end > r.end) invalid("Unexpected end of Pojoc payload");
  r.pos = end;
  return decodeText(r.data, start, end);
}

function readIntern(r: Cursor, table: ReadTable): string {
  const index = readLength(r);
  if (index >= table.length) invalid("Invalid Pojoc intern index");
  const entry = table[index];
  return typeof entry === "string"
    ? entry
    : resolveIntern(r, table, index, entry);
}

function resolveIntern(
  r: Cursor,
  table: ReadTable,
  index: number,
  entry: number,
): string {
  const pos = r.pos;
  r.pos = entry;
  try {
    const value = readString(r);
    table[index] = value;
    return value;
  } finally {
    r.pos = pos;
  }
}

const cursorReads: CursorOps = {
  take: takeBytes,
  view: cursorView,
  varint32: readVarint32,
  varint: readVarint,
  string: readString,
  intern: readIntern,
};

function limitCollection(count: number): void {
  // Variable-size values can consume zero bytes, so byte bounds alone cannot
  // rule out a huge number of objects in an otherwise tiny envelope.
  if (count > 16_777_216)
    invalid("Pojoc collection exceeds the object decode limit");
}

function tableLength(r: Cursor): number {
  const count = readLength(r);
  if (count > r.end - r.pos) invalid("Invalid Pojoc intern table length");
  limitCollection(count);
  return count;
}

function readTable(r: Cursor): ReadTable {
  const count = tableLength(r);
  const table: string[] = [];
  for (let i = 0; i < count; i++) table.push(readString(r));
  return table;
}

function readTableOffsets(r: Cursor): ReadTable {
  const count = tableLength(r);
  if (count === 0) return emptyTable;
  const table: number[] = [];
  for (let i = 0; i < count; i++) {
    table.push(r.pos);
    takeBytes(r, readLength(r));
  }
  return table;
}

const emptyTable: ReadTable = [];

type Envelope = Cursor & { version: bigint };

function openEnvelope(
  input: Input,
  offset: number,
  current: bigint,
  shortVersion: number,
): Envelope {
  const data = input instanceof Uint8Array ? input : new Uint8Array(input);
  if (!Number.isSafeInteger(offset) || offset < 0 || offset > data.length)
    invalid("Invalid Pojoc offset");
  const currentVersion = data[offset] === shortVersion;
  const firstLength = data[offset + 1];
  if (currentVersion && firstLength < 128) {
    const pos = offset + 2;
    const size = firstLength;
    if (size > data.length - pos) invalid("Unexpected end of Pojoc payload");
    return { data, view: undefined, pos, end: pos + size, version: current };
  }
  const envelope = {
    data,
    view: undefined,
    pos: currentVersion ? offset + 1 : offset,
    end: data.length,
    version: current,
  };
  if (!currentVersion) envelope.version = readVarint(envelope);
  const size = readVarint32(envelope);
  if (size > envelope.end - envelope.pos)
    invalid("Unexpected end of Pojoc payload");
  envelope.end = envelope.pos + size;
  return envelope;
}

function openSchemaEnvelope(
  input: Input,
  offset: number,
  current: bigint,
  shortVersion: number,
  currentSize: number | undefined,
  fixedSize: (version: bigint) => number | undefined,
): Envelope {
  const data = input instanceof Uint8Array ? input : new Uint8Array(input);
  if (!Number.isSafeInteger(offset) || offset < 0 || offset > data.length)
    invalid("Invalid Pojoc offset");
  const latest = data[offset] === shortVersion;
  const r = {
    data,
    view: undefined,
    pos: latest ? offset + 1 : offset,
    end: data.length,
    version: current,
  };
  if (!latest) r.version = readVarint(r);
  const size = (latest ? currentSize : fixedSize(r.version)) ?? readUint(r, 4);
  if (size > r.end - r.pos) invalid("Unexpected end of Pojoc payload");
  r.end = r.pos + size;
  return r;
}

export type { Envelope };
export {
  textEncoder,
  invalid,
  checkedVarint32,
  varint32Size,
  writeVarint32,
  cursorView,
  takeBytes,
  readBytes,
  readByte,
  readUint,
  readVarint,
  readVarint32,
  readLength,
  readVarint32Array,
  readString,
  readIntern,
  cursorReads,
  limitCollection,
  tableLength,
  readTable,
  readTableOffsets,
  emptyTable,
  openEnvelope,
  openSchemaEnvelope,
};
