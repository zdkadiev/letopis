mod support;
use letopis::{
    domain::RepoPath,
    markdown::{self, DiagnosticKind},
};
use support::{assert_error, build_whole_file, parse_file};

#[test]
fn uses_path_as_the_only_target() {
    // Given: navigation and graph fragments deliberately differ.
    let text = build_whole_file(
        "[code](../../src/cache.rs#anything \"letopis:type=external;path=src/cache.rs#L2-L4\")",
    );
    // When: parse without reading a target file.
    let parsed = parse_file("letopis/cache/note.md", &text);
    // Then: the terminal title address supplies the exact source range.
    assert_eq!(
        parsed.edges.first().unwrap().get_target().as_str(),
        "src/cache.rs#L2-L4"
    );
}

#[test]
fn compares_paths_from_the_containing_section_file() {
    // Given: the source section's anchor is not part of its parent directory.
    let text = "## Owner {#owner}\n<!-- letopis: {\"created_at\":1} -->\n[x](other.md#viewer \"letopis:type=related_to;path=letopis/cache/other.md#expiry\")";
    // When: parse a section in a nested Markdown file.
    let parsed = parse_file("letopis/cache/note.md", text);
    // Then: the nonexistent graph section and independent viewer fragment pass.
    let edge = parsed.edges.first().unwrap();
    assert_eq!(edge.get_source().as_str(), "letopis/cache/note.md#owner");
    assert_eq!(edge.get_target().as_str(), "letopis/cache/other.md#expiry");
}

#[test]
fn ignores_navigation_fragments_and_target_existence() {
    // Given: empty navigation paths refer to the containing section's file.
    for destination in ["#different", "", "#L0-invalid", "#%GG"] {
        let text = format!(
            "## Owner {{#owner}}\n<!-- letopis: {{\"created_at\":1}} -->\n[x](<{destination}> \"letopis:type=related_to;path=letopis/cache/note.md#other\")"
        );
        // When: no existence or fragment resolution is requested.
        let parsed = parse_file("letopis/cache/note.md", &text);
        // Then: only the title anchor identifies the target.
        assert_eq!(
            parsed.edges.first().unwrap().get_target().as_str(),
            "letopis/cache/note.md#other"
        );
    }
}

#[test]
fn rejects_legacy_titles() {
    // Given: valid destinations cannot repair malformed mandatory metadata.
    let path = RepoPath::parse("a.md").unwrap();
    for title in [
        "letopis:type=related_to",
        "letopis:type=related_to;path=",
        "letopis:type=Related;path=b.md",
        "letopis:path=b.md;type=includes",
        "letopis:type=includes;type=includes;path=b.md",
        "letopis:type=includes;target=b.md",
        "letopis:type=includes ;path=b.md",
    ] {
        let text = build_whole_file(&format!("[x](b.md \"{title}\")"));
        // When: parse a recognized Letopis declaration.
        let result = markdown::file_parse(&path, text.as_bytes());
        // Then: malformed titles invalidate the whole file.
        assert_error(result, DiagnosticKind::InvalidLinkTitle);
    }
}

#[test]
fn preserves_terminal_address_bytes() {
    // Given: everything after the first path delimiter is opaque address content.
    for (destination, target) in [
        ("unrelated.md", "https://Host/a%20b?q=x;y;path=z#part"),
        ("../../src/a;path=z.rs", "src/a;path=z.rs"),
        ("../../src/%20cache.rs", "src/ cache.rs"),
        ("../../src/cache.rs%20", "src/cache.rs "),
    ] {
        let text = build_whole_file(&format!(
            "[x]({destination} \"letopis:type=external;path={target}\")"
        ));
        // When: parse a remote URL or literal local filename.
        let parsed = parse_file("letopis/cache/note.md", &text);
        // Then: percent sequences, semicolons, case and whitespace are preserved.
        assert_eq!(parsed.edges.first().unwrap().get_target().as_str(), target);
    }
}

