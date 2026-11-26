import { object, setField } from "./compiled.js";
import type { Cursor } from "./types.js";
import type { Field, RootSpec, SchemaSpec, WireType } from "./legacy_types.js";
import { fixedWidths, isFixedScalar } from "./scalars.js";
import { invalid } from "./wire.js";

function isOptional(ty: WireType): ty is ["optional", WireType] {
  return typeof ty !== "string" && ty[0] === "optional";
}

function versionLayout(schema: SchemaSpec, version: string): RootSpec {
  const root = schema.roots[version];
  if (!root) invalid(`Unsupported Pojoc version ${version}`);
  return root;
}

type AccessField = {
  field: Field;
  ty: WireType;
  bit: number;
  index: number;
  offset: number;
};
type AccessLayout = {
  fields: AccessField[];
  byName: Map<string, AccessField>;
  bitmapBytes: number;
  fixedCount: number;
  fixedWidth: number;
};

const accessLayouts = new WeakMap<Field[], AccessLayout>();

function accessLayout(fields: Field[]): AccessLayout {
  let layout = accessLayouts.get(fields);
  if (layout) return layout;
  let optionals = 0;
  let fixedCount = 0;
  let fixedWidth = 0;
  const entries = fields.map((field, index): AccessField => {
    const wireType = field.ty;
    const optional = isOptional(wireType);
    const bit = optional ? optionals++ : -1;
    const ty = optional ? wireType[1] : wireType;
    let offset = -1;
    if (index === fixedCount && !optional && isFixedScalar(ty)) {
      offset = fixedWidth;
      fixedCount++;
      fixedWidth += fixedWidths[ty];
    }
    return { field, ty, bit, index, offset };
  });
  layout = {
    fields: entries,
    byName: new Map(entries.map((entry) => [entry.field.name, entry])),
    bitmapBytes: Math.ceil(optionals / 8),
    fixedCount,
    fixedWidth,
  };
  accessLayouts.set(fields, layout);
  return layout;
}

function present(r: Cursor, header: number, bit: number): boolean {
  return bit < 0 || (r.data[header + (bit >> 3)] & (1 << (bit & 7))) !== 0;
}

export type { AccessField, AccessLayout };
export { object, setField, isOptional, versionLayout, accessLayout, present };
