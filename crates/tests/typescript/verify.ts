import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { readdirSync, readFileSync } from "node:fs";
import process from "node:process";
import { edgeWithCollections, protoEdge, protoPlayer, protobufFixture } from "./edge_protobuf.ts";
import { payloadOffset, withPayload } from "./frame_fixtures.ts";

const useNpmRuntime = process.argv.includes("--npm-runtime");
const dir = new URL(useNpmRuntime ? "./generated/npm/" : "./generated/", import.meta.url);
const codecs = new Map();
const artifacts = readdirSync(dir).filter(name => /^.+-\d+\.bin$/.test(name));
assert(artifacts.length > 0, "Generate Rust fixtures before running the test");

for (const filename of artifacts) {
  const [, fixture, version] = /^(.+)-(\d+)\.bin$/.exec(filename)!;
  const schema = fixture === "edge-populated" ? "edge" : fixture;
  if (!codecs.has(schema)) codecs.set(schema, await import(new URL(`${schema}.ts`, dir).href));
  const codec = codecs.get(schema);
  const bytes = readFileSync(new URL(filename, dir));
  const decoded = codec.decodeFrom(bytes);
  assert.equal(decoded.version, BigInt(version), filename);
  assert.equal(decoded.offset, bytes.length, filename);
  const encoded = codec.encodeForVersion(decoded.value, BigInt(version));
  const mismatch = bytes.findIndex((byte, i) => byte !== encoded[i]);
  assert(
    bytes.length === encoded.length && mismatch === -1,
    `${filename}: Rust ${bytes.length} bytes, TypeScript ${encoded.length} bytes, first difference ${mismatch}`,
  );
  const keys = Object.keys(decoded.value);
  assert.deepEqual(codec.select(keys)(bytes), decoded.value, `projection of ${filename}`);
  const last = keys.at(-1);
  if (last !== undefined) {
    assert.deepEqual(codec.select([last])(bytes), { [last]: decoded.value[last] }, filename);
  }
  const view = codec.open(bytes);
  for (const key of keys.toReversed()) {
    assert.deepEqual(view[key], decoded.value[key], `view of ${filename}.${key}`);
    assert.strictEqual(view[key], view[key], `cached ${filename}.${key}`);
  }
  assert.throws(() => codec.decode(bytes.subarray(0, -1)), RangeError, filename);
  assert.throws(() => codec.open(bytes.subarray(0, -1)), RangeError, filename);
  if (schema === "player") {
    assert.equal(decoded.value.player_id, 17);
    protobufFixture(protoPlayer, decoded.value);
  }
  if (schema === "edge") protobufFixture(protoEdge, decoded.value);
  if (schema === "nested_pluck") assert.equal(codec.seek(bytes, "transform.position.x"), 99);
  if (fixture === "edge-populated") {
    assert.deepEqual(Array.from(decoded.value.legacy_positions), [-0x8000_0000, 0x7fff_ffff, -0x8000_0000, 0, 127, 128]);
    assert.deepEqual(Array.from(decoded.value.u32_delta_seq.slice(0, 4)), [0xffff_ffff, 0, 0xffff_ffff, 0]);
    if (decoded.version >= 7n) {
      assert.equal(codec.seek(bytes, "transform.position.x"), 0x1234_5678);
      assert.equal(codec.seek(bytes, "transform.position.label"), "shared-tag");
      assert.equal(codec.seek(bytes, "transform.position.prefix"), "point");
      assert.deepEqual(decoded.value.tree, {
        value: 1,
        left: { value: 2, left: null, right: null },
        right: { value: 3, left: null, right: null },
      });
      assert.deepEqual(decoded.value.ping, { count: 10, other: { count: 20, other: { count: 30, other: null } } });
      assert.equal(decoded.value.interned_generic.value, decoded.value.interned_label);
      assert.deepEqual(decoded.value.packed_varints, [0, 127, 128, 0x4000, 0x20_0000, 0x1000_0000, 0xffff_ffff]);
      assert.deepEqual(decoded.value.union_width_probe.x, decoded.version === 7n
        ? { kind: "V1", value: 42 }
        : { kind: "W259", value: 99 });
      if (decoded.version >= 8n) {
        assert.throws(() => codec.encodeForVersion(decoded.value, 7n), /union discriminant overflow/);
      }
    }
  }
}

