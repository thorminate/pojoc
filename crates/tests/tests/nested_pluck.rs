use pojoc_tests::{pluck_nested_pluck as pluck, pojoc_nested_pluck as schema};
use schema::{NestedPluck, Point, Transform, runtime::PojocVec};

#[test]
fn nested_selectors_match_decoding_across_versions() {
    for prefix in [None, Some("nested optional prefix")] {
        let value = NestedPluck {
            marker: 7,
            preamble: Some("root optional prefix"),
            legacy_text: "legacy text",
            legacy_numbers: PojocVec::from_slice(&[21, 22, 23]),
            legacy_bytes: PojocVec::from_slice(&[42, 43, 44, 45]),
            transform: Transform {
                description: "variable prefix",
                position: Point {
                    prefix,
                    x: 0x1234_5678,
                    label: "shared label",
                    scratch: PojocVec::from_slice(&[0; 48]),
                },
                tail: PojocVec::from_slice(&[3; 48]),
                mode: 51,
            },
            trailer: PojocVec::from_slice(&[9; 48]),
        };
        for version in [1, 2, 3] {
            let mut data = Vec::new();
            schema::encode_for_version(&mut data, &value, version).unwrap();
            let decoded = schema::decode(&data).unwrap();
            let position = &decoded.transform.position;
            let (marker, transform) = pluck!(schema, &data, marker, transform).unwrap();
            assert_eq!(marker, decoded.marker);
            assert_eq!(transform.position.x, position.x);
            assert_eq!(
                pluck!(
                    schema,
                    &data,
                    transform.position.label,
                    marker,
                    transform.position.x,
                    transform.position.prefix
                )
                .unwrap(),
                (position.label, decoded.marker, position.x, position.prefix),
            );
            assert_eq!(
                pluck!(schema, &data, transform.mode).unwrap(),
                decoded.transform.mode
            );
            assert_eq!(
                pluck!(schema, &data, transform.description).unwrap(),
                decoded.transform.description
            );
            assert_eq!(
                pluck!(schema, &data, transform.position).unwrap().x,
                position.x
            );
            if version == 1 {
                // Corrupt a batched legacy array length ahead of the target.
                let mut corrupt = data.clone();
                let envelope = schema::runtime::read_envelope(&data, &mut 0).unwrap();
                let mut pos = 0;
                schema::runtime::skip_intern_table(envelope.payload, &mut pos).unwrap();
                schema::runtime::read_u8(envelope.payload, &mut pos).unwrap();
                schema::runtime::read_u32(envelope.payload, &mut pos).unwrap();
                schema::runtime::skip_string(envelope.payload, &mut pos).unwrap();
                schema::runtime::skip_string(envelope.payload, &mut pos).unwrap();
                corrupt[data.len() - envelope.payload.len() + pos] = 127;
                assert!(matches!(
                    pluck!(schema, &corrupt, marker, transform),
                    Err(schema::runtime::Error::UnexpectedEof)
                ));
            }

            let x = value.transform.position.x.to_le_bytes();
            let end = data.windows(x.len()).position(|bytes| bytes == x).unwrap() + x.len();
            data.truncate(end);
            schema::runtime::patch_envelope_length(&mut data, 1, end - 5);
            assert_eq!(
                pluck!(schema, &data, transform.position.x).unwrap(),
                value.transform.position.x
            );
            assert!(schema::decode(&data).is_err());

            data.pop();
            schema::runtime::patch_envelope_length(&mut data, 1, end - 6);
            assert!(pluck!(schema, &data, transform.position.x).is_err());
        }
    }
}
