use pojoc::*;

fn table_buf(entries: &[&str]) -> Vec<u8> {
    let mut buf = Vec::new();
    write_intern_table(&mut buf, entries);
    buf
}

#[test]
fn skip_advances_exactly_as_read() {
    for entries in [
        vec![],
        vec![""],
        vec!["a"],
        vec!["shared", "shared-tag", "", "unique"],
    ] {
        let buf = table_buf(&entries);

        let mut read_pos = 0;
        let table = read_intern_table(&buf, &mut read_pos).unwrap();
        assert_eq!(table, entries);

        let mut skip_pos = 0;
        skip_intern_table(&buf, &mut skip_pos).unwrap();

        assert_eq!(
            skip_pos, read_pos,
            "skip and read disagree on table {entries:?}"
        );
        assert_eq!(skip_pos, buf.len());
    }
}

#[test]
fn skip_rejects_a_truncated_table() {
    let buf = table_buf(&["alpha", "beta"]);
    for len in 0..buf.len() {
        let mut pos = 0;
        assert!(
            skip_intern_table(&buf[..len], &mut pos).is_err(),
            "truncating to {len} bytes should not skip cleanly"
        );
    }
}

#[test]
fn lookup_matches_the_materialized_table_at_every_index() {
    let entries = ["", "a", "shared-tag", "a longer entry with spaces"];
    let buf = table_buf(&entries);

    let mut pos = 0;
    let table = read_intern_table(&buf, &mut pos).unwrap();

    for (idx, expected) in table.iter().enumerate() {
        assert_eq!(read_interned_string_at(&buf, 0, idx).unwrap(), *expected);
    }
}

#[test]
fn lookup_past_the_end_is_an_invalid_index() {
    let buf = table_buf(&["only"]);
    assert!(matches!(
        read_interned_string_at(&buf, 0, 1),
        Err(Error::InvalidInternIndex)
    ));
    assert!(matches!(
        read_interned_string_at(&table_buf(&[]), 0, 0),
        Err(Error::InvalidInternIndex)
    ));
}

#[test]
fn lookup_honors_a_nonzero_table_start() {
    // the table does not begin at 0 in a real payload — an envelope precedes it
    let mut buf = vec![0xAA, 0xBB, 0xCC];
    let start = buf.len();
    buf.extend_from_slice(&table_buf(&["first", "second"]));

    assert_eq!(read_interned_string_at(&buf, start, 1).unwrap(), "second");
}
