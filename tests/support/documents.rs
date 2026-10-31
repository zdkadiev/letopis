use letopis::{
    domain::{Edge, EdgeTarget, KnowledgeId, RepoPath, SourceUri},
    open_storage,
    service::{FileRow, NodeMetadata, Storage, StorageRead, StoredNode},
};
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

pub struct DocumentRepo(TempDir);
impl DocumentRepo {
    pub fn new() -> Self {
        let repo = Self(tempfile::tempdir().unwrap());
        repo.run_git(&["init", "-q"]);
        repo
    }
    pub fn get_root(&self) -> &Path {
        self.0.path()
    }
    pub fn write(&self, path: &str, bytes: impl AsRef<[u8]>) {
        fs::write(self.get_root().join(path), bytes).unwrap();
    }
    pub fn remove(&self, path: &str) {
        fs::remove_file(self.get_root().join(path)).unwrap();
    }
    pub fn run_git(&self, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(self.get_root())
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    pub fn get_state(&self) -> State {
        let mut storage = open_storage(
            &self.get_root().join(".letopis/cache.db"),
            &self.get_root().join(".letopis/vectors.db"),
        )
        .unwrap();
        let mut read = storage.begin_read().unwrap();
        State {
            revision: read.get_revision(),
            nodes: read.get_nodes().unwrap(),
            edges: read.get_edges_by_source_id(None).unwrap(),
            files: read.get_files().unwrap(),
        }
    }
    pub fn search(&self, text: &str) -> Vec<String> {
        let mut storage = open_storage(
            &self.get_root().join(".letopis/cache.db"),
            &self.get_root().join(".letopis/vectors.db"),
        )
        .unwrap();
        storage
            .begin_read()
            .unwrap()
            .match_text(text)
            .unwrap()
            .into_iter()
            .map(|row| row.id.as_str().to_owned())
            .collect()
    }
}
#[derive(Debug, PartialEq, Eq)]
pub struct State {
    pub revision: u64,
    pub nodes: Vec<StoredNode>,
    pub edges: Vec<Edge>,
    pub files: Vec<FileRow>,
}
impl State {
    pub fn get_node(&self, id: &str) -> &StoredNode {
        self.nodes
            .iter()
            .find(|row| match &row.metadata {
                NodeMetadata::Knowledge(k) => k.id.as_str() == id,
                NodeMetadata::Source(s) => s.uri.as_str() == id,
            })
            .unwrap()
    }
    pub fn get_file(&self, path: &str) -> &FileRow {
        self.files
            .iter()
            .find(|row| row.path == RepoPath::parse(path).unwrap())
            .unwrap()
    }
}
pub fn build_whole(created: u64, body: &str) -> String {
    format!("<!-- letopis: {{\"created_at\":{created}}} -->\n{body}\n")
}
pub fn build_external(created: u64, target: &str, body: &str) -> String {
    build_whole(
        created,
        &format!("{body} [source]({target} \"letopis:type=external;path={target}\")"),
    )
}
pub fn get_target(id: &str, source: bool) -> EdgeTarget {
    if source {
        EdgeTarget::Source(SourceUri::parse(id).unwrap())
    } else {
        EdgeTarget::Knowledge(KnowledgeId::parse(id).unwrap())
    }
}
