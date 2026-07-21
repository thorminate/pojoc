macro_rules! generated {
    ($vis:vis $name:ident, $path:literal) => {
        $vis mod $name {
            #![allow(warnings, clippy::all)]
            include!(concat!(env!("OUT_DIR"), $path));
        }
    };
}

macro_rules! generated_strict {
    ($vis:vis $name:ident, $path:literal) => {
        $vis mod $name {
            include!(concat!(env!("OUT_DIR"), $path));
        }
    };
}

generated!(pub proto_player,  "/proto_player.rs");
generated!(pub player_capnp,  "/schemas/player_capnp.rs");
generated!(flatbuf, "/flatbuf.rs");
generated!(pub player_bebop,  "/bebop-schema/player.rs");
generated_strict!(pub pojoc_player, "/pojoc_player.rs");
generated_strict!(pub pojoc_edge,   "/pojoc_edge.rs");
generated_strict!(pub pojoc_recursive, "/pojoc_recursive.rs");
generated_strict!(pub pojoc_constraints, "/pojoc_constraints.rs");
generated_strict!(pub pojoc_interning, "/pojoc_interning.rs");

pub use flatbuf::fb_player;

generated!(pub proto_intern_bench, "/proto_intern_bench.rs");
generated!(pub intern_bench_capnp, "/schemas/intern_bench_capnp.rs");
generated!(flatbuf_intern_bench, "/flatbuf_intern_bench.rs");
generated!(pub intern_bench_bebop, "/bebop-schema/intern_bench.rs");
generated_strict!(pub pojoc_intern_bench, "/pojoc_intern_bench.rs");

pub use flatbuf_intern_bench::fb_intern_bench;
