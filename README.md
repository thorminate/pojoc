# Pojoc

[![CI](https://github.com/thorminate/pojoc/actions/workflows/ci.yml/badge.svg)](https://github.com/thorminate/pojoc/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/pojoc.svg?label=pojoc)](https://crates.io/crates/pojoc)
[![crates.io](https://img.shields.io/crates/v/pojoc-build.svg?label=pojoc-build)](https://crates.io/crates/pojoc-build)
[![crates.io](https://img.shields.io/crates/v/pojoc-cli.svg?label=pojoc-cli)](https://crates.io/crates/pojoc-cli)
[![Release](https://img.shields.io/github/v/release/thorminate/pojoc)](https://github.com/thorminate/pojoc/releases)
[![License](https://img.shields.io/github/license/thorminate/pojoc)](LICENSE)

A schema compiler for a compact binary wire format, built around first-class schema evolution over time. It allows you to encode and decode to and from any version defined in the schema.

Here is an example of a schema in pojoc:

```pojoc
schema Player {
  version 1 {
    enum Status { Alive, Dead }

    type Stats {
      strength: i32
      agility: i32
    }

    fields {
      name: string = "Player"
      status: Status = Status::Alive
      stats: Stats
      inventory: [string] = []
    }
  }

  version 2 {
    // you can even evolve types with diff syntax!
    type Stats extends Stats@1 {
      + luck: i32 = 0
    }

    diff {
      ~ name -> display_name           // renamed, type unchanged
      + level: i32 = 1                 // new field, decoding older data makes this field decode to 1.
      ~ stats: Stats                   // even if you evolved the type,
        // it still counts as a new type
        // so you have to retype the field
        // for it to take effect
    }
  }
}
```

Running the build command in the cli (`pojoc build <file.pojoc>`) will generate a .rs file in the out dir (default is `out/`, can be changed with the --out-dir argument) with encoding and decoding functions to convert the generated structs into `Vec<u8>` and then decode from `&[u8]`.

See [`docs/schema.md`](docs/schema.md) for the full schema language reference.

## What you get

- **Schema evolution working perfectly out of the box**, you can decode from and encode to any version.
- **An uber-compact wire format**: varint integers, delta-encoded integer arrays, quantized floats (`vfloat(min, max, step)` packs a ranged float into as few bytes as the range needs), fixed-size arrays/strings/maps with no length prefix, lazy fields that skip decoding entirely until touched, and opt-in string interning (`intern string`) that dedups repeated string values into a shared per-message table.
- **Validation baked into the wire format**: `(min:, max:)` constraints on numbers, arrays, maps, and strings, enforced on both encode and decode.
- **Recursive types** via `box<T>` heap indirection, with a compile-time error if a self-referencing type forgets it.
- **Cross-schema imports** (`import "other.pojoc" as Other`) compiled as nested modules, can then be referenced as a type via `field: Other@1`.
- **Editor support**. A language server (`pojoc-lsp`) with real completions and hover support, used in both a VS Code extension and a JetBrains plugin.

## Using it in your project

```sh
cargo add pojoc
cargo add --build pojoc-build
cargo add serde --features derive
```

Want to compile schemas from the command line instead (CI checks, editor-less
validation, etc.)? Install the CLI:

```sh
cargo install pojoc-cli
pojoc build schemas/player.pojoc   # writes out/player.rs
pojoc build schemas/player.pojoc --no-serde   # omit Serialize/Deserialize (must match your `pojoc` dependency's own serde feature)
pojoc check schemas/player.pojoc   # validate without writing output
```

```rust,no_run
// build.rs
fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    pojoc_build::compile_dir("schemas", &out_dir).unwrap_or_else(|e| panic!("\n{}", e.render()));
}
```

`Error::render` gives a really neat `file:line:col` + caret-pointing-at-the-mistake error message, so an error looks nice.

Generated types derive `Serialize`/`Deserialize`, so `serde` (with the `derive`
feature) needs to be a direct dependency too. If you don't want that, disable
it: `cargo add pojoc --no-default-features` and
`cargo add --build pojoc-build --no-default-features` (the `pojoc`/`pojoc-build`
`serde` feature must be turned off on _both_ to stay consistent).

Then `include!(concat!(env!("OUT_DIR"), "/player.rs"));` wherever you want the generated module. That gives you `encode_vN(&mut Vec<u8>, &T)` / `decode_vN(&[u8], &mut usize) -> PojocResult<T>` for the latest and every prior version:

```rust,no_run
include!(concat!(env!("OUT_DIR"), "/player.rs"));

fn main() {
    // Player::default() applies the schema's declared defaults (display_name
    // "Player", status Alive, level 1) — the same defaults used to backfill
    // fields absent from older wire data.
    let player = Player::default();

    let mut bytes = Vec::new();
    encode_v2(&mut bytes, &player);

    let mut pos = 0;
    let decoded = decode_v2(&bytes, &mut pos).unwrap();
    assert_eq!(decoded.level, player.level);
}
```

Historical envelopes use the field order and packed bitset widths of their
declared version. If an affected generator wrote a historical version using a
newer layout, those records need regeneration from source or a migration that
uses the writer's layout. The envelope version alone cannot distinguish the
two layouts.

To build a standalone helper for a schema, run:

```sh
pojoc build schemas/player.pojoc --bin
./out/player serve --stdio
./out/player serve --stdio encode < players.jsonl > players.bin
./out/player serve --stdio decode < players.bin > players.jsonl
```

`--bin` needs Cargo and a Rust toolchain. It emits the executable (`player.exe`
on Windows) alongside `out/player.rs`. A compiler built from a checkout uses
that checkout's runtime while it is available. Released binaries use the
matching published runtime version. Pass `--runtime-path <dir>` to point
Cargo at another checkout's `crates/runtime` directory.

`serve --stdio` accepts consecutive Pojoc envelopes and emits one
latest-version envelope per request. The version varint and little-endian
payload length provide framing.

`serve --stdio encode` accepts one JSON value per line and writes one
latest-version Pojoc envelope per value. `serve --stdio decode` accepts Pojoc
envelopes from supported schema versions and writes one JSON line per envelope
using the latest schema fields. JSON uses the generated Serde representation,
including decoded values for lazy fields. Both JSON modes require a
Serde-enabled build.

The helper flushes each reply. EOF between messages exits successfully, and
encode also accepts a valid final JSON value without a newline. Invalid input
exits with an error on stderr. Pojoc requests and JSON encode replies are
limited to 16 MiB of binary payload. JSON input records are limited to 64 MiB.

The generated Rust module exposes the same loop as
`serve(&mut input, &mut output, max_payload_bytes)`, with a buffered reader as
input and a caller-supplied request limit. `serve_decode` visits the decoded
value from each frame.

When both versions have a fixed-width prefix followed by the same byte array,
`serve` decodes the prefix first and reads the array directly into its reusable
reply buffer. For mixed payloads of at least 32 KiB, `serve` can also stream
unchanged length-prefixed numeric arrays whose positions can be found from
preceding field lengths. Surrounding fields use the regular codec, including
seekable metadata and version upgrades. Other layouts use buffered decoding.

Applications with the generated codec can send native Pojoc bytes and call
`encode` and `decode` directly at the endpoints. The standalone helper is useful
for exercising the codec from a shell or another process.

### Benchmarking stdio

From the repo root:

```sh
cargo bench -p pojoc-tests --bench stdio
```

Cargo builds the helper automatically from the benchmark schema and the same
stdio entry point used by `--bin`. Set `POJOC_STDIO_BIN` to an absolute path
to benchmark a different build of the same helper. Criterion reports messages
per second and time per message. Each
request is a v1 envelope that decodes and re-encodes as v2, with 256 B, 4 KiB,
64 KiB, and 1 MiB of data. Labels show the total request size on the wire.

The benchmarks compare the codec alone, spawning per message for the smallest
payload, a persistent process with one request in flight, and a persistent
process that sends up to 64 requests at a time while draining stdout. The
round-trip case measures request/reply latency. The pipelined time is elapsed
time divided by completed messages, so it measures throughput per message.
The `stdio_rkyv` group runs the same version upgrade over pipes using checked
rkyv archive access and a length-and-version header that keeps the archive aligned.

## Building n' Testing

The compiler itself (`pojoc`, `pojoc-build`, `pojoc-cli`) is plain cargo, nothing special:

```sh
cargo build -p pojoc -p pojoc-build -p pojoc-cli
cargo test -p pojoc -p pojoc-build
```

`cargo build`/`cargo test` at the workspace root also pulls in `crates/tests`,
which round-trip-tests Pojoc and benchmarks it against Protobuf, Cap'n Proto,
FlatBuffers, Bebop, and rkyv using equivalent data — that requires `protoc`,
`capnp`, and the `flatc` CLI on `PATH`. Skip it by scoping to the crates above
if you don't need those comparisons.

## Benchmarks

Measured with [Criterion](https://github.com/bheisler/criterion.rs), run `cargo bench` to see these results yourself. The schema definitions are in `/crates/tests/schemas` and benches themselves are in `/crates/tests/benches` if you want to review them.

The player and stress suites report checked rkyv archive access and owned
deserialization separately. The seek suite also measures field reads from a prevalidated
archive. The interning suites compare plain strings and shared `Arc<str>`
references drawn from the same string pool.

**Serialized size** (identical populated message, bytes. Smaller is better):

| Format      | Bytes   |
| ----------- | ------- |
| **Pojoc**   | **429** |
| Protobuf    | 463     |
| rkyv        | 608     |
| Bebop       | 634     |
| FlatBuffers | 944     |
| Cap'n Proto | 1,008   |

**Encode / decode / full round-trip** (nanoseconds, lower is better):

| Format      |  Encode | Decode | Round-trip |
| ----------- | ------: | -----: | ---------: |
| **Pojoc**   | **233** |    250 |    **492** |
| Cap'n Proto |     532 | **90** |        628 |
| Bebop       |     554 |    322 |        857 |
| FlatBuffers |     920 |    690 |      1,680 |
| Protobuf    |     903 |  1,517 |      2,630 |

Cap'n Proto gets a slight edge here because they are zero-copy (flatbuffers is too, no idea why they are so slow tho). Pojoc isn't, so they naturally get an edge there. However, with pojoc's lazy keyword you can theoretically get near zero-copy speeds and defer actually loading them until needed. Also with Cap'n Proto's zero-copy approach, they end up copying the entire memory profile plus lookup tables into the output, severely bloating its results. With pojoc being unbelievably efficient in encoding it is still the **fastest end-to-end**.

## Security

`PojocMap`/`InternBuilder` hash with a fast, non-cryptographic hasher by default, seeded randomly per process to resist hash-flooding from untrusted wire data. Enable the `secure-hasher` feature on the `pojoc` runtime crate to switch to SipHash if you need the stronger guarantee. See [`docs/security.md`](docs/security.md) for the full threat model and tradeoffs.

## Project Layout

| Crate         | What it is                                                             |
| ------------- | ---------------------------------------------------------------------- |
| `pojoc`       | Runtime support library the generated code depends on.                 |
| `pojoc-build` | Compile `.pojoc` files.                                                |
| `pojoc-cli`   | `pojoc check` / `pojoc build`, thin wrapper over `pojoc-build`         |
| `pojoc-lsp`   | Language server powering the editor extensions, built on `pojoc-build` |
| `pojoc-tests` | Round-trip tests and cross-format benchmarks                           |

Editor tooling lives outside the Cargo workspace: `vscode-extension/` (TypeScript) and `jetbrains-plugin/` (Kotlin/Gradle).

## License

MIT, see [LICENSE](LICENSE).
