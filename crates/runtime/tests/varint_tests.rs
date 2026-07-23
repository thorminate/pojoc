use pojoc::*;

fn roundtrip(value: u64) {
    let mut buf = Vec::new();
    write_varint64(&mut buf, value);
    let mut pos = 0;
    let decoded = read_varint64(&buf, &mut pos).unwrap();
    assert_eq!(decoded, value);
    assert_eq!(pos, buf.len());
}

#[test]
fn roundtrip_zero() {
    roundtrip(0);
}
#[test]
fn roundtrip_one() {
    roundtrip(1);
}
#[test]
fn roundtrip_127() {
    roundtrip(127);
}
#[test]
fn roundtrip_128() {
    roundtrip(128);
}
#[test]
fn roundtrip_300() {
    roundtrip(300);
}
#[test]
fn roundtrip_u32_max() {
    roundtrip(u32::MAX as u64);
}
#[test]
fn roundtrip_u64_max() {
    roundtrip(u64::MAX);
}

#[test]
fn encoding_sizes() {
    let mut buf = Vec::new();
    write_varint64(&mut buf, 127);
    assert_eq!(buf.len(), 1);

    buf.clear();
    write_varint64(&mut buf, 128);
    assert_eq!(buf.len(), 2);

    buf.clear();
    write_varint64(&mut buf, u64::MAX);
    assert_eq!(buf.len(), 10);
}

#[test]
fn decode_empty_buffer_errors() {
    let mut pos = 0;
    assert_eq!(read_varint64(&[], &mut pos), Err(Error::UnexpectedEof));
}

#[test]
fn decode_truncated_errors() {
    // 0x80 = continuation bit set, never terminates
    let buf = vec![0x80u8; 5];
    let mut pos = 0;
    assert_eq!(read_varint64(&buf, &mut pos), Err(Error::UnexpectedEof));
}

#[test]
fn decode_overflow_errors() {
    // u64 varint maxes out at 10 bytes, 11 continuation bytes overflows
    let buf = vec![0x80u8; 11];
    let mut pos = 0;
    assert_eq!(read_varint64(&buf, &mut pos), Err(Error::VarIntOverflow));
}
