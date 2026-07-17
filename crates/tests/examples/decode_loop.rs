// Pure decode loop for sampling profilers (samply/instruments).
// Runs `pojoc_player::decode` in a tight loop for a fixed number of iterations
// so a flamegraph attributes decode's internal cost (bounds/unchecked reads,
// UTF-8 validation, string/collection construction, drop).
//   cargo build --release -p pojoc-tests --example decode_loop
//   samply record ./target/release/examples/decode_loop
use pojoc_tests::pojoc_player::{
    self, AABB, Class, Flags, Perks, Player, Region, Stats, Status, Transform, Vector3,
    runtime::{pojstr, pojtup, pojvec},
};
use std::hint::black_box;

fn make_pojoc_buf() -> Vec<u8> {
    let mut buf = Vec::new();
    pojoc_player::encode(
        &mut buf,
        &Player {
            player_id: 1.0,
            level: 12.5,
            status: Status::Alive,
            class: Class::Warrior,
            region: Region::Central,
            stats: Stats { strength: 14, agility: 8, intelligence: 11, endurance: 10, charisma: 6, resistance: 0.25 },
            inventory: pojvec!["sword", "shield", "healing_potion", "torch", "rope"],
            hotbar: pojvec!["sword", "healing_potion", "torch", "", "", ""; 6],
            callsign: pojstr!("NONE00"),
            session_token: pojstr!("SESSION000000000", 16),
            guild_tag: pojstr!("IRON", 4),
            status_code: pojstr!("00000000", 8),
            coordinates: (128.5, 64.0),
            kill_death: (42, 7),
            velocity: (1.0, 0.0, -1.0),
            spawn_point: (0.0, 0.0, 0.0),
            last_position: (10.0, 42.5, -3.0),
            position: Vector3 { x: 10.0, y: 42.5, z: -3.0, w: 1.0 },
            transform: Transform {
                position: Vector3 { x: 10.0, y: 42.5, z: -3.0, w: 1.0 },
                bounds: AABB { min_x: 0.0, min_y: 0.0, max_x: 100.0, max_y: 100.0 },
            },
            tags: pojvec!["starter_zone", "pvp_enabled", "vip", "quest_giver", "faction_red"],
            recent_zones: pojvec!["zone_forest", "zone_dungeon", "zone_town", "", "", "", "", ""; 8],
            achievement_ids: pojvec![101, 204, 305, 412],
            party_members: pojvec![2, 3, 0, 0; 4],
            is_nauseous: false,
            active_perks: Perks::TELEKINESIS | Perks::THIEVERY,
            account_flags: Flags::IS_VERIFIED | Flags::IS_PREMIUM,
            loadout: pojvec!(pojtup!("sword", 1), pojtup!("shield", 1), pojtup!("healing_potion", 5), pojtup!("torch", 3); 4),
        },
    );
    buf
}

fn main() {
    let buf = make_pojoc_buf();
    let iters: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(60_000_000);
    let mut acc = 0u64;
    for _ in 0..iters {
        let p = pojoc_player::decode(black_box(&buf)).unwrap();
        acc = acc.wrapping_add(black_box(&p).callsign.len() as u64);
    }
    println!("done: {acc}");
}
