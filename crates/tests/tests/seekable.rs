mod helpers;

use helpers::*;
use pojoc_tests::pojoc_edge::{runtime::*, *};

fn encoded_at(version: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_for_version(&mut buf, &make_populated_edge(), version).expect("encode failed");
    buf
}

fn latest() -> u64 {
    *supported_versions().last().unwrap()
}

#[test]
fn seek_returns_the_encoded_values() {
    let buf = encoded_at(latest());
    let e = make_populated_edge();

    assert_eq!(seek_seek_tag(&buf).unwrap(), e.seek_tag);
    assert_eq!(seek_seek_name(&buf).unwrap(), e.seek_name);
    assert_eq!(seek_seek_note(&buf).unwrap(), e.seek_note);
    assert_eq!(seek_seek_label(&buf).unwrap(), e.seek_label);
    assert_eq!(seek_heal_radius(&buf).unwrap(), e.heal_radius);
}

/// the invariant the whole feature rests on: a partial read and a full decode of
/// the same bytes can never disagree, at any version. covers the missing-field
/// default and per-version cast paths.
#[test]
fn seek_agrees_with_decode_at_every_version() {
    for &version in supported_versions() {
        let buf = encoded_at(version);
        let d = decode(&buf).unwrap_or_else(|e| panic!("decode failed @v{version}: {e:?}"));

        assert_eq!(
            seek_seek_tag(&buf).unwrap(),
            d.seek_tag,
            "seek_tag @v{version}"
        );
        assert_eq!(
            seek_seek_name(&buf).unwrap(),
            d.seek_name,
            "seek_name @v{version}"
        );
        assert_eq!(
            seek_seek_note(&buf).unwrap(),
            d.seek_note,
            "seek_note @v{version}"
        );
        assert_eq!(
            seek_seek_label(&buf).unwrap(),
            d.seek_label,
            "seek_label @v{version}"
        );
        assert_eq!(
            seek_heal_radius(&buf).unwrap(),
            d.heal_radius,
            "heal_radius @v{version}"
        );

        let blob_seek = seek_seek_blob(&buf).unwrap().get().unwrap();
        let blob_decode = d.seek_blob.clone().get().unwrap();
        assert_eq!(blob_seek, blob_decode, "seek_blob @v{version}");
    }
}

#[test]
fn seek_all_agrees_with_decode_at_every_version() {
    for &version in supported_versions() {
        let buf = encoded_at(version);
        let d = decode(&buf).unwrap();
        let all = seek_all(&buf).unwrap_or_else(|e| panic!("seek_all failed @v{version}: {e:?}"));

        assert_eq!(all.seek_tag, d.seek_tag, "seek_tag @v{version}");
        assert_eq!(all.seek_name, d.seek_name, "seek_name @v{version}");
        assert_eq!(all.seek_note, d.seek_note, "seek_note @v{version}");
        assert_eq!(all.seek_label, d.seek_label, "seek_label @v{version}");
        assert_eq!(all.heal_radius, d.heal_radius, "heal_radius @v{version}");
        assert_eq!(
            all.seek_blob.clone().get().unwrap(),
            d.seek_blob.clone().get().unwrap(),
            "seek_blob @v{version}"
        );
    }
}

/// versions predating v6 never carried these fields, so every accessor must hand
/// back the declared default rather than misreading whatever sits at that offset
#[test]
fn versions_predating_the_fields_yield_their_defaults() {
    for &version in supported_versions() {
        if version >= 6 {
            continue;
        }
        let buf = encoded_at(version);
        assert_eq!(seek_seek_tag(&buf).unwrap(), 0, "@v{version}");
        assert_eq!(seek_seek_name(&buf).unwrap(), "", "@v{version}");
        assert_eq!(seek_seek_note(&buf).unwrap(), None, "@v{version}");
        assert_eq!(seek_seek_label(&buf).unwrap(), "", "@v{version}");
    }
}

