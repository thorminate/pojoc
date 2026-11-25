# Pojoc

[![CI](https://github.com/thorminate/pojoc/actions/workflows/ci.yml/badge.svg)](https://github.com/thorminate/pojoc/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/thorminate/pojoc)](https://github.com/thorminate/pojoc/releases)
[![crates.io](https://img.shields.io/crates/v/pojoc-cli.svg)](https://crates.io/crates/pojoc-cli)

Pojoc is a schema compiler for a compact binary format. Define your data in a
`.pojoc` file and generate Rust or TypeScript code to encode and decode it.
Applications can share one schema across both languages.

A schema describes how its data changes over time. For example, you can rename
a field and add a new one with a default. A codec generated from that schema
can read earlier declared versions and return the latest shape.

Schemas can also choose how fields are stored, such as delta encoding for
integer arrays or interning for repeated strings. Mark a field `seekable` to
generate a reader for that field.

## 2.8× faster decoding than protobuf.js · 1.4× faster round trips than Cap’n Proto

| In-memory benchmark | Pojoc | Baseline |
| --- | ---: | ---: |
| Bun · Decode a 128-record directory page | **16.05 µs** · 62k/s | protobuf.js · 45.19 µs · 22k/s |
| Rust · Build, serialize, and read a populated player record | **0.98 µs** | Cap’n Proto · 1.35 µs |

Each directory record contains an ID, name, region, role, level, active flag,
and optional note. The Rust fixture includes inventory, flags, and nested
position data. Its round trip ends with a decoded Pojoc struct or a Cap’n Proto
root reader. See the [Rust round-trip benchmark](crates/tests/benches/roundtrip.rs).

### Over HTTP

In the wire demo, 16 concurrent clients fetched 128-record directory pages
from a Rust service through a Bun gateway over loopback HTTP.

| Transport | Requests/s | p95 latency | Reply body |
| --- | ---: | ---: | ---: |
| **Pojoc** | **18.6k** | **1.93 ms** | **4.42 KB** |
| Protobuf | 14.3k | 2.22 ms | 7.15 KB |
| JSON | 2.18k | 9.12 ms | 15.5 KB |
| GraphQL | 1.61k | 15.49 ms | 15.5 KB |

At a simulated **4 KiB/s** between client and gateway, the same page took
**1.11 s** with Pojoc, **1.78 s** with Protobuf, and **3.82 s** with JSON.
With gzip, those times were **396 ms**, **454 ms**, and **465 ms**.

For single-record lookups, Pojoc and Protobuf both handled about **23k
requests/s** in the binary HTTP rerun.

Measured on a Ryzen 7 5800X3D with Bun 1.3.12 and Rust 1.96. The HTTP table
reports median round throughput and mean uncompressed reply bodies on the Rust
hop. See the [TypeScript benchmark guide](crates/tests/typescript/README.md) and
the [Rust benchmark suite](crates/tests/benches/) for more workloads.

## Installation

### CLI

With Rust and Cargo installed, run the following in your terminal.

```sh
cargo install pojoc-cli --locked
pojoc --version
```

Your terminal needs Cargo's bin directory on `PATH`. This is typically
`$HOME/.cargo/bin` on Unix or `%USERPROFILE%\.cargo\bin` on Windows.
Prebuilt CLI binaries and editor plugins are available on the
[releases page](https://github.com/thorminate/pojoc/releases).

### Rust projects

Run these commands in the directory containing your project's `Cargo.toml`.
The default Rust generator uses Serde derives.

```sh
cargo add pojoc
cargo add --build pojoc-build
cargo add serde --features derive
```

Use [`pojoc-build` from `build.rs`](crates/build/src/lib.rs) to compile your
schemas during the Cargo build.

### TypeScript projects

With Node.js and npm installed, run this in the directory containing your
project's `package.json`. Use `npm init -y` first if you're starting a new
project.

```sh
npm install pojoc
```

Generated modules use the npm runtime. Use the same release of the runtime
and CLI. See the [TypeScript runtime guide](packages/runtime/README.md) for
the generation command.

## Documentation

- [Schema language and examples](docs/schema.md)
- [Security model](docs/security.md)

Pojoc is licensed under the [MIT License](LICENSE).
