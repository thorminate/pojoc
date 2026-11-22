//! An imported schema is a fully self-contained payload nested inside its
//! importer, so every feature that reads or writes a whole payload has to work
//! across that boundary the same way it does locally.

use pojoc_tests::pojoc_import_root::{runtime::*, *};

fn leaf(id: u32, sig: leaf::Sig<'static>, note: &'static str) -> leaf::ImportLeaf<'static> {
    leaf::ImportLeaf {
        id,
        count: 5,
        label: "shared",
        blob: LazyView::Owned(vec![1u8, 2, 3, 4].into()),
        tag: id * 10,
        sig,
        extra: "extra",
        note,
    }
}

fn populated() -> ImportRoot<'static> {
    ImportRoot {
        rid: 7,
        child: leaf(1, leaf::Sig::Text("hello"), "first"),
        name: "shared",
        seek_child: Some(leaf(2, leaf::Sig::Num(5), "second")),
        lazy_child: LazyView::Owned(Some(leaf(3, leaf::Sig::Text("deferred"), "third"))),
    }
}

fn encoded_at(version: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_for_version(&mut buf, &populated(), version).expect("encode failed");
    buf
}

#[test]
fn an_imported_payload_round_trips_through_every_holder() {
    let buf = encoded_at(2);
    let d = decode(&buf).unwrap();
    let want = populated();

    assert_eq!(d.rid, want.rid);
    assert_eq!(d.name, want.name);
    assert_eq!(d.child.id, want.child.id);
    assert_eq!(d.child.label, want.child.label);
    assert_eq!(d.child.note, want.child.note);
    assert!(matches!(d.child.sig, leaf::Sig::Text("hello")));
    assert_eq!(d.child.blob.get().unwrap().as_slice(), &[1, 2, 3, 4]);

    let seeked = d.seek_child.as_ref().unwrap();
    assert_eq!((seeked.id, seeked.note), (2, "second"));

    let lazy = d.lazy_child.get().unwrap();
    let lazy = lazy.as_ref().unwrap();
    assert_eq!((lazy.id, lazy.note, lazy.label), (3, "third", "shared"));
}

/// regression test: `skip_vN` did not step over the payload's own intern table,
/// so skipping an imported field left the cursor short by the table's length.
/// `lazy` is the shortest path to that bug — it captures the raw slice that the
/// skip delimits, so a mis-sized skip hands back bytes that will not decode.
#[test]
fn skipping_an_imported_payload_steps_over_its_intern_table() {
    let buf = encoded_at(2);
    let d = decode(&buf).unwrap();

    // the lazy payload sits behind the skip; a short one truncates the slice
    let lazy = d
        .lazy_child
        .get()
        .expect("lazy imported payload must decode");
    assert_eq!(lazy.as_ref().unwrap().id, 3);

    // and the skip has to leave the cursor where the *next* field starts, which
    // only the seek accessor exercises independently of decode
    let seeked = seek_seek_child(&buf).unwrap().unwrap();
    assert_eq!(
        (seeked.id, seeked.note, seeked.label),
        (2, "second", "shared")
    );
}

/// regression test: an imported schema's fallibility did not infect its
/// importer, so a constraint violation inside the nested payload was written as
/// truncated bytes and the error dropped on the floor.
#[test]
fn a_constraint_violated_inside_an_import_reaches_the_importers_caller() {
    let mut value = populated();
    value.child.count = 200;

    let mut buf = Vec::new();
    let err = encode(&mut buf, &value).expect_err("violation must not be swallowed");
    assert!(
        matches!(err, Error::ConstraintViolation { field, .. } if field == "count"),
        "unexpected error: {err:?}"
    );
}

/// the same infection through the two indirect holders, which reach the
/// imported writer down different codegen paths than a plain field does
#[test]
fn the_seekable_and_lazy_holders_propagate_the_violation_too() {
    for mutate in [
        (|v: &mut ImportRoot<'static>| v.seek_child.as_mut().unwrap().count = 0)
            as fn(&mut ImportRoot<'static>),
        |v: &mut ImportRoot<'static>| {
            v.lazy_child = LazyView::Owned(Some(leaf(3, leaf::Sig::Num(1), "x")));
            let LazyView::Owned(Some(inner)) = &mut v.lazy_child else {
                unreachable!()
            };
            inner.count = 0;
        },
    ] {
        let mut value = populated();
        mutate(&mut value);
        let mut buf = Vec::new();
        assert!(encode(&mut buf, &value).is_err());
    }
}

#[test]
fn an_older_version_predating_the_imported_holders_still_round_trips() {
    let buf = encoded_at(1);
    let d = decode(&buf).unwrap();

    assert_eq!(d.rid, 7);
    assert_eq!(d.child.id, 1);
    assert_eq!(d.child.label, "shared");
    assert!(matches!(d.child.sig, leaf::Sig::Text("hello")));
    // v1's Leaf@1 has neither field, so both take their defaults
    assert_eq!(d.child.note, "");
    assert_eq!(d.child.extra, "");
    assert!(d.seek_child.is_none());
    assert!(d.lazy_child.get().unwrap().is_none());
}

/// the nested payload carries its own table; resolving a root index against it
/// (or the reverse) would silently return the wrong string
#[test]
fn the_importers_intern_table_stays_distinct_from_the_nested_one() {
    let mut value = populated();
    value.name = "root-only";
    value.child.label = "leaf-only";
    value.child.extra = "root-only";

    let mut buf = Vec::new();
    encode(&mut buf, &value).unwrap();
    let d = decode(&buf).unwrap();

    assert_eq!(d.name, "root-only");
    assert_eq!(d.child.label, "leaf-only");
    assert_eq!(d.child.extra, "root-only");
}

#[test]
fn truncation_anywhere_errors_rather_than_panicking() {
    let buf = encoded_at(2);
    for cut in 0..buf.len() {
        let _ = decode(&buf[..cut]);
        let _ = seek_seek_child(&buf[..cut]);
    }
}