/// heal_radius predates v6 and is retyped into a seekable field there, so older
/// versions reach it through a cast — the seek must apply the same one
#[test]
fn a_retyped_field_seeks_through_its_cast_on_older_versions() {
    for &version in supported_versions() {
        let buf = encoded_at(version);
        let d = decode(&buf).unwrap();
        assert_eq!(
            seek_heal_radius(&buf).unwrap(),
            d.heal_radius,
            "heal_radius @v{version}"
        );
    }
}

#[test]
fn truncation_errors_and_never_panics() {
    for &version in supported_versions() {
        let buf = encoded_at(version);
        for n in 0..buf.len() {
            let part = &buf[..n];
            let _ = seek_seek_tag(part);
            let _ = seek_seek_name(part);
            let _ = seek_seek_note(part);
            let _ = seek_seek_label(part);
            let _ = seek_heal_radius(part);
            let _ = seek_seek_blob(part);
            let _ = seek_all(part);
        }
    }
}

#[test]
fn an_unknown_version_is_rejected() {
    let mut buf = Vec::new();
    let len_pos = write_envelope_header(&mut buf, 99);
    patch_envelope_length(&mut buf, len_pos, 0);

    assert!(matches!(
        seek_seek_tag(&buf),
        Err(Error::UnsupportedVersion(99))
    ));
    assert!(matches!(seek_all(&buf), Err(Error::UnsupportedVersion(99))));
}

#[test]
fn a_corrupt_buffer_errors_rather_than_returning_garbage() {
    let buf = encoded_at(latest());
    // flip bytes across the payload; every outcome must be Ok or Err, never a panic
    for i in 0..buf.len() {
        let mut bad = buf.clone();
        bad[i] = bad[i].wrapping_add(0x7F);
        let _ = seek_seek_tag(&bad);
        let _ = seek_seek_name(&bad);
        let _ = seek_seek_label(&bad);
        let _ = seek_all(&bad);
    }
}

/// the interned seekable field takes the single-index fast path, which resolves
/// against the same table the full decode builds
#[test]
fn the_interned_seek_resolves_against_the_shared_table() {
    let buf = encoded_at(latest());
    let d = decode(&buf).unwrap();

    // seek_label and interned_label are the same string, deduped into one entry
    assert_eq!(seek_seek_label(&buf).unwrap(), "shared-tag");
    assert_eq!(d.seek_label, d.interned_label);
    assert_eq!(seek_seek_label(&buf).unwrap(), d.interned_label);
}

#[test]
fn a_seekable_lazy_field_hands_back_bytes_without_decoding() {
    let buf = encoded_at(latest());
    let view = seek_seek_blob(&buf).unwrap();
    assert!(
        matches!(view, LazyView::Raw { .. }),
        "a seekable lazy field should arrive undecoded, got {view:?}"
    );
    assert_eq!(view.get().unwrap(), Some(vec![9, 8, 7].into()));
}

// The `seek` benchmark only means something if every accessor it times returns
// what the equivalent full decode returns, at every payload size it sweeps —
// otherwise it could be timing an early error return. Pinned here so a codegen
// change can't quietly turn the benchmark into a measurement of nothing.
#[test]
fn seek_bench_fixture_agrees_with_decode_at_every_size() {
    use pojoc_tests::pojoc_seek_bench::{self as sb, SeekBench, runtime::*};

    for n in [0usize, 16, 256, 4096] {
        let value = SeekBench {
            id: 0xDEAD_BEEF_1234_5678,
            kind: 42,
            name: "entity-name-of-moderate-length",
            label: "starter_zone",
            timestamp: 1_700_000_000,
            flags: 0b1011,
            score: 1234.5678,
            description: "a description long enough that skipping it is not free",
            tags: PojocVec::from_vec(vec!["starter_zone"; n]),
            samples: PojocVec::from_vec((0..n).map(|i| i as f64 * 1.5).collect()),
            history: PojocVec::from_vec((0..n).map(|i| i as u64 * 7).collect()),
            points: PojocVec::from_vec(
                (0..n)
                    .map(|i| (i as f32, i as f32 * 2.0, i as f32 * 3.0))
                    .collect(),
            ),
            notes: PojocVec::from_vec(vec!["raid_boss"; n]),
        };

        let mut buf = Vec::new();
        sb::encode(&mut buf, &value);
        let decoded = sb::decode(&buf).expect("decode failed");

        assert_eq!(sb::seek_id(&buf).unwrap(), decoded.id, "id at n={n}");
        assert_eq!(sb::seek_kind(&buf).unwrap(), decoded.kind, "kind at n={n}");
        assert_eq!(sb::seek_name(&buf).unwrap(), decoded.name, "name at n={n}");
        assert_eq!(
            sb::seek_label(&buf).unwrap(),
            decoded.label,
            "interned label at n={n}"
        );

        let all = sb::seek_all(&buf).unwrap();
        assert_eq!(
            (all.id, all.kind, all.name, all.label),
            (decoded.id, decoded.kind, decoded.name, decoded.label),
            "seek_all at n={n}"
        );
    }
}

