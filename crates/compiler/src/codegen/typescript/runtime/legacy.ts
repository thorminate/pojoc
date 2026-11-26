import type { Input, Version } from "./types.js";
import type { SchemaSpec } from "./legacy_types.js";
import { type Envelope, openEnvelope } from "./wire.js";
import {
  createPlanned,
  decodePlanned,
  encodePlanned,
  openPlanned,
  selectPlanned,
  seekPlanned,
} from "./codec.js";

// Released v1 artifacts call this entry point. New artifacts emit these exports directly.
function makeCodec<T>(schemas: SchemaSpec[], scope: number) {
  const version = BigInt(schemas[scope].latest);
  const shortVersion = version < 128n ? Number(version) : -1;
  const openFrame = (input: Input, offset = 0) =>
    openEnvelope(input, offset, version, shortVersion);
  const decodeFrame = (r: Envelope) =>
    decodePlanned<T>(r, version, schemas, scope);
  const encodeForVersion = (value: T, target: Version) =>
    encodePlanned(value, target, schemas, scope);

  function seek<K extends keyof T & string>(input: Input, path: K): T[K];
  function seek(input: Input, path: string): unknown;
  function seek(input: Input, path: string): unknown {
    return seekPlanned(
      input,
      path,
      version,
      schemas,
      scope,
      openFrame,
      decodeFrame,
    );
  }

  return {
    version,
    encode: (value: T) => encodeForVersion(value, version),
    encodeForVersion,
    decode: (input: Input) => decodeFrame(openFrame(input)),
    decodeFrom: (input: Input, offset = 0) => {
      const r = openFrame(input, offset);
      return { value: decodeFrame(r), version: r.version, offset: r.end };
    },
    open: (input: Input, offset = 0) =>
      openPlanned(
        openFrame(input, offset),
        version,
        schemas,
        scope,
        decodeFrame,
      ),
    select: <K extends keyof T & string>(keys: readonly K[]) =>
      selectPlanned(keys, version, schemas, scope, openFrame, decodeFrame),
    create: (value: Partial<T> = {}) => createPlanned<T>(value, schemas, scope),
    seek,
  };
}

export { makeCodec };
