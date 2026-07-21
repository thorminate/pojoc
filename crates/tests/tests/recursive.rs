use pojoc_tests::pojoc_recursive::*;

fn decode_static(buf: &[u8]) -> Recursive<'static> {
    decode(Vec::leak(buf.to_vec())).expect("decode failed")
}

#[test]
fn test_linked_list_roundtrip() {
    let original = Recursive {
        head: Some(Node {
            value: 1,
            next: Some(Box::new(Node {
                value: 2,
                next: Some(Box::new(Node {
                    value: 3,
                    next: None,
                    label: None,
                })),
                label: Some(Box::new(Label { text: "middle" })),
            })),
            label: Some(Box::new(Label { text: "head" })),
        }),
        ..Default::default()
    };

    let mut buf = Vec::new();
    encode(&mut buf, &original);
    let decoded = decode_static(&buf);

    let h = decoded.head.expect("head");
    assert_eq!(h.value, 1);
    assert_eq!(h.label.as_deref().map(|l| l.text), Some("head"));
    let n2 = h.next.expect("second node");
    assert_eq!(n2.value, 2);
    assert_eq!(n2.label.as_deref().map(|l| l.text), Some("middle"));
    let n3 = n2.next.expect("third node");
    assert_eq!(n3.value, 3);
    assert!(n3.next.is_none());
    assert!(n3.label.is_none());
}

#[test]
fn test_tree_roundtrip() {
    let original = Recursive {
        root: Some(TreeNode {
            value: 10,
            left: Some(Box::new(TreeNode {
                value: 5,
                left: None,
                right: None,
            })),
            right: Some(Box::new(TreeNode {
                value: 15,
                left: None,
                right: None,
            })),
        }),
        ..Default::default()
    };

    let mut buf = Vec::new();
    encode(&mut buf, &original);
    let decoded = decode_static(&buf);

    let root = decoded.root.expect("root");
    assert_eq!(root.value, 10);
    assert_eq!(root.left.as_deref().map(|n| n.value), Some(5));
    assert_eq!(root.right.as_deref().map(|n| n.value), Some(15));
}

#[test]
fn test_mutual_recursion_roundtrip() {
    let original = Recursive {
        ping: Some(Ping {
            count: 1,
            other: Some(Box::new(Pong {
                count: 2,
                other: Some(Box::new(Ping {
                    count: 3,
                    other: None,
                })),
            })),
        }),
        ..Default::default()
    };

    let mut buf = Vec::new();
    encode(&mut buf, &original);
    let decoded = decode_static(&buf);

    let ping = decoded.ping.expect("ping");
    assert_eq!(ping.count, 1);
    let pong = ping.other.expect("pong");
    assert_eq!(pong.count, 2);
    let ping2 = pong.other.expect("ping2");
    assert_eq!(ping2.count, 3);
    assert!(ping2.other.is_none());
}

#[test]
fn test_plain_ref_box_retype_across_versions() {
    let original = Recursive {
        plain_ref: Box::new(Node {
            value: 42,
            next: None,
            label: None,
        }),
        ..Default::default()
    };

    // Encode as the (retyped) latest version and every historical version;
    // v1 had `plain_ref: Node`, v2 retypes it to `plain_ref: box<Node>` —
    // this exercises the box/unbox FieldMapping::Cast path both ways.
    for &version in supported_versions() {
        let mut buf = Vec::new();
        encode_for_version(&mut buf, &original, version)
            .unwrap_or_else(|e| panic!("v{version}: encode_for_version failed: {e:?}"));
        let decoded: Recursive =
            decode(&buf).unwrap_or_else(|e| panic!("v{version}: decode failed: {e:?}"));
        assert_eq!(decoded.plain_ref.value, 42, "v{version}");
    }
}
