# Pojoc

A schema compiler for a compact binary wire format, built around one idea: **your schema should be allowed to change shape over time without breaking data you already wrote.**

You describe your data as a sequence of `version N { ... }` blocks. Each version can add, remove, rename, or retype fields relative to the last. Pojoc compiles that into a single Rust struct (always reflecting the latest shape) plus per-version encode/decode functions that know how to translate old wire data into it — so a `v1` blob you serialized years ago still decodes cleanly against your `v9` code.

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
    // Stats@1 -> Stats@2: grew a field, same identity
    type Stats extends Stats@1 {
      + luck: i32 = 0
    }

    diff {
      ~ name -> display_name: string   // renamed, same wire slot
      + level: i32 = 1                 // new field, old data defaults to 1
      ~ stats: Stats                   // re-pin to the v2 shape
    }
  }
}
```

`cargo run -p pojoc-cli -- build player.pojoc --out-dir src/generated` turns that into a Rust module with `encode`/`decode`/`encode_for_version`/`supported_versions` — no hand-written migration code, no separate IDL runtime to link against.

## What you get

- **Schema evolution as a first-class concept** — `extends`/`diff` for structs, enums, unions, and bitsets, with stable field identity preserved across renames so old and new wire data stay compatible.
- **Generics** — `type Box<T> { value: T }`, monomorphized at compile time into ordinary structs, including generic-aware evolution (`extends` can cross template names, add or drop type parameters) and `as Alias` when you want to name an instantiation yourself.
- **A wire format built for size**, not just speed: varint integers, delta-encoded integer arrays, quantized floats (`vfloat(min, max, step)` packs a bounded float into as few bits as the range needs), fixed-size arrays/strings/maps with no length prefix, and lazy fields that skip decoding entirely until touched.
- **Cross-schema imports** (`import "other.pojoc" as Other`) compiled as nested modules, no extra build step.
- **Editor support** — a language server (`pojoc-lsp`) with real completions (including type-parameter- and generic-aware suggestions) backing both a VS Code extension and a JetBrains plugin.

## Project layout

| Crate | What it is |
|---|---|
| `pojoc-core` | Shared resolved-type model used by both the analyzer and codegen |
| `pojoc-schema` | Lexer, parser, and IR analyzer — `.pojoc` source in, resolved schema out |
| `pojoc-codegen` | Resolved schema → generated Rust source |
| `pojoc` | Runtime support library the generated code depends on (varints, wire types, the envelope format) |
| `pojoc-cli` | `pojoc check` / `pojoc build` |
| `pojoc-lsp` | Language server powering the editor extensions |
| `pojoc-tests` | Round-trip tests and cross-format benchmarks |

Editor tooling lives outside the Cargo workspace: `vscode-extension/` (TypeScript) and `jetbrains-plugin/` (Kotlin/Gradle).

## Building

```sh
cargo build --workspace
cargo test --workspace
```

See `CLAUDE.md` for the less-obvious parts of the build (external tools needed by the benchmark/comparison crate, rebuilding the editor extensions' bundled LSP binary, etc.).

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

## License

MIT, see [LICENSE](LICENSE).
