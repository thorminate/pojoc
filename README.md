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

See [`docs/schema.md`](docs/schema.md) for the full schema language reference — every type, modifier, default, and the evolution syntax.

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

## Building n' Testing

plain ole' cargo, nothing special here.

```sh
cargo build
cargo test 
```

## Benchmarks

Measured with [Criterion](https://github.com/bheisler/criterion.rs), run `cargo bench` to see these results yourself. The schema definitions are in `/crates/tests/schemas` and benches themselves are in `/crates/tests/benches` if you want to review them.

**Serialized size** (identical populated message, bytes. Smaller is better):

| Format      | Bytes   |
|-------------|---------|
| **Pojoc**   | **429** |
| Protobuf    | 463     |
| Bebop       | 634     |
| FlatBuffers | 944     |
| Cap'n Proto | 1,008   |

**Encode / decode / full round-trip** (nanoseconds, lower is better):

| Format      |  Encode | Decode | Round-trip |
|-------------|--------:|-------:|-----------:|
| **Pojoc**   | **233** |    250 |    **492** |
| Cap'n Proto |     532 | **90** |        628 |
| Bebop       |     554 |    322 |        857 |
| FlatBuffers |     920 |    690 |      1,680 |
| Protobuf    |     903 |  1,517 |      2,630 |

Cap'n Proto gets a slight edge here because they are zero-copy (flatbuffers is too, no idea why they are so slow tho). Pojoc isn't, so they naturally get an edge there. However, with pojoc's lazy keyword you can theoretically get near zero-copy speeds and defer actually loading them until needed. Also with Cap'n Proto's zero-copy approach, they end up copying the entire memory profile plus lookup tables into the output, severely bloating its results. With pojoc being unbelievably efficient in encoding it is still the **fastest end-to-end**.

## Project Layout

| Crate         | What it is                                                             |
|---------------|------------------------------------------------------------------------|
| `pojoc`       | Runtime support library the generated code depends on.                 |
| `pojoc-build` | Compile `.pojoc` files.                                                |
| `pojoc-cli`   | `pojoc check` / `pojoc build`, thin wrapper over `pojoc-build`         |
| `pojoc-lsp`   | Language server powering the editor extensions, built on `pojoc-build` |
| `pojoc-tests` | Round-trip tests and cross-format benchmarks                           |

Editor tooling lives outside the Cargo workspace: `vscode-extension/` (TypeScript) and `jetbrains-plugin/` (Kotlin/Gradle).

## License

MIT, see [LICENSE](LICENSE).
