mod byte_array;
mod hybrid;

pub use byte_array::ByteArrayStream;
pub use hybrid::{HYBRID_MIN_PAYLOAD_BYTES, HybridLayout, HybridStream, StreamSpan};

use crate::{Error, PojocResult, read_varint64};
use std::io::{self, BufRead, Read, Write};

pub fn codec_error(err: Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err)
}

fn read_stream_varint(input: &mut impl Read, first: u8) -> io::Result<([u8; 10], usize)> {
    let mut bytes = [0; 10];
    bytes[0] = first;
    let mut len = 1;
    while bytes[len - 1] & 0x80 != 0 && len < bytes.len() {
        input.read_exact(&mut bytes[len..len + 1])?;
        len += 1;
    }
    Ok((bytes, len))
}

fn read_header(
    input: &mut impl BufRead,
    max_payload_bytes: usize,
) -> io::Result<Option<(u64, usize)>> {
    let Some(first) = input.bytes().next().transpose()? else {
        return Ok(None);
    };
    let (version_bytes, version_len) = read_stream_varint(input, first)?;
    if version_len == 10 && version_bytes[9] > 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid envelope version varint",
        ));
    }
    let version = read_varint64(&version_bytes[..version_len], &mut 0).map_err(codec_error)?;

    let mut length = [0; 4];
    input.read_exact(&mut length)?;
    let payload_len = u32::from_le_bytes(length) as usize;
    if payload_len > max_payload_bytes
        || (version_len + length.len())
            .checked_add(payload_len)
            .is_none()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "envelope payload exceeds limit",
        ));
    }
    Ok(Some((version, payload_len)))
}

pub fn serve_frames<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    max_payload_bytes: usize,
    mut reply: impl FnMut(&mut R, &mut W, u64, usize) -> io::Result<()>,
) -> io::Result<()> {
    while let Some((version, payload_len)) = read_header(input, max_payload_bytes)? {
        reply(input, output, version, payload_len)?;
        output.flush()?;
    }
    Ok(())
}

pub fn visit_payloads(
    input: &mut impl BufRead,
    max_payload_bytes: usize,
    mut visit: impl FnMut(u64, &[u8]) -> io::Result<()>,
) -> io::Result<()> {
    let mut payload = Vec::new();
    while let Some((version, payload_len)) = read_header(input, max_payload_bytes)? {
        visit(version, read_payload(input, payload_len, &mut payload)?)?;
    }
    Ok(())
}

pub fn read_buffered_reply(
    input: &mut impl BufRead,
    payload_len: usize,
    request: &mut Vec<u8>,
    response: &mut Vec<u8>,
    transcode: impl FnOnce(&[u8], &mut Vec<u8>) -> PojocResult<()>,
) -> io::Result<usize> {
    let payload = read_payload(input, payload_len, request)?;
    response.clear();
    transcode(payload, response).map_err(codec_error)?;
    Ok(response.len())
}

fn read_payload<'buf>(
    input: &mut impl BufRead,
    payload_len: usize,
    buffer: &'buf mut Vec<u8>,
) -> io::Result<&'buf [u8]> {
    // Retain initialized storage so only a new high-water mark needs zeroing.
    if buffer.len() < payload_len {
        buffer.resize(payload_len, 0);
    }
    let payload = &mut buffer[..payload_len];
    input.read_exact(payload)?;
    Ok(payload)
}
