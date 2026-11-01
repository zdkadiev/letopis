use crate::support::{
    git::{OwnedChild, isolate_command},
    reading::ReadingRepo,
};
use letopis::{
    domain::{EdgeTarget, TreeOid},
    service::{KnowledgeMetadata, NodeMetadata, SourceMetadata, Storage, StorageRead},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

const CACHE_TEMPLATE: &str = include_str!("../fixtures/markdown/thin_slice/cache.md.in");
const FLOW_TEMPLATE: &str = include_str!("../fixtures/markdown/thin_slice/flow.md.in");
const SOURCE: &[u8] = include_bytes!("../fixtures/markdown/thin_slice/cache.rs");
const SEARCH: &[u8] = br#"{"query":"CacheScope","limit":10}"#;
const USER_FILES: [&str; 6] = [
    ".git/HEAD",
    ".git/index",
    ".git/config",
    "src/cache.rs",
    "knowledge/cache.md",
    "knowledge/flow.md",
];

/// Disposable inputs whose projection can only be loaded by the product CLI.
pub struct SliceFixture {
    repo: ReadingRepo,
    source_tree: TreeOid,
}
/// An owned observation; no read transaction survives a CLI or HTTP boundary.
#[derive(Debug, PartialEq, Eq)]
pub struct ProjectionSnapshot {
    pub revision: u64,
    pub knowledge: BTreeMap<String, KnowledgeMetadata>,
    pub sources: BTreeMap<String, SourceMetadata>,
    pub edges: BTreeSet<(String, String, String)>,
}
/// User-owned bytes and refs, excluding the permitted retained snapshot namespace.
#[derive(Debug, PartialEq, Eq)]
pub struct UserState {
    files: BTreeMap<String, Vec<u8>>,
    refs: Vec<u8>,
}
impl SliceFixture {
    /// Commit a source basis and materialize knowledge without indexing it.
    /// # Panics
    /// Panics on fixture I/O or Git failure.
    pub fn new() -> Self {
        let repo = ReadingRepo::new();
        repo.write(".gitignore", b".letopis/\n");
        repo.write("src/cache.rs", SOURCE);
        run_git(repo.get_root(), &["add", ".gitignore", "src/cache.rs"]);
        run_git(repo.get_root(), &["commit", "-qm", "source basis"]);
        let oid = run_git(repo.get_root(), &["rev-parse", "HEAD^{tree}"]);
        let source_tree = TreeOid::parse(std::str::from_utf8(&oid).unwrap().trim()).unwrap();
        repo.write(
            "knowledge/cache.md",
            CACHE_TEMPLATE
                .replace("__SOURCE_TREE__", source_tree.as_str())
                .as_bytes(),
        );
        repo.write(
            "knowledge/flow.md",
            FLOW_TEMPLATE
                .replace("__SOURCE_TREE__", source_tree.as_str())
                .as_bytes(),
        );
        Self { repo, source_tree }
    }
    pub fn get_repo(&self) -> &ReadingRepo {
        &self.repo
    }
    pub fn get_source_tree(&self) -> &TreeOid {
        &self.source_tree
    }
    /// Build projections through a fresh product process without a library seed.
    /// # Panics
    /// Panics on process failure or invalid output.
    pub fn load(&self) -> Value {
        get_response(&self.repo.invoke(&["search"], SEARCH), 0)
    }
    /// Read an owned projection observation and release both storage/read handles.
    /// # Panics
    /// Panics on storage failures.
    pub fn get_projection(&self) -> ProjectionSnapshot {
        let mut storage = self.repo.open_storage();
        let mut read = storage.begin_read_tx().unwrap();
        let mut result = ProjectionSnapshot {
            revision: read.get_revision(),
            knowledge: BTreeMap::new(),
            sources: BTreeMap::new(),
            edges: BTreeSet::new(),
        };
        for node in read.get_nodes().unwrap() {
            match node.metadata {
                NodeMetadata::Knowledge(k) => {
                    result.knowledge.insert(k.id.as_str().to_owned(), k);
                }
                NodeMetadata::Source(s) => {
                    result.sources.insert(s.uri.as_str().to_owned(), s);
                }
            }
        }
        result.edges = read
            .get_edges_by_source_id(None)
            .unwrap()
            .iter()
            .map(|edge| {
                let target = match edge.get_target() {
                    EdgeTarget::Knowledge(id) => id.as_str(),
                    EdgeTarget::Source(id) => id.as_str(),
                };
                (
                    edge.get_source().as_str().to_owned(),
                    target.to_owned(),
                    edge.get_edge_type().as_str().to_owned(),
                )
            })
            .collect();
        result
    }
    /// Capture original user bytes and refs for a read-only product assertion.
    /// # Panics
    /// Panics on fixture I/O/Git failure.
    pub fn capture_user_state(&self) -> UserState {
        let files = USER_FILES
            .into_iter()
            .map(|path| (path.to_owned(), self.get_bytes(path)))
            .collect();
        let refs = run_git(
            self.repo.get_root(),
            &["for-each-ref", "--format=%(refname) %(objectname)"],
        );
        let refs = refs
            .split_inclusive(|b| *b == b'\n')
            .filter(|line| !line.starts_with(b"refs/letopis/snapshots/"))
            .flatten()
            .copied()
            .collect();
        UserState { files, refs }
    }
    /// Read exact live bytes for an independent snapshot oracle.
    /// # Panics
    /// Panics if fixture input is missing.
    pub fn get_bytes(&self, path: &str) -> Vec<u8> {
        fs::read(self.repo.get_root().join(path)).unwrap()
    }
    /// Read immutable Git bytes independently of the Letopis snapshot implementation.
    /// # Panics
    /// Panics when the object does not contain the requested path.
    pub fn get_tree_file(&self, tree: &str, path: &str) -> Vec<u8> {
        run_git(self.repo.get_root(), &["show", &format!("{tree}:{path}")])
    }
}
impl fmt::Debug for SliceFixture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliceFixture")
            .field("root", &self.repo.get_root())
            .field("source_tree", &self.source_tree)
            .finish()
    }
}
/// Emit a schema-validatable actual product observation after scenario assertions.
pub fn emit_slice(case: &str, command: &str, exit: i32, response: &Value) {
    println!(
        "THIN_SLICE_RESPONSE {}",
        json!({"case":case,"command":command,"exit":exit,"response":response})
    );
}
/// Check transport framing and parse the single product JSON response.
/// # Panics
/// Panics on the wrong exit, extra output, or nonempty stderr.
pub fn get_response(output: &Output, exit: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        output.stdout.iter().filter(|b| **b == b'\n').count(),
        1,
        "{output:?}"
    );
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    serde_json::from_slice(&output.stdout).unwrap()
}
fn run_git(root: &Path, args: &[&str]) -> Vec<u8> {
    let mut command = Command::new("git");
    isolate_command(&mut command);
    command
        .current_dir(root)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = OwnedChild::spawn(&mut command).wait(Instant::now() + Duration::from_secs(20));
    assert!(output.status.success(), "{output:?}");
    output.stdout
}
