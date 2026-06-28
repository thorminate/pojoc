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

pub use flatbuf::fb_player;
