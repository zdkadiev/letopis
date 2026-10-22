#![allow(dead_code)]
pub mod documents;
use letopis::{
    domain::RepoPath,
    markdown::{Diagnostic, DiagnosticKind, ParsedFile, parse},
};
pub fn parse_file(path: &str, s: &str) -> ParsedFile {
    parse(&RepoPath::parse(path).unwrap(), s.as_bytes())
        .unwrap()
        .unwrap()
}
pub fn build_whole_file(body: &str) -> String {
    format!("<!-- letopis: {{\"created_at\":1}} -->\n{body}")
}
pub fn assert_error(result: Result<Option<ParsedFile>, Vec<Diagnostic>>, want: DiagnosticKind) {
    let errors = result.expect_err("invalid file must never yield a partial projection");
    assert!(
        errors.iter().any(|e| e.kind == want),
        "expected {want:?}, got {errors:?}"
    );
}
