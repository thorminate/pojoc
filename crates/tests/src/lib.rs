pub mod proto_player {
    include!(concat!(env!("OUT_DIR"), "/player.rs"));
}

pub mod player_capnp {
    include!(concat!(env!("OUT_DIR"), "/schemas/player_capnp.rs"));
}

pub mod generated{
    pub mod flatbuf;
    pub mod pojoc;   
    pub mod pojoc_edge;  
}

#[allow(unused_imports)]
pub use generated::flatbuf::player_fb as fb_player;

pub use generated::pojoc as pojoc_player;

pub use generated::pojoc_edge as pojoc_edge;