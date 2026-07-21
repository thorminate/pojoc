use pojoc_tests::pojoc_edge::{runtime::*, *};

#[test]
fn test_truncated_buffer() {
    let mut buf = Vec::new();
    encode(&mut buf, &Edge::default()).expect("encode failed");
    buf.truncate(buf.len() / 2);
    assert!(decode(&buf).is_err());
}

#[test]
fn test_empty_buffer() {
    assert!(decode(&[]).is_err());
}

#[test]
fn test_unsupported_version() {
    let mut buf = Vec::new();
    let len_pos = write_envelope_header(&mut buf, 99);
    let payload_len = 0;
    patch_envelope_length(&mut buf, len_pos, payload_len);
    assert!(matches!(decode(&buf), Err(Error::UnsupportedVersion(99))));
}

#[test]
fn test_invalid_length() {
    // claim payload is huge but buffer is tiny
    let mut buf = vec![0x05]; // version varint = 5
    buf.extend_from_slice(&u32::MAX.to_le_bytes()); // absurd length
    assert!(decode(&buf).is_err());
}
