import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import protobuf from "protobufjs";
import type { Edge } from "./generated/edge.ts";

const root = protobuf
  .parse(readFileSync(new URL("./edge.proto", import.meta.url), "utf8"), {
    keepCase: true,
  })
  .root.resolveAll();

export const protoEdge = root.lookupType("edgebench.Edge").setup();
export const protoPlayer = root.lookupType("edgebench.Player").setup();

const collectionWrappers = new Set([
  "I32List",
  "I64List",
  "StringList",
  "IntBoolMap",
  "IntBoolMapList",
  "MatrixRow",
]);
const tupleRecords = new Set([
  "BlobEntry",
  "IntFloatEntry",
  "StringI64Entry",
  "StringI64ListEntry",
  "StringI32Entry",
  "NullableIntEntry",
  "MatrixCell",
]);
const wideIntegers = new Set(["sint64", "uint64"]);

type RecordValue = Record<string, unknown>;

function record(value: unknown): RecordValue {
  assert(value !== null && typeof value === "object");
  return value as RecordValue;
}

function hasPresence(field: protobuf.Field): boolean {
  return (
    !field.repeated &&
    !field.map &&
    Boolean(field.hasPresence || field.resolvedType instanceof protobuf.Type)
  );
}

function variant(type: protobuf.Type, value: RecordValue): RecordValue {
  const kind = value.kind;
  assert(typeof kind === "string");
  if (kind === "Unknown") {
    return { unknown: { discriminant: value.discriminant, data: value.data } };
  }
  if (type.name === "WidthUnion") {
    const match = /^(V[0-2]|W[0-9]{1,3})$/.exec(kind);
    assert(match, `Invalid UnionWidth variant ${kind}`);
    const ordinal = Number(kind.slice(1));
    assert(ordinal <= 259);
    return {
      known: {
        variant: ordinal + (kind[0] === "V" ? 1 : 4),
        value: value.value,
      },
    };
  }
  const name = kind.replace(/([a-z])([A-Z])/g, "$1_$2").toLowerCase();
  assert(
    Object.hasOwn(type.fields, name),
    `Missing ${type.name} arm for ${kind}`,
  );
  return { [name]: value.value };
}

function messageFields(type: protobuf.Type, value: unknown): RecordValue {
  if (type.oneofs?.kind) return variant(type, record(value));
  if (collectionWrappers.has(type.name)) return { values: value };
  if (tupleRecords.has(type.name)) {
    assert(
      Array.isArray(value) && value.length === type.fieldsArray.length,
      type.name,
    );
    return Object.fromEntries(
      type.fieldsArray.map((field, index) => [field.name, value[index]]),
    );
  }
  return record(value);
}

function projectMessage(type: protobuf.Type, value: unknown): RecordValue {
  const source = messageFields(type, value);
  for (const name of Object.keys(source)) {
    assert(
      Object.hasOwn(type.fields, name),
      `Protobuf omits ${type.name}.${name}`,
    );
  }
  const union = type.oneofs?.kind;
  const projected: RecordValue = {};
  for (const field of type.fieldsArray) {
    if (union && !Object.hasOwn(source, field.name)) continue;
    assert(
      Object.hasOwn(source, field.name),
      `No Edge value for ${type.name}.${field.name}`,
    );
    const item = source[field.name];
    if (item === null) {
      assert(hasPresence(field), field.fullName);
      continue;
    }
    const projectItem = (entry: unknown): unknown => {
      if (field.resolvedType instanceof protobuf.Type)
        return projectMessage(field.resolvedType, entry);
      if (typeof entry === "bigint") {
        assert(wideIntegers.has(field.type), field.fullName);
        return entry.toString();
      }
      return entry;
    };
    if (field.map) {
      assert(item instanceof Map, field.fullName);
      projected[field.name] = Object.fromEntries(
        Array.from(item, ([key, entry]) => [key, projectItem(entry)]),
      );
    } else if (field.repeated) {
      assert(
        Array.isArray(item) || ArrayBuffer.isView(item) || item instanceof Map,
        field.fullName,
      );
      projected[field.name] = Array.from(
        item as Iterable<unknown>,
        projectItem,
      );
    } else {
      projected[field.name] = projectItem(item);
    }
  }
  return projected;
}

