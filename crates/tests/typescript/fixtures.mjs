import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

const root = fileURLToPath(new URL("../../../", import.meta.url));
const out = fileURLToPath(new URL("./generated/", import.meta.url));
const build = (args) => execFileSync("cargo", args, { cwd: root, stdio: "inherit" });
build(["build", "--locked", "-p", "pojoc-cli"]);
const metadata = JSON.parse(execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], {
  cwd: root,
  encoding: "utf8",
}));
const compiler = join(metadata.target_directory, "debug", process.platform === "win32" ? "pojoc.exe" : "pojoc");
for (const name of ["player", "edge", "interning", "import_root", "nested_pluck", "recursive", "seek_retype", "constraints"]) {
  execFileSync(compiler, ["build", join(root, "crates", "tests", "schemas", `${name}.pojoc`), "--target", "typescript", "-o", out], {
    cwd: root, stdio: "inherit",
  });
}
build(["run", "--locked", "-p", "pojoc-tests", "--example", "typescript_fixtures", "--", out]);
