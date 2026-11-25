import { copyFile, cp, mkdir } from "node:fs/promises";

const source = new URL("../../crates/build/src/codegen/typescript/runtime/", import.meta.url);
const staging = new URL("./.build/", import.meta.url);
await mkdir(staging, { recursive: true });
await cp(source, staging, { recursive: true });
await copyFile(new URL("../../LICENSE", import.meta.url), new URL("./LICENSE", import.meta.url));
