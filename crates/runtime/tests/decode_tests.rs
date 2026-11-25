mod common;

use common::*;
use pojoc::*;

#[test]
fn roundtrip_u8() {
    let mut buf = Vec::new();
    write_u8(&mut buf, 255);
    let mut pos = 0;
    assert_eq!(read_u8(&buf, &mut pos).unwrap(), 255);
    assert_eq!(pos, 1);
}

#[test]
fn roundtrip_bool() {
    let mut buf = Vec::new();
    write_bool(&mut buf, true);
    write_bool(&mut buf, false);
    let mut pos = 0;
    assert!(read_bool(&buf, &mut pos).unwrap());
    assert!(!read_bool(&buf, &mut pos).unwrap());
}

#[test]
fn roundtrip_u32() {
    let mut buf = Vec::new();
    write_u32(&mut buf, 123456);
    let mut pos = 0;
    assert_eq!(read_u32(&buf, &mut pos).unwrap(), 123456);
    assert_eq!(pos, 4);
}

#[test]
fn roundtrip_u64() {
    let mut buf = Vec::new();
    write_u64(&mut buf, u64::MAX);
    let mut pos = 0;
    assert_eq!(read_u64(&buf, &mut pos).unwrap(), u64::MAX);
    assert_eq!(pos, 8);
}

#[test]
fn roundtrip_bytes() {
    let mut buf = Vec::new();
    write_bytes(&mut buf, b"hello world");
    let mut pos = 0;
    assert_eq!(read_bytes(&buf, &mut pos).unwrap(), b"hello world");
}

#[test]
fn roundtrip_string() {
    let mut buf = Vec::new();
    write_string(&mut buf, "pojoc rocks");
    let mut pos = 0;
    assert_eq!(read_string(&buf, &mut pos).unwrap(), "pojoc rocks");
}

#[test]
fn roundtrip_envelope() {
    let mut buf = Vec::new();
    let len_pos = write_envelope_header(&mut buf, 1);
    let payload = b"my payload";
    buf.extend_from_slice(payload);
    let payload_len = payload.len();
    patch_envelope_length(&mut buf, len_pos, payload_len);
    let mut pos = 0;
    let env = read_envelope(&buf, &mut pos, |_| Ok(None)).unwrap();
    assert_eq!(env.version, 1);
    assert_eq!(env.payload, payload);
    assert_eq!(pos, buf.len());
}

#[test]
fn stream_of_envelopes() {
    let mut buf = Vec::new();
    write_envelope(&mut buf, 1, b"first");
    write_envelope(&mut buf, 1, b"second");
    write_envelope(&mut buf, 2, b"third");

    let mut messages: Vec<(u64, Vec<u8>)> = Vec::new();
    read_envelope_stream(
        &buf,
        |_| Ok(None),
        |version, payload| {
            messages.push((version, payload.to_vec()));
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0], (1, b"first".to_vec()));
    assert_eq!(messages[1], (1, b"second".to_vec()));
    assert_eq!(messages[2], (2, b"third".to_vec()));
}

