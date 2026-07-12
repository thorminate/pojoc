# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Pojoc is a schema compiler: `.pojoc` schema files (versioned struct/enum/union/bitset/generic declarations) compile to Rust encode/decode/size-hint code for a compact binary wire format, with first-class support for evolving a schema across versions without breaking old data on the wire.

## Gotchas

- The runtime crate's package name is `pojoc`, not `pojoc-runtime` (path is `crates/runtime`) — `cargo test -p pojoc` runs it. The CLI binary is `pojoc` from package `pojoc-cli`; the LSP binary is `pojoc-lsp` from package `pojoc-lsp`.
- Building `crates/tests` requires `protoc`, `capnp`, and the `flatc` CLI on `PATH` (it round-trip-tests and benchmarks Pojoc against protobuf/Cap'n Proto/FlatBuffers/Bebop using equivalent schemas). If you only need the Pojoc compiler itself, build `pojoc-schema`/`pojoc-codegen`/`pojoc-cli` instead to skip this.
- `vscode-extension/` and `jetbrains-plugin/` bundle a **prebuilt** `pojoc-lsp` binary rather than building it — after changing `crates/lsp` (or its deps), you must manually rebuild and re-place it before the extensions pick up the change:
  - VS Code: `cargo build --release -p pojoc-lsp && cp target/release/pojoc-lsp vscode-extension/bin/pojoc-lsp`
  - JetBrains: same build, then copy to `jetbrains-plugin/src/main/resources/bin/pojoc-lsp-<rust-target-triple>` (`rustc -vV | grep host` for the triple, e.g. `aarch64-apple-darwin`) — `PojocLanguageServer.kt` matches the filename against the running arch/OS at startup, so the target-triple naming is load-bearing, not cosmetic.

## Architecture

### The versioned schema model

The generated Rust struct always reflects only the **latest** version's fields — there's one `struct Edge { ... }`, not one per version. Per-version `decode_vN`/`encode_vN`/`size_hint_vN` functions translate between that single struct and each version's historical wire format; `SchemaLineage` (`crates/schema/src/ir/lineage.rs`) computes this mapping per version (`FieldMapping::PassThrough`/`Cast`/`Discard`, `MissingField` defaults).

Every field has a stable `FieldId`, assigned once and preserved across renames/retypes (`~ old -> new: Type` diffs) — this is what lets lineage recognize "same field" across versions. Don't let analyzer refactors reassign these.

A named type's own evolution (`extends Foo@N { ... }`) has no `diff` keyword wrapper — the extends body's ops *are* the diff, unlike root-level fields which evolve through an explicit `diff { ... }` block.

### Generics

`type Box<T> { value: T }` / `field: Box<i32>` is monomorphized entirely at analysis time (`SchemaAnalyzer::template_shape`/`build_shape`/`resolve()` in `crates/schema/src/ir/analyzer.rs`) into a synthesized `TypeId` per distinct `(template, args)` combo — codegen has zero awareness of generics, it only ever sees concrete registered types. Two things that aren't obvious from the grammar alone:

- `extends` is generic-aware and can cross template names, passing params through or dropping one with `_` (`type Mono<A> extends Duo<A, _>@M { - secondary }`) — every field using a dropped param must be cleaned up in that same `diff` or it's a hard error.
- `as Alias` (`Box<i32> as MyInt`) names the monomorphized type explicitly instead of using the auto-mangled name (`BoxI32`); the auto-mangled name is still computed under the hood as a canonical identity so reusing the same alias for genuinely different `(template, args)` is rejected rather than silently aliasing to the wrong type.
- A struct-typed field (generic instantiation or not) can't be retyped across versions to an incompatible shape, because the generated Rust type name is derived from `TypeId.name` alone with no cast machinery for structs — see `crates/tests/schemas/edge.pojoc`'s `generic_mono_v3`/`generic_duo_v4`/`generic_mono_v5` for the pattern of adding a new field instead of retyping one in place when a generic's shape changes incompatibly.

### Other non-obvious invariants

- `pojoc_core::types::type_info()` is the single place that maps a resolved type to its Rust type/wire size/read-write-skip-size_hint function names — codegen is not supposed to special-case types itself; if you're adding a new type kind, extend `type_info()` rather than branching in `crates/codegen`.
- `crates/lsp/src/completion.rs` determines cursor context with its own lightweight token scanner over raw text, not a real incremental parse — it reuses the last successfully-parsed AST (`SchemaIndex`) for names/versions/etc. while scanning the live (possibly invalid mid-edit) text for where the cursor actually is.
- `crates/tests/schemas/edge.pojoc` + `crates/tests/tests/roundtrip.rs`/`helpers/mod.rs` is the "exercises everything" schema/test pair (all field kinds, generics, lazy fields, deltas, imports). Extend this pair for realistic end-to-end coverage of a new schema-language feature rather than writing a one-off `.pojoc` fixture.
