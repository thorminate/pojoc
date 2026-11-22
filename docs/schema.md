# Pojoc Schema Reference

A `.pojoc` file describes a versioned binary format. The compiler turns it into
Rust encode/decode/size-hint code where the generated struct always reflects the
**latest** version, and per-version functions translate to and from every
historical wire layout — so old data keeps decoding as the schema evolves.

This is a reference for the schema language itself. For benchmarks and setup see
[`README.md`](../README.md).

- [File structure](#file-structure)
- [Field types](#field-types)
- [Declared types](#declared-types)
- [Generics](#generics)
- [Defaults](#defaults)
- [Modifiers: `const`, `lazy`, `seekable`, and `intern`](#modifiers-const-lazy-seekable-and-intern)
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
| ------------- | ---------------------- | ----------------------------------------- |
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

### Constraints (`min`/`max`)

```pojoc
count:  u8(min: 0, max: 10)          = 1
tags:   [string](min: 0, max: 5)     = []
label:  string(min: 1, max: 20)      = "x"
```

Bounds a numeric value, or an array/map/string's element **count**/byte
**length**. Enforced on both encode and decode — violating a bound is a
runtime error (`Error::ConstraintViolation`), not a silent clamp.

- On numbers: bounds the value itself.
- On `string`: bounds the byte length.
- On `[T]` / `map<K, V>`: bounds the element count.
- Composes with everything else a type supports, e.g. `[string(min: 1, max: 20)]`.

> The generated `Default` impl uses each field's declared `= value` default
> (falling back to the type's zero value for a field with none). A
> constrained field's default — declared or zero — must itself satisfy the
> constraint, or encoding `T::default()` fails with `Error::ConstraintViolation`.

### `box<T>` — recursive types

```pojoc
type Node {
  value: i32 = 0
  next: box<Node>?
}
```

A type that references itself — directly, or through a cycle of types — must
go through `box<T>`, which heap-allocates the field (`Box<T>` in the
generated struct). Referencing a type from itself **without** `box`, plain or
as `T?`, is a compile-time error, since the generated struct would otherwise
need infinite size.

- `box<T>` alone doesn't terminate a recursive chain — pair it with `?` (as
  above) so the chain can end.
- Composes with everything else: `box<T>?`, `[box<T>]`, `Mono<box<T>>`, etc.
- On the wire, `box<T>` is transparent — it costs nothing beyond `T` itself;
  it only changes the Rust representation.

---

## Declared types

Declared inside a `version` block, referenced by later fields.

### `enum`

```pojoc
enum Status {
  Alive,
  Dead,
  Dying,
}
```

Wire values are assigned by declaration order, starting at 1 — every enum gets
a compiler-injected `Unknown = 0` variant ahead of your declared ones, which
is the `#[default]` and the value `T::default()` produces. `Unknown` is a
reserved variant name; declaring your own is a compile-time error. Decoding an
unrecognized discriminant is an error.

The wire discriminant is a fixed-width unsigned integer — `u8` for up to 256
total variants (including the injected `Unknown`), `u16` up to 65536, `u32`
beyond that — sized to the smallest width that fits, per resolved version (so
adding enough variants to cross a width threshold only affects versions from
that point forward; older wire data keeps decoding at the width it was
written with). Because an out-of-range discriminant is always a decode error
here (never preserved), this fixed width costs nothing forward-compatibility
would otherwise need. `union` needs the same per-version fixed width for a
harder case — see below, where an out-of-range discriminant must be
*preserved*, not rejected.

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

The discriminant is a fixed-width unsigned integer, same as `enum`'s — but
where an enum collapses to a single wire representation using its *latest*
shape, a union's discriminant width is frozen **per resolved version** and
never recomputed from "how many variants exist today": `Big@1` keeps decoding
and encoding at whatever width it had when it was first defined, forever, even
after a later `Big@2` (via `extends`) adds enough variants to need a wider
one. This is what lets `Unknown` do its job safely — it exists specifically to
carry a discriminant *this* resolved version doesn't recognize, and sizing
that field to "whatever the union has grown to by now" would silently
truncate exactly the values it's meant to preserve. A root field that's never
re-pinned after a `~ field: SomeUnion` diff stays on that pinned version's
own width and variant set permanently, same as it stays on an old shape for
any other type (see the `extends` section below) — a variant added to the
union afterward decodes as `Unknown` through that field, correctly, until the
field is explicitly re-pinned to the newer version.

A union variant can be renamed the same way struct fields are, via `extends`:

```pojoc
union Payload extends Payload@1 {
  ~ Move -> Walk   // rename, discriminant unchanged
}
```

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
missing when decoding older data, and also back the generated `Default` impl
— `T::default()` and a freshly-decoded old-version message use the same
declared value. A field with no declared default zero-inits instead (see the
[Constraints](#constraints-minmax) note above).

Retyping a field in place (`~ field: NewType`, with or without a rename) has
no syntax to supply a new default alongside the new type, since the old
default's shape may no longer fit — the field's default resets to the type's
zero value from that version forward. Give the field a fresh declared default
if it needs one, by adding it as a new field on the new type instead of
retyping in place.

| Type                     | Default syntax                                                           |
| ------------------------ | ------------------------------------------------------------------------ |
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

## Modifiers: `const`, `lazy`, `seekable`, and `intern`

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

### `seekable`

```pojoc
tag:  seekable u32     = 0
name: seekable string  = ""
blob: seekable lazy [u8] = []
```

Generates accessors that pull this one field out of an encoded message without
decoding the rest:

```rust
let tag = seek_tag(&buf)?;                    // one field, dispatched on the envelope version
let (tag, name) = pluck!(schema, &buf, tag, name)?;   // several, in one walk
let all = seek_all(&buf)?;                    // every seekable field
```

`seek_<field>` returns exactly what `decode(&buf)?.<field>` would — including a
version's cast and, for a message predating the field, its declared default. A
per-version `seek_v<N>_<field>` is also generated, taking the payload and a start
offset.

#### Plucking a subset

`seek_all` has to reach the *last* seekable field before it can return, so on a
schema with many of them it costs far more than the few you wanted. Each schema
generates a `pluck_<schema>!` macro that reads exactly the fields you name:

```rust
use my_crate::schemas::edge;
use my_crate::pluck_edge as pluck;   // the macro lands at your crate root

let id = pluck!(edge, &buf, id)?;               // the value itself
let (id, name) = pluck!(edge, &buf, id, name)?; // a tuple, in the order asked for
```

The first argument is the generated module, which must be a single ident in
scope — `macro_rules!` resolves paths at the call site, so it has to be named.
Fields may be listed in any order, and the result is never `Option`: naming a
field guarantees it is bound. A field the payload's version predates yields that
version's default, exactly as `decode` would.

The macro expands to a const-generic walk that skips unrequested fields instead of
reading them and returns as soon as the last requested one is bound. Because the
selection is a constant, every test folds away at monomorphisation — naming only
fixed-width fields never enters the variable region at all. Reading one field
costs the same as its dedicated accessor; reading several costs one walk. The
walk and its bit constants are `#[doc(hidden)]`; the macro is the intended entry
point. The selection is sized per schema, so the number of seekable fields is
unbounded.

On root fields this is opt-in because it changes the field's position on the
wire. A fixed-width seekable field is read at a constant offset. A variable-width
one is hoisted to the front of the variable region so reaching it crosses only
other seekable fields. Marking every field can make the generated code grow
quadratically.

- Combines with `lazy`, in that order (`seekable lazy T`): you get the raw slice
  without decoding it.
- Toggling a root field in a later version relayouts **only that version**. Messages
  already written under earlier versions are unaffected.
- `~ field: T` in a diff **clears** it, like `lazy`. Restate `seekable` to keep
  it. The compiler warns when a `~` drops it from a root field.
- A seekable field that reaches the intern table as anything other than a bare
  `seekable intern string` makes the seek materialize the whole table; the
  compiler warns about it.

#### Nested paths

```pojoc
type Vector {
  x: seekable f32 = 0
  y: f32 = 0
}
type Movement {
  velocity: seekable Vector
}
fields {
  movement: seekable Movement
}
```

```rust
let x = pluck!(schema, &buf, movement.velocity.x)?;
```

Mark every component of the path `seekable`. Intermediate fields must be
required, eager fields of a local named struct type. The selected leaf can
itself be optional or lazy. Markers inside named structs preserve declaration
order on the wire. The reader skips the prefix at each level and returns as
soon as it reads the leaf. Historical layouts that require conversion or a
missing-field default can fall back to the version decoder.

Tuples can mix root and dotted selectors. Calls with dotted selectors read each
path separately and preserve the requested tuple order. Root-only calls use
one const-mask walk.

### `intern`

```pojoc
label:   intern string           = ""
tags:    [intern string]         = []
wrapped: Mono<intern string>     // composes as a generic argument too
```

Unlike `const`/`lazy`/`seekable` (field-level modifiers), `intern` is a **type-level**
wrapper — it can wrap a bare `string` anywhere a type can appear: a plain
field, inside an array/map, or as a generic argument.

Repeated string values across the whole encoded message are deduped into a
single shared lookup table (one table per top-level message, not per-field or
per-struct) and referenced elsewhere by index — essentially free compression
whenever the same strings recur, e.g. tags, labels, or enum-like string values
drawn from a small pool.

- Decodes to the same `&'buf str` as a plain string — no API difference at the
  call site.
- Encoding does a hash-map lookup-or-insert per interned value (a small CPU
  cost); decoding a table index is _cheaper_ than reading a plain string, so
  `intern` is close to a pure win whenever a value repeats.
- Can't combine with `lazy` on the same field — `lazy` skips decoding the
  field's bytes entirely, which conflicts with participating in a table built
  once per message.
- Must wrap a bare `string` — not `string?`, not `string(N)`, not any other
  type.

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

| Op                        | Meaning                                           |
| ------------------------- | ------------------------------------------------- |
| `+ field: Type = default` | add a new field (needs a default unless optional) |
| `- field`                 | remove a field                                    |
| `~ field: NewType`        | retype in place                                   |
| `~ old -> new: Type`      | rename `old` to `new` (with optional retype)      |

Old wire data still decodes: removed fields are read and discarded, added fields
fall back to their default, renamed/retyped fields are mapped through.

> A struct-typed (or generic) field can't be retyped to an _incompatible_ shape
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
  + Heal: HealPayload    // add a variant
  ~ Move -> Walk         // rename a variant (its discriminant is unchanged)
}

bitset Flags extends Flags@1 {
  + IsStreamer          // add a flag
  - IsMuted             // remove a flag
}
```

Note: because a type's evolution produces a _new_ type version, a root field of
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
pojoc show  <input.pojoc>                  # print the latest version, all diffs applied
```

`show` resolves the schema and prints what the newest version actually looks
like — renames, retypes, removals and additions from every `diff` already
folded in, so you can read the current shape without replaying the history by
hand. It prints declared enums, bitsets, unions and types followed by the root
`fields` block; pass `--fields-only` to skip the declared types.

`build` and `check` accept `--verbose`. In a build script, prefer
`pojoc_build::compile_dir("schemas", &out_dir)` (see the README's
[setup section](../README.md#using-it-in-your-project)).
