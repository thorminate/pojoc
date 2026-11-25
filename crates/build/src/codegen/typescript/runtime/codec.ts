import { finishDecode, versionNumber } from "./compiled.js";
import type { Input, Obj, Version } from "./types.js";
import type { SchemaSpec } from "./legacy_types.js";
import {
  type Envelope,
  emptyTable,
  invalid,
  readTableOffsets,
  takeBytes,
} from "./wire.js";
import {
  beginWrite,
  finishEnvelope,
  takeWrite,
  writeVarint,
} from "./writer.js";
import {
  type AccessField,
  type AccessLayout,
  accessLayout,
  object,
  setField,
  versionLayout,
} from "./schema.js";
import { defaults } from "./versions.js";
import { readPayload, writePayload } from "./payload.js";
import { compileView, readAccessField, skipField } from "./access.js";

function encodePlanned<T>(
  value: T,
  version: Version,
  schemas: SchemaSpec[],
  scope: number,
): Uint8Array {
  const n = versionNumber(version);
  const schema = schemas[scope];
  const writer = beginWrite(
    schema.runtime === "bun" || schema.runtime === "node",
  );
  writeVarint(writer, n);
  const length = takeWrite(writer, 5);
  writePayload(writer, schemas, scope, n.toString(), object(value));
  return finishEnvelope(writer, length);
}

function decodePlanned<T>(
  reader: Envelope,
  current: bigint,
  schemas: SchemaSpec[],
  scope: number,
): T {
  const version =
    reader.version === current
      ? schemas[scope].latest
      : reader.version.toString();
  return finishDecode(reader, readPayload(reader, schemas, scope, version));
}

function createPlanned<T>(
  value: Partial<T>,
  schemas: SchemaSpec[],
  scope: number,
): T {
  const schema = schemas[scope];
  return defaults(
    versionLayout(schema, schema.latest).fields,
    object(value),
    schemas,
    scope,
  ) as T;
}

type OpenFrame = (input: Input, offset: number) => Envelope;
type DecodeFrame<T> = (reader: Envelope) => T;
const views = new WeakMap<
  SchemaSpec[],
  Map<number, ReturnType<typeof compileView>>
>();

function openPlanned<T>(
  r: Envelope,
  current: bigint,
  schemas: SchemaSpec[],
  scope: number,
  decodeFrame: DecodeFrame<T>,
): Readonly<T> {
  if (r.version !== current) return Object.freeze(decodeFrame(r));
  let byScope = views.get(schemas);
  if (!byScope) views.set(schemas, (byScope = new Map()));
  let view = byScope.get(scope);
  if (!view) {
    const schema = schemas[scope];
    const latest = versionLayout(schema, schema.latest);
    view = compileView(
      accessLayout(latest.fields),
      latest.intern,
      schemas,
      scope,
    );
    byScope.set(scope, view);
  }
  return new view(r) as unknown as Readonly<T>;
}

function selectPlanned<T, K extends keyof T & string>(
  keys: readonly K[],
  current: bigint,
  schemas: SchemaSpec[],
  scope: number,
  openFrame: OpenFrame,
  decodeFrame: DecodeFrame<T>,
) {
  const schema = schemas[scope];
  const latest = versionLayout(schema, schema.latest);
  const layout = accessLayout(latest.fields);
  const fields = Array.from(new Set(keys), (key) => {
    const entry = layout.byName.get(key);
    if (!entry) invalid(`Unknown Pojoc field ${key}`);
    return entry;
  }).sort((a, b) => a.index - b.index);

  return function projectFields(input: Input, offset = 0): Pick<T, K> {
    const r = openFrame(input, offset);
    const result: Obj = {};
    if (r.version !== current) {
      const value = decodeFrame(r);
      for (const { field } of fields)
        setField(result, field.name, value[field.name as K]);
      return result as Pick<T, K>;
    }
    if (fields.length === 0) return result as Pick<T, K>;
    const table = latest.intern ? readTableOffsets(r) : emptyTable;
    const header = takeBytes(r, layout.bitmapBytes);
    const start = takeBytes(r, layout.fixedWidth);
    let tail = r.pos;
    let next = layout.fixedCount;
    for (const entry of fields) {
      if (entry.offset >= 0) {
        r.pos = start + entry.offset;
      } else {
        r.pos = tail;
        while (next < entry.index)
          skipField(r, header, layout.fields[next++], schemas, scope);
      }
      setField(
        result,
        entry.field.name,
        readAccessField(r, header, entry, schemas, scope, table),
      );
      if (entry.offset < 0) {
        tail = r.pos;
        next++;
      }
    }
    return result as Pick<T, K>;
  };
}

function seekPlanned(
  input: Input,
  path: string,
  current: bigint,
  schemas: SchemaSpec[],
  scope: number,
  openFrame: OpenFrame,
  decodeFrame: DecodeFrame<unknown>,
): unknown {
  const schema = schemas[scope];
  const latest = versionLayout(schema, schema.latest);
  const names = path.split(".");
  const hops: { layout: AccessLayout; entry: AccessField }[] = [];
  let layout = accessLayout(latest.fields);
  for (let i = 0; i < names.length; i++) {
    const entry = layout.byName.get(names[i]);
    if (!entry?.field.seek) invalid(`Unknown seekable Pojoc path ${path}`);
    hops.push({ layout, entry });
    if (i + 1 < names.length) {
      const ty = entry.field.ty;
      if (entry.field.lazy || typeof ty === "string" || ty[0] !== "record")
        invalid(`Invalid nested path ${path}`);
      layout = accessLayout(schema.records[ty[2]]);
    }
  }
  const r = openFrame(input, 0);
  if (r.version !== current) {
    let value: unknown = decodeFrame(r);
    for (const name of names) value = object(value)[name];
    return value;
  }
  const table = latest.intern ? readTableOffsets(r) : emptyTable;
  for (let i = 0; i < hops.length; i++) {
    const { layout, entry } = hops[i];
    const header = takeBytes(r, layout.bitmapBytes);
    const start = takeBytes(r, layout.fixedWidth);
    if (entry.offset >= 0) r.pos = start + entry.offset;
    else {
      for (let j = layout.fixedCount; j < entry.index; j++)
        skipField(r, header, layout.fields[j], schemas, scope);
    }
    if (i + 1 === hops.length)
      return readAccessField(r, header, entry, schemas, scope, table);
  }
}

export {
  createPlanned,
  decodePlanned,
  encodePlanned,
  openPlanned,
  selectPlanned,
  seekPlanned,
};
