import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";

const source = new URL("../../crates/build/src/codegen/typescript_runtime.ts", import.meta.url);
const staging = new URL("./.build/", import.meta.url);
await mkdir(staging, { recursive: true });
const runtime = await readFile(source, "utf8");
await writeFile(new URL("index.ts", staging), `${runtime}\nexport { makeCodec };\nexport type { SchemaSpec };\n`);
await copyFile(new URL("../../LICENSE", import.meta.url), new URL("./LICENSE", import.meta.url));
