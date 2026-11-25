import { castScalar } from "./compiled.js";
import type { Obj } from "./types.js";
import type { Field, SchemaSpec, WireType } from "./legacy_types.js";
import {
  bytesAsInt,
  integerKind,
  resized,
  sequence,
  typedValues,
  unsignedBytes,
} from "./scalars.js";
import { object, setField } from "./schema.js";

function defaultValue(
  ty: WireType,
  supplied: unknown,
  schemas: SchemaSpec[],
  scope: number,
): unknown {
  const schema = schemas[scope];
  const plan = Array.isArray(supplied) ? supplied : [];
  if (typeof ty === "string") {
    if (ty === "string" || ty === "intern") return supplied ?? "";
    if (ty === "bool") return supplied ?? false;
    const value =
      plan[0] === "bytes"
        ? bytesAsInt(Uint8Array.from(plan[1]))
        : plan[0] === "integer"
          ? plan[1]
          : (supplied ?? 0);
    if (integerKind(ty)?.[0] === 64) return BigInt(value);
    return ty === "f32" ? Math.fround(Number(value)) : Number(value);
  }
  switch (ty[0]) {
    case "optional":
      return supplied == null
        ? null
        : defaultValue(ty[1], supplied, schemas, scope);
    case "box":
    case "constraint":
      return defaultValue(ty[1], supplied, schemas, scope);
    case "bytes":
    case "bits": {
      const width = ty[0] === "bits" ? ty[2] : ty[1];
      if (plan[0] === "bytes") return resized(Uint8Array.from(plan[1]), width);
      if (plan[0] === "integer") return unsignedBytes(BigInt(plan[1]), width);
      return new Uint8Array(width);
    }
    case "enum":
    case "vfloat":
      return Number(plan[0] === "integer" ? plan[1] : (supplied ?? 0));
    case "union": {
      const first = ty[3][0];
      if (first === undefined)
        return { kind: "Unknown", discriminant: 0, data: new Uint8Array() };
      return {
        kind: first[1],
        value: defaultValue(first[2], null, schemas, scope),
      };
    }
    case "record":
      return defaults(schema.records[ty[2]], {}, schemas, scope);
    case "import": {
      const imported = schema.imports[ty[1]];
      const target = schemas[imported];
      return defaults(
        target.roots[target.latest].fields,
        {},
        schemas,
        imported,
      );
    }
    case "tuple":
      return ty[1].map((item, i) =>
        defaultValue(item, plan[1]?.[i], schemas, scope),
      );
    case "array":
    case "delta": {
      const items = plan[0] === "items" ? (plan[1] as unknown[]) : [];
      const result = Array.from({ length: ty[2] ?? items.length }, (_, i) =>
        defaultValue(
          ty[1],
          plan[0] === "repeat" ? plan[1] : items[i],
          schemas,
          scope,
        ),
      );
      return typedValues(ty[1], result);
    }
    case "map": {
      const items = (plan[0] === "pairs" ? plan[1] : []) as [
        unknown,
        unknown,
      ][];
      const result = items.map(([key, value]): [unknown, unknown] => [
        defaultValue(ty[1], key, schemas, scope),
        defaultValue(ty[2], value, schemas, scope),
      ]);
      if (ty[3] === null) return new Map(result);
      while (result.length < ty[3]) {
        result.push([
          defaultValue(ty[1], null, schemas, scope),
          defaultValue(ty[2], null, schemas, scope),
        ]);
      }
      return result.slice(0, ty[3]);
    }
  }
}

function defaults(
  fields: Field[],
  value: Obj,
  schemas: SchemaSpec[],
  scope: number,
): Obj {
  const result: Obj = {};
  for (const field of fields) {
    setField(
      result,
      field.name,
      Object.prototype.hasOwnProperty.call(value, field.name)
        ? value[field.name]
        : defaultValue(field.ty, field.default, schemas, scope),
    );
  }
  return result;
}

function adapt(
  value: unknown,
  from: WireType,
  to: WireType,
  schemas: SchemaSpec[],
  scope: number,
  down: boolean,
): unknown {
  if (value == null) return defaultValue(to, null, schemas, scope);
  if (
    typeof from !== "string" &&
    (from[0] === "optional" || from[0] === "box" || from[0] === "constraint")
  ) {
    return adapt(value, from[1], to, schemas, scope, down);
  }
  if (
    typeof to !== "string" &&
    (to[0] === "optional" || to[0] === "box" || to[0] === "constraint")
  ) {
    return adapt(value, from, to[1], schemas, scope, down);
  }
  if (typeof from === "string" && typeof to === "string" && from !== to)
    return castScalar(value, from === "f32" || from === "f64", to);
  if (
    typeof from !== "string" &&
    from[0] === "bits" &&
    typeof to === "string"
  ) {
    return castScalar(bytesAsInt(value as Uint8Array), false, to);
  }
  if (typeof to !== "string" && to[0] === "bits" && typeof from === "string") {
    return unsignedBytes(BigInt(value as number | bigint), to[2]);
  }
  if (typeof from === "string" || typeof to === "string") return value;
  if (from[0] === "record" && to[0] === "record") {
    const records = schemas[scope].records;
    return project(
      object(value),
      records[down ? from[2] : from[1]],
      records[down ? to[1] : to[2]],
      schemas,
      scope,
      down,
    );
  }
  if (to[0] === "bytes") return resized(value as Uint8Array, to[1]);
  if (to[0] === "bits") return resized(value as Uint8Array, to[2]);
  if (to[0] === "array" || to[0] === "delta") {
    const items =
      from[0] === "map"
        ? Array.from((value as Map<unknown, unknown>).values())
        : sequence(value);
    const result = Array.from({ length: to[2] ?? items.length }, (_, i) =>
      i < items.length ? items[i] : defaultValue(to[1], null, schemas, scope),
    );
    return typedValues(to[1], result);
  }
  if (to[0] === "map" && from[0] === "array") {
    const items = sequence(value);
    return new Map(
      Array.from({ length: items.length }, (_, i) => [i, items[i]] as const),
    );
  }
  if (to[0] === "map" && to[3] !== null) {
    const items = (value as [unknown, unknown][]).slice(0, to[3]);
    if (down) {
      while (items.length < to[3]) {
        items.push([
          defaultValue(to[1], null, schemas, scope),
          defaultValue(to[2], null, schemas, scope),
        ]);
      }
    }
    return items;
  }
  return value;
}

function project(
  value: Obj,
  from: Field[],
  to: Field[],
  schemas: SchemaSpec[],
  scope: number,
  down: boolean,
): Obj {
  const source = new Map(from.map((field) => [field.id, field]));
  const result: Obj = {};
  for (const field of to) {
    const old = source.get(field.id);
    const prior = old && value[old.name];
    const cast = down ? field.cast : old?.cast;
    let current: unknown;
    if (!old)
      current = defaultValue(
        field.ty,
        down ? null : field.default,
        schemas,
        scope,
      );
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

export { defaults, project };
