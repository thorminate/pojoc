import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import type { Type } from "protobufjs";
import { engine, time, timeCases } from "./benchmark_timer.ts";
import {
  edgeWithCollections,
  protoEdge,
  protoPlayer,
  protobufFixture,
} from "./edge_protobuf.ts";
import * as edge from "./generated/edge.ts";
import * as player from "./generated/player.ts";

type EdgeProbe = Pick<edge.Edge, "seek_tag" | "transform" | "packed_varints">;
type PlayerProbe = Pick<
  player.Player,
  "player_id" | "stats" | "achievement_ids"
>;

function observeEdge(value: EdgeProbe): number {
  return (
    value.seek_tag ^ value.transform.position.x ^ value.packed_varints.length
  );
}

function observePlayer(value: PlayerProbe): number {
  return value.player_id + value.stats.strength + value.achievement_ids.length;
}

type Case = {
  codec: string;
  version: bigint | "-";
  frame: Uint8Array;
  encode: () => Uint8Array;
  read: (frame: Uint8Array) => number;
  seek?: () => number;
};

function readFixture(name: string, version: bigint): Uint8Array {
  return new Uint8Array(
    readFileSync(
      new URL(`./generated/${name}-${version}.bin`, import.meta.url),
    ),
  );
}

function protobufCase<T>(
  type: Type,
  value: T,
  observe: (value: T) => number,
): Case {
  const { input, frame } = protobufFixture(type, value);
  return {
    codec: "protobuf.js",
    version: "-",
    frame,
    encode: () => type.encodeDelimited(input).finish(),
    read: (bytes) => observe(type.decodeDelimited(bytes) as unknown as T),
  };
}

function edgeCase(frame: Uint8Array, version: bigint) {
  const decoded = edge.decodeFrom(frame);
  assert.equal(decoded.version, version);
  assert.equal(decoded.offset, frame.length);
  const value = decoded.value;
  const encode =
    version === edge.version
      ? () => edge.encode(value)
      : () => edge.encodeForVersion(value, version);
  assert.deepEqual(encode(), frame);
  const pojoc: Case = {
    codec: "Pojoc",
    version,
    frame,
    encode,
    read: (bytes) => observeEdge(edge.decode(bytes)),
  };
  if (version >= 7n) {
    const path = "transform.position.x";
    const seek = () => edge.seek(frame, path) as number;
    assert.equal(seek(), value.transform.position.x);
    pojoc.seek = seek;
  }
  return { value, pojoc };
}

let checksum = 0;

function report(name: string, cases: Case[]): void {
  const expected = cases[0].read(cases[0].frame);
  for (const item of cases)
    assert.equal(item.read(item.frame), expected, item.codec);
  const encode = timeCases(
    cases.map((item) => () => {
      checksum = (checksum + item.encode().length) | 0;
    }),
  );
  const decode = timeCases(
    cases.map((item) => () => {
      checksum = (checksum + item.read(item.frame)) | 0;
    }),
  );
  const roundtrip = timeCases(
    cases.map((item) => () => {
      checksum = (checksum + item.read(item.encode())) | 0;
    }),
  );
  for (const [index, item] of cases.entries()) {
    const seek = item.seek;
    const seekNs = seek
      ? time(() => {
          checksum = (checksum + seek()) | 0;
        }).toFixed(1)
      : "-";
    console.log(
      [
        name,
        item.codec,
        item.version,
        item.frame.length,
        encode[index].toFixed(1),
        decode[index].toFixed(1),
        roundtrip[index].toFixed(1),
        Math.round(1e9 / encode[index]),
        Math.round(1e9 / decode[index]),
        seekNs,
      ].join("\t"),
    );
  }
}

console.log(engine);
console.log(
  "fixture\tcodec\tversion\tbytes\tencode_ns\tdecode_ns\troundtrip_ns\tencodes_per_second\tdecodes_per_second\tnested_seek_ns",
);

const playerFrame = readFixture("player", player.version);
const playerValue = player.decode(playerFrame);
assert.deepEqual(player.encode(playerValue), playerFrame);
report("Player", [
  {
    codec: "Pojoc",
    version: player.version,
    frame: playerFrame,
    encode: () => player.encode(playerValue),
    read: (bytes) => observePlayer(player.decode(bytes)),
  },
  protobufCase(protoPlayer, playerValue, observePlayer),
]);

for (const name of ["edge", "edge-populated"]) {
  const { value, pojoc } = edgeCase(
    readFixture(name, edge.version),
    edge.version,
  );
  report(name, [pojoc, protobufCase(protoEdge, value, observeEdge)]);
}
const collections = edgeWithCollections(
  edge.decode(readFixture("edge-populated", edge.version)),
);
const expanded = edgeCase(edge.encode(collections), edge.version);
report("edge-collections", [
  expanded.pojoc,
  protobufCase(protoEdge, expanded.value, observeEdge),
]);
for (const version of [6n, 7n]) {
  if (version === edge.version) continue;
  const { pojoc } = edgeCase(readFixture("edge-populated", version), version);
  report("edge-populated", [pojoc]);
}
console.error(`Checksum ${checksum}`);