const fixed = codecs.get("typescript_fixed_records");
const fixedFrame = readFileSync(new URL("typescript_fixed_records-2.bin", dir));
const fixedValues = {
  rows: [
    { id: 0xffff_ffff, value: -32768, ratio: -1.5, enabled: true },
    { id: 0, value: 32767, ratio: 2.25, enabled: false },
  ],
};
assert.deepEqual(fixed.decode(fixedFrame), fixedValues);
assert.deepEqual(fixed.decode(fixed.encode({ rows: [] })), { rows: [] });
for (const overrides of [{ id: -1 }, { id: 0x1_0000_0000 }, { value: 0.5 }, { value: 32768 }]) {
  assert.throws(() => fixed.encode({ rows: [{ ...fixedValues.rows[0], ...overrides }] }), RangeError);
}
assert.throws(() => fixed.encode({ rows: [{ ...fixedValues.rows[0], enabled: 1 }] }), TypeError);
// Keep the envelope length valid while the row count demands more bytes than it contains.
const shortFixedFrame = withPayload(fixedFrame, fixedFrame.subarray(payloadOffset(fixedFrame), -1));
const fixedStream = Buffer.concat([shortFixedFrame, fixed.encode(fixedValues)]);
assert.throws(() => fixed.decodeFrom(fixedStream), /Unexpected end of Pojoc payload/);
assert.deepEqual(fixed.decodeFrom(fixedStream, shortFixedFrame.length).value, fixedValues);

assert.throws(() => fixed.open(fixedStream).rows, /Unexpected end of Pojoc payload/);
assert.throws(() => fixed.select(["rows"])(fixedStream), /Unexpected end of Pojoc payload/);
assert.deepEqual(fixed.open(fixedStream, shortFixedFrame.length).rows, fixedValues.rows);
assert.deepEqual(fixed.select(["rows"])(fixedStream, shortFixedFrame.length), fixedValues);

const oldFixedFrame = readFileSync(new URL("typescript_fixed_records-1.bin", dir));
for (const rows of [
  fixed.openRows(fixedStream, "rows", shortFixedFrame.length),
  fixed.openRows(oldFixedFrame, "rows"),
]) {
  assert.equal(rows.length, fixedValues.rows.length);
  for (const [i, row] of fixedValues.rows.entries()) {
    for (const key of Object.keys(row) as (keyof typeof row)[]) {
      assert.equal(rows.get(i, key), row[key]);
    }
  }
  for (const index of [-1, rows.length, 0.5, NaN, Infinity]) {
    assert.throws(() => rows.get(index, "id"), /row index out of bounds/);
  }
  assert.throws(() => rows.get(0, "toString"), /Unknown Pojoc row field/);
}
assert.throws(() => fixed.openRows(fixedStream, "missing"), /Unknown fixed Pojoc row array/);
assert.throws(() => fixed.openRows(fixedStream, "rows"), /Unexpected end of Pojoc payload/);
assert.equal(fixed.openRows(fixed.encode({ rows: [] }), "rows").length, 0);

