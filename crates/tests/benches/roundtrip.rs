use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::time::Duration;

use pojoc_tests::{
    fb_player, player_capnp as capnp_player,
    pojoc_player::{self, AABB, Class, Player, Status, Transform, Vector3, runtime::*},
    proto_player,
};

use flatbuffers::FlatBufferBuilder;
use pojoc_tests::fb_player::StatsArgs;
use pojoc_tests::pojoc_player::{Flags, Perks, Region, Stats};
use prost::Message;

fn make_pojoc_player() -> Player {
    Player {
        player_id: 1.0,
        level: 12.5,
        status: Status::Alive,
        class: Class::Warrior,
        region: Region::Central,

        stats: Stats {
            strength: 14,
            agility: 8,
            intelligence: 11,
            endurance: 10,
            charisma: 6,
            resistance: 0.25,
        },

        inventory: pojvec!["sword", "shield", "healing_potion", "torch", "rope"],

        hotbar: pojvec![
            "sword",
            "healing_potion",
            "torch",
            "",
            "",
            "";
            6
        ],

        callsign: pojstr!("NONE00"),
        session_token: pojstr!("SESSION000000000", 16),
        guild_tag: pojstr!("IRON", 4),
        status_code: pojstr!("00000000", 8),

        coordinates: (128.5, 64.0),
        kill_death: (42, 7),
        velocity: (1.0, 0.0, -1.0),
        spawn_point: (0.0, 0.0, 0.0),
        last_position: (10.0, 42.5, -3.0),

        position: Vector3 {
            x: 10.0,
            y: 42.5,
            z: -3.0,
            w: 1.0,
        },

        transform: Transform {
            position: Vector3 {
                x: 10.0,
                y: 42.5,
                z: -3.0,
                w: 1.0,
            },
            bounds: AABB {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 100.0,
                max_y: 100.0,
            },
        },

        tags: pojvec![
            "starter_zone",
            "pvp_enabled",
            "vip",
            "quest_giver",
            "faction_red"
        ],

        recent_zones: pojvec![
            "zone_forest",
            "zone_dungeon",
            "zone_town",
            "",
            "",
            "",
            "",
            "";
            8
        ],

        chat_history: pojvec![
            "hello world",
            "anyone here?",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "";
            32
        ],

        achievement_ids: pojvec![101u32, 204, 305, 412],

        party_members: pojvec![u32 => 2, 3, 0, 0; 4],

        is_nauseous: false,

        active_perks: Perks::TELEKINESIS | Perks::THIEVERY,

        account_flags: Flags::IS_VERIFIED | Flags::IS_PREMIUM,

        quest_progress: pojmap!(
            "main_quest_01" => 3,
            "side_quest_forest" => 1,
            "daily_kill_10" => 7,
        ),

        quick_slots: pojmap!(
            1 => "sword",
            2 => "shield",
            3 => "healing_potion",
            4 => "torch",
            5 => "",
            6 => "",
            7 => "",
            8 => "",
            9 => "",
            10 => "";
            10
        ),

        skill_levels: pojmap!(
            "swordsmanship" => 4.5f32,
            "stealth" => 2.0f32,
            "arcana" => 1.5f32,
        ),

        leaderboard_scores: pojmap!(
            "kills" => 1042i64,
            "score" => 88500i64,
            "playtime_seconds" => 72400i64,
        ),

        loadout: pojvec!(
            pojtup!("sword", 1),
            pojtup!("shield", 1),
            pojtup!("healing_potion", 5),
            pojtup!("torch", 3);
            4
        ),
    }
}