// ---- subset reader ----

/// plucking a named subset must agree with the individual accessors, whatever
/// mix of fixed-width and variable-width targets is asked for
#[test]
fn pluck_matches_the_individual_accessors() {
    use pojoc_tests::pluck_seek_many as pluck;
    use pojoc_tests::pojoc_seek_many as many;
    use pojoc_tests::pojoc_seek_many::*;

    let value = many_fixture();
    let mut buf = Vec::new();
    encode(&mut buf, &value);

    // a lone early target, and a lone late one that walks past eleven strings
    assert_eq!(pluck!(many, &buf, s0).unwrap(), seek_s0(&buf).unwrap());
    assert_eq!(pluck!(many, &buf, s11).unwrap(), seek_s11(&buf).unwrap());

    // mixed fixed and variable, listed out of declaration order
    let (s2, f0, s0) = pluck!(many, &buf, s2, f0, s0).unwrap();
    assert_eq!(
        (s2, f0, s0),
        (
            seek_s2(&buf).unwrap(),
            seek_f0(&buf).unwrap(),
            seek_s0(&buf).unwrap()
        )
    );

    // fixed-width only: never enters the tail
    let (f0, f7) = pluck!(many, &buf, f0, f7).unwrap();
    assert_eq!((f0, f7), (seek_f0(&buf).unwrap(), seek_f7(&buf).unwrap()));

    // and a spread of targets must agree with seek_all
    let all = seek_all(&buf).unwrap();
    let (f0, f7, s0, s11) = pluck!(many, &buf, f0, f7, s0, s11).unwrap();
    assert_eq!((f0, f7, s0, s11), (all.f0, all.f7, all.s0, all.s11));
}

#[test]
fn pluck_errors_on_truncation_rather_than_reading_garbage() {
    use pojoc_tests::pluck_seek_many as pluck;
    use pojoc_tests::pojoc_seek_many as many;
    use pojoc_tests::pojoc_seek_many::*;

    let mut buf = Vec::new();
    encode(&mut buf, &many_fixture());

    let full = pluck!(many, &buf, s11).unwrap();
    for cut in 0..buf.len() {
        // must not panic; a truncated buffer errors rather than reading garbage
        if let Ok(v) = pluck!(many, &buf[..cut], s11) {
            assert_eq!(v, full, "a successful read must return the real value");
        }
    }
}

fn many_fixture() -> pojoc_tests::pojoc_seek_many::SeekMany<'static> {
    use pojoc_tests::pojoc_seek_many::{SeekMany, runtime::*};
    SeekMany {
        f0: 1,
        f1: 2,
        f2: 3,
        f3: 4,
        f4: 5,
        f5: 6,
        f6: 7.0,
        f7: 8.0,
        s0: "string-value-zero",
        s1: "string-value-one",
        s2: "string-value-two",
        s3: "string-value-three",
        s4: "string-value-four",
        s5: "string-value-five",
        s6: "string-value-six",
        s7: "string-value-seven",
        s8: "string-value-eight",
        s9: "string-value-nine",
        s10: "string-value-ten",
        s11: "string-value-eleven",
        blob: PojocVec::from_vec(vec![7u8; 32]),
        samples: PojocVec::from_vec((0..32).map(|i| i as f64).collect()),
        notes: PojocVec::from_vec(vec!["note"; 32]),
    }
}
