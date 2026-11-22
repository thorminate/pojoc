type Primitive = "u8" | "i8" | "u16" | "i16" | "u32" | "i32" | "u64" | "i64"
  | "f32" | "f64" | "bool" | "string" | "varint32" | "varint64" | "intern";
type Variant = [number, string, WireType];
type WireType = Primitive
  | ["record", string, string]
  | ["enum", number, number[], number]
  | ["bits", number, number]
  | ["union", number, Variant[], Variant[]]
  | ["array" | "delta", WireType, number | null]
  | ["map", WireType, WireType, number | null]
  | ["tuple", WireType[]]
  | ["optional" | "box", WireType]
  | ["constraint", WireType, number | null, number | null]
  | ["vfloat", number, number, number]
  | ["bytes", number]
  | ["import", string, string];
type Field = { id: string; name: string; ty: WireType; default: unknown; seek: boolean; lazy: boolean; cast: boolean };
type RootSpec = { fields: Field[]; intern: boolean };
type SchemaSpec = {
  latest: string;
  roots: Record<string, RootSpec>;
  records: Record<string, Field[]>;
  imports: Record<string, number>;
};
type Obj = Record<string, unknown>;
type Input = Uint8Array | ArrayBuffer;
type Version = number | bigint;
type NumericArray = Uint8Array | Int8Array | Uint16Array | Int16Array | Uint32Array
  | Int32Array | BigUint64Array | BigInt64Array | Float32Array | Float64Array;
type NumericConstructor = {
  readonly BYTES_PER_ELEMENT: number;
  new (length: number): NumericArray;
  new (buffer: ArrayBufferLike, offset: number, length: number): NumericArray;
};

const packed: Partial<Record<Primitive, NumericConstructor>> = {
  u8: Uint8Array, i8: Int8Array, u16: Uint16Array, i16: Int16Array,
  u32: Uint32Array, i32: Int32Array, u64: BigUint64Array, i64: BigInt64Array,
  f32: Float32Array, f64: Float64Array,
};
const littleEndian = new Uint8Array(new Uint16Array([1]).buffer)[0] === 1;
const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
const loneSurrogate = /[\uD800-\uDFFF]/u;
const maxU64 = 0xffff_ffff_ffff_ffffn;

function invalid(reason: string): never {
  throw new RangeError(reason);
}

function checkedVarint32(value: unknown): number {
  if (typeof value !== "number" || (value >>> 0) !== value) {
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

class Reader {
  readonly data: Uint8Array;
  readonly view: DataView;
  pos = 0;
  constructor(data: Uint8Array) {
    this.data = data;
    this.view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  }
  take(count: number): number {
    if (!Number.isSafeInteger(count) || count < 0 || count > this.data.length - this.pos) {
      invalid("Unexpected end of Pojoc payload");
    }
    const start = this.pos;
    this.pos += count;
    return start;
  }
  bytes(count: number): Uint8Array {
    const start = this.take(count);
    return this.data.subarray(start, this.pos);
  }
  byte(): number { return this.view.getUint8(this.take(1)); }
  uint(width: number): number {
    const start = this.take(width);
    if (width === 1) return this.view.getUint8(start);
    if (width === 2) return this.view.getUint16(start, true);
    return this.view.getUint32(start, true);
  }
  varint(): bigint {
    let byte = this.byte();
    if (byte < 128) return BigInt(byte);
    let value = BigInt(byte & 127);
    for (let shift = 7n; shift <= 63n; shift += 7n) {
      byte = this.byte();
      if (shift === 63n && byte > 1) invalid("Pojoc varint overflow");
      value |= BigInt(byte & 127) << shift;
      if (byte < 128) return value;
    }
    return invalid("Pojoc varint overflow");
  }
  varint32(extendedLength = false): number {
    const data = this.data;
    let pos = this.pos;
    if (pos >= data.length) invalid("Unexpected end of Pojoc payload");
    const first = data[pos++];
    if (first < 128) {
      this.pos = pos;
      return first;
    }
    if (data.length - pos < 4) return this.varint32Slow(extendedLength);

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
            if (extendedLength) return this.varint32Slow(true);
            invalid("Pojoc varint32 overflow");
          }
          value |= byte << 28;
        }
      }
    }
    this.pos = pos;
    return value >>> 0;
  }
  private varint32Slow(extendedLength: boolean): number {
    const first = this.byte();
    let value = first & 127;
    for (let shift = 7; shift <= 28; shift += 7) {
      const byte = this.byte();
      const bits = byte & 127;
      if (shift === 28 && bits > 15) invalid("Pojoc varint32 overflow");
      value |= bits << shift;
      if (byte < 128) return value >>> 0;
    }
    // Lengths are encoded as u64 on the wire, but this target limits them to u32.
    if (extendedLength) {
      for (let i = 5; i < 10; i++) {
        const byte = this.byte();
        if (byte === 0) return value >>> 0;
        if (byte !== 128) break;
      }
    }
    return invalid("Pojoc varint32 overflow");
  }
  length(): number { return this.varint32(true); }
  varint32Array(count: number): number[] {
    if (count > this.data.length - this.pos) invalid("Unexpected end of Pojoc payload");
    const result: number[] = [];
    for (let i = 0; i < count; i++) result.push(this.varint32());
    return result;
  }
  string(): string { return textDecoder.decode(this.bytes(this.length())); }
}

