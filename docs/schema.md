# Pojoc Schema Reference

A `.pojoc` file describes a versioned binary format. The compiler turns it into
Rust encode/decode/size-hint code where the generated struct always reflects the
**latest** version, and per-version functions translate to and from every
historical wire layout — so old data keeps decoding as the schema evolves.

This is a reference for the schema language itself. For benchmarks and setup see
[`README.md`](../README.md).

- [File structure](#file-structure)
- [Field types](#field-types)
- [Declared types](#declared-types-enum-struct-union-bitset)
- [Generics](#generics)
- [Defaults](#defaults)
- [Modifiers: `const` and `lazy`](#modifiers-const-and-lazy)
- [Schema evolution](#schema-evolution)
- [Imports](#imports)
- [Comments](#comments)
- [CLI](#cli)

---

## File structure

A file contains exactly one `schema`, holding one or more `version` blocks.

```pojoc
schema Player {
  version 1 {
    // type declarations (enum / type / union / bitset) ...

    fields {
      // the root message's fields
      name: string = "Player"
    }
  }

  version 2 {
    diff {
      // how the root message changed since v1
      + level: i32 = 1
    }
  }
}
```

- The **first** version declares the root message in a `fields { }` block.
- **Later** versions describe changes in a `diff { }` block (see
  [Schema evolution](#schema-evolution)).
- Any version may declare or evolve named types (`enum`, `type`, `union`,
  `bitset`) alongside its `fields`/`diff` block.
- Versions are integers and each must be unique; a type or field reference
  resolves to the latest version at or below the point it's used.

---

## Field types

### Primitives

| Category      | Types                  | Aliases                                   |
|---------------|------------------------|-------------------------------------------|
| Unsigned ints | `u8` `u16` `u32` `u64` | `byte`/`uchar`, `ushort`, `uint`, `ulong` |
| Signed ints   | `i8` `i16` `i32` `i64` | `char`, `short`, `int`, `long`            |
| Floats        | `f32` `f64`            | `float`, `double`                         |
| Bool          | `bool`                 | `boolean`                                 |
| String        | `string`               | `str`                                     |


### Varints

```pojoc
count: varint32 = 0     // 1–5 bytes, value range of u32
big:   varint64 = 0     // 1–10 bytes, value range of u64
```

LEB128-style variable-length integers. Smaller values take fewer bytes, is slower to encode/decode however.

### Fixed-length string

```pojoc
code: string(8) = "00000000"   // exactly 8 bytes on the wire, no length prefix
```

Decodes to `[u8; N]`. The default literal's byte length must equal `N`.

### Ranged / quantized floats (`vfloat`)

```pojoc
angle: vfloat(min: 0, max: 360, step: 0.1) = 0.0
```

Packs a float in `[min, max]` at resolution `step` into the fewest bytes the
range needs (`(max-min)/step` steps → `u16` or `u32` backing). Decodes to `f32`.
Lossy to `step`; ideal for angles, normalized values, positions.

### Arrays

```pojoc
tags:     [string]           = []            // length-prefixed, variable count
hotbar:   [string](6)        = [..""]        // fixed 6 elements, no length prefix
scores:   [i32](delta)       = []            // delta-encoded (see below)
frames:   [u8](delta, 8)     = [..0]         // fixed-length delta array
```

- `[T]` — variable-length, length-prefixed.
- `[T](N)` — exactly `N` elements, no length prefix.
- `[T](delta)` — **delta encoding**: stores successive differences as varints.
  Great for sorted or slowly-changing integer sequences. Integer element types
  only (`u8`..`u64`, `i8`..`i64`).
- `[T](delta, N)` — fixed-length delta array.

### Tuples

```pojoc
coordinates: (f32, f32)           = (0.0, 0.0)
loadout:     [(string, i32)](4)   = [..("", 0)]
```

Heterogeneous fixed-arity groups. A tuple of only fixed-width elements is itself
fixed-width on the wire.

### Maps

```pojoc
config:  map<string, string>       = {}
scores:  map<string, i64>(4)       = {"ok": -1, "bad": 0, "x": 1, "y": 2}
```

- `map<K, V>` — variable-size, length-prefixed.
- `map<K, V>(N)` — fixed `N` entries.

Values (and keys) may themselves be any type, including nested maps/arrays and
unions: `map<string, [map<i32, bool>]>`.

### Optional

```pojoc
nickname: string?
level:    i32?
```

`T?` is present-or-absent, packed into a per-message optional-flags header.
Decodes to `Option<T>`.

### Nested declared types

Reference any `type`/`enum`/`union`/`bitset` by name:

```pojoc
stats:  Stats            // a struct
status: Status           // an enum
action: Payload          // a union
perms:  SystemPrivileges // a bitset
```

---

## Declared types (`enum`, `type`, `union`, `bitset`)

Declared inside a `version` block, referenced by later fields.

### `enum`

```pojoc
enum Status {
  Alive,   // first variant is the Default
  Dead,
  Dying,
}
```

Wire values are assigned by declaration order. Decoding an unknown discriminant
is an error.

### `type` (struct)

```pojoc
type Stats {
  strength: i32 = 0
  agility:  i32 = 0
}

type Empty {}   // zero-field structs are allowed
```

Struct fields follow the same type/default rules as root fields.

### `union` (tagged)

```pojoc
union Payload {
  Move:   MovePayload,
  Attack: AttackPayload,
}
```

A discriminant + length-prefixed payload. An unrecognized discriminant is
preserved losslessly as an `Unknown { discriminant, data }` variant (so a proxy
running an older schema can forward newer variants unchanged). Payloads are
typically structs.

### `bitset`

```pojoc
bitset SystemPrivileges {
  Read,
  Write,
  Execute,
  Admin,
}
```

A packed set of boolean flags (1/2/4 bytes by flag count). Generated code gets
getters/setters/`with_*` builders and `|` `&` `!` operators. See
[Defaults](#defaults) for bitset default syntax.

---

## Generics

Templates are monomorphized per distinct instantiation — each `(template, args)`
becomes its own generated struct.

```pojoc
type Box<T>          { value: T }
type Pair<A, B>      { first: A  second: B }

fields {
  boxed:  Box<i32>              // -> generated type BoxI32
  pair:   Pair<i32, string>     // -> generated type PairI32String
  flag:   Box<bool> as FlagBox  // name the monomorphized type explicitly
}
```

- `as Alias` names the generated struct instead of using the auto-mangled name.
- Generics compose with everything: `[Box<i32>]`, `map<string, Box<bool>>`, etc.

Generics also evolve (see [Schema evolution](#schema-evolution)), including
crossing template names and dropping a type parameter with `_`.

---

## Defaults

A field may declare a default with `= value`. Defaults fill in fields that are
missing when decoding older data, and back the generated `Default` impl.

| Type                     | Default syntax                                                           |
|--------------------------|--------------------------------------------------------------------------|
| Int / float              | `42`, `-1`, `3.14`, `3.40282347e38`, `-1.79e+308`                        |
| Bool                     | `true` / `false`                                                         |
| String                   | `"text"`                                                                 |
| Fixed string `string(N)` | `"literal"` (byte length must equal `N`)                                 |
| Array                    | `[]`, `[1, 2, 3]`                                                        |
| Fixed array              | `[..x]` fills all slots with `x` — e.g. `[..0]`, `[..""]`, `[..("", 0)]` |
| Map                      | `{}`, `{"k": v, ...}`                                                    |
| Enum                     | `Status::Alive`                                                          |
| Tuple                    | `(0.0, 0.0)`                                                             |
| Bitset                   | `SystemPrivileges(Read: true, Write: true)`, or `0` for empty            |
| `vfloat`                 | a plain float literal, e.g. `0.0`                                        |

```pojoc
region:    Region        = Region::North
spawn:     (f32,f32,f32) = (0.0, 0.0, 0.0)
hotbar:    [string](6)   = [..""]
flags:     HardwareFlags = HardwareFlags(IsCpuBound: true, HasVulkan: true)
perks:     Perks         = 0
```

---

## Modifiers: `const` and `lazy`

### `const`

```pojoc
max_hp: const f32  = 100.0
verified: const bool = true
```

A compile-time constant baked into the generated type as an associated `const`
(not encoded on the wire). Holds primitive values only.

### `lazy`

```pojoc
audit_log: lazy [string]?
big_blob:  lazy [u8] = []
```

The field's bytes are **skipped** on decode and kept as a raw slice; you decode
them on demand via `LazyView::get()`. Use it for large, rarely-inspected
payloads to get near-zero-copy decode speed for the fields that don't need eager
decoding.

> A `lazy` field **added in a `diff`** must be optional (`?`), so that older
> messages lacking it decode to `None`.

---

## Schema evolution

The generated struct is always the latest version. Each historical version gets
`encode_vN` / `decode_vN` functions, and the compiler maps every field across
versions by a stable identity (so a field survives renames and retypes). Two
mechanisms drive evolution: `diff` for the root message, and `extends` for named
types.

### `diff` — evolving the root message

```pojoc
version 3 {
  diff {
    + tags: [string] = []            // add a field
    - name                           // remove a field
    ~ level: f32                     // retype an existing field
    ~ id -> player_id: f64           // rename (and optionally retype)
  }
}
```

| Op | Meaning |
|---|---|
| `+ field: Type = default` | add a new field (needs a default unless optional) |
| `- field` | remove a field |
| `~ field: NewType` | retype in place |
| `~ old -> new: Type` | rename `old` to `new` (with optional retype) |

Old wire data still decodes: removed fields are read and discarded, added fields
fall back to their default, renamed/retyped fields are mapped through.

> A struct-typed (or generic) field can't be retyped to an *incompatible* shape
> in place. When a nested type's shape changes incompatibly, add a **new** field
> pinned to the new type rather than retyping the old one.

### `extends` — evolving a named type

`enum`, `type`, `union`, and `bitset` evolve by redeclaring them with
`extends X@N`, where `@N` pins the version being extended:

```pojoc
enum Status extends Status@1 {
  + Disqualified        // add a variant
  ~ Dying -> Downed     // rename a variant
}

type Stats extends Stats@1 {
  + luck: i32 = 0       // add a field
}

union Payload extends Payload@1 {
  + Heal: HealPayload   // add a variant (add-only)
}

bitset Flags extends Flags@1 {
  + IsStreamer          // add a flag
  - IsMuted             // remove a flag
}
```

Note: because a type's evolution produces a *new* type version, a root field of
that type only "sees" the new shape when you re-pin it in the `diff`
(`~ stats: Stats`) — otherwise it keeps decoding at its original version. New
fields added fresh in a later version automatically use the most current type.

### Evolving generics

Generic templates evolve like any other type, and can cross template names or
drop a parameter with `_`:

```pojoc
type Box<T> extends Box<T>@1 { + label: string = "unlabeled" }  // carry T through
type Duo<A, B> extends Mono<A>@3 { + secondary: B? }            // add a param
type Mono<A> extends Duo<A, _>@4 { - secondary }                // drop B via `_`
```

Every field that used a dropped parameter must be cleaned up in that same
`extends` body, or it's a hard error.

---

## Imports

Pull in another schema and reference its root message as a field type:

```pojoc
schema Edge {
  import "player.pojoc" as Player

  version 1 {
    fields {
      owner: Player@2          // pin a specific version of the imported root
    }
  }
}
```

Imports are compiled as nested modules. The `@N` pins which version of the
imported message the field expects; re-pin it in a later `diff` to advance it
(`~ owner -> new_owner: Player@6`).

---

## Comments

```pojoc
// a line comment

/// a doc comment — carried through onto the generated Rust item
type Stats { ... }
```

`///` doc comments attach to the following declaration (schema, type, field,
variant) and appear as `///` docs in the generated code.

---

## CLI

```sh
pojoc build <input.pojoc> --out-dir out/   # generate <stem>.rs (default out dir: out/)
pojoc check <input.pojoc>                  # validate without writing output
```

Both accept `--verbose`. In a build script, prefer
`pojoc_build::compile_dir("schemas", &out_dir)` (see the README's
[setup section](../README.md#using-it-in-your-project)).
