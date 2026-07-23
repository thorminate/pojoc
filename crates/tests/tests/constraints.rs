use pojoc_tests::pojoc_constraints::runtime::Error;
use pojoc_tests::pojoc_constraints::*;

#[test]
fn test_valid_values_roundtrip() {
    let original = Constraints {
        count: 7,
        tags: pojoc::pojvec!("a", "b"),
        label: "hello",
    };

    let mut buf = Vec::new();
    encode(&mut buf, &original).expect("valid value must encode");
    let decoded: Constraints = decode(&buf).expect("valid value must decode");

    assert_eq!(decoded.count, 7);
    assert_eq!(decoded.tags.len(), 2);
    assert_eq!(decoded.tags[0], "a");
    assert_eq!(decoded.tags[1], "b");
    assert_eq!(decoded.label, "hello");
}

#[test]
fn test_boundary_values_roundtrip() {
    let original = Constraints {
        count: 10,
        tags: pojoc::pojvec!("a", "b", "c"),
        label: "0123456789",
    };

    let mut buf = Vec::new();
    encode(&mut buf, &original).expect("boundary value must encode");
    let decoded: Constraints = decode(&buf).expect("boundary value must decode");
    assert_eq!(decoded.count, 10);
    assert_eq!(decoded.tags.len(), 3);
    assert_eq!(decoded.label, "0123456789");
}

#[test]
fn test_encode_rejects_out_of_range_count() {
    let original = Constraints {
        count: 11,
        tags: Default::default(),
        label: "ok",
    };
    let mut buf = Vec::new();
    let err = encode(&mut buf, &original).expect_err("out-of-range count must be rejected");
    assert!(matches!(
        err,
        Error::ConstraintViolation { field: "count", .. }
    ));
}

#[test]
fn test_encode_rejects_too_many_tags() {
    let original = Constraints {
        count: 1,
        tags: pojoc::pojvec!("a", "b", "c", "d"),
        label: "ok",
    };
    let mut buf = Vec::new();
    let err = encode(&mut buf, &original).expect_err("too many tags must be rejected");
    assert!(matches!(
        err,
        Error::ConstraintViolation { field: "tags", .. }
    ));
}

#[test]
fn test_encode_rejects_empty_label() {
    let original = Constraints {
        count: 1,
        tags: Default::default(),
        label: "",
    };
    let mut buf = Vec::new();
    let err = encode(&mut buf, &original).expect_err("empty label must be rejected");
    assert!(matches!(
        err,
        Error::ConstraintViolation { field: "label", .. }
    ));
}

// finds count's byte offset by diffing two encodings that differ only in count, then patches it out of range
#[test]
fn test_decode_rejects_out_of_range_count() {
    let mut buf_a = Vec::new();
    encode(
        &mut buf_a,
        &Constraints {
            count: 1,
            tags: Default::default(),
            label: "ok",
        },
    )
    .unwrap();

    let mut buf_b = Vec::new();
    encode(
        &mut buf_b,
        &Constraints {
            count: 2,
            tags: Default::default(),
            label: "ok",
        },
    )
    .unwrap();

    assert_eq!(
        buf_a.len(),
        buf_b.len(),
        "same-shape messages must be same length"
    );
    let diff_positions: Vec<usize> = (0..buf_a.len()).filter(|&i| buf_a[i] != buf_b[i]).collect();
    assert_eq!(
        diff_positions.len(),
        1,
        "count should be the only differing byte between these two messages"
    );
    let count_offset = diff_positions[0];

    let mut malformed = buf_a.clone();
    malformed[count_offset] = 99;
    let err = decode(&malformed).expect_err("malformed count must be rejected, not panic");
    assert!(matches!(
        err,
        Error::ConstraintViolation { field: "count", .. }
    ));
}