class Writer {
  data = new Uint8Array(128);
  view = new DataView(this.data.buffer);
  pos = 0;
  private reserve(end: number): void {
    if (end > 0xffff_ffff + 16) invalid("Pojoc payload too large");
    if (end > this.data.length) {
      const next = new Uint8Array(Math.min(0xffff_ffff + 16, Math.max(end, this.data.length * 2)));
      next.set(this.data);
      this.data = next;
      this.view = new DataView(next.buffer);
    }
  }
  take(count: number): number {
    const start = this.pos;
    const end = start + count;
    this.reserve(end);
    this.pos = end;
    return start;
  }
  bytes(value: Uint8Array): void {
    const start = this.take(value.length);
    this.data.set(value, start);
  }
  byte(value: number): void {
    const start = this.take(1);
    this.view.setUint8(start, value);
  }
  uint(width: number, value: number): void {
    const start = this.take(width);
    if (width === 1) this.view.setUint8(start, value);
    else if (width === 2) this.view.setUint16(start, value, true);
    else this.view.setUint32(start, value, true);
  }
  varint(value: bigint): void {
    if (value < 0n || value > maxU64) invalid("Expected unsigned 64-bit varint");
    while (value >= 128n) {
      this.byte(Number(value & 127n) | 128);
      value >>= 7n;
    }
    this.byte(Number(value));
  }
  length(value: number): void {
    if (!Number.isSafeInteger(value) || value < 0 || value > 0xffff_ffff) invalid("Invalid Pojoc length");
    this.varint32(value);
  }
  varint32(value: number): void {
    const start = this.take(varint32Size(value));
    writeVarint32(this.data, start, value);
  }
  varint32Array(items: ArrayLike<unknown>): void {
    const count = items.length;
    let pos = this.pos;
    this.reserve(pos + count);
    let data = this.data;
    for (let i = 0; i < count; i++) {
      const value = checkedVarint32(items[i]);
      if (data.length - pos < 5) {
        this.reserve(pos + varint32Size(value));
        data = this.data;
      }
      pos = writeVarint32(data, pos, value);
    }
    this.pos = pos;
  }
  string(value: unknown): void {
    if (typeof value !== "string") throw new TypeError("Expected a Pojoc string");
    if (loneSurrogate.test(value)) throw new TypeError("Invalid Unicode string");
    const bytes = textEncoder.encode(value);
    this.length(bytes.length);
    this.bytes(bytes);
  }
  finish(): Uint8Array { return this.data.subarray(0, this.pos); }
}

function arrayOf(ty: WireType): NumericConstructor | undefined {
  return typeof ty === "string" ? packed[ty] : undefined;
}

function readPacked(r: Reader, ctor: NumericConstructor, count: number): NumericArray {
  const width = ctor.BYTES_PER_ELEMENT;
  const bytes = r.bytes(count * width);
  if ((littleEndian || width === 1) && bytes.byteOffset % width === 0) {
    return new ctor(bytes.buffer, bytes.byteOffset, count);
  }
  const array = new ctor(count);
  const copy = new Uint8Array(array.buffer);
  copy.set(bytes);
  if (!littleEndian && width > 1) {
    for (let i = 0; i < copy.length; i += width) copy.subarray(i, i + width).reverse();
  }
  return array;
}

function integerKind(ty: WireType): [number, boolean] | undefined {
  if (ty === "varint32") return [32, false];
  if (ty === "varint64") return [64, false];
  if (typeof ty !== "string" || (ty[0] !== "u" && ty[0] !== "i")) return;
  if (ty === "intern") return;
  return [Number(ty.slice(1)), ty[0] === "i"];
}

function integer(ty: WireType, value: unknown): bigint {
  const [bits, signed] = integerKind(ty)!;
  if (bits === 64 ? typeof value !== "bigint" : typeof value !== "number" || !Number.isInteger(value)) {
    throw new TypeError(`Expected ${bits === 64 ? "bigint" : "integer"} for ${ty}`);
  }
  const n = BigInt(value as number | bigint);
  const wrapped = signed ? BigInt.asIntN(bits, n) : BigInt.asUintN(bits, n);
  if (n !== wrapped) invalid(`Integer out of range for ${ty}`);
  return n;
}

function readScalar(r: Reader, ty: Primitive, table: string[]): unknown {
  switch (ty) {
    case "u8": return r.byte();
    case "i8": return r.view.getInt8(r.take(1));
    case "u16": return r.uint(2);
    case "i16": return r.view.getInt16(r.take(2), true);
    case "u32": return r.uint(4);
    case "i32": return r.view.getInt32(r.take(4), true);
    case "u64": return r.view.getBigUint64(r.take(8), true);
    case "i64": return r.view.getBigInt64(r.take(8), true);
    case "f32": return r.view.getFloat32(r.take(4), true);
    case "f64": return r.view.getFloat64(r.take(8), true);
    case "bool": return r.byte() !== 0;
    case "string": return r.string();
    case "varint32": return r.varint32();
    case "varint64": return r.varint();
    case "intern": {
      const index = r.length();
      if (index >= table.length) invalid("Invalid Pojoc intern index");
      return table[index];
    }
  }
}

