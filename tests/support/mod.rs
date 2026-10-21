#![allow(dead_code)]
use letopis::{
    adapters::markdown::{DiagnosticKind, ParsedFile, parse},
    domain::RepoPath,
};
pub fn parsed(path: &str, s: &str) -> ParsedFile {
    parse(&RepoPath::parse(path).unwrap(), s.as_bytes())
        .unwrap()
        .unwrap()
}
pub fn whole(body: &str) -> String {
    format!("<!-- letopis: {{\"created_at\":1}} -->\n{body}")
}
pub fn error(input: &[u8], want: DiagnosticKind) {
    let result = parse(&RepoPath::parse("knowledge/a.md").unwrap(), input);
    let errors = result.expect_err("invalid file must never yield a partial projection");
    assert!(
        errors.iter().any(|e| e.kind == want),
        "expected {want:?}, got {errors:?}"
    );
}
