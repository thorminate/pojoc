# Pojoc Schema Reference

A `.pojoc` file describes a versioned binary format. The compiler generates
Rust or TypeScript codecs for the same wire format. Both decode historical
messages into the latest version's shape and can encode for a selected version.

This is a reference for the schema language itself. For benchmarks and setup see
[`README.md`](../README.md).

- [File structure](#file-structure)
- [Field types](#field-types)
- [Rust construction macros](#rust-construction-macros)
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

Rust uses the corresponding numeric types, `bool`, and borrowed `&str`.
TypeScript uses `bigint` for `i64` and `u64`, `number` for the remaining
numeric primitives, `boolean` for `bool`, and `string` for `string`.

### Varints

```pojoc
count: varint32 = 0     // 1–5 bytes, value range of u32
big:   varint64 = 0     // 1–10 bytes, value range of u64
```

LEB128-style variable-length integers. Smaller values take fewer bytes.
Rust represents them as `u32` and `u64`. TypeScript represents them as `number`
and `bigint`, respectively.

### Fixed-length string

```pojoc
code: string(8) = "00000000"   // exactly 8 bytes on the wire, no length prefix
```

Decodes to `[u8; N]` in Rust and `Uint8Array` in TypeScript. The default
literal's UTF-8 byte length must equal `N`.

### Ranged / quantized floats (`vfloat`)

```pojoc
angle: vfloat(min: 0, max: 360, step: 0.1) = 0.0
```

Packs a float in `[min, max]` at resolution `step` into a `u16` or `u32` on
the wire. Decodes to `f32` in Rust and `number` in TypeScript. Useful for
angles, normalized values, and positions where rounding to `step` is acceptable.

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

In TypeScript, arrays of fixed-width integers and floats use the matching
typed array, such as `Uint32Array` for `[u32](delta)` and `BigInt64Array` for
`[i64]`. Other element types use `Array<T>`, such as `Array<bigint>` for
`[varint64]`. A fixed-length array uses the same TypeScript type and the
encoder checks its length.

### Tuples

```pojoc
coordinates: (f32, f32)           = (0.0, 0.0)
loadout:     [(string, i32)](4)   = [..("", 0)]
```

Heterogeneous fixed-arity groups. A tuple of only fixed-width elements is itself
fixed-width on the wire. In TypeScript, `coordinates` has type
`[number, number]` and `loadout` has type `Array<[string, number]>`.

### Maps

```pojoc
config:  map<string, string>       = {}
scores:  map<string, i64>(4)       = {"ok": -1, "bad": 0, "x": 1, "y": 2}
```

- `map<K, V>` — variable-size, length-prefixed.
- `map<K, V>(N)` — fixed `N` entries.

Rust uses `PojocMap<K, V>` and `PojocFixedMap<K, V, N>`, respectively.
TypeScript uses `Map<K, V>` for the variable-size form and `Array<[K, V]>`
for the fixed-size form.

`PojocMap` and the Rust string interner use aHash with runtime-generated
seeds by default. Enable the `secure-hasher` feature on your `pojoc`
dependency to use Rust's standard `RandomState` (currently SipHash-1-3)
for both.

Values (and keys) may themselves be any type, including nested maps/arrays and
unions: `map<string, [map<i32, bool>]>`.

### Optional

```pojoc
nickname: string?
level:    i32?
```

`T?` can hold a value or be absent. Optional record fields share a presence
bitmap. Optional values inside collections or tuples carry their own presence
byte.

Rust uses `Option<T>`, with `None` for absence. TypeScript uses `T | null`,
with `null` for absence. The generated properties are required, so a decoded
TypeScript object always has `nickname: string | null` and
`level: number | null`. You can omit either property when calling `create()`
to fill in its schema default.

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

`min` and `max` bound numeric values, UTF-8 string byte lengths, and array or
map element counts. On encode and decode, Rust returns
`Error::ConstraintViolation` and TypeScript throws `RangeError` when a value
falls outside its bounds.

Constraints can be nested. `[string(min: 1, max: 20)]` bounds the byte length
of each string in the array.

The field's default must satisfy its constraints to be encoded. See
[Defaults](#defaults) for how each target fills in fields.

### `box<T>` — recursive types

```pojoc
type Node {
  value: i32 = 0
  next: box<Node>?
}
```

A cycle of direct struct fields must go through `box<T>` so the Rust struct
has a finite size. Rust represents `box<T>` as `Box<T>`. TypeScript uses `T`
as the field type, so `next` in this example is `Node | null`.

- `box<T>` alone doesn't terminate a recursive chain — pair it with `?` (as
  above) so the chain can end.
- Composes with everything else: `box<T>?`, `[box<T>]`, `Mono<box<T>>`, etc.
- `box<T>` has the same wire layout as `T`.

---

## Rust construction macros

The `pojoc` runtime provides macros for constructing values to pass to
generated Rust codecs.

| Macro                                            | Result                                                                   |
|--------------------------------------------------|--------------------------------------------------------------------------|
| `pojstr!("ABC123", 6)`                           | A `[u8; 6]` for `string(6)`, with a compile-time UTF-8 byte-length check |
| `pojvec!["ready", "active"]`                     | A `PojocVec` for a variable-length array                                 |
| `pojvec![20, 21, 22, 23; 4]`                     | A four-element Rust array for a fixed-length field                       |
| `pojmap!("wins" => 3_u32)`                       | A `PojocMap` for a variable-length map                                   |
| `pojmap!("wins" => 3_u32, "losses" => 1_u32; 2)` | A two-entry `PojocFixedMap` with a compile-time entry-count check        |
| `pojtup!(1_i16, 2_i16)`                          | A tuple whose elements convert to the expected types via `Into`          |

```rust
use pojoc::{PojocFixedMap, PojocMap, PojocVec, pojmap, pojstr, pojtup, pojvec};

let code = pojstr!("ABC123", 6);
let tags: PojocVec<&str> = pojvec!["ready", "active"];
let samples: [i16; 4] = pojvec![20, 21, 22, 23; 4];
let scores: PojocMap<&str, u32> = pojmap!("wins" => 3_u32);
let fixed: PojocFixedMap<&str, u32, 2> = pojmap!("wins" => 3_u32, "losses" => 1_u32; 2);
let coordinates: (i32, i32) = pojtup!(1_i16, 2_i16);
```

To read selected fields from encoded bytes, use the [generated pluck macro](#plucking-a-subset-in-rust).

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

Wire values are assigned by declaration order, starting at 1. Every enum gets
a compiler-injected `Unknown = 0` variant, which is also the type's default
value. `Unknown` is reserved. Decoding an unrecognized discriminant is an error.

Rust generates an enum. TypeScript generates a union of numeric literals and
a named object with the same values, so `Status.Alive` has the value `1`.

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

Struct fields follow the same type and default rules as root fields. Rust
generates a struct and TypeScript exports a plain object type.

### `union` (tagged)

```pojoc
union Payload {
  Move:   MovePayload,
  Attack: AttackPayload,
}
```

A wire discriminant and a length-prefixed payload. Both targets preserve an
unrecognized discriminant and its payload bytes so proxies can forward them.
Rust uses enum variants, including `Unknown { discriminant, data }`. TypeScript
uses a discriminated union with cases such as `{ kind: "Move", value: payload }`
and `{ kind: "Unknown", discriminant, data }`, where `data` is a `Uint8Array`.

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

A packed set of boolean flags (1/2/4 bytes by flag count). Rust generates
getters, setters, `with_*` builders, and `|`, `&`, `!` operators. TypeScript
uses a `Uint8Array` and exports `SystemPrivilegesBits` with the bit index of
each flag. See [Defaults](#defaults) for bitset default syntax.

---

## Generics

Templates are monomorphized per distinct instantiation. Each `(template, args)`
becomes its own Rust struct or TypeScript object type.

```pojoc
type Box<T>          { value: T }
type Pair<A, B>      { first: A  second: B }

fields {
  boxed:  Box<i32>              // -> generated type BoxI32
  pair:   Pair<i32, string>     // -> generated type PairI32String
  flag:   Box<bool> as FlagBox  // name the monomorphized type explicitly
}
```

- `as Alias` sets the generated type's name.
- Generics compose with everything: `[Box<i32>]`, `map<string, Box<bool>>`, etc.

Generics also evolve (see [Schema evolution](#schema-evolution)), including
crossing template names and dropping a type parameter with `_`.

---

## Defaults

A field may declare a default with `= value`. Both codecs fill in missing
fields when decoding older data. Rust also uses these defaults in `T::default()`.
TypeScript's `create()` fills in omitted root fields, so `create({ name: "Ada" })`
sets `name` and fills in the remaining fields. A field without a declared
default uses its type's default value. This is `None` in Rust and `null` in
TypeScript for optional fields.

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

A primitive compile-time constant that takes up no bytes on the wire. Rust
generates an associated `const`. TypeScript exports root constants in
`constants` and named type constants in `<Type>Constants`.

### `lazy`

```pojoc
audit_log: lazy [string]?
big_blob:  lazy [u8] = []
```

Rust `decode()` retains the field's bytes for on-demand decoding through
`LazyView::get()`. TypeScript `decode()` materializes the field as its normal
value. For the latest version, TypeScript `open(bytes)` reads top-level
fields on demand, including fields declared `lazy`.

> A `lazy` field **added in a `diff`** must be optional (`?`), so that older
> messages lacking it decode as absent.

### `seekable`

```pojoc
tag:  seekable u32     = 0
name: seekable string  = ""
blob: seekable lazy [u8] = []
```

Both targets generate accessors that can read individual fields from encoded
bytes. Rust provides a dedicated accessor per field and macros for reading
subsets.

```rust
let tag = seek_tag(&buf)?;                    // one field, dispatched on the envelope version
let (tag, name) = pluck!(schema, &buf, tag, name)?;   // several, in one walk
let all = seek_all(&buf)?;                    // every seekable field
```

`seek_<field>` returns exactly what `decode(&buf)?.<field>` would — including a
version's cast and, for a message predating the field, its declared default. A
per-version `seek_v<N>_<field>` is also generated, taking the payload and a start
offset.

In TypeScript, use `seek(bytes, "tag")`. It returns the same field value as
`decode(bytes).tag`. For arbitrary top-level fields, use `open(bytes).field`
or `select(["tag", "name"])(bytes)`. `open` and `select` decode the full
message for historical versions.

#### Plucking a subset in Rust

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

- Combines with `lazy`, in that order (`seekable lazy T`). Rust returns a
  `LazyView`. TypeScript decodes the selected field.
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

```ts
const x = seek(bytes, "movement.velocity.x");
```

TypeScript infers the return type for a top-level field name. Dotted paths
use the `unknown` return type.

Mark every component of the path `seekable`. Intermediate fields must be
required, eager fields of a local named struct type. The selected leaf can
itself be optional or lazy. Markers inside named structs preserve declaration
order on the wire. The reader skips the prefix at each level and returns as
soon as it reads the leaf. Historical layouts that require conversion or a
missing-field default can fall back to the version decoder.

Rust `pluck!` can mix root and dotted selectors. Calls with dotted selectors
read each path separately and preserve the requested tuple order. Root-only
calls use one const-mask walk.

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

- Decodes to `&str` in Rust and `string` in TypeScript, just like `string`.
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

Both targets decode into the latest shape. Rust's `encode_for_version` and
TypeScript's `encodeForVersion` write an envelope for a selected version.
The compiler tracks each field across renames and retypes. Use `diff` to evolve
the root message and `extends` to evolve a named type.

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

Rust generates nested modules for imports. TypeScript emits the imported
types and codec functions into the same generated module. The `@N` pins the
version of the imported message used on the wire. Re-pin it in a later `diff`
to advance it (`~ owner -> new_owner: Player@6`).

---

## Comments

```pojoc
// a line comment

/// a doc comment — carried through onto the generated Rust item
type Stats { ... }
```

`///` doc comments attach to the following declaration (schema, type, field,
variant) and appear as `///` docs in the generated Rust code.

---

## CLI

```sh
pojoc build <input.pojoc> --out-dir out/   # generate <stem>.rs (default out dir: out/)
pojoc build <input.pojoc> --target typescript --out-dir out/   # generate <stem>.ts
pojoc check <input.pojoc>                  # validate without writing output
pojoc show  <input.pojoc>                  # print the latest version, all diffs applied
```

`show` resolves the schema and prints what the newest version actually looks
like — renames, retypes, removals and additions from every `diff` already
folded in, so you can read the current shape without replaying the history by
hand. It prints declared enums, bitsets, unions and types followed by the root
`fields` block; pass `--fields-only` to skip the declared types.

`build` and `check` accept `--verbose`. A Cargo build script can use
`pojoc_build::compile_dir` for Rust or `pojoc_build::compile_typescript` for
TypeScript.

The TypeScript target imports the `pojoc` npm runtime by default. Use
`--standalone` to embed the runtime, `--runtime portable|bun|node` to select
a runtime profile, or `--compact` to omit dedicated seek code for historical
layouts. Historical seeks fall back to decoding when a dedicated path is
unavailable. See the [TypeScript runtime guide](../packages/runtime/README.md)
for setup and examples.
