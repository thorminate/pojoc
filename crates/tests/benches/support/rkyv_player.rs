use pojoc_tests::pojoc_player::{Class, Flags, Perks, Region, Status};
use rkyv::{Archive, Deserialize, Serialize};

#[derive(Archive, Serialize, Deserialize)]
pub struct RkyvPlayer {
    player_id: f64,
    level: f32,
    status: u8,
    class: u8,
    region: u8,
    inventory: Vec<String>,
    callsign: String,
    stats: (i32, i32, i32, i32, i32, f32),
    hotbar: [String; 6],
    session_token: [u8; 16],
    coordinates: (f32, f32),
    position: [f32; 4],
    kill_death: (i32, i32),
    tags: Vec<String>,
    transform: ([f32; 4], [f32; 4]),
    recent_zones: [String; 8],
    velocity: (f32, f32, f32),
    status_code: [u8; 8],
    is_nauseous: bool,
    guild_tag: [u8; 4],
    spawn_point: (f32, f32, f32),
    achievement_ids: Vec<u32>,
    active_perks: [u8; 1],
    account_flags: [u8; 1],
    loadout: [(String, i32); 4],
    party_members: [u32; 4],
    last_position: (f32, f32, f32),
}

impl RkyvPlayer {
    pub fn new(inventory: Vec<String>) -> Self {
        Self {
            player_id: 1.0,
            level: 12.5,
            status: Status::Alive as u8,
            class: Class::Warrior as u8,
            region: Region::Central as u8,
            inventory,
            callsign: "NONE00".into(),
            stats: (14, 8, 11, 10, 6, 0.25),
            hotbar: ["sword", "healing_potion", "torch", "", "", ""].map(str::to_owned),
            session_token: *b"SESSION000000000",
            coordinates: (128.5, 64.0),
            position: [10.0, 42.5, -3.0, 1.0],
            kill_death: (42, 7),
            tags: [
                "starter_zone",
                "pvp_enabled",
                "vip",
                "quest_giver",
                "faction_red",
            ]
            .map(str::to_owned)
            .into(),
            transform: ([10.0, 42.5, -3.0, 1.0], [0.0, 0.0, 100.0, 100.0]),
            recent_zones: [
                "zone_forest",
                "zone_dungeon",
                "zone_town",
                "",
                "",
                "",
                "",
                "",
            ]
            .map(str::to_owned),
            velocity: (1.0, 0.0, -1.0),
            status_code: *b"00000000",
            is_nauseous: false,
            guild_tag: *b"IRON",
            spawn_point: (0.0, 0.0, 0.0),
            achievement_ids: vec![101, 204, 305, 412],
            active_perks: (Perks::TELEKINESIS | Perks::THIEVERY).0,
            account_flags: (Flags::IS_VERIFIED | Flags::IS_PREMIUM).0,
            loadout: [
                ("sword", 1),
                ("shield", 1),
                ("healing_potion", 5),
                ("torch", 3),
            ]
            .map(|(item, count)| (item.to_owned(), count)),
            party_members: [2, 3, 0, 0],
            last_position: (10.0, 42.5, -3.0),
        }
    }
}

impl Default for RkyvPlayer {
    fn default() -> Self {
        Self::new(
            ["sword", "shield", "healing_potion", "torch", "rope"]
                .map(str::to_owned)
                .into(),
        )
    }
}
