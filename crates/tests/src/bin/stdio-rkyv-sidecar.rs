use pojoc_tests::stdio_rkyv as codec;

struct Mode;

impl Mode {
    const USAGE_SUFFIX: &str = "";

    fn parse(args: &[std::ffi::OsString]) -> Option<Self> {
        args.is_empty().then_some(Self)
    }

    fn run(self, input: &mut impl io::BufRead, output: &mut impl io::Write) -> io::Result<()> {
        codec::serve(input, output, MAX_PAYLOAD_BYTES)
    }
}

include!("../../../cli/src/sidecar.rs.in");
