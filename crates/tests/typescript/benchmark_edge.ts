import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { engine, time } from "./benchmark_timer.ts";
import * as edge from "./generated/edge.ts";

function observe(value: edge.Edge): number {
  return value.seek_tag ^ value.transform.position.x ^ value.packed_varints.length;
}

let checksum = 0;
console.log(engine);
console.log("version\tbytes\tencode_ns\tdecode_ns\troundtrip_ns\tnested_seek_ns");
for (const version of new Set([6n, 7n, edge.version])) {
  const fixture = new URL(`./generated/edge-populated-${version}.bin`, import.meta.url);
  const frame = new Uint8Array(readFileSync(fixture));
  const decoded = edge.decodeFrom(frame);
  assert.equal(decoded.version, version);
  assert.equal(decoded.offset, frame.length);
  const value = decoded.value;
  const encode = version === edge.version
    ? () => edge.encode(value)
    : () => edge.encodeForVersion(value, version);
  assert.deepEqual(encode(), frame);

  const write = time(() => { checksum = (checksum + encode().length) | 0; });
  const read = time(() => { checksum = (checksum + observe(edge.decode(frame))) | 0; });
  const roundtrip = time(() => { checksum = (checksum + observe(edge.decode(encode()))) | 0; });
  let nestedSeek = "-";
  if (version >= 7n) {
    const path = "transform.position.x";
    assert.equal(edge.seek(frame, path), value.transform.position.x);
    nestedSeek = time(() => {
      checksum = (checksum + (edge.seek(frame, path) as number)) | 0;
    }).toFixed(1);
  }
  console.log(`${version}\t${frame.length}\t${write.toFixed(1)}\t${read.toFixed(1)}\t${roundtrip.toFixed(1)}\t${nestedSeek}`);
}
console.error(`Checksum ${checksum}`);
