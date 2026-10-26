use letopis::domain::{SourceUri, ValueError};

type ByteSelectionCase<'a> = (&'a str, &'a [u8], Option<&'a [u8]>);

#[test]
fn normalize_local_source_identity() {
    // Given
    for (raw, expected, path, lines) in [
        (
            "./src/../src/a.rs#L010-L025",
            "src/a.rs#L10-L25",
            "src/a.rs",
            Some((10, 25)),
        ),
        ("./src//A%2fЯ.rs", "src/A%2fЯ.rs", "src/A%2fЯ.rs", None),
        ("a#L1-L1", "a#L1-L1", "a", Some((1, 1))),
        (
            "./docs/https:notes",
            "docs/https:notes",
            "docs/https:notes",
            None,
        ),
        ("http-notes", "http-notes", "http-notes", None),
    ] {
        // When
        let source = SourceUri::parse(raw).unwrap();
        let local = source.get_local().unwrap();
        // Then
        assert_eq!(source.as_str(), expected);
        let reparsed = SourceUri::parse(source.as_str()).unwrap();
        assert_eq!(reparsed.as_str(), expected);
        assert!(reparsed.get_local().is_some());
        assert_eq!(local.get_path().as_str(), path);
        assert_eq!(
            local
                .get_lines()
                .map(|range| (range.get_start(), range.get_end())),
            lines
        );
    }
    // When
    let whole = SourceUri::parse("a").unwrap();
    let range = SourceUri::parse("a#L1-L1").unwrap();
    // Then
    assert_ne!(whole, range);
}

#[test]
fn preserve_opaque_remote_identity() {
    // Given
    for raw in [
        "HTTP://Host/a?x=%2f#L01-L02",
        "hTtPs://Host/../a//Я?x=%2F#arbitrary",
        "https://host",
        "http://host?query#fragment",
    ] {
        // When
        let remote = SourceUri::parse(raw).unwrap();
        // Then
        assert_eq!(remote.as_str(), raw);
        assert!(remote.get_local().is_none());
        let reparsed = SourceUri::parse(remote.as_str()).unwrap();
        assert_eq!(reparsed.as_str(), raw);
        assert!(reparsed.get_local().is_none());
    }
    // When
    let upper = SourceUri::parse("https://Host/%2f").unwrap();
    let lower = SourceUri::parse("https://host/%2F").unwrap();
    // Then
    assert_ne!(upper, lower);
}

#[test]
fn reject_local_normalization_exposing_reserved_remote_prefix() {
    // Given: these are local spellings, not raw absolute remote URLs.
    for raw in [
        "./https:notes",
        "dir/../HTTP:notes",
        "./https://Host/a",
        "./hTtPs:notes#L01-L02",
        "dir/../http://Host/a#L1-L2",
        "./HTTP:notes#L1-L1",
    ] {
        // When
        let result = SourceUri::parse(raw);
        // Then
        assert_eq!(result, Err(ValueError::InvalidPath), "{raw:?}");
    }
}

#[test]
fn reject_malformed_source_addresses() {
    // Given
    for raw in [
        "https://",
        "https:/host/a",
        "HTTP:host",
        "http:bad://host",
        "http:///a",
        "http://?q",
        "https://#fragment",
        "https://host/a b",
        "https://host/a\\b",
        "https://host/\n",
        "https://host/\u{7f}",
        "https://host/\u{2003}",
        "../outside",
        "/absolute",
        "a/../../outside",
        "",
        ".",
        "a\0b",
        "a#L0-L1",
        "a#L2-L1",
        "a#L1-L18446744073709551616",
        "a#L+1-L2",
        "a#L1-Lx",
        "a#anchor",
        "a#L1-L2#L3-L4",
    ] {
        // When
        let result = SourceUri::parse(raw);
        // Then
        assert!(result.is_err(), "accepted {raw:?}");
    }
}

#[test]
fn select_exact_source_lines() {
    // Given: LF terminates a line, while CR and non-UTF-8 remain source bytes.
    let cases: &[ByteSelectionCase<'_>] = &[
        ("a", b"", Some(b"")),
        ("a", b"\xff\r\nx\n", Some(b"\xff\r\nx\n")),
        ("a#L1-L1", b"", None),
        ("a#L1-L1", b"\xff\r\n", Some(b"\xff\r\n")),
        ("a#L1-L1", b"\n", Some(b"\n")),
        ("a#L2-L2", b"x\n", None),
        ("a#L2-L2", b"x\ny", Some(b"y")),
        ("a#L2-L3", b"x\ny\r\nz", Some(b"y\r\nz")),
        ("a#L2-L3", b"x\ny\n", None),
        ("a#L1-L2", b"x", None),
        ("a#L18446744073709551615-L18446744073709551615", b"x", None),
    ];
    for (raw, bytes, expected) in cases {
        let source = SourceUri::parse(raw).unwrap();
        // When
        let actual = source.get_local().unwrap().select_bytes(bytes);
        // Then
        assert_eq!(actual, *expected, "{raw} over {bytes:?}");
    }
}
