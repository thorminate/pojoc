# TypeScript interoperability and benchmarks

From this directory, install the pinned protobuf.js benchmark dependency and
generate the TypeScript codecs. Node 24 or later can run the TypeScript sources
directly. Bun can run the same sources.

```sh
npm ci
npm run generate
npm run bench:node
npm run bench:bun
```

The benchmark uses matched telemetry rows with 16, 256, and 4,096 floats and
actor IDs per message. Pojoc uses its versioned envelope and protobuf.js uses
length-delimited messages with packed repeated fields. The protobuf ID field
uses `uint32` varints. The Pojoc codecs differ only in the actor ID field,
which uses fixed-width `[u32]`, `[varint32]`, or `[u32](delta)`. Each fixture's
IDs increase by seven. The decode loop reads the last float and last actor ID
from each decoded message. Schema loading, setup, object preparation, and
fixture assertions happen outside the timed loops. The harness interleaves
codecs and reports median nanoseconds per message for encode, decode, and
round trip, along with messages per second and mean encoded bytes.

To check Rust interoperability, generate Rust envelopes and the matching
TypeScript modules before running the fixture tests. This uses the workspace's
usual Rust test dependencies, including protoc, capnp, and flatc. The Deno 2
type check covers the runtime, generated codecs, fixture test, and Edge benchmark.

```sh
npm run fixtures
npm run typecheck
npm run test:node
npm run test:bun
```

## Edge and Player benchmark

After `npm run fixtures`, run the populated Edge benchmark with either engine.

```sh
npm run bench:edge:node
npm run bench:edge:bun
```

The benchmark compares Player and the latest Edge shape against
[`edge.proto`](edge.proto) with protobuf.js. Edge cases include the Rust
version-probe and populated fixtures, plus a larger fixture that fills nested
maps, lists, blobs, and numeric arrays. It also measures Pojoc v6 and v7
upgrade work and the dotted seek of `transform.position.x` where available.

Both codecs use framed messages. Pojoc uses its versioned envelope and
protobuf.js uses a length-delimited message. The harness checks every logical
field before timing, including optional presence, 64-bit values, and union
payloads. It times each decoder's native output. Protobuf input preparation
and conversion between interface shapes happen outside the timed loops.

The Protobuf schema uses native maps, messages, and `oneof` fields. Ordered
fixed maps and tuples use repeated entry messages. Optional collections use
wrappers to preserve the distinction between absent and present-empty.
Quantized Pojoc fields use Protobuf floats holding the same decoded value.
Delta arrays use their original values, and interned strings are ordinary
strings. Pojoc's schema constants live outside both messages. Validation,
historical upgrades, and generated seek methods are specific to Pojoc.

## Wasm SIMD experiment

Build the npm runtime first, then run the delta kernel comparison from the
repository root. This needs Clang with a wasm32 backend and `wasm-ld`.

```sh
npm --prefix packages/runtime run build
node crates/tests/typescript/experiments/delta_simd.mjs
bun crates/tests/typescript/experiments/delta_simd.mjs
```

The JavaScript cases allocate a fresh output array. The resident Wasm cases
reuse input and output in Wasm memory. The copied Wasm cases copy input into
Wasm and return a fresh output array. The timer excludes module compilation and
instantiation. The experiment compares different proportions of two-byte
deltas and checks every kernel against the runtime before timing.
