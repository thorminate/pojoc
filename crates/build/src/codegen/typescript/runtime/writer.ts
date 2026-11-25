import {
  checkedVarint32,
  invalid,
  textEncoder,
  varint32Size,
  writeVarint32,
} from "./wire.js";

const loneSurrogate = /[\uD800-\uDFFF]/u;
const maxU64 = 0xffff_ffff_ffff_ffffn;

type WriteCursor = {
  data: Uint8Array<ArrayBuffer>;
  pos: number;
  view: DataView | undefined;
  readonly fillPresenceInPlace: boolean;
};

// The map contains all entries in first-occurrence order once promoted.
type WriteInterner = {
  first: string | undefined;
  second: string | undefined;
  entries: Map<string, number> | undefined;
};

// Bytes past pos remain zero until an append uses them, including bitmap slots.
function beginWrite(fillPresenceInPlace: boolean): WriteCursor {
  return {
    data: new Uint8Array(128),
    pos: 0,
    view: undefined,
    fillPresenceInPlace,
  };
}

function writeView(w: WriteCursor): DataView {
  return (w.view ??= new DataView(w.data.buffer));
}

function writeU32At(data: Uint8Array, pos: number, value: number): void {
  data[pos] = value;
  data[pos + 1] = value >>> 8;
  data[pos + 2] = value >>> 16;
  data[pos + 3] = value >>> 24;
}

function reserveWrite(w: WriteCursor, end: number): void {
  if (end > w.data.length) {
    if (end > 0xffff_ffff + 16) invalid("Pojoc payload too large");
    const next = new Uint8Array(
      Math.min(0xffff_ffff + 16, Math.max(end, w.data.length * 2)),
    );
    next.set(w.data);
    w.data = next;
    w.view = undefined;
  }
}
function takeWrite(w: WriteCursor, count: number): number {
  const start = w.pos;
  const end = start + count;
  reserveWrite(w, end);
  w.pos = end;
  return start;
}
function writeBytes(w: WriteCursor, value: Uint8Array): void {
  const start = takeWrite(w, value.length);
  w.data.set(value, start);
}
function writeByte(w: WriteCursor, value: number): void {
  const start = takeWrite(w, 1);
  w.data[start] = value;
}
function writeUint(w: WriteCursor, width: number, value: number): void {
  const start = takeWrite(w, width);
  if (width === 1) w.data[start] = value;
  else if (width === 2) {
    w.data[start] = value;
    w.data[start + 1] = value >>> 8;
  } else writeU32At(w.data, start, value);
}
function writeVarint(w: WriteCursor, value: bigint): void {
  if (value < 0n || value > maxU64) invalid("Expected unsigned 64-bit varint");
  while (value >= 128n) {
    writeByte(w, Number(value & 127n) | 128);
    value >>= 7n;
  }
  writeByte(w, Number(value));
}
function writeLength(w: WriteCursor, value: number): void {
  if (typeof value !== "number" || value !== value >>> 0)
    invalid("Invalid Pojoc length");
  writeU32Varint(w, value);
}
function writeU32Varint(w: WriteCursor, value: number): void {
  if (value < 0x80 && w.pos < w.data.length) {
    w.data[w.pos++] = value;
    return;
  }
  const start = takeWrite(w, varint32Size(value));
  writeVarint32(w.data, start, value);
}
function writeVarint32Array(w: WriteCursor, items: ArrayLike<unknown>): void {
  const count = items.length;
  let pos = w.pos;
  reserveWrite(w, pos + count);
  let data = w.data;
  for (let i = 0; i < count; i++) {
    const value = checkedVarint32(items[i]);
    if (data.length - pos < 5) {
      reserveWrite(w, pos + varint32Size(value));
      data = w.data;
    }
    pos = writeVarint32(data, pos, value);
  }
  w.pos = pos;
}
function writeString(w: WriteCursor, value: unknown): void {
  if (typeof value !== "string") throw new TypeError("Expected a Pojoc string");
  if (value.length < 0x80) {
    const start = w.pos;
    const end = start + 1 + value.length;
    reserveWrite(w, end);
    const data = w.data;
    let at = start + 1;
    for (let i = 0; i < value.length; i++) {
      const byte = value.charCodeAt(i);
      if (byte >= 0x80) break;
      data[at++] = byte;
    }
    if (at === end) {
      data[start] = value.length;
      w.pos = end;
      return;
    }

    // Bound the short UTF-8 write, then fill in its actual length.
    if (loneSurrogate.test(value))
      throw new TypeError("Invalid Unicode string");
    reserveWrite(w, start + 2 + value.length * 3);
    const written = textEncoder.encodeInto(
      value,
      w.data.subarray(start + 1),
    ).written;
    if (written >= 0x80)
      w.data.copyWithin(start + 2, start + 1, start + 1 + written);
    w.pos = writeVarint32(w.data, start, written) + written;
    return;
  }

  // Native encoding is faster than sizing long strings in JavaScript.
  if (loneSurrogate.test(value)) throw new TypeError("Invalid Unicode string");
  const bytes = textEncoder.encode(value);
  writeLength(w, bytes.length);
  writeBytes(w, bytes);
}
function finishWrite(w: WriteCursor): Uint8Array {
  return w.data.subarray(0, w.pos);
}

function finishEnvelope(
  writer: WriteCursor,
  length: number,
): Uint8Array<ArrayBuffer> {
  const bytes = writer.pos - length - 5;
  if (bytes > 0xffff_ffff) invalid("Pojoc payload exceeds its envelope");
  // Place the header next to the payload inside the reserved space.
  const start = 5 - varint32Size(bytes);
  const data = writer.data;
  if (start !== 0) {
    if (length === 1) data[start] = data[0];
    else data.copyWithin(start, 0, length);
  }
  writeVarint32(data, start + length, bytes);
  return data.subarray(start, writer.pos);
}

function prependInternTable(
  w: WriteCursor,
  bodyStart: number,
  table: Map<string, number> | WriteInterner,
): void {
  const bodyEnd = w.pos;
  writeInternTable(w, table);
  if (bodyStart === bodyEnd) return;

  // The table length is known after encoding the body. Move the body in place.
  const prefix = w.data.slice(bodyEnd, w.pos);
  w.data.copyWithin(bodyStart + prefix.length, bodyStart, bodyEnd);
  w.data.set(prefix, bodyStart);
}

function writeInternTable(
  w: WriteCursor,
  table: Map<string, number> | WriteInterner,
): void {
  if (!(table instanceof Map) && !table.entries) {
    const { first, second } = table;
    if (first === undefined) {
      writeByte(w, 0);
    } else {
      writeByte(w, second === undefined ? 1 : 2);
      writeString(w, first);
      if (second !== undefined) writeString(w, second);
    }
    return;
  }
  const entries = table instanceof Map ? table : table.entries!;
  writeLength(w, entries.size);
  for (const text of entries.keys()) writeString(w, text);
}

export type { WriteCursor, WriteInterner };
export {
  beginWrite,
  writeView,
  writeU32At,
  takeWrite,
  writeBytes,
  writeByte,
  writeUint,
  writeVarint,
  writeLength,
  writeU32Varint,
  writeVarint32Array,
  writeString,
  finishWrite,
  finishEnvelope,
  prependInternTable,
};
