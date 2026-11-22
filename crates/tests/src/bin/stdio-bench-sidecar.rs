#[allow(dead_code)]
mod codec {
    include!(concat!(env!("OUT_DIR"), "/pojoc_stdio_bench.rs"));
}

include!("../../../cli/src/sidecar.rs.in");
