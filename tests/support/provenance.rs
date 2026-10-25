use letopis::{
    domain::{RepoPath, SourceUri, TreeOid},
    open_git_snapshots,
    service::{GitSnapshots, SnapshotFile},
};
use std::{collections::BTreeSet, path::Path};
/// Create a raw tree from exactly the supplied captured source bytes.
///
/// # Panics
/// Panics if the fixture path or repository cannot produce the requested tree.
pub fn create_basis(root: &Path, files: &[(&str, &[u8])]) -> TreeOid {
    let git = open_git_snapshots(root).unwrap();
    let paths: Vec<_> = files
        .iter()
        .map(|(name, _)| RepoPath::parse(name).unwrap())
        .collect();
    let entries: Vec<_> = paths
        .iter()
        .zip(files)
        .map(|(path, (_, bytes))| SnapshotFile { path, bytes })
        .collect();
    git.create_tree(&entries).unwrap()
}
/// Parse explicit source addresses without computing provenance expectations.
///
/// # Panics
/// Panics if an address in the test fixture is invalid.
pub fn parse_sources(raw: &[&str]) -> BTreeSet<SourceUri> {
    raw.iter().map(|s| SourceUri::parse(s).unwrap()).collect()
}
/// Build an explicit Markdown fixture with an optional author-declared basis.
pub fn build_note(snapshot: Option<&TreeOid>, sources: &[&str], body: &str) -> String {
    let mut metadata = serde_json::json!({"created_at":3});
    if let Some(tree) = snapshot {
        metadata["source_snapshot"] = tree.as_str().into();
    }
    let mut note = format!("<!-- letopis: {metadata} -->\n{body}\n");
    for source in sources {
        note.push_str(&format!("[source]({source} \"letopis:type=external\")\n"));
    }
    note
}
