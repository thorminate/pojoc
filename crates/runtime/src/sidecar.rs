use std::io::{self, BufRead, Write};
use std::process::ExitCode;

const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const USAGE: &str = "Usage: <binary> serve --stdio";

pub trait Codec {
    const JSON: bool = false;

    fn serve(
        input: &mut impl BufRead,
        output: &mut impl Write,
        max_payload_bytes: usize,
    ) -> io::Result<()>;

    fn encode_json(
        _input: &mut impl BufRead,
        _output: &mut impl Write,
        _max_payload_bytes: usize,
    ) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn decode_json(
        _input: &mut impl BufRead,
        _output: &mut impl Write,
        _max_payload_bytes: usize,
    ) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

enum Mode {
    RoundTrip,
    Encode,
    Decode,
}

impl Mode {
    fn run<C: Codec>(self, input: &mut impl BufRead, output: &mut impl Write) -> io::Result<()> {
        match self {
            Self::RoundTrip => C::serve(input, output, MAX_PAYLOAD_BYTES),
            Self::Encode => C::encode_json(input, output, MAX_PAYLOAD_BYTES),
            Self::Decode => C::decode_json(input, output, MAX_PAYLOAD_BYTES),
        }
    }
}

fn serve_stdio<C: Codec>(mode: Mode) -> io::Result<()> {
    let mut input = io::stdin().lock();
    let output = io::stdout();

    #[cfg(any(unix, windows))]
    {
        use std::io::IsTerminal;
        #[cfg(unix)]
        use std::os::fd::AsFd;
        #[cfg(windows)]
        use std::os::windows::io::AsHandle;

        // Avoid splitting a binary reply on embedded newlines.
        if !output.is_terminal() {
            #[cfg(unix)]
            let handle = output.as_fd().try_clone_to_owned();
            #[cfg(windows)]
            let handle = output.as_handle().try_clone_to_owned();
            if let Ok(handle) = handle {
                let mut output = std::fs::File::from(handle);
                return mode.run::<C>(&mut input, &mut output);
            }
        }
    }

    mode.run::<C>(&mut input, &mut output.lock())
}

pub fn main<C: Codec>() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let suffix = if C::JSON { " [encode|decode]" } else { "" };
    let mode = match args.as_slice() {
        [help] if help == "--help" || help == "-h" => {
            println!("{USAGE}{suffix}");
            return ExitCode::SUCCESS;
        }
        [serve, stdio, options @ ..] if serve == "serve" && stdio == "--stdio" => match options {
            [] => Some(Mode::RoundTrip),
            [mode] if C::JSON && mode == "encode" => Some(Mode::Encode),
            [mode] if C::JSON && mode == "decode" => Some(Mode::Decode),
            _ => None,
        },
        _ => None,
    };
    let Some(mode) = mode else {
        eprintln!("{USAGE}{suffix}");
        return ExitCode::FAILURE;
    };
    if let Err(err) = serve_stdio::<C>(mode) {
        eprintln!("{err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[cfg(feature = "sidecar-json")]
pub mod json {
    use crate::{Envelope, PojocResult, stream::codec_error};
    use serde::{Deserialize, Serialize};
    use serde_json::Value;
    use std::io::{self, BufRead, Read, Write};

    const MAX_JSON_BYTES: usize = 64 * 1024 * 1024;

    fn invalid_data(err: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, err)
    }

    pub fn from_value<'de, T: Deserialize<'de>>(value: &'de Value) -> io::Result<T> {
        T::deserialize(value).map_err(invalid_data)
    }

    pub fn encode(
        input: &mut impl BufRead,
        output: &mut impl Write,
        max_payload_bytes: usize,
        open_envelope: for<'a> fn(&'a [u8], &mut usize) -> PojocResult<Envelope<'a>>,
        mut transcode: impl FnMut(&Value, &mut Vec<u8>) -> io::Result<()>,
    ) -> io::Result<()> {
        let mut record = Vec::new();
        let mut envelope = Vec::new();
        loop {
            record.clear();
            let mut limited = (&mut *input).take((MAX_JSON_BYTES + 1) as u64);
            if limited.read_until(b'\n', &mut record)? == 0 {
                return Ok(());
            }
            let line = record.strip_suffix(b"\n").unwrap_or(&record);
            if line.len() > MAX_JSON_BYTES {
                return Err(invalid_data("JSON record exceeds limit"));
            }
            // Borrow strings from a parsed record, including strings with JSON escapes.
            let value = serde_json::from_slice(line).map_err(invalid_data)?;
            envelope.clear();
            transcode(&value, &mut envelope)?;
            let frame = open_envelope(&envelope, &mut 0).map_err(codec_error)?;
            if frame.payload.len() > max_payload_bytes {
                return Err(invalid_data("envelope payload exceeds limit"));
            }
            output.write_all(&envelope)?;
            output.flush()?;
        }
    }

    pub struct Lines<'a, W> {
        output: &'a mut W,
        record: Vec<u8>,
    }

    impl<'a, W: Write> Lines<'a, W> {
        pub fn new(output: &'a mut W) -> Self {
            Self {
                output,
                record: Vec::new(),
            }
        }

        pub fn write(&mut self, value: &impl Serialize) -> io::Result<()> {
            self.record.clear();
            serde_json::to_writer(&mut self.record, value).map_err(invalid_data)?;
            self.record.push(b'\n');
            self.output.write_all(&self.record)?;
            self.output.flush()
        }
    }
}

#[doc(hidden)]
#[macro_export]
macro_rules! stdio_sidecar {
    ($codec:ident $(, $mode:ident)?) => {
        fn main() -> ::std::process::ExitCode {
            struct Codec;

            impl $crate::sidecar::Codec for Codec {
                fn serve(
                    input: &mut impl ::std::io::BufRead,
                    output: &mut impl ::std::io::Write,
                    max_payload_bytes: usize,
                ) -> ::std::io::Result<()> {
                    $codec::serve(input, output, max_payload_bytes)
                }

                $($crate::stdio_sidecar!(@mode $codec, $mode);)?
            }

            $crate::sidecar::main::<Codec>()
        }
    };
    (@mode $codec:ident, json) => {
        const JSON: bool = true;

        fn encode_json(
            input: &mut impl ::std::io::BufRead,
            output: &mut impl ::std::io::Write,
            max_payload_bytes: usize,
        ) -> ::std::io::Result<()> {
            let latest = *$codec::supported_versions().last().unwrap();
            $crate::sidecar::json::encode(input, output, max_payload_bytes, $codec::open_envelope, |json, envelope| {
                let value = $crate::sidecar::json::from_value(json)?;
                $codec::encode_for_version(envelope, &value, latest)
                    .map_err($crate::stream::codec_error)
            })
        }

        fn decode_json(
            input: &mut impl ::std::io::BufRead,
            output: &mut impl ::std::io::Write,
            max_payload_bytes: usize,
        ) -> ::std::io::Result<()> {
            let mut lines = $crate::sidecar::json::Lines::new(output);
            $codec::serve_decode(input, max_payload_bytes, |value| lines.write(&value))
        }
    };
}
