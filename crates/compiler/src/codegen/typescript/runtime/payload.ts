import { writeScalar } from "./legacy_scalar.js";
import { validateRange } from "./compiled.js";
import {
  type WriteCursor,
  beginWrite,
  finishWrite,
  prependInternTable,
  takeWrite,
  writeByte,
  writeBytes,
  writeLength,
  writeUint,
  writeVarint32Array,
} from "./writer.js";
import type {
  Cursor,
  CursorOps,
  FixedScalar,
  Obj,
  ReadTable,
} from "./types.js";
import type {
  BoundLayout,
  CursorReads,
  Field,
  FixedLayout,
  LayoutReader,
  SchemaSpec,
  WireType,
} from "./legacy_types.js";
import {
  cursorReads,
  cursorView,
  emptyTable,
  invalid,
  limitCollection,
  readByte,
  readBytes,
  readLength,
  readTable,
  readUint,
  readVarint32Array,
  takeBytes,
} from "./wire.js";
import {
  arrayOf,
  decodeDelta,
  encodeDelta,
  fixedWidths,
  isFixedScalar,
  littleEndian,
  readFixedScalar,
  readPacked,
  readScalar,
  resized,
  sequence,
} from "./scalars.js";
import {
  accessLayout,
  isOptional,
  object,
  setField,
  versionLayout,
} from "./schema.js";
import { project } from "./versions.js";

