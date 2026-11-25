use super::{codec_error, read_stream_varint};
use crate::{
    Error, PojocResult, patch_envelope_length, read_array_len, write_array_len,
    write_envelope_header,
};
use std::io::{self, BufRead, Read};

pub struct ByteArrayStream {
    prefix: Vec<u8>,
    header: Vec<u8>,
    length_pos: usize,
    preserve_array_length: bool,
}

impl ByteArrayStream {
    pub fn new(reply_version: u64, preserve_array_length: bool) -> Self {
        let mut header = Vec::new();
        let length_pos = write_envelope_header(&mut header, reply_version);
        Self {
            prefix: Vec::new(),
            header,
            length_pos,
            preserve_array_length,
        }
    }

    fn finish_frame(frame: &mut io::Take<&mut impl BufRead>) -> io::Result<()> {
        if frame.limit() != 0 {
            io::copy(frame, &mut io::sink())?;
            if frame.limit() != 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
        }
        Ok(())
    }

    fn reject_frame(frame: &mut io::Take<&mut impl BufRead>, err: Error) -> io::Result<usize> {
        // A short transport read takes precedence over a codec error.
        Self::finish_frame(frame)?;
        Err(codec_error(err))
    }

    pub fn read_reply(
        &mut self,
        input: &mut impl BufRead,
        payload_len: usize,
        prefix_len: usize,
        response: &mut Vec<u8>,
        encode_prefix: impl FnOnce(&[u8], &mut Vec<u8>) -> PojocResult<()>,
    ) -> io::Result<usize> {
        let mut frame = input.take(payload_len as u64);
        self.prefix.resize(prefix_len + 1, 0);
        frame.read_exact(&mut self.prefix[..prefix_len])?;

        // Use the normal decoder and latest encoder with a synthetic empty array.
        // This preserves upgrades and wire ordering for all the fields before it.
        self.prefix[prefix_len] = 0;
        let length_pos = self.length_pos;
        let payload_start = length_pos + 4;
        self.header.truncate(payload_start);
        if let Err(err) = encode_prefix(&self.prefix, &mut self.header) {
            return Self::reject_frame(&mut frame, err);
        }
        let empty_array_len = self.header.pop();
        debug_assert_eq!(empty_array_len, Some(0));

        let mut first = [0];
        frame.read_exact(&mut first)?;
        let (array_length, length_len) = match read_stream_varint::<10>(&mut frame, first[0]) {
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof && frame.limit() == 0 => {
                return Err(codec_error(Error::UnexpectedEof));
            }
            result => result?,
        };
        let array_length = &array_length[..length_len];
        let bytes = match read_array_len(array_length, &mut 0) {
            Ok(bytes) => bytes,
            Err(err) => return Self::reject_frame(&mut frame, err),
        };
        if bytes as u64 > frame.limit() {
            return Self::reject_frame(&mut frame, Error::UnexpectedEof);
        }

        // Lazy fields forward the length varint exactly as it arrived.
        if self.preserve_array_length {
            self.header.extend_from_slice(array_length);
        } else {
            write_array_len(&mut self.header, bytes);
        }
        let Some(payload_len) = (self.header.len() - payload_start)
            .checked_add(bytes)
            .filter(|&len| u32::try_from(len).is_ok())
        else {
            return Self::reject_frame(&mut frame, Error::InvalidLength);
        };
        patch_envelope_length(&mut self.header, length_pos, payload_len);
        let header_len = self.header.len();
        let Some(reply_len) = header_len.checked_add(bytes) else {
            return Self::reject_frame(&mut frame, Error::InvalidLength);
        };

        // Retain initialized storage across frames so reading a blob does not zero it first.
        if response.len() < reply_len {
            response.resize(reply_len, 0);
        }
        response[..header_len].copy_from_slice(&self.header);
        frame.read_exact(&mut response[header_len..reply_len])?;
        Self::finish_frame(&mut frame)?;
        Ok(reply_len)
    }
}
