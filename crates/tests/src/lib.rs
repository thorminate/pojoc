pub mod proto_player {
    #![allow(clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/proto_player.rs"));
}

pub mod player_capnp {
    #![allow(clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/schemas/player_capnp.rs"));
}

pub mod flatbuf {
    #![allow(clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/flatbuf.rs"));
}

pub mod pojoc_player {
    include!(concat!(env!("OUT_DIR"), "/pojoc_player.rs"));
}

pub mod pojoc_edge {
    include!(concat!(env!("OUT_DIR"), "/pojoc_edge.rs"));
}

pub use flatbuf::fb_player;