const intern = codecs.get("interning");
const internBytes = readFileSync(new URL("interning-1.bin", dir));
const labels = ["", "São Paulo", "🦀", ...Array.from({ length: 128 }, (_, i) => `label ${i}`)];
for (const count of [1, 2, 3, labels.length]) {
  const unique = labels.slice(0, count);
  const value = intern.create({
    primary_label: unique[0],
    raw_tags: [...unique, ...unique.toReversed()],
    mono_label: { value: unique[0] },
  });
  const table = Buffer.concat([
    Buffer.from(varintBytes(BigInt(count))),
    ...unique.flatMap((label) => {
      const bytes = Buffer.from(label);
      return [Buffer.from(varintBytes(BigInt(bytes.length))), bytes];
    }),
  ]);
  const frame = intern.encode(value);
  const start = payloadOffset(frame);
  assert.deepEqual(Buffer.from(frame.subarray(start, start + table.length)), table);
  assert.deepEqual(intern.decode(frame), value);
  assert.throws(() => intern.encode({ ...value, primary_label: "\uD800" }), /Invalid Unicode string/);
}
const badIntern = Buffer.from(internBytes);
const third = badIntern.indexOf("third");
assert(third >= 0);
badIntern[third] = 0xff;
const internView = intern.open(badIntern);
assert.equal(internView.primary_label, "first");
assert.throws(() => internView.mono_label, TypeError);
assert.equal(internView.plain_label, "literal");
assert.deepEqual(intern.select(["plain_label", "primary_label", "plain_label"])(badIntern), {
  plain_label: "literal",
  primary_label: "first",
});
assert.throws(() => intern.select(["mono_label"])(badIntern), TypeError);
assert.throws(() => intern.decode(badIntern), TypeError);
assert.throws(() => intern.select(["missing"]), /Unknown Pojoc field/);

const importRoot = codecs.get("import_root");
const child = importRoot.decode(readFileSync(new URL("import_root-2.bin", dir)));
assert.equal(child.name, "outside");
assert.equal(child.child.label, "inside");
for (const size of [0, 126, 127, 128, 16_381, 16_382]) {
  const value = {
    ...child,
    child: { ...child.child, sig: { kind: "Text", value: "x".repeat(size) } },
  };
  assert.deepEqual(importRoot.decode(importRoot.encode(value)), value);
}

const constraints = codecs.get("constraints");
const formatting = await import(new URL("typescript_formatting.ts", dir).href);
const formattedFrame = formatting.encode(formatting.create());
assert.deepEqual([...formattedFrame.subarray(0, 2)], [128, 1]);
assert.equal(formatting.open(formattedFrame).constructor, 7);
assert.deepEqual(formatting.select(["constructor"])(formattedFrame), { constructor: 7 });

assert.equal(constraints.create().count, 5);
assert.throws(() => constraints.encode(constraints.create({ count: 11 })), RangeError);
const valid = constraints.encode(constraints.create());
const two = Buffer.concat([valid, valid]);
const next = constraints.decodeFrom(two);
assert.equal(next.offset, valid.length);
assert.equal(constraints.decodeFrom(two, next.offset).offset, two.length);

// The one-byte fast path must still respect the frame boundary and preserve the
// general reader for a version encoded across more than one byte.
for (let length = 0; length < payloadOffset(valid); length++) {
  assert.throws(() => constraints.decode(valid.subarray(0, length)), RangeError);
}
const oversized = withPayload(valid, valid.subarray(payloadOffset(valid)), 0xffff_ffff);
assert.throws(() => constraints.decode(oversized), RangeError);
assert.throws(() => constraints.decode(Uint8Array.of(...Array(9).fill(0x80), 2)), RangeError);
assert.throws(() => constraints.decode(Uint8Array.of(...Array(9).fill(0xff), 1)), /18446744073709551615/);
const wideVersion = Buffer.concat([Buffer.from([valid[0] | 0x80, 0]), valid.subarray(1)]);
assert.deepEqual(constraints.decodeFrom(wideVersion), { ...next, offset: wideVersion.length });

const nested = codecs.get("nested_pluck");
assert.throws(() => nested.seek(readFileSync(new URL("nested_pluck-3.bin", dir)), "transform.tail"));
const compact = await import(new URL("compact/nested_pluck.ts", dir).href);

function trailingByte(frame: Uint8Array): Buffer {
  const payload = Buffer.concat([frame.subarray(payloadOffset(frame)), Buffer.from([0xff])]);
  return withPayload(frame, payload);
}