function writeScalar(w: Writer, ty: Primitive, value: unknown, table: Map<string, number>): void {
  switch (ty) {
    case "string": w.string(value); return;
    case "varint32": w.varint32(checkedVarint32(value)); return;
    case "intern": {
      if (typeof value !== "string") throw new TypeError("Expected an interned string");
      let index = table.get(value);
      if (index === undefined) {
        index = table.size;
        table.set(value, index);
      }
      w.length(index);
      return;
    }
    case "bool": {
      if (typeof value !== "boolean") throw new TypeError("Expected a Pojoc boolean");
      w.byte(Number(value));
      return;
    }
    case "f32":
    case "f64": {
      if (typeof value !== "number") throw new TypeError("Expected a Pojoc number");
      const start = w.take(ty === "f32" ? 4 : 8);
      if (ty === "f32") w.view.setFloat32(start, value, true);
      else w.view.setFloat64(start, value, true);
      return;
    }
    default: {
      const [bits, signed] = integerKind(ty)!;
      if (bits < 64) {
        const min = signed ? -(2 ** (bits - 1)) : 0;
        const max = (2 ** (signed ? bits - 1 : bits)) - 1;
        if (typeof value !== "number" || !Number.isInteger(value) || value < min || value > max) {
          invalid(`Integer out of range for ${ty}`);
        }
        w.uint(bits / 8, value);
        return;
      }
      const n = integer(ty, value);
      if (ty === "varint64") w.varint(n);
      else {
        const start = w.take(8);
        w.view.setBigUint64(start, n, true);
      }
    }
  }
}

function sequence(value: unknown): ArrayLike<unknown> {
  if (Array.isArray(value) || (ArrayBuffer.isView(value) && !(value instanceof DataView))) {
    return value as ArrayLike<unknown>;
  }
  throw new TypeError("Expected a Pojoc array");
}

function typedValues(ty: WireType, items: unknown[]): unknown[] | NumericArray {
  const ctor = arrayOf(ty);
  if (!ctor) return items;
  const target = new ctor(items.length);
  const writable = target as { [index: number]: unknown };
  items.forEach((item, i) => { writable[i] = item; });
  return target;
}

function unsignedBytes(value: bigint, width: number): Uint8Array {
  const bytes = new Uint8Array(width);
  for (let n = 0; n < width; n++, value >>= 8n) bytes[n] = Number(value & 255n);
  return bytes;
}

function bytesAsInt(bytes: Uint8Array): bigint {
  let value = 0n;
  for (let i = Math.min(bytes.length, 16) - 1; i >= 0; i--) value = (value << 8n) | BigInt(bytes[i]);
  return value;
}

function limitCollection(count: number): void {
  // Variable-size values can consume zero bytes, so byte bounds alone cannot
  // rule out a huge number of objects in an otherwise tiny envelope.
  if (count > 16_777_216) invalid("Pojoc collection exceeds the object decode limit");
}

function signedVarint(r: Reader): bigint {
  const value = r.varint();
  return (value >> 1n) ^ -(value & 1n);
}

function signedDelta32(r: Reader): number {
  if (r.pos >= r.data.length) invalid("Unexpected end of Pojoc payload");
  const byte = r.data[r.pos];
  if (byte < 128) {
    r.pos++;
    return (byte >>> 1) ^ -(byte & 1);
  }
  // Only the low 32 bits can affect the reconstructed element.
  return Number(BigInt.asIntN(32, signedVarint(r)));
}

function writeSignedVarint(w: Writer, value: bigint): void {
  const wrapped = BigInt.asIntN(64, value);
  w.varint((wrapped << 1n) ^ (wrapped >> 63n));
}

function decodeDelta(r: Reader, ty: WireType, count: number): unknown[] | NumericArray {
  limitCollection(count);
  if (count > r.data.length - r.pos) invalid("Truncated Pojoc delta array");
  const [bits, signed] = integerKind(ty)!;
  if (bits < 64) {
    const items = new (arrayOf(ty)!)(count);
    if (count === 0) return items;
    let previous = signed ? signedDelta32(r) : r.varint32();
    items[0] = previous;
    for (let i = 1; i < count; i++) {
      previous = (previous + signedDelta32(r)) | 0;
      items[i] = previous;
    }
    return items;
  }
  const wrap = signed ? BigInt.asIntN : BigInt.asUintN;
  const items = new (signed ? BigInt64Array : BigUint64Array)(count);
  let previous = 0n;
  for (let i = 0; i < count; i++) {
    const next = i ? previous + signedVarint(r) : signed ? signedVarint(r) : r.varint();
    previous = wrap(64, next);
    items[i] = previous;
  }
  return items;
}

