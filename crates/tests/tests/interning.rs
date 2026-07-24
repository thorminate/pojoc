use pojoc_tests::pojoc_interning::*;

fn decode_static(buf: &[u8]) -> Interning<'static> {
    decode(Vec::leak(buf.to_vec())).expect("decode failed")
}

#[test]
fn test_interning_roundtrip() {
    let original = Interning {
        primary_label: "shared",
        tags: pojoc::pojvec!(
            Tag { label: "shared" },
            Tag { label: "shared" },
            Tag { label: "unique" },
        ),
        plain_label: "not interned but happens to match too: shared",
        raw_tags: pojoc::pojvec!("shared", "raw-unique"),
        mono_label: MonoInternedString { value: "shared" },
    };

    let mut buf = Vec::new();
    encode(&mut buf, &original);
    let decoded = decode_static(&buf);

    assert_eq!(decoded.primary_label, "shared");
    assert_eq!(decoded.tags.len(), 3);
    assert_eq!(decoded.tags[0].label, "shared");
    assert_eq!(decoded.tags[1].label, "shared");
    assert_eq!(decoded.tags[2].label, "unique");
    assert_eq!(
        decoded.plain_label,
        "not interned but happens to match too: shared"
    );
    assert_eq!(decoded.raw_tags.len(), 2);
    assert_eq!(decoded.raw_tags[0], "shared");
    assert_eq!(decoded.raw_tags[1], "raw-unique");
    assert_eq!(decoded.mono_label.value, "shared");
}

#[test]
fn test_interning_dedups_repeated_strings_across_nested_and_root() {
    // "shared" appears 5 times across root, nested, array, and generic fields but should be stored once
    let interned = Interning {
        primary_label: "shared",
        tags: pojoc::pojvec!(Tag { label: "shared" }, Tag { label: "shared" }),
        plain_label: "",
        raw_tags: pojoc::pojvec!("shared"),
        mono_label: MonoInternedString { value: "shared" },
    };
    let mut interned_buf = Vec::new();
    encode(&mut interned_buf, &interned);

    // same shape but all-distinct strings, so any size difference comes from dedup
    let distinct = Interning {
        primary_label: "aaaaaaaaaaaaaaaaaaaa",
        tags: pojoc::pojvec!(
            Tag {
                label: "bbbbbbbbbbbbbbbbbbbb"
            },
            Tag {
                label: "cccccccccccccccccccc"
            },
        ),
        plain_label: "",
        raw_tags: pojoc::pojvec!("dddddddddddddddddddd"),
        mono_label: MonoInternedString {
            value: "eeeeeeeeeeeeeeeeeeee",
        },
    };
    let mut distinct_buf = Vec::new();
    encode(&mut distinct_buf, &distinct);

    assert!(
        interned_buf.len() < distinct_buf.len(),
        "repeated-string message ({} bytes) should be smaller than the \
         all-distinct-strings message ({} bytes) once deduped",
        interned_buf.len(),
        distinct_buf.len()
    );

    let decoded = decode_static(&interned_buf);
    assert_eq!(decoded.primary_label, "shared");
    for tag in decoded.tags.iter() {
        assert_eq!(tag.label, "shared");
    }
    assert_eq!(decoded.raw_tags[0], "shared");
    assert_eq!(decoded.mono_label.value, "shared");
}

#[test]
fn test_interning_empty_strings_roundtrip() {
    let original = Interning::default();
    let mut buf = Vec::new();
    encode(&mut buf, &original);
    let decoded = decode_static(&buf);
    assert_eq!(decoded.primary_label, "");
    assert_eq!(decoded.tags.len(), 0);
    assert_eq!(decoded.plain_label, "");
    assert_eq!(decoded.raw_tags.len(), 0);
    assert_eq!(decoded.mono_label.value, "");
}
