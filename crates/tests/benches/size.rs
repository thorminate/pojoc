use pojoc_tests::{
    pojoc_player::{self, Player, Vector3, pojvec},
    proto_player,
    player_capnp,
    fb_player,
};

use prost::Message;
use capnp::message::Builder;
use flatbuffers::FlatBufferBuilder;

fn make_player() -> Player {
    Player {
        player_id: 1.0,
        level: 12.5,
        inventory: pojvec![
            "sword",
            "shield",
            "healing_potion",
        ],
        position: Vector3 {
            x: 10.0,
            y: 42.5,
            z: -3.0,
            w: 1.0,
        },
        tags: pojvec![
            "starter_zone",
            "pvp_enabled",
            "vip",
        ],
        is_nauseous: false,
        is_dead: true,
    }
}

fn main() {
    let player = make_player();

    // POJOC
    let mut pojoc_buf = Vec::new();
    pojoc_player::encode(&mut pojoc_buf, &player);

    // Protobuf
    let proto_buf = proto_player::Player {
        player_id: player.player_id,
        level: player.level,
        inventory: player.inventory.iter().map(|s| s.to_string()).collect(),
        
        position: Some(proto_player::Vector3 {
            x: player.position.x,
            y: player.position.y,
            z: player.position.z,
            w: player.position.w,
        }),
        tags: player.tags.iter().map(|s| s.to_string()).collect(),
        is_nauseous: player.is_nauseous,
    }
        .encode_to_vec();

    // Cap'n Proto
    let mut msg = Builder::new_default();

    let mut p = msg.init_root::<player_capnp::player::Builder>();

    p.set_player_id(player.player_id);
    p.set_level(player.level);
    p.set_is_nauseous(player.is_nauseous);

    // inventory
    {
        let mut inv = p.reborrow().init_inventory(player.inventory.len() as u32);
        for (i, v) in player.inventory.iter().enumerate() {
            inv.set(i as u32, v);
        }
    }

    // tags
    {
        let mut tags = p.reborrow().init_tags(player.tags.len() as u32);
        for (i, v) in player.tags.iter().enumerate() {
            tags.set(i as u32, v);
        }
    }

    // position
    {
        let mut pos = p.reborrow().init_position();
        pos.set_x(player.position.x);
        pos.set_y(player.position.y);
        pos.set_z(player.position.z);
        pos.set_w(player.position.w);
    }
    
    let mut capnp_buf = Vec::new();
    capnp::serialize::write_message(&mut capnp_buf, &msg).unwrap();

    // FlatBuffers
    let mut builder = FlatBufferBuilder::with_capacity(256);

    let inv = player
        .inventory
        .iter()
        .map(|s| builder.create_string(s))
        .collect::<Vec<_>>();

    let tags = player
        .tags
        .iter()
        .map(|s| builder.create_string(s))
        .collect::<Vec<_>>();

    let inv_vec = builder.create_vector(&inv);
    let tags_vec = builder.create_vector(&tags);

    let position = fb_player::Vector3::new(
        player.position.x,
        player.position.y,
        player.position.z,
        player.position.w,
    );

    let fb_player = fb_player::Player::create(&mut builder, &fb_player::PlayerArgs {
        player_id: player.player_id,
        level: player.level,
        inventory: Some(inv_vec),
        position: Some(&position),
        tags: Some(tags_vec),
        is_nauseous: player.is_nauseous,
    });

    builder.finish(fb_player, None);
    let fb_buf = builder.finished_data();

    // 📊 PRINT RESULTS
    println!("\n=== binary size comparison ===");
    println!("POJOC:        {} bytes", pojoc_buf.len());
    println!("Protobuf:     {} bytes", proto_buf.len());
    println!("Cap'n Proto:  {} bytes", capnp_buf.len());
    println!("FlatBuffers:  {} bytes", fb_buf.len());
    println!("==============================\n");
}