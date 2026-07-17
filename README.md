# Pojoc

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
      ~ name -> display_name: string   // renamed
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

## What you get

- **Schema evolution working perfectly out of the box**, you can decode from and encode to any version.
- **An uber-compact wire format**: varint integers, delta-encoded integer arrays, quantized floats (`vfloat(min, max, step)` packs a ranged float into as few bytes as the range needs), fixed-size arrays/strings/maps with no length prefix, and lazy fields that skip decoding entirely until touched.
- **Cross-schema imports** (`import "other.pojoc" as Other`) compiled as nested modules, can then be referenced as a type via `field: Other@1`.
- **Editor support**. A language server (`pojoc-lsp`) with real completions and hover support, used in both a VS Code extension and a JetBrains plugin.

## Using it in your project

```sh
cargo add pojoc
cargo add --build pojoc-build
```

```rust,no_run
// build.rs
fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    pojoc_build::compile_dir("schemas", &out_dir).unwrap_or_else(|e| panic!("{e}"));
}
```

Then `include!(concat!(env!("OUT_DIR"), "/player.rs"));` wherever you want the generated module.

## Building

plain ole' cargo, nothing special here.

```sh
cargo build
cargo test 
```

## Benchmarks

Measured with [Criterion](https://github.com/bheisler/criterion.rs) against the same `Player` schema encoded in Protobuf, Cap'n Proto, FlatBuffers, and Bebop (`cargo bench -p pojoc-tests`). Numbers are one machine, one run — treat them as a shape, not a guarantee, and re-run locally if it matters for your decision.

**Serialized size** (identical populated message, bytes — smaller is better):

| Format | Bytes |
|---|---|
| **Pojoc** | **719** |
| Protobuf | 842 |
| Bebop | 1,099 |
| FlatBuffers | 2,000 |
| Cap'n Proto | 2,128 |

**Encode / decode / full round-trip** (nanoseconds, lower is better):

| Format | Encode | Decode | Round-trip |
|---|---:|---:|---:|
| Cap'n Proto | 1,077 | **112** | 1,152 |
| **Pojoc** | **853** | 1,129 | 2,263 |
| Bebop | 1,401 | 983 | 2,772 |
| FlatBuffers | 2,324 | 2,072 | 4,457 |
| Protobuf | 3,099 | 3,368 | 6,241 |

Cap'n Proto's decode is essentially free because it's zero-copy (reading is pointer arithmetic over the wire buffer, not deserialization) — that's a real, deliberate design trade-off on its part, not a fluke. Pojoc trades that zero-copy property for a smaller wire size and still comes out fastest on encode and second-fastest end-to-end; `lazy` fields exist specifically for cases where you want decode-on-demand back for the fields that need it, without giving it up everywhere.

## The technical deets

| Crate | What it is                                                                                       |
|---|--------------------------------------------------------------------------------------------------|
| `pojoc` | Runtime support library the generated code depends on (varints, wire types, the envelope format) |
| `pojoc-build` | Compile `.pojoc` files from a `build.rs` file for example                                        |
| `pojoc-cli` | `pojoc check` / `pojoc build`, thin wrapper over `pojoc-build`                                   |
| `pojoc-lsp` | Language server powering the editor extensions, also built on `pojoc-build`                      |
| `pojoc-tests` | Round-trip tests and cross-format benchmarks                                                     |

Editor tooling lives outside the Cargo workspace: `vscode-extension/` (TypeScript) and `jetbrains-plugin/` (Kotlin/Gradle).

## License

MIT, see [LICENSE](LICENSE).
