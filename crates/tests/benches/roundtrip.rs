use std::time::Duration;
use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;

use pojoc_tests::{
    pojoc_player::{self, Player},
    proto_player,
    player_capnp,
    fb_player,
};

use prost::Message;
use capnp::message::{Builder, ReaderOptions};
use flatbuffers::FlatBufferBuilder;
use pojoc_runtime::pojvec;

fn make_player() -> Player {
    Player {
        player_id: 1.0,
        level: 12.5,
        inventory: pojvec!["sword", "shield", "healing_potion"],
        position: pojoc_player::Vector3 {
            x: 10.0, y: 42.5, z: -3.0, w: 1.0
        },
        tags: pojvec!["starter_zone", "pvp_enabled", "vip"],
        is_nauseous: false,
        is_dead: true,
    }
}

fn make_fb_player<'a>(
    builder: &mut FlatBufferBuilder<'a>
) -> flatbuffers::WIPOffset<fb_player::Player<'a>> {
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

    fb_player::Player::create(builder, &fb_player::PlayerArgs {
        player_id: 1.0,
        level: 12.5,
        inventory: Some(inv_vec),
        position: Some(&position),
        tags: Some(tags_vec),
        is_nauseous: false,
    })
}

fn roundtrip_pojoc(c: &mut Criterion) {
    let player = make_player();

    c.bench_function("pojoc roundtrip", |b| {
        b.iter(|| {
            let mut buf = Vec::new();
            pojoc_player::encode(&mut buf, black_box(&player));
            let decoded = pojoc_player::decode(black_box(&buf)).unwrap();
            black_box(decoded);
        })
    });
}

fn roundtrip_proto(c: &mut Criterion) {
    let player = proto_player::Player {
        player_id: 1.0,
        level: 12.5,
        inventory: vec!["sword".into(), "shield".into(), "healing_potion".into()],
        position: Some(proto_player::Vector3 {
            x: 10.0, y: 42.5, z: -3.0, w: 1.0,
        }),
        tags: vec!["starter_zone".into(), "pvp_enabled".into(), "vip".into()],
        is_nauseous: false,
    };

    c.bench_function("protobuf roundtrip", |b| {
        b.iter(|| {
            let buf = black_box(&player).encode_to_vec();
            let decoded =
                proto_player::Player::decode(black_box(buf.as_slice())).unwrap();
            black_box(decoded);
        })
    });
}

fn roundtrip_capnp(c: &mut Criterion) {
    c.bench_function("capnp roundtrip", |b| {
        b.iter(|| {
            let mut msg = Builder::new_default();
            {
                let mut p = msg.init_root::<player_capnp::player::Builder>();
                p.set_player_id(1.0);
                p.set_level(12.5);
                p.set_is_nauseous(false);
            }

            let mut buf = Vec::new();
            capnp::serialize::write_message(&mut buf, &msg).unwrap();

            let reader = capnp::serialize::read_message(
                black_box(buf.as_slice()),
                ReaderOptions::new(),
            ).unwrap();

            let _ = reader.get_root::<player_capnp::player::Reader>().unwrap();

            black_box(reader);
        })
    });
}

fn roundtrip_fb(c: &mut Criterion) {
    c.bench_function("flatbuffers roundtrip", |b| {
        b.iter(|| {
            let mut builder = FlatBufferBuilder::with_capacity(256);

            let player = make_fb_player(&mut builder);

            builder.finish(player, None);

            let buf = builder.finished_data().to_vec();

            let _ = flatbuffers::root::<fb_player::Player>(
                black_box(&buf)
            ).unwrap();

            black_box(buf);
        })
    });
}

fn criterion_config() -> Criterion {
    Criterion::default()
        .sample_size(500)
        .measurement_time(Duration::from_secs(10))
        .warm_up_time(Duration::from_secs(3))
}

criterion_group!(
    name = roundtrip;
    config = criterion_config();
    targets =
        roundtrip_pojoc,
        roundtrip_proto,
        roundtrip_capnp,
        roundtrip_fb
);

criterion_main!(roundtrip);