fn make_proto_player() -> proto_player::Player {
    proto_player::Player {
        player_id: 1.0,
        level: 12.5,
        status: proto_player::Status::Alive as i32,
        class: proto_player::Class::Warrior as i32,
        region: proto_player::Region::Central as i32,

        stats: Some(proto_player::Stats {
            strength: 14,
            agility: 8,
            intelligence: 11,
            endurance: 10,
            charisma: 6,
            resistance: 0.25,
        }),

        inventory: vec![
            "sword".into(),
            "shield".into(),
            "healing_potion".into(),
            "torch".into(),
            "rope".into(),
        ],

        hotbar: vec![
            "sword".into(),
            "healing_potion".into(),
            "torch".into(),
            "".into(),
            "".into(),
            "".into(),
        ],

        callsign: "NONE00".into(),
        session_token: "SESSION000000000".into(),
        guild_tag: "IRON".into(),
        status_code: "00000000".into(),

        coordinates: Some(proto_player::Coordinates { x: 128.5, y: 64.0 }),
        kill_death: Some(proto_player::KillDeath {
            kills: 42,
            deaths: 7,
        }),
        velocity: Some(proto_player::Velocity {
            x: 1.0,
            y: 0.0,
            z: -1.0,
        }),
        spawn_point: Some(proto_player::Point3D {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
        last_position: Some(proto_player::Point3D {
            x: 10.0,
            y: 42.5,
            z: -3.0,
        }),

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
            "quest_giver".into(),
            "faction_red".into(),
        ],

        recent_zones: vec![
            "zone_forest".into(),
            "zone_dungeon".into(),
            "zone_town".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
        ],

        chat_history: {
            let mut ch = vec!["hello world".into(), "anyone here?".into()];
            ch.resize(32, "".into());
            ch
        },

        transform: Some(proto_player::Transform {
            position: Some(proto_player::Vector3 {
                x: 10.0,
                y: 42.5,
                z: -3.0,
                w: 1.0,
            }),
            bounds: Some(proto_player::Aabb {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 100.0,
                max_y: 100.0,
            }),
        }),

        achievement_ids: vec![101, 204, 305, 412],
        party_members: vec![2, 3, 0, 0],

        is_nauseous: false,

        active_perks: 0x01 | 0x02,
        account_flags: 0x01 | 0x02,

        quest_progress: [
            ("main_quest_01".to_string(), 3),
            ("side_quest_forest".to_string(), 1),
            ("daily_kill_10".to_string(), 7),
        ]
        .into_iter()
        .collect(),

        quick_slots: [
            (1, "sword".to_string()),
            (2, "shield".to_string()),
            (3, "healing_potion".to_string()),
            (4, "torch".to_string()),
            (5, "".to_string()),
            (6, "".to_string()),
            (7, "".to_string()),
            (8, "".to_string()),
            (9, "".to_string()),
            (10, "".to_string()),
        ]
        .into_iter()
        .collect(),

        skill_levels: [
            ("swordsmanship".to_string(), 4.5f32),
            ("stealth".to_string(), 2.0f32),
            ("arcana".to_string(), 1.5f32),
        ]
        .into_iter()
        .collect(),

        leaderboard_scores: [
            ("kills".to_string(), 1042i64),
            ("score".to_string(), 88500i64),
            ("playtime_seconds".to_string(), 72400i64),
        ]
        .into_iter()
        .collect(),

        loadout: vec![
            proto_player::LoadoutEntry {
                item: "sword".into(),
                quantity: 1,
            },
            proto_player::LoadoutEntry {
                item: "shield".into(),
                quantity: 1,
            },
            proto_player::LoadoutEntry {
                item: "healing_potion".into(),
                quantity: 5,
            },
            proto_player::LoadoutEntry {
                item: "torch".into(),
                quantity: 3,
            },
        ],
    }
}

fn make_capnp_message() -> capnp::message::Builder<capnp::message::HeapAllocator> {
    let mut msg = capnp::message::Builder::new_default();

    {
        let mut p = msg.init_root::<capnp_player::player::Builder>();

        p.set_player_id(1.0);
        p.set_level(12.5);
        p.set_status(capnp_player::Status::Alive);
        p.set_class(capnp_player::Class::Warrior);
        p.set_region(capnp_player::Region::Central);
        p.set_is_nauseous(false);
        p.set_callsign("NONE00");
        p.set_session_token("SESSION000000000");
        p.set_guild_tag("IRON");
        p.set_status_code("00000000");
        p.set_active_perks(0x01 | 0x02);
        p.set_account_flags(0x01 | 0x02);

        {
            let mut stats = p.reborrow().init_stats();
            stats.set_strength(14);
            stats.set_agility(8);
            stats.set_intelligence(11);
            stats.set_endurance(10);
            stats.set_charisma(6);
            stats.set_resistance(0.25);
        }

        {
            let mut inv = p.reborrow().init_inventory(5);
            inv.set(0, "sword");
            inv.set(1, "shield");
            inv.set(2, "healing_potion");
            inv.set(3, "torch");
            inv.set(4, "rope");
        }

        {
            let mut hb = p.reborrow().init_hotbar(6);
            hb.set(0, "sword");
            hb.set(1, "healing_potion");
            hb.set(2, "torch");
            hb.set(3, "");
            hb.set(4, "");
            hb.set(5, "");
        }

        {
            let mut coords = p.reborrow().init_coordinates();
            coords.set_x(128.5);
            coords.set_y(64.0);
        }

        {
            let mut kd = p.reborrow().init_kill_death();
            kd.set_kills(42);
            kd.set_deaths(7);
        }

        {
            let mut pos = p.reborrow().init_position();
            pos.set_x(10.0);
            pos.set_y(42.5);
            pos.set_z(-3.0);
            pos.set_w(1.0);
        }

        {
            let mut tags = p.reborrow().init_tags(5);
            tags.set(0, "starter_zone");
            tags.set(1, "pvp_enabled");
            tags.set(2, "vip");
            tags.set(3, "quest_giver");
            tags.set(4, "faction_red");
        }

        {
            let mut rz = p.reborrow().init_recent_zones(8);
            rz.set(0, "zone_forest");
            rz.set(1, "zone_dungeon");
            rz.set(2, "zone_town");
            rz.set(3, "");
            rz.set(4, "");
            rz.set(5, "");
            rz.set(6, "");
            rz.set(7, "");
        }

        {
            let mut ch = p.reborrow().init_chat_history(32);
            ch.set(0, "hello world");
            ch.set(1, "anyone here?");
            for i in 2..32 {
                ch.set(i, "");
            }
        }

        {
            let mut aid = p.reborrow().init_achievement_ids(4);
            aid.set(0, 101);
            aid.set(1, 204);
            aid.set(2, 305);
            aid.set(3, 412);
        }

        {
            let mut pm = p.reborrow().init_party_members(4);
            pm.set(0, 2);
            pm.set(1, 3);
            pm.set(2, 0);
            pm.set(3, 0);
        }

        {
            let mut t = p.reborrow().init_transform();
            {
                let mut pos = t.reborrow().init_position();
                pos.set_x(10.0);
                pos.set_y(42.5);
                pos.set_z(-3.0);
                pos.set_w(1.0);
            }
            {
                let mut b = t.reborrow().init_bounds();
                b.set_min_x(0.0);
                b.set_min_y(0.0);
                b.set_max_x(100.0);
                b.set_max_y(100.0);
            }
        }

        {
            let mut v = p.reborrow().init_velocity();
            v.set_x(1.0);
            v.set_y(0.0);
            v.set_z(-1.0);
        }

        {
            let mut sp = p.reborrow().init_spawn_point();
            sp.set_x(0.0);
            sp.set_y(0.0);
            sp.set_z(0.0);
        }

        {
            let mut lp = p.reborrow().init_last_position();
            lp.set_x(10.0);
            lp.set_y(42.5);
            lp.set_z(-3.0);
        }

        {
            let mut qp = p.reborrow().init_quest_progress(3);
            let entries = [
                ("main_quest_01", 3),
                ("side_quest_forest", 1),
                ("daily_kill_10", 7),
            ];
            for (i, &(k, v)) in entries.iter().enumerate() {
                let mut entry = qp.reborrow().get(i as u32);
                entry.set_key(k);
                entry.set_value(v);
            }
        }

        {
            let mut qs = p.reborrow().init_quick_slots(10);
            let slots = [
                (1, "sword"),
                (2, "shield"),
                (3, "healing_potion"),
                (4, "torch"),
                (5, ""),
                (6, ""),
                (7, ""),
                (8, ""),
                (9, ""),
                (10, ""),
            ];
            for (i, &(k, v)) in slots.iter().enumerate() {
                let mut entry = qs.reborrow().get(i as u32);
                entry.set_key(k);
                entry.set_value(v);
            }
        }

        {
            let mut sl = p.reborrow().init_skill_levels(3);
            let skills = [("swordsmanship", 4.5), ("stealth", 2.0), ("arcana", 1.5)];
            for (i, &(k, v)) in skills.iter().enumerate() {
                let mut entry = sl.reborrow().get(i as u32);
                entry.set_key(k);
                entry.set_value(v);
            }
        }

        {
            let mut ls = p.reborrow().init_leaderboard_scores(3);
            let scores = [
                ("kills", 1042),
                ("score", 88500),
                ("playtime_seconds", 72400),
            ];
            for (i, &(k, v)) in scores.iter().enumerate() {
                let mut entry = ls.reborrow().get(i as u32);
                entry.set_key(k);
                entry.set_value(v);
            }
        }

        {
            let mut lo = p.reborrow().init_loadout(4);
            let loadouts = [
                ("sword", 1),
                ("shield", 1),
                ("healing_potion", 5),
                ("torch", 3),
            ];
            for (i, &(item, qty)) in loadouts.iter().enumerate() {
                let mut entry = lo.reborrow().get(i as u32);
                entry.set_item(item);
                entry.set_quantity(qty);
            }
        }
    }
    msg
}

fn make_fb_player(builder: &mut FlatBufferBuilder) {
    let inv_vec = {
        let items = vec![
            builder.create_string("sword"),
            builder.create_string("shield"),
            builder.create_string("healing_potion"),
            builder.create_string("torch"),
            builder.create_string("rope"),
        ];
        builder.create_vector(&items)
    };

    let hotbar_vec = {
        let mut items = vec![
            builder.create_string("sword"),
            builder.create_string("healing_potion"),
            builder.create_string("torch"),
        ];
        for _ in 3..6 {
            items.push(builder.create_string(""));
        }
        builder.create_vector(&items)
    };

    let tags_vec = {
        let items = vec![
            builder.create_string("starter_zone"),
            builder.create_string("pvp_enabled"),
            builder.create_string("vip"),
            builder.create_string("quest_giver"),
            builder.create_string("faction_red"),
        ];
        builder.create_vector(&items)
    };

    let recent_zones_vec = {
        let mut items = vec![
            builder.create_string("zone_forest"),
            builder.create_string("zone_dungeon"),
            builder.create_string("zone_town"),
        ];
        for _ in 3..8 {
            items.push(builder.create_string(""));
        }
        builder.create_vector(&items)
    };

    let chat_history_vec = {
        let mut items = vec![
            builder.create_string("hello world"),
            builder.create_string("anyone here?"),
        ];
        for _ in 2..32 {
            items.push(builder.create_string(""));
        }
        builder.create_vector(&items)
    };

    let quest_progress_vec = {
        let mut entries = Vec::new();
        for &(k, v) in &[
            ("main_quest_01", 3),
            ("side_quest_forest", 1),
            ("daily_kill_10", 7),
        ] {
            let key = builder.create_string(k);
            entries.push(fb_player::QuestProgressEntry::create(
                builder,
                &fb_player::QuestProgressEntryArgs {
                    key: Some(key),
                    value: v,
                },
            ));
        }
        builder.create_vector(&entries)
    };

    let quick_slots_vec = {
        let mut entries = Vec::new();
        let slots = [
            (1, "sword"),
            (2, "shield"),
            (3, "healing_potion"),
            (4, "torch"),
            (5, ""),
            (6, ""),
            (7, ""),
            (8, ""),
            (9, ""),
            (10, ""),
        ];
        for &(k, v) in &slots {
            let val = builder.create_string(v);
            // Changed QuestProgressEntryArgs to QuickSlotsEntryArgs
            entries.push(fb_player::QuickSlotsEntry::create(
                builder,
                &fb_player::QuickSlotsEntryArgs {
                    key: k,
                    value: Some(val),
                },
            ));
        }
        builder.create_vector(&entries)
    };

    let skill_levels_vec = {
        let mut entries = Vec::new();
        for &(k, v) in &[
            ("swordsmanship", 4.5f32),
            ("stealth", 2.0f32),
            ("arcana", 1.5f32),
        ] {
            let key = builder.create_string(k);
            entries.push(fb_player::SkillLevelsEntry::create(
                builder,
                &fb_player::SkillLevelsEntryArgs {
                    key: Some(key),
                    value: v,
                },
            ));
        }
        builder.create_vector(&entries)
    };

    let leaderboard_scores_vec = {
        let mut entries = Vec::new();
        for &(k, v) in &[
            ("kills", 1042i64),
            ("score", 88500i64),
            ("playtime_seconds", 72400i64),
        ] {
            let key = builder.create_string(k);
            entries.push(fb_player::LeaderboardScoresEntry::create(
                builder,
                &fb_player::LeaderboardScoresEntryArgs {
                    key: Some(key),
                    value: v,
                },
            ));
        }
        builder.create_vector(&entries)
    };

    let loadout_vec = {
        let mut entries = Vec::new();
        for &(k, v) in &[
            ("sword", 1i32),
            ("shield", 1i32),
            ("healing_potion", 5i32),
            ("torch", 3i32),
        ] {
            let item = builder.create_string(k);
            entries.push(fb_player::LoadoutEntry::create(
                builder,
                &fb_player::LoadoutEntryArgs {
                    item: Some(item),
                    quantity: v,
                },
            ));
        }
        builder.create_vector(&entries)
    };

    let achievement_ids_vec = builder.create_vector(&[101u32, 204, 305, 412]);
    let party_members_vec = builder.create_vector(&[2u32, 3, 0, 0]);

    let callsign = builder.create_string("NONE00");
    let session_token = builder.create_string("SESSION000000000");
    let guild_tag = builder.create_string("IRON");
    let status_code = builder.create_string("00000000");

    let stats = fb_player::Stats::create(
        builder,
        &StatsArgs {
            agility: 32,
            strength: 14,
            intelligence: 11,
            endurance: 10,
            charisma: 6,
            resistance: 0.25,
        },
    );
    let coordinates = fb_player::Coordinates::new(128.5, 64.0);
    let kill_death = fb_player::KillDeath::new(42, 7);
    let position = fb_player::Vector3::new(10.0, 42.5, -3.0, 1.0);
    let bounds = fb_player::AABB::new(0.0, 0.0, 100.0, 100.0);
    let transform = fb_player::Transform::new(&position, &bounds);
    let velocity = fb_player::Velocity::new(1.0, 0.0, -1.0);
    let spawn_point = fb_player::Point3D::new(0.0, 0.0, 0.0);
    let last_position = fb_player::Point3D::new(10.0, 42.5, -3.0);

    let player = fb_player::Player::create(
        builder,
        &fb_player::PlayerArgs {
            player_id: 1.0,
            level: 12.5,
            status: fb_player::Status::Alive,
            class: fb_player::Class::Warrior,
            region: fb_player::Region::Central,
            stats: Some(stats),
            inventory: Some(inv_vec),
            hotbar: Some(hotbar_vec),
            callsign: Some(callsign),
            session_token: Some(session_token),
            guild_tag: Some(guild_tag),
            status_code: Some(status_code),
            coordinates: Some(&coordinates),
            kill_death: Some(&kill_death),
            velocity: Some(&velocity),
            spawn_point: Some(&spawn_point),
            last_position: Some(&last_position),
            position: Some(&position),
            tags: Some(tags_vec),
            recent_zones: Some(recent_zones_vec),
            chat_history: Some(chat_history_vec),
            transform: Some(&transform),
            achievement_ids: Some(achievement_ids_vec),
            party_members: Some(party_members_vec),
            is_nauseous: false,
            active_perks: 0x01 | 0x02,
            account_flags: 0x01 | 0x02,
            quest_progress: Some(quest_progress_vec),
            quick_slots: Some(quick_slots_vec),
            skill_levels: Some(skill_levels_vec),
            leaderboard_scores: Some(leaderboard_scores_vec),
            loadout: Some(loadout_vec),
        },
    );
    builder.finish(player, None);
}

fn roundtrip_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("roundtrip");

    group
        .sample_size(500)
        .measurement_time(Duration::from_secs(10))
        .warm_up_time(Duration::from_secs(3));

    group.bench_function("pojoc", |b| {
        b.iter(|| {
            let pojoc_player = make_pojoc_player();
            let mut buf = Vec::new();
            pojoc_player::encode(&mut buf, black_box(&pojoc_player));
            let decoded = pojoc_player::decode(black_box(&buf)).unwrap();
            black_box(decoded);
        })
    });

    group.bench_function("protobuf", |b| {
        b.iter(|| {
            let proto_player = make_proto_player();
            let buf = black_box(&proto_player).encode_to_vec();
            let decoded = proto_player::Player::decode(black_box(buf.as_slice())).unwrap();
            black_box(decoded);
        })
    });

    group.bench_function("capnp", |b| {
        b.iter(|| {
            let capnp_msg = make_capnp_message();
            let mut buf = Vec::new();
            capnp::serialize::write_message(&mut buf, &capnp_msg).unwrap();
            let reader = capnp::serialize::read_message(
                black_box(buf.as_slice()),
                capnp::message::ReaderOptions::new(),
            )
            .unwrap();
            let decoded = reader.get_root::<capnp_player::player::Reader>().unwrap();
            black_box(decoded);
        })
    });

    group.bench_function("flatbuffers", |b| {
        b.iter(|| {
            let mut builder = FlatBufferBuilder::with_capacity(256);
            make_fb_player(&mut builder);
            let buf = builder.finished_data().to_vec();
            let decoded = flatbuffers::root::<fb_player::Player>(black_box(&buf)).unwrap();
            black_box(decoded);
        })
    });

    group.finish();
}

criterion_group!(benches, roundtrip_benchmarks);
criterion_main!(benches);
