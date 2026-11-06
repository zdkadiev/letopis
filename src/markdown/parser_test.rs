use super::{MarkdownParser, file_parse, navigation_path_decode};
use crate::{
    domain::RepoPath,
    service::{DiagnosticKind, MarkdownCodec},
};

#[test]
fn parser_interface_preserves_all_free_function_outcomes() {
    // Given: valid, unmarked and invalid byte sequences exercise every parser outcome.
    let path = RepoPath::parse("note.md").unwrap();
    let parser = MarkdownParser::new();
    let cases: &[&[u8]] = &[
        include_bytes!("../../tests/fixtures/markdown/whole.md"),
        include_bytes!("../../tests/fixtures/markdown/sections.md"),
        b"unmarked",
        b"\xff",
        b"<!-- letopis: {} -->\ntext",
    ];
    for bytes in cases {
        // When: parse each sequence through the trait and public free operation.
        let actual = parser.file_parse(&path, bytes);
        let existing = file_parse(&path, bytes);
        // Then: parity only; existing X-4 tests carry independent behavior expectations.
        assert_eq!(format!("{actual:?}"), format!("{existing:?}"));
    }
}

#[test]
fn rejects_literal_and_encoded_controls_and_backslashes() {
    // Given: dangerous navigation bytes need rejection even without a target mismatch.
    for raw in [
        "cache\\key.rs",
        "cache\u{0}.rs",
        "cache\u{7f}.rs",
        "cache\u{85}.rs",
        "cache%5ckey.rs",
        "cache%00.rs",
        "cache%7f.rs",
        "cache%C2%85.rs",
    ] {
        // When: decode navigation independently of the canonical target.
        let result = navigation_path_decode(raw);
        // Then: literal and decoded forbidden characters have the same safe reason.
        assert_eq!(result, Err(DiagnosticKind::LinkPathMismatch), "{raw:?}");
    }
}

#[test]
fn rejects_malformed_navigation_percent_escapes() {
    // Given: malformed escapes must fail independently of target comparison.
    for raw in ["%", "%A", "%GG", "%G0", "%0G", "cache%20key%.rs"] {
        // When: decode the navigation path directly.
        let result = navigation_path_decode(raw);
        // Then: preserving malformed percent bytes cannot masquerade as a path mismatch.
        assert_eq!(result, Err(DiagnosticKind::LinkPathMismatch), "{raw:?}");
    }
}

#[test]
fn rejects_malformed_navigation_utf8() {
    // Given: percent bytes encode invalid, truncated, overlong or surrogate UTF-8.
    for raw in ["%FF", "%80", "%E2%82", "%C0%AF", "%ED%A0%80"] {
        // When: decode the navigation path directly.
        let result = navigation_path_decode(raw);
        // Then: replacing invalid bytes with Unicode replacement characters must fail.
        assert_eq!(result, Err(DiagnosticKind::LinkPathMismatch), "{raw:?}");
    }
}
