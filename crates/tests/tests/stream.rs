use pojoc_tests::pojoc_player::{self as player, Player, runtime::read_envelope};
use pojoc_tests::pojoc_stdio_bench::{self as eager, StdioBench};
use pojoc_tests::pojoc_stdio_hybrid::{
    self as hybrid, StdioHybrid,
    runtime::{LazyView, PojocVec},
};
use pojoc_tests::pojoc_stdio_lazy as lazy;
use std::io::{self, BufReader, ErrorKind, Read};

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

#[cfg(feature = "sidecar-json")]
#[test]
fn sidecar_encodes_and_decodes_multiple_records() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let run = |mode, input: &[u8]| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_stdio-bench-sidecar"))
            .args(["serve", "--stdio", mode])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    };

    let first = serde_json::json!({"id": 1, "flags": 17, "payload": [0, 10, 255]});
    let second = serde_json::json!({"id": 2, "flags": 0, "payload": []});
    let encoded = run("encode", format!("{first}\n{second}").as_bytes());
    assert!(encoded.status.success(), "{:?}", encoded.stderr);
    let mut pos = 0;
    for (id, flags, payload) in [(1, 17, &[0, 10, 255][..]), (2, 0, &[][..])] {
        let start = pos;
        assert_eq!(read_envelope(&encoded.stdout, &mut pos).unwrap().version, 2);
        let value = eager::decode(&encoded.stdout[start..pos]).unwrap();
        assert_eq!(
            (value.id, value.flags, value.payload.as_slice()),
            (id, flags, payload)
        );
    }
    assert_eq!(pos, encoded.stdout.len());

    let mut frames = Vec::new();
    let legacy = StdioBench {
        id: 3,
        ..Default::default()
    };
    eager::encode_for_version(&mut frames, &legacy, 1).unwrap();
    frames.extend(encoded.stdout);
    let decoded = run("decode", &frames);
    assert!(decoded.status.success(), "{:?}", decoded.stderr);
    let records: Vec<serde_json::Value> = decoded
        .stdout
        .strip_suffix(b"\n")
        .unwrap()
        .split(|&b| b == b'\n')
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(
        records,
        [
            serde_json::json!({"id": 3, "flags": 0, "payload": []}),
            first,
            second
        ]
    );

    let invalid = run("encode", b"{broken json}\n");
    assert!(!invalid.status.success());
    assert!(invalid.stdout.is_empty());
}

#[test]
fn human_readable_serde_roundtrips_lazy_fields_and_borrowed_strings() {
    let record: serde_json::Value = serde_json::from_str(
        r#"{"id":7,"name":"first\nsecond","label":"a \"quote\"","samples":[44],"blob":[1,2],"notes":["\u00e5"],"flags":5}"#,
    )
    .unwrap();
    let value: StdioHybrid<'_> = serde::Deserialize::deserialize(&record).unwrap();
    let mut frame = Vec::new();
    hybrid::encode(&mut frame, &value);
    let mut input = frame.as_slice();
    hybrid::serve_decode(&mut input, 1024, |decoded| {
        assert_eq!(serde_json::to_value(decoded).unwrap(), record);
        Ok(())
    })
    .unwrap();
}

#[test]
fn human_readable_serde_uses_tagged_unions_with_borrowed_payloads() {
    use pojoc_tests::pojoc_edge::Payload;

    let message = serde_json::json!({"Message": "line 1\nline 2"});
    let value: Payload<'_> = serde::Deserialize::deserialize(&message).unwrap();
    assert!(matches!(value, Payload::Message("line 1\nline 2")));
    assert_eq!(serde_json::to_value(value).unwrap(), message);

    let unknown = serde_json::json!({"Unknown": {"discriminant": 127, "data": [1, 2]}});
    let value: Payload<'_> = serde::Deserialize::deserialize(&unknown).unwrap();
    assert_eq!(serde_json::to_value(value).unwrap(), unknown);
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

struct Fragmented<'a>(&'a [u8]);

impl Read for Fragmented<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let count = bytes.len().min(3);
        self.0.read(&mut bytes[..count])
    }
}

fn envelope(version: u64, payload: &[u8]) -> Vec<u8> {
    let mut request = Vec::new();
    let length_pos = eager::runtime::write_envelope_header(&mut request, version);
    request.extend_from_slice(payload);
    eager::runtime::patch_envelope_length(&mut request, length_pos, payload.len());
    request
}

