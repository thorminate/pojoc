# Pojoc TypeScript runtime

Install the package at the same release version as the Pojoc compiler, then
generate a codec that imports it.

```sh
npm install pojoc
pojoc build player.pojoc --target typescript-npm -o generated
```

Use the exports of the generated module for schema-specific encoding,
decoding, and seeking. The runtime has zero runtime dependencies and ships
as JavaScript ES modules with TypeScript declarations. The `pojoc/v1`
entry point identifies the schema descriptor format used by the generator.

## Publishing

The tag release workflow takes the package version from the Git tag. It
publishes stable versions to `latest` and prereleases to `next`.

For the first release, set a GitHub Actions secret named `NPM_TOKEN` with
permission to publish `pojoc`. After the package exists, configure
[npm trusted publishing](https://docs.npmjs.com/trusted-publishers/) for
`thorminate/pojoc` and `release.yml` with direct publishing enabled. The
workflow can then use OIDC and the bootstrap token can be removed.
