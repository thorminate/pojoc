# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Pojoc is a schema compiler: `.pojoc` schema files (versioned struct/enum/union/bitset/generic declarations) compile to Rust encode/decode/size-hint code for a compact binary wire format, with first-class support for evolving a schema across versions without breaking old data on the wire.

## Gotchas

- The runtime crate's package name is `pojoc`, not `pojoc-runtime` (path is `crates/runtime`) — `cargo test -p pojoc` runs it. The CLI binary is `pojoc` from package `pojoc-cli`; the LSP binary is `pojoc-lsp` from package `pojoc-lsp`.
- Building `crates/tests` requires `protoc`, `capnp`, and the `flatc` CLI on `PATH` (it round-trip-tests and benchmarks Pojoc against protobuf/Cap'n Proto/FlatBuffers/Bebop using equivalent schemas). If you only need the Pojoc compiler itself, build `pojoc-build`/`pojoc-cli` instead to skip this.
- `vscode-extension/` and `jetbrains-plugin/` bundle a **prebuilt** `pojoc-lsp` binary rather than building it — after changing `crates/lsp` (or its deps), rebuild and re-place it before the extensions pick up the change:
  - VS Code: `cargo build --release -p pojoc-lsp && cp target/release/pojoc-lsp vscode-extension/bin/pojoc-lsp`
  - JetBrains: same build, then copy to `jetbrains-plugin/src/main/resources/bin/pojoc-lsp-<rust-target-triple>` (`rustc -vV | grep host` for the triple, e.g. `aarch64-apple-darwin`) — `PojocLanguageServer.kt` matches the filename against the running arch/OS at startup, so the target-triple naming is load-bearing.

## Architecture

### The versioned schema model

The generated Rust struct reflects only the **latest** version's fields — one `struct Edge { ... }`, not one per version. Per-version `decode_vN`/`encode_vN`/`size_hint_vN` functions translate between that struct and each version's historical wire format; `SchemaLineage` (`crates/build/src/schema/ir/lineage.rs`) computes this mapping per version.

Every field has a stable `FieldId`, assigned once and preserved across renames/retypes — this is what lets lineage recognize "same field" across versions. Don't let analyzer refactors reassign these.

A named type's own evolution (`extends Foo@N { ... }`) has no `diff` keyword wrapper — the extends body's ops *are* the diff, unlike root-level fields which evolve through an explicit `diff { ... }` block.

### Generics

`type Box<T> { value: T }` / `field: Box<i32>` is monomorphized entirely at analysis time (`SchemaAnalyzer` in `crates/build/src/schema/ir/analyzer.rs`) into a synthesized `TypeId` per distinct `(template, args)` combo — codegen has zero awareness of generics, it only ever sees concrete registered types.

- `extends` is generic-aware and can cross template names, passing params through or dropping one with `_` (`type Mono<A> extends Duo<A, _>@M { - secondary }`) — every field using a dropped param must be cleaned up in that same `diff` or it's a hard error.
- `as Alias` (`Box<i32> as MyInt`) names the monomorphized type explicitly instead of the auto-mangled name (`BoxI32`); the auto-mangled name is still computed as a canonical identity, so reusing the same alias for a different `(template, args)` is rejected rather than silently misaliased.
- A struct-typed field can't be retyped across versions to an incompatible shape, since the generated Rust type name comes from `TypeId.name` alone with no cast machinery for structs — see `crates/tests/schemas/edge.pojoc`'s `generic_mono_v3`/`generic_duo_v4`/`generic_mono_v5` for the pattern of adding a new field instead of retyping one in place.

### Strings are zero-copy (borrowed) everywhere

`string` fields decode as a **borrowed `&'buf str`** into the input buffer — there is no owned string type. A decoded value therefore borrows its buffer and can't outlive it; callers own on demand with `.to_string()`. This makes every type that transitively holds a string (or a `lazy` field) *lifetime-infected*: it carries a `<'buf>` parameter. `compute_lifetime_infected` (`crates/build/src/codegen/mod.rs`) computes the infected set, and `type_info` renders infected names with `<'buf>` wherever they nest — arrays, maps, tuples, generics, imports. `FixedString` stays `[u8; N]` (unaffected). Borrowed strings inside **union payloads** are not yet supported — codegen hard-errors on them. When comparing a decoded (`'buf`) value against a `'static`-built original in tests, leak the buffer so both are `'static` (see `decode_static` in `roundtrip.rs`).

### Other non-obvious invariants

- `pojoc_build::core::types::type_info()` is the single place that maps a resolved type to its Rust type/wire size/read-write-skip-size_hint function names — extend `type_info()` for a new type kind rather than branching in codegen.
- `crates/lsp/src/completion.rs` determines cursor context with its own lightweight token scanner over raw text, not a real incremental parse — it reuses the last successfully-parsed AST (`SchemaIndex`) for names/versions/etc. while scanning the live (possibly invalid mid-edit) text for where the cursor actually is.
- `crates/tests/schemas/edge.pojoc` + `crates/tests/tests/roundtrip.rs`/`helpers/mod.rs` is the "exercises everything" schema/test pair. Extend this pair for new schema-language features rather than writing a one-off `.pojoc` fixture.