// V2 shares V3's seek code. V1 needs historical ordering and field mapping.
for (const version of [1, 2]) {
  const frame = readFileSync(new URL(`nested_pluck-${version}.bin`, dir));
  const malformed = trailingByte(frame);
  assert.throws(() => nested.decode(malformed), /Unexpected trailing Pojoc payload/);
  assert.equal(nested.seek(malformed, "transform.position.x"), 99);
  const transform = nested.decode(frame).transform;
  assert.equal(nested.seek(malformed, "transform.description"), transform.description);
  assert.equal(nested.seek(malformed, "transform.mode"), transform.mode);
  assert.equal(nested.seek(malformed, "transform.position.label"), transform.position.label);
  assert.equal(compact.seek(frame, "transform.position.x"), 99);
  if (version === 2) assert.equal(compact.seek(malformed, "transform.position.x"), 99);
  else assert.throws(() => compact.seek(malformed, "transform.position.x"), /Unexpected trailing Pojoc payload/);

  const start = payloadOffset(frame);
  const xOffset = frame.indexOf(Buffer.from([99, 0, 0, 0]), start);
  assert(xOffset >= start);
  const short = withPayload(frame, frame.subarray(start, xOffset + 3));
  assert.throws(() => nested.seek(Buffer.concat([short, frame]), "transform.position.x"), RangeError);
}

const seekRetype = codecs.get("seek_retype");
const retypeV1 = readFileSync(new URL("seek_retype-1.bin", dir));
assert.equal(seekRetype.seek(trailingByte(retypeV1), "label"), "label");
assert.throws(() => seekRetype.seek(retypeV1, "label.inner"), RangeError);
for (const label of [null, "", "plain", "\uFEFFÅ🙂"]) {
  const frame = seekRetype.encode(seekRetype.create({ id: 0xffff_ffff, label }));
  assert.equal(seekRetype.seek(frame, "id"), 0xffff_ffff);
  assert.equal(seekRetype.seek(frame, "label"), label);
}
const seekText = Buffer.from(seekRetype.encode(seekRetype.create({ label: "seek UTF-8" })));
const seekTextStart = seekText.indexOf("seek UTF-8", payloadOffset(seekText));
assert(seekTextStart >= payloadOffset(seekText));
const shortSeekText = withPayload(seekText, seekText.subarray(payloadOffset(seekText), -1));
assert.throws(() => seekRetype.seek(Buffer.concat([shortSeekText, seekText]), "label"), RangeError);
seekText[seekTextStart] = 0xff;
assert.throws(() => seekRetype.seek(seekText, "label"), TypeError);

const edge = codecs.get("edge");
assert.equal(edge.constants.pi_const, Math.PI);
const edgeDefaults = edge.create();
const decodedDefaults = edge.decode(edge.encode(edgeDefaults));
const edgeCollections = edgeWithCollections(decodedDefaults);
protobufFixture(protoEdge, edge.decode(edge.encode(edgeCollections)));
function varintBytes(value: bigint): number[] {
  const bytes = [];
  while (value >= 128n) {
    bytes.push(Number(value & 127n) | 128);
    value >>= 7n;
  }
  bytes.push(Number(value));
  return bytes;
}

