type Primitive =
  | "u8"
  | "i8"
  | "u16"
  | "i16"
  | "u32"
  | "i32"
  | "u64"
  | "i64"
  | "f32"
  | "f64"
  | "bool"
  | "string"
  | "varint32"
  | "varint64"
  | "intern";
type FixedScalar = Exclude<
  Primitive,
  "string" | "varint32" | "varint64" | "intern"
>;
type Cursor = {
  readonly data: Uint8Array;
  view: DataView | undefined;
  pos: number;
  end: number;
};
// An entry is either a decoded string or the offset of its length prefix.
type ReadTable = (string | number)[];
type CursorOps = {
  take(r: Cursor, count: number): number;
  view(r: Cursor): DataView;
  varint32(r: Cursor, extendedLength?: boolean): number;
  varint(r: Cursor): bigint;
  string(r: Cursor): string;
  intern(r: Cursor, table: ReadTable): string;
};
type Obj = Record<string, unknown>;
type Input = Uint8Array | ArrayBuffer;
type Version = number | bigint;

export type {
  Primitive,
  FixedScalar,
  Cursor,
  CursorOps,
  ReadTable,
  Obj,
  Input,
  Version,
};