function encodeDelta(w: Writer, ty: WireType, value: ArrayLike<unknown>): void {
  const [bits, signed] = integerKind(ty)!;
  if (bits < 64) {
    const bound = 2 ** (signed ? bits - 1 : bits);
    const min = signed ? -bound : 0;
    let previous = 0;
    for (let i = 0; i < value.length; i++) {
      const next = value[i];
      if (typeof next !== "number" || !Number.isInteger(next)) throw new TypeError(`Expected integer for ${ty}`);
      if (next < min || next >= bound) invalid(`Integer out of range for ${ty}`);
      const delta = next - previous;
      const zigzag = delta < 0 ? -2 * delta - 1 : 2 * delta;
      const encoded = i === 0 && !signed ? next : zigzag;
      // A jump across the full 32-bit range can need 33 ZigZag bits.
      if (encoded <= 0xffff_ffff) w.varint32(encoded);
      else w.varint(BigInt(encoded));
      previous = next;
    }
    return;
  }
  let previous = 0n;
  for (let i = 0; i < value.length; i++) {
    const next = integer(ty, value[i]);
    if (i || signed) writeSignedVarint(w, next - previous);
    else w.varint(next);
    previous = next;
  }
}

function resized(bytes: Uint8Array, width: number): Uint8Array {
  if (bytes.length === width) return bytes;
  const result = new Uint8Array(width);
  result.set(bytes.subarray(0, width));
  return result;
}

function object(value: unknown): Obj {
  if (typeof value !== "object" || value === null) throw new TypeError("Expected a Pojoc record");
  return value as Obj;
}

function setField(target: Obj, name: string, value: unknown): void {
  if (name === "__proto__") {
    Object.defineProperty(target, name, { value, enumerable: true, writable: true, configurable: true });
  } else target[name] = value;
}

function isOptional(ty: WireType): ty is ["optional", WireType] {
  return typeof ty !== "string" && ty[0] === "optional";
}

function headerLength(fields: Field[]): number {
  let count = 0;
  for (const field of fields) if (isOptional(field.ty)) count++;
  return Math.ceil(count / 8);
}

function readValue(
  r: Reader, ty: WireType, schemas: SchemaSpec[], scope: number, table: string[], historical = false,
): unknown {
  if (typeof ty === "string") return readScalar(r, ty, table);
  const schema = schemas[scope];
  switch (ty[0]) {
    case "bytes": return r.bytes(ty[1]);
    case "bits": return resized(r.bytes(ty[1]), ty[2]);
    case "enum": {
      const value = r.uint(ty[1]);
      if (value !== 0 && !ty[2].includes(value)) invalid("Invalid Pojoc enum discriminant");
      return value;
    }
    case "union": {
      const discriminant = r.uint(ty[1]);
      const payload = r.bytes(r.length());
      const variant = ty[2].find(v => v[0] === discriminant);
      if (!variant) return { kind: "Unknown", discriminant, data: payload };
      const inner = new Reader(payload);
      const value = readValue(inner, variant[2], schemas, scope, table);
      if (inner.pos !== payload.length) invalid("Invalid Pojoc union payload length");
      validate(variant[2], value, variant[1]);
      return { kind: variant[1], value };
    }
    case "record": {
      const fields = schema.records[historical ? ty[1] : ty[2]];
      return readFields(r, fields, schemas, scope, table, !historical);
    }
    case "import": return readPayload(r, schemas, schema.imports[ty[1]], ty[2]);
    case "optional": return r.byte() ? readValue(r, ty[1], schemas, scope, table, historical) : null;
    case "constraint":
    case "box": return readValue(r, ty[1], schemas, scope, table, historical);
    case "vfloat": return Math.fround(ty[1] + r.uint(ty[3]) * ty[2]);
    case "tuple": return ty[1].map(item => readValue(r, item, schemas, scope, table));
    case "delta": return decodeDelta(r, ty[1], ty[2] ?? r.length());
    case "array": {
      const count = ty[2] ?? r.length();
      const ctor = arrayOf(ty[1]);
      if (ctor) return readPacked(r, ctor, count);
      limitCollection(count);
      if (ty[1] === "varint32") return r.varint32Array(count);
      const result = [];
      for (let i = 0; i < count; i++) result.push(readValue(r, ty[1], schemas, scope, table));
      return result;
    }
    case "map": {
      const count = ty[3] ?? r.length();
      limitCollection(count);
      const entries: [unknown, unknown][] = [];
      for (let i = 0; i < count; i++) {
        const key = readValue(r, ty[1], schemas, scope, table);
        const value = readValue(r, ty[2], schemas, scope, table);
        entries.push([key, value]);
      }
      return ty[3] === null ? new Map(entries) : entries;
    }
  }
}

