pub mod codec {
    include!(concat!(env!("OUT_DIR"), "/pojoc_stdio_bench.rs"));
}

#[cfg(feature = "sidecar-json")]
pojoc::stdio_sidecar!(codec, json);
#[cfg(not(feature = "sidecar-json"))]
pojoc::stdio_sidecar!(codec);
