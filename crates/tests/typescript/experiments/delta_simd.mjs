import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, unlinkSync, rmdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { decodeDelta } from "../../../../packages/runtime/dist/scalars.js";
import {
  readVarint,
  readVarint32,
} from "../../../../packages/runtime/dist/wire.js";
import { engine, time } from "../benchmark_timer.ts";

const temp = mkdtempSync(join(tmpdir(), "pojoc-delta-simd-"));
const binary = join(temp, "delta_simd.wasm");
let wasm;
try {
  execFileSync(
    "clang",
    [
      "--target=wasm32",
      "-O3",
      "-msimd128",
      "-fno-vectorize",
      "-fno-slp-vectorize",
      "-ffreestanding",
      "-nostdlib",
      "-Wl,--no-entry",
      "-Wl,--export=__heap_base",
      "-Wl,--strip-all",
      fileURLToPath(new URL("./delta_simd.c", import.meta.url)),
      "-o",
      binary,
    ],
    { stdio: "inherit" },
  );
  wasm = (await WebAssembly.instantiate(readFileSync(binary))).instance.exports;
} finally {
  try {
    unlinkSync(binary);
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  rmdirSync(temp);
}

function signedVarintReference(r) {
  const bits = readVarint(r);
  return (bits >> 1n) ^ -(bits & 1n);
}

function signedDeltaReference(r) {
  if (r.pos >= r.end) throw new RangeError("Unexpected end");
  const byte = r.data[r.pos];
  if (byte < 128) {
    r.pos++;
    return (byte >>> 1) ^ -(byte & 1);
  }
  return Number(BigInt.asIntN(32, signedVarintReference(r)));
}

// The original TypeScript delta loop, specialized to u32 for comparison.
function readBigIntDelta(r, count) {
  const out = new Uint32Array(count);
  let previous = readVarint32(r);
  out[0] = previous;
  for (let i = 1; i < count; i++) {
    previous = (previous + signedDeltaReference(r)) | 0;
    out[i] = previous;
  }
  return out;
}

function cursor(bytes) {
  return { data: bytes, view: undefined, pos: 0, end: bytes.length };
}

function fixture(count, widePercentage) {
  const bytes = [0xfe, 0xff, 0xff, 0xff, 0x0f];
  let random = 123;
  for (let i = 1; i < count; i++) {
    random = (Math.imul(random, 1664525) + 1013904223) >>> 0;
    if (random % 100 < widePercentage) {
      bytes.push(0x80 | ((random >>> 24) & 127), 1 + ((random >>> 16) & 31));
    } else {
      bytes.push(random >>> 25);
    }
  }
  return Uint8Array.from(bytes);
}

function arena(bytes, count) {
  const inputOffset = wasm.__heap_base.value;
  const outputOffset = Math.ceil((inputOffset + bytes.length) / 16) * 16;
  const end = outputOffset + count * 4;
  if (end > wasm.memory.buffer.byteLength) {
    wasm.memory.grow(Math.ceil((end - wasm.memory.buffer.byteLength) / 65536));
  }
  const input = new Uint8Array(wasm.memory.buffer, inputOffset, bytes.length);
  const output = new Uint32Array(wasm.memory.buffer, outputOffset, count);
  input.set(bytes);
  return {
    input,
    output,
    decode: (kernel) => kernel(inputOffset, bytes.length, outputOffset, count),
  };
}

const { decode_scalar: scalar, decode_simd: simd } = wasm;
for (const count of [0, 1, 15, 16, 17, 31, 32, 33]) {
  for (const bytes of [
    new Uint8Array(),
    fixture(count, 0),
    fixture(count, 10),
  ]) {
    const block = arena(bytes, count);
    let expected;
    const read = cursor(bytes);
    try {
      expected = decodeDelta(read, "u32", count);
    } catch {
      expected = undefined;
    }
    for (const kernel of [scalar, simd]) {
      assert.equal(block.decode(kernel), expected ? read.pos : -1);
      if (expected) assert.deepEqual(block.output, expected);
    }
  }
}
for (const [delta, valid] of [
  [[0xfe, 0xff, 0xff, 0xff, 0x1f], true],
  [[...Array(9).fill(0xff), 1], true],
  [[...Array(9).fill(0xff), 2], false],
  [[0x80], false],
]) {
  const bytes = Uint8Array.of(0, ...delta);
  const block = arena(bytes, 2);
  for (const kernel of [scalar, simd]) {
    assert.equal(block.decode(kernel), valid ? bytes.length : -1);
    if (valid)
      assert.deepEqual(block.output, decodeDelta(cursor(bytes), "u32", 2));
  }
}

let sink = 0;
function consume(values, index) {
  sink = (sink + values[values.length - 1 - index]) | 0;
}

console.log(engine + " (ns/value)");
console.log(
  [
    "count",
    "wide %",
    "JS BigInt",
    "JS number",
    "Wasm scalar",
    "Wasm SIMD",
    "Wasm scalar + copies",
    "Wasm SIMD + copies",
  ].join("\t"),
);
for (const count of [64, 4096, 65536]) {
  for (const wide of [0, 10, 100]) {
    const bytes = fixture(count, wide);
    const block = arena(bytes, count);
    const expected = decodeDelta(cursor(bytes), "u32", count);
    assert.deepEqual(readBigIntDelta(cursor(bytes), count), expected);
    for (const kernel of [scalar, simd]) {
      assert.equal(block.decode(kernel), bytes.length);
      assert.deepEqual(block.output, expected);
    }
    const cases = [
      (i) => consume(readBigIntDelta(cursor(bytes), count), i),
      (i) => consume(decodeDelta(cursor(bytes), "u32", count), i),
      (i) => {
        block.decode(scalar);
        consume(block.output, i);
      },
      (i) => {
        block.decode(simd);
        consume(block.output, i);
      },
      ...[scalar, simd].map((kernel) => (i) => {
        block.input.set(bytes);
        block.decode(kernel);
        consume(block.output.slice(), i);
      }),
    ];
    const results = cases.map((work) => (time(work) / count).toFixed(2));
    console.log([count, wide, ...results].join("\t"));
  }
}
console.log("checksum", sink);
