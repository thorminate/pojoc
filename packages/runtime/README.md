# Pojoc for TypeScript

The Pojoc CLI compiles a `.pojoc` schema into a TypeScript module with types
and binary codecs. The `pojoc` npm package supplies shared wire primitives.
Use the same release of the CLI and runtime.

## Generate and use a codec

Save this schema as `schemas/player.pojoc`.

```pojoc
schema Player {
  version 1 {
    fields {
      id: u32 = 0
      name: string = ""
    }
  }

  version 2 {
    diff { + level: u32 = 1 }
  }
}
```

From the directory containing `package.json`, install the runtime and
generate `src/generated/player.ts`.

```sh
npm install pojoc
npm install --save-dev @pojoc/cli
npm exec -- pojoc build schemas/player.pojoc --target typescript -o src/generated
```

From `src/app.ts`, import the codec and its `Player` type.

```ts
import { create, decode, encode, encodeForVersion } from "./generated/player.ts";
import type { Player } from "./generated/player.ts";

const player: Player = create({ id: 42, name: "Mira" });
const bytes = encode(player); // Uint8Array containing a version 2 envelope
const roundTrip = decode(bytes); // { id: 42, name: "Mira", level: 1 }
const legacy = encodeForVersion(player, 1);
const upgraded = decode(legacy); // level is filled in from the version 2 default
```

`create()` fills in omitted fields from the schema defaults. Optional fields
have type `T | null`. Pass `null` to encode an absent value.

Include the generated file in your regular TypeScript build. Regenerate it
when the schema or any of its imports change. For a frontend-only project,
run the generation command from an npm script before type checking or bundling.

## What the compiler emits

1. Pojoc resolves schema imports, applies each version's `diff`, and computes
   the wire layouts shared with the Rust target.
2. It emits a single `.ts` module per entry schema. The module includes
   public TypeScript types, schema-specific readers and writers, version
   dispatch, and field access functions. Imported schema layouts are included
   in that module.
3. `encode` writes the latest version. `decode` reads the version in the
   envelope, runs that version's reader, and returns the current type. The
   compiler emits those functions ahead of time. At runtime they call shared
   operations such as varint and string decoding from `pojoc/v1/compiler`.

The [schema reference](../../docs/schema.md) covers TypeScript type mappings,
defaults, and field access.

## Generation options

| Option | Effect |
| --- | --- |
| `--standalone` | Embed the shared runtime in the generated module so it can run without the npm package. |
| `--runtime portable` | Use the portable profile, which is the default for browser and server builds. |
| `--runtime bun` or `--runtime node` | Enable the specialized writer for supported variable-length layouts. Both profiles currently use the same writer specialization. `--bun` is shorthand for `--runtime bun`. |
| `--compact` | Reduce output size by omitting dedicated seeks for historical layouts. Those seeks fall back to decoding. |

## Generate from Cargo

For a Rust backend and TypeScript frontend in the same project, add
`pojoc-build` as a build dependency.

```sh
cargo add --build pojoc-build --no-default-features
```

In `build.rs`, write the TypeScript module to a directory the frontend can
import.

```rust
use pojoc_build::{TypeScriptOptions, compile_typescript};

fn main() {
    compile_typescript(
        "schemas/player.pojoc",
        "web/src/generated",
        TypeScriptOptions::default(),
    )
    .unwrap_or_else(|error| panic!("{}", error.render()));
}
```

The paths are relative to the Rust package. Cargo tracks the schema and its
transitive imports, and generation leaves an identical output file untouched.
The `standalone`, `runtime`, and `compact` fields on `TypeScriptOptions`
correspond to the CLI options above.
Run `cargo build` before building the frontend so the file is available.
The Rust side can use `pojoc_build::schema!` with the same schema file.

## Publishing

The tag release workflow takes the package version from the Git tag. It
publishes stable versions to `latest` and prereleases to `next`.

For the first release, set a GitHub Actions secret named `NPM_TOKEN` with
permission to publish `pojoc`. After the package exists, configure
[npm trusted publishing](https://docs.npmjs.com/trusted-publishers/) for
`thorminate/pojoc` and `release.yml` with direct publishing enabled. The
workflow can then use OIDC and the bootstrap token can be removed.
