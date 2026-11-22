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
    let env = read_envelope(&buf, &mut pos).unwrap();
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
    read_envelope_stream(&buf, |version, payload| {
        messages.push((version, payload.to_vec()));
        Ok(())
    })
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
    write_varint64(&mut buf, 1);
    write_u32(&mut buf, 100);
    let mut pos = 0;
    match read_envelope(&buf, &mut pos) {
        Err(Error::InvalidLength) => {}
        Ok(e) => panic!("expected error, got envelope: {:?}", e),
        Err(e) => panic!("wrong error: {:?}", e),
    }
}
