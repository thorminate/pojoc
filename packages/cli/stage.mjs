import { chmod, copyFile, mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";

// Match the Rust targets and filenames in the release build matrix.
const targets = {
  "linux-x64": "x86_64-unknown-linux-gnu",
  "win32-x64": "x86_64-pc-windows-msvc",
  "darwin-x64": "x86_64-apple-darwin",
  "darwin-arm64": "aarch64-apple-darwin",
};

const [tag, artifacts, output] = process.argv.slice(2);
if (!/^v\d+\.\d+\.\d+(?:-[\da-zA-Z-]+(?:\.[\da-zA-Z-]+)*)?$/.test(tag) || !artifacts || !output) {
  console.error("Usage: node packages/cli/stage.mjs <release-tag> <artifacts-dir> <output-dir>");
  process.exit(1);
}

const version = tag.slice(1);
const shared = {
  version,
  license: "MIT",
  repository: {
    type: "git",
    url: "git+https://github.com/thorminate/pojoc.git",
    directory: "packages/cli",
  },
  publishConfig: {
    access: "public",
    registry: "https://registry.npmjs.org/",
  },
};
const optionalDependencies = {};

async function writeManifest(directory, manifest) {
  await writeFile(join(directory, "package.json"), JSON.stringify(manifest, null, 2) + "\n");
  await copyFile(new URL("../../LICENSE", import.meta.url), join(directory, "LICENSE"));
}

for (const [platform, target] of Object.entries(targets)) {
  const [os, cpu] = platform.split("-");
  const extension = os === "win32" ? ".exe" : "";
  const name = `@pojoc/cli-${platform}`;
  const directory = resolve(output, `cli-${platform}`);
  const bin = join(directory, "bin");
  await mkdir(bin, { recursive: true });
  const executable = join(bin, `pojoc${extension}`);
  await copyFile(join(artifacts, `pojoc-${tag}-${target}${extension}`), executable);
  await chmod(executable, 0o755);
  await writeManifest(directory, {
    name,
    ...shared,
    description: `Pojoc schema compiler binary for ${platform}`,
    os: [os],
    cpu: [cpu],
    ...(os === "linux" ? { libc: ["glibc"] } : {}),
    files: ["bin"],
  });
  optionalDependencies[name] = version;
}

const directory = resolve(output, "cli");
await mkdir(directory, { recursive: true });
await copyFile(new URL("./pojoc.cjs", import.meta.url), join(directory, "pojoc.cjs"));
await copyFile(new URL("./README.md", import.meta.url), join(directory, "README.md"));
await writeManifest(directory, {
  name: "@pojoc/cli",
  ...shared,
  description: "Native Pojoc schema compiler for Rust and TypeScript",
  bin: { pojoc: "./pojoc.cjs" },
  files: ["pojoc.cjs"],
  optionalDependencies,
});
