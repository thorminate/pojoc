import { Buffer } from "node:buffer";

function skipVarint(frame: Uint8Array, offset: number): number {
  while (frame[offset++] >= 0x80) {
    // Skip the continuation bytes of a known-good header.
  }
  return offset;
}

export function payloadOffset(frame: Uint8Array): number {
  return skipVarint(frame, 0) + 4;
}

export function withPayload(
  frame: Uint8Array,
  payload: Uint8Array,
  length = payload.length,
): Buffer {
  const prefix = Buffer.alloc(4);
  prefix.writeUInt32LE(length);
  return Buffer.concat([frame.subarray(0, skipVarint(frame, 0)), prefix, payload]);
}