function writeValue(
  w: Writer, ty: WireType, value: unknown, schemas: SchemaSpec[], scope: number,
  table: Map<string, number>, historical = false,
): void {
  if (typeof ty === "string") return writeScalar(w, ty, value, table);
  const schema = schemas[scope];
  switch (ty[0]) {
    case "bytes":
      if (!(value instanceof Uint8Array) || value.length !== ty[1]) invalid("Invalid fixed bytes length");
      w.bytes(value);
      return;
    case "bits":
      if (!(value instanceof Uint8Array) || value.length !== ty[2]) invalid("Invalid bitset width");
      w.bytes(resized(value, ty[1]));
      return;
    case "enum":
      if (typeof value !== "number" || (value !== 0 && !ty[2].includes(value))) invalid("Invalid enum value");
      w.uint(ty[1], value <= ty[3] ? value : 0);
      return;
    case "union": {
      const item = object(value);
      const variant = ty[3].find(v => v[1] === item.kind);
      const discriminant = (item.kind === "Unknown" ? item.discriminant : variant?.[0]) as number;
      if (!Number.isInteger(discriminant) || discriminant < 0) invalid("Invalid Pojoc union kind");
      if (discriminant >= 2 ** (ty[1] * 8)) invalid("Pojoc union discriminant overflow");
      let payload: Uint8Array;
      if (variant) {
        validate(variant[2], item.value, variant[1]);
        const nested = new Writer();
        writeValue(nested, variant[2], item.value, schemas, scope, table);
        payload = nested.finish();
      } else {
        if (!(item.data instanceof Uint8Array)) throw new TypeError("Expected bytes for unknown union variant");
        payload = item.data;
      }
      w.uint(ty[1], discriminant);
      w.length(payload.length);
      w.bytes(payload);
      return;
    }
    case "record": {
      const fields = schema.records[historical ? ty[1] : ty[2]];
      writeFields(w, fields, object(value), schemas, scope, table, !historical);
      return;
    }
    case "import": writePayload(w, schemas, schema.imports[ty[1]], ty[2], object(value)); return;
    case "optional":
      w.byte(value == null ? 0 : 1);
      if (value != null) writeValue(w, ty[1], value, schemas, scope, table, historical);
      return;
    case "constraint":
    case "box": return writeValue(w, ty[1], value, schemas, scope, table, historical);
    case "vfloat": {
      if (typeof value !== "number") throw new TypeError("Expected a Pojoc vfloat");
      const steps = Math.round((Math.fround(value) - ty[1]) / ty[2]);
      w.uint(ty[3], Math.min(2 ** (ty[3] * 8) - 1, Math.max(0, steps || 0)));
      return;
    }
    case "tuple": {
      const items = sequence(value);
      if (items.length !== ty[1].length) invalid("Invalid Pojoc tuple length");
      ty[1].forEach((kind, i) => writeValue(w, kind, items[i], schemas, scope, table));
      return;
    }
    case "array":
    case "delta": {
      const items = sequence(value);
      if (ty[2] === null) w.length(items.length);
      else if (items.length !== ty[2]) invalid("Invalid fixed array length");
      if (ty[0] === "delta") return encodeDelta(w, ty[1], items);
      if (ty[1] === "varint32") return w.varint32Array(items);
      const ctor = arrayOf(ty[1]);
      if (littleEndian && ctor && value instanceof ctor) {
        w.bytes(new Uint8Array(value.buffer, value.byteOffset, value.byteLength));
      } else {
        for (let i = 0; i < items.length; i++) writeValue(w, ty[1], items[i], schemas, scope, table);
      }
      return;
    }
    case "map": {
      let entries: Iterable<[unknown, unknown]>;
      if (ty[3] === null) {
        if (!(value instanceof Map)) throw new TypeError("Expected a Pojoc Map");
        w.length(value.size);
        entries = value;
      } else {
        if (!Array.isArray(value) || value.length !== ty[3]) invalid("Invalid fixed map length");
        entries = value;
      }
      for (const [key, item] of entries) {
        writeValue(w, ty[1], key, schemas, scope, table);
        writeValue(w, ty[2], item, schemas, scope, table);
      }
      return;
    }
  }
}

function validate(ty: WireType, value: unknown, field: string): void {
  if (typeof ty === "string") return;
  if (ty[0] === "optional" && value != null) return validate(ty[1], value, field);
  if (ty[0] !== "constraint") return;
  const [, , min, max] = ty;
  const n = typeof value === "string" ? textEncoder.encode(value).length
    : value instanceof Map ? value.size
    : Array.isArray(value) || ArrayBuffer.isView(value) ? sequence(value).length
    : Number(value);
  if ((min !== null && n < min) || (max !== null && n > max)) invalid(`Constraint violation in ${field}`);
}

function readFields(
  r: Reader, fields: Field[], schemas: SchemaSpec[], scope: number, table: string[], checks: boolean,
): Obj {
  const header = r.bytes(headerLength(fields));
  let bit = 0;
  const result: Obj = {};
  for (const field of fields) {
    let value: unknown;
    if (isOptional(field.ty)) {
      const present = header[bit >> 3] & 1 << (bit & 7);
      bit++;
      value = present ? readValue(r, field.ty[1], schemas, scope, table, field.cast) : null;
    } else {
      value = readValue(r, field.ty, schemas, scope, table, field.cast);
    }
    if (checks) validate(field.ty, value, field.name);
    setField(result, field.name, value);
  }
  return result;
}

