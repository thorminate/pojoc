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
fixture assertions happen outside the timed loops. The harness reports
median nanoseconds per message for encode, decode, and round trip, plus mean
encoded bytes per fixture.

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

## Edge benchmark

After `npm run fixtures`, run the populated Edge benchmark with either engine.

```sh
npm run bench:edge:node
npm run bench:edge:bun
```

It measures one populated Rust fixture per version (v6, v7, and latest).
The benchmark reports the frame size and median time per encode, decode,
round trip, and dotted seek of `transform.position.x` where present. Fixture
generation, file reads, and byte-equality checks happen before timing. All
decodes return the latest Edge shape, including for historical frames.