#[test]
fn decodes_only_navigation_paths_once() {
    // Given: navigation encoding is independent of literal metadata.
    for (destination, target) in [
        ("../../src/cache%20key.rs", "src/cache key.rs"),
        ("../../src/cache%2520key.rs", "src/cache%20key.rs"),
        ("../../src/%D0%BA%D1%8D%D1%88.rs", "src/кэш.rs"),
        ("../../src/кэш.rs", "src/кэш.rs"),
        ("../../src/cache%3Fkey.rs", "src/cache?key.rs"),
        ("./cache:key.rs", "letopis/cache/cache:key.rs"),
        ("../../src//./cache.rs", "src/cache.rs"),
    ] {
        let text = build_whole_file(&format!(
            "[x]({destination} \"letopis:type=external;path={target}\")"
        ));
        // When: decode the navigation filename once.
        let parsed = parse_file("letopis/cache/note.md", &text);
        // Then: the canonical target retains its literal bytes.
        assert_eq!(parsed.edges.first().unwrap().get_target().as_str(), target);
    }
}

#[test]
fn rejects_mismatched_and_unsupported_navigation_paths() {
    // Given: valid metadata is checked independently of unsafe navigation forms.
    let path = RepoPath::parse("letopis/cache/note.md").unwrap();
    for destination in [
        "../../src/other.rs",
        "../../../escape.rs",
        "%",
        "%GG",
        "%A",
        "%FF",
        "%2Fcache.rs",
        "%5Ccache.rs",
        "%00cache.rs",
        "%7Fcache.rs",
        "%C2%85cache.rs",
        "/absolute",
        "//host",
        "https://host/file",
        "cache.rs?q=x",
        "cache:key.rs",
        "../../src",
        "../..",
    ] {
        let text = build_whole_file(&format!(
            "[x]({destination} \"letopis:type=external;path=src/cache.rs\")"
        ));
        // When: resolve local navigation relative to the containing file.
        let result = markdown::file_parse(&path, text.as_bytes());
        // Then: failures use a safe, stable reason rather than exposing either path.
        let errors = result.unwrap_err();
        assert_eq!(
            DiagnosticKind::LinkPathMismatch.get_code(),
            "link_path_mismatch"
        );
        assert_eq!(
            errors[0].kind,
            DiagnosticKind::LinkPathMismatch,
            "{destination}"
        );
        assert_eq!(
            errors[0].message,
            "Markdown link path does not match letopis path"
        );
        let span = errors[0].bytes.unwrap();
        assert_eq!(
            &text[span.get_start()..span.get_end()],
            text.lines().last().unwrap()
        );
    }
}

#[test]
fn rejects_bad_metadata_before_comparing_navigation() {
    // Given: invalid canonical addresses and unrelated navigation coexist.
    let path = RepoPath::parse("letopis/cache/note.md").unwrap();
    for (target, kind) in [
        ("../escape.rs", "external"),
        ("src/cache.rs#L0-L1", "external"),
        ("a.md#L1-L2", "related_to"),
    ] {
        let text = build_whole_file(&format!(
            "[x](unrelated.md \"letopis:type={kind};path={target}\")"
        ));
        // When: canonical target parsing precedes comparison.
        let result = markdown::file_parse(&path, text.as_bytes());
        // Then: target grammar retains its own diagnostic.
        assert_error(result, DiagnosticKind::InvalidTarget);
    }
}

#[test]
fn rejects_encoded_separators_even_when_decoding_would_match() {
    // Given: forbidden encoded separators could otherwise yield the target file.
    let path = RepoPath::parse("letopis/cache/note.md").unwrap();
    for (destination, target) in [
        ("../../src%2Fcache.rs", "src/cache.rs"),
        ("../../src/cache%5Ckey.rs", "src/cache\\key.rs"),
        ("../../src/cache?key.rs", "src/cache?key.rs"),
        ("cache:key.rs", "letopis/cache/cache:key.rs"),
    ] {
        let text = build_whole_file(&format!(
            "[x]({destination} \"letopis:type=external;path={target}\")"
        ));
        // When: comparison rejects unsupported navigation syntax before matching.
        let result = markdown::file_parse(&path, text.as_bytes());
        // Then: the exact canonical target cannot authorize ambiguous navigation.
        assert_error(result, DiagnosticKind::LinkPathMismatch);
    }
}

#[test]
fn uses_metadata_ranges_even_when_navigation_is_a_canonical_address() {
    // Given: both source addresses are valid but their line ranges differ.
    let text = build_whole_file(
        "[code](src/cache.rs#L9-L10 \"letopis:type=external;path=src/cache.rs#L2-L4\")",
    );
    // When: parse a root-level note without source reads.
    let parsed = parse_file("note.md", &text);
    // Then: using the destination would silently select the wrong source bytes.
    assert_eq!(
        parsed.edges.first().unwrap().get_target().as_str(),
        "src/cache.rs#L2-L4"
    );
}