function writeFields(
  w: Writer, fields: Field[], value: Obj, schemas: SchemaSpec[], scope: number,
  table: Map<string, number>, checks: boolean,
): void {
  const header = new Uint8Array(headerLength(fields));
  let bit = 0;
  for (const field of fields) {
    if (isOptional(field.ty)) {
      if (value[field.name] != null) header[bit >> 3] |= 1 << (bit & 7);
      bit++;
    }
  }
  w.bytes(header);
  for (const field of fields) {
    const item = value[field.name];
    if (checks) validate(field.ty, item, field.name);
    if (isOptional(field.ty)) {
      if (item != null) writeValue(w, field.ty[1], item, schemas, scope, table, field.cast);
    } else {
      writeValue(w, field.ty, item, schemas, scope, table, field.cast);
    }
  }
}

function defaultValue(ty: WireType, supplied: unknown, schemas: SchemaSpec[], scope: number): unknown {
  const schema = schemas[scope];
  const plan = Array.isArray(supplied) ? supplied : [];
  if (typeof ty === "string") {
    if (ty === "string" || ty === "intern") return supplied ?? "";
    if (ty === "bool") return supplied ?? false;
    const value = plan[0] === "bytes" ? bytesAsInt(Uint8Array.from(plan[1])) : plan[0] === "integer" ? plan[1] : supplied ?? 0;
    if (integerKind(ty)?.[0] === 64) return BigInt(value);
    return ty === "f32" ? Math.fround(Number(value)) : Number(value);
  }
  switch (ty[0]) {
    case "optional": return supplied == null ? null : defaultValue(ty[1], supplied, schemas, scope);
    case "box":
    case "constraint": return defaultValue(ty[1], supplied, schemas, scope);
    case "bytes":
    case "bits": {
      const width = ty[0] === "bits" ? ty[2] : ty[1];
      if (plan[0] === "bytes") return resized(Uint8Array.from(plan[1]), width);
      if (plan[0] === "integer") return unsignedBytes(BigInt(plan[1]), width);
      return new Uint8Array(width);
    }
    case "enum":
    case "vfloat": return Number(plan[0] === "integer" ? plan[1] : supplied ?? 0);
    case "union": {
      const first = ty[3][0];
      if (first === undefined) return { kind: "Unknown", discriminant: 0, data: new Uint8Array() };
      return { kind: first[1], value: defaultValue(first[2], null, schemas, scope) };
    }
    case "record": return defaults(schema.records[ty[2]], {}, schemas, scope);
    case "import": {
      const imported = schema.imports[ty[1]];
      const target = schemas[imported];
      return defaults(target.roots[target.latest].fields, {}, schemas, imported);
    }
    case "tuple": return ty[1].map((item, i) => defaultValue(item, plan[1]?.[i], schemas, scope));
    case "array":
    case "delta": {
      const items = plan[0] === "items" ? plan[1] as unknown[] : [];
      const result = Array.from({ length: ty[2] ?? items.length }, (_, i) =>
        defaultValue(ty[1], plan[0] === "repeat" ? plan[1] : items[i], schemas, scope));
      return typedValues(ty[1], result);
    }
    case "map": {
      const items = (plan[0] === "pairs" ? plan[1] : []) as [unknown, unknown][];
      const result = items.map(([key, value]): [unknown, unknown] => [
        defaultValue(ty[1], key, schemas, scope), defaultValue(ty[2], value, schemas, scope),
      ]);
      if (ty[3] === null) return new Map(result);
      while (result.length < ty[3]) {
        result.push([defaultValue(ty[1], null, schemas, scope), defaultValue(ty[2], null, schemas, scope)]);
      }
      return result.slice(0, ty[3]);
    }
  }
}

function defaults(fields: Field[], value: Obj, schemas: SchemaSpec[], scope: number): Obj {
  const result: Obj = {};
  for (const field of fields) {
    setField(result, field.name, Object.prototype.hasOwnProperty.call(value, field.name)
      ? value[field.name] : defaultValue(field.ty, field.default, schemas, scope));
  }
  return result;
}

