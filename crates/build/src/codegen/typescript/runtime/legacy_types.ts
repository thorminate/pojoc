import type { Primitive, Cursor, CursorOps, Obj, ReadTable } from "./types.js";

type Variant = [number, string, WireType];
type WireType =
  | Primitive
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
type Field = {
  id: string;
  name: string;
  ty: WireType;
  default: unknown;
  seek: boolean;
  lazy: boolean;
  cast: boolean;
};
type BoundLayout = (r: Cursor, table: ReadTable) => Obj;
type CursorReads = CursorOps & {
  field: (r: Cursor, table: ReadTable, index: number) => unknown;
};
type LayoutReader = (r: Cursor, table: ReadTable, read: CursorReads) => Obj;
type FixedLayout = {
  width: number;
  read: (r: Cursor, pos: number, read: CursorOps) => Obj;
};
type RootSpec = {
  fields: Field[];
  intern: boolean;
  decodeCursor?: LayoutReader;
};
type SchemaSpec = {
  latest: string;
  roots: Record<string, RootSpec>;
  records: Record<string, Field[]>;
  cursorRecordReaders?: Record<string, LayoutReader | FixedLayout>;
  runtime?: "bun" | "node";
  imports: Record<string, number>;
};
export type {
  WireType,
  Field,
  BoundLayout,
  CursorReads,
  LayoutReader,
  FixedLayout,
  RootSpec,
  SchemaSpec,
};