#[test]
fn buffered_stream_exposes_only_the_current_payload_after_reuse() {
    let mut request = Vec::new();
    let mut response = Vec::new();
    for bytes in [&[1, 2, 3, 4][..], &[5][..], &[][..], &[6, 7][..]] {
        let mut input = bytes;
        let response_len = stream::read_buffered_reply(
            &mut input,
            bytes.len(),
            &mut request,
            &mut response,
            |payload, output| {
                assert_eq!(payload, bytes);
                output.extend_from_slice(payload);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(&response[..response_len], bytes);
    }
}

#[test]
fn read_truncated_u32_errors() {
    let buf = vec![0x01, 0x02];
    let mut pos = 0;
    assert_eq!(read_u32(&buf, &mut pos), Err(Error::UnexpectedEof));
}

#[test]
fn unaligned_pod_arrays_decode_into_inline_and_heap_storage() {
    let expected = [
        0x0504_0302_u32,
        0,
        48,
        300,
        u32::MAX,
        0x0a09_0807,
        1,
        31,
        42,
    ];
    for len in [8, 9] {
        let mut wire = Vec::new();
        write_pod_array(&mut wire, &expected[..len]);
        let mut aligned = [0_u32; 10];
        let bytes = bytemuck::cast_slice_mut::<u32, u8>(&mut aligned);
        bytes[..wire.len()].copy_from_slice(&wire);
        // The one-byte count puts the elements at an unaligned address.
        let mut pos = 0;
        let values = read_pod_array::<u32>(&bytes[..wire.len()], &mut pos).unwrap();
        assert_eq!(values.as_slice(), &expected[..len]);
        assert_eq!(values.spilled(), len > 8);
        assert_eq!(pos, wire.len());
        assert_eq!(
            read_pod_array::<u32>(&bytes[..wire.len() - 1], &mut 0),
            Err(Error::UnexpectedEof)
        );
    }
}

#[test]
fn read_envelope_bad_length_errors() {
    let mut buf = Vec::new();
    let length_pos = write_envelope_header(&mut buf, 1);
    patch_envelope_length(&mut buf, length_pos, 100);
    let mut pos = 0;
    match read_envelope(&buf, &mut pos, |_| Ok(None)) {
        Err(Error::InvalidLength) => {}
        Ok(e) => panic!("expected error, got envelope: {:?}", e),
        Err(e) => panic!("wrong error: {:?}", e),
    }
}

#[test]
fn dynamic_envelope_length_stays_at_the_same_offset() {
    let version = 128;
    for size in [0, 127, 128, 16_383, 16_384, 2_097_152] {
        let payload = vec![0xab; size];
        let mut expected = vec![42];
        expected.extend_from_slice(&[128, 1]);
        expected.extend_from_slice(&(size as u32).to_le_bytes());
        let expected_start = expected.len();
        expected.extend_from_slice(&payload);
        for hint in [0, size, 16_384] {
            let mut frame = vec![42];
            let length_pos = write_envelope_header_with_hint(&mut frame, &[128, 1], hint);
            assert_eq!(frame.len(), expected_start);
            frame.extend_from_slice(&payload);
            assert_eq!(
                patch_envelope_length(&mut frame, length_pos, size),
                expected_start
            );
            assert_eq!(frame, expected);
            let envelope = read_envelope(&frame, &mut 1, |_| Ok(None)).unwrap();
            assert_eq!(envelope.version, version);
            assert_eq!(envelope.payload, payload);
        }
    }
}

#[test]
fn rejects_truncated_lengths_and_overflowing_version_tags() {
    assert!(matches!(
        read_envelope(&[1, 0x80], &mut 0, |_| Ok(None)),
        Err(Error::UnexpectedEof)
    ));
    for last in [2, 0x80] {
        let mut wire = vec![0x80; 9];
        wire.push(last);
        assert!(matches!(
            read_envelope(&wire, &mut 0, |_| Ok(None)),
            Err(Error::VarIntOverflow)
        ));
    }
}

#[test]
fn full_u64_version_range_and_schema_bound_fixed_payload() {
    let mut dynamic = Vec::new();
    write_envelope(&mut dynamic, u64::MAX, b"hello");
    assert_eq!(
        read_envelope(&dynamic, &mut 0, |_| Ok(None))
            .unwrap()
            .version,
        u64::MAX
    );

    let wire = [1, 0xca, 0xfe, 2, 0, 0, 0, 0];
    let schema = |version| match version {
        1 => Ok(Some(2)),
        2 => Ok(None),
        v => Err(Error::UnsupportedVersion(v)),
    };
    assert!(matches!(
        read_envelope(&[3], &mut 0, schema),
        Err(Error::UnsupportedVersion(3))
    ));
    let mut pos = 0;
    let fixed = read_envelope(&wire, &mut pos, schema).unwrap();
    assert_eq!(fixed.payload, [0xca, 0xfe]);
    assert_eq!(pos, 3);
    assert_eq!(read_envelope(&wire, &mut pos, schema).unwrap().version, 2);
    assert_eq!(pos, wire.len());
}
