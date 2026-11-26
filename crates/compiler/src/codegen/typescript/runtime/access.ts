import type { Cursor, ReadTable } from "./types.js";
import type { Field, SchemaSpec, WireType } from "./legacy_types.js";
import {
  emptyTable,
  limitCollection,
  readByte,
  readLength,
  readTableOffsets,
  readVarint,
  readVarint32,
  tableLength,
  takeBytes,
} from "./wire.js";
import { arrayOf, fixedWidths } from "./scalars.js";
import {
  type AccessField,
  type AccessLayout,
  accessLayout,
  present,
  versionLayout,
} from "./schema.js";
import { readValue, validate } from "./payload.js";

function skipRecord(
  r: Cursor,
  fields: Field[],
  schemas: SchemaSpec[],
  scope: number,
): void {
  const layout = accessLayout(fields);
  const header = takeBytes(r, layout.bitmapBytes);
  takeBytes(r, layout.fixedWidth);
  for (let i = layout.fixedCount; i < layout.fields.length; i++) {
    skipField(r, header, layout.fields[i], schemas, scope);
  }
}

function skipValue(
  r: Cursor,
  ty: WireType,
  schemas: SchemaSpec[],
  scope: number,
  historical = false,
): void {
  if (typeof ty === "string") {
    if (ty === "string") takeBytes(r, readLength(r));
    else if (ty === "varint32") readVarint32(r);
    else if (ty === "varint64") readVarint(r);
    else if (ty === "intern") readLength(r);
    else takeBytes(r, fixedWidths[ty]);
    return;
  }
  const schema = schemas[scope];
  switch (ty[0]) {
    case "bytes":
    case "bits":
    case "enum":
      takeBytes(r, ty[1]);
      return;
    case "union":
      takeBytes(r, ty[1]);
      takeBytes(r, readLength(r));
      return;
    case "vfloat":
      takeBytes(r, ty[3]);
      return;
    case "optional":
      if (readByte(r)) skipValue(r, ty[1], schemas, scope, historical);
      return;
    case "constraint":
    case "box":
      return skipValue(r, ty[1], schemas, scope, historical);
    case "record":
      return skipRecord(
        r,
        schema.records[historical ? ty[1] : ty[2]],
        schemas,
        scope,
      );
    case "import": {
      const childScope = schema.imports[ty[1]];
      const child = versionLayout(schemas[childScope], ty[2]);
      if (child.intern) {
        const count = tableLength(r);
        for (let i = 0; i < count; i++) takeBytes(r, readLength(r));
      }
      return skipRecord(r, child.fields, schemas, childScope);
    }
    case "tuple":
      for (const item of ty[1]) skipValue(r, item, schemas, scope);
      return;
    case "array":
    case "delta": {
      const count = ty[2] ?? readLength(r);
      const ctor = ty[0] === "array" && arrayOf(ty[1]);
      if (ctor) {
        takeBytes(r, count * ctor.BYTES_PER_ELEMENT);
        return;
      }
      limitCollection(count);
      const element = ty[1];
      if (
        ty[0] === "array" &&
        typeof element !== "string" &&
        element[0] === "record"
      ) {
        const record = schema.cursorRecordReaders?.[element[2]];
        if (record && typeof record !== "function") {
          takeBytes(r, count * record.width);
          return;
        }
      }
      for (let i = 0; i < count; i++) {
        if (ty[0] === "delta") readVarint(r);
        else skipValue(r, element, schemas, scope);
      }
      return;
    }
    case "map": {
      const count = ty[3] ?? readLength(r);
      limitCollection(count);
      for (let i = 0; i < count; i++) {
        skipValue(r, ty[1], schemas, scope);
        skipValue(r, ty[2], schemas, scope);
      }
    }
  }
}

function skipField(
  r: Cursor,
  header: number,
  entry: AccessField,
  schemas: SchemaSpec[],
  scope: number,
): void {
  if (present(r, header, entry.bit))
    skipValue(r, entry.ty, schemas, scope, entry.field.cast);
}

function readAccessField(
  r: Cursor,
  header: number,
  entry: AccessField,
  schemas: SchemaSpec[],
  scope: number,
  table: ReadTable,
): unknown {
  if (!present(r, header, entry.bit)) return null;
  const { ty, field } = entry;
  const value = readValue(r, ty, schemas, scope, table, field.cast);
  validate(ty, value, field.name);
  return value;
}

function compileView(
  layout: AccessLayout,
  intern: boolean,
  schemas: SchemaSpec[],
  scope: number,
) {
  class View {
    #positions: number[] = [];
    #values: unknown[] = [];
    #access?: { table: ReadTable; header: number; start: number };
    #cursor: Cursor;

    constructor(cursor: Cursor) {
      this.#cursor = cursor;
    }

    static getter(entry: AccessField) {
      return function (this: View): unknown {
        return this.#read(entry);
      };
    }

    #read(entry: AccessField): unknown {
      const { index, offset } = entry;
      if (Object.hasOwn(this.#values, index)) return this.#values[index];
      const r = this.#cursor;
      if (!this.#access) {
        const frameStart = r.pos;
        try {
          const table = intern ? readTableOffsets(r) : emptyTable;
          const header = takeBytes(r, layout.bitmapBytes);
          const start = takeBytes(r, layout.fixedWidth);
          this.#positions.push(r.pos);
          this.#access = { table, header, start };
        } catch (error) {
          r.pos = frameStart;
          throw error;
        }
      }
      const { table, header, start } = this.#access;
      const positions = this.#positions;
      const tailIndex = index - layout.fixedCount;
      if (offset >= 0) {
        r.pos = start + offset;
      } else {
        r.pos = positions[positions.length - 1];
        for (let i = positions.length - 1; i < tailIndex; i++) {
          skipField(
            r,
            header,
            layout.fields[i + layout.fixedCount],
            schemas,
            scope,
          );
          positions.push(r.pos);
        }
        r.pos = positions[tailIndex];
      }
      const value = readAccessField(r, header, entry, schemas, scope, table);
      if (offset < 0 && tailIndex + 1 === positions.length)
        positions.push(r.pos);
      this.#values[index] = value;
      return value;
    }
  }

  for (const entry of layout.fields) {
    Object.defineProperty(View.prototype, entry.field.name, {
      get: View.getter(entry),
    });
  }
  return View;
}

export { skipField, readAccessField, compileView };