function readValue(
  r: Cursor,
  ty: WireType,
  schemas: SchemaSpec[],
  scope: number,
  table: ReadTable,
  historical = false,
): unknown {
  if (typeof ty === "string") return readScalar(r, ty, table);
  const schema = schemas[scope];
  switch (ty[0]) {
    case "bytes":
      return readBytes(r, ty[1]);
    case "bits":
      return resized(readBytes(r, ty[1]), ty[2]);
    case "enum": {
      const value = readUint(r, ty[1]);
      if (value !== 0 && !ty[2].includes(value))
        invalid("Invalid Pojoc enum discriminant");
      return value;
    }
    case "union": {
      const discriminant = readUint(r, ty[1]);
      const size = readLength(r);
      const variant = ty[2].find((v) => v[0] === discriminant);
      if (!variant)
        return { kind: "Unknown", discriminant, data: readBytes(r, size) };
      if (size > r.end - r.pos) invalid("Unexpected end of Pojoc payload");
      const outerEnd = r.end;
      r.end = r.pos + size;
      try {
        const value = readValue(r, variant[2], schemas, scope, table);
        if (r.pos !== r.end) invalid("Invalid Pojoc union payload length");
        validate(variant[2], value, variant[1]);
        return { kind: variant[1], value };
      } finally {
        r.end = outerEnd;
      }
    }
    case "record": {
      const name = historical ? ty[1] : ty[2];
      const fields = schema.records[name];
      const decode = !historical && schema.cursorRecordReaders?.[name];
      if (decode && typeof decode !== "function") {
        return decode.read(r, takeBytes(r, decode.width), cursorReads);
      }
      return decode
        ? bindLayout(decode, fields, schemas, scope)(r, table)
        : readFields(r, fields, schemas, scope, table, !historical);
    }
    case "import":
      return readPayload(r, schemas, schema.imports[ty[1]], ty[2]);
    case "optional":
      return readByte(r)
        ? readValue(r, ty[1], schemas, scope, table, historical)
        : null;
    case "constraint":
    case "box":
      return readValue(r, ty[1], schemas, scope, table, historical);
    case "vfloat":
      return Math.fround(ty[1] + readUint(r, ty[3]) * ty[2]);
    case "tuple":
      return ty[1].map((item) => readValue(r, item, schemas, scope, table));
    case "delta":
      return decodeDelta(r, ty[1], ty[2] ?? readLength(r));
    case "array": {
      const count = ty[2] ?? readLength(r);
      const ctor = arrayOf(ty[1]);
      if (ctor) return readPacked(r, ctor, count);
      const element = ty[1];
      if (typeof element !== "string" && element[0] === "record") {
        const decode = schema.cursorRecordReaders?.[element[2]];
        if (decode && typeof decode !== "function")
          return readFixedRecords(r, count, decode.width, decode.read);
      }
      limitCollection(count);
      if (element === "varint32") return readVarint32Array(r, count);
      const result = [];
      for (let i = 0; i < count; i++)
        result.push(readValue(r, element, schemas, scope, table));
      return result;
    }
    case "map": {
      const count = ty[3] ?? readLength(r);
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
  w: WriteCursor,
  ty: WireType,
  value: unknown,
  schemas: SchemaSpec[],
  scope: number,
  table: Map<string, number>,
  historical = false,
): void {
  if (typeof ty === "string") return writeScalar(w, ty, value, table);
  const schema = schemas[scope];
  switch (ty[0]) {
    case "bytes":
      if (!(value instanceof Uint8Array) || value.length !== ty[1])
        invalid("Invalid fixed bytes length");
      writeBytes(w, value);
      return;
    case "bits":
      if (!(value instanceof Uint8Array) || value.length !== ty[2])
        invalid("Invalid bitset width");
      writeBytes(w, resized(value, ty[1]));
      return;
    case "enum":
      if (typeof value !== "number" || (value !== 0 && !ty[2].includes(value)))
        invalid("Invalid enum value");
      writeUint(w, ty[1], value <= ty[3] ? value : 0);
      return;
    case "union": {
      const item = object(value);
      const variant = ty[3].find((v) => v[1] === item.kind);
      const discriminant = (
        item.kind === "Unknown" ? item.discriminant : variant?.[0]
      ) as number;
      if (!Number.isInteger(discriminant) || discriminant < 0)
        invalid("Invalid Pojoc union kind");
      if (discriminant >= 2 ** (ty[1] * 8))
        invalid("Pojoc union discriminant overflow");
      let payload: Uint8Array;
      if (variant) {
        validate(variant[2], item.value, variant[1]);
        const nested = beginWrite(w.fillPresenceInPlace);
        writeValue(nested, variant[2], item.value, schemas, scope, table);
        payload = finishWrite(nested);
      } else {
        if (!(item.data instanceof Uint8Array))
          throw new TypeError("Expected bytes for unknown union variant");
        payload = item.data;
      }
      writeUint(w, ty[1], discriminant);
      writeLength(w, payload.length);
      writeBytes(w, payload);
      return;
    }
    case "record": {
      const fields = schema.records[historical ? ty[1] : ty[2]];
      writeFields(w, fields, object(value), schemas, scope, table, !historical);
      return;
    }
    case "import":
      writePayload(w, schemas, schema.imports[ty[1]], ty[2], object(value));
      return;
    case "optional":
      writeByte(w, value == null ? 0 : 1);
      if (value != null)
        writeValue(w, ty[1], value, schemas, scope, table, historical);
      return;
    case "constraint":
    case "box":
      return writeValue(w, ty[1], value, schemas, scope, table, historical);
    case "vfloat": {
      if (typeof value !== "number")
        throw new TypeError("Expected a Pojoc vfloat");
      const steps = Math.round((Math.fround(value) - ty[1]) / ty[2]);
      writeUint(
        w,
        ty[3],
        Math.min(2 ** (ty[3] * 8) - 1, Math.max(0, steps || 0)),
      );
      return;
    }
    case "tuple": {
      const items = sequence(value);
      if (items.length !== ty[1].length) invalid("Invalid Pojoc tuple length");
      ty[1].forEach((kind, i) =>
        writeValue(w, kind, items[i], schemas, scope, table),
      );
      return;
    }
    case "array":
    case "delta": {
      const items = sequence(value);
      if (ty[2] === null) writeLength(w, items.length);
      else if (items.length !== ty[2]) invalid("Invalid fixed array length");
      if (ty[0] === "delta") return encodeDelta(w, ty[1], items);
      if (ty[1] === "varint32") return writeVarint32Array(w, items);
      const ctor = arrayOf(ty[1]);
      if (littleEndian && ctor && value instanceof ctor) {
        writeBytes(
          w,
          new Uint8Array(value.buffer, value.byteOffset, value.byteLength),
        );
      } else {
        for (let i = 0; i < items.length; i++)
          writeValue(w, ty[1], items[i], schemas, scope, table);
      }
      return;
    }
    case "map": {
      let entries: Iterable<[unknown, unknown]>;
      if (ty[3] === null) {
        if (!(value instanceof Map))
          throw new TypeError("Expected a Pojoc Map");
        writeLength(w, value.size);
        entries = value;
      } else {
        if (!Array.isArray(value) || value.length !== ty[3])
          invalid("Invalid fixed map length");
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
  if (ty[0] === "optional" && value != null)
    return validate(ty[1], value, field);
  if (ty[0] !== "constraint") return;
  const [, , min, max] = ty;
  validateRange(value, min, max, field);
}

type FixedRead = { name: string; ty: FixedScalar; offset: number };
type ScalarRun = { width: number; scalars: FixedRead[] };
type FieldRead = {
  name: string;
  ty: WireType;
  cast: boolean;
  presenceBit: number;
  constrained: boolean;
};
type FieldReadPlan = { headerBytes: number; steps: (ScalarRun | FieldRead)[] };
const fieldReadPlans = new WeakMap<Field[], FieldReadPlan>();

function needsValidation(ty: WireType): boolean {
  while (isOptional(ty)) ty = ty[1];
  return typeof ty !== "string" && ty[0] === "constraint";
}

function fieldReadPlan(fields: Field[]): FieldReadPlan {
  let plan = fieldReadPlans.get(fields);
  if (plan !== undefined) return plan;

  const layout = accessLayout(fields);
  const steps: FieldReadPlan["steps"] = [];
  let run: ScalarRun | undefined;
  for (const { field, ty, bit } of layout.fields) {
    if (bit < 0 && isFixedScalar(ty)) {
      if (!run) {
        run = { width: 0, scalars: [] };
        steps.push(run);
      }
      run.scalars.push({ name: field.name, ty, offset: run.width });
      run.width += fixedWidths[ty];
    } else {
      run = undefined;
      steps.push({
        name: field.name,
        ty,
        cast: field.cast,
        presenceBit: bit,
        constrained: needsValidation(ty),
      });
    }
  }
  plan = { headerBytes: layout.bitmapBytes, steps };
  fieldReadPlans.set(fields, plan);
  return plan;
}

function readFields(
  r: Cursor,
  fields: Field[],
  schemas: SchemaSpec[],
  scope: number,
  table: ReadTable,
  checks: boolean,
): Obj {
  const { headerBytes, steps } = fieldReadPlan(fields);
  const header = headerBytes === 0 ? r.pos : takeBytes(r, headerBytes);
  const { data } = r;
  const result: Obj = {};
  for (const step of steps) {
    if ("scalars" in step) {
      // The run fits in this payload view before any scalar read uses its precomputed offset.
      const start = takeBytes(r, step.width);
      const view = cursorView(r);
      for (const { name, ty, offset } of step.scalars) {
        setField(result, name, readFixedScalar(view, start + offset, ty));
      }
      continue;
    }

    const { name, ty, cast, presenceBit, constrained } = step;
    if (
      presenceBit >= 0 &&
      (data[header + (presenceBit >> 3)] & (1 << (presenceBit & 7))) === 0
    ) {
      setField(result, name, null);
      continue;
    }
    const value = readValue(r, ty, schemas, scope, table, cast);
    if (checks && constrained) validate(ty, value, name);
    setField(result, name, value);
  }
  return result;
}

function readFixedRecords(
  r: Cursor,
  count: number,
  width: number,
  read: FixedLayout["read"],
): Obj[] {
  limitCollection(count);
  // Reserve the entire block before any generated reader uses its offsets.
  let pos = takeBytes(r, count * width);
  const rows: Obj[] = [];
  for (let i = 0; i < count; i++, pos += width)
    rows.push(read(r, pos, cursorReads));
  return rows;
}

function readRecordArray(
  r: Cursor,
  count: number,
  table: ReadTable,
  read: (r: Cursor, table: ReadTable, ops: CursorOps) => Obj,
): Obj[] {
  limitCollection(count);
  const rows: Obj[] = [];
  for (let i = 0; i < count; i++) rows.push(read(r, table, cursorReads));
  return rows;
}

const boundLayouts = new WeakMap<LayoutReader, BoundLayout>();

function bindField(
  field: Field,
  schemas: SchemaSpec[],
  scope: number,
): (r: Cursor, table: ReadTable) => unknown {
  const ty = isOptional(field.ty) ? field.ty[1] : field.ty;
  const schema = schemas[scope];
  if (typeof ty !== "string" && ty[0] === "array") {
    const [, element, fixedCount] = ty;
    if (typeof element !== "string" && element[0] === "record") {
      const key = element[2];
      const decode = schema.cursorRecordReaders?.[key];
      if (decode && typeof decode !== "function") {
        return (r) =>
          readFixedRecords(
            r,
            fixedCount ?? readLength(r),
            decode.width,
            decode.read,
          );
      }
      if (decode) {
        let read: BoundLayout | undefined;
        return (r, table) => {
          const count = fixedCount ?? readLength(r);
          if (count === 0) return [];
          // Resolve on first use so self-referential layouts can bind safely.
          read ??= bindLayout(decode, schema.records[key], schemas, scope);
          return readRecordArray(r, count, table, read);
        };
      }
    }
  }
  return (r, table) => {
    const value = readValue(r, ty, schemas, scope, table, field.cast);
    validate(ty, value, field.name);
    return value;
  };
}

function bindLayout(
  decode: LayoutReader,
  fields: Field[],
  schemas: SchemaSpec[],
  scope: number,
): BoundLayout {
  let bound = boundLayouts.get(decode);
  if (bound !== undefined) return bound;
  const fieldsByIndex = fields.map((field) => bindField(field, schemas, scope));
  const reads: CursorReads = {
    ...cursorReads,
    field: (r, table, index) => fieldsByIndex[index](r, table),
  };
  bound = (r, table) => decode(r, table, reads);
  boundLayouts.set(decode, bound);
  return bound;
}

function writeFieldsSinglePass(
  w: WriteCursor,
  fields: Field[],
  value: Obj,
  schemas: SchemaSpec[],
  scope: number,
  table: Map<string, number>,
  checks: boolean,
): void {
  const { bitmapBytes: headerBytes } = accessLayout(fields);
  const header = w.pos;
  if (headerBytes !== 0) {
    takeWrite(w, headerBytes);
    w.data.fill(0, header, header + headerBytes);
  }
  let bit = 0;
  for (const field of fields) {
    const item = value[field.name];
    if (checks) validate(field.ty, item, field.name);
    if (isOptional(field.ty)) {
      if (item != null) {
        // The writer can grow while encoding an earlier field.
        w.data[header + (bit >> 3)] |= 1 << (bit & 7);
        writeValue(w, field.ty[1], item, schemas, scope, table, field.cast);
      }
      bit++;
    } else {
      writeValue(w, field.ty, item, schemas, scope, table, field.cast);
    }
  }
}

function writeFields(
  w: WriteCursor,
  fields: Field[],
  value: Obj,
  schemas: SchemaSpec[],
  scope: number,
  table: Map<string, number>,
  checks: boolean,
): void {
  if (w.fillPresenceInPlace) {
    writeFieldsSinglePass(w, fields, value, schemas, scope, table, checks);
    return;
  }
  const header = new Uint8Array(accessLayout(fields).bitmapBytes);
  let bit = 0;
  for (const field of fields) {
    if (isOptional(field.ty)) {
      if (value[field.name] != null) header[bit >> 3] |= 1 << (bit & 7);
      bit++;
    }
  }
  writeBytes(w, header);
  for (const field of fields) {
    const item = value[field.name];
    if (checks) validate(field.ty, item, field.name);
    if (isOptional(field.ty)) {
      if (item != null)
        writeValue(w, field.ty[1], item, schemas, scope, table, field.cast);
    } else {
      writeValue(w, field.ty, item, schemas, scope, table, field.cast);
    }
  }
}

function readPayload(
  r: Cursor,
  schemas: SchemaSpec[],
  scope: number,
  version: string,
): Obj {
  const schema = schemas[scope];
  const source = versionLayout(schema, version);
  const table = source.intern ? readTable(r) : emptyTable;
  const value = source.decodeCursor
    ? bindLayout(source.decodeCursor, source.fields, schemas, scope)(r, table)
    : readFields(
        r,
        source.fields,
        schemas,
        scope,
        table,
        version === schema.latest,
      );
  if (version === schema.latest) return value;
  const latest = versionLayout(schema, schema.latest).fields;
  const result = project(value, source.fields, latest, schemas, scope, false);
  for (const field of latest)
    validate(field.ty, result[field.name], field.name);
  return result;
}

function writePayload(
  w: WriteCursor,
  schemas: SchemaSpec[],
  scope: number,
  version: string,
  value: Obj,
): void {
  const schema = schemas[scope];
  const target = versionLayout(schema, version);
  const latest = versionLayout(schema, schema.latest).fields;
  const table = new Map<string, number>();
  let wire = value;
  if (version !== schema.latest) {
    for (const field of latest)
      validate(field.ty, value[field.name], field.name);
    wire = project(value, latest, target.fields, schemas, scope, true);
  }
  const start = w.pos;
  writeFields(
    w,
    target.fields,
    wire,
    schemas,
    scope,
    table,
    version === schema.latest,
  );
  if (target.intern) prependInternTable(w, start, table);
}

export {
  readValue,
  validate,
  bindLayout,
  readPayload,
  writePayload,
  readFixedRecords,
  readRecordArray,
};
