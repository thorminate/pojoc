pojoc_build::schema!(pub mod imported from "schemas/import_root.pojoc");

pub mod json {
    pojoc_build::schema!("schemas/stdio_bench.pojoc", serde);
}

#[test]
fn macro_codec_reads_historical_versions_and_imports() {
    use imported::{ImportRoot, leaf, runtime::LazyView};

    let child = leaf::ImportLeaf {
        id: 7,
        count: 5,
        label: "shared",
        blob: LazyView::Owned(vec![1, 2, 3].into()),
        tag: 12,
        sig: leaf::Sig::Num(3),
        extra: "new",
        note: "note",
    };
    let value = ImportRoot {
        rid: 11,
        child,
        name: "shared",
        seek_child: None,
        lazy_child: LazyView::Owned(None),
    };

    let mut bytes = Vec::new();
    imported::encode_for_version(&mut bytes, &value, 1).unwrap();
    let decoded = imported::decode(&bytes).unwrap();
    assert_eq!(decoded.child.id, 7);
    assert_eq!(decoded.child.note, "");
    assert_eq!(decoded.child.blob.get().unwrap().as_slice(), &[1, 2, 3]);
    assert!(
        pluck_import_root!(imported, &bytes, seek_child)
            .unwrap()
            .is_none()
    );

    let generated = pojoc_tests::pojoc_import_root::decode(&bytes).unwrap();
    assert_eq!(generated.child.id, decoded.child.id);
    assert_eq!(generated.child.label, decoded.child.label);
}

#[test]
fn schema_macro_can_emit_serde_into_the_current_module() {
    let value = json::StdioBench {
        id: 42,
        payload: vec![1, 2].into(),
        flags: 7,
    };
    let text = serde_json::to_string(&value).unwrap();
    let decoded: json::StdioBench = serde_json::from_str(&text).unwrap();
    assert_eq!(decoded.id, 42);
    assert_eq!(decoded.payload.as_slice(), &[1, 2]);
    assert_eq!(decoded.flags, 7);
}
