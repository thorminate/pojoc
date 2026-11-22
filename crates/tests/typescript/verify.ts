import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { readdirSync, readFileSync } from "node:fs";
import process from "node:process";

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
  assert.throws(() => codec.decode(bytes.subarray(0, -1)), RangeError, filename);
  if (schema === "player") assert.equal(decoded.value.player_id, 17);
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

const importRoot = codecs.get("import_root");
const child = importRoot.decode(readFileSync(new URL("import_root-2.bin", dir)));
assert.equal(child.name, "outside");
assert.equal(child.child.label, "inside");

const constraints = codecs.get("constraints");
assert.equal(constraints.create().count, 5);
assert.throws(() => constraints.encode(constraints.create({ count: 11 })), RangeError);
const valid = constraints.encode(constraints.create());
const two = Buffer.concat([valid, valid]);
const next = constraints.decodeFrom(two);
assert.equal(next.offset, valid.length);
assert.equal(constraints.decodeFrom(two, next.offset).offset, two.length);

const nested = codecs.get("nested_pluck");
assert.throws(() => nested.seek(readFileSync(new URL("nested_pluck-3.bin", dir)), "transform.tail"));
const edge = codecs.get("edge");
assert.equal(edge.constants.pi_const, Math.PI);
const edgeDefaults = edge.create();
const decodedDefaults = edge.decode(edge.encode(edgeDefaults));
for (const name of ["action", "final_action"] as const) {
  assert.deepEqual(edgeDefaults[name], { kind: "Move", value: { dx: 0, dy: 0 } });
  assert.deepEqual(decodedDefaults[name], edgeDefaults[name]);
}
assert.equal(edgeDefaults.control.kind, "Ping");
assert.deepEqual(decodedDefaults.control, edgeDefaults.control);
assert.throws(() => constraints.decodeFrom(two, -1), RangeError);
const player = codecs.get("player");
const playerBytes = readFileSync(new URL("player-6.bin", dir));
const unaligned = new Uint8Array(playerBytes.length + 1);
unaligned.set(playerBytes, 1);
assert.deepEqual(
  Array.from(player.decode(unaligned.subarray(1)).achievement_ids),
  [1, 0xffff_ffff, 3],
);
console.log(`Verified ${artifacts.length} Rust envelopes, TypeScript byte equality, seeks and malformed inputs`);
