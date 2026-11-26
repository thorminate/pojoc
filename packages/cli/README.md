# Pojoc CLI

Install the schema compiler as a project dev dependency.

```sh
npm install --save-dev @pojoc/cli
npm exec -- pojoc --version
npm exec -- pojoc build schemas/player.pojoc --target typescript -o src/generated
```

Use the same `pojoc` command in npm scripts. For generated TypeScript that
imports the runtime, install the separate `pojoc` package at the same release.
Run `npm update @pojoc/cli` to update the compiler within the version range
in `package.json`. Commit the lockfile to share the version with CI.

The compiler is available for Windows x64, Linux x64 with glibc, and macOS x64
and arm64. npm installs the native binary as an optional dependency for your
platform. See the [TypeScript compiler guide](https://github.com/thorminate/pojoc/blob/main/packages/runtime/README.md)
for usage and generation flags.
