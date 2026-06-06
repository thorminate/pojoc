use criterion::{criterion_group, criterion_main, Criterion};
use std::time::Duration;
use std::hint::black_box;

use pojoc_tests::{
    pojoc_player::{self, Player, Vector3},
    proto_player,
    player_capnp,
    fb_player,
};

use prost::Message;
use capnp::message::Builder;
use flatbuffers::FlatBufferBuilder;
use pojoc_runtime::{pojvec};

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

fn encode_pojoc(c: &mut Criterion) {
    let player = make_player();

    c.bench_function("pojoc encode", |b| {
        b.iter(|| {
            let mut buf = Vec::new();
            pojoc_player::encode(&mut buf, black_box(&player));
            black_box(buf);
        })
    });
}

fn encode_proto(c: &mut Criterion) {
    let player = proto_player::Player {
        player_id: 1.0,
        level: 12.5,
        inventory: vec![
            "sword".into(),
            "shield".into(),
            "healing_potion".into(),
        ],
        position: Some(proto_player::Vector3 {
            x: 10.0,
            y: 42.5,
            z: -3.0,
            w: 1.0,
        }),
        tags: vec![
            "starter_zone".into(),
            "pvp_enabled".into(),
            "vip".into(),
        ],
        is_nauseous: false,
    };

    c.bench_function("protobuf encode", |b| {
        b.iter(|| {
            black_box(&player).encode_to_vec()
        })
    });
}

fn encode_capnp(c: &mut Criterion) {
    c.bench_function("capnp encode", |b| {
        b.iter(|| {
            let mut message = Builder::new_default();
            let mut p = message.init_root::<player_capnp::player::Builder>();

            p.set_player_id(1.0);
            p.set_level(12.5);
            p.set_is_nauseous(false);

            let mut buf = Vec::new();
            capnp::serialize::write_message(&mut buf, &message).unwrap();

            black_box(buf)
        })
    });
}

fn encode_fb(c: &mut Criterion) {
    c.bench_function("flatbuffers encode", |b| {
        b.iter(|| {
            let mut builder = FlatBufferBuilder::with_capacity(256);

            let inventory = vec![
                builder.create_string("sword"),
                builder.create_string("shield"),
                builder.create_string("healing_potion"),
            ];

            let tags = vec![
                builder.create_string("starter_zone"),
                builder.create_string("pvp_enabled"),
                builder.create_string("vip"),
            ];

            let inv_vec = builder.create_vector(&inventory);
            let tags_vec = builder.create_vector(&tags);

            let position = fb_player::Vector3::new(10.0, 42.5, -3.0, 1.0);

            let player = fb_player::Player::create(&mut builder, &fb_player::PlayerArgs {
                player_id: 1.0,
                level: 12.5,
                inventory: Some(inv_vec),
                position: Some(&position),
                tags: Some(tags_vec),
                is_nauseous: false,
            });

            builder.finish(player, None);

            black_box(builder.finished_data().to_vec())
        })
    });
}

fn config() -> Criterion {
    Criterion::default()
        .sample_size(500)
        .measurement_time(Duration::from_secs(10))
        .warm_up_time(Duration::from_secs(3))
}

criterion_group! {
    name = encode;
    config = config();
    targets =
        encode_pojoc,
        encode_proto,
        encode_capnp,
        encode_fb
}

criterion_main!(encode);