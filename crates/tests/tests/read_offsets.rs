use codec::runtime::*;
use pojoc_tests::{pluck_read_offsets as pluck, pojoc_read_offsets as codec};

fn payload(version: u64, blob: &[u8]) -> Vec<u8> {
    let mut payload = vec![0x81, 0x2b, 1];
    if version == 2 {
        payload.extend([0x38, 1]);
    }
    payload.extend([7, 8, 1]);
    if version == 1 {
        write_u16(&mut payload, 0x1234);
    } else {
        write_u32(&mut payload, 0x1234);
    }
    write_bytes(&mut payload, blob);
    if version == 1 {
        write_array_len(&mut payload, 0);
    }
    for (key, value) in [(11, 42), (103, 211)] {
        write_u32(&mut payload, key);
        write_u16(&mut payload, value);
    }
    write_string(&mut payload, "after the map");
    payload
}

fn envelope(version: u64, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let length_pos = write_envelope_header(&mut bytes, version);
    bytes.extend_from_slice(payload);
    patch_envelope_length(&mut bytes, length_pos, payload.len());
    bytes
}

#[test]
fn historical_and_current_field_offsets_match_the_wire_bytes() {
    let mut stream = Vec::new();
    let mut expected_replies = Vec::new();
    let blob = vec![0xa5; 40 * 1024];
    for version in [1, 2] {
        let payload = payload(version, &blob);
        let frame = envelope(version, &payload);
        let decoded = codec::decode(&frame).unwrap();
        assert_eq!(
            decoded.flags.0,
            [
                0x81,
                0x2b,
                1,
                if version == 2 { 0x38 } else { 0 },
                u8::from(version == 2)
            ]
        );
        assert_eq!(decoded.snapshot.0, [7, 8, 1, 0, 0]);
        assert_eq!(codec::seek_snapshot(&frame).unwrap().0, decoded.snapshot.0);
        assert_eq!(decoded.stamp, 0x1234);
        assert_eq!(decoded.blob.as_slice(), blob);
        assert_eq!(decoded.marker, "after the map");
        assert_eq!(codec::seek_stamp(&frame).unwrap(), decoded.stamp);
        assert_eq!(codec::seek_marker(&frame).unwrap(), decoded.marker);
        assert_eq!(codec::seek_all(&frame).unwrap().marker, decoded.marker);
        assert_eq!(
            pluck!(codec, &frame, stamp, marker).unwrap(),
            (decoded.stamp, decoded.marker)
        );
        let mut pos = 0;
        match version {
            1 => codec::skip_v1(&payload, &mut pos),
            _ => codec::skip_v2(&payload, &mut pos),
        }
        .unwrap();
        assert_eq!(pos, payload.len());
        let mut reencoded = Vec::new();
        codec::encode_for_version(&mut reencoded, &decoded, version).unwrap();
        assert_eq!(reencoded, frame);
        codec::encode(&mut expected_replies, &decoded);
        stream.extend_from_slice(&frame);
    }
    let mut replies = Vec::new();
    codec::serve(&mut stream.as_slice(), &mut replies, 64 * 1024).unwrap();
    assert_eq!(replies, expected_replies);
}

#[test]
fn truncated_fixed_block_and_seek_prefixes_return_errors() {
    for (version, stamp_end) in [(1, 8), (2, 12)] {
        let payload = payload(version, &[]);
        for end in 0..payload.len() {
            let frame = envelope(version, &payload[..end]);
            assert!(codec::decode(&frame).is_err(), "v{version} cut at {end}");
            assert!(
                codec::seek_marker(&frame).is_err(),
                "v{version} cut at {end}"
            );
            assert!(pluck!(codec, &frame, stamp, marker).is_err());
            if end < stamp_end {
                assert!(codec::seek_stamp(&frame).is_err());
            } else {
                assert_eq!(codec::seek_stamp(&frame).unwrap(), 0x1234);
            }
        }
    }
}

#[test]
fn discarded_array_length_is_checked_before_advancing() {
    let mut payload = payload(1, &[]);
    // The empty blob follows the fixed block. The retired array is next.
    let retired_len = 8 + 1;
    let mut invalid_len = Vec::new();
    write_varint64(&mut invalid_len, u64::MAX);
    payload.splice(retired_len..retired_len + 1, invalid_len);
    let frame = envelope(1, &payload);
    assert!(matches!(codec::decode(&frame), Err(Error::InvalidLength)));
    assert!(codec::seek_marker(&frame).is_err());
}
