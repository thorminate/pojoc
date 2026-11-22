//! Test-only rkyv sidecar for the same v1-to-v2 upgrade as `stdio_bench.pojoc`.
use rkyv::{
    Archive, Archived, Serialize, api::high::to_bytes_in, rancor::Error, util::AlignedVec,
    with::AsVec,
};
use std::io::{self, BufRead, Read, Write};

// [payload length: u32 LE][version: u32 LE][archive]. Eight bytes of framing
// keep the archive aligned both in the pipe read buffer and the reply writer.
const HEADER: usize = 8;

#[derive(Archive, Serialize)]
struct Request<'a> {
    id: u64,
    #[rkyv(with = AsVec)]
    payload: &'a [u8],
}

#[derive(Archive, Serialize)]
struct Reply<'a> {
    id: u64,
    #[rkyv(with = AsVec)]
    payload: &'a [u8],
    flags: u32,
}

const _: () = {
    assert!(HEADER.is_multiple_of(std::mem::align_of::<Archived<Request<'static>>>()));
    assert!(HEADER.is_multiple_of(std::mem::align_of::<Archived<Reply<'static>>>()));
};

fn begin_frame(buf: &mut AlignedVec, version: u32) {
    buf.clear();
    buf.extend_from_slice(&[0; 4]);
    buf.extend_from_slice(&version.to_le_bytes());
}

fn finish_frame(buf: &mut AlignedVec) -> io::Result<()> {
    let len = u32::try_from(buf.len() - HEADER)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    buf[..4].copy_from_slice(&len.to_le_bytes());
    Ok(())
}

fn encode_reply(buf: &mut AlignedVec, id: u64, payload: &[u8], flags: u32) -> io::Result<()> {
    begin_frame(buf, 2);
    to_bytes_in::<_, Error>(&Reply { id, payload, flags }, &mut *buf)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    finish_frame(buf)
}

pub fn frame_pair(payload: &[u8]) -> (AlignedVec, AlignedVec) {
    let mut request = AlignedVec::new();
    begin_frame(&mut request, 1);
    to_bytes_in::<_, Error>(&Request { id: 42, payload }, &mut request).unwrap();
    finish_frame(&mut request).unwrap();
    let mut reply = AlignedVec::new();
    encode_reply(&mut reply, 42, payload, 0).unwrap();
    (request, reply)
}

fn frame_len(header: &[u8]) -> usize {
    u32::from_le_bytes(header[..4].try_into().unwrap()) as usize
}

pub fn upgrade(frame: &[u8], reply: &mut AlignedVec) -> io::Result<()> {
    let (header, archive) = frame
        .split_at_checked(HEADER)
        .ok_or(io::ErrorKind::UnexpectedEof)?;
    if frame_len(header) != archive.len() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let invalid = |err: Error| io::Error::new(io::ErrorKind::InvalidData, err);
    match u32::from_le_bytes(header[4..].try_into().unwrap()) {
        1 => {
            let value = rkyv::access::<Archived<Request<'_>>, Error>(archive).map_err(invalid)?;
            encode_reply(reply, value.id.to_native(), value.payload.as_slice(), 0)
        }
        2 => {
            let value = rkyv::access::<Archived<Reply<'_>>, Error>(archive).map_err(invalid)?;
            encode_reply(
                reply,
                value.id.to_native(),
                value.payload.as_slice(),
                value.flags.to_native(),
            )
        }
        version => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported fixture version: {version}"),
        )),
    }
}

pub fn serve(
    input: &mut impl BufRead,
    output: &mut impl Write,
    max_payload_bytes: usize,
) -> io::Result<()> {
    let mut request = AlignedVec::<16>::new();
    let mut response = AlignedVec::new();
    let mut header = [0; HEADER];
    while let Some(first) = input.bytes().next().transpose()? {
        header[0] = first;
        input.read_exact(&mut header[1..])?;
        let payload_len = frame_len(&header);
        let len = HEADER
            .checked_add(payload_len)
            .filter(|_| payload_len <= max_payload_bytes)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "archive payload exceeds limit")
            })?;
        if request.len() < len {
            request.resize(len, 0);
        }
        request[..HEADER].copy_from_slice(&header);
        input.read_exact(&mut request[HEADER..len])?;
        upgrade(&request[..len], &mut response)?;
        output.write_all(&response)?;
        output.flush()?;
    }
    Ok(())
}
