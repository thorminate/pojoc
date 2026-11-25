import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import protobuf from "protobufjs";
import { engine, timeCases } from "./benchmark_timer.ts";
import { payloadOffset, withPayload } from "./frame_fixtures.ts";
import * as pojoc from "./generated/telemetry.ts";
import * as delta from "./generated/telemetry_delta.ts";
import * as varint from "./generated/telemetry_varint.ts";

const proto = protobuf.parse(readFileSync(new URL("./telemetry.proto", import.meta.url), "utf8"), {
  keepCase: true,
}).root.lookupType("Telemetry");

let sink = 0;

function verifyVarint32(): void {
  const widths = [
    [0, [0x00]],
    [0x7f, [0x7f]],
    [0x80, [0x80, 0x01]],
    [0x3fff, [0xff, 0x7f]],
    [0x4000, [0x80, 0x80, 0x01]],
    [0x1f_ffff, [0xff, 0xff, 0x7f]],
    [0x20_0000, [0x80, 0x80, 0x80, 0x01]],
    [0x0fff_ffff, [0xff, 0xff, 0xff, 0x7f]],
    [0x1000_0000, [0x80, 0x80, 0x80, 0x80, 0x01]],
    [0xffff_ffff, [0xff, 0xff, 0xff, 0xff, 0x0f]],
  ] as const;
  const value = varint.create();
  const empty = varint.encode(value);
  const prefix = empty.subarray(payloadOffset(empty), -1); // Exclude the empty actor_ids count.
  function withIds(bytes: number[]): Uint8Array {
    const frame = new Uint8Array(prefix.length + bytes.length);
    frame.set(prefix);
    frame.set(bytes, prefix.length);
    return Uint8Array.from(withPayload(empty, frame));
  }

  const repeated = Array.from({ length: 32 }, () => widths).flat();
  const ids = repeated.map(([id]) => id);
  const encodedIds = repeated.flatMap(([, bytes]) => [...bytes]);
  const frame = withIds([0xc0, 0x02, ...encodedIds]); // 320 entries, then the varints.
  assert.deepEqual(varint.encode({ ...value, actor_ids: ids }), frame);
  assert.deepEqual(varint.decode(frame).actor_ids, ids);
  assert.deepEqual(varint.decode(withIds([0x81, 0x80, 0x80, 0x80, 0x80, 0, 0x7f])).actor_ids, [0x7f]);
  for (const bytes of [
    [2, 0x00],
    [1, 0x80],
    [1, 0xff, 0xff, 0xff, 0xff, 0x10],
    [1, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00],
  ]) {
    assert.throws(() => varint.decode(withIds(bytes)), RangeError);
  }
  for (const id of [-1, 1.5, NaN, 0x1_0000_0000]) {
    assert.throws(() => varint.encode({ ...value, actor_ids: [id] }), RangeError);
  }
}

type Samples = { samples: ArrayLike<number>; actor_ids: ArrayLike<number> };

function observe({ samples, actor_ids }: Samples): number {
  return samples[samples.length - 1] + actor_ids[actor_ids.length - 1];
}

function pojocCase<T extends Samples>(
  name: string,
  codec: { encode(value: T): Uint8Array; decode(frame: Uint8Array): T },
  inputs: T[],
) {
  const frames = inputs.map(codec.encode);
  for (let i = 0; i < inputs.length; i++) {
    assert.deepEqual(codec.decode(frames[i]), inputs[i], name);
  }
  return {
    name,
    frames,
    encode: (i: number) => codec.encode(inputs[i]),
    decode: (frame: Uint8Array) => observe(codec.decode(frame)),
  };
}

verifyVarint32();
console.log(engine);
console.log("elements\tcodec\tbytes\tencode_ns\tdecode_ns\troundtrip_ns\tencodes_per_second\tdecodes_per_second");
for (const count of [16, 256, 4096]) {
  const inputs: pojoc.Telemetry[] = Array.from({ length: 16 }, (_, i) => ({
    tick: i + 1,
    server_id: 0xf000_0000_0000_0000n + BigInt(i),
    label: `server ${i} · telemetry`,
    position: { x: 0.5, y: -1.25 },
    samples: Float32Array.from({ length: count }, (_, j) => (j - i) / 16),
    actor_ids: Uint32Array.from({ length: count }, (_, j) => j * 7 + i),
  }));
  const varintInputs = inputs.map(value => ({ ...value, actor_ids: Array.from(value.actor_ids) }));
  const protoInputs = varintInputs.map(value => proto.fromObject({
    ...value,
    server_id: value.server_id.toString(),
    samples: Array.from(value.samples),
    actor_ids: value.actor_ids,
  }));
  const protoFrames = protoInputs.map(value => proto.encodeDelimited(value).finish());
  for (let i = 0; i < inputs.length; i++) {
    const expected = inputs[i];
    const pb = proto.toObject(proto.decodeDelimited(protoFrames[i]), { longs: String });
    assert.deepEqual(pb, {
      ...expected,
      server_id: expected.server_id.toString(),
      samples: Array.from(expected.samples),
      actor_ids: Array.from(expected.actor_ids),
    });
  }

  const cases = [
    pojocCase("Pojoc fixed", pojoc, inputs),
    pojocCase("Pojoc varint32", varint, varintInputs),
    pojocCase("Pojoc delta", delta, inputs),
    {
      name: "protobuf.js",
      frames: protoFrames,
      encode: (i: number) => proto.encodeDelimited(protoInputs[i]).finish(),
      decode: (frame: Uint8Array) => observe(proto.decodeDelimited(frame) as unknown as Samples),
    },
  ];
  const writes = timeCases(cases.map(({ encode }) => i => { sink ^= encode(i).length; }));
  const reads = timeCases(cases.map(({ decode, frames }) => i => { sink ^= decode(frames[i]); }));
  const trips = timeCases(cases.map(({ encode, decode }) => i => { sink ^= decode(encode(i)); }));
  for (const [index, { name, frames }] of cases.entries()) {
    const bytes = frames.reduce((sum, frame) => sum + frame.length, 0) / frames.length;
    console.log([
      count, name, bytes.toFixed(1), writes[index].toFixed(1), reads[index].toFixed(1),
      trips[index].toFixed(1), Math.round(1e9 / writes[index]), Math.round(1e9 / reads[index]),
    ].join("\t"));
  }
}
if (!Number.isInteger(sink)) throw new Error("Invalid benchmark sink");
