use pojoc_runtime::{pojvec};
use pojoc_tests::pojoc_edge::*;

#[test]
fn test_empty_string_roundtrip() {
    let val = Edge {
        empty_str: "".into(),
        ..Default::default()
    };
    let mut buf = Vec::new();
    encode(&mut buf, &val);
    let decoded = decode(&buf).unwrap();
    assert_eq!(decoded.empty_str, "");
}

#[test]
fn test_empty_array_roundtrip() {
    let val = Edge {
        empty_arr: pojvec![],
        ..Default::default()
    };
    let mut buf = Vec::new();
    encode(&mut buf, &val);
    let decoded = decode(&buf).unwrap();
    assert!(decoded.empty_arr.is_empty());
}

#[test]
fn test_large_int_roundtrip() {
    let val = Edge {
        large_int: i64::MAX,
        ..Default::default()
    };
    let mut buf = Vec::new();
    encode(&mut buf, &val);
    let decoded = decode(&buf).unwrap();
    assert_eq!(decoded.large_int, i64::MAX);
}

#[test]
fn test_min_float_roundtrip() {
    let val = Edge {
        min_float: f32::MIN,
        ..Default::default()
    };
    let mut buf = Vec::new();
    encode(&mut buf, &val);
    let decoded = decode(&buf).unwrap();
    assert_eq!(decoded.min_float, f32::MIN);
}

#[test]
fn test_nan_float_roundtrip() {
    let val = Edge {
        nan_float: f32::NAN,
        ..Default::default()
    };
    let mut buf = Vec::new();
    encode(&mut buf, &val);
    let decoded = decode(&buf).unwrap();
    assert!(decoded.nan_float.is_nan());
}

#[test]
fn test_inf_float_roundtrip() {
    let val = Edge {
        inf_float: f32::INFINITY,
        ..Default::default()
    };
    let mut buf = Vec::new();
    encode(&mut buf, &val);
    let decoded = decode(&buf).unwrap();
    assert!(decoded.inf_float.is_infinite());
    assert!(decoded.inf_float.is_sign_positive());
}

#[test]
fn test_neg_inf_float_roundtrip() {
    let val = Edge {
        inf_float: f32::NEG_INFINITY,
        ..Default::default()
    };
    let mut buf = Vec::new();
    encode(&mut buf, &val);
    let decoded = decode(&buf).unwrap();
    assert!(decoded.inf_float.is_infinite());
    assert!(decoded.inf_float.is_sign_negative());
}