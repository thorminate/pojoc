use pojoc_tests::pojoc_player::{self as player, Player, runtime::read_envelope};
use std::io::{BufReader, ErrorKind};

#[test]
fn serves_multiple_envelopes_and_upgrades_older_versions() {
    let mut requests = Vec::new();
    let first = Player {
        player_id: 42.0,
        is_nauseous: true,
        ..Player::default()
    };
    player::encode_for_version(&mut requests, &first, 1).unwrap();
    let second = Player {
        player_id: 81.0,
        is_nauseous: true,
        ..Player::default()
    };
    player::encode(&mut requests, &second);

    let mut input = BufReader::with_capacity(1, requests.as_slice());
    let mut responses = Vec::new();
    player::serve(&mut input, &mut responses, 1024).unwrap();

    let mut pos = 0;
    let latest = *player::supported_versions().last().unwrap();
    for (id, is_nauseous) in [(42.0, false), (81.0, true)] {
        let start = pos;
        assert_eq!(read_envelope(&responses, &mut pos).unwrap().version, latest);
        let response = player::decode(&responses[start..pos]).unwrap();
        assert_eq!(response.player_id, id);
        assert_eq!(response.is_nauseous, is_nauseous);
    }
    assert_eq!(pos, responses.len());
}

#[test]
fn rejects_truncated_and_oversize_requests_without_a_reply() {
    let mut responses = Vec::new();
    for (mut input, expected) in [
        (&[0x81][..], ErrorKind::UnexpectedEof),
        (&[1, 1][..], ErrorKind::UnexpectedEof),
        (&[1, 1, 0, 0, 0][..], ErrorKind::UnexpectedEof),
        (&[1, 129, 0, 0, 0][..], ErrorKind::InvalidData),
        (&[0x80; 10][..], ErrorKind::InvalidData),
    ] {
        let error = player::serve(&mut input, &mut responses, 128).unwrap_err();
        assert_eq!(error.kind(), expected);
        assert!(responses.is_empty());
    }
}

#[test]
fn skipping_a_truncated_array_stops_after_the_last_complete_element() {
    let value = Player {
        achievement_ids: player::runtime::pojvec![u32 => 11, 22],
        ..Player::default()
    };
    let mut payload = Vec::new();
    player::encode_v4(&mut payload, &value);

    let mut pos = 0;
    player::skip_v4(&payload, &mut pos).unwrap();
    assert_eq!(pos, payload.len());

    // The v4 payload ends with the achievement IDs. Remove a byte of the last ID.
    payload.pop();
    pos = 0;
    assert!(matches!(
        player::skip_v4(&payload, &mut pos),
        Err(player::runtime::Error::UnexpectedEof)
    ));
    assert_eq!(pos, payload.len() - 3);
}
