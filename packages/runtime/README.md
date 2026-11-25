# Pojoc TypeScript runtime

Install the package at the same release version as the Pojoc compiler, then
generate a codec that imports it.

```sh
npm install pojoc
pojoc build player.pojoc --target typescript -o generated
```

Use the generated module to create, encode, and decode values.

```ts
import { create, decode, encode } from "./generated/player.ts";

const player = create();
const bytes = encode(player); // Uint8Array
const roundTrip = decode(bytes); // Player
```

`create()` fills in omitted fields from the schema defaults. Optional fields
have type `T | null`. Pass `null` to encode an absent value.

The runtime ships as JavaScript ES modules with TypeScript declarations and
zero runtime dependencies. Generated modules import shared primitives from
`pojoc/v1/compiler`. The [schema reference](../../docs/schema.md) covers
type mappings, defaults, and field access.

## Publishing

The tag release workflow takes the package version from the Git tag. It
publishes stable versions to `latest` and prereleases to `next`.

For the first release, set a GitHub Actions secret named `NPM_TOKEN` with
permission to publish `pojoc`. After the package exists, configure
[npm trusted publishing](https://docs.npmjs.com/trusted-publishers/) for
`thorminate/pojoc` and `release.yml` with direct publishing enabled. The
workflow can then use OIDC and the bootstrap token can be removed.