// Locate a signed delta array in a generated frame and replace just its bytes.
const deltaMarker = Buffer.from([1, ...varintBytes(2n * 0x1234_5678n)]);
const deltaFrame = Buffer.from(edge.encode(edge.create({
  legacy_positions: Int32Array.of(0x1234_5678),
})));
const deltaStart = deltaFrame.indexOf(deltaMarker, payloadOffset(deltaFrame));
assert(deltaStart >= payloadOffset(deltaFrame));
assert.equal(deltaFrame.indexOf(deltaMarker, deltaStart + 1), -1);
const beforeDeltas = deltaFrame.subarray(payloadOffset(deltaFrame), deltaStart);
const afterDeltas = deltaFrame.subarray(deltaStart + deltaMarker.length);
const zigzags = [
  0n, 1n,
  ...Array.from({ length: 9 }, (_, i) => 1n << BigInt(7 * (i + 1))),
  0x1_ffff_ffffn, 0xffff_ffff_ffff_ffffn,
];
let previous = 0n;
const expectedDeltas = Int32Array.from(zigzags.map((bits) => {
  previous = BigInt.asIntN(32, previous + ((bits >> 1n) ^ -(bits & 1n)));
  return Number(previous);
}));
const deltaPayload = Buffer.concat([
  beforeDeltas,
  Buffer.from([zigzags.length, ...zigzags.flatMap(varintBytes)]),
  afterDeltas,
]);
const deltaDecoded = edge.decode(Uint8Array.from(withPayload(deltaFrame, deltaPayload)));
assert.deepEqual(deltaDecoded, { ...decodedDefaults, legacy_positions: expectedDeltas });

for (let width = 1; width <= 9; width++) {
  const truncated = withPayload(deltaFrame, Buffer.concat([
    beforeDeltas, Buffer.from([1, ...Array(width).fill(0x80)]),
  ]));
  assert.throws(() => edge.decodeFrom(Buffer.concat([truncated, deltaFrame])), /Unexpected end/);
}
for (const last of [2, 0x80]) {
  const overflow = withPayload(deltaFrame, Buffer.concat([
    beforeDeltas, Buffer.from([1, ...Array(9).fill(0x80), last]), afterDeltas,
  ]));
  assert.throws(() => edge.decode(overflow), /Pojoc varint overflow/);
}

for (const name of ["action", "final_action"] as const) {
  assert.deepEqual(edgeDefaults[name], { kind: "Move", value: { dx: 0, dy: 0 } });
  assert.deepEqual(decodedDefaults[name], edgeDefaults[name]);
}
assert.equal(edgeDefaults.control.kind, "Ping");
assert.deepEqual(decodedDefaults.control, edgeDefaults.control);
assert.throws(() => constraints.decodeFrom(two, -1), RangeError);
const player = codecs.get("player");
const playerBytes = readFileSync(new URL("player-6.bin", dir));
let sharedPackedArrays = 0;
for (let prefix = 0; prefix < 4; prefix++) {
  const input = new Uint8Array(playerBytes.length + prefix);
  input.set(playerBytes, prefix);
  const ids = player.decode(input.subarray(prefix)).achievement_ids;
  assert.deepEqual(Array.from(ids), [1, 0xffff_ffff, 3]);
  if (ids.buffer === input.buffer) sharedPackedArrays++;
}
const littleEndian = new Uint8Array(new Uint16Array([1]).buffer)[0] === 1;
assert.equal(sharedPackedArrays, littleEndian ? 1 : 0);

// Version 1 starts with two adjacent four-byte scalars. Keep the envelope's
// declared length consistent after truncating their run, with another frame after it.
const firstPlayer = readFileSync(new URL("player-1.bin", dir));
const firstPlayerStart = payloadOffset(firstPlayer);
for (let length = 0; length < 8; length++) {
  const truncated = withPayload(firstPlayer, firstPlayer.subarray(firstPlayerStart, firstPlayerStart + length));
  const stream = Buffer.concat([truncated, firstPlayer]);
  assert.throws(() => player.decodeFrom(stream), RangeError);
  assert.equal(player.decodeFrom(stream, truncated.length).value.player_id, 17);
}

