use pojoc_tests::pojoc_player::*;
use pojoc_runtime::*;

#[test]
fn test_roundtrip_v1() {
    let mut buf = Vec::new();
    let len_pos = write_envelope_header(&mut buf, 1);
    let payload_start = buf.len();
    // v1 fields: id(f32), name(string), level(int), inventory
    write_f32(&mut buf, 197.657f64 as f32);
    write_string(&mut buf, "Player");
    write_i32(&mut buf, 12i32);
    write_array_len(&mut buf, 2);
    write_string(&mut buf, "sword");
    write_string(&mut buf, "shield");
    let payload_len = buf.len() - payload_start;
    patch_envelope_length(&mut buf, len_pos, payload_len);

    let decoded = decode(&buf).unwrap();
    assert!((decoded.player_id - 197.657f64 as f32 as f64).abs() < 0.001);
    assert_eq!(decoded.level, 12.0);
    assert_eq!(decoded.inventory.as_slice(), &["sword", "shield"]);
    assert_eq!(decoded.tags.as_slice(), &[] as &[String]);
    assert_eq!(decoded.is_dead, false);
}

#[test]
fn test_roundtrip_v2() {
    let mut buf = Vec::new();
    let len_pos = write_envelope_header(&mut buf, 2);
    let payload_start = buf.len();
    // v2 fields: id(f32), name(string), level(float), inventory, position(Vector3@2)
    write_f32(&mut buf, 197.657f64 as f32);
    write_string(&mut buf, "Player");
    write_f32(&mut buf, 12f32);
    write_array_len(&mut buf, 2);
    write_string(&mut buf, "sword");
    write_string(&mut buf, "shield");
    write_f32(&mut buf, 0.1);
    write_f32(&mut buf, 0.1);
    write_f32(&mut buf, 0.1);
    let payload_len = buf.len() - payload_start;
    patch_envelope_length(&mut buf, len_pos, payload_len);

    let decoded = decode(&buf).unwrap();
    assert!((decoded.player_id - 197.657f64 as f32 as f64).abs() < 0.001);
    assert_eq!(decoded.level, 12.0);
    assert_eq!(decoded.inventory.as_slice(), &["sword", "shield"]);
    assert_eq!(decoded.position.x, 0.1);
    assert_eq!(decoded.position.y, 0.1);
    assert_eq!(decoded.position.z, 0.1);
    assert_eq!(decoded.tags.as_slice(), &[] as &[String]);
    assert_eq!(decoded.is_dead, false);
}

#[test]
fn test_roundtrip_v3() {
    let mut buf = Vec::new();
    let len_pos = write_envelope_header(&mut buf, 3);
    let payload_start = buf.len();
    // v3 fields: player_id(f64), level(float), inventory, position(Vector3@3), tags(string[])
    write_f64(&mut buf, 197.657f64);
    write_f32(&mut buf, 12f32);
    write_array_len(&mut buf, 2);
    write_string(&mut buf, "sword");
    write_string(&mut buf, "shield");
    write_f32(&mut buf, 0.1);
    write_f32(&mut buf, 0.1);
    write_f32(&mut buf, 0.1);
    write_f32(&mut buf, 1.0);
    write_array_len(&mut buf, 1);
    write_string(&mut buf, "starter_zone");
    let payload_len = buf.len() - payload_start;
    patch_envelope_length(&mut buf, len_pos, payload_len);

    let decoded = decode(&buf).unwrap();
    assert!((decoded.player_id - 197.657f64).abs() < 0.001);
    assert_eq!(decoded.level, 12.0);
    assert_eq!(decoded.inventory.as_slice(), &["sword", "shield"]);
    assert_eq!(decoded.position.x, 0.1);
    assert_eq!(decoded.position.y, 0.1);
    assert_eq!(decoded.position.z, 0.1);
    assert_eq!(decoded.position.w, 1.0);
    assert_eq!(decoded.tags.as_slice(), &["starter_zone"]);
    assert_eq!(decoded.is_dead, false);
}

#[test]
fn test_roundtrip_v4() {
    let mut buf = Vec::new();
    let len_pos = write_envelope_header(&mut buf, 4);
    let payload_start = buf.len();
    // v4 fields: player_id(f64), level(float), inventory, position(Vector3@3), tags(string[]), is_nauseous(bool)
    write_f64(&mut buf, 197.657f64);
    write_f32(&mut buf, 12f32);
    write_array_len(&mut buf, 2);
    write_string(&mut buf, "sword");
    write_string(&mut buf, "shield");
    write_f32(&mut buf, 0.1);
    write_f32(&mut buf, 0.1);
    write_f32(&mut buf, 0.1);
    write_f32(&mut buf, 1.0);
    write_array_len(&mut buf, 1);
    write_string(&mut buf, "starter_zone");
    write_bool(&mut buf, true);
    let payload_len = buf.len() - payload_start;
    patch_envelope_length(&mut buf, len_pos, payload_len);

    let decoded = decode(&buf).unwrap();
    assert!((decoded.player_id - 197.657f64).abs() < 0.001);
    assert_eq!(decoded.level, 12.0);
    assert_eq!(decoded.inventory.as_slice(), &["sword", "shield"]);
    assert_eq!(decoded.position.x, 0.1);
    assert_eq!(decoded.position.y, 0.1);
    assert_eq!(decoded.position.z, 0.1);
    assert_eq!(decoded.position.w, 1.0);
    assert_eq!(decoded.tags.as_slice(), &["starter_zone"]);
    assert_eq!(decoded.is_nauseous, true);
    assert_eq!(decoded.is_dead, false);
}

#[test]
fn test_roundtrip_v5() {
    let player = Player {
        player_id: 197.657,
        level: 12.5,
        inventory: pojvec!["sword", "shield", "healing_potion"],
        position: Vector3 { x: 10.0, y: 42.5, z: -3.0, w: 1.0 },
        tags: pojvec!["starter_zone", "pvp_enabled", "vip"],
        is_nauseous: false,
        is_dead: true,
    };

    let mut buf = Vec::new();
    encode(&mut buf, &player);
    let decoded = decode(&buf).unwrap();

    assert_eq!(decoded.player_id, player.player_id);
    assert_eq!(decoded.level, player.level);
    assert_eq!(decoded.inventory, player.inventory);
    assert_eq!(decoded.position.x, player.position.x);
    assert_eq!(decoded.position.y, player.position.y);
    assert_eq!(decoded.position.z, player.position.z);
    assert_eq!(decoded.position.w, player.position.w);
    assert_eq!(decoded.tags, player.tags);
    assert_eq!(decoded.is_nauseous, player.is_nauseous);
    assert_eq!(decoded.is_dead, player.is_dead);
}