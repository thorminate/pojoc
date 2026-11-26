# Pojoc

[![CI](https://github.com/thorminate/pojoc/actions/workflows/ci.yml/badge.svg)](https://github.com/thorminate/pojoc/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/thorminate/pojoc)](https://github.com/thorminate/pojoc/releases)
[![crates.io](https://img.shields.io/crates/v/pojoc-cli.svg)](https://crates.io/crates/pojoc-cli)

Pojoc generates Rust and TypeScript codecs for a compact binary format.
Write a `.pojoc` schema once and use it to pass messages between both
languages.

When your needs change, you can remove, typecast,
add and rename fields, and the generated decoder will be able to read messages
written by earlier versions and automagically upgrade the data to the latest
format.

Mark a field `seekable` to read it straight from the encoded bytes if you need
to quickly read a field in transit. Use delta encoding for sequences of nearby
integers or interning to store repeated strings once per message.

## 2.8× faster decoding than protobuf.js · 1.4× faster round trips than Cap’n Proto

| In-memory benchmark                                         |                Pojoc |                       Baseline |
|-------------------------------------------------------------|---------------------:|-------------------------------:|
| Bun · Decode a 128-record directory page                    | **16.05 µs** · 62k/s | protobuf.js · 45.19 µs · 22k/s |
| Rust · Build, serialize, and read a populated player record |          **0.98 µs** |          Cap’n Proto · 1.35 µs |

The directory entries contain an ID, name, region, role, level, active flag,
and optional note. The player record has inventory, flags, and a nested
position. The Rust round trip ends with a decoded Pojoc struct or a Cap’n
Proto root reader. [Rust round-trip benchmark](crates/tests/benches/roundtrip.rs)

### TypeScript field access

Each test reads one field from a record containing 4,096 other strings. The
encoded record is 69.7 KB in Pojoc, 114.8 KB in FlatBuffers, and 164.0 KB in
Cap’n Proto. The table shows median times on Node 26.13.1 and Bun 1.3.12.

| Field                         | Pojoc, Node / Bun | FlatBuffers, Node / Bun | Cap’n Proto, Node / Bun |
|-------------------------------|------------------:|------------------------:|------------------------:|
| Fixed `id`                    |    **22 / 25 ns** |              63 / 40 ns |          56.8 / 16.8 µs |
| Nested `transform.position.x` |    **30 / 30 ns** |              94 / 72 ns |          35.6 / 17.0 µs |
| String `label`                |  **140 / 126 ns** |            185 / 162 ns |          36.0 / 17.8 µs |

The Cap’n Proto times include the segment copies `capnp-es` 0.0.17 makes
when it opens a message.

In the current Pojoc layout, `label` comes before the array. In the earlier
layout, the reader skips the array to reach it.

### Over HTTP

Sixteen clients fetched 128-record directory pages concurrently from a Rust
service through a Bun gateway. All requests ran over loopback HTTP.

| Transport | Requests/s | p95 latency | Reply body |
| --- | ---: | ---: | ---: |
| **Pojoc** | **18.6k** | **1.93 ms** | **4.42 KB** |
| Protobuf | 14.3k | 2.22 ms | 7.15 KB |
| JSON | 2.18k | 9.12 ms | 15.5 KB |
| GraphQL | 1.61k | 15.49 ms | 15.5 KB |

At a simulated **4 KiB/s** between client and gateway, the same page took
**1.11 s** with Pojoc, **1.78 s** with Protobuf, and **3.82 s** with JSON.
With gzip, those times were **396 ms**, **454 ms**, and **465 ms**.

Pojoc and Protobuf each handled about **23k requests/s** in the single-record
HTTP test.

The HTTP tests ran on a Ryzen 7 5800X3D with Bun 1.3.12 and Rust 1.96. The table
uses the median request rate across runs and the mean uncompressed response
size from the Rust service. [TypeScript benchmarks](crates/tests/typescript/README.md)
· [Rust benchmarks](crates/tests/benches/)

## Installation

The Rust and TypeScript examples use `schemas/player.pojoc`.
[Sample player schema](packages/runtime/README.md#generate-and-use-a-codec)

### CLI

If you already have Rust and Cargo, install the CLI and check its version.

```sh
cargo install pojoc-cli --locked
pojoc --version
```

Or download the prebuilt CLI or an editor plugin from the
[releases page](https://github.com/thorminate/pojoc/releases).

### Rust projects

Add the runtime and schema compiler to your Rust project.

```sh
cargo add pojoc
cargo add pojoc-build
```

Add the module to `lib.rs` or `main.rs`. The schema path is relative to
your project’s `Cargo.toml`.

```rust
pojoc_build::schema!(pub mod player from "schemas/player.pojoc");
```

Use the generated types and codec functions from the `player` module.
For Serde support, add `, serde` after the schema path and run
`cargo add serde --features derive`.

To generate codec files on disk, including TypeScript, call
[`pojoc-build` from `build.rs`](crates/build/src/lib.rs). To construct field
values, use [`pojstr!`, `pojvec!`, `pojmap!`, and `pojtup!`](docs/schema.md#rust-construction-macros).

### TypeScript projects

From the directory containing your app’s `package.json`, install the runtime
and the CLI, then generate a codec.

```sh
npm install pojoc
npm install --save-dev @pojoc/cli
npm exec -- pojoc build schemas/player.pojoc --target typescript -o src/generated
```

Import `src/generated/player.ts` from your app. Keep the CLI and npm runtime
on the same release. The compiler guide has a sample `player.pojoc`, a
round-trip example, and the available generation options.
[TypeScript compiler guide](packages/runtime/README.md)

## Documentation

- [Schema language and examples](docs/schema.md)

Pojoc is licensed under the [MIT License](LICENSE).