for (const preamble of [
  "", "a".repeat(127), "a".repeat(128), "å".repeat(64),
  "🦀".repeat(32), "漢".repeat(128), "\uFEFFå🦀".repeat(64),
]) {
  const record = nested.create({ preamble });
  assert.equal(nested.decode(nested.encode(record)).preamble, preamble);
}
assert.throws(() => nested.encode(nested.create({ preamble: "\uD800" })), TypeError);
assert.throws(() => nested.encode(nested.create({ preamble: "a".repeat(128) + "\uDC00" })), TypeError);
const batch = await import(new URL("typescript_batch.ts", dir).href);
const batchValue = { rows: [{ id: 0x1234, label: "hello" }] };
const batchFrame = Uint8Array.of(1, 9, 0, 0, 0, 1, 0x34, 0x12, 5, 104, 101, 108, 108, 111);
assert.deepEqual(batch.decode(batchFrame), batchValue);
assert.deepEqual(batch.encode(batchValue), batchFrame);
assert.deepEqual(batch.decode(batch.encode({ rows: [] })), { rows: [] });
for (const label of ["", "\u0000\u007f", "a".repeat(32), "a".repeat(33), "a".repeat(30) + "å", "\uFEFF🦀"]) {
  const value = { rows: [{ id: 1, label }, { id: 2, label: "World" }] };
  assert.deepEqual(batch.decode(batch.encode(value)), value);
}

for (const [length, prefix] of [
  [123, [127, 0, 0, 0]],
  [124, [128, 0, 0, 0]],
  [16_378, [255, 63, 0, 0]],
  [16_379, [0, 64, 0, 0]],
] as const) {
  const value = { rows: [{ id: 1, label: "x".repeat(length) }] };
  const frame = batch.encode(value);
  assert.deepEqual([...frame.subarray(1, payloadOffset(frame))], [...prefix]);
  assert.deepEqual(batch.decode(frame), value);
}

const invalidUtf8 = batchFrame.slice();
invalidUtf8[invalidUtf8.length - 1] = 0xff;
assert.throws(() => batch.decode(invalidUtf8), TypeError);
const truncatedRow = batchFrame.slice(0, -1);
truncatedRow[1]--;
assert.throws(() => batch.decode(truncatedRow), RangeError);
// The next envelope cannot supply a missing scalar, string, or varint byte.
const truncatedFixed = withPayload(batchFrame, Uint8Array.of(1, 0x34));
const truncatedLength = withPayload(batchFrame, Uint8Array.of(1, 0x34, 0x12, 0x80));
for (const broken of [truncatedFixed, truncatedLength, truncatedRow]) {
  const stream = Buffer.concat([Buffer.from([0]), broken, batchFrame]);
  assert.throws(() => batch.decodeFrom(stream, 1), /Unexpected end of Pojoc payload/);
  assert.deepEqual(batch.decodeFrom(stream, 1 + broken.length).value, batchValue);
}
const oversizedArray = withPayload(batchFrame, Uint8Array.of(0x81, 0x80, 0x80, 0x08));
assert.throws(() => batch.decode(oversizedArray), /object decode limit/);

for (const profile of ["bun", "node"]) {
  const tunedBatch = await import(new URL(`${profile}/typescript_batch.ts`, dir).href);
  for (const count of [0, 1, 8, 9, 16, 128]) {
    const value = { rows: Array.from({ length: count }, (_, id) => ({ id, label: id % 2 ? "á🦀" : "ASCII" })) };
    const frame = batch.encode(value);
    assert.deepEqual(tunedBatch.decode(frame), value);
    assert.deepEqual(tunedBatch.encode(value), frame);
    const stream = Buffer.concat([Buffer.from([0]), frame, batchFrame]);
    const decoded = tunedBatch.decodeFrom(stream, 1);
    assert.equal(decoded.offset, 1 + frame.length);
    assert.deepEqual(decoded.value, value);
    assert.deepEqual(tunedBatch.decodeFrom(stream, decoded.offset).value, batchValue);
  }
  for (const length of [1019, 1020, 1021]) {
    const value = { rows: [{ id: 9, label: "x".repeat(length) }] };
    assert.deepEqual(tunedBatch.decode(batch.encode(value)), value);
  }
  assert.throws(() => tunedBatch.decode(invalidUtf8), TypeError);
  assert.throws(() => tunedBatch.decode(truncatedRow), RangeError);
  assert.throws(() => tunedBatch.decode(oversizedArray), /object decode limit/);
  const trailing = trailingByte(batchFrame);
  assert.throws(() => tunedBatch.decode(trailing), /trailing Pojoc payload/);

  const codec = await import(new URL(`${profile}/edge.ts`, dir).href);
  for (const filename of artifacts.filter(name => name.startsWith("edge-"))) {
    const bytes = readFileSync(new URL(filename, dir));
    const decoded = edge.decodeFrom(bytes);
    assert.deepEqual(codec.decodeFrom(bytes), decoded, `${profile}: ${filename} decode`);
    assert.deepEqual(
      codec.encodeForVersion(decoded.value, decoded.version),
      Uint8Array.from(bytes),
      `${profile}: ${filename} wire bytes`,
    );
  }
}

