import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { check } from "prettier";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

const root = fileURLToPath(new URL("../../../", import.meta.url));
const useNpmRuntime = process.argv.includes("--npm-runtime");
const out = fileURLToPath(
  new URL(useNpmRuntime ? "./generated/npm/" : "./generated/", import.meta.url),
);
const build = (args) =>
  execFileSync("cargo", args, { cwd: root, stdio: "inherit" });
build(["build", "--locked", "-p", "pojoc-cli"]);
const metadata = JSON.parse(
  execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], {
    cwd: root,
    encoding: "utf8",
  }),
);
const compiler = join(
  metadata.target_directory,
  "debug",
  process.platform === "win32" ? "pojoc.exe" : "pojoc",
);
async function generate(schema, directory, flags = []) {
  const input = join(root, "crates", "tests", "schemas", `${schema}.pojoc`);
  const args = [
    "build",
    input,
    "--target",
    "typescript",
    "-o",
    directory,
    ...flags,
  ];
  if (!useNpmRuntime) args.push("--standalone");
  execFileSync(compiler, args, { cwd: root, stdio: "inherit" });

  const filepath = join(directory, `${schema}.ts`);
  const source = readFileSync(filepath, "utf8");
  if (
    /\b(?:SchemaSpec|makeCodec|WeakMap|writeScalar)\b|\bclass Writer\b/.test(
      source,
    )
  ) {
    throw new Error(
      "Generated fixture uses schema state or a legacy codec: " + filepath,
    );
  }
  if (!(await check(source, { filepath }))) {
    throw new Error(`Generated fixture is not Prettier-formatted: ${filepath}`);
  }
}

for (const schema of [
  "player",
  "edge",
  "interning",
  "import_root",
  "nested_pluck",
  "recursive",
  "seek_retype",
  "constraints",
  "typescript_batch",
  "typescript_fixed_records",
  "fixed_envelope",
  "typescript_formatting",
  "typescript_query",
]) {
  await generate(schema, out);
}
for (const [profile, flags] of [
  ["bun", ["--bun"]],
  ["node", ["--runtime", "node"]],
]) {
  for (const schema of [
    "edge",
    "seek_retype",
    "typescript_batch",
    "typescript_fixed_records",
    "fixed_envelope",
    "typescript_query",
  ]) {
    await generate(schema, join(out, profile), flags);
  }
}
await generate("nested_pluck", join(out, "compact"), ["--compact"]);
build([
  "run",
  "--locked",
  "-p",
  "pojoc-tests",
  "--example",
  "typescript_fixtures",
  "--",
  out,
]);
