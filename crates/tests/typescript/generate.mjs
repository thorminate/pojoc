import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const cwd = fileURLToPath(new URL(".", import.meta.url));
for (const name of ["telemetry", "telemetry_delta", "telemetry_varint"]) {
  execFileSync("cargo", [
    "run", "--locked", "--release", "--manifest-path", "../../../Cargo.toml", "-p", "pojoc-cli",
    "--", "build", `${name}.pojoc`, "--target", "typescript", "--standalone", "-o", "generated",
  ], { cwd, stdio: "inherit" });
}