// The flat writer must refresh its lazy DataView after growing the buffer.
const grownFixed = { rows: Array.from({ length: 48 }, (_, i) => fixedValues.rows[i % 2]) };
const grownBytes = fixed.encode(grownFixed);
assert.deepEqual(fixed.decode(grownBytes), grownFixed);
for (const profile of ["bun", "node"]) {
  const tuned = await import(new URL(profile + "/typescript_fixed_records.ts", dir).href);
  assert.deepEqual(tuned.encode(grownFixed), grownBytes);
  assert.deepEqual(tuned.decode(grownBytes), grownFixed);
}

const portableQuery: typeof import("./generated/typescript_query.ts") = await import(
  new URL("typescript_query.ts", dir).href
);
const portableRetype: typeof import("./generated/seek_retype.ts") = await import(
  new URL("seek_retype.ts", dir).href
);
for (const profile of ["bun", "node"]) {
  const query: typeof import("./generated/bun/typescript_query.ts") = await import(
    new URL(profile + "/typescript_query.ts", dir).href
  );
  for (const offset of [0, 127, 128, 16383, 16384, 0xffff_ffff]) {
    const value = { offset, limit: 0xffff_ffff - offset };
    const bytes = portableQuery.encode(value) satisfies BodyInit;
    assert.deepEqual(query.encode(value) satisfies BodyInit, bytes);
    assert.deepEqual(query.encodeForVersion(value, 1), bytes);
    assert.deepEqual(query.encodeForVersion(value, 1n), bytes);
    assert.deepEqual(query.decode(bytes), value);
  }
  assert.throws(() => query.encode({ offset: 0x1_0000_0000, limit: 1 }), /varint32/);
  assert.throws(() => query.encodeForVersion({ offset: 0, limit: 1 }, 2), /Unsupported Pojoc version/);

  const retype = await import(new URL(profile + "/seek_retype.ts", dir).href);
  const old = readFileSync(new URL("seek_retype-1.bin", dir));
  const current = portableRetype.decode(old);
  assert.deepEqual(retype.encode(current), portableRetype.encode(current));
  const historical = portableRetype.encodeForVersion(current, 1n) satisfies BodyInit;
  assert.deepEqual(retype.encodeForVersion(current, 1n), historical);
  assert.deepEqual(retype.decode(old), current);
}

const fixedEnvelope = await import(new URL("fixed_envelope.ts", dir).href);
for (const profile of ["bun", "node"]) {
  const tuned = await import(new URL(profile + "/fixed_envelope.ts", dir).href);
  for (const version of [1, 2, 128]) {
    const frame = Uint8Array.from(readFileSync(new URL(`fixed_envelope-${version}.bin`, dir)));
    const value = fixedEnvelope.decode(frame);
    assert.deepEqual(tuned.encodeForVersion(value, version), frame);
    if (version === 128) assert.deepEqual(tuned.encode(value), frame);
  }
}

console.log(`Verified ${artifacts.length} Rust envelopes, TypeScript byte equality, seeks and malformed inputs`);
