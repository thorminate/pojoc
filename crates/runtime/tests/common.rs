use pojoc::*;

pub fn write_envelope(buf: &mut Vec<u8>, version: u64, payload: &[u8]) {
    let len_pos = write_envelope_header(buf, version);
    buf.extend_from_slice(payload);
    let payload_len = payload.len();
    patch_envelope_length(buf, len_pos, payload_len);
}