#[test]
fn incremental_eager_replies_match_the_slice_codec() {
    let latest = StdioBench {
        id: 17,
        flags: 83,
        payload: eager::runtime::PojocVec::from_slice(&[0xab; 257]),
    };
    let mut large = Vec::new();
    eager::encode(&mut large, &latest);

    let mut payload = 42_u64.to_le_bytes().to_vec();
    // A noncanonical array length followed by trailing bytes inside the envelope.
    payload.extend([0x83, 0, 1, 2, 3, 0xff]);
    let frames = [large, envelope(1, &payload), envelope(1, &[0; 9])];
    let mut expected = Vec::new();
    for frame in &frames {
        eager::encode(&mut expected, &eager::decode(frame).unwrap());
    }

    let requests = frames.concat();
    let mut input = BufReader::with_capacity(3, Fragmented(&requests));
    let mut replies = Vec::new();
    eager::serve(&mut input, &mut replies, 512).unwrap();
    assert_eq!(replies, expected);
}

#[test]
fn lazy_stream_preserves_raw_array_lengths_across_layouts() {
    let mut historical = 42_u64.to_le_bytes().to_vec();
    historical.extend([0x83, 0, 1, 2, 3]);
    lazy::runtime::write_string(&mut historical, "removed in v2");

    let mut latest = 43_u64.to_le_bytes().to_vec();
    lazy::runtime::write_u32(&mut latest, 19);
    latest.extend([0x83, 0, 4, 5, 6]);

    // The old layout has a field after the array, while the latest has a trailing array.
    let frames = [envelope(1, &historical), envelope(2, &latest)];
    let mut expected = Vec::new();
    for frame in &frames {
        lazy::encode(&mut expected, &lazy::decode(frame).unwrap());
    }
    let requests = frames.concat();
    let mut input = BufReader::with_capacity(3, Fragmented(&requests));
    let mut replies = Vec::new();
    lazy::serve(&mut input, &mut replies, 512).unwrap();
    assert_eq!(replies, expected);
}

#[test]
fn incremental_stream_requires_the_complete_envelope_before_replying() {
    let mut payload = 42_u64.to_le_bytes().to_vec();
    payload.extend([1, 2, 3]); // One array byte, followed by a trailing byte.
    let request = envelope(1, &payload);
    let mut replies = Vec::new();
    let error = eager::serve(&mut &request[..request.len() - 1], &mut replies, 512).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::UnexpectedEof);
    assert!(replies.is_empty());

    payload[8] = 10; // The array cannot borrow bytes from the next envelope.
    let mut requests = envelope(1, &payload);
    requests.extend_from_slice(&request);
    let mut input = requests.as_slice();
    let error = eager::serve(&mut input, &mut replies, 512).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidData);
    assert_eq!(input, request);
    assert!(replies.is_empty());
}

fn hybrid_requests() -> [Vec<u8>; 3] {
    let long_note = "long ".repeat(300);
    // Use a noncanonical varint for 8192. The lazy blob must forward it verbatim.
    let mut raw_blob = vec![0x80, 0xc0, 0];
    raw_blob.resize(8192 + 3, 0x9b);
    let populated = StdioHybrid {
        id: 123,
        name: Some("seekable name"),
        label: "interned label",
        samples: PojocVec::from_vec((0..10240).map(|i| i * 0x10101).collect()),
        blob: LazyView::new(&raw_blob, |buf, pos| {
            hybrid::runtime::read_pod_array(buf, pos).map(Some)
        }),
        notes: PojocVec::from_slice(&["one", &long_note]),
        flags: 97,
    };
    let absent = StdioHybrid {
        blob: LazyView::Owned(None),
        name: None,
        ..populated.clone()
    };
    let mut old = Vec::new();
    let mut latest = Vec::new();
    let mut empty = Vec::new();
    hybrid::encode_for_version(&mut old, &populated, 1).unwrap();
    hybrid::encode(&mut latest, &populated);
    hybrid::encode(&mut empty, &absent);
    [old, empty, latest]
}

#[test]
fn hybrid_stream_matches_the_slice_codec_across_versions() {
    let requests = hybrid_requests();
    let mut expected = Vec::new();
    for request in &requests {
        hybrid::encode(&mut expected, &hybrid::decode(request).unwrap());
    }
    let requests = requests.concat();
    let mut input = BufReader::with_capacity(3, Fragmented(&requests));
    let mut replies = Vec::new();
    hybrid::serve(&mut input, &mut replies, 65536).unwrap();
    assert_eq!(replies, expected);
}

#[test]
fn hybrid_stream_withholds_a_reply_if_the_suffix_is_truncated() {
    let [large, _, _] = hybrid_requests();
    let mut replies = Vec::new();
    let error = hybrid::serve(&mut &large[..large.len() - 1], &mut replies, 65536).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::UnexpectedEof);
    assert!(replies.is_empty());
}
