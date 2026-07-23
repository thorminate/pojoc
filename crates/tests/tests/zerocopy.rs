// pojoc_player is generated in zero-copy mode (borrowed &'buf str), unlike the owned player embedded in edge tested elsewhere
use pojoc_tests::pojoc_player::{
    self, AABB, Class, Flags, Perks, Player, Region, Stats, Status, Transform, Vector3,
    runtime::pojvec,
};

fn sample() -> Player<'static> {
    Player {
        player_id: 7.0,
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
        inventory: pojvec!["sword", "shield", "healing_potion"],
        hotbar: pojvec!["sword", "healing_potion", "torch", "", "", ""; 6],
        callsign: "NONE00",
        session_token: *b"SESSION000000000",
        guild_tag: *b"IRON",
        status_code: *b"00000000",
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
        tags: pojvec!["starter_zone", "pvp_enabled", "vip"],
        recent_zones: pojvec!["zone_forest", "zone_dungeon", "", "", "", "", "", ""; 8],
        achievement_ids: pojvec![101u32, 204, 305],
        party_members: pojvec![u32 => 2, 3, 0, 0; 4],
        is_nauseous: false,
        active_perks: Perks::TELEKINESIS | Perks::THIEVERY,
        account_flags: Flags::IS_VERIFIED | Flags::IS_PREMIUM,
        loadout: [("sword", 1), ("shield", 1), ("torch", 3), ("", 0)],
    }
}

#[test]
fn zerocopy_player_string_fields_roundtrip() {
    let mut buf = Vec::new();
    pojoc_player::encode(&mut buf, &sample());

    let p = pojoc_player::decode(&buf).unwrap();

    assert_eq!(p.callsign, "NONE00");
    assert_eq!(
        p.inventory.as_slice(),
        &["sword", "shield", "healing_potion"]
    );
    assert_eq!(p.tags.as_slice(), &["starter_zone", "pvp_enabled", "vip"]);
    assert_eq!(p.hotbar, ["sword", "healing_potion", "torch", "", "", ""]);
    assert_eq!(p.recent_zones[0], "zone_forest");
    assert_eq!(p.recent_zones[1], "zone_dungeon");
    assert_eq!(p.loadout[0], ("sword", 1));
    assert_eq!(p.loadout[2], ("torch", 3));
    assert_eq!(p.player_id, 7.0);
    assert_eq!(p.party_members, [2, 3, 0, 0]);
    assert_eq!(p.session_token, *b"SESSION000000000");
}

#[test]
fn zerocopy_decoded_strings_borrow_the_input_buffer() {
    let mut buf = Vec::new();
    pojoc_player::encode(&mut buf, &sample());

    let p = pojoc_player::decode(&buf).unwrap();

    let buf_range = buf.as_ptr_range();
    let s = p.callsign.as_ptr();
    assert!(
        buf_range.contains(&s),
        "decoded callsign should borrow from the input buffer"
    );
}