function castScalar(value: unknown, from: WireType, to: WireType): unknown {
  if (to === "bool") return Boolean(value);
  if (to === "f32") return Math.fround(Number(value));
  if (to === "f64") return Number(value);
  const target = integerKind(to);
  if (!target) return value;
  const [bits, signed] = target;
  let n: bigint;
  if (from === "f32" || from === "f64" || (typeof from !== "string" && from[0] === "vfloat")) {
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

function adapt(
  value: unknown, from: WireType, to: WireType, schemas: SchemaSpec[], scope: number, down: boolean,
): unknown {
  if (value == null) return defaultValue(to, null, schemas, scope);
  if (typeof from !== "string" && (from[0] === "optional" || from[0] === "box" || from[0] === "constraint")) {
    return adapt(value, from[1], to, schemas, scope, down);
  }
  if (typeof to !== "string" && (to[0] === "optional" || to[0] === "box" || to[0] === "constraint")) {
    return adapt(value, from, to[1], schemas, scope, down);
  }
  if (typeof from === "string" && typeof to === "string" && from !== to) return castScalar(value, from, to);
  if (typeof from !== "string" && from[0] === "bits" && typeof to === "string") {
    return castScalar(bytesAsInt(value as Uint8Array), "u64", to);
  }
  if (typeof to !== "string" && to[0] === "bits" && typeof from === "string") {
    return unsignedBytes(BigInt(value as number | bigint), to[2]);
  }
  if (typeof from === "string" || typeof to === "string") return value;
  if (from[0] === "record" && to[0] === "record") {
    const records = schemas[scope].records;
    return project(object(value), records[down ? from[2] : from[1]], records[down ? to[1] : to[2]], schemas, scope, down);
  }
  if (to[0] === "bytes") return resized(value as Uint8Array, to[1]);
  if (to[0] === "bits") return resized(value as Uint8Array, to[2]);
  if (to[0] === "array" || to[0] === "delta") {
    const items = from[0] === "map" ? Array.from((value as Map<unknown, unknown>).values()) : sequence(value);
    const result = Array.from({ length: to[2] ?? items.length }, (_, i) =>
      i < items.length ? items[i] : defaultValue(to[1], null, schemas, scope));
    return typedValues(to[1], result);
  }
  if (to[0] === "map" && from[0] === "array") {
    const items = sequence(value);
    return new Map(Array.from({ length: items.length }, (_, i) => [i, items[i]] as const));
  }
  if (to[0] === "map" && to[3] !== null) {
    const items = (value as [unknown, unknown][]).slice(0, to[3]);
    if (down) {
      while (items.length < to[3]) {
        items.push([defaultValue(to[1], null, schemas, scope), defaultValue(to[2], null, schemas, scope)]);
      }
    }
    return items;
  }
  return value;
}

function project(
  value: Obj, from: Field[], to: Field[], schemas: SchemaSpec[], scope: number, down: boolean,
): Obj {
  const source = new Map(from.map(field => [field.id, field]));
  const result: Obj = {};
  for (const field of to) {
    const old = source.get(field.id);
    const prior = old && value[old.name];
    const cast = down ? field.cast : old?.cast;
    let current: unknown;
    if (!old) current = defaultValue(field.ty, down ? null : field.default, schemas, scope);
    else if (cast && (down ? old.lazy : field.lazy)) {
      // Rust drops a value when a historical cast crosses into a lazy target.
      current = defaultValue(field.ty, null, schemas, scope);
    } else if (cast || JSON.stringify(old.ty) !== JSON.stringify(field.ty)) {
      current = adapt(prior, old.ty, field.ty, schemas, scope, down);
    } else current = prior;
    setField(result, field.name, current);
  }
  return result;
}

function versionLayout(schema: SchemaSpec, version: string): RootSpec {
  const root = schema.roots[version];
  if (!root) invalid(`Unsupported Pojoc version ${version}`);
  return root;
}

function readTable(r: Reader): string[] {
  const count = r.length();
  if (count > r.data.length - r.pos) invalid("Invalid Pojoc intern table length");
  limitCollection(count);
  const table = [];
  for (let i = 0; i < count; i++) table.push(r.string());
  return table;
}

function readPayload(r: Reader, schemas: SchemaSpec[], scope: number, version: string): Obj {
  const schema = schemas[scope];
  const source = versionLayout(schema, version);
  const latest = versionLayout(schema, schema.latest).fields;
  const value = readFields(r, source.fields, schemas, scope, source.intern ? readTable(r) : [], version === schema.latest);
  if (version === schema.latest) return value;
  const result = project(value, source.fields, latest, schemas, scope, false);
  for (const field of latest) validate(field.ty, result[field.name], field.name);
  return result;
}

function writePayload(w: Writer, schemas: SchemaSpec[], scope: number, version: string, value: Obj): void {
  const schema = schemas[scope];
  const target = versionLayout(schema, version);
  const latest = versionLayout(schema, schema.latest).fields;
  const table = new Map<string, number>();
  let wire = value;
  if (version !== schema.latest) {
    for (const field of latest) validate(field.ty, value[field.name], field.name);
    wire = project(value, latest, target.fields, schemas, scope, true);
  }
  if (target.intern) {
    const body = new Writer();
    writeFields(body, target.fields, wire, schemas, scope, table, version === schema.latest);
    w.length(table.size);
    for (const text of table.keys()) w.string(text);
    w.bytes(body.finish());
  } else writeFields(w, target.fields, wire, schemas, scope, table, version === schema.latest);
}

function openEnvelope(input: Input, offset: number): { version: bigint; reader: Reader; offset: number } {
  const bytes = input instanceof Uint8Array ? input : new Uint8Array(input);
  if (!Number.isSafeInteger(offset) || offset < 0 || offset > bytes.length) invalid("Invalid Pojoc offset");
  const envelope = new Reader(bytes.subarray(offset));
  const version = envelope.varint();
  const reader = new Reader(envelope.bytes(envelope.uint(4)));
  return { version, reader, offset: offset + envelope.pos };
}

function skipValue(r: Reader, ty: WireType, schemas: SchemaSpec[], scope: number, table: string[]): void {
  if (typeof ty === "string") {
    if (ty === "string") r.take(r.length());
    else if (ty === "varint32") r.varint32();
    else if (ty === "varint64" || ty === "intern") r.varint();
    else r.take(arrayOf(ty)?.BYTES_PER_ELEMENT ?? 1);
    return;
  }
  switch (ty[0]) {
    case "bytes":
    case "bits":
    case "enum": r.take(ty[1]); return;
    case "union": r.take(ty[1]); r.take(r.length()); return;
    case "vfloat": r.take(ty[3]); return;
    case "optional":
      if (r.byte()) skipValue(r, ty[1], schemas, scope, table);
      return;
    case "constraint":
    case "box": return skipValue(r, ty[1], schemas, scope, table);
    case "record":
      positionField(r, schemas[scope].records[ty[2]], schemas, scope, table);
      return;
    case "import": readPayload(r, schemas, schemas[scope].imports[ty[1]], ty[2]); return;
    case "tuple":
      for (const item of ty[1]) skipValue(r, item, schemas, scope, table);
      return;
    case "array":
    case "delta": {
      const count = ty[2] ?? r.length();
      const ctor = ty[0] === "array" && arrayOf(ty[1]);
      if (ctor) { r.take(count * ctor.BYTES_PER_ELEMENT); return; }
      limitCollection(count);
      for (let i = 0; i < count; i++) {
        if (ty[0] === "delta") r.varint();
        else skipValue(r, ty[1], schemas, scope, table);
      }
      return;
    }
    case "map": {
      const count = ty[3] ?? r.length();
      limitCollection(count);
      for (let i = 0; i < count; i++) {
        skipValue(r, ty[1], schemas, scope, table);
        skipValue(r, ty[2], schemas, scope, table);
      }
    }
  }
}

function positionField(
  r: Reader, fields: Field[], schemas: SchemaSpec[], scope: number, table: string[], wanted?: Field,
): WireType | null {
  const header = r.bytes(headerLength(fields));
  let bit = 0;
  for (const field of fields) {
    let ty: WireType | null = field.ty;
    if (isOptional(field.ty)) {
      const present = header[bit >> 3] & 1 << (bit & 7);
      bit++;
      ty = present ? field.ty[1] : null;
    }
    if (field === wanted) return ty;
    if (ty !== null) skipValue(r, ty, schemas, scope, table);
  }
  return null;
}

function makeCodec<T>(schemas: SchemaSpec[], scope: number) {
  const schema = schemas[scope];
  const current = BigInt(schema.latest);
  const latest = versionLayout(schema, schema.latest);

  function encodeForVersion(value: T, version: Version): Uint8Array {
    if (typeof version === "number" && !Number.isSafeInteger(version)) invalid("Use bigint for a large Pojoc version");
    const n = BigInt(version);
    const writer = new Writer();
    writer.varint(n);
    const length = writer.take(4);
    writePayload(writer, schemas, scope, n.toString(), object(value));
    const bytes = writer.pos - length - 4;
    if (bytes > 0xffff_ffff) invalid("Pojoc payload exceeds its envelope");
    writer.view.setUint32(length, bytes, true);
    return writer.finish();
  }

  function decodeFrom(input: Input, offset = 0): { value: T; version: bigint; offset: number } {
    const envelope = openEnvelope(input, offset);
    const value = readPayload(envelope.reader, schemas, scope, envelope.version.toString()) as T;
    if (envelope.reader.pos !== envelope.reader.data.length) invalid("Unexpected trailing Pojoc payload");
    return { value, version: envelope.version, offset: envelope.offset };
  }

  function seek<K extends keyof T & string>(input: Input, path: K): T[K];
  function seek(input: Input, path: string): unknown;
  function seek(input: Input, path: string): unknown {
    const names = path.split(".");
    let fields = latest.fields;
    const hops: { fields: Field[]; field: Field }[] = [];
    for (let i = 0; i < names.length; i++) {
      const field = fields.find(item => item.name === names[i]);
      if (!field?.seek) invalid(`Unknown seekable Pojoc path ${path}`);
      hops.push({ fields, field });
      if (i + 1 < names.length) {
        if (field.lazy || typeof field.ty === "string" || field.ty[0] !== "record") invalid(`Invalid nested path ${path}`);
        fields = schema.records[field.ty[2]];
      }
    }
    const envelope = openEnvelope(input, 0);
    if (envelope.version !== current) {
      let value: unknown = decodeFrom(input).value;
      for (const name of names) value = object(value)[name];
      return value;
    }
    const r = envelope.reader;
    const table = latest.intern ? readTable(r) : [];
    let selected: WireType | null = null;
    for (const hop of hops) selected = positionField(r, hop.fields, schemas, scope, table, hop.field);
    const leaf = hops[hops.length - 1].field;
    const value = selected === null ? null : readValue(r, selected, schemas, scope, table);
    validate(leaf.ty, value, leaf.name);
    return value;
  }

  return {
    version: current,
    encode: (value: T) => encodeForVersion(value, current),
    encodeForVersion,
    decode: (input: Input) => decodeFrom(input).value,
    decodeFrom,
    create: (value: Partial<T> = {}) => defaults(latest.fields, object(value), schemas, scope) as T,
    seek,
  };
}
