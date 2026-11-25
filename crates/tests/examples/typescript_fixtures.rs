//! Write real Rust envelopes for the generated TypeScript interoperability tests.

use pojoc_tests::{
    pojoc_constraints as constraints, pojoc_edge as edge, pojoc_fixed_envelope as envelope,
    pojoc_import_root as imports, pojoc_interning as intern, pojoc_nested_pluck as nested,
    pojoc_player as player, pojoc_recursive as recursive, pojoc_seek_retype as retype,
    pojoc_typescript_fixed_records as fixed,
};
use std::{env, fs, path::Path};

#[path = "../tests/helpers/mod.rs"]
mod helpers;

fn fixtures(
    dir: &Path,
    schema: &str,
    versions: std::ops::RangeInclusive<u64>,
    mut encode: impl FnMut(&mut Vec<u8>, u64) -> pojoc::PojocResult<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    for version in versions {
        let mut bytes = Vec::new();
        encode(&mut bytes, version)?;
        fs::write(dir.join(format!("{schema}-{version}.bin")), bytes)?;
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = env::args_os()
        .nth(1)
        .ok_or("usage: typescript_fixtures OUTPUT_DIR")?;
    let dir = Path::new(&dir);
    fs::create_dir_all(dir)?;

    let player = player::Player {
        player_id: 17.0,
        callsign: "Hej 日本語 🌍",
        level: 42.0,
        achievement_ids: [1, u32::MAX, 3].into_iter().collect(),
        stats: player::Stats {
            strength: -17,
            agility: 43,
            ..Default::default()
        },
        ..Default::default()
    };
    fixtures(dir, "player", 1..=6, |buf, v| {
        player::encode_for_version(buf, &player, v)
    })?;

    let edge = helpers::make_version_probe_edge();
    let latest_edge = *edge::supported_versions().last().unwrap();
    fixtures(dir, "edge", 1..=latest_edge, |buf, v| {
        edge::encode_for_version(buf, &edge, v)
    })?;
    let mut populated = helpers::make_populated_edge();
    populated.legacy_positions = [i32::MIN, i32::MAX, i32::MIN, 0, 127, 128]
        .into_iter()
        .collect();
    populated.u32_delta_seq[..4].copy_from_slice(&[u32::MAX, 0, u32::MAX, 0]);
    populated.delta_i64_seq = [i64::MAX, i64::MIN, -1].into_iter().collect();
    populated.delta_u64_seq = [0, u64::MAX, 0, 1].into_iter().collect();
    fixtures(dir, "edge-populated", 6..=7, |buf, v| {
        edge::encode_for_version(buf, &populated, v)
    })?;
    populated.union_width_probe.as_mut().unwrap().x = edge::union_width::Big::W259(99);
    fixtures(dir, "edge-populated", 8..=latest_edge, |buf, v| {
        edge::encode_for_version(buf, &populated, v)
    })?;

    let fixed_batch = fixed::TypescriptFixedRecords {
        rows: [
            fixed::Row {
                id: u32::MAX,
                value: i16::MIN,
                ratio: -1.5,
                enabled: true,
            },
            fixed::Row {
                id: 0,
                value: i16::MAX,
                ratio: 2.25,
                enabled: false,
            },
        ]
        .into_iter()
        .collect(),
    };
    fixtures(dir, "typescript_fixed_records", 1..=2, |buf, v| {
        fixed::encode_for_version(buf, &fixed_batch, v)
    })?;

    let value = envelope::FixedEnvelope {
        point: envelope::Point {
            x: 42,
            y: 1234,
            z: 9,
        },
        ..Default::default()
    };
    for (version, header, payload_size) in [
        (1, &[1][..], 127),
        (2, &[2, 128, 0, 0, 0][..], 128),
        (128, &[128, 1][..], 129),
    ] {
        let mut bytes = vec![0xaa];
        envelope::encode_for_version(&mut bytes, &value, version)?;
        assert_eq!(bytes[0], 0xaa);
        let frame = &bytes[1..];
        assert!(frame.starts_with(header));
        assert_eq!(frame.len(), header.len() + payload_size);
        let decoded = envelope::decode(frame)?;
        assert_eq!(
            (decoded.point.x, decoded.point.y, decoded.marker),
            (42, 1234, 7)
        );
        if version == 128 {
            let mut current = Vec::new();
            envelope::encode(&mut current, &value);
            assert_eq!(current, frame);
        }
        fs::write(dir.join(format!("fixed_envelope-{version}.bin")), frame)?;
    }

    let interned = intern::Interning {
        primary_label: "first",
        tags: [
            intern::Tag { label: "second" },
            intern::Tag { label: "first" },
        ]
        .into_iter()
        .collect(),
        plain_label: "literal",
        raw_tags: ["second", "third", "first"].into_iter().collect(),
        mono_label: intern::MonoInternedString { value: "third" },
    };
    fixtures(dir, "interning", 1..=1, |buf, v| {
        intern::encode_for_version(buf, &interned, v)
    })?;

    let child = imports::ImportRoot {
        name: "outside",
        child: imports::leaf::ImportLeaf {
            label: "inside",
            count: 5,
            ..Default::default()
        },
        ..Default::default()
    };
    fixtures(dir, "import_root", 1..=2, |buf, v| {
        imports::encode_for_version(buf, &child, v)
    })?;

    let nested = nested::NestedPluck {
        transform: nested::Transform {
            position: nested::Point {
                x: 99,
                label: "shared",
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    fixtures(dir, "nested_pluck", 1..=3, |buf, v| {
        nested::encode_for_version(buf, &nested, v)
    })?;

    let recursive = recursive::Recursive {
        head: Some(recursive::Node {
            value: 125,
            next: Some(Box::new(recursive::Node {
                value: -6,
                ..Default::default()
            })),
            ..Default::default()
        }),
        ..Default::default()
    };
    fixtures(dir, "recursive", 1..=2, |buf, v| {
        recursive::encode_for_version(buf, &recursive, v)
    })?;

    let retype = retype::SeekRetype {
        id: 17,
        label: Some("label"),
    };
    fixtures(dir, "seek_retype", 1..=2, |buf, v| {
        retype::encode_for_version(buf, &retype, v)
    })?;

    let constraints = constraints::Constraints {
        count: 8,
        tags: ["x"].into_iter().collect(),
        label: "valid",
    };
    fixtures(dir, "constraints", 1..=1, |buf, v| {
        constraints::encode_for_version(buf, &constraints, v)
    })?;
    Ok(())
}