function assertItem(
  field: protobuf.Field,
  observed: unknown,
  expected: unknown,
  path: string,
): void {
  if (field.resolvedType instanceof protobuf.Type) {
    assertMessage(field.resolvedType, record(observed), record(expected), path);
  } else if (wideIntegers.has(field.type)) {
    assert.equal(String(observed), expected, path);
  } else if (field.type === "bytes") {
    assert.deepEqual(
      Uint8Array.from(observed as Uint8Array),
      Uint8Array.from(expected as Uint8Array),
      path,
    );
  } else {
    assert.equal(observed, expected, path);
  }
}

function assertMessage(
  type: protobuf.Type,
  decoded: RecordValue,
  expected: RecordValue,
  path: string,
): void {
  for (const field of type.fieldsArray) {
    const name = field.name;
    const location = `${path}.${name}`;
    if (hasPresence(field)) {
      assert.equal(
        Object.hasOwn(decoded, name),
        Object.hasOwn(expected, name),
        location,
      );
    }
    if (!Object.hasOwn(expected, name)) continue;
    const actual = decoded[name];
    const wanted = expected[name];
    if (field.map) {
      const actualEntries = record(actual);
      const wantedEntries = record(wanted);
      assert.deepEqual(
        Object.keys(actualEntries).sort(),
        Object.keys(wantedEntries).sort(),
        location,
      );
      for (const key of Object.keys(wantedEntries)) {
        assertItem(
          field,
          actualEntries[key],
          wantedEntries[key],
          `${location}[${key}]`,
        );
      }
    } else if (field.repeated) {
      assert(Array.isArray(actual) && Array.isArray(wanted), location);
      assert.equal(actual.length, wanted.length, location);
      for (let index = 0; index < wanted.length; index++) {
        assertItem(
          field,
          actual[index],
          wanted[index],
          `${location}[${index}]`,
        );
      }
    } else {
      assertItem(field, actual, wanted, location);
    }
  }
}

export function protobufFixture(type: protobuf.Type, source: unknown) {
  const expected = projectMessage(type, source);
  const input = type.fromObject(expected);
  const frame = type.encodeDelimited(input).finish();
  const decoded = type.decodeDelimited(frame);
  assertMessage(type, record(decoded), expected, type.name);
  return { input, frame };
}

export function edgeWithCollections(value: Edge): Edge {
  return {
    ...value,
    delta_positions: Int32Array.from(
      { length: 512 },
      (_, i) => 200 + 7 * i + (i % 13 === 0 ? 3 : 0),
    ),
    plain_f32_arr: Float32Array.from({ length: 128 }, (_, i) => i / 8),
    packed_varints: Array.from({ length: 256 }, (_, i) => i * 130),
    interned_tags: Array.from({ length: 24 }, (_, i) =>
      i % 2 ? "shared-tag" : "unique-tag",
    ),
    lazy_audit_log: [],
    opt_delta_arr: new Int32Array(),
    opt_i64: 0n,
    opt_vfloat: 0,
    lazy_annotated_blob: [[0xffff_ffff_ffff_ffffn, Uint8Array.of(0, 128, 255)]],
    nested_map: new Map([
      [
        "zones",
        [
          new Map([
            [-3, true],
            [0, false],
          ]),
          new Map(),
        ],
      ],
    ]),
    lazy_sensor_dump: new Map([
      ["track", Int32Array.from({ length: 256 }, (_, i) => i)],
    ]),
    ultimate_boss_structure: {
      ...value.ultimate_boss_structure,
      matrix: [
        [
          [
            new Map<string, number | null>([
              ["present", -3],
              ["zero", 0],
              ["missing", null],
            ]),
            null,
          ],
          [new Map(), new Uint8Array(8)],
          [new Map(), null],
          [new Map(), null],
        ],
      ],
    },
    final_action: {
      kind: "Rich",
      value: { note: "shared-tag", blob: Uint8Array.of(7, 8, 9) },
    },
    control_log: [
      ...value.control_log,
      { kind: "Unknown", discriminant: 17, data: Uint8Array.of(1, 2) },
    ],
  };
}
